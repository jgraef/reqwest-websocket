use std::{
    pin::Pin,
    task::{Context, Poll},
};

use futures_channel::{mpsc, oneshot};
use futures_util::{ready, select_biased, FutureExt, Sink, SinkExt, Stream, StreamExt};
use reqwest::{Request, Url};
use tracing::Instrument;
use web_sys::{
    js_sys::{Array, ArrayBuffer, JsString, Uint8Array},
    wasm_bindgen::{closure::Closure, JsCast, JsValue},
    CloseEvent, ErrorEvent, Event, MessageEvent,
};

use crate::protocol::Message;

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    #[error("Invalid URL: {0}")]
    InvalidUrl(Url),

    #[error("Connection failed")]
    ConnectionFailed,

    #[error("{0}")]
    ErrorEvent(String),

    #[error("Unknown error")]
    Unknown,

    #[error("Send error")]
    SendError,
}

impl From<ErrorEvent> for Error {
    fn from(event: ErrorEvent) -> Self {
        Self::ErrorEvent(event.message())
    }
}

impl From<JsValue> for Error {
    fn from(_value: JsValue) -> Self {
        Self::Unknown
    }
}

struct Outgoing {
    message: Message,
    ack_tx: oneshot::Sender<Result<(), Error>>,
}

#[derive(Debug)]
pub struct WebSocket {
    outgoing_tx: mpsc::Sender<Outgoing>,
    incoming_rx: mpsc::UnboundedReceiver<Result<Message, Error>>,
    ack_rx: Option<oneshot::Receiver<Result<(), Error>>>,
    protocol: String,
}

impl WebSocket {
    pub async fn new(request: Request, protocols: &Vec<String>) -> Result<Self, Error> {
        // get websocket URL from request.
        // this contains query parameters. everything else is ignored, as web_sys only accepts an URL.
        let mut url = request.url().clone();
        let scheme = match url.scheme() {
            "http" | "ws" => "ws",
            "https" | "wss" => "wss",
            _ => return Err(Error::InvalidUrl(url)),
        };
        if let Err(_) = url.set_scheme(scheme) {
            return Err(Error::InvalidUrl(url));
        }

        // create the websocket
        let websocket = web_sys::WebSocket::new_with_str_sequence(
            &url.to_string(),
            &protocols
                .into_iter()
                .map(|s| JsString::from(s.to_owned()))
                .collect::<Array>(),
        )
        .map_err(|_| Error::ConnectionFailed)?;
        websocket.set_binary_type(web_sys::BinaryType::Arraybuffer);

        // outgoing channel. only needs a capacity of 1, as we wait for acks anyway
        let (outgoing_tx, outgoing_rx) = mpsc::channel(1);

        // note: this needs to be unbounded, because we can't block in the event handlers
        let (incoming_tx, incoming_rx) = mpsc::unbounded();

        // channel for connect acks. message type: `Result<String, Error>`, where `String` is the protocol reported by the websocket
        let (connect_ack_tx, connect_ack_rx) = oneshot::channel();

        // spawn a task for the websocket locally. this way our `WebSocket` struct is `Send + Sync`, while the code that has the
        // `web_sys::Websocket` (which is not `Send + Sync`) stays on the same thread.
        tracing::debug!("spawning websocket task");
        let task_span = tracing::info_span!("websocket");
        wasm_bindgen_futures::spawn_local(
            run_websocket(websocket, connect_ack_tx, outgoing_rx, incoming_tx).instrument(task_span),
        );

        // wait for connection ack, or error
        tracing::debug!("waiting for ack");
        let protocol = connect_ack_rx
            .await
            .expect("websocket handler dropped ack sender")?;
        tracing::debug!("ack received");

        Ok(Self {
            outgoing_tx,
            incoming_rx,
            ack_rx: None,
            protocol,
        })
    }

    fn poll_ack(&mut self, cx: &mut Context) -> Poll<Result<(), Error>> {
        if let Some(ack_rx) = &mut self.ack_rx {
            let result = ready!(ack_rx.poll_unpin(cx)).unwrap_or(Ok(()));
            self.ack_rx = None;
            Poll::Ready(result)
        } else {
            Poll::Ready(Ok(()))
        }
    }

    pub fn protocol(&self) -> &str {
        &self.protocol
    }
}

impl Stream for WebSocket {
    type Item = Result<Message, Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.incoming_rx
            .poll_next_unpin(cx)
            .map_ok(|message| {
                tracing::debug!("message: {message:?}");
                message
            })
            .map_err(|e| {
                tracing::error!("receive error: {e}");
                e
            })
    }
}

impl Sink<Message> for WebSocket {
    type Error = Error;

    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        ready!(self.outgoing_tx.poll_ready(cx)).map_err(|_| Error::SendError)?;
        self.poll_ack(cx)
    }

    fn start_send(mut self: Pin<&mut Self>, message: Message) -> Result<(), Self::Error> {
        let (ack_tx, ack_rx) = oneshot::channel();
        self.ack_rx = Some(ack_rx);
        self.outgoing_tx
            .start_send(Outgoing { message, ack_tx })
            .map_err(|_| Error::SendError)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        ready!(self.outgoing_tx.poll_flush_unpin(cx)).map_err(|_| Error::SendError)?;
        self.poll_ack(cx)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        ready!(self.outgoing_tx.poll_close_unpin(cx)).map_err(|_| Error::SendError)?;
        self.poll_ack(cx)
    }
}

