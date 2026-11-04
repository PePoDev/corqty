mod support;

use std::time::Duration;

use axum::{
    Router,
    body::{Body, BodyDataStream, to_bytes},
    http::{Request, StatusCode, header},
    response::Response,
};
use corqty_api::{config::ApiConfig, events, router::build_router, serve, state::AppState};
use corqty_persistence::store::PgStore;
use corqty_test_support::API_URL;
use futures_util::StreamExt;
use serde_json::{Value, json};
use serial_test::serial;
use sqlx::PgPool;
use support::AuthFixture;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};
use tower::ServiceExt;
use uuid::Uuid;

const INFRASTRUCTURE_BLUEPRINT_ID: &str = "00000000-0000-7000-8000-000000000101";

/// An event stream, read one frame at a time.
struct Frames {
    body: BodyDataStream,
    buffer: String,
}

impl Frames {
    fn new(response: Response) -> Self {
        assert_eq!(response.status(), StatusCode::OK);
        Self {
            body: response.into_body().into_data_stream(),
            buffer: String::new(),
        }
    }

    /// The next frame without its closing blank line, or None once the stream has ended.
    /// Fails unless one of them comes within 2 s.
    async fn next(&mut self) -> Option<String> {
        let frame = async {
            loop {
                if let Some(end) = self.buffer.find("\n\n") {
                    let frame = self.buffer[..end].to_owned();
                    self.buffer.drain(..end + 2);
                    return Some(frame);
                }
                let chunk = self.body.next().await?.unwrap();
                self.buffer.push_str(std::str::from_utf8(&chunk).unwrap());
            }
        };
        tokio::time::timeout(Duration::from_secs(2), frame)
            .await
            .expect("a frame or the end within 2 s")
    }

    /// The next frame, which must be an unnamed event, as its id and its JSON data.
    async fn event(&mut self) -> (i64, Value) {
        let frame = self.next().await.expect("an event");
        let (id, data) = frame.split_once('\n').unwrap();
        (
            id.strip_prefix("id: ").unwrap().parse().unwrap(),
            serde_json::from_str(data.strip_prefix("data: ").unwrap()).unwrap(),
        )
    }

    async fn events(&mut self, count: usize) -> Vec<(i64, Value)> {
        let mut events = Vec::new();
        for _ in 0..count {
            events.push(self.event().await);
        }
        events
    }
}

/// Asserts the status and returns the JSON body.
async fn body(response: Response, status: StatusCode) -> Value {
    assert_eq!(response.status(), status);
    let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    serde_json::from_slice(&body).unwrap()
}

/// Starts a run of a new workflow made from the simulated-infrastructure blueprint and
/// returns the run's URL and the run. Nothing executes it, so it stays queued with the single
/// event `run.state queued`.
async fn start_run(admin: &AuthFixture) -> (String, Value) {
    let org = format!("/api/v1/orgs/{}", admin.organization_id);
    let request = json!({ "name": "Provision", "blueprintId": INFRASTRUCTURE_BLUEPRINT_ID });
    let created = body(
        admin
            .post_json(&format!("{org}/workflows"), &request.to_string())
            .await,
        StatusCode::CREATED,
    )
    .await;
    let workflow = format!("{org}/workflows/{}", created["id"].as_str().unwrap());
    let published = admin
        .post_json(&format!("{workflow}/publish"), r#"{"revision":1}"#)
        .await;
    assert_eq!(published.status(), StatusCode::CREATED);
    let input = r#"{"input":{"region":"eu-west-1"}}"#;
    let run = body(
        admin.post_json(&format!("{workflow}/runs"), input).await,
        StatusCode::CREATED,
    )
    .await;
    (format!("{org}/runs/{}", run["id"].as_str().unwrap()), run)
}

/// Opens the event stream at `path` through `app` as `member`, with `Last-Event-ID` when
/// given, as the browser sends it when it reconnects.
async fn open(
    app: &Router,
    member: &AuthFixture,
    path: &str,
    last_event_id: Option<&str>,
) -> Response {
    let mut request = Request::get(path).header(header::COOKIE, member.cookie_header());
    if let Some(last_event_id) = last_event_id {
        request = request.header("last-event-id", last_event_id);
    }
    app.clone()
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap()
}

/// Appends a log line to the run's events through the owner pool, the way the worker
/// functions do, and returns the event as the stream sends it.
async fn append(admin: &AuthFixture, run: &Value, text: &str) -> (i64, Value) {
    let mut payload =
        json!({ "type": "node.log", "nodeId": "vpc", "stream": "stdout", "text": text });
    let run_id: Uuid = run["id"].as_str().unwrap().parse().unwrap();
    let seq: i64 = sqlx::query_scalar("SELECT corqty.append_run_event($1, $2)")
        .bind(run_id)
        .bind(&payload)
        .fetch_one(&admin.owner)
        .await
        .unwrap();
    payload["seq"] = json!(seq);
    (seq, payload)
}

fn queued() -> (i64, Value) {
    (
        1,
        json!({ "type": "run.state", "state": "queued", "seq": 1 }),
    )
}

/// The events that cancelling the queued run appends after seq `last`: each node canceled,
/// in plan order, then the run.
fn cancel_events(run: &Value, last: i64) -> Vec<(i64, Value)> {
    let mut seq = last;
    let mut events: Vec<(i64, Value)> = run["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|node| {
            seq += 1;
            let event = json!({
                "type": "node.state", "nodeId": node["nodeId"], "state": "canceled",
                "attempt": 0, "seq": seq,
            });
            (seq, event)
        })
        .collect();
    let event = json!({ "type": "run.state", "state": "canceled", "seq": seq + 1 });
    events.push((seq + 1, event));
    events
}

