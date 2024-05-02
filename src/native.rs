use std::borrow::Cow;

use reqwest::{
    header::{
        HeaderName,
        HeaderValue,
    },
    RequestBuilder,
    Response,
    StatusCode,
    Version,
};

use crate::{
    Error,
    Message,
};

pub async fn send_request(request_builder: RequestBuilder) -> Result<WebSocketResponse, Error> {
    let (client, request_result) = request_builder.build_split();
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

    // prepare request
    let version = request.version();
    let nonce;
    match version {
        Version::HTTP_10 | Version::HTTP_11 => {
            // HTTP 1 requires us to set some headers
            let nonce_value = tungstenite::handshake::client::generate_key();
            let headers = request.headers_mut();
            headers.insert(
                reqwest::header::CONNECTION,
                HeaderValue::from_static("upgrade"),
            );
            headers.insert(
                reqwest::header::UPGRADE,
                HeaderValue::from_static("websocket"),
            );
            headers.insert(
                reqwest::header::SEC_WEBSOCKET_KEY,
                HeaderValue::from_str(&nonce_value).expect("nonce is a invalid header value"),
            );
            headers.insert(
                reqwest::header::SEC_WEBSOCKET_VERSION,
                HeaderValue::from_static("13"),
            );
            nonce = Some(nonce_value);
        }
        Version::HTTP_2 => {
            //todo!("implement websocket upgrade for http 2");
            return Err(HandshakeError::UnsupportedHttpVersion(version).into());
        }
        _ => {
            return Err(HandshakeError::UnsupportedHttpVersion(version).into());
        }
    }

    // execute request
    let response = client.execute(request).await?;

    Ok(WebSocketResponse {
        response,
        version,
        nonce,
    })
}

pub type WebSocketStream =
    async_tungstenite::WebSocketStream<tokio_util::compat::Compat<reqwest::Upgraded>>;

/// Error during Websocket handshake
#[derive(Debug, thiserror::Error)]
pub enum HandshakeError {
    #[error("unsupported http version: {0:?}")]
    UnsupportedHttpVersion(Version),

    #[error("the server responded with a different http version. this could be the case because reqwest silently upgraded the connection to http2. see: https://github.com/jgraef/reqwest-websocket/issues/2")]
    ServerRespondedWithDifferentVersion,

    #[error("missing header {header}")]
    MissingHeader { header: HeaderName },

    #[error("unexpected value for header {header}: expected {expected}, but got {got:?}.")]
    UnexpectedHeaderValue {
        header: HeaderName,
        got: HeaderValue,
        expected: Cow<'static, str>,
    },

    #[error("expected the server to select a protocol.")]
    ExpectedAProtocol,

    #[error("unexpected protocol: {got}")]
    UnexpectedProtocol { got: String },

    #[error("unexpected status code: {0}")]
    UnexpectedStatusCode(StatusCode),
}

pub struct WebSocketResponse {
    pub response: Response,
    pub version: Version,
    pub nonce: Option<String>,
}