async fn run_websocket(
    websocket: web_sys::WebSocket,
    connect_ack_tx: oneshot::Sender<Result<String, Error>>,
    mut outgoing_rx: mpsc::Receiver<Outgoing>,
    incoming_tx: mpsc::UnboundedSender<Result<Message, Error>>,
) {
    let (mut error_tx, mut error_rx) = mpsc::unbounded();
    let (close_tx, mut close_rx) = oneshot::channel();
    let (open_tx, mut open_rx) = oneshot::channel();

    // register error handler
    // this will try to put the first error into a oneshot channel for errors that
    // happen during opening. once that has been used, or the oneshot
    // channel is dropped, this uses the regular message channel
    let on_error_callback = {
        tracing::debug!("error event");
        Closure::<dyn FnMut(Event)>::new(move |event: Event| {
            let error = match event.dyn_into::<ErrorEvent>() {
                Ok(error) => Error::from(error),
                Err(_event) => Error::Unknown,
            };
            let _ = error_tx.send(error);
        })
    };
    websocket.set_onerror(Some(on_error_callback.as_ref().unchecked_ref()));

    // register close handler
    let on_close_callback = {
        let mut close_tx = Some(close_tx);
        let incoming_tx = incoming_tx.clone();

        Closure::<dyn FnMut(CloseEvent)>::new(move |event: CloseEvent| {
            tracing::debug!("close event");
            if let Some(close_tx) = close_tx.take() {
                let _ = incoming_tx.unbounded_send(Ok(Message::Close {
                    code: event.code().into(),
                    reason: event.reason(),
                }));
                let _ = close_tx.send(());
            }
        })
    };
    websocket.set_onclose(Some(on_close_callback.as_ref().unchecked_ref()));

    // register open handler
    let on_open_callback = {
        let mut open_tx = Some(open_tx);

        Closure::<dyn FnMut(Event)>::new(move |_event: Event| {
            tracing::debug!("open event");
            if let Some(open_tx) = open_tx.take() {
                let _ = open_tx.send(());
            }
        })
    };
    websocket.set_onopen(Some(on_open_callback.as_ref().unchecked_ref()));

    // register message handler
    let on_message_callback = {
        let incoming_tx = incoming_tx.clone();

        Closure::<dyn FnMut(MessageEvent)>::new(move |event: MessageEvent| {
            tracing::debug!("message event");
            if let Ok(abuf) = event.data().dyn_into::<ArrayBuffer>() {
                let array = Uint8Array::new(&abuf);
                let data = array.to_vec();
                let _ = incoming_tx.unbounded_send(Ok(Message::Binary(data)));
            } else if let Ok(text) = event.data().dyn_into::<JsString>() {
                let _ = incoming_tx.unbounded_send(Ok(Message::Text(text.into())));
            } else {
                tracing::debug!(event = ?event.data(), "received unknown message event");
            }
        })
    };
    websocket.set_onmessage(Some(on_message_callback.as_ref().unchecked_ref()));

    // first wait for open/close/error and send connect ack
    let mut run_socket = false;
    select_biased! {
        _ = open_rx => {
            let _ = connect_ack_tx.send(Ok(websocket.protocol()));
            run_socket = true;
        }
        _ = &mut close_rx => {
            let _ = connect_ack_tx.send(Err(Error::ConnectionFailed));
        }
        error_opt = error_rx.next() => {
            if let Some(error) = error_opt {
                let _ = connect_ack_tx.send(Err(error));
            }
        }
    }

    // we can remove the open handler
    websocket.set_onopen(None);

    // connection established. listen for close/error events and outgoing messages
    while run_socket {
        select_biased! {
            _ = &mut close_rx => {
                // close event received
                // the event handler takes care of sending the close frame into incoming_tx
                run_socket = false;
            }
            error_opt = error_rx.next() => {
                // error event received
                if let Some(error) = error_opt {
                    if incoming_tx.unbounded_send(Err(error)).is_err() {
                        // receiver half dropped
                        run_socket = false;
                    }
                }
            }
            message_opt = outgoing_rx.next() => {
                if let Some(Outgoing { message, ack_tx }) = message_opt {
                    let result = send_message(&websocket, message);
                    let _ = ack_tx.send(result);
                }
                else {
                    // sender half dropped
                    run_socket = false;
                }
            }
        }
    }

    // cleanup
    let _ = websocket.close();
    websocket.set_onmessage(None);
    websocket.set_onclose(None);
    websocket.set_onerror(None);
}

fn send_message(websocket: &web_sys::WebSocket, message: Message) -> Result<(), Error> {
    match message {
        Message::Text(text) => websocket.send_with_str(&text)?,
        Message::Binary(data) => websocket.send_with_u8_array(&data)?,
        Message::Close { code, reason } => {
            websocket.close_with_code_and_reason(code.into(), &reason)?
        }
        #[allow(deprecated)]
        Message::Ping(_) | Message::Pong(_) => {
            // ignored!
        }
    }
    Ok::<(), Error>(())
}