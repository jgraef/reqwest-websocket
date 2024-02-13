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
//! // don't use `ws://` or `wss://` for the url, but rather `http://` or `https://`
//! let response = Client::default()
//!     .get("https://echo.websocket.org/")
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
    ops::Deref,
    pin::Pin,
    task::{
        Context,
        Poll,
    },
};

use async_tungstenite::WebSocketStream;
use futures_util::{
    Sink,
    SinkExt,
    Stream,
    StreamExt,
};
use reqwest::{
    header,
    RequestBuilder,
    Response,
    StatusCode,
    Upgraded,
};
use tokio_util::compat::{
    Compat,
    TokioAsyncReadCompatExt,
};
use tungstenite::protocol::Role;
pub use tungstenite::Message;

/// Errors returned by `reqwest_websocket`
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("websocket upgrade failed")]
    HandshakeFailed,

    #[error("reqwest error")]
    Reqwest(#[from] reqwest::Error),

    #[error("tungstenite error")]
    Tungstenite(#[from] tungstenite::Error),
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
    nonce: String,
}

impl UpgradedRequestBuilder {
    fn new(inner: RequestBuilder) -> Self {
        let nonce = tungstenite::handshake::client::generate_key();

        let inner = inner
            .header(header::CONNECTION, "upgrade")
            .header(header::UPGRADE, "websocket")
            .header(header::SEC_WEBSOCKET_KEY, &nonce)
            .header(header::SEC_WEBSOCKET_VERSION, "13"); // ??

        Self { inner, nonce }
    }

    /// Sends the request and returns and [`UpgradeResponse`].
    pub async fn send(self) -> Result<UpgradeResponse, Error> {
        let inner = self.inner.send().await?;
        Ok(UpgradeResponse {
            inner,
            nonce: self.nonce,
        })
    }
}

/// The server's response to the websocket upgrade request.
///
/// This implements `Deref<Target = Response>`, so you can access all the usual
/// information from the [`Response`].
pub struct UpgradeResponse {
    inner: Response,
    nonce: String,
}

impl Deref for UpgradeResponse {
    type Target = Response;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl UpgradeResponse {
    /// Turns the response into a websocket. This checks if the websocket
    /// handshake was successful.
    pub async fn into_websocket(self) -> Result<WebSocket, Error> {
        let headers = self.inner.headers();

        if self.inner.status() != StatusCode::SWITCHING_PROTOCOLS {
            tracing::debug!(status_code = %self.inner.status(), "server responded with unexpected status code");
            return Err(Error::HandshakeFailed);
        }

        if !headers
            .get(header::CONNECTION)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.eq_ignore_ascii_case("upgrade"))
            .unwrap_or_default()
        {
            tracing::debug!("server responded with invalid Connection header");
            return Err(Error::HandshakeFailed);
        }

        if !headers
            .get(header::UPGRADE)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.eq_ignore_ascii_case("websocket"))
            .unwrap_or_default()
        {
            tracing::debug!("server responded with invalid Upgrade header");
            return Err(Error::HandshakeFailed);
        }

        let accept = headers
            .get(header::SEC_WEBSOCKET_ACCEPT)
            .and_then(|v| v.to_str().ok())
            .ok_or(Error::HandshakeFailed)?;
        let expected_accept = tungstenite::handshake::derive_accept_key(self.nonce.as_bytes());
        if accept != &expected_accept {
            tracing::debug!(got=?accept, expected=expected_accept, "server responded with invalid accept token");
            return Err(Error::HandshakeFailed);
        }

        let protocol = headers
            .get(header::SEC_WEBSOCKET_PROTOCOL)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_owned());

        let inner = WebSocketStream::from_raw_socket(
            self.inner.upgrade().await?.compat(),
            Role::Client,
            None,
        )
        .await;

        Ok(WebSocket { inner, protocol })
    }
}

/// A websocket connection
pub struct WebSocket {
    inner: WebSocketStream<Compat<Upgraded>>,
    protocol: Option<String>,
}

impl WebSocket {
    pub fn protocol(&self) -> Option<&str> {
        self.protocol.as_ref().map(|s| s.as_str())
    }
}

impl Stream for WebSocket {
    type Item = Result<Message, Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.inner.poll_next_unpin(cx).map_err(Into::into)
    }
}

impl Sink<Message> for WebSocket {
    type Error = Error;

    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready_unpin(cx).map_err(Into::into)
    }

    fn start_send(mut self: Pin<&mut Self>, item: Message) -> Result<(), Self::Error> {
        self.inner.start_send_unpin(item).map_err(Into::into)
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
    use tungstenite::Message;

    use crate::RequestBuilderExt;

    #[tokio::test]
    async fn test_handshake() {
        let mut websocket = Client::default()
            .get("https://echo.websocket.org/")
            .upgrade()
            .send()
            .await
            .unwrap()
            .into_websocket()
            .await
            .unwrap();

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

        panic!("didn't receive text back")
    }
}