impl WebSocketResponse {
    pub async fn into_stream_and_protocol(
        self,
        protocols: Vec<String>,
    ) -> Result<(WebSocketStream, Option<String>), Error> {
        let headers = self.response.headers();

        if self.response.version() != self.version {
            return Err(HandshakeError::ServerRespondedWithDifferentVersion.into());
        }

        if self.response.status() != reqwest::StatusCode::SWITCHING_PROTOCOLS {
            tracing::debug!(status_code = %self.response.status(), "server responded with unexpected status code");
            return Err(HandshakeError::UnexpectedStatusCode(self.response.status()).into());
        }

        if let Some(header) = headers.get(reqwest::header::CONNECTION) {
            if !header
                .to_str()
                .ok()
                .map(|s| s.eq_ignore_ascii_case("upgrade"))
                .unwrap_or_default()
            {
                tracing::debug!("server responded with invalid Connection header: {header:?}");
                return Err(HandshakeError::UnexpectedHeaderValue {
                    header: reqwest::header::CONNECTION,
                    got: header.clone(),
                    expected: "upgrade".into(),
                }
                .into());
            }
        }
        else {
            tracing::debug!("missing Connection header");
            return Err(HandshakeError::MissingHeader {
                header: reqwest::header::CONNECTION,
            }
            .into());
        }

        if let Some(header) = headers.get(reqwest::header::UPGRADE) {
            if !header
                .to_str()
                .ok()
                .map(|s| s.eq_ignore_ascii_case("websocket"))
                .unwrap_or_default()
            {
                tracing::debug!("server responded with invalid Upgrade header: {header:?}");
                return Err(HandshakeError::UnexpectedHeaderValue {
                    header: reqwest::header::UPGRADE,
                    got: header.clone(),
                    expected: "websocket".into(),
                }
                .into());
            }
        }
        else {
            tracing::debug!("missing Upgrade header");
            return Err(HandshakeError::MissingHeader {
                header: reqwest::header::UPGRADE,
            }
            .into());
        }

        if let Some(nonce) = &self.nonce {
            let expected_nonce = tungstenite::handshake::derive_accept_key(nonce.as_bytes());

            if let Some(header) = headers.get(reqwest::header::SEC_WEBSOCKET_ACCEPT) {
                if !header
                    .to_str()
                    .ok()
                    .map(|s| s == expected_nonce)
                    .unwrap_or_default()
                {
                    tracing::debug!(
                        "server responded with invalid Sec-Websocket-Accept header: {header:?}"
                    );
                    return Err(HandshakeError::UnexpectedHeaderValue {
                        header: reqwest::header::SEC_WEBSOCKET_ACCEPT,
                        got: header.clone(),
                        expected: expected_nonce.into(),
                    }
                    .into());
                }
            }
            else {
                tracing::debug!("missing Sec-Websocket-Accept header");
                return Err(HandshakeError::MissingHeader {
                    header: reqwest::header::SEC_WEBSOCKET_ACCEPT,
                }
                .into());
            }
        }

        let protocol = headers
            .get(reqwest::header::SEC_WEBSOCKET_PROTOCOL)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_owned());

        match (protocols.is_empty(), &protocol) {
            (true, None) => {
                // we didn't request any protocols, so we don't expect one
                // in return
            }
            (false, None) => {
                // server didn't reply with a protocol
                return Err(HandshakeError::ExpectedAProtocol.into());
            }
            (false, Some(protocol)) => {
                if !protocols.contains(protocol) {
                    // the responded protocol is none which we requested
                    return Err(HandshakeError::UnexpectedProtocol {
                        got: protocol.clone(),
                    }
                    .into());
                }
            }
            (true, Some(protocol)) => {
                // we didn't request any protocols but got one anyway
                return Err(HandshakeError::UnexpectedProtocol {
                    got: protocol.clone(),
                }
                .into());
            }
        }

        use tokio_util::compat::TokioAsyncReadCompatExt;

        let inner = WebSocketStream::from_raw_socket(
            self.response.upgrade().await?.compat(),
            tungstenite::protocol::Role::Client,
            None,
        )
        .await;

        Ok((inner, protocol))
    }
}

#[derive(Debug, thiserror::Error)]
#[error("could not convert message")]
pub struct FromTungsteniteMessageError {
    pub original: tungstenite::Message,
}

impl TryFrom<tungstenite::Message> for Message {
    type Error = FromTungsteniteMessageError;

    fn try_from(value: tungstenite::Message) -> Result<Self, Self::Error> {
        match value {
            tungstenite::Message::Text(text) => Ok(Self::Text(text)),
            tungstenite::Message::Binary(data) => Ok(Self::Binary(data)),
            tungstenite::Message::Ping(data) => Ok(Self::Ping(data)),
            tungstenite::Message::Pong(data) => Ok(Self::Pong(data)),
            _ => Err(FromTungsteniteMessageError { original: value }),
        }
    }
}

impl From<Message> for tungstenite::Message {
    fn from(value: Message) -> Self {
        match value {
            Message::Text(text) => Self::Text(text),
            Message::Binary(data) => Self::Binary(data),
            Message::Ping(text) => Self::Ping(text),
            Message::Pong(data) => Self::Pong(data),
        }
    }
}
