//! Provides wrappers for [`reqwest`][2] to enable [websocket][1] connections.
//!
//! # Example
//!
//! ```
//! # use reqwest::Client;
//! # use reqwest_websocket::Message;
//! # use futures_util::{TryStreamExt, SinkExt};
//! # fn main() {
//! # run(); // intentionally ignore the future. we only care that it compiles.
//! # }
//! # async fn run() -> Result<(), Box<dyn std::error::Error>> {
//! // Extends the reqwest::RequestBuilder to allow websocket upgrades
//! use reqwest_websocket::RequestBuilderExt;
//!
//! // create a GET request, upgrade it and send it.
//! let response = Client::default()
//!     .get("wss://echo.websocket.org/")
//!     .upgrade() // prepares the websocket upgrade.
//!     .send()
//!     .await?;
//!
//! // turn the response into a websocket stream
//! let mut websocket = response.into_websocket().await?;
//!
//! // the websocket implements `Sink<Message>`.
//! websocket.send(Message::Text("Hello, World".into())).await?;
//!
//! // the websocket is also a `TryStream` over `Message`s.
//! while let Some(message) = websocket.try_next().await? {
//!     match message {
//!         Message::Text(text) => println!("{text}"),
//!         _ => {}
//!     }
//! }
//! # Ok(())
//! # }
//! ```
//!
//! [1]: https://en.wikipedia.org/wiki/WebSocket
//! [2]: https://docs.rs/reqwest/latest/reqwest/index.html

use std::{
    pin::Pin,
    task::{
        Context,
        Poll,
    },
};

use futures_util::{
    Sink,
    SinkExt,
    Stream,
    StreamExt,
};
use reqwest::{
    Client,
    IntoUrl,
    RequestBuilder,
};

#[cfg(target_arch = "wasm32")]
mod wasm;

