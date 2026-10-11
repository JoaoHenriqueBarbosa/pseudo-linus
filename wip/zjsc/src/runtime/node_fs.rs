//! O módulo `node:fs` do bun 1.4.2.
//!
//! A semântica (opções, flags, codificações, `Stats`, `Dirent`, erros do libuv) mora em `node_fs.js`, um programa
//! escondido do hospedeiro (as funções dele se comportam como builtin do JSC: `toString` nativo, frame fora do
//! `Error.stack`). Ele só toca a máquina pelo `binding` deste arquivo, que é uma fina camada sobre o trait
//! [`Host`](crate::runtime::host::Host): cada função do binding faz uma chamada de sistema e, se ela falha, lança o
//! `-errno` como número (o JavaScript monta o erro do Node com `syscall`, `path` e `dest`).
//!
//! Cada export que é função vira uma nativa com o nome e o comprimento do bun, que repassa a chamada à função
//! de mesmo nome do programa escondido: assim a pilha mostra o frame nativo do bun (`at readFileSync (unknown)`) e
//! nenhum frame do JavaScript do porte.

use std::cell::RefCell;
use std::collections::HashMap;

use crate::api::eval::{evaluate_hidden_program, js_text};
use crate::host_function;
use crate::llint::LLIntFailure;
use crate::runtime::call_data::{call as call_function, get_call_data, CallData};
use crate::runtime::host::{host, FsDirEntry, FsStat, FsTime};
use crate::runtime::host_call::{pending_or, HostCall, HostResult, Thrown};
use crate::runtime::identifier::Identifier;
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::js_function::put_direct_native_function_with_display_name;
use crate::runtime::js_generic_typed_array_view::JSGenericTypedArrayView;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_module_loader::rust_string;
use crate::runtime::js_object::JSObject;
use crate::runtime::js_value::{js_number, JSValue};
use crate::runtime::native_function::NativeFunction;
use crate::runtime::property_name::PropertyName;
use crate::runtime::object_constructor::{construct_array_of, construct_empty_object, enumerable_own_entries};
use crate::runtime::string_regexp_support::get_object_property;
use crate::runtime::throw_scope::{throw_exception, ThrowScope};
use crate::wtf::text::wtf_string::String as WtfString;

/// O programa escondido com a semântica do módulo.
const PRELUDE: &str = include_str!("node_fs.js");

thread_local! {
    /// O objeto `{ table }` do programa escondido (raiz do coletor): as funções que as nativas repassam.
    static IMPLEMENTATION: RefCell<Option<JSValue>> = const { RefCell::new(None) };
    /// Cada nativa exportada (pela célula) e o nome da função do programa escondido que ela repassa.
    static SHIMS: RefCell<HashMap<usize, String>> = RefCell::new(HashMap::new());
}

pub(crate) fn visit_gc_roots(marker: &mut crate::runtime::gc::Marker) {
    marker.value(IMPLEMENTATION.with(|slot| slot.borrow().unwrap_or_else(JSValue::undefined)));
}

pub(crate) fn reset_for_program() {
    let _ = IMPLEMENTATION.try_with(|slot| slot.borrow_mut().take());
    let _ = SHIMS.try_with(|shims| shims.borrow_mut().clear());
}

/// Lança `-errno` como número (o JavaScript do módulo monta o erro).
fn fail(global_object: &JSGlobalObject, errno: i32) -> Thrown {
    let mut scope = ThrowScope::new(global_object.vm());
    throw_exception(global_object, &mut scope, js_number(f64::from(-errno)));
    Thrown::Pending
}

fn text_argument(global_object: &JSGlobalObject, call: &HostCall, index: usize) -> Result<String, Thrown> {
    Ok(rust_string(&pending_or(global_object, call.argument(index).to_wtf_string())?))
}

fn number_argument(global_object: &JSGlobalObject, call: &HostCall, index: usize) -> Result<f64, Thrown> {
    pending_or(global_object, call.argument(index).to_number())
}

fn int_argument(global_object: &JSGlobalObject, call: &HostCall, index: usize) -> Result<i32, Thrown> {
    Ok(number_argument(global_object, call, index)? as i32)
}

/// Um argumento opcional: ausente, `undefined`, `null` ou negativo é `None` (o `position` do `read` e do `write`).
fn optional_offset(global_object: &JSGlobalObject, call: &HostCall, index: usize) -> Result<Option<u64>, Thrown> {
    let value = call.argument(index);
    if value.is_undefined_or_null() {
        return Ok(None);
    }
    let number = number_argument(global_object, call, index)?;
    Ok((number >= 0.0).then_some(number as u64))
}

