use crate::{Error, Message};
use serde::{de::DeserializeOwned, Serialize};

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
    #[cfg_attr(docsrs, doc(cfg(feature = "json")))]
    pub fn text_from_json<T: Serialize + ?Sized>(json: &T) -> Result<Self, Error> {
        serde_json::to_string(json)
            .map(Message::Text)
            .map_err(Into::into)
    }

    /// Tries to serialize the JSON as a [`Message::Binary`].
    ///
    /// # Optional
    ///
    /// This requires that the optional `json` feature is enabled.
    ///
    /// # Errors
    ///
    /// Serialization can fail if `T`'s implementation of `Serialize` decides to
    /// fail, or if `T` contains a map with non-string keys.
    #[cfg_attr(docsrs, doc(cfg(feature = "json")))]
    pub fn binary_from_json<T: Serialize + ?Sized>(json: &T) -> Result<Self, Error> {
        serde_json::to_vec(json)
            .map(Message::Binary)
            .map_err(Into::into)
    }

    /// Tries to deserialize the message body as JSON.
    ///
    /// # Optional
    ///
    /// This requires that the optional `json` feature is enabled.
    ///
    /// # Errors
    ///
    /// This method fails whenever the response body is not in `JSON` format,
    /// or it cannot be properly deserialized to target type `T`.
    ///
    /// For more details please see [`serde_json::from_str`] and
    /// [`serde_json::from_slice`].
    #[cfg_attr(docsrs, doc(cfg(feature = "json")))]
    pub fn json<T: DeserializeOwned>(&self) -> Result<T, Error> {
        use serde::de::Error as _;
        match self {
            Self::Text(x) => serde_json::from_str(x).map_err(Into::into),
            Self::Binary(x) => serde_json::from_slice(x).map_err(Into::into),
            Self::Ping(_) | Self::Pong(_) | Self::Close { .. } => {
                Err(serde_json::Error::custom("neither text nor binary").into())
            }
        }
    }
}

#[cfg(test)]
mod test {
    use serde::{Deserialize, Serialize};

    use crate::{Error, Message};

    #[derive(Default, Serialize, Deserialize)]
    struct Content {
        message: String,
    }

    #[test]
    pub fn text_json() -> Result<(), Error> {
        let content = Content::default();
        let message = Message::text_from_json(&content)?;
        assert!(matches!(message, Message::Text(_)));
        let _: Content = message.json()?;

        Ok(())
    }

    #[test]
    pub fn binary_json() -> Result<(), Error> {
        let content = Content::default();
        let message = Message::binary_from_json(&content)?;
        assert!(matches!(message, Message::Binary(_)));
        let _: Content = message.json()?;

        Ok(())
    }
}
