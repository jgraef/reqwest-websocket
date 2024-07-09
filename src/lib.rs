#![forbid(unsafe_code)]
#![cfg_attr(docsrs, feature(doc_cfg))]

//! Provides wrappers for [`reqwest`][2] to enable [`WebSocket`][1] connections.
//!
//! # Example
//!
//! ```
//! # use reqwest::Client;
//! # use reqwest_websocket::{Message, Error};
//! # use futures_util::{TryStreamExt, SinkExt};
//! #
//! # fn main() {
//! #     // Intentionally ignore the future. We only care that it compiles.
//! #     let _ = run();
//! # }
//! #
//! # async fn run() -> Result<(), Error> {
//! // Extends the `reqwest::RequestBuilder` to allow WebSocket upgrades.
//! use reqwest_websocket::RequestBuilderExt;
//!
//! // Creates a GET request, upgrades and sends it.
//! let response = Client::default()
//!     .get("wss://echo.websocket.org/")
//!     .upgrade() // Prepares the WebSocket upgrade.
//!     .send()
//!     .await?;
//!
//! // Turns the response into a WebSocket stream.
//! let mut websocket = response.into_websocket().await?;
//!
//! // The WebSocket implements `Sink<Message>`.
//! websocket.send(Message::Text("Hello, World".into())).await?;
//!
//! // The WebSocket is also a `TryStream` over `Message`s.
//! while let Some(message) = websocket.try_next().await? {
//!     if let Message::Text(text) = message {
//!         println!("received: {text}")
//!     }
//! }
//! # Ok(())
//! # }
//! ```
//!
//! [1]: https://en.wikipedia.org/wiki/WebSocket
//! [2]: https://docs.rs/reqwest/latest/reqwest/index.html

#[cfg(feature = "json")]
mod json;
#[cfg(not(target_arch = "wasm32"))]
mod native;
mod protocol;
#[cfg(target_arch = "wasm32")]
mod wasm;

use std::{
    pin::Pin,
    task::{ready, Context, Poll},
};

#[cfg(not(target_arch = "wasm32"))]
#[cfg_attr(docsrs, doc(cfg(not(target_arch = "wasm32"))))]
pub use crate::native::HandshakeError;
pub use crate::protocol::{CloseCode, Message};
use futures_util::{Sink, SinkExt, Stream, StreamExt};
use reqwest::{Client, ClientBuilder, IntoUrl, RequestBuilder};

