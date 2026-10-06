//! Regressão da fita, sem rede e sem IA: uma API falsa local conta as chamadas e confere a chave.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::post;
use axum::Router;
use pl_agent_harness::cassette::{Cassette, Mode, Options};
use serde_json::{json, Value};

const KEY: &str = "secret-test-key";

struct Upstream {
    url: String,
    hits: Arc<AtomicUsize>,
}

/// Responde `resposta <n>` (contando as chamadas) como SSE; 429 quando o corpo pede `"fail": true`.
async fn upstream() -> Upstream {
    let hits = Arc::new(AtomicUsize::new(0));
    let app = Router::new()
        .route(
            "/v1/messages",
            post(|State(h): State<Arc<AtomicUsize>>, headers: HeaderMap, body: String| async move {
                assert_eq!(headers.get("x-api-key").and_then(|v| v.to_str().ok()), Some(KEY));
                let n = h.fetch_add(1, Ordering::SeqCst) + 1;
                let v: Value = serde_json::from_str(&body).unwrap();
                if v["fail"] == true {
                    return (StatusCode::TOO_MANY_REQUESTS, [("content-type", "application/json")], "{}".to_string());
                }
                (StatusCode::OK, [("content-type", "text/event-stream")], format!("data: resposta {n}\n\n"))
            }),
        )
        .with_state(hits.clone());
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    Upstream { url, hits }
}

fn opts(mode: Mode, up: &Upstream, key: Option<&str>) -> Options {
    Options { mode, strict: false, upstream: up.url.clone(), api_key: key.map(str::to_string) }
}

async fn post_json(c: &Cassette, channel: &str, body: Value) -> (u16, String) {
    let r = reqwest::Client::new()
        .post(format!("{}/v1/messages", c.base_url(channel)))
        .header("x-api-key", "whatever")
        .json(&body)
        .send()
        .await
        .unwrap();
    (r.status().as_u16(), r.text().await.unwrap())
}

