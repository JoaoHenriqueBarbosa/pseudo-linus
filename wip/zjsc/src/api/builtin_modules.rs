//! Registro dos módulos embutidos do `require` (`node:vm`, `node:fs`, ...).
//!
//! O bun aceita um módulo embutido com e sem o prefixo `node:` (`require("vm") === require("node:vm")`), exceto os que
//! só existem com prefixo (`node:test`, `node:sqlite`: `require("test")` cai na busca em `node_modules`). Com o prefixo
//! `node:` e nome fora da tabela, o `require` lança `ERR_UNKNOWN_BUILTIN_MODULE` (`throw_require_failure`).
//!
//! Cada entrada tem um instalador (`Installer`) que monta o objeto do módulo na primeira chamada; o resultado fica num
//! cache por nome canônico (sem prefixo), então `require("vm")` e `require("node:vm")` devolvem o mesmo objeto. Os
//! embutidos não entram em `require.cache` (medido no bun 1.4.2: `require.cache["os"]` é `undefined`).
//!
//! Para acrescentar um módulo: escrever o instalador no arquivo do módulo e pôr uma linha em `BUILTINS`.

use std::cell::RefCell;

use crate::api::eval::{js_text, key_of};
use crate::runtime::host_call::{HostResult, Thrown};
use crate::runtime::identifier::Identifier;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_value::JSValue;
use crate::runtime::object_constructor::{construct_array_of, construct_empty_object, enumerable_own_entries};

/// Monta o objeto do módulo. Roda uma vez por programa; o resultado é guardado no cache do registro.
pub(crate) type Installer = fn(&JSGlobalObject) -> HostResult;

/// Como o nome do módulo é aceito pelo `require`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Scheme {
    /// `vm` e `node:vm`.
    Both,
    /// Só `node:test`: o nome nu segue para a busca em `node_modules`.
    NodeOnly,
}

#[derive(Debug)]
pub(crate) struct BuiltinEntry {
    /// O nome canônico, sem prefixo (`vm`, `fs/promises`).
    pub name: &'static str,
    pub scheme: Scheme,
    pub install: Installer,
}

/// O que o `require` faz com um pedido.
#[derive(Debug)]
pub(crate) enum Resolved<'a> {
    Builtin(&'a BuiltinEntry),
    /// Tem prefixo `node:` e o nome não está na tabela.
    UnknownNode,
    /// Não é embutido: segue a resolução de arquivos.
    NotBuiltin,
}

/// Os módulos instalados. Acrescentar aqui, na ordem alfabética.
pub(crate) static BUILTINS: &[BuiltinEntry] = &[
    BuiltinEntry { name: "buffer", scheme: Scheme::Both, install: crate::runtime::node_buffer::install_buffer_module },
    BuiltinEntry { name: "module", scheme: Scheme::Both, install: install_module_module },
    BuiltinEntry { name: "os", scheme: Scheme::Both, install: crate::runtime::node_os::install_os_module },
    BuiltinEntry { name: "vm", scheme: Scheme::Both, install: crate::api::eval::install_vm_module },
];

/// `require("module").builtinModules` do bun 1.4.2, na ordem exata medida com `bun -e` (76 nomes). Os `bun:*` já
/// constam, embora o `require("bun:test")` ainda falhe como módulo inexistente até haver instalador (a resolução cai
/// em `MODULE_NOT_FOUND`, igual ao `bun:zz`); `node:sqlite` é o único com prefixo na lista.
pub(crate) static BUILTIN_MODULE_NAMES: &[&str] = &[
    "_http_agent", "_http_client", "_http_common", "_http_incoming", "_http_outgoing", "_http_server", "_stream_duplex",
    "_stream_passthrough", "_stream_readable", "_stream_transform", "_stream_wrap", "_stream_writable", "_tls_common",
    "_tls_wrap", "assert", "assert/strict", "async_hooks", "buffer", "bun:ffi", "bun:jsc", "bun:sqlite", "bun:test", "bun",
    "child_process", "cluster", "console", "constants", "crypto", "dgram", "diagnostics_channel", "dns", "dns/promises",
    "domain", "events", "fs", "fs/promises", "http", "http2", "https", "inspector", "inspector/promises", "module", "net",
    "node:sqlite", "os", "path", "path/posix", "path/win32", "perf_hooks", "process", "punycode", "querystring", "readline",
    "readline/promises", "repl", "stream", "stream/consumers", "stream/promises", "stream/web", "string_decoder", "sys",
    "timers", "timers/promises", "tls", "trace_events", "tty", "undici", "url", "util", "util/types", "v8", "vm", "wasi",
    "worker_threads", "ws", "zlib",
];

