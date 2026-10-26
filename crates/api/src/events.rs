//! Run events as server-sent events. A stream replays the run's events after the client's
//! cursor, then follows the run live: one listener per process turns the database's
//! `corqty_run_events` notifications into wake-ups, and a wake-up for a run makes that run's
//! streams read again. The database stays the only source of the events.

use std::{
    collections::{HashMap, VecDeque},
    convert::Infallible,
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};

use axum::{
    Extension,
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::{
        IntoResponse, Response,
        sse::{Event, KeepAlive, Sse},
    },
};
use corqty_domain::rbac::Capability;
use corqty_persistence::{models::SessionRecord, runs, store::PgStore};
use futures_util::StreamExt;
use serde::Deserialize;
use serde_json::Value;
use sqlx::postgres::PgListener;
use tokio::{
    sync::{
        OwnedSemaphorePermit,
        broadcast::{self, error::RecvError},
    },
    task::JoinHandle,
    time::Instant,
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::{authz::authorize, error::ApiError, extract::ApiPath, state::AppState};

/// The channel that `corqty.notify_run_event()` notifies with the run id.
const CHANNEL: &str = "corqty_run_events";
/// Events read per query.
const PAGE: i64 = 500;
/// How long the browser waits before it reconnects.
const RETRY: Duration = Duration::from_secs(2);
const KEEP_ALIVE: Duration = Duration::from_secs(15);
/// A stream reads again this long after its last read even without a wake-up, in case a
/// notification was lost.
const SAFETY_READ: Duration = Duration::from_secs(30);
/// How long a stream stays open. The browser then reconnects from its last event.
const MAX_STREAM: Duration = Duration::from_secs(600);
const MIN_BACKOFF: Duration = Duration::from_millis(250);
const MAX_BACKOFF: Duration = Duration::from_secs(10);
/// How many streams one API process keeps open, and how many of them one user may have. Every
/// open stream reads the database when its run changes, so the streams must not be able to
/// take every pooled connection.
pub const MAX_STREAMS: usize = 256;
const MAX_STREAMS_PER_USER: usize = 8;

/// What the listener tells the open streams.
#[derive(Clone, Copy, Debug)]
pub enum Wake {
    /// The run has new events.
    Run(Uuid),
    /// Notifications may have been missed, so every stream reads again.
    Resync,
}

#[derive(Deserialize)]
pub struct EventsQuery {
    after: Option<i64>,
}

/// Streams the run's events after the cursor: the `Last-Event-ID` header that the browser
/// sends when it reconnects, else `?after=`, else 0. Each event is unnamed, with its seq as
/// the id and its payload as the data. A finished run whose events the client already has
/// answers 204, which stops the browser from reconnecting. A stream past the process's or the
/// user's limit answers 429.
pub async fn stream_events(
    State(state): State<AppState>,
    Extension(session): Extension<SessionRecord>,
    ApiPath((organization_id, run_id)): ApiPath<(Uuid, Uuid)>,
    Query(query): Query<EventsQuery>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let cursor = headers
        .get("last-event-id")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse().ok())
        .or(query.after)
        .unwrap_or(0);
    let mut access = authorize(
        &state,
        &session,
        organization_id,
        Capability::ViewOrganization,
    )
    .await?;
    let run = runs::get_run(&mut access.tx, run_id)
        .await?
        .ok_or(ApiError::NotFound)?;
    access.commit().await?;
    if is_finished(&run.summary.status) && cursor >= run.last_event_seq {
        return Ok(StatusCode::NO_CONTENT.into_response());
    }
    let slot =
        Slot::take(&state, session.user_id).ok_or(ApiError::TooManyRequests("too_many_streams"))?;
    // Subscribed before the first read, so an event committed after that read wakes the
    // stream.
    let follower = Follower {
        store: state.store.clone(),
        user_id: session.user_id,
        organization_id,
        run_id,
        last_seq: cursor,
        wake: state.wake.subscribe(),
        ready: VecDeque::from([Event::default().retry(RETRY)]),
        stale: true,
        read_again_at: Instant::now(),
        finished: false,
        _slot: slot,
    };
    // axum's graceful shutdown waits for open responses, so every stream ends on the token.
    let shutdown = state.shutdown.clone();
    let end = async move {
        tokio::select! {
            () = shutdown.cancelled() => {}
            () = tokio::time::sleep(MAX_STREAM) => {}
        }
    };
    let stream = futures_util::stream::unfold(follower, Follower::advance).take_until(end);
    Ok((
        // Replaces Sse's `no-cache`. `no-transform` keeps proxies such as Next.js from
        // compressing, and so buffering, the stream; `x-accel-buffering` does it for nginx.
        [
            ("cache-control", "no-cache, no-transform"),
            ("x-accel-buffering", "no"),
        ],
        Sse::new(stream).keep_alive(KeepAlive::new().interval(KEEP_ALIVE)),
    )
        .into_response())
}

