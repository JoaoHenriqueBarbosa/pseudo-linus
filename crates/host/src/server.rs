//! HTTP do daemon.
//!
//! | rota | o quê |
//! |---|---|
//! | `GET /healthz` | sem autenticação; 200 se algum worker está pronto, 503 se nenhum |
//! | `POST /rpc` | JSON-RPC 2.0 (requisição única ou lote). `exec.stream` e `session.exec.stream` respondem em NDJSON: uma notificação `exec.output` por pedaço de saída e a resposta na última linha |
//! | `GET /ws` | WebSocket com JSON-RPC multiplexado: várias requisições ao mesmo tempo, notificações de streaming no meio |
//!
//! Autenticação: `Authorization: Bearer plk_...` em `/rpc` e no upgrade do `/ws`. Credencial ruim é
//! HTTP 401 com o erro JSON-RPC no corpo; um IP com muitas falhas seguidas leva 429 por um minuto.
//! Cliente que desconecta no meio de um `exec` cancela a execução (o worker mata o processo).

use std::collections::HashMap;
use std::convert::Infallible;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, Instant};

use bytes::Bytes;
use futures_util::{SinkExt, StreamExt};
use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt, Full, Limited, StreamBody};
use hyper::body::{Frame, Incoming};
use hyper::header::{self, HeaderMap, HeaderValue};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use parking_lot::Mutex;
use serde_json::{Value, json};
use tokio::net::TcpListener;
use tokio::sync::{Semaphore, mpsc, watch};
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::protocol::{Role, WebSocketConfig};

use crate::auth::Principal;
use crate::methods::{Notifier, STREAMING_METHODS};
use crate::rpc::{self, RpcError, codes};
use crate::supervisor::{SlotState, Supervisor};
use crate::timeutil::now_unix;

type Body = BoxBody<Bytes, std::io::Error>;

const AUTH_FAIL_WINDOW: Duration = Duration::from_secs(60);
const AUTH_FAIL_MAX: u32 = 20;

fn full(b: impl Into<Bytes>) -> Body {
    Full::new(b.into()).map_err(|never| match never {}).boxed()
}

fn json_response(status: StatusCode, v: &Value) -> Response<Body> {
    let mut r = Response::new(full(serde_json::to_vec(v).expect("JSON sempre serializa")));
    *r.status_mut() = status;
    r.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static("application/json"));
    r
}

fn rpc_error_response(status: StatusCode, e: &RpcError) -> Response<Body> {
    json_response(status, &rpc::failure(Value::Null, e))
}

/// Falhas de autenticação por IP, pra frear tentativa e erro.
#[derive(Default)]
struct AuthLimiter {
    map: Mutex<HashMap<IpAddr, (u32, Instant)>>,
}

impl AuthLimiter {
    fn blocked(&self, ip: IpAddr) -> bool {
        let mut m = self.map.lock();
        match m.get(&ip) {
            Some((n, since)) if since.elapsed() < AUTH_FAIL_WINDOW => *n >= AUTH_FAIL_MAX,
            Some(_) => {
                m.remove(&ip);
                false
            }
            None => false,
        }
    }

    fn fail(&self, ip: IpAddr) {
        let mut m = self.map.lock();
        if m.len() > 100_000 {
            m.retain(|_, (_, since)| since.elapsed() < AUTH_FAIL_WINDOW);
        }
        let e = m.entry(ip).or_insert((0, Instant::now()));
        if e.1.elapsed() >= AUTH_FAIL_WINDOW {
            *e = (0, Instant::now());
        }
        e.0 += 1;
    }

    fn success(&self, ip: IpAddr) {
        self.map.lock().remove(&ip);
    }
}

struct Ctx {
    sup: Arc<Supervisor>,
    limiter: AuthLimiter,
    shutdown: watch::Receiver<bool>,
}

