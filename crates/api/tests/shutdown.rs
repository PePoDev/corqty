use std::time::Duration;

use corqty_api::{config::ApiConfig, serve, state::AppState};
use corqty_persistence::store::PgStore;
use sqlx::PgPool;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

#[tokio::test]
async fn serve_returns_once_the_shutdown_token_is_cancelled() {
    // The health route never touches the database, so the lazy pool never connects.
    let pool = PgPool::connect_lazy("postgres://corqty_api@127.0.0.1:55432/corqty_test").unwrap();
    let state = AppState::new(PgStore::new(pool), ApiConfig::for_tests());
    let shutdown = state.shutdown.clone();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(serve(listener, state));

    let mut stream = TcpStream::connect(address).await.unwrap();
    stream
        .write_all(b"GET /api/v1/health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).await.unwrap();
    assert!(response.starts_with("HTTP/1.1 204"), "{response}");
    assert!(!server.is_finished());

    shutdown.cancel();
    tokio::time::timeout(Duration::from_secs(2), server)
        .await
        .expect("serve returns within 2 s")
        .unwrap()
        .unwrap();
}
