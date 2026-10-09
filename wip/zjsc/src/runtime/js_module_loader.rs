//! Tradução de `runtime/JSModuleLoader.h` (o `enum Status`) e a parte do `JSModuleLoader.cpp` que o
//! carregamento de ponta a ponta precisa: `loadModule`/`hostLoadImportedModule` (carregar o grafo),
//! `requestImportModule`/`importModule` (o `import()`), `continueDynamicImport` (o encadeamento de
//! `dynamicImportLoadSettled`), `createImportMetaProperties`, `makeModule` (JavaScript, JSON e Text) e
//! `globalFuncImportModule` (`JSGlobalObjectFunctions.cpp`).
//!
//! DIVERGÊNCIAS e LACUNAS, e por quê:
//!
//! - Este JSC não tem `ModuleLoader.js` (o carregador é todo C++: `ModuleRegistryEntry`,
//!   `ModuleLoadingContext`, `ModuleGraphLoadingState`, `ModuleLoaderPayload`, com `JSPromise` em cada
//!   passo). Aqui o carregamento do grafo é síncrono sobre um [`ModuleHost`] (a `moduleLoaderResolve`/
//!   `moduleLoaderFetch` do `GlobalObjectMethodTable`): o host resolve o especificador e entrega o fonte, e
//!   o registro (`ModuleRegistryEntry`) é um `HashMap` por chave e tipo. O que o carregamento assíncrono
//!   acrescenta (`innerModuleLoading` com `pendingModulesCount`, `finishLoadingImportedModule`,
//!   `ModuleFailure`/`attachErrorInfo`, `ImportMap`, as tarefas `Module*` e `DynamicImportLoadSettled`)
//!   não existe; o grafo é carregado em profundidade e o primeiro erro rejeita a promessa interna do
//!   `import()`. A avaliação, porém, é a do C++: `link`, `evaluate` (com top-level await) e o
//!   encadeamento `DynamicImportEvaluateSettled` -> `ImportModuleNamespace` (em `js_microtask.rs`) que
//!   resolve a promessa que o `import()` devolve.
//! - O tipo do módulo é o `SourceProviderSourceType` que o host decide ([`ModuleHost::source_type`], o
//!   `sourceCode.provider()->sourceType()` de `makeModule`): JavaScript, JSON e Text. WebAssembly e
//!   `HostDefined` sem tipo conhecido devolvem `TypeError`.
//! - O referrer do `import()` é a `SourceOrigin` do executável chamador (`callerSourceOrigin`, ver
//!   `interpreter/caller_source_origin.rs`): os módulos nascem com a origem da própria chave, então o
//!   `import()` feito por uma função, um callback ou depois de um `await` ainda resolve contra o módulo
//!   onde foi escrito. Código que não é módulo (origem nula) resolve sem referrer.
//! - `import.meta` é um objeto de protótipo nulo com `url` (a `moduleLoaderCreateImportMetaProperties`
//!   padrão do C++ devolve um objeto vazio; o `url` vem do `ModuleHost::import_meta_url`).
//! - O `referrerAsyncOrder` do `import()` é o `asyncEvaluationOrder` do módulo referrer (o que o hook
//!   `moduleLoaderImportModule` do Bun calcula), `-1` sem referrer ou fora de avaliação assíncrona.
//! - O loader vive num `thread_local` (um `VM` por thread, um realm), como o registro de células.

use crate::runtime::js_promise_host::PromiseHost;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::bytecode::bytecode_intrinsics_table::LinkTimeConstant;
use crate::interpreter::call_frame::NativeCallFrame;
use crate::interpreter::caller_source_origin::caller_source_origin;
use crate::parser::nodes::ModuleProgramNode;
use crate::parser::parser_error::ParserError;
use crate::parser::parser_modes::{
    JSParserBuiltinMode, JSParserScriptMode, SourceParseMode, STRICT_MODE_LEXICALLY_SCOPED_FEATURE,
};
use crate::parser::parser::parse_root_node;
use crate::parser::source_code::make_source;
use crate::parser::source_provider::SourceProviderSourceType;
use crate::parser::source_tainted_origin::SourceTaintedOrigin;
use crate::runtime::abstract_module_record::AbstractModuleRecordRef;
use crate::runtime::constructor_kind::ConstructorKind;
use crate::runtime::enumeration_mode::{DontEnumPropertiesMode, PrivateSymbolMode, PropertyNameMode};
use crate::runtime::error::{create_error, create_syntax_error, create_type_error};
use crate::runtime::node_error::throw_native_type_error;
use crate::runtime::error_type::ErrorType;
use crate::runtime::host_call::Thrown;
use crate::runtime::identifier::Identifier;
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::iterator_operations::get_value_property;
use crate::runtime::js_function::{call_host_function_as_constructor, JSFunction};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_microtask::{resolve_deferred_import_namespace, take_pending_exception};
use crate::runtime::js_module_record::{ModuleResult, Status as RecordStatus};
use crate::runtime::js_object::{JSFinalObject, JSObject, JSObjectRef};
use crate::runtime::js_typeof::js_type_string_for_value;
use crate::runtime::structure::StructureRef;
use crate::runtime::vm::VM;
use crate::runtime::js_promise::{JSPromise, JSPromiseRef, Status as PromiseStatus};
use crate::runtime::timers::{event_loop_is_driven, schedule_module_load};
use crate::runtime::js_promise_combinators_context::JSPromiseCombinatorsGlobalContext;
use crate::runtime::js_string::js_string;
use crate::runtime::js_value::{js_null, EncodedJSValue, JSValue};
use crate::runtime::microtask::InternalMicrotask;
use crate::runtime::module_analyzer::ModuleAnalyzer;
use crate::runtime::own_property_names::get_own_property_names;
use crate::runtime::property_attribute::DONT_ENUM;
use crate::runtime::property_name::PropertyName;
use crate::runtime::property_name_array::PropertyNameArrayBuilder;
use crate::runtime::script_fetch_parameters::{ScriptFetchParameters, ScriptFetchParametersRef, ScriptFetchParametersType};
use crate::runtime::source_origin::SourceOrigin;
use crate::runtime::string_prototype_natives::to_wtf_string_or_type_error;
use crate::runtime::synthetic_module_record::{create_text_module, parse_json_module, try_create_with_export_names_and_values};
use crate::runtime::throw_scope::{throw_exception, ThrowScope};
use crate::wtf::text::text_position::TextPosition;
use crate::wtf::text::conversion_mode::ConversionMode;
use crate::wtf::text::wtf_string::{String as WtfString};
use crate::wtf::url::URL;

/// `JSModuleLoader::Status` (enum sem classe, `int`): começa em 1.
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Fetch = 1,
    Instantiate = 2,
    Satisfy = 3,
    Link = 4,
    Ready = 5,
}

/// O que o embedder fornece ao carregador: `moduleLoaderResolve`, `moduleLoaderFetch` e
/// `moduleLoaderCreateImportMetaProperties` do `GlobalObjectMethodTable`.
pub trait ModuleHost {
    /// `moduleLoaderResolve(globalObject, loader, name, referrer, ...)`: a chave canônica do módulo.
    /// `referrer` é a chave do módulo que pede, ou `None` (ponto de entrada ou `import()` fora de módulo).
    fn resolve(&self, specifier: &str, referrer: Option<&str>) -> Result<String, String>;

    /// Os exports (nomes e valores) de um módulo embutido da chave `key`, ou `None` quando `key` não é embutido e o
    /// módulo vem de `fetch`. O padrão não tem embutidos.
    fn builtin_exports(&self, global_object: &JSGlobalObject, key: &str) -> Option<Result<(Vec<Identifier>, Vec<JSValue>), Thrown>> {
        let _ = (global_object, key);
        None
    }

    /// `moduleLoaderFetch`: o fonte do módulo de chave `key` (JavaScript, o texto de um JSON ou o texto de
    /// um módulo Text).
    fn fetch(&self, key: &str) -> Result<String, String>;

    /// O `import.meta.url` de `key`. O padrão é a própria chave.
    fn import_meta_url(&self, key: &str) -> String {
        key.to_owned()
    }

    /// O `import.meta.main` de `key`: verdadeiro para o ponto de entrada do programa (a extensão do Bun
    /// sobre o `createImportMetaProperties` do JSC). O padrão é falso.
    fn is_main_module(&self, key: &str) -> bool {
        let _ = key;
        false
    }

    /// O `import.meta.resolve(specifier)` (URL, `sync` falso) e o `import.meta.resolveSync(specifier)`
    /// (caminho, `sync` verdadeiro) de quem tem a chave `referrer`. O padrão resolve pelo host e mostra a
    /// chave como `import_meta_url` (menos o `file://` no caso síncrono). O Bun não confere a existência do
    /// arquivo em `resolve`; um host que sabe disso sobrescreve.
    fn import_meta_resolve(&self, specifier: &str, referrer: &str, sync: bool) -> Result<String, String> {
        let key = self.resolve(specifier, Some(referrer))?;
        let url = self.import_meta_url(&key);
        Ok(if sync { url.strip_prefix("file://").map_or(url.clone(), str::to_owned) } else { url })
    }