/// Errors returned by `reqwest_websocket`.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    #[cfg(not(target_arch = "wasm32"))]
    #[cfg_attr(docsrs, doc(cfg(not(target_arch = "wasm32"))))]
    #[error("websocket upgrade failed")]
    Handshake(#[from] HandshakeError),

    #[error("reqwest error")]
    Reqwest(#[from] reqwest::Error),

    #[cfg(not(target_arch = "wasm32"))]
    #[cfg_attr(docsrs, doc(cfg(not(target_arch = "wasm32"))))]
    #[error("tungstenite error")]
    Tungstenite(#[from] tungstenite::Error),

    #[cfg(target_arch = "wasm32")]
    #[cfg_attr(docsrs, doc(cfg(target_arch = "wasm32")))]
    #[error("web_sys error")]
    WebSys(#[from] wasm::Error),

    /// Error during serialization/deserialization.
    #[error("serde_json error")]
    #[cfg(feature = "json")]
    #[cfg_attr(docsrs, doc(cfg(feature = "json")))]
    Json(#[from] serde_json::Error),
}

/// Opens a `WebSocket` connection at the specified `URL`.
///
/// This is a shorthand for creating a [`Request`], sending it, and turning the
/// [`Response`] into a [`WebSocket`].
///
/// [`Request`]: reqwest::Request
/// [`Response`]: reqwest::Response
pub async fn websocket(url: impl IntoUrl) -> Result<WebSocket, Error> {
    builder_http1_only(Client::builder())
        .build()?
        .get(url)
        .upgrade()
        .send()
        .await?
        .into_websocket()
        .await
}

#[inline]
#[cfg(not(target_arch = "wasm32"))]
fn builder_http1_only(builder: ClientBuilder) -> ClientBuilder {
    builder.http1_only()
}

#[inline]
#[cfg(target_arch = "wasm32")]
fn builder_http1_only(builder: ClientBuilder) -> ClientBuilder {
    builder
}

/// Trait that extends [`reqwest::RequestBuilder`] with an `upgrade` method.
pub trait RequestBuilderExt {
    /// Upgrades the [`RequestBuilder`] to perform a `WebSocket` handshake.
    ///
    /// This returns a wrapped type, so you must do this after you set up
    /// your request, and just before sending the request.
    fn upgrade(self) -> UpgradedRequestBuilder;
}

impl RequestBuilderExt for RequestBuilder {
    fn upgrade(self) -> UpgradedRequestBuilder {
        UpgradedRequestBuilder::new(self)
    }
}

/// Wrapper for a [`reqwest::RequestBuilder`] that performs the
/// `WebSocket` handshake when sent.
pub struct UpgradedRequestBuilder {
    inner: RequestBuilder,
    protocols: Vec<String>,
}

impl UpgradedRequestBuilder {
    pub(crate) fn new(inner: RequestBuilder) -> Self {
        Self {
            inner,
            protocols: vec![],
        }
    }

    /// Sends the request and returns an [`UpgradeResponse`].
    pub async fn send(self) -> Result<UpgradeResponse, Error> {
        #[cfg(not(target_arch = "wasm32"))]
        let inner = native::send_request(self.inner).await?;

        #[cfg(target_arch = "wasm32")]
        let inner = wasm::WebSocket::new(self.inner.build()?, &self.protocols).await?;

        Ok(UpgradeResponse {
            inner,
            protocols: self.protocols,
        })
    }
}

/// The server's response to the `WebSocket` upgrade request.
///
/// On non-wasm platforms, this implements `Deref<Target = Response>`, so you
/// can access all the usual information from the [`reqwest::Response`].
pub struct UpgradeResponse {
    #[cfg(not(target_arch = "wasm32"))]
    inner: native::WebSocketResponse,

    #[cfg(target_arch = "wasm32")]
    inner: wasm::WebSocket,

    #[allow(dead_code)]
    protocols: Vec<String>,
}

#[cfg(not(target_arch = "wasm32"))]
impl std::ops::Deref for UpgradeResponse {
    type Target = reqwest::Response;

    fn deref(&self) -> &Self::Target {
        &self.inner.response
    }
}

impl UpgradeResponse {
    /// Turns the response into a `WebSocket`.
    /// This checks if the `WebSocket` handshake was successful.
    pub async fn into_websocket(self) -> Result<WebSocket, Error> {
        #[cfg(not(target_arch = "wasm32"))]
        let (inner, protocol) = self.inner.into_stream_and_protocol(self.protocols).await?;

        #[cfg(target_arch = "wasm32")]
        let (inner, protocol) = {
            let protocol = self.inner.protocol().to_owned();
            (self.inner, Some(protocol))
        };

        Ok(WebSocket { inner, protocol })
    }

    /// Consumes the response and returns the inner [`reqwest::Response`].
    #[must_use]
    #[cfg(not(target_arch = "wasm32"))]
    pub fn into_inner(self) -> reqwest::Response {
        self.inner.response
    }
}

/// A `WebSocket` connection. Implements [`futures_util::Stream`] and
/// [`futures_util::Sink`].
#[derive(Debug)]
pub struct WebSocket {
    #[cfg(not(target_arch = "wasm32"))]
    inner: native::WebSocketStream,

    #[cfg(target_arch = "wasm32")]
    inner: wasm::WebSocket,

    protocol: Option<String>,
}

impl WebSocket {
    /// Returns the protocol negotiated during the handshake.
    pub fn protocol(&self) -> Option<&str> {
        self.protocol.as_deref()
    }

    /// Closes the connection with a given code and (optional) reason.
    ///
    /// # WASM
    ///
    /// On wasm `code` must be [`CloseCode::Normal`], [`CloseCode::Iana(_)`],
    /// or [`CloseCode::Library(_)`]. Furthermore `reason` must be at most 123
    /// bytes long. Otherwise the call to [`close`][Self::close] will fail.
    pub async fn close(self, code: CloseCode, reason: Option<&str>) -> Result<(), Error> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let mut inner = self.inner;
            inner
                .close(Some(tungstenite::protocol::CloseFrame {
                    code: code.into(),
                    reason: reason.unwrap_or_default().into(),
                }))
                .await?;
        }

        #[cfg(target_arch = "wasm32")]
        {
            let mut inner = self.inner;
            inner
                .send(Message::Close {
                    code,
                    reason: reason.unwrap_or_default().to_owned(),
                })
                .await?;
        }

        Ok(())
    }
}

impl Stream for WebSocket {
    type Item = Result<Message, Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match ready!(self.inner.poll_next_unpin(cx)) {
            None => Poll::Ready(None),
            Some(Err(error)) => Poll::Ready(Some(Err(error.into()))),
            Some(Ok(message)) => {
                match message.try_into() {
                    Ok(message) => Poll::Ready(Some(Ok(message))),

                    #[cfg(target_arch = "wasm32")]
                    Err(e) => match e {},

                    #[cfg(not(target_arch = "wasm32"))]
                    Err(e) => {
                        // this fails only for raw frames (which are not received)
                        panic!("Received an invalid frame: {e}");
                    }
                }
            }
        }
    }
}

impl Sink<Message> for WebSocket {
    type Error = Error;

    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready_unpin(cx).map_err(Into::into)
    }

    fn start_send(mut self: Pin<&mut Self>, item: Message) -> Result<(), Self::Error> {
        self.inner.start_send_unpin(item.into()).map_err(Into::into)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_flush_unpin(cx).map_err(Into::into)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_close_unpin(cx).map_err(Into::into)
    }
}