/// As entradas do diretório como `[nome, tipo, nome, tipo...]`, com o tipo em bits `S_IFMT`.
fn entries_array(global_object: &JSGlobalObject, entries: Vec<FsDirEntry>) -> JSValue {
    let mut values = Vec::with_capacity(entries.len() * 2);
    for entry in entries {
        values.push(js_text(global_object.vm(), &String::from_utf8_lossy(&entry.name)));
        values.push(js_number(f64::from(entry.mode_type)));
    }
    construct_array_of(global_object, &values).as_value()
}

fn stat_array(global_object: &JSGlobalObject, stat: FsStat) -> JSValue {
    let number = |value: u64| js_number(value as f64);
    let signed = |value: i64| js_number(value as f64);
    let values = [
        number(stat.dev),
        number(stat.ino),
        number(u64::from(stat.mode)),
        number(stat.nlink),
        number(u64::from(stat.uid)),
        number(u64::from(stat.gid)),
        number(stat.rdev),
        number(stat.size),
        number(stat.blksize),
        number(stat.blocks),
        signed(stat.atime.0),
        signed(stat.atime.1),
        signed(stat.mtime.0),
        signed(stat.mtime.1),
        signed(stat.ctime.0),
        signed(stat.ctime.1),
        signed(stat.birthtime.0),
        signed(stat.birthtime.1),
    ];
    construct_array_of(global_object, &values).as_value()
}

/// Os bytes do typed array do argumento `index` (o trecho que a visão enxerga).
fn bytes_argument(_global_object: &JSGlobalObject, call: &HostCall, index: usize) -> Result<Vec<u8>, Thrown> {
    match JSGenericTypedArrayView::from_value(&call.argument(index)) {
        Some(view) => Ok(view.with_view_bytes(|bytes| bytes.to_vec())),
        None => Err(Thrown::type_error("buffer must be a TypedArray")),
    }
}

/// Um tempo em segundos (com fração) como o `utimensat` o quer; `NaN` é "agora".
fn time_argument(global_object: &JSGlobalObject, call: &HostCall, index: usize) -> Result<FsTime, Thrown> {
    let seconds = number_argument(global_object, call, index)?;
    if seconds.is_nan() {
        return Ok(FsTime::Now);
    }
    let whole = seconds.floor();
    Ok(FsTime::At(whole as i64, ((seconds - whole) * 1e9).round() as i64))
}

/// O argumento `$index` da chamada, lido no tipo que o método do `Host` pede.
macro_rules! host_argument {
    ($global_object:ident, $call:ident, $index:literal, text) => {
        &text_argument($global_object, $call, $index)?
    };
    ($global_object:ident, $call:ident, $index:literal, int) => {
        int_argument($global_object, $call, $index)?
    };
    ($global_object:ident, $call:ident, $index:literal, unsigned) => {
        number_argument($global_object, $call, $index)? as u32
    };
    ($global_object:ident, $call:ident, $index:literal, length) => {
        number_argument($global_object, $call, $index)?.max(0.0) as u64
    };
    ($global_object:ident, $call:ident, $index:literal, time) => {
        time_argument($global_object, $call, $index)?
    };
    ($global_object:ident, $call:ident, $index:literal, bytes) => {
        &bytes_argument($global_object, $call, $index)?
    };
    ($global_object:ident, $call:ident, $index:literal, position) => {
        optional_offset($global_object, $call, $index)?
    };
    ($global_object:ident, $call:ident, $index:literal, flag) => {
        $call.argument($index).to_boolean()
    };
}

/// Define a função do binding que chama o método `$method` do `Host` com os argumentos dados e converte o resultado
/// com `$convert` (`unit` não devolve nada, `text` devolve a string, `stat` o array do `stat`).
macro_rules! host_binding {
    ($function:ident, $body:ident, $method:ident($($index:literal $kind:ident),*) => $convert:expr) => {
        fn $body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
            host().$method($(host_argument!(global_object, call, $index, $kind)),*).map(|value| $convert(global_object, value)).map_err(|errno| fail(global_object, errno))
        }
        host_function!($function, $body);
    };
}

fn unit(_global_object: &JSGlobalObject, _value: ()) -> JSValue {
    JSValue::undefined()
}

fn text(global_object: &JSGlobalObject, value: String) -> JSValue {
    js_text(global_object.vm(), &value)
}

fn descriptor(_global_object: &JSGlobalObject, value: i32) -> JSValue {
    js_number(f64::from(value))
}

fn count(_global_object: &JSGlobalObject, value: usize) -> JSValue {
    js_number(value as f64)
}