    /// O `sourceCode.provider()->sourceType()` do fonte de `key`, que `makeModule` usa para escolher entre
    /// `parseJSONModule`, `createTextModule` e o parser de módulo. `request_type` é o tipo do atributo
    /// `type` do pedido e `host_defined_type` o texto do tipo `HostDefined` (vazio nos demais). O padrão
    /// segue o atributo: `json` é JSON, `text` (o tipo que o Bun entrega ao host) é Text, o resto é módulo;
    /// como no Bun, uma chave que termina em `.json` é JSON mesmo sem o atributo.
    fn source_type(&self, key: &str, request_type: ScriptFetchParametersType, host_defined_type: &str) -> SourceProviderSourceType {
        if key.ends_with(".json") {
            return SourceProviderSourceType::JSON;
        }
        match request_type {
            ScriptFetchParametersType::JSON => SourceProviderSourceType::JSON,
            ScriptFetchParametersType::Text => SourceProviderSourceType::Text,
            ScriptFetchParametersType::HostDefined if host_defined_type == "text" => SourceProviderSourceType::Text,
            _ => SourceProviderSourceType::Module,
        }
    }
}

/// `JSModuleLoader` mais o `ModuleRegistry`: o host e o `Map` (chave, tipo) -> registro.
struct ModuleLoader {
    host: Rc<dyn ModuleHost>,
    registry: RefCell<HashMap<(String, ScriptFetchParametersType), AbstractModuleRecordRef>>,
}

thread_local! {
    static LOADER: RefCell<Option<Rc<ModuleLoader>>> = const { RefCell::new(None) };
    /// A `Structure` das instâncias de cada classe de mensagem do bun do realm em curso (`RESOLVE`, `BUILD`;
    /// o protótipo é o `.prototype` da classe).
    static MESSAGE_STRUCTURES: RefCell<[Option<StructureRef>; 2]> = const { RefCell::new([None, None]) };
    /// O estado de cada instância de `ResolveMessage` e `BuildMessage` (a chave é o valor codificado da instância).
    static MESSAGE_STATES: RefCell<HashMap<EncodedJSValue, MessageState>> = RefCell::new(HashMap::new());
    /// Os `import.meta` já criados (o valor codificado de cada objeto): `import.meta.resolve` só vale com
    /// `this` sendo um deles (no Bun, `TypeError: import.meta.resolve must be bound to an import.meta object`).
    static IMPORT_META_OBJECTS: RefCell<std::collections::HashSet<EncodedJSValue>> = RefCell::new(std::collections::HashSet::new());
}

/// Índice (e parâmetro const dos nativos) da classe `ResolveMessage`.
const RESOLVE: usize = 0;
/// Índice (e parâmetro const dos nativos) da classe `BuildMessage`.
const BUILD: usize = 1;
const MESSAGE_CLASS_NAMES: [&str; 2] = ["ResolveMessage", "BuildMessage"];

/// O estado de uma instância de `ResolveMessage` ou `BuildMessage` (o que o bun guarda na parte nativa do objeto).
#[derive(Clone)]
struct MessageState {
    /// `RESOLVE` ou `BUILD`: a classe da instância.
    kind: usize,
    message: String,
    code: String,
    specifier: String,
    referrer: String,
    /// O `importKind` de uma `ResolveMessage`: `import-statement` (ESM) ou `require-call` (o `require` do CJS).
    import_kind: &'static str,
    /// O que o programa gravou em `message` e `stack` (os acessores têm setter; `toString` e `toJSON` seguem o original).
    message_override: Option<JSValue>,
    stack_override: Option<JSValue>,
}

impl MessageState {
    /// O texto de `stack` e de `toString()`.
    fn text(&self) -> String {
        format!("{}: {}", MESSAGE_CLASS_NAMES[self.kind], self.message)
    }
}

fn js_text(vm: &VM, text: &str) -> JSValue {
    JSValue::from_js_string(js_string(vm, &WtfString::from_utf8(text.as_bytes())))
}

fn property_key(vm: &VM, name: &str) -> PropertyName {
    PropertyName::from_identifier(&Identifier::from_span(vm, name.as_bytes()))
}

/// O estado de `this` quando é uma instância da classe `kind` (a de outra classe não vale).
fn with_message_state<R>(kind: usize, this: JSValue, pick: impl FnOnce(&MessageState) -> R) -> Option<R> {
    MESSAGE_STATES.with(|states| states.borrow().get(&this.encode()).filter(|state| state.kind == kind).map(pick))
}

/// Como o bun descreve o `this` recebido na mensagem de `ERR_INVALID_THIS`; `undefined` não acrescenta nada.
pub(crate) fn describe_received(global_object: &JSGlobalObject, value: JSValue) -> Option<String> {
    let vm = global_object.vm();
    if value.is_undefined() {
        return None;
    }
    if value.is_null() {
        return Some("null".to_owned());
    }
    let kind = js_type_string_for_value(value);
    let object = JSObject::from_value(&value);
    match (kind, object) {
        ("function", Some(function)) => {
            let name = function.get(vm, &property_key(vm, "name"));
            Some(format!("function {}", if name.is_string() { rust_string(&name.to_wtf_string()) } else { String::new() }))
        }
        ("object", Some(object)) => {
            let constructor = object.get(vm, &property_key(vm, "constructor"));
            let name = JSObject::from_value(&constructor)
                .map(|constructor| constructor.get(vm, &property_key(vm, "name")))
                .filter(|name| name.is_string())
                .map_or_else(|| "Object".to_owned(), |name| rust_string(&name.to_wtf_string()));
            Some(format!("an instance of {name}"))
        }
        ("string", _) => Some(format!("type string ('{}')", rust_string(&value.to_wtf_string()))),
        ("bigint", _) => Some(format!("type bigint ({}n)", rust_string(&value.to_wtf_string()))),
        ("symbol", _) => {
            let text = crate::runtime::symbol::as_symbol(value).try_get_descriptive_string().unwrap_or_default();
            Some(format!("type symbol ({})", rust_string(&text)))
        }
        (other, _) => Some(format!("type {other} ({})", rust_string(&value.to_wtf_string()))),
    }
}

/// `TypeError` com `code` (os erros nativos do bun), devolvendo o valor nulo do host.
fn throw_coded_type_error(global_object: &JSGlobalObject, message: &str, code: &str) -> EncodedJSValue {
    let _pending = crate::runtime::node_error::throw_coded_type_error(global_object, message, code);
    JSValue::undefined().encode()
}

/// `ERR_INVALID_THIS` do bun: `TypeError` com `code`.
fn throw_invalid_this(global_object: &JSGlobalObject, kind: usize, this: JSValue) -> EncodedJSValue {
    let mut message = format!("Expected this to be instanceof {}", MESSAGE_CLASS_NAMES[kind]);
    if let Some(received) = describe_received(global_object, this) {
        message.push_str(", but received ");
        message.push_str(&received);
    }
    throw_coded_type_error(global_object, &message, "ERR_INVALID_THIS")
}

/// O construtor chamado sem `new`.
fn message_call<const KIND: usize>(global_object: &JSGlobalObject, _call_frame: &mut NativeCallFrame<'_>) -> EncodedJSValue {
    throw_coded_type_error(global_object, &format!("{} constructor cannot be invoked without 'new'", MESSAGE_CLASS_NAMES[KIND]), "ERR_ILLEGAL_CONSTRUCTOR")
}

/// O construtor chamado com `new`: a classe não é construível pelo programa.
fn message_construct(global_object: &JSGlobalObject, _call_frame: &mut NativeCallFrame<'_>) -> EncodedJSValue {
    throw_coded_type_error(global_object, "Illegal constructor", "ERR_ILLEGAL_CONSTRUCTOR")
}

/// Os setters de `message` e `stack`: guardam o valor do programa sem tocar no texto original.
fn store_message_override<const KIND: usize>(
    global_object: &JSGlobalObject,
    call_frame: &mut NativeCallFrame<'_>,
    label: &str,
    store: fn(&mut MessageState, JSValue),
) -> EncodedJSValue {
    let this = call_frame.this_value();
    let value = call_frame.argument(0);
    let stored = MESSAGE_STATES.with(|states| states.borrow_mut().get_mut(&this.encode()).filter(|state| state.kind == KIND).map(|state| store(state, value)));
    if stored.is_none() {
        throw_native_type_error(global_object, &format!("The {}.{label} setter can only be used on instances of {}", MESSAGE_CLASS_NAMES[KIND], MESSAGE_CLASS_NAMES[KIND]));
    }
    JSValue::undefined().encode()
}

fn message_set_message<const KIND: usize>(global_object: &JSGlobalObject, call_frame: &mut NativeCallFrame<'_>) -> EncodedJSValue {
    store_message_override::<KIND>(global_object, call_frame, "message", |state, value| state.message_override = Some(value))
}