// O erro já é a resposta HTTP final, devolvida uma vez por requisição; caixa extra não ganha nada.
#[allow(clippy::result_large_err)]
fn authenticate(ctx: &Ctx, headers: &HeaderMap, peer: SocketAddr) -> Result<Principal, Response<Body>> {
    let ip = peer.ip();
    if ctx.limiter.blocked(ip) {
        let e = RpcError::new(codes::UNAUTHORIZED, "rate_limited", "falhas de autenticação demais; espere um minuto");
        return Err(rpc_error_response(StatusCode::TOO_MANY_REQUESTS, &e));
    }
    let unauthorized = |msg: String| {
        let mut r = rpc_error_response(StatusCode::UNAUTHORIZED, &RpcError::unauthorized(msg));
        r.headers_mut().insert(header::WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"));
        r
    };
    let Some(h) = headers.get(header::AUTHORIZATION) else {
        ctx.limiter.fail(ip);
        return Err(unauthorized(crate::auth::AuthError::Missing.to_string()));
    };
    let token = h
        .to_str()
        .ok()
        .and_then(|s| s.split_once(' '))
        .filter(|(scheme, _)| scheme.eq_ignore_ascii_case("bearer"))
        .map(|(_, t)| t.trim());
    let Some(token) = token else {
        ctx.limiter.fail(ip);
        return Err(unauthorized("cabeçalho Authorization precisa ser `Bearer <chave>`".into()));
    };
    match ctx.sup.auth.authenticate(token, now_unix()) {
        Ok(p) => {
            ctx.limiter.success(ip);
            Ok(p)
        }
        Err(e) if e.is_credential_error() => {
            ctx.limiter.fail(ip);
            tracing::warn!(%ip, "autenticação recusada: {e}");
            Err(unauthorized(e.to_string()))
        }
        Err(e) => {
            tracing::error!("falha ao ler as chaves: {e}");
            Err(rpc_error_response(StatusCode::INTERNAL_SERVER_ERROR, &RpcError::internal("falha interna ao autenticar")))
        }
    }
}

fn health(sup: &Supervisor) -> Response<Body> {
    let workers = sup.workers();
    let ready = workers.iter().filter(|w| w.state == SlotState::Ready).count();
    let status = if sup.is_shutting_down() {
        "stopping"
    } else if ready == workers.len() {
        "ok"
    } else if ready > 0 {
        "degraded"
    } else {
        "unavailable"
    };
    let code = if ready > 0 && !sup.is_shutting_down() { StatusCode::OK } else { StatusCode::SERVICE_UNAVAILABLE };
    let w: Vec<Value> = workers
        .iter()
        .map(|w| json!({ "index": w.index, "state": w.state, "restarts": w.restarts, "generation": w.generation }))
        .collect();
    json_response(
        code,
        &json!({
            "status": status,
            "workers": w,
            "uptime_secs": sup.started.elapsed().as_secs(),
            "version": env!("CARGO_PKG_VERSION"),
        }),
    )
}

/// Atende uma requisição (sem streaming). `None` = notificação, sem resposta.
async fn dispatch_one(sup: &Arc<Supervisor>, p: &Principal, item: Value) -> Option<Value> {
    let r = match rpc::parse_request(item) {
        Ok(r) => r,
        Err((id, e)) => return Some(rpc::failure(id, &e)),
    };
    let res = sup.handle(p, &r.method, r.params, None).await;
    r.id.map(|id| match res {
        Ok(v) => rpc::success(id, v),
        Err(e) => rpc::failure(id, &e),
    })
}

#[allow(clippy::result_large_err)]
async fn read_body(sup: &Supervisor, headers: &HeaderMap, body: Incoming) -> Result<Bytes, Response<Body>> {
    let max = sup.cfg.service.max_request_bytes.0 as usize;
    // Content-Length declarado acima do teto: responde já, sem ler o corpo.
    let declared = headers.get(header::CONTENT_LENGTH).and_then(|v| v.to_str().ok()).and_then(|v| v.parse::<u64>().ok());
    if declared.is_some_and(|n| n > max as u64) {
        return Err(rpc_error_response(
            StatusCode::PAYLOAD_TOO_LARGE,
            &RpcError::too_large(format!("corpo maior que o teto de {max} bytes")),
        ));
    }
    match Limited::new(body, max).collect().await {
        Ok(c) => Ok(c.to_bytes()),
        Err(e) if e.downcast_ref::<http_body_util::LengthLimitError>().is_some() => Err(rpc_error_response(
            StatusCode::PAYLOAD_TOO_LARGE,
            &RpcError::too_large(format!("corpo maior que o teto de {max} bytes")),
        )),
        Err(e) => Err(rpc_error_response(StatusCode::BAD_REQUEST, &RpcError::invalid_request(format!("corpo ilegível: {e}")))),
    }
}

async fn rpc_http(ctx: Arc<Ctx>, req: Request<Incoming>, peer: SocketAddr) -> Response<Body> {
    let p = match authenticate(&ctx, req.headers(), peer) {
        Ok(p) => p,
        Err(r) => return r,
    };
    let (parts, body) = req.into_parts();
    let body = match read_body(&ctx.sup, &parts.headers, body).await {
        Ok(b) => b,
        Err(r) => return r,
    };
    let v: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => {
            return json_response(StatusCode::OK, &rpc::failure(Value::Null, &RpcError::parse_error(format!("JSON inválido: {e}"))));
        }
    };
    match v {
        Value::Array(items) => {
            if items.is_empty() {
                return json_response(StatusCode::OK, &rpc::failure(Value::Null, &RpcError::invalid_request("lote vazio")));
            }
            // Num lote, `exec.stream` responde como `exec` (sem notificações).
            let futs = items.into_iter().map(|item| {
                let (sup, p) = (ctx.sup.clone(), p.clone());
                async move { dispatch_one(&sup, &p, item).await }
            });
            let out: Vec<Value> = futures_util::future::join_all(futs).await.into_iter().flatten().collect();
            if out.is_empty() {
                no_content()
            } else {
                json_response(StatusCode::OK, &Value::Array(out))
            }
        }
        single => match rpc::parse_request(single) {
            Err((id, e)) => json_response(StatusCode::OK, &rpc::failure(id, &e)),
            Ok(r) if r.id.is_some() && STREAMING_METHODS.contains(&r.method.as_str()) => stream_response(ctx.sup.clone(), p, r),
            Ok(r) => {
                let res = ctx.sup.handle(&p, &r.method, r.params, None).await;
                match r.id {
                    Some(id) => json_response(
                        StatusCode::OK,
                        &match res {
                            Ok(v) => rpc::success(id, v),
                            Err(e) => rpc::failure(id, &e),
                        },
                    ),
                    None => no_content(),
                }
            }
        },
    }
}