/// Errors returned by `reqwest_websocket`
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("websocket upgrade failed")]
    HandshakeFailed,

    #[error("reqwest error")]
    Reqwest(#[from] reqwest::Error),

    #[cfg(not(target_arch = "wasm32"))]
    #[error("tungstenite error")]
    Tungstenite(#[from] tungstenite::Error),

    #[cfg(target_arch = "wasm32")]
    #[error("web_sys error")]
    WebSys(#[from] wasm::WebSysError),
}

#[derive(Clone, Debug)]
pub enum Message {
    Text(String),
    Binary(Vec<u8>),
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, thiserror::Error)]
#[error("could not convert message")]
pub struct FromTungsteniteMessageError {
    pub original: tungstenite::Message,
}

#[cfg(not(target_arch = "wasm32"))]
impl TryFrom<tungstenite::Message> for Message {
    type Error = FromTungsteniteMessageError;

    fn try_from(value: tungstenite::Message) -> Result<Self, Self::Error> {
        match value {
            tungstenite::Message::Text(text) => Ok(Self::Text(text)),
            tungstenite::Message::Binary(data) => Ok(Self::Binary(data)),
            _ => Err(FromTungsteniteMessageError { original: value }),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl From<Message> for tungstenite::Message {
    fn from(value: Message) -> Self {
        match value {
            Message::Text(text) => Self::Text(text),
            Message::Binary(data) => Self::Binary(data),
        }
    }
}

/// Opens a websocket at the specified URL.
///
/// This is a shorthand for creating a request, sending it, and turning the
/// response into a websocket.
pub async fn websocket(url: impl IntoUrl) -> Result<WebSocket, Error> {
    Ok(Client::new()
        .get(url)
        .upgrade()
        .send()
        .await?
        .into_websocket()
        .await?)
}

/// Trait that extends [`reqwest::RequestBuilder`] with an `upgrade` method.
pub trait RequestBuilderExt {
    fn upgrade(self) -> UpgradedRequestBuilder;
}

impl RequestBuilderExt for RequestBuilder {
    /// Upgrades the [`RequestBuilder`] to peform a
    /// websocket handshake. This returns a wrapped type, so you you must do
    /// this after you setup your request, and just before you send the
    /// request.
    fn upgrade(self) -> UpgradedRequestBuilder {
        UpgradedRequestBuilder::new(self)
    }
}

/// Wrapper for [`RequestBuilder`] that performs the
/// websocket handshake when sent.
pub struct UpgradedRequestBuilder {
    inner: RequestBuilder,

    #[cfg(not(target_arch = "wasm32"))]
    nonce: String,

    protocols: Vec<String>,
}

impl UpgradedRequestBuilder {
    pub(crate) fn new(inner: RequestBuilder) -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        let (nonce, inner) = {
            let nonce = tungstenite::handshake::client::generate_key();
            let inner = inner
                .header(reqwest::header::CONNECTION, "upgrade")
                .header(reqwest::header::UPGRADE, "websocket")
                .header(reqwest::header::SEC_WEBSOCKET_KEY, &nonce)
                .header(reqwest::header::SEC_WEBSOCKET_VERSION, "13");

            (nonce, inner)
        };

        Self {
            inner,
            #[cfg(not(target_arch = "wasm32"))]
            nonce,
            protocols: vec![],
        }
    }

    /// Sends the request and returns and [`UpgradeResponse`].
    pub async fn send(self) -> Result<UpgradeResponse, Error> {
        #[cfg(not(target_arch = "wasm32"))]
        let inner = {
            let (client, request_result) = self.inner.build_split();
            let mut request = request_result?;

            // change the scheme from wss? to https?
            let url = request.url_mut();
            match url.scheme() {
                "ws" => {
                    url.set_scheme("http")
                        .expect("url should accept http scheme")
                }
                "wss" => {
                    url.set_scheme("https")
                        .expect("url should accept https scheme")
                }
                _ => {}
            }

            client.execute(request).await?
        };

        #[cfg(target_arch = "wasm32")]
        let inner = wasm::WebSysWebSocketStream::new(self.inner.build()?, &self.protocols).await?;

        Ok(UpgradeResponse {
            inner,
            #[cfg(not(target_arch = "wasm32"))]
            nonce: self.nonce,
            protocols: self.protocols,
        })
    }
}

/// The server's response to the websocket upgrade request.
///
/// This implements `Deref<Target = Response>`, so you can access all the usual
/// information from the [`Response`].
pub struct UpgradeResponse {
    #[cfg(not(target_arch = "wasm32"))]
    inner: reqwest::Response,

    #[cfg(target_arch = "wasm32")]
    inner: wasm::WebSysWebSocketStream,

    #[cfg(not(target_arch = "wasm32"))]
    nonce: String,

    #[allow(dead_code)]
    protocols: Vec<String>,
}

#[cfg(not(target_arch = "wasm32"))]
impl std::ops::Deref for UpgradeResponse {
    type Target = reqwest::Response;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl UpgradeResponse {
    /// Turns the response into a websocket. This checks if the websocket
    /// handshake was successful.
    pub async fn into_websocket(self) -> Result<WebSocket, Error> {
        #[cfg(not(target_arch = "wasm32"))]
        let (inner, protocol) = {
            let headers = self.inner.headers();

            if self.inner.status() != reqwest::StatusCode::SWITCHING_PROTOCOLS {
                tracing::debug!(status_code = %self.inner.status(), "server responded with unexpected status code");
                return Err(Error::HandshakeFailed);
            }

            if !headers
                .get(reqwest::header::CONNECTION)
                .and_then(|v| v.to_str().ok())
                .map(|s| s.eq_ignore_ascii_case("upgrade"))
                .unwrap_or_default()
            {
                tracing::debug!("server responded with invalid Connection header");
                return Err(Error::HandshakeFailed);
            }

            if !headers
                .get(reqwest::header::UPGRADE)
                .and_then(|v| v.to_str().ok())
                .map(|s| s.eq_ignore_ascii_case("websocket"))
                .unwrap_or_default()
            {
                tracing::debug!("server responded with invalid Upgrade header");
                return Err(Error::HandshakeFailed);
            }

            let accept = headers
                .get(reqwest::header::SEC_WEBSOCKET_ACCEPT)
                .and_then(|v| v.to_str().ok())
                .ok_or(Error::HandshakeFailed)?;
            let expected_accept = tungstenite::handshake::derive_accept_key(self.nonce.as_bytes());
            if accept != expected_accept {
                tracing::debug!(got=?accept, expected=expected_accept, "server responded with invalid accept token");
                return Err(Error::HandshakeFailed);
            }

            let protocol = headers
                .get(reqwest::header::SEC_WEBSOCKET_PROTOCOL)
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_owned());

            match (self.protocols.is_empty(), &protocol) {
                (true, None) => {
                    // we didn't request any protocols, so we don't expect one
                    // in return
                }
                (false, None) => {
                    // server didn't reply with a protocol
                    return Err(Error::HandshakeFailed);
                }
                (false, Some(protocol)) => {
                    if !self.protocols.contains(protocol) {
                        // the responded protocol is none which we requested
                        return Err(Error::HandshakeFailed);
                    }
                }
                (true, Some(_)) => {
                    // we didn't request any protocols but got one anyway
                    return Err(Error::HandshakeFailed);
                }
            }

            use tokio_util::compat::TokioAsyncReadCompatExt;

            let inner = async_tungstenite::WebSocketStream::from_raw_socket(
                self.inner.upgrade().await?.compat(),
                tungstenite::protocol::Role::Client,
                None,
            )
            .await;

            (inner, protocol)
        };

        #[cfg(target_arch = "wasm32")]
        let (inner, protocol) = {
            let protocol = self.inner.protocol();
            (self.inner, Some(protocol))
        };

        Ok(WebSocket { inner, protocol })
    }
}

/// A websocket connection
pub struct WebSocket {
    #[cfg(not(target_arch = "wasm32"))]
    inner: async_tungstenite::WebSocketStream<tokio_util::compat::Compat<reqwest::Upgraded>>,

    #[cfg(target_arch = "wasm32")]
    inner: wasm::WebSysWebSocketStream,

    protocol: Option<String>,
}

impl WebSocket {
    pub fn protocol(&self) -> Option<&str> {
        self.protocol.as_deref()
    }
}

impl Stream for WebSocket {
    type Item = Result<Message, Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        loop {
            match self.inner.poll_next_unpin(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(None) => return Poll::Ready(None),
                Poll::Ready(Some(Err(error))) => return Poll::Ready(Some(Err(error.into()))),
                Poll::Ready(Some(Ok(message))) => {
                    match message.try_into() {
                        Ok(message) => return Poll::Ready(Some(Ok(message))),
                        Err(_) => {
                            // this won't convert pings, pongs, etc. but we
                            // don't care about those.
                        }
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
mod tests {
    use futures_util::{
        SinkExt,
        TryStreamExt,
    };
    use reqwest::Client;

    use super::{
        Message,
        RequestBuilderExt,
    };
    use crate::{
        websocket,
        WebSocket,
    };

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

    #[tokio::test]
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

    #[tokio::test]
    async fn test_shorthand() {
        let websocket = websocket("https://echo.websocket.org/").await.unwrap();
        test_websocket(websocket).await;
    }

    #[tokio::test]
    async fn test_with_ws_scheme() {
        let websocket = websocket("wss://echo.websocket.org/").await.unwrap();

        test_websocket(websocket).await;
    }
}
