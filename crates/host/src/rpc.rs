//! JSON-RPC 2.0: envelope, erros e códigos.
//!
//! Os códigos de -32768 a -32000 são do JSON-RPC; os nossos ficam de -32001 a -32099 (a faixa de
//! "server error" da especificação). Todo erro nosso leva em `data.kind` um nome estável pra máquina,
//! e a `message` é a frase pra pessoa.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub mod codes {
    pub const PARSE_ERROR: i32 = -32700;
    pub const INVALID_REQUEST: i32 = -32600;
    pub const METHOD_NOT_FOUND: i32 = -32601;
    pub const INVALID_PARAMS: i32 = -32602;
    pub const INTERNAL: i32 = -32603;
    /// Chave ausente, inválida, expirada ou revogada.
    pub const UNAUTHORIZED: i32 = -32001;
    /// Autenticado, mas sem permissão (método de admin, por exemplo).
    pub const FORBIDDEN: i32 = -32002;
    /// Quota do usuário esgotada.
    pub const QUOTA_EXCEEDED: i32 = -32003;
    /// Sandbox, sessão ou snapshot inexistente (ou de outro usuário).
    pub const NOT_FOUND: i32 = -32004;
    /// A sandbox estava num worker que caiu e não tinha snapshot persistido.
    pub const SANDBOX_LOST: i32 = -32005;
    /// O worker da sandbox está reiniciando; tente de novo.
    pub const WORKER_UNAVAILABLE: i32 = -32006;
    /// O serviço inteiro está sem capacidade (controle de admissão global).
    pub const CAPACITY: i32 = -32007;
    /// Corpo, arquivo ou saída maior que o permitido.
    pub const TOO_LARGE: i32 = -32008;
    /// A sessão já está executando um comando.
    pub const BUSY: i32 = -32009;
    /// Erro do sistema de arquivos ou de processo dentro da sandbox; `data.errno` traz o número.
    pub const OS_ERROR: i32 = -32010;
    /// A sessão terminou (o shell saiu) ou se perdeu na queda do worker.
    pub const SESSION_LOST: i32 = -32011;
    /// O worker caiu com a requisição em andamento.
    pub const WORKER_CRASHED: i32 = -32012;
    /// A requisição foi cancelada (cliente desconectou ou o daemon está desligando).
    pub const CANCELLED: i32 = -32013;
    /// Falha ao aplicar o isolamento exigido (Landlock ou seccomp).
    pub const ISOLATION: i32 = -32014;
}