fn no_content() -> Response<Body> {
    let mut r = Response::new(full(Bytes::new()));
    *r.status_mut() = StatusCode::NO_CONTENT;
    r
}

fn ndjson_line(v: &Value) -> Bytes {
    let mut b = serde_json::to_vec(v).expect("JSON sempre serializa");
    b.push(b'\n');
    Bytes::from(b)
}

/// Resposta NDJSON: notificações enquanto o comando roda e a resposta no fim. Se o cliente fecha a
/// conexão, o futuro é descartado e a execução é cancelada.
fn stream_response(sup: Arc<Supervisor>, p: Principal, r: rpc::Request) -> Response<Body> {
    let id = r.id.clone().expect("só com id");
    let (ntx, mut nrx) = mpsc::unbounded_channel::<Value>();
    let (btx, brx) = mpsc::channel::<Result<Frame<Bytes>, std::io::Error>>(64);
    tokio::spawn(async move {
        let notifier = Notifier { tx: ntx, request_id: id.clone() };
        let work = sup.handle(&p, &r.method, r.params, Some(notifier));
        tokio::pin!(work);
        let res = loop {
            tokio::select! {
                res = &mut work => break Some(res),
                Some(n) = nrx.recv() => {
                    if btx.send(Ok(Frame::data(ndjson_line(&n)))).await.is_err() {
                        break None;
                    }
                }
                _ = btx.closed() => break None,
            }
        };
        let Some(res) = res else { return };
        while let Ok(n) = nrx.try_recv() {
            if btx.send(Ok(Frame::data(ndjson_line(&n)))).await.is_err() {
                return;
            }
        }
        let last = match res {
            Ok(v) => rpc::success(id, v),
            Err(e) => rpc::failure(id, &e),
        };
        let _ = btx.send(Ok(Frame::data(ndjson_line(&last)))).await;
    });
    let stream = futures_util::stream::unfold(brx, |mut rx| async move { rx.recv().await.map(|x| (x, rx)) });
    let mut resp = Response::new(BodyExt::boxed(StreamBody::new(stream)));
    resp.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static("application/x-ndjson"));
    resp.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    resp
}