host_binding!(open_function, open_body, fs_open(0 text, 1 unsigned, 2 unsigned) => descriptor);
host_binding!(close_function, close_body, fs_close(0 int) => unit);
host_binding!(fstat_function, fstat_body, fs_fstat(0 int) => stat_array);
host_binding!(stat_function, stat_body, fs_stat(0 text, 1 flag) => stat_array);
host_binding!(readdir_function, readdir_body, fs_readdir(0 text) => entries_array);
host_binding!(mkdir_function, mkdir_body, fs_mkdir(0 text, 1 unsigned) => unit);
host_binding!(chmod_function, chmod_body, fs_chmod(0 text, 1 unsigned) => unit);
host_binding!(access_function, access_body, fs_access(0 text, 1 unsigned) => unit);
host_binding!(unlink_function, unlink_body, fs_unlink(0 text) => unit);
host_binding!(rmdir_function, rmdir_body, fs_rmdir(0 text) => unit);
host_binding!(rename_function, rename_body, fs_rename(0 text, 1 text) => unit);
host_binding!(link_function, link_body, fs_link(0 text, 1 text) => unit);
host_binding!(symlink_function, symlink_body, fs_symlink(0 text, 1 text) => unit);
host_binding!(readlink_function, readlink_body, fs_readlink(0 text) => text);
host_binding!(realpath_function, realpath_body, fs_realpath(0 text) => text);
host_binding!(fchmod_function, fchmod_body, fs_fchmod(0 int, 1 unsigned) => unit);
host_binding!(ftruncate_function, ftruncate_body, fs_ftruncate(0 int, 1 length) => unit);
host_binding!(fsync_function, fsync_body, fs_fsync(0 int) => unit);
host_binding!(utimes_function, utimes_body, fs_utimes(0 text, 1 time, 2 time, 3 flag) => unit);
host_binding!(futimes_function, futimes_body, fs_futimes(0 int, 1 time, 2 time) => unit);

/// `read(fd, buffer, offset, length, position)`: lê para dentro de `buffer` e devolve quantos bytes.
fn read_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let fd = int_argument(global_object, call, 0)?;
    let Some(view) = JSGenericTypedArrayView::from_value(&call.argument(1)) else {
        return Err(Thrown::type_error("buffer must be a TypedArray"));
    };
    let offset = number_argument(global_object, call, 2)? as usize;
    let length = number_argument(global_object, call, 3)? as usize;
    let bytes = host().fs_read(fd, length, optional_offset(global_object, call, 4)?).map_err(|errno| fail(global_object, errno))?;
    view.with_vector_mut(|destination| destination[offset..offset + bytes.len()].copy_from_slice(&bytes));
    Ok(count(global_object, bytes.len()))
}
host_function!(read_function, read_body);

host_binding!(write_function, write_body, fs_write(0 int, 1 bytes, 2 position) => count);

/// `random(length)`: bytes aleatórios do sistema (o sufixo do `mkdtemp`).
fn random_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let length = number_argument(global_object, call, 0)? as usize;
    let mut bytes = vec![0u8; length];
    host().random_fill(&mut bytes);
    Ok(construct_array_of(global_object, &bytes.iter().map(|byte| js_number(f64::from(*byte))).collect::<Vec<_>>()).as_value())
}
host_function!(random_function, random_body);

/// O objeto `binding` que o programa escondido recebe.
fn binding(global_object: &JSGlobalObject) -> JSValue {
    let vm = global_object.vm();
    let object = construct_empty_object(global_object);
    let functions: [(&str, u32, NativeFunction); 23] = [
        ("open", 3, open_function),
        ("close", 1, close_function),
        ("read", 5, read_function),
        ("write", 3, write_function),
        ("fstat", 1, fstat_function),
        ("stat", 2, stat_function),
        ("readdir", 1, readdir_function),
        ("mkdir", 2, mkdir_function),
        ("chmod", 2, chmod_function),
        ("access", 2, access_function),
        ("unlink", 1, unlink_function),
        ("rmdir", 1, rmdir_function),
        ("rename", 2, rename_function),
        ("link", 2, link_function),
        ("symlink", 2, symlink_function),
        ("readlink", 1, readlink_function),
        ("realpath", 1, realpath_function),
        ("fchmod", 2, fchmod_function),
        ("ftruncate", 2, ftruncate_function),
        ("fsync", 1, fsync_function),
        ("utimes", 4, utimes_function),
        ("futimes", 3, futimes_function),
        ("random", 1, random_function),
    ];
    for (name, length, function) in functions {
        put_direct_native_function_with_display_name(
            vm,
            global_object,
            &object,
            &Identifier::from_span(vm, name.as_bytes()),
            &WtfString::from_latin1(name.as_bytes()),
            length,
            function,
            ImplementationVisibility::Public,
            Intrinsic::NoIntrinsic,
            0,
        );
    }
    object.as_value()
}

