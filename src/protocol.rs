#[cfg(feature = "json")]
use serde::{
    de::DeserializeOwned,
    Serialize,
};

#[cfg(feature = "json")]
use crate::Result;

/// A `WebSocket` message, which can be a text string or binary data.
#[derive(Clone, Debug)]
pub enum Message {
    Text(String),
    Binary(Vec<u8>),
}

impl Message {
    /// Tries to serialize the JSON as a [`Message::Text`].
    ///
    /// # Optional
    ///
    /// This requires the optional `json` feature enabled.
    ///
    /// # Errors
    ///
    /// Serialization can fail if `T`'s implementation of `Serialize` decides to
    /// fail, or if `T` contains a map with non-string keys.
    #[cfg(feature = "json")]
    #[cfg_attr(docsrs, doc(cfg(feature = "json")))]
    pub fn text_from_json<T: Serialize + ?Sized>(json: &T) -> Result<Self> {
        serde_json::to_string(json)
            .map(Message::Text)
            .map_err(Into::into)
    }

    /// Tries to serialize the JSON as a [`Message::Binary`].
    ///
    /// # Optional
    ///
    /// This requires the optional `json` feature enabled.
    ///
    /// # Errors
    ///
    /// Serialization can fail if `T`'s implementation of `Serialize` decides to
    /// fail, or if `T` contains a map with non-string keys.
    #[cfg(feature = "json")]
    #[cfg_attr(docsrs, doc(cfg(feature = "json")))]
    pub fn binary_from_json<T: Serialize + ?Sized>(json: &T) -> Result<Self> {
        serde_json::to_vec(json)
            .map(Message::Binary)
            .map_err(Into::into)
    }

    /// Tries to deserialize the message body as JSON.
    ///
    /// # Optional
    ///
    /// This requires the optional `json` feature enabled.
    ///
    /// # Errors
    ///
    /// Serialization can fail if `T`'s implementation of `Serialize` decides to
    /// fail, or if `T` contains a map with non-string keys.
    #[cfg(feature = "json")]
    #[cfg_attr(docsrs, doc(cfg(feature = "json")))]
    pub fn json<T: DeserializeOwned>(&self) -> Result<T> {
        match self {
            Self::Text(x) => serde_json::from_str(x),
            Self::Binary(x) => serde_json::from_slice(x),
        }
        .map_err(Into::into)
    }
}

/// Status code used to indicate why an endpoint is closing the `WebSocket`
/// connection.
///
/// Copied from `tungstenite`, since we also need this for the `Wasm`
/// backend[1].
///
/// [1]: https://docs.rs/tungstenite/latest/tungstenite/protocol/frame/coding/enum.CloseCode.html
#[non_exhaustive]
#[derive(Debug, Default, Eq, PartialEq, Clone, Copy)]
pub enum CloseCode {
    /// Indicates a normal closure, meaning that the purpose for
    /// which the connection was established has been fulfilled.
    #[default]
    Normal,
    /// Indicates that an endpoint is "going away", such as a server
    /// going down or a browser having navigated away from a page.
    Away,
    /// Indicates that an endpoint is terminating the connection due
    /// to a protocol error.
    Protocol,
    /// Indicates that an endpoint is terminating the connection
    /// because it has received a type of data it cannot accept (e.g., an
    /// endpoint that understands only text data MAY send this if it
    /// receives a binary message).
    Unsupported,
    /// Indicates that no status code was included in a closing frame. This
    /// close code makes it possible to use a single method, `on_close` to
    /// handle even cases where no close code was provided.
    Status,
    /// Indicates an abnormal closure. If the abnormal closure was due to an
    /// error, this close code will not be used. Instead, the `on_error` method
    /// of the handler will be called with the error. However, if the connection
    /// is simply dropped, without an error, this close code will be sent to the
    /// handler.
    Abnormal,
    /// Indicates that an endpoint is terminating the connection
    /// because it has received data within a message that was not
    /// consistent with the type of the message (e.g., non-UTF-8 \[RFC3629\]
    /// data within a text message).
    Invalid,
    /// Indicates that an endpoint is terminating the connection
    /// because it has received a message that violates its policy.  This
    /// is a generic status code that can be returned when there is no
    /// other more suitable status code (e.g., Unsupported or Size) or if there
    /// is a need to hide specific details about the policy.
    Policy,
    /// Indicates that an endpoint is terminating the connection
    /// because it has received a message that is too big for it to
    /// process.
    Size,
    /// Indicates that an endpoint (client) is terminating the
    /// connection because it has expected the server to negotiate one or
    /// more extension, but the server didn't return them in the response
    /// message of the `WebSocket` handshake.  The list of extensions that
    /// are needed should be given as the reason for closing.
    /// Note that this status code is not used by the server, because it
    /// can fail the `WebSocket` handshake instead.
    Extension,
    /// Indicates that a server is terminating the connection because
    /// it encountered an unexpected condition that prevented it from
    /// fulfilling the request.
    Error,
    /// Indicates that the server is restarting. A client may choose to
    /// reconnect, and if it does, it should use a randomized delay of 5-30
    /// seconds between attempts.
    Restart,
    /// Indicates that the server is overloaded and the client should either
    /// connect to a different IP (when multiple targets exist), or
    /// reconnect to the same IP when a user has performed an action.
    Again,
    #[doc(hidden)]
    Tls,
    #[doc(hidden)]
    Reserved(u16),
    #[doc(hidden)]
    Iana(u16),
    #[doc(hidden)]
    Library(u16),
    #[doc(hidden)]
    Bad(u16),
}

