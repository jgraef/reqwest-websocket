# `reqwest-websocket`

[![crates.io](https://img.shields.io/crates/v/reqwest-websocket.svg)](https://crates.io/crates/reqwest-websocket)
[![Documentation](https://docs.rs/reqwest-websocket/badge.svg)](https://docs.rs/reqwest-websocket)
[![MIT](https://img.shields.io/crates/l/reqwest-websocket.svg)](./LICENSE)

Provides wrappers for [`reqwest`][2] to enable [websocket][1] connections.

## Example

For a full example take a look at [`hello_world.rs`](examples/hello_world.rs).

 ```rust
// Extends the reqwest::RequestBuilder to allow websocket upgrades
use reqwest_websocket::RequestBuilderExt;

// don't use `ws://` or `wss://` for the url, but rather `http://` or `https://`
let response = Client::default()
    .get("https://echo.websocket.org/")
    .upgrade() // prepares the websocket upgrade.
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

[1]: https://en.wikipedia.org/wiki/WebSocket
[2]: https://docs.rs/reqwest/latest/reqwest/index.html