/// O corpo de toda nativa exportada: repassa a chamada à função do programa escondido de mesmo nome.
fn forward_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let Some(name) = SHIMS.with(|shims| shims.borrow().get(&call.callee()).cloned()) else {
        return Ok(JSValue::undefined());
    };
    let table = IMPLEMENTATION.with(|slot| slot.borrow().unwrap_or_else(JSValue::undefined));
    let function = get_object_property(global_object, table, &Identifier::from_span(global_object.vm(), name.as_bytes()))?;
    let data = get_call_data(function);
    match call_function(global_object, function, &data, call.this_value(), call.arguments()) {
        Ok(value) => Ok(value),
        Err(LLIntFailure::Thrown) => Err(Thrown::Pending),
        Err(_) => Err(Thrown::Unported("node:fs")),
    }
}
host_function!(forward_function, forward_body);

/// Monta o objeto do programa escondido (uma vez por programa) e devolve a sua tabela de exports.
fn implementation(global_object: &JSGlobalObject) -> Result<JSValue, Thrown> {
    if let Some(table) = IMPLEMENTATION.with(|slot| *slot.borrow()) {
        return Ok(table);
    }
    let realm = crate::runtime::js_module_record::realm();
    let factory = evaluate_hidden_program(&realm, PRELUDE).map_err(|_| Thrown::Unported("node:fs: o programa escondido lançou"))?;
    let data = get_call_data(factory);
    let table = match call_function(global_object, factory, &data, JSValue::undefined(), &[binding(global_object)]) {
        Ok(table) => table,
        Err(LLIntFailure::Thrown) => return Err(Thrown::Pending),
        Err(_) => return Err(Thrown::Unported("node:fs")),
    };
    IMPLEMENTATION.with(|slot| *slot.borrow_mut() = Some(table));
    Ok(table)
}

/// Põe `value` (uma função do programa escondido, ou um dado embrulhado em `{ v }`) em `target` sob `identifier`; a
/// função vira uma nativa que repassa a chamada à entrada `table_name` da tabela.
fn install_entry(global_object: &JSGlobalObject, target: &JSObject, identifier: &Identifier, table_name: &str, value: JSValue) -> Result<(), Thrown> {
    let vm = global_object.vm();
    if matches!(get_call_data(value), CallData::None) {
        // Um dado do módulo (`constants`, `Stats`, `F_OK`...) vem embrulhado em `{ v }`.
        let wrapped = get_object_property(global_object, value, &Identifier::from_span(vm, b"v"))?;
        target.put_direct(vm, &PropertyName::from_identifier(identifier), wrapped, 0);
        return Ok(());
    }
    let length = pending_or(global_object, get_object_property(global_object, value, &Identifier::from_span(vm, b"length"))?.to_number())? as u32;
    let native = put_direct_native_function_with_display_name(
        vm,
        global_object,
        target,
        identifier,
        &WtfString::from_utf8(identifier.utf8().as_slice()),
        length,
        forward_function,
        ImplementationVisibility::Public,
        Intrinsic::NoIntrinsic,
        0,
    );
    if let JSValue::Cell(cell_id) = native.as_value() {
        SHIMS.with(|shims| shims.borrow_mut().insert(cell_id, table_name.to_owned()));
    }
    Ok(())
}

/// Instalador do registro: o objeto do módulo, com as funções como nativas e o resto como dado. As entradas
/// `promises.X` da tabela formam o `fs.promises`, que ocupa o lugar de `promises` na ordem das chaves.
pub(crate) fn install_fs_module(global_object: &JSGlobalObject) -> HostResult {
    let vm = global_object.vm();
    let table = implementation(global_object)?;
    let module = construct_empty_object(global_object);
    let promises = construct_empty_object(global_object);
    for (identifier, value) in enumerable_own_entries(global_object, table)? {
        let name = String::from_utf8_lossy(&identifier.utf8()).into_owned();
        if name == "promises" {
            module.put_direct(vm, &PropertyName::from_identifier(&identifier), promises.as_value(), 0);
        } else if let Some(member) = name.strip_prefix("promises.") {
            install_entry(global_object, &promises, &Identifier::from_span(vm, member.as_bytes()), &name, value)?;
        } else {
            install_entry(global_object, &module, &identifier, &name, value)?;
        }
    }
    Ok(module.as_value())
}

/// Instalador de `fs/promises`: o mesmo objeto de `require("fs").promises`.
pub(crate) fn install_fs_promises_module(global_object: &JSGlobalObject) -> HostResult {
    let crate::api::builtin_modules::Resolved::Builtin(entry) = crate::api::builtin_modules::resolve("fs") else {
        return Ok(JSValue::undefined());
    };
    let module = crate::api::builtin_modules::load(global_object, entry)?;
    get_object_property(global_object, module, &Identifier::from_span(global_object.vm(), b"promises"))
}