impl CloseCode {
    /// Check if this `CloseCode` is allowed.
    #[must_use]
    pub const fn is_allowed(self) -> bool {
        !matches!(
            self,
            Self::Bad(_) | Self::Reserved(_) | Self::Status | Self::Abnormal | Self::Tls
        )
    }
}

impl std::fmt::Display for CloseCode {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let code: u16 = (*self).into();
        write!(f, "{code}")
    }
}

impl From<CloseCode> for u16 {
    fn from(code: CloseCode) -> Self {
        match code {
            CloseCode::Normal => 1000,
            CloseCode::Away => 1001,
            CloseCode::Protocol => 1002,
            CloseCode::Unsupported => 1003,
            CloseCode::Status => 1005,
            CloseCode::Abnormal => 1006,
            CloseCode::Invalid => 1007,
            CloseCode::Policy => 1008,
            CloseCode::Size => 1009,
            CloseCode::Extension => 1010,
            CloseCode::Error => 1011,
            CloseCode::Restart => 1012,
            CloseCode::Again => 1013,
            CloseCode::Tls => 1015,
            CloseCode::Reserved(code)
            | CloseCode::Iana(code)
            | CloseCode::Library(code)
            | CloseCode::Bad(code) => code,
        }
    }
}

impl From<u16> for CloseCode {
    fn from(code: u16) -> Self {
        match code {
            1000 => Self::Normal,
            1001 => Self::Away,
            1002 => Self::Protocol,
            1003 => Self::Unsupported,
            1005 => Self::Status,
            1006 => Self::Abnormal,
            1007 => Self::Invalid,
            1008 => Self::Policy,
            1009 => Self::Size,
            1010 => Self::Extension,
            1011 => Self::Error,
            1012 => Self::Restart,
            1013 => Self::Again,
            1015 => Self::Tls,
            1016..=2999 => Self::Reserved(code),
            3000..=3999 => Self::Iana(code),
            4000..=4999 => Self::Library(code),
            _ => Self::Bad(code),
        }
    }
}

#[cfg(test)]
#[cfg(feature = "json")]
mod test {
    use serde::{
        Deserialize,
        Serialize,
    };

    use crate::{
        Message,
        Result,
    };

    #[derive(Default, Serialize, Deserialize)]
    struct Content {
        message: String,
    }

    #[test]
    pub fn text_json() -> Result<()> {
        let content = Content::default();
        let message = Message::text_from_json(&content)?;
        assert!(matches!(message, Message::Text(_)));
        let _: Content = message.json()?;

        Ok(())
    }

    #[test]
    pub fn binary_json() -> Result<()> {
        let content = Content::default();
        let message = Message::binary_from_json(&content)?;
        assert!(matches!(message, Message::Binary(_)));
        let _: Content = message.json()?;

        Ok(())
    }
}