#[cfg(test)]
pub mod tests {
    use futures_util::{SinkExt, TryStreamExt};
    use reqwest::Client;
    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::wasm_bindgen_test;

    #[cfg(target_arch = "wasm32")]
    wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

    use super::{websocket, CloseCode, Message, RequestBuilderExt, WebSocket};

    macro_rules! assert_send_sync {
        ($ty:ty) => {
            const _: () = {
                struct Assert<T: Send + Sync>(std::marker::PhantomData<T>);
                Assert::<$ty>(std::marker::PhantomData);
            };
        };
    }

    assert_send_sync!(WebSocket);

    async fn test_websocket(mut websocket: WebSocket) {
        let text = "Hello, World!";
        websocket
            .send(Message::Text(text.to_owned()))
            .await
            .unwrap();

        while let Some(message) = websocket.try_next().await.unwrap() {
            match message {
                Message::Text(s) => {
                    if s == text {
                        return;
                    }
                }
                _ => {}
            }
        }

        panic!("didn't receive text back");
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), tokio::test)]
    async fn test_with_request_builder() {
        let websocket = Client::default()
            .get("https://echo.websocket.org/")
            .upgrade()
            .send()
            .await
            .unwrap()
            .into_websocket()
            .await
            .unwrap();

        test_websocket(websocket).await;
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), tokio::test)]
    async fn test_shorthand() {
        let websocket = websocket("https://echo.websocket.org/").await.unwrap();
        test_websocket(websocket).await;
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), tokio::test)]
    async fn test_with_ws_scheme() {
        let websocket = websocket("wss://echo.websocket.org/").await.unwrap();

        test_websocket(websocket).await;
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), tokio::test)]
    async fn test_close() {
        let websocket = websocket("https://echo.websocket.org/").await.unwrap();
        websocket
            .close(CloseCode::Normal, Some("test"))
            .await
            .expect("close returned an error");
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), tokio::test)]
    async fn test_send_close_frame() {
        let mut websocket = websocket("https://echo.websocket.org/").await.unwrap();
        websocket
            .send(Message::Close {
                code: CloseCode::Normal,
                reason: "Can you please reply with a close frame?".into(),
            })
            .await
            .unwrap();

        let mut close_received = false;
        while let Some(message) = websocket.try_next().await.unwrap() {
            match message {
                Message::Close { code, .. } => {
                    assert_eq!(code, CloseCode::Normal);
                    close_received = true;
                }
                _ => {}
            }
        }

        assert!(close_received, "No close frame was received");
    }

    #[test]
    fn close_code_from_u16() {
        let byte = 1008u16;
        assert_eq!(CloseCode::from(byte), CloseCode::Policy);
    }

    #[test]
    fn close_code_into_u16() {
        let text = CloseCode::Away;
        let byte: u16 = text.into();
        assert_eq!(byte, 1001u16);
        assert_eq!(u16::from(text), 1001u16);
    }
}