fn message_set_stack<const KIND: usize>(global_object: &JSGlobalObject, call_frame: &mut NativeCallFrame<'_>) -> EncodedJSValue {
    store_message_override::<KIND>(global_object, call_frame, "stack", |state, value| state.stack_override = Some(value))
}

/// Os acessores do protótipo de uma classe de mensagem, na ordem em que o bun os instala.
macro_rules! message_getters {
    ($table:ident, $kind:expr, $class:literal; $($function:ident, $label:literal, $value:expr;)*) => {
        $(
            fn $function(global_object: &JSGlobalObject, call_frame: &mut NativeCallFrame<'_>) -> EncodedJSValue {
                let make: fn(&JSGlobalObject, &MessageState) -> JSValue = $value;
                match with_message_state($kind, call_frame.this_value(), |state| make(global_object, state)) {
                    Some(value) => value.encode(),
                    None => {
                        throw_native_type_error(
                            global_object,
                            concat!("The ", $class, ".", $label, " getter can only be used on instances of ", $class),
                        );
                        JSValue::undefined().encode()
                    }
                }
            }
        )*
        const $table: &[(&str, NativeFn)] = &[$(($label, $function as NativeFn)),*];
    };
}

message_getters! {
    RESOLVE_MESSAGE_GETTERS, RESOLVE, "ResolveMessage";
    resolve_message_code, "code", |g, state| js_text(g.vm(), &state.code);
    resolve_message_column, "column", |_g, _state| JSValue::Int32(0);
    resolve_message_import_kind, "importKind", |g, state| js_text(g.vm(), state.import_kind);
    resolve_message_level, "level", |g, _state| js_text(g.vm(), "error");
    resolve_message_line, "line", |_g, _state| JSValue::Int32(0);
    resolve_message_message, "message", |g, state| state.message_override.unwrap_or_else(|| js_text(g.vm(), &state.message));
    resolve_message_position, "position", |_g, _state| JSValue::null();
    resolve_message_referrer, "referrer", |g, state| js_text(g.vm(), &state.referrer);
    resolve_message_require_stack, "requireStack", |_g, _state| JSValue::undefined();
    resolve_message_specifier, "specifier", |g, state| js_text(g.vm(), &state.specifier);
    resolve_message_stack, "stack", |g, state| state.stack_override.unwrap_or_else(|| js_text(g.vm(), &state.text()));
}

message_getters! {
    BUILD_MESSAGE_GETTERS, BUILD, "BuildMessage";
    build_message_column, "column", |_g, _state| JSValue::Int32(0);
    build_message_level, "level", |g, _state| js_text(g.vm(), "error");
    build_message_line, "line", |_g, _state| JSValue::Int32(0);
    build_message_message, "message", |g, state| state.message_override.unwrap_or_else(|| js_text(g.vm(), &state.message));
    build_message_notes, "notes", |g, _state| crate::runtime::js_array::construct_array(g.vm(), &g.array_structure(), &[]).as_value();
    build_message_position, "position", |_g, _state| JSValue::null();
}

/// `toJSON` de uma classe de mensagem: os campos comuns, e os do resolvedor só em `ResolveMessage`.
fn message_to_json<const KIND: usize>(global_object: &JSGlobalObject, call_frame: &mut NativeCallFrame<'_>) -> EncodedJSValue {
    let vm = global_object.vm();
    let this = call_frame.this_value();
    let Some(state) = with_message_state(KIND, this, MessageState::clone) else {
        return throw_invalid_this(global_object, KIND, this);
    };
    let object = JSFinalObject::create(vm, &global_object.object_structure_for_object_constructor());
    let mut fields = vec![
        ("name", js_text(vm, MESSAGE_CLASS_NAMES[KIND])),
        ("position", JSValue::null()),
        ("message", js_text(vm, &state.message)),
        ("level", js_text(vm, "error")),
    ];
    if KIND == RESOLVE {
        fields.extend([
            ("specifier", js_text(vm, &state.specifier)),
            ("importKind", js_text(vm, state.import_kind)),
            ("referrer", js_text(vm, &state.referrer)),
        ]);
    }
    for (name, value) in fields {
        object.put_direct(vm, &property_key(vm, name), value, 0);
    }
    object.as_value().encode()
}

/// `toString` de uma classe de mensagem.
fn message_to_string<const KIND: usize>(global_object: &JSGlobalObject, call_frame: &mut NativeCallFrame<'_>) -> EncodedJSValue {
    let this = call_frame.this_value();
    match with_message_state(KIND, this, MessageState::text) {
        Some(text) => js_text(global_object.vm(), &text).encode(),
        None => throw_invalid_this(global_object, KIND, this),
    }
}

/// `[Symbol.toPrimitive]` de uma classe de mensagem: `"string"` e `"default"` dão o texto, qualquer outra dica dá `null`.
fn message_to_primitive<const KIND: usize>(global_object: &JSGlobalObject, call_frame: &mut NativeCallFrame<'_>) -> EncodedJSValue {
    let this = call_frame.this_value();
    let hint = call_frame.argument(0);
    let wants_text = hint.is_string() && matches!(rust_string(&hint.to_wtf_string()).as_str(), "string" | "default");
    match with_message_state(KIND, this, MessageState::text) {
        Some(text) if wants_text => js_text(global_object.vm(), &text).encode(),
        Some(_) => JSValue::null().encode(),
        None => throw_invalid_this(global_object, KIND, this),
    }
}

/// O que difere entre as classes de mensagem do bun: os acessores, os setters e as funções genéricas
/// instanciadas para a classe; o resto da instalação é comum.
struct MessageClassSpec {
    getters: &'static [(&'static str, NativeFn)],
    setters: &'static [(&'static str, NativeFn)],
    call: NativeFn,
    to_json: NativeFn,
    to_string: NativeFn,
    to_primitive: NativeFn,
}

/// Indexada por `RESOLVE` e `BUILD`. `ResolveMessage` tem setter em `message` e `stack`; `BuildMessage` só em `message`.
const MESSAGE_CLASS_SPECS: [MessageClassSpec; 2] = [
    MessageClassSpec {
        getters: RESOLVE_MESSAGE_GETTERS,
        setters: &[("message", message_set_message::<RESOLVE> as NativeFn), ("stack", message_set_stack::<RESOLVE> as NativeFn)],
        call: message_call::<RESOLVE> as NativeFn,
        to_json: message_to_json::<RESOLVE> as NativeFn,
        to_string: message_to_string::<RESOLVE> as NativeFn,
        to_primitive: message_to_primitive::<RESOLVE> as NativeFn,
    },
    MessageClassSpec {
        getters: BUILD_MESSAGE_GETTERS,
        setters: &[("message", message_set_message::<BUILD> as NativeFn)],
        call: message_call::<BUILD> as NativeFn,
        to_json: message_to_json::<BUILD> as NativeFn,
        to_string: message_to_string::<BUILD> as NativeFn,
        to_primitive: message_to_primitive::<BUILD> as NativeFn,
    },
];

