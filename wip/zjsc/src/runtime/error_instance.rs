//! Porte de `runtime/ErrorInstance.h` e da parte de `ErrorInstance.cpp` que não depende do
//! interpretador: a célula `ErrorInstance` (um `JSNonFinalObject`), registrada no `cell_registry`
//! como `CellEntry::ErrorInstance`, com `create`, `createStructure`, os acessores e as flags
//! (`setParseError`, `setOutOfMemoryError`, `setStackOverflowError`...).
//!
//! DIVERGÊNCIAS: `m_stackTrace` (`Vector<StackFrame>`), `captureStackTrace`, `computeErrorInfo`,
//! `materializeErrorInfoIfNeeded`, `m_sourceAppender`, `appendSourceToErrorMessage` e o
//! `getOwnPropertySlot`/`put`/`deleteProperty`/`defineOwnProperty` sobrescritos dependem de
//! `StackFrame`, `Interpreter`, `PropertySlot` e das propriedades `message`/`cause`/`stack`, e entram
//! com eles. Sem eles, `create` não grava `message` por `putDirect`: a mensagem fica em `ErrorData`.
//! `m_bunErrorData` e `m_catchableFromWasm` não existem. O dado observável (tipo, mensagem, linha,
//! coluna, `sourceURL`, pilha opcional e as flags) mora em `ErrorData`, que `ParserError::toErrorObject`
//! monta antes de materializar a célula.
//!
//! `Error.prepareStackTrace` (extensão do `Bun`, `onComputeErrorInfoJSValue`) roda na materialização
//! preguiçosa: os frames ficam em `ErrorData::pending_frames` e a primeira leitura de `stack` (o gancho em
//! `JSObject::getOwnPropertySlot`) os formata, uma vez por erro. Medido no `bun` 1.4.2. `put`,
//! `deleteProperty`, `defineOwnProperty` e os nomes próprios (modo `Include`) também materializam, como o
//! `ErrorInstance.cpp` (ganchos em `js_object.rs` e `own_property_names.rs`).

use std::cell::RefCell;
use std::rc::Rc;

