# `reqwest-websocket`

[![crates.io](https://img.shields.io/crates/v/reqwest-websocket.svg)](https://crates.io/crates/reqwest-websocket)
[![Documentation](https://docs.rs/reqwest-websocket/badge.svg)](https://docs.rs/reqwest-websocket)
[![MIT](https://img.shields.io/crates/l/reqwest-websocket.svg)](./LICENSE)

Extension for [`reqwest`][2] to allow [websocket][1] connections.

This crate contains the extension trait [`RequestBuilderExt`][4], which adds an
`upgrade` method to `reqwest::RequestBuilder` that prepares the HTTP request to
upgrade the connection to a WebSocket. After you call `upgrade()`, you can send
your upgraded request as usual with `send()`, which will return an
`UpgradeResponse`. The `UpgradeResponse` wraps `reqwest::Response` (and also
dereferences to it), so you can inspect the response if needed. Finally, you can
use `into_websocket()` on the response to turn it into an async stream and sink
for messages. Both text and binary messages are supported.

## Example

For a full example take a look at [`hello_world.rs`](examples/hello_world.rs).

```rust
// extends the reqwest::RequestBuilder to allow websocket upgrades
use reqwest_websocket::RequestBuilderExt;

// create a GET request, upgrade it and send it.
let response = Client::default()
    .get("wss://echo.websocket.org/")
    .upgrade() // <-- prepares the websocket upgrade.
    .send()
    .await?;

// turn the response into a websocket stream
let mut websocket = response.into_websocket().await?;

// the websocket implements `Sink<Message>`.
websocket.send(Message::Text("Hello, World".into())).await?;

// the websocket is also a `TryStream` over `Message`s.
while let Some(message) = websocket.try_next().await? {
    match message {
        Message::Text(text) => println!("{text}"),
        _ => {}
    }
}
```

## Support for WebAssembly

`reqwest-websocket` uses the HTTP upgrade functionality built into `reqwest`,
which is not available on WebAssembly. When you use `reqwest-websocket` in
WebAssembly, it falls back to using [`web_sys::WebSocket`][3]. This means that
everything except the URL (including query parameters) is not used for your
request.

[1]: https://en.wikipedia.org/wiki/WebSocket
[2]: https://docs.rs/reqwest/latest/reqwest/index.html
[3]: https://docs.rs/web-sys/latest/web_sys/struct.WebSocket.html
[4]: https://docs.rs/reqwest-websocket/0.1.0/reqwest_websocket/trait.RequestBuilderExt.html