/// As classes `ResolveMessage` e `BuildMessage` do bun (a camada de resolução e de leitura dele, fora do
/// JavaScriptCore), nativas e globais do realm desde a criação: construtor `[native code]` que não constrói,
/// protótipo que herda de `Error.prototype` com acessores nativos, e instâncias sem propriedades próprias.
/// Medido no bun 1.4.2.
pub fn install_message_classes(global_object: &JSGlobalObject) {
    use crate::runtime::js_getter_setter::GetterSetter;
    use crate::runtime::property_attribute::{ACCESSOR, DONT_DELETE, READ_ONLY};
    use crate::runtime::property_descriptor::PropertyDescriptor;

    let vm = global_object.vm();
    let native = |name: &str, length: u32, function: NativeFn, construct: NativeFn| {
        JSFunction::create_native(
            vm,
            global_object,
            length,
            &WtfString::from_latin1(name.as_bytes()),
            function,
            ImplementationVisibility::Public,
            Intrinsic::NoIntrinsic,
            construct,
        )
    };
    let accessor_name = |prefix: &str, name: &str| js_text(vm, &format!("{prefix} {name}"));
    let name_key = PropertyName::from_identifier(&vm.property_names.name);
    let error_prototype = global_object.error_structure_for(ErrorType::Error).stored_prototype();
    for (kind, spec) in MESSAGE_CLASS_SPECS.iter().enumerate() {
        let class_name = MESSAGE_CLASS_NAMES[kind];
        let prototype_structure = JSFinalObject::create_structure(vm, Some(global_object), error_prototype.clone(), JSFinalObject::DEFAULT_INLINE_CAPACITY);
        let prototype = JSFinalObject::create(vm, &prototype_structure);
        let constructor = native(class_name, 0, spec.call, message_construct);

        for (name, function) in spec.getters {
            let getter = native(name, 0, *function, call_host_function_as_constructor);
            // O `name` do acessor é `get <nome>`, mas o `toString` do executável nativo traz só `<nome>`.
            let _ = getter.define_own_property(global_object, &name_key, &PropertyDescriptor::new(accessor_name("get", name), READ_ONLY | DONT_ENUM), false);
            let setter = spec.setters.iter().find(|(setter_name, _)| setter_name == name).map(|(_, function)| {
                let setter = native(name, 1, *function, call_host_function_as_constructor);
                let _ = setter.define_own_property(global_object, &name_key, &PropertyDescriptor::new(accessor_name("set", name), READ_ONLY | DONT_ENUM), false);
                setter.as_value()
            });
            let accessor = GetterSetter::create_from_values(vm, getter.as_value(), setter.unwrap_or_else(JSValue::undefined));
            let _ = prototype.put_direct_accessor(vm, &property_key(vm, name), accessor, ACCESSOR | DONT_DELETE);
        }
        for (name, function) in [("toJSON", spec.to_json), ("toString", spec.to_string)] {
            prototype.put_direct(vm, &property_key(vm, name), native(name, 0, function, call_host_function_as_constructor).as_value(), DONT_DELETE);
        }
        prototype.put_direct(vm, &property_key(vm, "name"), js_text(vm, class_name), READ_ONLY);
        prototype.put_direct(vm, &property_key(vm, "constructor"), constructor.as_value(), DONT_ENUM);
        let to_primitive = native("toPrimitive", 1, spec.to_primitive, call_host_function_as_constructor);
        prototype.put_direct(vm, &PropertyName::from_identifier(&vm.property_names.to_primitive_symbol), to_primitive.as_value(), READ_ONLY | DONT_ENUM);
        prototype.put_direct(vm, &PropertyName::from_identifier(&vm.property_names.to_string_tag_symbol), js_text(vm, class_name), READ_ONLY | DONT_ENUM);
        constructor.put_direct(vm, &PropertyName::from_identifier(&vm.property_names.prototype), prototype.as_value(), READ_ONLY | DONT_ENUM | DONT_DELETE);
        // No próprio objeto global: `globalThis` é um `JSGlobalProxy`, e `putDirect` nele não encaminha ao alvo.
        global_object.put_direct(vm, &property_key(vm, class_name), constructor.as_value(), 0);
        let instance_structure = JSFinalObject::create_structure(vm, Some(global_object), prototype.as_value(), JSFinalObject::DEFAULT_INLINE_CAPACITY);
        MESSAGE_STRUCTURES.with(|slots| slots.borrow_mut()[kind] = Some(instance_structure));
    }
}

/// Literal de string JavaScript de `text`, só com ASCII (o resto sai como `\uXXXX`).
fn js_string_literal(text: &str) -> String {
    let mut quoted = String::from("\"");
    for unit in text.encode_utf16() {
        match unit {
            0x22 => quoted.push_str("\\\""),
            0x5c => quoted.push_str("\\\\"),
            0x20..=0x7e => quoted.push(unit as u8 as char),
            other => quoted.push_str(&format!("\\u{other:04x}")),
        }
    }
    quoted.push('"');
    quoted
}

/// O caminho do arquivo que a `SourceOrigin` do chamador descreve (`file:///a/b.js` vira `/a/b.js`), ou `/` sem origem.
fn referrer_path(referrer: Option<&str>) -> String {
    match referrer {
        Some(origin) if !origin.is_empty() => origin.strip_prefix("file://").unwrap_or(origin).to_owned(),
        _ => "/".to_owned(),
    }
}

/// A mensagem e o código que o resolvedor do bun dá a `specifier` que não resolve: `node:` desconhecido é
/// `ERR_UNKNOWN_BUILTIN_MODULE`, caminho (relativo, absoluto ou `file:`) é `Cannot find module`, o resto
/// (especificador "nu") é `Cannot find package`.
fn unresolved_specifier_message(specifier: &str, referrer: &str) -> (String, &'static str, String) {
    if specifier.starts_with("node:") {
        return (format!("No such built-in module: {specifier}"), "ERR_UNKNOWN_BUILTIN_MODULE", specifier.to_owned());
    }
    let shown = specifier.strip_prefix("file://").unwrap_or(specifier);
    let is_path = shown.starts_with("./") || shown.starts_with("../") || shown.starts_with('/') || shown.starts_with('\\') || shown.is_empty() || shown == "." || shown == "..";
    if is_path {
        return (format!("Cannot find module '{shown}' imported from {referrer}"), "ERR_MODULE_NOT_FOUND", shown.to_owned());
    }
    // O nome do pacote é o primeiro segmento (dois, quando tem escopo `@a/b`).
    let segments = if shown.starts_with('@') { 2 } else { 1 };
    let name = shown.splitn(segments + 1, '/').take(segments).collect::<Vec<_>>().join("/");
    (format!("Cannot find package '{name}' imported from {referrer}"), "ERR_MODULE_NOT_FOUND", specifier.to_owned())
}

/// Lança uma instância de `ResolveMessage` ou `BuildMessage` com `state` e devolve `Thrown::Pending`; sem a
/// classe instalada no realm, cai no `Error` simples com a mesma mensagem.
fn throw_message(global_object: &JSGlobalObject, state: MessageState) -> Thrown {
    let vm = global_object.vm();
    let structure = MESSAGE_STRUCTURES.with(|slots| slots.borrow()[state.kind].clone());
    let Some(structure) = structure else {
        return throw_error(global_object, &state.message);
    };
    let error = JSFinalObject::create(vm, &structure).as_value();
    MESSAGE_STATES.with(|states| states.borrow_mut().insert(error.encode(), state));
    let mut scope = ThrowScope::new(vm);
    throw_exception(global_object, &mut scope, error);
    Thrown::Pending
}

/// Lança um `ResolveMessage` do bun (`message`, `code`).
fn throw_resolve_message(global_object: &JSGlobalObject, message: &str, code: &str, specifier: &str, referrer: &str) -> Thrown {
    let state = MessageState {
        kind: RESOLVE,
        message: message.to_owned(),
        code: code.to_owned(),
        specifier: specifier.to_owned(),
        referrer: referrer.to_owned(),
        import_kind: "import-statement",
        message_override: None,
        stack_override: None,
    };
    throw_message(global_object, state)
}

/// A falha do `require(id)` e do `require.resolve(request)` do CJS, quando não há módulo que resolva (medido no bun
/// 1.4.2): argumento que não é string é `TypeError` `ERR_INVALID_ARG_TYPE` (o argumento se chama `id` em `require` e
/// `request` em `require.resolve`), string vazia é `ERR_INVALID_ARG_VALUE`, e o resto é um `ResolveMessage` com a
/// "Require stack" de `filename`: em `require`, `ERR_UNKNOWN_BUILTIN_MODULE` para `node:` desconhecido e
/// `MODULE_NOT_FOUND` no mais, com `importKind` `require-call`; em `require.resolve`, sempre `MODULE_NOT_FOUND`
/// ("Cannot find module 'node:x'") e `importKind` `require-resolve`.
pub(crate) fn throw_require_failure(global_object: &JSGlobalObject, argument: JSValue, resolve: bool, filename: &str) -> Thrown {
    let argument_name = if resolve { "request" } else { "id" };
    if !argument.is_string() {
        let received = describe_received(global_object, argument).unwrap_or_else(|| "undefined".to_owned());
        return crate::runtime::node_error::throw_coded_type_error(
            global_object,
            &format!("The \"{argument_name}\" argument must be of type string. Received {received}"),
            "ERR_INVALID_ARG_TYPE",
        );
    }
    let specifier = rust_string(&argument.to_wtf_string());
    if specifier.is_empty() {
        return crate::runtime::node_error::throw_coded_type_error(
            global_object,
            "The argument 'id' must be a non-empty string. Received ''",
            "ERR_INVALID_ARG_VALUE",
        );
    }
    let (message, code) = if specifier.starts_with("node:") && !resolve {
        (format!("No such built-in module: {specifier}"), "ERR_UNKNOWN_BUILTIN_MODULE")
    } else {
        (format!("Cannot find module '{specifier}'\nRequire stack:\n- {filename}"), "MODULE_NOT_FOUND")
    };
    let state = MessageState {
        kind: RESOLVE,
        message,
        code: code.to_owned(),
        specifier,
        referrer: filename.to_owned(),
        import_kind: if resolve { "require-resolve" } else { "require-call" },
        message_override: None,
        stack_override: None,
    };
    throw_message(global_object, state)
}

/// Quando o bun lê `specifier` como arquivo em vez de resolver (`//...`, a sintaxe de caminho UNC), a falha é um
/// `BuildMessage` do leitor: `EISDIR reading "<specifier>"` se o caminho é a raiz, `ENOENT reading "<specifier>"`
/// nos demais (sem host de arquivos, só a raiz existe).
fn build_message_text(specifier: &str) -> Option<String> {
    if !specifier.starts_with("//") {
        return None;
    }
    let code = if specifier.bytes().all(|byte| byte == b'/') { "EISDIR" } else { "ENOENT" };
    Some(format!("{code} reading \"{specifier}\""))
}

