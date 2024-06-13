mod app;

use wasm_bindgen::JsCast;

use crate::app::App;

fn main() {
    tracing_wasm::set_as_global_default();
    console_error_panic_hook::set_once();

    tracing::info!("starting app");

    let root = gloo_utils::document()
        .get_element_by_id("root")
        .expect("no root node found")
        .dyn_into()
        .unwrap();

    leptos::mount_to(root, App);
}