fn header_has(headers: &HeaderMap, name: header::HeaderName, token: &str) -> bool {
    headers
        .get_all(name)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(','))
        .any(|t| t.trim().eq_ignore_ascii_case(token))
}

async fn websocket(ctx: Arc<Ctx>, req: Request<Incoming>, peer: SocketAddr) -> Response<Body> {
    let p = match authenticate(&ctx, req.headers(), peer) {
        Ok(p) => p,
        Err(r) => return r,
    };
    let h = req.headers();
    let key = h.get(header::SEC_WEBSOCKET_KEY).map(|k| k.as_bytes().to_vec());
    let ok = header_has(h, header::CONNECTION, "upgrade")
        && header_has(h, header::UPGRADE, "websocket")
        && h.get(header::SEC_WEBSOCKET_VERSION).is_some_and(|v| v == "13");
    let Some(key) = key.filter(|_| ok) else {
        return rpc_error_response(StatusCode::BAD_REQUEST, &RpcError::invalid_request("upgrade de WebSocket inválido"));
    };
    let accept = tokio_tungstenite::tungstenite::handshake::derive_accept_key(&key);
    let max = ctx.sup.cfg.service.max_request_bytes.0 as usize;
    tokio::spawn(async move {
        match hyper::upgrade::on(req).await {
            Ok(up) => {
                let cfg = WebSocketConfig::default().max_message_size(Some(max)).max_frame_size(Some(max));
                let ws = WebSocketStream::from_raw_socket(TokioIo::new(up), Role::Server, Some(cfg)).await;
                ws_session(ctx, p, ws).await;
            }
            Err(e) => tracing::warn!(%peer, "upgrade de WebSocket falhou: {e}"),
        }
    });
    let mut r = Response::new(full(Bytes::new()));
    *r.status_mut() = StatusCode::SWITCHING_PROTOCOLS;
    let hs = r.headers_mut();
    hs.insert(header::UPGRADE, HeaderValue::from_static("websocket"));
    hs.insert(header::CONNECTION, HeaderValue::from_static("Upgrade"));
    hs.insert(header::SEC_WEBSOCKET_ACCEPT, HeaderValue::from_str(&accept).expect("base64 é cabeçalho válido"));
    r
}

async fn ws_session(ctx: Arc<Ctx>, p: Principal, ws: WebSocketStream<TokioIo<hyper::upgrade::Upgraded>>) {
    let (mut sink, mut stream) = ws.split();
    let (out_tx, mut out_rx) = mpsc::unbounded_channel::<Value>();
    let writer = tokio::spawn(async move {
        while let Some(v) = out_rx.recv().await {
            let text = serde_json::to_string(&v).expect("JSON sempre serializa");
            if sink.send(Message::text(text)).await.is_err() {
                break;
            }
        }
        let _ = sink.close().await;
    });
    let mut tasks = tokio::task::JoinSet::new();
    let mut shutdown = ctx.shutdown.clone();
    loop {
        tokio::select! {
            msg = stream.next() => match msg {
                Some(Ok(Message::Text(t))) => {
                    let v: Value = match serde_json::from_str(t.as_str()) {
                        Ok(v) => v,
                        Err(e) => {
                            let _ = out_tx.send(rpc::failure(Value::Null, &RpcError::parse_error(format!("JSON inválido: {e}"))));
                            continue;
                        }
                    };
                    let items = match v {
                        Value::Array(items) if items.is_empty() => {
                            let _ = out_tx.send(rpc::failure(Value::Null, &RpcError::invalid_request("lote vazio")));
                            continue;
                        }
                        Value::Array(items) => items,
                        one => vec![one],
                    };
                    for item in items {
                        let (sup, p, out) = (ctx.sup.clone(), p.clone(), out_tx.clone());
                        tasks.spawn(async move {
                            let r = match rpc::parse_request(item) {
                                Ok(r) => r,
                                Err((id, e)) => {
                                    let _ = out.send(rpc::failure(id, &e));
                                    return;
                                }
                            };
                            let notifier = match (&r.id, STREAMING_METHODS.contains(&r.method.as_str())) {
                                (Some(id), true) => Some(Notifier { tx: out.clone(), request_id: id.clone() }),
                                _ => None,
                            };
                            let res = sup.handle(&p, &r.method, r.params, notifier).await;
                            if let Some(id) = r.id {
                                let _ = out.send(match res {
                                    Ok(v) => rpc::success(id, v),
                                    Err(e) => rpc::failure(id, &e),
                                });
                            }
                        });
                    }
                }
                Some(Ok(Message::Binary(_))) => {
                    let _ = out_tx.send(rpc::failure(Value::Null, &RpcError::invalid_request("mande JSON-RPC em mensagem de texto")));
                }
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                Some(Ok(_)) => {}
            },
            Some(_) = tasks.join_next(), if !tasks.is_empty() => {}
            _ = shutdown.changed() => break,
        }
    }
    // Conexão fechada: o que estava em andamento é cancelado (os futuros são descartados).
    tasks.abort_all();
    while tasks.join_next().await.is_some() {}
    drop(out_tx);
    let _ = writer.await;
}

