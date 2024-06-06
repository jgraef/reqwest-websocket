# 0.4.0

- close: (#9)
  - integrate `GitHub Action`s and `Dependabot`.
- close: (#12 and #13)
  - add `json` feature (disabled by default).
  - add `serde` and `serde_json` optional dependencies.
  - add `Message::*_from_json` and `Method::json` methods.
  - add `Error::Json(serde_json::Error)` variant.

# 0.3.1

- close: (#11)
  - add `protocol::CloseReason`.
  - add `WebSocket::close` method.
  - move `Message` to `protocol`, but re-exported to root.
