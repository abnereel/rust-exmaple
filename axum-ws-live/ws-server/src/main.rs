use std::net::SocketAddr;

use axum::{routing::get, Extension, Router, Server};
use ws_server::{ws_handler, ChatState};

#[tokio::main]
async fn main() {
    let addr = SocketAddr::from(([127, 0, 0, 1], 8000));
    let app = Router::new().route("/", get(ws_handler).layer(Extension(ChatState::default())));
    println!("Listening on {:?}", addr);
    Server::bind(&addr)
        .serve(app.into_make_service())
        .await
        .unwrap();
}