#[tokio::test]
#[serial]
async fn streams_replay_the_events_after_the_cursor() {
    let admin = AuthFixture::registered("admin@example.com", "Acme").await;
    let (path, run) = start_run(&admin).await;
    let events = format!("{path}/events");
    let second = append(&admin, &run, "first line").await;
    let third = append(&admin, &run, "second line").await;

    let response = open(&admin.app, &admin, &format!("{events}?after=1"), None).await;
    assert_eq!(response.status(), StatusCode::OK);
    let headers = response.headers();
    assert_eq!(headers[header::CONTENT_TYPE], "text/event-stream");
    // A single value, with `no-transform`, which keeps proxies such as Next.js from
    // compressing, and so buffering, the stream.
    let cache_control: Vec<_> = headers.get_all(header::CACHE_CONTROL).iter().collect();
    assert_eq!(cache_control, ["no-cache, no-transform"]);
    assert_eq!(headers["x-accel-buffering"], "no");
    let mut frames = Frames::new(response);
    assert_eq!(frames.next().await.as_deref(), Some("retry: 2000"));
    assert_eq!(frames.events(2).await, [second.clone(), third.clone()]);

    // The browser sends Last-Event-ID when it reconnects, and it wins over the ?after= of
    // the URL it reconnects to.
    let mut frames =
        Frames::new(open(&admin.app, &admin, &format!("{events}?after=1"), Some("2")).await);
    assert_eq!(frames.next().await.as_deref(), Some("retry: 2000"));
    assert_eq!(frames.event().await, third);

    // Without a cursor the stream starts at the first event.
    let mut frames = Frames::new(open(&admin.app, &admin, &events, None).await);
    assert_eq!(frames.next().await.as_deref(), Some("retry: 2000"));
    assert_eq!(frames.events(3).await, [queued(), second, third]);
}