/// Lança um `BuildMessage` do bun com `message`.
pub(crate) fn throw_build_message(global_object: &JSGlobalObject, message: String) -> Thrown {
    let state = MessageState {
        kind: BUILD,
        message,
        code: String::new(),
        specifier: String::new(),
        referrer: String::new(),
        import_kind: "import-statement",
        message_override: None,
        stack_override: None,
    };
    throw_message(global_object, state)
}

/// Fim do programa (`cell_registry::reset_program_state`): o carregador pode guardar valores e globais do
/// programa.
pub(crate) fn reset_for_program() {
    let taken = LOADER.try_with(|loader| loader.borrow_mut().take());
    drop(taken);
    let _ = MESSAGE_STRUCTURES.try_with(|slots| *slots.borrow_mut() = [None, None]);
    let _ = MESSAGE_STATES.try_with(|states| states.borrow_mut().clear());
    let _ = PENDING_IMPORTS.try_with(|pending| pending.borrow_mut().clear());
}

fn loader() -> Option<Rc<ModuleLoader>> {
    LOADER.with(|loader| loader.borrow().clone())
}

pub(crate) fn rust_string(string: &WtfString) -> String {
    String::from_utf8_lossy(&string.utf8(ConversionMode::LenientConversion)).into_owned()
}

fn key_text(identifier: &Identifier) -> String {
    String::from_utf8_lossy(&identifier.utf8()).into_owned()
}

fn throw_type_error(global_object: &JSGlobalObject, message: &str) -> Thrown {
    let mut scope = ThrowScope::new(global_object.vm());
    throw_exception(global_object, &mut scope, create_type_error(global_object, &WtfString::from_utf8(message.as_bytes())));
    Thrown::Pending
}

pub(crate) fn throw_error(global_object: &JSGlobalObject, message: &str) -> Thrown {
    let mut scope = ThrowScope::new(global_object.vm());
    throw_exception(global_object, &mut scope, create_error(global_object, &WtfString::from_utf8(message.as_bytes())));
    Thrown::Pending
}

/// `installModuleLoader`: instala o host e a `LinkTimeConstant::ImportModule` (`importModule`, o alvo do
/// `ImportNode`).
pub fn install_module_loader(global_object: &JSGlobalObject, host: Rc<dyn ModuleHost>) {
    let new_loader = Rc::new(ModuleLoader { host, registry: RefCell::new(HashMap::new()) });
    LOADER.with(|loader| *loader.borrow_mut() = Some(new_loader));
    install_import_module_constant(global_object);
}

/// `m_linkTimeConstants[LinkTimeConstant::importModule].initLater(...)` (JSGlobalObject.cpp:2007): o upstream
/// o cria sempre, com ou sem host de módulos; sem host, o `import()` devolve uma promessa rejeitada. Por isso
/// `init_link_time_constants` também o instala, e um `import()` num global sem carregador não derruba a compilação.
pub fn install_import_module_constant(global_object: &JSGlobalObject) {
    let vm = global_object.vm();
    let function = JSFunction::create_native(
        vm,
        global_object,
        0,
        &WtfString::from_latin1(b"importModule"),
        global_func_import_module,
        ImplementationVisibility::Private,
        Intrinsic::NoIntrinsic,
        call_host_function_as_constructor,
    );
    global_object.set_link_time_constant(LinkTimeConstant::ImportModule, function.as_value());
}

impl ModuleLoader {
    /// `loadModule` + `hostLoadImportedModule` + `innerModuleLoading`: o registro do módulo `key` do tipo
    /// pedido por `parameters`, com o grafo todo carregado.
    fn load_module(
        &self,
        global_object: &JSGlobalObject,
        key: &str,
        parameters: Option<&ScriptFetchParametersRef>,
    ) -> ModuleResult<AbstractModuleRecordRef> {
        let request_type = parameters.map_or(ScriptFetchParametersType::JavaScript, |parameters| parameters.type_());
        let registry_key = (key.to_owned(), request_type);
        if let Some(record) = self.registry.borrow().get(&registry_key) {
            return Ok(Rc::clone(record));
        }

        let vm = global_object.vm();
        if let Some(exports) = self.host.builtin_exports(global_object, key) {
            let (names, values) = exports?;
            let module_key = Identifier::from_string(vm, &WtfString::from_utf8(key.as_bytes()));
            let record = try_create_with_export_names_and_values(global_object, &module_key, &names, &values, SourceProviderSourceType::Module)?;
            self.registry.borrow_mut().insert(registry_key, Rc::clone(&record));
            return Ok(record);
        }
        let text = match self.host.fetch(key) {
            Ok(text) => text,
            Err(message) => return Err(throw_error(global_object, &message)),
        };
        let host_defined_type = parameters.map(|parameters| rust_string(parameters.host_defined_import_type())).unwrap_or_default();
        let source_type = self.host.source_type(key, request_type, &host_defined_type);

        // `JSModuleLoader::makeModule`: o `SourceProviderSourceType` escolhe o tipo de registro. A origem do
        // fonte é a chave, que `callerSourceOrigin` devolve como referrer do `import()`.
        let source = make_source(
            &WtfString::from_utf8(text.as_bytes()),
            &SourceOrigin::new(URL::from_string(&WtfString::from_utf8(key.as_bytes()))),
            SourceTaintedOrigin::Untainted,
            WtfString::from_utf8(key.as_bytes()),
            TextPosition::default(),
            source_type,
        );
        let module_key = Identifier::from_string(vm, &WtfString::from_utf8(key.as_bytes()));

        match source_type {
            // https://tc39.es/proposal-json-modules/#sec-parse-json-module
            SourceProviderSourceType::JSON => {
                let record = parse_json_module(global_object, &module_key, &source)?;
                self.registry.borrow_mut().insert(registry_key, Rc::clone(&record));
                return Ok(record);
            }
            // https://tc39.es/proposal-import-text/#sec-create-text-module
            SourceProviderSourceType::Text => {
                let record = create_text_module(global_object, &module_key, &source)?;
                self.registry.borrow_mut().insert(registry_key, Rc::clone(&record));
                return Ok(record);
            }
            SourceProviderSourceType::Module => {}
            _ => return Err(throw_type_error(global_object, "Only JavaScript, JSON and Text modules are supported by this module host")),
        }

        let mut error = ParserError::new();
        let vm_rc = global_object.vm_rc();
        let root = parse_root_node::<ModuleProgramNode>(
            &vm_rc,
            &source,
            ImplementationVisibility::Public,
            JSParserBuiltinMode::NotBuiltin,
            STRICT_MODE_LEXICALLY_SCOPED_FEATURE,
            JSParserScriptMode::Module,
            SourceParseMode::ModuleAnalyzeMode,
            &mut error,
            ConstructorKind::None,
            None,
            None,
        );
        if error.is_valid() {
            let error_object = error.to_error_object(global_object, &source).expect("ParserError válido");
            let mut scope = ThrowScope::new(vm);
            throw_exception(global_object, &mut scope, error_object);
            return Err(Thrown::Pending);
        }
        let root = root.expect("parseRootNode sem erro devolveu nulo");

        let analyzer = ModuleAnalyzer::new(vm, &module_key, &source, root.base.features);
        let record = match analyzer.analyze(&root) {
            Ok(record) => record,
            Err((error_type, message)) => return Err(throw_analysis_error(global_object, error_type, &message)),
        };
        record.set_status(RecordStatus::Unlinked);
        // Registrar antes das dependências: o ciclo (`a` importa `b` importa `a`) acha o registro.
        self.registry.borrow_mut().insert(registry_key.clone(), Rc::clone(&record));

        let requests: Vec<_> = record.requested_modules().iter().cloned().collect();
        for request in &requests {
            let specifier = key_text(&request.specifier);
            let imported_key = match self.host.resolve(&specifier, Some(key)) {
                Ok(imported_key) => imported_key,
                Err(message) => {
                    self.registry.borrow_mut().remove(&registry_key);
                    return Err(throw_error(global_object, &message));
                }
            };
            match self.load_module(global_object, &imported_key, request.attributes.as_ref()) {
                Ok(imported) => record.set_imported_module(request, &imported),
                Err(error) => {
                    self.registry.borrow_mut().remove(&registry_key);
                    return Err(error);
                }
            }
        }
        Ok(record)
    }

