# Upcoming Version: 0.4.5

Document new features here. Document whether your changes are *breaking* semver-compatibility.

# 0.4.4

 - Added `UpgradedRequestBuilder::web_socket_config`
 - Fixed issues with `async-tungstenite` and `futures-util` [#32][github_32], [#33][github_33]

The current changes need a *patch* version bump.

# 0.4.3

 - Update `tungstenite` and `async-tungstenite` dependencies (#29)

# 0.4.2

- `UpgradedRequestBuilder::protocols` (patch)
- fix #24 (patch)

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

[github_32]: https://github.com/jgraef/reqwest-websocket/issues/32
[github_33]: https://github.com/jgraef/reqwest-websocket/pull/33
