use std::error::Error;

use futures_util::{
    SinkExt,
    StreamExt,
    TryStreamExt,
};
use reqwest::Client;
use reqwest_websocket::{
    Message,
    RequestBuilderExt,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let websocket = Client::default()
        .get("https://echo.websocket.org/")
        .upgrade()
        .send()
        .await?
        .into_websocket()
        .await?;

    let (mut tx, mut rx) = websocket.split();

    tokio::spawn(async move {
        for i in 1..11 {
            tx.send(Message::Text(format!("Hello, World! #{i}")))
                .await
                .unwrap();
        }
    });

    while let Some(message) = rx.try_next().await? {
        match message {
            Message::Text(text) => println!("received: {text}"),
            _ => {}
        }
    }

    Ok(())
}
