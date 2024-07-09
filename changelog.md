# Upcoming Version: 0.4.2

Document new features here. Document whether your changes are *breaking* semver-compatibility.

The current changes need a *patch* version bump.

# 0.4.1

- made `WebSocket` and `WebSysWebSocket` `Debug` (patch)
- bugfix #16 #18 (patch)

# 0.4.0

- marked `Error` and `CloseCode` non-exhaustive. *breaking*
- added `Ping`, `Pong`, `Close` variants to `Message`

- json: (#12 and #13)
  - add `json` feature (disabled by default).
  - add `serde` and `serde_json` optional dependencies.
  - add `Message::*_from_json` and `Method::json` methods.
  - add `Error::Json(serde_json::Error)` variant. *breaking*
   - this would normally break semver-compatibility, but it's behind a feature flag that wasn't available before. 

- close: (#11)
  - add `protocol::CloseReason`.
  - add `WebSocket::close` method.
  - move `Message` to `protocol`, but re-exported to root.

# 0.3.0

Start of changelog
