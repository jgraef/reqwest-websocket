use std::future::Future;

use futures_util::{SinkExt, StreamExt, TryStreamExt};
use http::HeaderValue;
use reqwest_websocket::{Error, Message, Upgrade};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Error> {
    let request_builder = reqwest::Client::builder()
        .http1_only()
        .build()?
        .get("wss://echo.websocket.org/");

    // wrap it in SigningRequestBuilder
    let signing_request_builder = SigningRequestBuilder(request_builder);

    // continue normally
    let websocket = signing_request_builder
        .upgrade()
        .send()
        .await?
        .into_websocket()
        .await?;

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

#[derive(Debug)]
pub struct SigningClient(reqwest::Client);

impl reqwest_websocket::Client for SigningClient {
    fn execute(
        &self,
        mut request: reqwest::Request,
    ) -> impl Future<Output = Result<reqwest::Response, Error>> + '_ {
        // we have to send the request, but can mess with it as we please :)

        let signature = mock_signing(&request);
        request
            .headers_mut()
            .insert("X-MySignature", HeaderValue::from_str(&signature).unwrap());

        // make sure to use the right `execute` method. It didn't work when I just called `self.0.execute` as that resolves to `reqwest::Client::execute` which doesn't perform the websocket handshake
        reqwest_websocket::Client::execute(&self.0, request)
    }
}

#[derive(Debug)]
pub struct SigningRequestBuilder(reqwest::RequestBuilder);

impl reqwest_websocket::RequestBuilder for SigningRequestBuilder {
    type Client = SigningClient;

    fn build_split(self) -> (Self::Client, Result<reqwest::Request, Error>) {
        let (client, request) = self.0.build_split();

        // the request here is before it is modified to be a websocket upgrade

        (SigningClient(client), request.map_err(Into::into))
    }
}

fn mock_signing(_request: &reqwest::Request) -> String {
    "d2hhdCBhcmUgeW91IGxvb2tpbmcgZm9yPyBUaGlzIGlzIGFuIGV4YW1wbGUgOkQK".to_string()
}