    /// `continueDynamicImport` (o corpo de `dynamicImportLoadSettled` depois do carregamento): liga o
    /// módulo e avalia (ou, com `deferred`, avalia só as dependências assíncronas), e deixa a promessa
    /// interna `capability` liquidar com o namespace.
    fn continue_dynamic_import(
        &self,
        global_object: &JSGlobalObject,
        module: &AbstractModuleRecordRef,
        capability: &JSPromiseRef,
        deferred: bool,
        referrer_async_order: i64,
    ) -> ModuleResult<()> {
        // https://tc39.es/ecma262/#sec-ContinueDynamicImport
        // Step-6 linkAndEvaluateClosure
        // 6.a. Let link be Completion(module.Link()).
        // 6.b. If link is an abrupt completion, then 6.b.i. Perform ! Call(promiseCapability.[[Reject]], ...).
        if let Err(failure) = module.link(global_object) {
            return reject_with_failure(global_object, capability, failure);
        }

        if !deferred {
            // 6.c. Let evaluatePromise be module.Evaluate().
            let evaluate_promise = match module.evaluate_dynamic(global_object, referrer_async_order, Some(capability)) {
                Ok(promise) => promise,
                Err(failure) => return reject_with_failure(global_object, capability, failure),
            };
            // 6.d-f. Perform PerformPromiseThen(evaluatePromise, onFulfilled, onRejected).
            evaluate_promise.perform_promise_then_with_internal_microtask(
                global_object,
                InternalMicrotask::DynamicImportEvaluateSettled,
                Some(capability.cell_id()),
                module.as_value(),
                JSValue::empty(),
            );
            return Ok(());
        }

        // Deferred phase: do not evaluate the deferred root. Eagerly evaluate only the post-order list of
        // unexecuted top-level-await modules in the graph; once they all settle, hand back the deferred
        // namespace.
        let mut evaluation_list: Vec<AbstractModuleRecordRef> = Vec::new();
        let mut seen: Vec<AbstractModuleRecordRef> = Vec::new();
        module.gather_asynchronous_transitive_dependencies(&mut evaluation_list, &mut seen);

        // If evaluationList is empty, perform fulfilledClosure() and return.
        if evaluation_list.is_empty() {
            resolve_deferred_import_namespace(global_object, capability, module);
            return Ok(());
        }

        // For each Module Record dep of evaluationList, append dep.Evaluate() to asyncDepsEvaluationPromises.
        let mut async_deps_evaluation_promises: Vec<JSPromiseRef> = Vec::new();
        for dep in &evaluation_list {
            match dep.evaluate_dynamic(global_object, referrer_async_order, Some(capability)) {
                Ok(promise) => async_deps_evaluation_promises.push(promise),
                Err(failure) => return reject_with_failure(global_object, capability, failure),
            }
        }

        // Let evaluatePromise be ! SafePerformPromiseAll(asyncDepsEvaluationPromises). The AND-join is inlined:
        // each dep promise either rejects capabilityPromise (idempotent), or decrements the join count; the
        // last dep to fulfill resolves the deferred namespace.
        let join_context =
            JSPromiseCombinatorsGlobalContext::create(capability.as_value(), module.as_value(), async_deps_evaluation_promises.len() as u64);
        for dep_promise in &async_deps_evaluation_promises {
            dep_promise.perform_promise_then_with_internal_microtask(
                global_object,
                InternalMicrotask::DynamicImportDeferDependencySettled,
                Some(capability.cell_id()),
                join_context.as_value(),
                JSValue::empty(),
            );
        }
        Ok(())
    }
}

/// Rejeita `capability` com a exceção que `failure` deixou pendente (o `rejectWithCaughtException`); a
/// lacuna do porte sobe.
fn reject_with_failure(global_object: &JSGlobalObject, capability: &JSPromise, failure: Thrown) -> ModuleResult<()> {
    match failure {
        Thrown::Pending => {
            capability.reject(global_object, take_pending_exception(global_object));
            Ok(())
        }
        other => Err(other),
    }
}

/// `retrieveImportAttributesFromDynamicImportOptions` + `retrieveTypeImportAttribute`
/// (`Completion.cpp`): o `ScriptFetchParameters` que `options.with` descreve, ou `None` sem atributos.
fn retrieve_fetch_parameters(global_object: &JSGlobalObject, options: JSValue) -> ModuleResult<Option<ScriptFetchParametersRef>> {
    // https://tc39.es/proposal-import-attributes/#sec-evaluate-import-call
    let vm = global_object.vm();
    if options.is_undefined() {
        return Ok(None);
    }
    if !options.is_object() {
        return Err(throw_type_error(global_object, "dynamic import's options should be an object"));
    }

    let attributes = get_value_property(global_object, options, &PropertyName::from_identifier(&vm.property_names.with_keyword))?;
    if attributes.is_undefined() {
        return Ok(None);
    }
    if !attributes.is_object() {
        return Err(throw_type_error(global_object, "dynamic import's options.with should be an object"));
    }

    let mut properties = PropertyNameArrayBuilder::new(vm, PropertyNameMode::Strings, PrivateSymbolMode::Exclude);
    get_own_property_names(vm, &attributes.as_object(), &mut properties, DontEnumPropertiesMode::Exclude)?;
    let type_name = Identifier::from_span(vm, b"type");
    let mut type_value: Option<WtfString> = None;
    let mut any_attribute = false;
    for key in properties.iter() {
        let value = get_value_property(global_object, attributes, &PropertyName::from_identifier(key))?;
        if !value.is_string() {
            return Err(throw_type_error(global_object, "dynamic import's options.with includes non string property"));
        }
        any_attribute = true;
        if *key == type_name {
            type_value = Some(value.to_wtf_string());
        }
    }
    if !any_attribute {
        return Ok(None);
    }

    // `retrieveTypeImportAttribute`: sem `type`, os atributos viajam com o tipo `None`.
    let Some(type_value) = type_value else {
        return Ok(Some(ScriptFetchParameters::create(ScriptFetchParametersType::None)));
    };
    let type_text = rust_string(&type_value);
    match ScriptFetchParameters::parse_type(&type_text) {
        Some(ScriptFetchParametersType::HostDefined) => Ok(Some(ScriptFetchParameters::create_host_defined(&type_value))),
        Some(type_) => Ok(Some(ScriptFetchParameters::create(type_))),
        None => Err(throw_type_error(global_object, &format!("Import attribute type \"{type_text}\" is not valid"))),
    }
}

/// `JSModuleLoader::requestImportModule`: resolve `specifier` contra `referrer`, carrega o grafo e
/// encadeia `continueDynamicImport`. A promessa devolvida (a do `import()`) resolve com o namespace depois
/// da avaliação (inclusive a assíncrona) e rejeita com o erro de carga, ligação ou avaliação.
fn request_import_module(
    global_object: &JSGlobalObject,
    specifier: &str,
    parameters: Option<ScriptFetchParametersRef>,
    deferred: bool,
    referrer: Option<&str>,
    defer_load: bool,
) -> ModuleResult<JSPromiseRef> {
    let vm = global_object.vm();
    // Sem host de módulos (script avulso) nada resolve: o bun rejeita com o `ResolveMessage` do resolvedor dele.
    let Some(loader) = loader() else {
        if let Some(text) = build_message_text(specifier) {
            return Err(throw_build_message(global_object, text));
        }
        let from = referrer_path(referrer);
        let (message, code, shown) = unresolved_specifier_message(specifier, &from);
        return Err(throw_resolve_message(global_object, &message, code, &shown, &from));
    };
    let key = match loader.host.resolve(specifier, referrer) {
        Ok(key) => key,
        Err(message) => {
            if let Some(text) = build_message_text(specifier) {
                return Err(throw_build_message(global_object, text));
            }
            let code = if message.starts_with("No such built-in module") { "ERR_UNKNOWN_BUILTIN_MODULE" } else { "ERR_MODULE_NOT_FOUND" };
            return Err(throw_resolve_message(global_object, &message, code, specifier, &referrer_path(referrer)));
        }
    };

    // O `referrerAsyncOrder`: a ordem de avaliação assíncrona do módulo que faz o `import()`.
    let referrer_async_order = referrer
        .and_then(|referrer| loader.registry.borrow().get(&(referrer.to_owned(), ScriptFetchParametersType::JavaScript)).cloned())
        .map_or(-1, |record| if record.async_evaluation_order().has_order() { record.async_evaluation_order().order() } else { -1 });

    // A promessa interna (o `statePromise` do C++): liquida com o namespace.
    let state_promise = JSPromise::create(vm, &global_object.promise_structure());
    if defer_load {
        let already_loaded = {
            let request_type = parameters.as_ref().map_or(ScriptFetchParametersType::JavaScript, |parameters| parameters.type_());
            loader.registry.borrow().contains_key(&(key.clone(), request_type))
        };
        let pending = PendingImport { key, parameters, deferred, referrer_async_order, hops_left: IMPORT_LOAD_HOPS - 1 };
        // Módulo novo: a leitura do arquivo é uma macrotask no bun, o corpo e o `.then` só rodam quando o laço
        // de eventos a entrega (depois de esvaziar a fila de microtasks inteira). Módulo já carregado, ou sem
        // laço de eventos: os saltos de microtask medidos.
        if !already_loaded && event_loop_is_driven() {
            let promise = Rc::clone(&state_promise);
            schedule_module_load(Box::new(move |global_object| finish_pending_import(global_object, &promise, pending)));
        } else {
            // O carregamento do `import()` é assíncrono: o módulo só é buscado, ligado e avaliado numa
            // microtask (`DynamicImportLoadSettled`), depois do código síncrono que se segue ao `import()`.
            PENDING_IMPORTS.with(|map| map.borrow_mut().insert(state_promise.cell_id(), pending));
            schedule_import_load_step(global_object, &state_promise);
        }
    } else {
        run_dynamic_import(loader.as_ref(), global_object, &state_promise, &key, parameters.as_ref(), deferred, referrer_async_order)?;
    }

    let result_promise = JSPromise::create(vm, &global_object.promise_structure());
    result_promise.mark_as_handled();
    state_promise.perform_promise_then_with_internal_microtask(
        global_object,
        InternalMicrotask::ImportModuleNamespace,
        Some(result_promise.cell_id()),
        JSValue::undefined(),
        JSValue::empty(),
    );
    Ok(result_promise)
}