use crate::llint::{LLIntFailure, LLIntResult};
use crate::runtime::cell_registry::{self, CellEntry};
use crate::runtime::class_info::ClassInfo;
use crate::runtime::error_natives::error_header_of_object;
use crate::runtime::host_function_support::ObjectRef;
use crate::runtime::error_type::{error_type_name, ErrorType};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_call_site::prepare_stack_trace;
use crate::runtime::js_object::{JSNonFinalObject, JSObject, JSObjectHandle, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_string::js_string;
use crate::runtime::property_attribute::DONT_ENUM;
use crate::runtime::property_name::PropertyName;
use crate::runtime::stack_frame::{format_stack_trace, StackFrame};
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::{
    TypeInfo, GET_OWN_PROPERTY_SLOT_IS_IMPURE_FOR_PROPERTY_ABSENCE, OVERRIDES_GET_OWN_PROPERTY_SLOT,
    OVERRIDES_GET_OWN_SPECIAL_PROPERTY_NAMES, OVERRIDES_PUT,
};
use crate::runtime::js_value::{js_number, JSValue};
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::vm::VM;
use crate::wtf::text::conversion_mode::ConversionMode;
use crate::wtf::text::wtf_string::{String as WtfString};

/// `const ClassInfo ErrorInstance::s_info`.
pub static ERROR_INSTANCE_S_INFO: ClassInfo =
    ClassInfo { class_name: "Error", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

/// O dado observável de um `ErrorInstance`.
#[derive(Clone, Debug, Default)]
pub struct ErrorData {
    /// `errorType()`: `None` só no `Default`.
    pub error_type: Option<ErrorType>,
    pub message: WtfString,
    /// `line()`; `-1` quando não definida.
    pub line: i32,
    pub column: i32,
    pub source_url: WtfString,
    pub stack: Option<WtfString>,
    /// `m_stackTrace`: os frames capturados que ainda não viraram `stack`.
    pub pending_frames: Option<Vec<StackFrame>>,
    /// O fonte do frame de cima, gravado quando a pilha é materializada: o relato de erro do `console.log` mostra o
    /// trecho dele.
    pub source_text: Option<std::rc::Rc<str>>,
    pub is_parse_error: bool,
    /// O `N` de `    at <parse> (:N)` no texto de `stack` de um `SyntaxError`: a linha do erro no fonte embrulhado de
    /// `new Function` (só `FunctionExecutable::fromGlobalCode` grava); `0` nos demais (`eval`, `RegExp`, `JSON.parse`).
    pub parse_frame_line: i32,
    pub is_out_of_memory_error: bool,
    pub is_stack_overflow_error: bool,
}

impl ErrorData {
    /// Os campos de `ErrorInstance::create(vm, structure, message, ..., errorType)`.
    pub fn create(error_type: ErrorType, message: WtfString) -> ErrorData {
        ErrorData { error_type: Some(error_type), message, line: -1, ..ErrorData::default() }
    }

    /// O `name` do protótipo do tipo (`errorTypeName`).
    pub fn name(&self) -> &'static str {
        match self.error_type {
            Some(error_type) => error_type_name(error_type),
            None => "",
        }
    }
}

/// `class ErrorInstance : public JSNonFinalObject`.
#[derive(Debug)]
pub struct ErrorInstance {
    base: JSNonFinalObject,
    data: RefCell<ErrorData>,
}

/// O `ErrorInstance*`.
pub type ErrorInstanceRef = Rc<ErrorInstance>;

impl std::ops::Deref for ErrorInstance {
    type Target = JSNonFinalObject;

    fn deref(&self) -> &JSNonFinalObject {
        &self.base
    }
}

impl ErrorInstance {
    /// `StructureFlags = Base::StructureFlags | OverridesGetOwnPropertySlot |
    /// OverridesGetOwnSpecialPropertyNames | OverridesPut | GetOwnPropertySlotIsImpureForPropertyAbsence`.
    pub const STRUCTURE_FLAGS: u32 = JSNonFinalObject::STRUCTURE_FLAGS
        | OVERRIDES_GET_OWN_PROPERTY_SLOT
        | OVERRIDES_GET_OWN_SPECIAL_PROPERTY_NAMES
        | OVERRIDES_PUT
        | GET_OWN_PROPERTY_SLOT_IS_IMPURE_FOR_PROPERTY_ABSENCE;

    /// `createStructure(vm, globalObject, prototype)`.
    pub fn create_structure(vm: &VM, global_object: Option<&JSGlobalObject>, prototype: JSValue) -> StructureRef {
        Structure::create(
            vm,
            global_object,
            prototype,
            TypeInfo::new(JSType::ErrorInstanceType, ErrorInstance::STRUCTURE_FLAGS),
            &ERROR_INSTANCE_S_INFO,
        )
    }

    /// `ErrorInstance(vm, structure, errorType)` seguido do `finishCreation` sem captura de pilha, e o
    /// registro da célula. `create(vm, structure, message, cause, ..., errorType)` é este com
    /// `ErrorData::create`.
    pub fn create_with_data(vm: &VM, structure: StructureRef, data: ErrorData) -> ErrorInstanceRef {
        let cell_id = cell_registry::reserve();
        let instance = Rc::new(ErrorInstance { base: JSNonFinalObject::new(vm, structure), data: RefCell::new(data) });
        instance.base.set_cell_id(cell_id);
        cell_registry::set(cell_id, CellEntry::ErrorInstance(Rc::clone(&instance)));
        instance
    }

    /// `create(vm, structure, message, cause, sourceAppender, runtimeType, errorType, ...)` na parte que
    /// não captura pilha.
    pub fn create(vm: &VM, structure: StructureRef, message: WtfString, error_type: ErrorType) -> ErrorInstanceRef {
        ErrorInstance::create_with_data(vm, structure, ErrorData::create(error_type, message))
    }

    /// Procura a célula pelo `cell_id` guardado em `JSValue::Cell`.
    pub fn from_cell_id(cell_id: usize) -> Option<ErrorInstanceRef> {
        match cell_registry::get(cell_id) {
            Some(CellEntry::ErrorInstance(instance)) => Some(instance),
            _ => None,
        }
    }

    /// O "endereço" da célula, o que o `JSValue` codifica.
    pub fn cell_id(&self) -> usize {
        self.base.cell_id()
    }

    /// `JSValue(JSCell*)`.
    pub fn as_value(&self) -> JSValue {
        JSValue::from_cell(self.cell_id())
    }

    /// O `JSObject*` da célula (o retorno `JSObject*` de `ParserError::toErrorObject`).
    pub fn as_object(&self) -> JSObjectHandle {
        JSObject::from_cell_id(self.cell_id()).expect("ErrorInstance registrada como objeto")
    }

    /// `errorType()`.
    pub fn error_type(&self) -> Option<ErrorType> {
        self.data.borrow().error_type
    }

    /// O `name` do protótipo do tipo (`errorTypeName`).
    pub fn name(&self) -> &'static str {
        self.data.borrow().name()
    }

    /// A mensagem do erro.
    pub fn message(&self) -> WtfString {
        self.data.borrow().message.clone()
    }

    /// O fonte do frame de cima da pilha materializada, se houve.
    pub fn source_text(&self) -> Option<std::rc::Rc<str>> {
        self.data.borrow().source_text.clone()
    }

    /// `line()`; `-1` quando não definida.
    pub fn line(&self) -> i32 {
        self.data.borrow().line
    }

    /// `setLine(line)`.
    pub fn set_line(&self, line: i32) {
        self.data.borrow_mut().line = line;
    }

    /// `column()`.
    pub fn column(&self) -> i32 {
        self.data.borrow().column
    }

    /// Grava o `N` do frame `<parse> (:N)` (ver `ErrorData::parse_frame_line`).
    pub fn set_parse_frame_line(&self, line: i32) {
        self.data.borrow_mut().parse_frame_line = line.max(0);
    }

    /// `setColumn(column)`.
    pub fn set_column(&self, column: i32) {
        self.data.borrow_mut().column = column;
    }

    /// `sourceURL()`.
    pub fn source_url(&self) -> WtfString {
        self.data.borrow().source_url.clone()
    }

    /// `setSourceURL(sourceURL)`.
    pub fn set_source_url(&self, source_url: WtfString) {
        self.data.borrow_mut().source_url = source_url;
    }

    /// `isParseError()`.
    pub fn is_parse_error(&self) -> bool {
        self.data.borrow().is_parse_error
    }

    /// `setParseError()`.
    pub fn set_parse_error(&self) {
        self.data.borrow_mut().is_parse_error = true;
    }

    /// `isOutOfMemoryError()`.
    pub fn is_out_of_memory_error(&self) -> bool {
        self.data.borrow().is_out_of_memory_error
    }

    /// `setOutOfMemoryError()`.
    pub fn set_out_of_memory_error(&self) {
        self.data.borrow_mut().is_out_of_memory_error = true;
    }

    /// `isStackOverflowError()`.
    pub fn is_stack_overflow_error(&self) -> bool {
        self.data.borrow().is_stack_overflow_error
    }

    /// `setStackOverflowError()`.
    pub fn set_stack_overflow_error(&self) {
        self.data.borrow_mut().is_stack_overflow_error = true;
    }

    /// `stackString()`: o texto de `stack` já materializado, se houver.
    pub fn stack_string(&self) -> Option<WtfString> {
        self.data.borrow().stack.clone()
    }

    /// `m_stackTrace` materializado (`materializeErrorInfoIfNeeded`): grava a propriedade própria `stack`
    /// (`DontEnum`, como o `putDirect` do C++) com `stack`, que pode ser qualquer valor (o retorno de
    /// `Error.prepareStackTrace`). O texto guardado é o do valor quando é string e o vazio nos outros casos,
    /// o `emptyString()` que o C++ usa quando a propriedade `stack` já foi materializada por fora.
    pub fn set_stack_value(&self, vm: &VM, stack: JSValue) {
        self.put_direct(vm, &PropertyName::from_identifier(&vm.property_names.stack), stack, DONT_ENUM);
        let text = if stack.is_string() { stack.to_wtf_string() } else { WtfString::default() };
        self.data.borrow_mut().stack = Some(text);
    }

    /// Se o erro já tem pilha (frames guardados ou texto materializado): `set_pending_stack` seria um
    /// no-op, então quem captura pode pular o percurso de frames.
    pub fn has_stack_info(&self) -> bool {
        let data = self.data.borrow();
        data.stack.is_some() || data.pending_frames.is_some()
    }

    /// `m_stackTrace = frames` (`captureStackTrace` do `finishCreation`): a pilha só é formatada na primeira
    /// leitura de `stack` ([`ErrorInstance::materialize_stack`]). Não faz nada se `stack` já foi
    /// materializada ou se já há frames guardados.
    pub fn set_pending_stack(&self, frames: Vec<StackFrame>) {
        let mut data = self.data.borrow_mut();
        if data.stack.is_none() && data.pending_frames.is_none() {
            data.pending_frames = Some(frames);
        }
    }

    /// `Error.captureStackTrace(error)` num `ErrorInstance`: troca os frames guardados e descarta o texto já
    /// materializado; o cabeçalho é refeito na próxima leitura de `stack`.
    pub fn replace_pending_stack(&self, frames: Vec<StackFrame>) {
        let mut data = self.data.borrow_mut();
        data.stack = None;
        data.pending_frames = Some(frames);
    }

    /// `materializeErrorInfoIfNeeded`: com frames guardados, roda `Error.prepareStackTrace` se definido,
    /// senão formata o cabeçalho (`Error.prototype.toString` do tipo) e as linhas, e grava `stack`. Os
    /// frames são tirados antes de rodar o gancho, então a releitura de `stack` dentro dele não recursa.
    /// `Err` é a exceção que o gancho lançou, já pendente no `VM`.
    pub fn materialize_stack(&self, global_object: &JSGlobalObject) -> LLIntResult<()> {
        let Some(frames) = self.data.borrow_mut().pending_frames.take() else {
            return Ok(());
        };
        let vm = global_object.vm();
        if let Some(top) = frames.iter().find(|top| top.has_line_and_column_info()) {
            self.data.borrow_mut().source_text = top.source_text().map(std::rc::Rc::from);
        }
        // Medido no `bun` 1.4.2 sem gancho: `originalLine,originalColumn,line,column,sourceURL` (todos `DontEnum`,
        // graváveis, configuráveis) entram ANTES de `stack`; o `original*` do bun vem da posição no fonte
        // transpilado, que aqui não existe, então valem a posição do frame de cima. Com `Error.prepareStackTrace`
        // definido a ordem medida é `stack,line,column` (cai no laço de baixo).
        let hook_defined = prepare_stack_trace(global_object, self.as_value(), &frames)?;
        if hook_defined.is_none() {
            // Medido no `bun`: quando o frame de cima é o construtor padrão (`class X extends TypeError {}`, fonte
            // sintético `unknown:1:28`), `originalLine`, `originalColumn` e `sourceURL` não são gravados (só
            // `line`, `column` e `stack`); construtor explícito ou `extends Error` (cujo frame padrão não aparece) gravam.
            // Com o frame de cima sintético a ordem medida é `line,column,stack` (antes de `stack`, não depois).
            if let Some(top) = frames.iter().find(|top| top.has_line_and_column_info()) {
                let names = &vm.property_names;
                let has_source = top.source_url() != "unknown";
                let original = [(&names.original_line, top.line()), (&names.original_column, top.column())];
                let original: &[_] = if has_source { &original } else { &[] };
                for (name, value) in original.iter().copied().chain([(&names.line, top.line()), (&names.column, top.column())]) {
                    self.put_direct(vm, &PropertyName::from_identifier(name), js_number(f64::from(value)), DONT_ENUM);
                }
                if has_source {
                    let url = js_string(vm, &WtfString::from_latin1(top.source_url().as_bytes()));
                    self.put_direct(vm, &PropertyName::from_identifier(&names.source_url), JSValue::from_js_string(url), DONT_ENUM);
                }
            }
        }
        if let Some(prepared) = hook_defined {
            self.set_stack_value(vm, prepared);
        } else {
            // Medido no `bun`: o cabeçalho é `Error.prototype.toString` do objeto no momento da leitura
            // (`this.name = ...` depois do `super`, `E.prototype.name`).
            let object = ObjectRef::from_value(&self.as_value()).expect("ErrorInstance é objeto");
            let header = error_header_of_object(global_object, &object, false).map_err(|_| LLIntFailure::Thrown)?;
            let parse_line = (self.error_type() == Some(ErrorType::SyntaxError)).then(|| self.data.borrow().parse_frame_line);
            self.set_stack_value(vm, JSValue::from_js_string(js_string(vm, &stack_text(&header, &frames, parse_line))));
        }
        // `putDirect(line)` e `putDirect(column)` (`DontEnum`) do `materializeErrorInfoIfNeeded`, depois de
        // `stack` na ordem medida no `bun` (`message,stack,line,column`); valem a posição do frame de cima.
        // No C++ vêm de `getLineColumnAndSource` (`m_lineColumn`); o `sourceURL` só entra se não vazio e
        // não aparece na medição, então não é gravado.
        if let Some(top) = frames.iter().find(|top| top.has_line_and_column_info()).or(frames.first()) {
            for (name, value) in [(&vm.property_names.line, top.line()), (&vm.property_names.column, top.column())] {
                self.put_direct(vm, &PropertyName::from_identifier(name), js_number(f64::from(value)), DONT_ENUM);
            }
            self.set_line(top.line() as i32);
            self.set_column(top.column() as i32);
        }
        Ok(())
    }
}

