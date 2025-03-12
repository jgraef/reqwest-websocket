use futures::{channel::mpsc, StreamExt, TryStreamExt, SinkExt};
use leptos::{component, create_node_ref, html::Input, spawn_local, view, For, IntoView, RwSignal, SignalUpdate, SignalWith};

#[derive(Clone, Debug)]
enum Message {
    User(String),
    Server(String),
    Error(String),
}

#[component]
pub fn App() -> impl IntoView {
    let message_input = create_node_ref::<Input>();
    let (send_tx, mut send_rx) = mpsc::unbounded::<String>();

    let messages = RwSignal::new(vec![]);

    spawn_local(async move {
        let websocket = reqwest_websocket::websocket("https://echo.websocket.org/").await.unwrap();
        let (mut sender, mut receiver) = websocket.split();

        futures::join!(
            async move {
                loop {
                    match receiver.try_next().await {
                        Err(e) => {
                            messages.update(|messages| messages.push(Message::Error(e.to_string())));
                            break;
                        }
                        Ok(None) => {
                            break;
                        }
                        Ok(Some(reqwest_websocket::Message::Text(text))) => {
                            messages.update(|messages| messages.push(Message::Server(text)))
                        },
                        _ => {}
                    }
                }
            },
            async move {
                while let Some(message) = send_rx.next().await {
                    sender.send(message.into()).await.unwrap();
                }
            }
        );
    });

    let send_message = move || {
        let input = message_input.get().unwrap();
        let message = input.value();
        input.set_value("");

        messages.update(|messages| messages.push(Message::User(message.clone())));
        send_tx.unbounded_send(message).unwrap();
    };
    
    view! {
        <div class="d-flex flex-column" style="width: 100vw; height: 100vh;">
            <main class="main d-flex flex-column w-100 h-100 mw-100 mh-100">
                <div class="d-flex flex-column w-100 p-4 overflow-y-scroll">
                    <For
                        each=move || messages.with(|messages| messages.iter().map(ToOwned::to_owned).enumerate().collect::<Vec<_>>())
                        key=|(i, _)| *i
                        children=|(_, message)| {
                            match message {
                                Message::User(message) => {
                                    view! {
                                        <div class="card bg-light bg-gradient mb-3 bx-3 w-75 ms-auto">
                                            <div class="card-header">"User"</div>
                                            <div class="card-body">
                                                <p class="card-text">{message}</p>
                                            </div>
                                        </div>
                                    }
                                },
                                Message::Server(message) => {
                                    view! {
                                        <div class="card bg-light bg-gradient mb-3 bx-3 w-75 me-auto">
                                            <div class="card-header">"Server"</div>
                                            <div class="card-body">
                                                <p class="card-text">{message.to_string()}</p>
                                            </div>
                                        </div>
                                    }
                                },
                                Message::Error(message) => {
                                    view! {
                                        <div class="card bg-error bg-gradient mb-3 bx-3 w-75 mx-auto">
                                            <div class="card-header">"Error"</div>
                                            <div class="card-body">
                                                <p class="card-text">{message}</p>
                                            </div>
                                        </div>
                                    }
                                },
                            }
                        }
                    />
                </div>
                <div class="d-flex flex-row w-100 mt-auto">
                    <div class="input-group">
                        <form
                            class="d-flex flex-row w-100"
                            on:submit={
                                let send_message = send_message.clone();
                                move |e| {
                                    e.prevent_default();
                                    send_message();
                                }
                            }
                        >
                            <input
                                type="text"
                                class="form-control"
                                placeholder="Type your message here"
                                aria-label="Your message"
                                aria-describedby="button-send"
                                node_ref=message_input
                            />
                            <button
                                class="btn btn-outline-secondary"
                                type="button"
                                id="button-send"
                                on:click=move |_| send_message()
                            >
                                <i class="bi bi-send"></i>
                            </button>
                        </form>
                    </div>                  
                </div>
            </main>
        </div>
    }
}