/// `InternalMicrotask::DynamicImportLoadSettled`: `arguments[0]` é a promessa interna do `import()`; carrega o
/// grafo e encadeia `continueDynamicImport`.
pub fn dynamic_import_load_settled(global_object: &JSGlobalObject, arguments: [JSValue; 4]) {
    let Some(state_promise) = JSPromise::from_value(&arguments[0]) else { return };
    let Some(mut pending) = PENDING_IMPORTS.with(|pending| pending.borrow_mut().remove(&state_promise.cell_id())) else { return };
    if pending.hops_left > 0 {
        // Um passo do carregador: o próximo só roda na microtask seguinte.
        pending.hops_left -= 1;
        PENDING_IMPORTS.with(|map| map.borrow_mut().insert(state_promise.cell_id(), pending));
        schedule_import_load_step(global_object, &state_promise);
        return;
    }
    finish_pending_import(global_object, &state_promise, pending);
}

/// O passo final do `import()`: carrega o grafo e encadeia `continueDynamicImport` (ou rejeita).
fn finish_pending_import(global_object: &JSGlobalObject, state_promise: &JSPromiseRef, pending: PendingImport) {
    let Some(loader) = loader() else { return };
    match run_dynamic_import(
        &loader,
        global_object,
        state_promise,
        &pending.key,
        pending.parameters.as_ref(),
        pending.deferred,
        pending.referrer_async_order,
    ) {
        Ok(()) => {}
        Err(Thrown::Unported(what)) => panic!("módulos: {what} ainda não portado"),
        Err(_) => state_promise.reject(global_object, take_pending_exception(global_object)),
    }
}

/// Agenda um passo do carregador: uma promessa já cumprida cuja reação é a `DynamicImportLoadSettled`.
fn schedule_import_load_step(global_object: &JSGlobalObject, state_promise: &JSPromiseRef) {
    let trigger = JSPromise::create(global_object.vm(), &global_object.promise_structure());
    trigger.fulfill(global_object, JSValue::undefined());
    trigger.perform_promise_then_with_internal_microtask(
        global_object,
        InternalMicrotask::DynamicImportLoadSettled,
        Some(state_promise.cell_id()),
        JSValue::undefined(),
        JSValue::empty(),
    );
}

/// O corpo de `dynamicImportLoadSettled`: carrega `key` e continua o `import()`, ou rejeita `state_promise`.
fn run_dynamic_import(
    loader: &ModuleLoader,
    global_object: &JSGlobalObject,
    state_promise: &JSPromiseRef,
    key: &str,
    parameters: Option<&ScriptFetchParametersRef>,
    deferred: bool,
    referrer_async_order: i64,
) -> ModuleResult<()> {
    match loader.load_module(global_object, key, parameters) {
        Ok(record) => loader.continue_dynamic_import(global_object, &record, state_promise, deferred, referrer_async_order),
        Err(failure) => reject_with_failure(global_object, state_promise, failure),
    }
}

/// Um `import()` à espera da microtask que carrega o módulo.
struct PendingImport {
    key: String,
    parameters: Option<ScriptFetchParametersRef>,
    deferred: bool,
    referrer_async_order: i64,
    /// Passos do carregador (busca, resolução) que ainda separam o `import()` do `link`/`evaluate`; cada
    /// um é uma microtask, como no bun (medido: o corpo de um módulo já carregado roda na geração 2).
    hops_left: u8,
}

/// Microtasks entre o `import()` e o início do `link`/`evaluate`, contando a de `DynamicImportLoadSettled`
/// (medido no bun 1.4.2: um `import()` de módulo já carregado resolve na geração 9 de uma cadeia de
/// `.then`, o corpo roda na 2; 7 gerações separam o fim do corpo do callback do `.then` do usuário).
const IMPORT_LOAD_HOPS: u8 = 2;

thread_local! {
    static PENDING_IMPORTS: RefCell<HashMap<usize, PendingImport>> = RefCell::new(HashMap::new());
}

/// `load_and_evaluate_module`: carrega `specifier` (sem referrer), liga e avalia, e devolve a promessa do
/// namespace do módulo (`loadAndEvaluateModule` de `jsc.cpp`): o namespace só existe depois que as
/// microtasks drenarem, porque a avaliação pode ser assíncrona.
pub fn load_and_evaluate_module(global_object: &JSGlobalObject, specifier: &str) -> JSPromiseRef {
    match request_import_module(global_object, specifier, None, false, None, false) {
        Ok(promise) => promise,
        Err(Thrown::Unported(what)) => panic!("módulos: {what} ainda não portado"),
        Err(_) => {
            let promise = JSPromise::create(global_object.vm(), &global_object.promise_structure());
            promise.reject(global_object, take_pending_exception(global_object));
            promise
        }
    }
}

/// `createImportMetaProperties(globalObject, key, moduleRecord, scriptFetcher)`: o objeto de `import.meta`.
pub fn create_import_meta_properties(global_object: &JSGlobalObject, key: &Identifier) -> ModuleResult<JSValue> {
    let vm = global_object.vm();
    let object = new_null_prototype_object(global_object);
    IMPORT_META_OBJECTS.with(|objects| objects.borrow_mut().insert(object.as_value().encode()));
    if let Some(loader) = loader() {
        let key_string = key_text(key);
        let url = loader.host.import_meta_url(&key_string);
        // No Bun nenhuma dessas propriedades aparece em `Object.keys(import.meta)` nem em `JSON.stringify`,
        // mas todas respondem a `in` e à leitura: aqui são não enumeráveis.
        let define = |name: &[u8], value: JSValue| {
            object.put_direct(vm, &PropertyName::from_identifier(&Identifier::from_span(vm, name)), value, DONT_ENUM);
        };
        let text_value = |text: &str| JSValue::from_js_string(js_string(vm, &WtfString::from_utf8(text.as_bytes())));
        define(b"url", text_value(&url));
        // `file:///caminho` dá `path`/`filename`, `dirname`/`dir` e `file` (o nome do arquivo), como o Bun; o caminho
        // sai da URL com os escapes `%XX` desfeitos (`a%20b.mjs` dá `a b.mjs`).
        let file_url = URL::from_string(&WtfString::from_utf8(url.as_bytes()));
        if file_url.protocol_is_file() {
            let path = String::from_utf8(file_url.file_system_path().utf8(ConversionMode::LenientConversion)).unwrap_or_default();
            let path = path.as_str();
            let (directory, file) = path.rsplit_once('/').map_or(("", path), |(directory, file)| (if directory.is_empty() { "/" } else { directory }, file));
            define(b"path", text_value(path));
            define(b"filename", text_value(path));
            define(b"dirname", text_value(directory));
            define(b"dir", text_value(directory));
            define(b"file", text_value(file));
        }
        define(b"main", JSValue::Bool(loader.host.is_main_module(&key_string)));
        // `resolve` e `resolveSync` (comprimento 0 no Bun), nativas: o referrer sai do chamador.
        for (name, function) in [(&b"resolve"[..], import_meta_resolve as NativeFn), (&b"resolveSync"[..], import_meta_resolve_sync as NativeFn)] {
            let native = JSFunction::create_native(
                vm,
                global_object,
                0,
                &WtfString::from_latin1(name),
                function,
                ImplementationVisibility::Public,
                Intrinsic::NoIntrinsic,
                call_host_function_as_constructor,
            );
            define(name, native.as_value());
        }
    }
    Ok(object.as_value())
}