async fn route(ctx: Arc<Ctx>, req: Request<Incoming>, peer: SocketAddr) -> Response<Body> {
    let path = req.uri().path().to_string();
    match (req.method(), path.as_str()) {
        (&Method::GET | &Method::HEAD, "/healthz") => health(&ctx.sup),
        (&Method::POST, "/rpc") => rpc_http(ctx, req, peer).await,
        (&Method::GET, "/ws") => websocket(ctx, req, peer).await,
        (_, "/healthz" | "/rpc" | "/ws") => {
            rpc_error_response(StatusCode::METHOD_NOT_ALLOWED, &RpcError::invalid_request("método HTTP não permitido nesta rota"))
        }
        _ => rpc_error_response(StatusCode::NOT_FOUND, &RpcError::invalid_request(format!("rota desconhecida: {path}"))),
    }
}

/// Atende até `shutdown` disparar; depois para de aceitar, pede encerramento gracioso das conexões e
/// espera até `grace`.
pub async fn serve(sup: Arc<Supervisor>, listener: TcpListener, mut shutdown: watch::Receiver<bool>, grace: Duration) {
    let max_conns = sup.cfg.service.max_connections as usize;
    let conns = Arc::new(Semaphore::new(max_conns));
    let ctx = Arc::new(Ctx { sup, limiter: AuthLimiter::default(), shutdown: shutdown.clone() });
    loop {
        tokio::select! {
            r = listener.accept() => {
                let (stream, peer) = match r {
                    Ok(x) => x,
                    Err(e) => {
                        tracing::warn!("accept falhou: {e}");
                        tokio::time::sleep(Duration::from_millis(50)).await;
                        continue;
                    }
                };
                let _ = stream.set_nodelay(true);
                let io = TokioIo::new(stream);
                match conns.clone().try_acquire_owned() {
                    Ok(permit) => {
                        let ctx = ctx.clone();
                        let mut stop = shutdown.clone();
                        tokio::spawn(async move {
                            let svc = service_fn(move |req| {
                                let ctx = ctx.clone();
                                async move { Ok::<_, Infallible>(route(ctx, req, peer).await) }
                            });
                            let conn = http1::Builder::new()
                                .timer(hyper_util::rt::TokioTimer::new())
                                .header_read_timeout(Duration::from_secs(30))
                                .serve_connection(io, svc)
                                .with_upgrades();
                            tokio::pin!(conn);
                            tokio::select! {
                                _ = &mut conn => {}
                                _ = stop.changed() => {
                                    conn.as_mut().graceful_shutdown();
                                    let _ = conn.await;
                                }
                            }
                            drop(permit);
                        });
                    }
                    Err(_) => {
                        tokio::spawn(async move {
                            let svc = service_fn(|_req| async {
                                Ok::<_, Infallible>(rpc_error_response(
                                    StatusCode::SERVICE_UNAVAILABLE,
                                    &RpcError::capacity("connections", "conexões demais; tente de novo"),
                                ))
                            });
                            let _ = http1::Builder::new().keep_alive(false).serve_connection(io, svc).await;
                        });
                    }
                }
            }
            _ = shutdown.changed() => break,
        }
    }
    drop(listener);
    let all = u32::try_from(max_conns).unwrap_or(u32::MAX);
    if tokio::time::timeout(grace, conns.acquire_many(all)).await.is_err() {
        tracing::warn!("conexões ainda abertas depois de {} s; encerrando assim mesmo", grace.as_secs());
    }
}