/// Objeto de erro do JSON-RPC.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, thiserror::Error)]
#[error("{message}")]
pub struct RpcError {
    pub code: i32,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl RpcError {
    pub fn new(code: i32, kind: &str, message: impl Into<String>) -> RpcError {
        RpcError { code, message: message.into(), data: Some(json!({ "kind": kind })) }
    }

    /// Acrescenta campos em `data` (que é sempre objeto).
    pub fn with(mut self, key: &str, value: impl Into<Value>) -> RpcError {
        match &mut self.data {
            Some(Value::Object(m)) => {
                m.insert(key.to_string(), value.into());
            }
            _ => self.data = Some(json!({ key: value.into() })),
        }
        self
    }

    pub fn kind(&self) -> Option<&str> {
        self.data.as_ref()?.get("kind")?.as_str()
    }

    pub fn parse_error(msg: impl Into<String>) -> RpcError {
        RpcError::new(codes::PARSE_ERROR, "parse_error", msg)
    }
    pub fn invalid_request(msg: impl Into<String>) -> RpcError {
        RpcError::new(codes::INVALID_REQUEST, "invalid_request", msg)
    }
    pub fn method_not_found(method: &str) -> RpcError {
        RpcError::new(codes::METHOD_NOT_FOUND, "method_not_found", format!("método desconhecido: {method}"))
    }
    pub fn invalid_params(msg: impl Into<String>) -> RpcError {
        RpcError::new(codes::INVALID_PARAMS, "invalid_params", msg)
    }
    pub fn internal(msg: impl Into<String>) -> RpcError {
        RpcError::new(codes::INTERNAL, "internal", msg)
    }
    pub fn unauthorized(msg: impl Into<String>) -> RpcError {
        RpcError::new(codes::UNAUTHORIZED, "unauthorized", msg)
    }
    pub fn forbidden(msg: impl Into<String>) -> RpcError {
        RpcError::new(codes::FORBIDDEN, "forbidden", msg)
    }
    pub fn quota(resource: &str, limit: u64, used: u64, msg: impl Into<String>) -> RpcError {
        RpcError::new(codes::QUOTA_EXCEEDED, "quota_exceeded", msg)
            .with("resource", resource)
            .with("limit", limit)
            .with("used", used)
    }
    pub fn capacity(resource: &str, msg: impl Into<String>) -> RpcError {
        RpcError::new(codes::CAPACITY, "capacity", msg).with("resource", resource)
    }
    pub fn not_found(what: &str, id: &str) -> RpcError {
        RpcError::new(codes::NOT_FOUND, "not_found", format!("{what} {id} não existe")).with("id", id)
    }
    pub fn too_large(msg: impl Into<String>) -> RpcError {
        RpcError::new(codes::TOO_LARGE, "too_large", msg)
    }
    pub fn busy(msg: impl Into<String>) -> RpcError {
        RpcError::new(codes::BUSY, "busy", msg)
    }
    pub fn cancelled(msg: impl Into<String>) -> RpcError {
        RpcError::new(codes::CANCELLED, "cancelled", msg)
    }
    /// Erro de errno: `message` no formato `contexto: strerror`, como as ferramentas GNU.
    pub fn errno(errno: sysabi::Errno, context: &str) -> RpcError {
        let msg = if context.is_empty() { errno.message() } else { format!("{context}: {}", errno.message()) };
        let e = RpcError::new(codes::OS_ERROR, "os_error", msg).with("errno", errno.0);
        match errno.name() {
            Some(n) => e.with("errno_name", n),
            None => e,
        }
    }
}

/// Requisição já validada.
#[derive(Clone, Debug, PartialEq)]
pub struct Request {
    /// `None` = notificação (sem resposta).
    pub id: Option<Value>,
    pub method: String,
    pub params: Value,
}

/// Valida um objeto de requisição. Em erro devolve o id (quando deu pra ler) junto.
pub fn parse_request(v: Value) -> Result<Request, (Value, RpcError)> {
    let Value::Object(mut m) = v else {
        return Err((Value::Null, RpcError::invalid_request("a requisição precisa ser um objeto")));
    };
    let id = m.remove("id");
    let id_for_err = id.clone().unwrap_or(Value::Null);
    if let Some(i) = &id
        && !(i.is_string() || i.is_number() || i.is_null())
    {
        return Err((Value::Null, RpcError::invalid_request("id precisa ser texto, número ou null")));
    }
    match m.remove("jsonrpc") {
        Some(Value::String(s)) if s == "2.0" => {}
        _ => return Err((id_for_err, RpcError::invalid_request("jsonrpc precisa ser \"2.0\""))),
    }
    let method = match m.remove("method") {
        Some(Value::String(s)) => s,
        _ => return Err((id_for_err, RpcError::invalid_request("method precisa ser texto"))),
    };
    let params = match m.remove("params") {
        None => Value::Object(Default::default()),
        Some(p @ (Value::Object(_) | Value::Array(_))) => p,
        Some(_) => return Err((id_for_err, RpcError::invalid_request("params precisa ser objeto ou lista"))),
    };
    if let Some(extra) = m.keys().next() {
        return Err((id_for_err, RpcError::invalid_request(format!("campo desconhecido na requisição: {extra}"))));
    }
    Ok(Request { id, method, params })
}

pub fn success(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

pub fn failure(id: Value, err: &RpcError) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": err })
}

pub fn notification(method: &str, params: Value) -> Value {
    json!({ "jsonrpc": "2.0", "method": method, "params": params })
}

/// Desserializa os parâmetros de um método, com a mensagem de erro do serde.
pub fn params<T: serde::de::DeserializeOwned>(p: Value) -> Result<T, RpcError> {
    serde_json::from_value(p).map_err(|e| RpcError::invalid_params(format!("parâmetros inválidos: {e}")))
}

/// Resposta de um cliente: separa `result` de `error`.
pub fn parse_response(v: Value) -> Result<Value, RpcError> {
    let Value::Object(mut m) = v else {
        return Err(RpcError::internal("resposta JSON-RPC que não é objeto"));
    };
    if let Some(e) = m.remove("error") {
        return Err(serde_json::from_value(e).unwrap_or_else(|e| RpcError::internal(format!("erro ilegível: {e}"))));
    }
    m.remove("result").ok_or_else(|| RpcError::internal("resposta JSON-RPC sem result nem error"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_validation() {
        let ok = parse_request(json!({"jsonrpc":"2.0","id":1,"method":"exec","params":{"a":1}})).unwrap();
        assert_eq!(ok.id, Some(json!(1)));
        assert_eq!(ok.method, "exec");
        let notif = parse_request(json!({"jsonrpc":"2.0","method":"x"})).unwrap();
        assert_eq!(notif.id, None);
        assert_eq!(notif.params, json!({}));
        let (id, e) = parse_request(json!({"jsonrpc":"1.0","id":"a","method":"x"})).unwrap_err();
        assert_eq!(id, json!("a"));
        assert_eq!(e.code, codes::INVALID_REQUEST);
        assert!(parse_request(json!({"jsonrpc":"2.0","id":{},"method":"x"})).is_err());
        assert!(parse_request(json!({"jsonrpc":"2.0","id":1,"method":"x","params":3})).is_err());
        assert!(parse_request(json!({"jsonrpc":"2.0","id":1,"method":"x","extra":3})).is_err());
        assert!(parse_request(json!([1])).is_err());
    }

    #[test]
    fn errno_errors_carry_glibc_message() {
        let e = RpcError::errno(sysabi::Errno::ENOENT, "/nope");
        assert_eq!(e.message, "/nope: No such file or directory");
        assert_eq!(e.code, codes::OS_ERROR);
        assert_eq!(e.data.as_ref().unwrap()["errno"], 2);
        assert_eq!(e.data.as_ref().unwrap()["errno_name"], "ENOENT");
        assert_eq!(e.kind(), Some("os_error"));
    }

    #[test]
    fn response_round_trip() {
        assert_eq!(parse_response(success(json!(1), json!({"x": 2}))).unwrap(), json!({"x": 2}));
        let err = RpcError::busy("ocupada");
        assert_eq!(parse_response(failure(json!(1), &err)).unwrap_err(), err);
    }
}