#[tokio::test]
#[serial]
async fn open_streams_follow_the_run_live_until_it_finishes() {
    let admin = AuthFixture::registered("admin@example.com", "Acme").await;
    let (path, run) = start_run(&admin).await;
    // An API process of its own, whose listener wakes its streams.
    let pool = PgPool::connect(API_URL).await.unwrap();
    let state = AppState::new(PgStore::new(pool.clone()), ApiConfig::for_tests());
    let listener = events::spawn_listener(
        API_URL.to_owned(),
        state.wake.clone(),
        state.shutdown.clone(),
    );
    let app = build_router(state.clone());
    let mut frames = Frames::new(open(&app, &admin, &format!("{path}/events"), None).await);
    assert_eq!(frames.next().await.as_deref(), Some("retry: 2000"));
    assert_eq!(frames.event().await, queued());

    // Committed after the replay: the notification wakes the stream.
    let live = append(&admin, &run, "live").await;
    assert_eq!(frames.event().await, live);

    // The listener's connection drops. The listener reconnects and wakes every stream, so
    // an event committed in the meantime still arrives.
    let terminated: Vec<bool> = sqlx::query_scalar(
        "SELECT pg_terminate_backend(pid) FROM pg_stat_activity
         WHERE usename = current_user AND query LIKE 'LISTEN %'",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(terminated, [true]);
    let after_reconnect = append(&admin, &run, "after the reconnect").await;
    assert_eq!(frames.event().await, after_reconnect);

    // The cancel finishes the run, and the stream ends after the run's terminal event.
    let canceled = admin.post(&format!("{path}/cancel")).await;
    assert_eq!(canceled.status(), StatusCode::OK);
    assert_eq!(frames.events(4).await, cancel_events(&run, 3));
    assert_eq!(frames.next().await, None);

    state.shutdown.cancel();
    tokio::time::timeout(Duration::from_secs(2), listener)
        .await
        .expect("the listener stops within 2 s")
        .unwrap();
}

#[tokio::test]
#[serial]
async fn finished_runs_end_their_streams_and_answer_204_once_replayed() {
    let admin = AuthFixture::registered("admin@example.com", "Acme").await;
    let (path, run) = start_run(&admin).await;
    let canceled = admin.post(&format!("{path}/cancel")).await;
    assert_eq!(canceled.status(), StatusCode::OK);
    let events = format!("{path}/events");

    // A replay that reaches the terminal event of a finished run ends there.
    let mut frames =
        Frames::new(open(&admin.app, &admin, &format!("{events}?after=3"), None).await);
    assert_eq!(frames.next().await.as_deref(), Some("retry: 2000"));
    assert_eq!(frames.events(2).await, cancel_events(&run, 1)[2..]);
    assert_eq!(frames.next().await, None);

    // A client that already has every event gets 204, which stops the browser from
    // reconnecting.
    for (query, last_event_id) in [("?after=5", None), ("?after=0", Some("5"))] {
        let response = open(
            &admin.app,
            &admin,
            &format!("{events}{query}"),
            last_event_id,
        )
        .await;
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
    }

    // To another organization's member the run does not exist, under either organization.
    let outsider = admin
        .register_another("outsider@example.com", "Outside")
        .await;
    let elsewhere = format!(
        "/api/v1/orgs/{}/runs/{}/events",
        outsider.organization_id,
        run["id"].as_str().unwrap()
    );
    for path in [&events, &elsewhere] {
        let response = open(&admin.app, &outsider, path, None).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
    }
}

#[tokio::test]
#[serial]
async fn each_user_keeps_at_most_8_streams_open() {
    let admin = AuthFixture::registered("admin@example.com", "Acme").await;
    let (path, _) = start_run(&admin).await;
    let events = format!("{path}/events");
    let mut streams = Vec::new();
    for _ in 0..8 {
        streams.push(Frames::new(open(&admin.app, &admin, &events, None).await));
    }
    let refused = body(
        open(&admin.app, &admin, &events, None).await,
        StatusCode::TOO_MANY_REQUESTS,
    )
    .await;
    assert_eq!(refused["code"], "too_many_streams");

    // A stream that ends gives its slot back.
    drop(streams.remove(0));
    let mut frames = Frames::new(open(&admin.app, &admin, &events, None).await);
    assert_eq!(frames.next().await.as_deref(), Some("retry: 2000"));
}

#[tokio::test]
#[serial]
async fn the_server_stops_on_shutdown_although_a_stream_is_open() {
    let admin = AuthFixture::registered("admin@example.com", "Acme").await;
    let (path, _) = start_run(&admin).await;
    let pool = PgPool::connect(API_URL).await.unwrap();
    let state = AppState::new(PgStore::new(pool), ApiConfig::for_tests());
    let shutdown = state.shutdown.clone();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(serve(listener, state));

    let mut stream = TcpStream::connect(address).await.unwrap();
    let request = format!(
        "GET {path}/events HTTP/1.1\r\nHost: localhost\r\nCookie: {}\r\n\r\n",
        admin.cookie_header()
    );
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut received = Vec::new();
    tokio::time::timeout(Duration::from_secs(2), async {
        while !String::from_utf8_lossy(&received).contains(r#""run.state""#) {
            let mut chunk = [0; 4096];
            let read = stream.read(&mut chunk).await.unwrap();
            assert_ne!(read, 0, "the stream ended early");
            received.extend_from_slice(&chunk[..read]);
        }
    })
    .await
    .expect("the first event within 2 s");

    // axum's graceful shutdown waits for open responses, so the stream must end on its own.
    shutdown.cancel();
    tokio::time::timeout(Duration::from_secs(2), server)
        .await
        .expect("serve returns within 2 s")
        .unwrap()
        .unwrap();
    // The response ended cleanly, with the last chunk of its chunked body.
    stream.read_to_end(&mut received).await.unwrap();
    assert!(String::from_utf8_lossy(&received).ends_with("0\r\n\r\n"));
}