#[tokio::test]
async fn record_then_replay_without_network_or_key() {
    let up = upstream().await;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.json");

    let c = Cassette::open_at(&path, opts(Mode::Record, &up, Some(KEY))).await.unwrap();
    assert_eq!(post_json(&c, "a", json!({ "m": 1, "metadata": { "user_id": "x" } })).await.1, "data: resposta 1\n\n");
    assert_eq!(post_json(&c, "a", json!({ "m": 2 })).await.1, "data: resposta 2\n\n");
    assert!(c.finish().await.unwrap().is_empty());
    assert_eq!(up.hits.load(Ordering::SeqCst), 2);
    assert!(!std::fs::read_to_string(&path).unwrap().contains(KEY), "a chave não pode ir para a fita");

    // Replay: sem chave, e a API nem é chamada. O metadata diferente não conta como divergência.
    let c = Cassette::open_at(&path, opts(Mode::Replay, &up, None)).await.unwrap();
    assert_eq!(post_json(&c, "a", json!({ "m": 1, "metadata": { "user_id": "y" } })).await.1, "data: resposta 1\n\n");
    assert_eq!(post_json(&c, "a", json!({ "m": 2 })).await.1, "data: resposta 2\n\n");
    assert!(c.finish().await.unwrap().is_empty());
    assert_eq!(up.hits.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn channels_keep_their_own_order() {
    let up = upstream().await;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.json");
    let c = Cassette::open_at(&path, opts(Mode::Record, &up, Some(KEY))).await.unwrap();
    post_json(&c, "alpha", json!({ "q": "a1" })).await;
    post_json(&c, "beta", json!({ "q": "b1" })).await;
    post_json(&c, "alpha", json!({ "q": "a2" })).await;
    c.finish().await.unwrap();

    // Ordem trocada entre canais no replay: cada canal segue a própria sequência.
    let c = Cassette::open_at(&path, opts(Mode::Replay, &up, None)).await.unwrap();
    assert_eq!(post_json(&c, "beta", json!({ "q": "b1" })).await.1, "data: resposta 2\n\n");
    assert_eq!(post_json(&c, "alpha", json!({ "q": "a1" })).await.1, "data: resposta 1\n\n");
    assert_eq!(post_json(&c, "alpha", json!({ "q": "a2" })).await.1, "data: resposta 3\n\n");
    assert!(c.finish().await.unwrap().is_empty());
}

#[tokio::test]
async fn divergence_is_reported_and_strict_fails() {
    let up = upstream().await;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.json");
    let c = Cassette::open_at(&path, opts(Mode::Record, &up, Some(KEY))).await.unwrap();
    post_json(&c, "a", json!({ "messages": [{ "content": "pid 10" }] })).await;
    c.finish().await.unwrap();

    let c = Cassette::open_at(&path, opts(Mode::Replay, &up, None)).await.unwrap();
    let (status, _) = post_json(&c, "a", json!({ "messages": [{ "content": "pid 11" }] })).await;
    assert_eq!(status, 200);
    let d = c.finish().await.unwrap();
    assert_eq!(d.len(), 1, "{d:?}");
    assert!(d[0].contains("$.messages[0].content") && d[0].contains("pid 10") && d[0].contains("pid 11"), "{d:?}");

    let mut o = opts(Mode::Replay, &up, None);
    o.strict = true;
    let c = Cassette::open_at(&path, o).await.unwrap();
    let (status, body) = post_json(&c, "a", json!({ "messages": [{ "content": "pid 12" }] })).await;
    assert_eq!(status, 502, "{body}");
    assert!(body.contains("divergiu"), "{body}");
}

#[tokio::test]
async fn replay_past_the_end_fails_and_auto_records_the_rest() {
    let up = upstream().await;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.json");
    let c = Cassette::open_at(&path, opts(Mode::Record, &up, Some(KEY))).await.unwrap();
    post_json(&c, "a", json!({ "n": 1 })).await;
    c.finish().await.unwrap();

    let c = Cassette::open_at(&path, opts(Mode::Replay, &up, None)).await.unwrap();
    post_json(&c, "a", json!({ "n": 1 })).await;
    let (status, body) = post_json(&c, "a", json!({ "n": 2 })).await;
    assert_eq!(status, 502);
    assert!(body.contains("a fita acabou"), "{body}");
    c.finish().await.unwrap();

    // Auto: reproduz a primeira, grava a segunda, e a fita final tem as duas.
    let c = Cassette::open_at(&path, opts(Mode::Auto, &up, Some(KEY))).await.unwrap();
    assert_eq!(post_json(&c, "a", json!({ "n": 1 })).await.1, "data: resposta 1\n\n");
    assert_eq!(post_json(&c, "a", json!({ "n": 2 })).await.1, "data: resposta 2\n\n");
    c.finish().await.unwrap();
    let c = Cassette::open_at(&path, opts(Mode::Replay, &up, None)).await.unwrap();
    assert_eq!(post_json(&c, "a", json!({ "n": 1 })).await.1, "data: resposta 1\n\n");
    assert_eq!(post_json(&c, "a", json!({ "n": 2 })).await.1, "data: resposta 2\n\n");
    assert!(c.finish().await.unwrap().is_empty());
    assert_eq!(up.hits.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn transient_errors_never_enter_the_tape() {
    let up = upstream().await;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.json");
    let c = Cassette::open_at(&path, opts(Mode::Record, &up, Some(KEY))).await.unwrap();
    assert_eq!(post_json(&c, "a", json!({ "fail": true })).await.0, 429);
    assert_eq!(post_json(&c, "a", json!({ "n": 1 })).await.1, "data: resposta 2\n\n");
    c.finish().await.unwrap();
    let c = Cassette::open_at(&path, opts(Mode::Replay, &up, None)).await.unwrap();
    assert_eq!(post_json(&c, "a", json!({ "n": 1 })).await.1, "data: resposta 2\n\n");
    assert!(c.finish().await.unwrap().is_empty());
}

#[tokio::test]
async fn replay_without_tape_is_an_error() {
    let up = upstream().await;
    let dir = tempfile::tempdir().unwrap();
    let r = Cassette::open_at(&dir.path().join("none.json"), opts(Mode::Replay, &up, None)).await;
    assert!(r.is_err());
}