/// Os nomes que `materializeErrorInfoIfNeeded(vm, propertyName)` reconhece.
fn is_error_info_property(vm: &VM, property_name: &PropertyName) -> bool {
    let names = &vm.property_names;
    [&names.line, &names.column, &names.source_url, &names.stack].into_iter().any(|name| *property_name == *name)
}

/// `materializeErrorInfoIfNeeded` de um objeto qualquer: só age em `ErrorInstance` com frames guardados.
/// `true` se materializou (a `Structure` do objeto pode ter mudado, e o `VM` pode ter exceção pendente).
pub fn materialize_error_info(object: &JSObject) -> bool {
    let Some(error) = ErrorInstance::from_cell_id(object.cell_id()) else {
        return false;
    };
    if error.data.borrow().pending_frames.is_none() {
        return false;
    }
    let Some(global_object) = object.structure().realm() else {
        return false;
    };
    match error.materialize_stack(&global_object) {
        Ok(()) | Err(LLIntFailure::Thrown) => {}
        Err(unported) => panic!("caminho ainda não portado: {unported:?}"),
    }
    true
}

/// `materializeErrorInfoIfNeeded(vm, propertyName)`: o gancho de `getOwnPropertySlot`, `put`,
/// `deleteProperty` e `defineOwnProperty` para `stack`, `line`, `column` e `sourceURL`.
pub fn materialize_for_property(object: &JSObject, vm: &VM, property_name: &PropertyName) -> bool {
    is_error_info_property(vm, property_name) && materialize_error_info(object)
}

/// O texto de `stack`: `header` e as linhas dos `frames` (ver `stack_frame.rs`). Medido no `bun` 1.4.2: todo
/// `ErrorInstance` do tipo `SyntaxError` (`new SyntaxError`, `JSON.parse`, `eval`, `new RegExp`, `BigInt('x')`,
/// subclasses) abre a lista com a linha `    at <parse> (:0)`, fora do limite de `stackTraceLimit` e ausente da lista de
/// `CallSite` do `Error.prepareStackTrace`, por isso é só texto. `parse_line` é `None` fora de `SyntaxError`;
/// `new Function` mostra `(:N)`, a linha do erro no fonte embrulhado (`ErrorData::parse_frame_line`), os demais `(:0)`.
pub fn stack_text(header: &WtfString, frames: &[StackFrame], parse_line: Option<i32>) -> WtfString {
    let mut header = String::from_utf8_lossy(&header.utf8(ConversionMode::LenientConversion)).into_owned();
    if let Some(line) = parse_line {
        header.push_str(&format!("\n    at <parse> (:{line})"));
    }
    WtfString::from_utf8(format_stack_trace(&header, frames).as_bytes())
}