/// Instalador de `node:module`: por ora só `builtinModules`, o array da tabela `BUILTIN_MODULE_NAMES`.
fn install_module_module(global_object: &JSGlobalObject) -> HostResult {
    let vm = global_object.vm();
    let module_object = construct_empty_object(global_object);
    let names: Vec<JSValue> = BUILTIN_MODULE_NAMES.iter().map(|name| js_text(vm, name)).collect();
    let array = construct_array_of(global_object, &names);
    module_object.put_direct(vm, &key_of(vm, "builtinModules"), array.as_value(), 0);
    Ok(module_object.as_value())
}

/// A chave canônica de um `import` embutido (`node:path`), ou `None` quando `specifier` não é embutido instalado.
pub(crate) fn import_key(specifier: &str) -> Option<String> {
    match resolve(specifier) {
        Resolved::Builtin(entry) => Some(format!("node:{}", entry.name)),
        _ => None,
    }
}

/// Os exports do namespace de `import "node:X"`: `default` é o mesmo objeto do `require`, e cada propriedade própria
/// enumerável dele vira export nomeado (medido: `import * as os from "node:os"` tem as chaves do `require("os")` mais
/// `default`). `None` quando `key` não é uma chave de `import_key`.
pub(crate) fn import_exports(global_object: &JSGlobalObject, key: &str) -> Option<Result<(Vec<Identifier>, Vec<JSValue>), Thrown>> {
    let name = key.strip_prefix("node:")?;
    let entry = BUILTINS.iter().find(|entry| entry.name == name)?;
    Some(exports_of(global_object, entry))
}

fn exports_of(global_object: &JSGlobalObject, entry: &'static BuiltinEntry) -> Result<(Vec<Identifier>, Vec<JSValue>), Thrown> {
    let vm = global_object.vm();
    let object = load(global_object, entry)?;
    let mut names = vec![Identifier::from_span(vm, b"default")];
    let mut values = vec![object];
    for (name, value) in enumerable_own_entries(global_object, object)? {
        if names.contains(&name) {
            continue;
        }
        names.push(name);
        values.push(value);
    }
    Ok((names, values))
}

thread_local! {
    /// Nome canônico para o objeto do módulo, por programa.
    static CACHE: RefCell<Vec<(&'static str, JSValue)>> = const { RefCell::new(Vec::new()) };
}

/// Classifica `request` contra `table`.
pub(crate) fn resolve_in<'a>(table: &'a [BuiltinEntry], request: &str) -> Resolved<'a> {
    if let Some(name) = request.strip_prefix("node:") {
        return table.iter().find(|entry| entry.name == name).map_or(Resolved::UnknownNode, Resolved::Builtin);
    }
    table
        .iter()
        .find(|entry| entry.scheme == Scheme::Both && entry.name == request)
        .map_or(Resolved::NotBuiltin, Resolved::Builtin)
}

pub(crate) fn resolve(request: &str) -> Resolved<'static> {
    resolve_in(BUILTINS, request)
}

/// O objeto do módulo: do cache, ou recém-instalado (e guardado).
pub(crate) fn load(global_object: &JSGlobalObject, entry: &'static BuiltinEntry) -> HostResult {
    if let Some(cached) = CACHE.with(|cache| cache.borrow().iter().find(|(name, _)| *name == entry.name).map(|(_, value)| value.clone())) {
        return Ok(cached);
    }
    // Sem empréstimo durante o instalador: ele pode reentrar no `require`.
    let installed = (entry.install)(global_object)?;
    CACHE.with(|cache| cache.borrow_mut().push((entry.name, installed.clone())));
    Ok(installed)
}

/// Fim do programa (`eval::reset_for_program`): o cache é do programa.
pub(crate) fn reset_for_program() {
    let taken = CACHE.try_with(|cache| std::mem::take(&mut *cache.borrow_mut()));
    drop(taken);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn install_nothing(_: &JSGlobalObject) -> HostResult {
        Ok(JSValue::undefined())
    }

    #[test]
    fn prefix_rules_follow_bun() {
        let table = [
            BuiltinEntry { name: "vm", scheme: Scheme::Both, install: install_nothing },
            BuiltinEntry { name: "test", scheme: Scheme::NodeOnly, install: install_nothing },
        ];
        assert!(matches!(resolve_in(&table, "vm"), Resolved::Builtin(entry) if entry.name == "vm"));
        assert!(matches!(resolve_in(&table, "node:vm"), Resolved::Builtin(entry) if entry.name == "vm"));
        assert!(matches!(resolve_in(&table, "node:test"), Resolved::Builtin(entry) if entry.name == "test"));
        assert!(matches!(resolve_in(&table, "test"), Resolved::NotBuiltin));
        assert!(matches!(resolve_in(&table, "node:nope"), Resolved::UnknownNode));
        assert!(matches!(resolve_in(&table, "node:"), Resolved::UnknownNode));
        assert!(matches!(resolve_in(&table, "./vm"), Resolved::NotBuiltin));
    }
}
