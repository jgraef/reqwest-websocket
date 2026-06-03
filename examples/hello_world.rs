use futures_util::{SinkExt, StreamExt, TryStreamExt};
use reqwest_websocket::{Error, Message};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Error> {
    let websocket = reqwest_websocket::websocket("wss://echo.websocket.org/").await?;

    let (mut tx, mut rx) = websocket.split();

    futures_util::future::join(
        async move {
            for i in 1..11 {
                tx.send(format!("Hello, World! #{i}").into()).await.unwrap();
            }
        },
        async move {
            while let Some(message) = rx.try_next().await.unwrap() {
                if let Message::Text(text) = message {
                    println!("received: {text}");
                }
            }
        },
    )
    .await;

    Ok(())
}