type NativeFn = fn(&JSGlobalObject, &mut NativeCallFrame<'_>) -> EncodedJSValue;

fn import_meta_resolve(global_object: &JSGlobalObject, call_frame: &mut NativeCallFrame<'_>) -> EncodedJSValue {
    resolve_for_import_meta(global_object, call_frame, false)
}

fn import_meta_resolve_sync(global_object: &JSGlobalObject, call_frame: &mut NativeCallFrame<'_>) -> EncodedJSValue {
    resolve_for_import_meta(global_object, call_frame, true)
}

/// O referrer que o segundo argumento (`parent`) de `import.meta.resolve` produz: medido no bun 1.4.2, o texto
/// perde o `file://`, vale como caminho enraizado em `/` (nunca no diretório de trabalho; `./s/` e `s/x.mjs`
/// dão `/s/`, `./s` e o vazio dão `/`) e só o diretório (até a última barra) conta, com `.` e `..` resolvidos
/// e as barras duplas preservadas (`http://h/p/q.mjs` dá `/http://h/p/q.mjs`). A resolução do diretório é do
/// `join_relative` do host, que enxerga o resultado como um referrer comum.
fn parent_referrer(parent: &str) -> String {
    let path = parent.strip_prefix("file://").unwrap_or(parent);
    if path.starts_with('/') { path.to_owned() } else { format!("/{path}") }
}

/// O corpo de `import.meta.resolve` (`sync` falso, devolve a URL) e `import.meta.resolveSync` (devolve o
/// caminho): converte o argumento em string e pergunta ao host; falha de resolução lança.
fn resolve_for_import_meta(global_object: &JSGlobalObject, call_frame: &mut NativeCallFrame<'_>, sync: bool) -> EncodedJSValue {
    let vm = global_object.vm();
    // Medido no bun 1.4.2: `resolve` exige `this` ser o `import.meta` (chamada solta, `.call({})` e objeto
    // derivado lançam); `resolveSync` só recusa `this` que não é célula (`null`, número), aceita `{}` e string.
    let this_value = call_frame.this_value();
    // A chamada solta `r()` de uma variável fora de registrador (capturada, ou de escopo de módulo) entrega o
    // objeto de escopo como `this` (célula: passa, mas sem chamador, ver abaixo); `r.call(undefined)` e a
    // variável local entregam `undefined` e o bun recusa. A distinção sai do bytecode do
    // `FunctionCallResolveNode`, não daqui.
    let bound = if sync { this_value.is_cell() } else { IMPORT_META_OBJECTS.with(|objects| objects.borrow().contains(&this_value.encode())) };
    if !bound {
        let name = if sync { "resolveSync" } else { "resolve" };
        throw_native_type_error(global_object, &format!("import.meta.{name} must be bound to an import.meta object"));
        return JSValue::undefined().encode();
    }
    let source_origin = caller_source_origin(&vm.interpreter(), call_frame.call_frame());
    let argument = call_frame.argument(0);
    let specifier = match to_wtf_string_or_type_error(&argument) {
        Ok(string) => rust_string(&string),
        Err(message) => {
            throw_type_error(global_object, message);
            return JSValue::undefined().encode();
        }
    };
    // `resolveSync("")` é recusado antes de resolver (TypeError `ERR_INVALID_ARG_VALUE`, sem a classe de resolução).
    if sync && specifier.is_empty() {
        return throw_coded_type_error(global_object, "The argument 'id' must be a non-empty string. Received ''", "ERR_INVALID_ARG_VALUE");
    }
    let caller = if source_origin.is_null() { String::new() } else { rust_string(source_origin.string()) };
    let mut referrer = caller.clone();
    let mut cited: Option<String> = None;
    // Se falso, a busca nem chega ao host: o bun resolveria contra o diretório de trabalho, que o sandbox não tem.
    let mut search = true;
    let parent = call_frame.argument(1);
    let meta_this = IMPORT_META_OBJECTS.with(|objects| objects.borrow().contains(&this_value.encode()));
    if sync && !meta_this {
        // Medido: `resolveSync` com `this` que não é o `import.meta` (`{}`, `Object.create(import.meta)`, o
        // objeto de escopo da chamada solta) não tem chamador: cita `undefined` e procura no diretório de
        // trabalho; `this` string vale como o caminho que cita (absoluto, resolve contra ele).
        if this_value.is_string() {
            let text = rust_string(&this_value.as_js_string().value());
            if text.starts_with('/') {
                referrer = parent_referrer(&text);
            } else {
                search = false;
            }
            cited = Some(text);
        } else {
            search = false;
            cited = Some("undefined".to_owned());
        }
    } else if parent.is_string() {
        // O segundo argumento: string troca o chamador como origem da resolução de `resolve` e, em
        // `resolveSync`, só quando é caminho absoluto (o relativo é ignorado). O texto citado na mensagem de
        // falha é a string como veio.
        let text = rust_string(&parent.as_js_string().value());
        if !sync || text.starts_with('/') {
            referrer = parent_referrer(&text);
        }
        cited = Some(text);
    } else if sync && call_frame.argument_count() > 1 {
        // `resolveSync` com argumento não string cita `undefined`; `resolve` ignora o valor.
        cited = Some("undefined".to_owned());
    }
    let Some(loader) = loader() else { return JSValue::undefined().encode() };
    let outcome = if search { loader.host.import_meta_resolve(&specifier, &referrer, sync) } else { Err(String::new()) };
    match outcome {
        Ok(text) => JSValue::from_js_string(js_string(vm, &WtfString::from_utf8(text.as_bytes()))).encode(),
        Err(_) => {
            // O host só diz que não achou; a mensagem é a do resolvedor do bun, citando o chamador (ou o parent).
            let shown_referrer = cited.unwrap_or_else(|| {
                let url = loader.host.import_meta_url(&caller);
                referrer_path(Some(url.as_str()))
            });
            let (mut message, code, shown) = unresolved_specifier_message(&specifier, &shown_referrer);
            if shown_referrer.is_empty() {
                if let Some(head) = message.strip_suffix(" imported from ") {
                    message = format!("{head} from ''");
                }
            }
            let _ = throw_resolve_message(global_object, &message, code, &shown, &shown_referrer);
            JSValue::undefined().encode()
        }
    }
}

/// `globalFuncImportModule`: `importModule(specifier, parameters, deferred)` devolve a promessa.
fn global_func_import_module(global_object: &JSGlobalObject, call_frame: &mut NativeCallFrame<'_>) -> EncodedJSValue {
    let vm = global_object.vm();

    // `rejectWithCaughtException`: a promessa já rejeitada com a exceção pendente.
    let reject_with_caught_exception = |message: Option<&str>| -> EncodedJSValue {
        let promise = JSPromise::create(vm, &global_object.promise_structure());
        match message {
            Some(message) => promise.reject(global_object, create_type_error(global_object, &WtfString::from_utf8(message.as_bytes())).as_value()),
            None => promise.reject(global_object, take_pending_exception(global_object)),
        }
        promise.as_value().encode()
    };

    let source_origin = caller_source_origin(&vm.interpreter(), call_frame.call_frame());
    let argument = call_frame.argument(0);
    let specifier = match to_wtf_string_or_type_error(&argument) {
        // Medido no bun 1.4.2: o `toString` do specifier que lança (`{ toString() { throw new RangeError('x') } }`) rejeita
        // a promessa com essa exceção, antes de olhar as opções.
        Ok(_) if vm.exception().is_some() => return reject_with_caught_exception(None),
        Ok(string) => rust_string(&string),
        Err(message) => return reject_with_caught_exception(Some(message)),
    };

    // `JSModuleLoader::importModule`: os atributos do segundo argumento e a fase do terceiro.
    let parameters = match retrieve_fetch_parameters(global_object, call_frame.argument(1)) {
        Ok(parameters) => parameters,
        Err(Thrown::Unported(what)) => panic!("módulos: {what} ainda não portado"),
        Err(_) => return reject_with_caught_exception(None),
    };
    let deferred = call_frame.argument(2).is_true();
    let referrer = (!source_origin.is_null()).then(|| rust_string(source_origin.string()));
    let import_promise = match request_import_module(global_object, &specifier, parameters, deferred, referrer.as_deref(), true) {
        Ok(promise) => promise,
        Err(Thrown::Unported(what)) => panic!("módulos: {what} ainda não portado"),
        Err(_) => return reject_with_caught_exception(None),
    };

    // `USE(BUN_JSC_ADDITIONS)`: a promessa do usuário é uma segunda, que adota a do carregador.
    let promise = JSPromise::create(vm, &global_object.promise_structure());
    if import_promise.status() == PromiseStatus::Fulfilled {
        promise.fulfill(global_object, import_promise.result());
    } else {
        promise.resolve(global_object, import_promise.as_value());
    }
    promise.as_value().encode()
}

/// O erro de `analyze` (`createError(globalObject, errorType, message)`) lançado.
fn throw_analysis_error(global_object: &JSGlobalObject, error_type: ErrorType, message: &str) -> Thrown {
    let message = WtfString::from_utf8(message.as_bytes());
    let error = match error_type {
        ErrorType::SyntaxError => create_syntax_error(global_object, &message),
        ErrorType::TypeError => create_type_error(global_object, &message),
        _ => create_error(global_object, &message),
    };
    let mut scope = ThrowScope::new(global_object.vm());
    throw_exception(global_object, &mut scope, error);
    Thrown::Pending
}

/// `constructEmptyObject(vm, globalObject->nullPrototypeObjectStructure())`.
pub(crate) fn new_null_prototype_object(global_object: &JSGlobalObject) -> JSObjectRef {
    let vm = global_object.vm();
    let structure = JSFinalObject::create_structure(vm, Some(global_object), js_null(), JSFinalObject::DEFAULT_INLINE_CAPACITY);
    JSFinalObject::create(vm, &structure)
}
