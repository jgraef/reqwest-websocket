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
pub use reqwest::RequestBuilder;
use reqwest::{
    header,
    Response,
    StatusCode,
    Upgraded,
};
pub use tokio_util::compat::{
    Compat,
    TokioAsyncReadCompatExt,
    TokioAsyncWriteCompatExt,
};
use tungstenite::protocol::Role;
pub use tungstenite::Message;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("websocket upgrade failed")]
    HandshakeFailed,

    #[error("reqwest error")]
    Reqwest(#[from] reqwest::Error),

    #[error("tungstenite error")]
    Tungstenite(#[from] tungstenite::Error),
}

pub trait RequestBuilderExt {
    fn upgrade(self) -> UpgradedRequestBuilder;
}

impl RequestBuilderExt for RequestBuilder {
    fn upgrade(self) -> UpgradedRequestBuilder {
        UpgradedRequestBuilder::new(self)
    }
}

pub struct UpgradedRequestBuilder {
    inner: RequestBuilder,
    nonce: String,
}

impl UpgradedRequestBuilder {
    pub fn new(inner: RequestBuilder) -> Self {
        let nonce = tungstenite::handshake::client::generate_key();

        let inner = inner
            .header(header::CONNECTION, "upgrade")
            .header(header::UPGRADE, "websocket")
            .header(header::SEC_WEBSOCKET_KEY, &nonce)
            .header(header::SEC_WEBSOCKET_VERSION, "13"); // ??

        Self { inner, nonce }
    }

    pub async fn send(self) -> Result<UpgradeResponse, Error> {
        let inner = self.inner.send().await?;
        Ok(UpgradeResponse {
            inner,
            nonce: self.nonce,
        })
    }
}

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
    pub async fn into_websocket(self) -> Result<WebSocket, Error> {
        let headers = self.inner.headers();
        if self.inner.status() != StatusCode::SWITCHING_PROTOCOLS
            || headers
                .get(header::CONNECTION)
                .map(|v| v == "upgrade")
                .unwrap_or_default()
            || headers
                .get(header::UPGRADE)
                .map(|v| v == "websocket")
                .unwrap_or_default()
        {
            return Err(Error::HandshakeFailed);
        }

        let accept = headers
            .get(header::SEC_WEBSOCKET_ACCEPT)
            .ok_or(Error::HandshakeFailed)?;
        let expected_accept = tungstenite::handshake::derive_accept_key(self.nonce.as_bytes());
        if accept == &expected_accept {
            return Err(Error::HandshakeFailed);
        }

        let protocol = headers
            .get(header::SEC_WEBSOCKET_PROTOCOL)
            .and_then(|v| v.to_str().ok().map(|s| s.to_owned()));

        let inner = WebSocketStream::from_raw_socket(
            self.inner.upgrade().await?.compat(),
            Role::Client,
            None,
        )
        .await;

        Ok(WebSocket { inner, protocol })
    }
}

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