/// Listens for `corqty_run_events` on a connection of its own and wakes the streams of each
/// notified run. After every (re)connect it wakes all streams, because notifications sent
/// while it was not listening are lost. It retries a failed connect with backoff and stops
/// on `shutdown`.
pub fn spawn_listener(
    database_url: String,
    wake: broadcast::Sender<Wake>,
    shutdown: CancellationToken,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut backoff = MIN_BACKOFF;
        loop {
            let connected = tokio::select! {
                () = shutdown.cancelled() => return,
                connected = listen(&database_url) => connected,
            };
            let mut listener = match connected {
                Ok(listener) => listener,
                Err(error) => {
                    tracing::warn!(error = %error, "the run event listener could not connect");
                    tokio::select! {
                        () = shutdown.cancelled() => return,
                        () = tokio::time::sleep(backoff) => {}
                    }
                    backoff = (backoff * 2).min(MAX_BACKOFF);
                    continue;
                }
            };
            backoff = MIN_BACKOFF;
            // A send fails only while no stream is open.
            let _ = wake.send(Wake::Resync);
            loop {
                let received = tokio::select! {
                    () = shutdown.cancelled() => return,
                    received = listener.try_recv() => received,
                };
                match received {
                    Ok(Some(notification)) => {
                        if let Ok(run_id) = notification.payload().parse() {
                            let _ = wake.send(Wake::Run(run_id));
                        }
                    }
                    // The connection dropped, and the listener has already reconnected.
                    Ok(None) => {
                        let _ = wake.send(Wake::Resync);
                    }
                    Err(error) => {
                        tracing::warn!(error = %error, "the run event listener lost its connection");
                        break;
                    }
                }
            }
        }
    })
}

async fn listen(database_url: &str) -> Result<PgListener, sqlx::Error> {
    let mut listener = PgListener::connect(database_url).await?;
    listener.listen(CHANNEL).await?;
    Ok(listener)
}

/// An open stream's place among the streams that the process and its user may have open,
/// given back when the stream ends.
struct Slot {
    _permit: OwnedSemaphorePermit,
    per_user: Arc<Mutex<HashMap<Uuid, usize>>>,
    user_id: Uuid,
}

impl Slot {
    /// A slot for another stream of the user, or None when the process or the user has as many
    /// open as it may.
    fn take(state: &AppState, user_id: Uuid) -> Option<Self> {
        let permit = Arc::clone(&state.streams).try_acquire_owned().ok()?;
        let mut per_user = state
            .streams_per_user
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let open = per_user.entry(user_id).or_default();
        if *open >= MAX_STREAMS_PER_USER {
            return None;
        }
        *open += 1;
        Some(Self {
            _permit: permit,
            per_user: Arc::clone(&state.streams_per_user),
            user_id,
        })
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        let mut per_user = self.per_user.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(open) = per_user.get_mut(&self.user_id) {
            *open -= 1;
            if *open == 0 {
                per_user.remove(&self.user_id);
            }
        }
    }
}

/// One stream's place in its run's events.
// ponytail: the session is checked when the stream opens, so a stream outlives a logout by
// at most MAX_STREAM. Every read still checks the membership, through RLS.
struct Follower {
    store: PgStore,
    user_id: Uuid,
    organization_id: Uuid,
    run_id: Uuid,
    last_seq: i64,
    wake: broadcast::Receiver<Wake>,
    /// Frames read but not sent yet.
    ready: VecDeque<Event>,
    /// Whether the run may have events that the stream has not read.
    stale: bool,
    read_again_at: Instant,
    /// Whether the run's terminal `run.state` has been read.
    finished: bool,
    /// Held while the stream lives.
    _slot: Slot,
}

impl Follower {
    /// The next frame, reading and waiting as needed, or None once the stream is over.
    async fn advance(mut self) -> Option<(Result<Event, Infallible>, Self)> {
        loop {
            if let Some(frame) = self.ready.pop_front() {
                return Some((Ok(frame), self));
            }
            if self.finished {
                return None;
            }
            if self.stale {
                if let Err(error) = self.read().await {
                    // The browser reconnects from the last event it received.
                    tracing::warn!(error = %error, "reading run events failed");
                    return None;
                }
                continue;
            }
            tokio::select! {
                () = tokio::time::sleep_until(self.read_again_at) => self.stale = true,
                wake = self.wake.recv() => match wake {
                    Ok(Wake::Run(run_id)) => self.stale = run_id == self.run_id,
                    Ok(Wake::Resync) | Err(RecvError::Lagged(_)) => self.stale = true,
                    Err(RecvError::Closed) => return None,
                },
            }
        }
    }

    /// Reads the next page of events in a short tenant transaction, so an open stream holds
    /// no connection. After a full page the stream stays stale and reads again at once.
    async fn read(&mut self) -> Result<(), sqlx::Error> {
        let mut tx = self
            .store
            .begin_tenant(self.user_id, self.organization_id)
            .await?;
        let events = runs::events_after(&mut tx, self.run_id, self.last_seq, PAGE).await?;
        tx.commit().await?;
        self.stale = events.len() == PAGE as usize;
        self.read_again_at = Instant::now() + SAFETY_READ;
        for event in events {
            self.finished |= ends_run(&event.payload);
            self.last_seq = event.seq;
            self.ready.push_back(
                Event::default()
                    .id(event.seq.to_string())
                    .data(event.payload.to_string()),
            );
        }
        Ok(())
    }
}

/// Whether a run in this status can change no more.
fn is_finished(status: &str) -> bool {
    !matches!(status, "queued" | "running")
}

/// Whether the event is the run's terminal `run.state`, its last event.
fn ends_run(payload: &Value) -> bool {
    payload["type"] == "run.state" && payload["state"].as_str().is_some_and(is_finished)
}
