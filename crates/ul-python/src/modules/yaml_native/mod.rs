//! `yaml._yaml`: o binding Cython do libyaml 0.2.5 que o PyYAML 6.0.3 instala como
//! `yaml/_yaml.cpython-313-x86_64-linux-gnu.so`, reescrito em Rust.
//!
//! O scanner, o parser e o emissor são o porte do libyaml (`types`, `reader`, `scanner`, `parser`,
//! `emitter`). Este módulo é a cola: expõe o módulo nativo interno `_yaml_core`, cujos objetos
//! devolvem tuplas simples, e carrega `_yaml_impl` (modules/py/_yaml_impl.py), a camada em Python
//! que faz o papel do `_yaml.pyx` (classes `Mark`, `CParser` e `CEmitter`, objetos de token, evento
//! e nó do pacote `yaml`). O módulo entregue ao `import yaml._yaml` é uma cópia de `_yaml_impl` com
//! o nome `yaml._yaml`; os dois nomes de apoio não existem para o programa.

mod emitter;
mod parser;
mod reader;
mod scanner;
mod types;

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::rc::Rc;

use crate::modules::ModuleBuilder;
use crate::native_util::bind;
use crate::object::{ExtObject, Kw, ModuleObj, Value};
use crate::vm::{exc, type_error, PyResult, Vm};
use emitter::{Emitter, LineBreak};
use reader::{Parser, Source};
use types::{CollectionStyle, Encoding, Event, Mark, ScalarStyle, Token, YamlError, ET, TT};

fn mark_value(m: Mark) -> Value {
    Value::tuple(vec![Value::Int(m.index as i64), Value::Int(m.line as i64), Value::Int(m.column as i64)])
}

fn opt_str(s: Option<&str>) -> Value {
    s.map_or(Value::None, text)
}

/// Nome da codificação como o `_yaml.pyx` a informa: UTF-8 some quando a entrada era `str`.
fn encoding_value(encoding: Encoding, unicode_source: bool) -> Value {
    match encoding {
        Encoding::Utf8 if unicode_source => Value::None,
        Encoding::Utf8 => Value::str("utf-8"),
        Encoding::Utf16Le => Value::str("utf-16-le"),
        Encoding::Utf16Be => Value::str("utf-16-be"),
    }
}

fn style_value(style: ScalarStyle) -> Value {
    match style {
        ScalarStyle::Any => Value::None,
        ScalarStyle::Plain => Value::str(""),
        ScalarStyle::SingleQuoted => Value::str("'"),
        ScalarStyle::DoubleQuoted => Value::str("\""),
        ScalarStyle::Literal => Value::str("|"),
        ScalarStyle::Folded => Value::str(">"),
    }
}

fn token_kind(ty: TT) -> i64 {
    match ty {
        TT::StreamStart => 1,
        TT::StreamEnd => 2,
        TT::VersionDirective => 3,
        TT::TagDirective => 4,
        TT::DocumentStart => 5,
        TT::DocumentEnd => 6,
        TT::BlockSequenceStart => 7,
        TT::BlockMappingStart => 8,
        TT::BlockEnd => 9,
        TT::FlowSequenceStart => 10,
        TT::FlowSequenceEnd => 11,
        TT::FlowMappingStart => 12,
        TT::FlowMappingEnd => 13,
        TT::BlockEntry => 14,
        TT::FlowEntry => 15,
        TT::Key => 16,
        TT::Value => 17,
        TT::Alias => 18,
        TT::Anchor => 19,
        TT::Tag => 20,
        TT::Scalar => 21,
    }
}

/// `(tipo, início, fim, a, b)`: o token no formato que a camada em Python consome.
fn token_value(token: Token, unicode_source: bool) -> Value {
    let (a, b) = match token.ty {
        TT::StreamStart => (encoding_value(token.encoding, unicode_source), Value::None),
        TT::VersionDirective => (Value::Int(token.major), Value::Int(token.minor)),
        TT::TagDirective | TT::Tag => (text(&token.a), text(&token.b)),
        TT::Alias | TT::Anchor => (text(&token.a), Value::None),
        TT::Scalar => (text(&token.a), style_value(token.style)),
        _ => (Value::None, Value::None),
    };
    Value::tuple(vec![Value::Int(token_kind(token.ty)), mark_value(token.start), mark_value(token.end), a, b])
}

fn flow_style_value(style: CollectionStyle) -> Value {
    match style {
        CollectionStyle::Any => Value::None,
        CollectionStyle::Block => Value::Bool(false),
        CollectionStyle::Flow => Value::Bool(true),
    }
}

/// `(tipo, início, fim, a, b, c, d, e, f)`: o evento no formato que a camada em Python consome.
fn event_value(event: Event, unicode_source: bool) -> Value {
    let none = || Value::None;
    let (kind, fields): (i64, [Value; 6]) = match event.ty {
        ET::StreamStart => (1, [encoding_value(event.encoding, unicode_source), none(), none(), none(), none(), none()]),
        ET::StreamEnd => (2, [none(), none(), none(), none(), none(), none()]),
        ET::DocumentStart => {
            let version = event.version.map_or(Value::None, |(major, minor)| Value::tuple(vec![Value::Int(major), Value::Int(minor)]));
            let tags = if event.tags.is_empty() {
                Value::None
            } else {
                Value::list(event.tags.into_iter().map(|(h, p)| Value::tuple(vec![text(&h), text(&p)])).collect())
            };
            (3, [version, tags, Value::Bool(!event.implicit), none(), none(), none()])
        }
        ET::DocumentEnd => (4, [Value::Bool(!event.implicit), none(), none(), none(), none(), none()]),
        ET::Alias => (5, [opt_str(event.anchor.as_deref()), none(), none(), none(), none(), none()]),
        ET::Scalar => (
            6,
            [
                opt_str(event.anchor.as_deref()),
                opt_str(event.tag.as_deref()),
                text(&event.value),
                Value::Bool(event.plain_implicit),
                Value::Bool(event.quoted_implicit),
                style_value(event.style),
            ],
        ),
        ET::SequenceStart | ET::MappingStart => (
            if event.ty == ET::SequenceStart { 7 } else { 9 },
            [
                opt_str(event.anchor.as_deref()),
                opt_str(event.tag.as_deref()),
                Value::Bool(event.implicit),
                flow_style_value(event.collection_style),
                none(),
                none(),
            ],
        ),
        ET::SequenceEnd => (8, [none(), none(), none(), none(), none(), none()]),
        ET::MappingEnd => (10, [none(), none(), none(), none(), none(), none()]),
    };
    let mut items = vec![Value::Int(kind), mark_value(event.start), mark_value(event.end)];
    items.extend(fields);
    Value::tuple(items)
}

/// Falha do leitor, do scanner ou do parser como tupla `(-1, origem, ...)`; erro do `read` do
/// programa sobe como exceção.
fn error_value(error: YamlError) -> PyResult<Value> {
    match error {
        YamlError::Py(e) => Err(e),
        YamlError::Reader { problem, offset, value } => Ok(Value::tuple(vec![
            Value::Int(-1),
            Value::str("reader"),
            Value::str(problem),
            Value::Int(offset as i64),
            Value::Int(value),
        ])),
        YamlError::Marked { from_parser, context, context_mark, problem, problem_mark } => Ok(Value::tuple(vec![
            Value::Int(-1),
            Value::str(if from_parser { "parser" } else { "scanner" }),
            opt_str(context),
            context.map_or(Value::None, |_| mark_value(context_mark)),
            Value::str(problem),
            mark_value(problem_mark),
        ])),
    }
}

struct YamlParser {
    parser: RefCell<Parser>,
    unicode_source: Rc<Cell<bool>>,
}

impl ExtObject for YamlParser {
    fn type_name(&self) -> &'static str {
        "_yaml_core.Parser"
    }

    fn methods(&self) -> &'static [&'static str] {
        &["scan", "parse"]
    }

    fn call_method(&self, _vm: &mut Vm, name: &str, _args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
        let unicode = self.unicode_source.get();
        let result = match name {
            "scan" => self.parser.borrow_mut().scan().map(|t| t.map(|t| token_value(t, unicode))),
            "parse" => self.parser.borrow_mut().parse().map(|e| e.map(|e| event_value(e, unicode))),
            _ => return Err(exc("AttributeError", format!("'_yaml_core.Parser' object has no attribute '{name}'"))),
        };
        match result {
            Ok(Some(v)) => Ok(v),
            Ok(None) => Ok(Value::None),
            Err(e) => error_value(e),
        }
    }
}

/// Fonte de bytes de um arquivo: chama `stream.read(size)` e guarda o excedente, como o
/// `input_handler` do `_yaml.pyx`.
fn reader_source(read: Value, unicode_source: Rc<Cell<bool>>) -> Source {
    let mut cache: Option<(Vec<u8>, usize)> = None;
    Box::new(move |size| {
        if cache.is_none() {
            let mut vm = crate::vm::current().ok_or_else(|| exc("RuntimeError", "no active interpreter"))?;
            let value = vm.call(&read, vec![Value::Int(size as i64)], Vec::new())?;
            let bytes = match &value {
                Value::Str(s) => {
                    unicode_source.set(true);
                    crate::textcodec::encode_utf8(s.as_str(), "strict")?
                }
                Value::Bytes(b) => b.to_vec(),
                _ => return Err(type_error("a string value is expected")),
            };
            cache = Some((bytes, 0));
        }
        let Some((bytes, pos)) = cache.as_mut() else { return Ok(Vec::new()) };
        let n = size.min(bytes.len() - *pos);
        let chunk = bytes[*pos..*pos + n].to_vec();
        *pos += n;
        if *pos == bytes.len() {
            cache = None;
        }
        Ok(chunk)
    })
}

fn parser_from_bytes(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let a = bind("parser_from_bytes", args, kw, &["data", "unicode_source"], 2)?;
    let Some(Value::Bytes(data)) = &a[0] else { return Err(type_error("parser_from_bytes() argument 1 must be bytes")) };
    let unicode_source = Rc::new(Cell::new(a[1].as_ref().is_some_and(Value::is_true)));
    Ok(Value::Ext(Rc::new(YamlParser { parser: RefCell::new(Parser::from_bytes(data.to_vec())), unicode_source })))
}

fn parser_from_reader(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let a = bind("parser_from_reader", args, kw, &["read"], 1)?;
    let read = a[0].clone().unwrap_or(Value::None);
    let unicode_source = Rc::new(Cell::new(false));
    let parser = Parser::new(reader_source(read, unicode_source.clone()));
    Ok(Value::Ext(Rc::new(YamlParser { parser: RefCell::new(parser), unicode_source })))
}

/// Texto de uma palavra-chave ASCII do protocolo com a camada em Python (estilo, codificação).
fn opt_string(v: Option<&Value>) -> Option<String> {
    match v {
        Some(Value::Str(s)) => Some(s.as_str().to_string()),
        _ => None,
    }
}

/// Texto do usuário (âncora, tag, valor) como UTF-8 de verdade, o que o `_yaml.pyx` faz com
/// `encode('utf-8')`: surrogates solitários levantam `UnicodeEncodeError`.
fn user_string(v: Option<&Value>) -> PyResult<Option<String>> {
    let Some(Value::Str(s)) = v else { return Ok(None) };
    let bytes = crate::textcodec::encode_utf8(s.as_str(), "strict")?;
    Ok(Some(String::from_utf8(bytes).unwrap_or_default()))
}

/// O texto que a libyaml produziu (UTF-8 de verdade) na codificação interna dos `str` da VM.
fn text(s: &str) -> Value {
    let mut out = String::with_capacity(s.len());
    crate::methods::bytesm::push_valid_utf8(&mut out, s);
    Value::str(out)
}

fn flag(v: Option<&Value>) -> bool {
    v.is_some_and(Value::is_true)
}

/// A versão `(maior, menor)` do `%YAML`, convertida como o Cython converte para `int` de C.
fn pair(v: &Value) -> PyResult<Option<(i64, i64)>> {
    match v {
        Value::Tuple(items) if items.len() >= 2 => Ok(Some((c_int(&items[0])?, c_int(&items[1])?))),
        _ => Ok(None),
    }
}

fn scalar_style(v: Option<&Value>) -> ScalarStyle {
    match opt_string(v).as_deref() {
        Some("'") => ScalarStyle::SingleQuoted,
        Some("\"") => ScalarStyle::DoubleQuoted,
        Some("|") => ScalarStyle::Literal,
        Some(">") => ScalarStyle::Folded,
        _ => ScalarStyle::Plain,
    }
}

fn tag_pairs(v: Option<&Value>) -> PyResult<Vec<(String, String)>> {
    let Some(Value::List(items)) = v else { return Ok(Vec::new()) };
    let mut out = Vec::new();
    for item in items.borrow().iter() {
        if let Value::Tuple(p) = item {
            if p.len() == 2 {
                if let (Some(handle), Some(prefix)) = (user_string(p.first())?, user_string(p.get(1))?) {
                    out.push((handle, prefix));
                }
            }
        }
    }
    Ok(out)
}

/// O evento que o `CEmitter` em Python montou: `(tipo, campos...)` com o mesmo desenho de
/// [`event_value`], sem as marcas.
fn event_from_value(v: &Value) -> PyResult<Event> {
    let Value::Tuple(items) = v else { return Err(type_error("event must be a tuple")) };
    let Some(Value::Int(kind)) = items.first() else { return Err(type_error("event must start with a kind")) };
    let at = |i: usize| items.get(i);
    let mark = Mark::default();
    let ty = match *kind {
        1 => ET::StreamStart,
        2 => ET::StreamEnd,
        3 => ET::DocumentStart,
        4 => ET::DocumentEnd,
        5 => ET::Alias,
        6 => ET::Scalar,
        7 => ET::SequenceStart,
        8 => ET::SequenceEnd,
        9 => ET::MappingStart,
        10 => ET::MappingEnd,
        _ => return Err(type_error("invalid event kind")),
    };
    let mut event = Event::new(ty, mark, mark);
    match ty {
        ET::StreamStart => {
            event.encoding = match opt_string(at(1)).as_deref() {
                Some("utf-16-le") => Encoding::Utf16Le,
                Some("utf-16-be") => Encoding::Utf16Be,
                _ => Encoding::Utf8,
            };
        }
        ET::DocumentStart => {
            event.version = match at(1) {
                Some(v) => pair(v)?,
                None => None,
            };
            event.tags = tag_pairs(at(2))?;
            event.implicit = flag(at(3));
        }
        ET::DocumentEnd => event.implicit = flag(at(1)),
        ET::Alias => event.anchor = user_string(at(1))?,
        ET::Scalar => {
            event.anchor = user_string(at(1))?;
            event.tag = user_string(at(2))?;
            event.value = user_string(at(3))?.unwrap_or_default();
            event.plain_implicit = flag(at(4));
            event.quoted_implicit = flag(at(5));
            event.style = scalar_style(at(6));
        }
        ET::SequenceStart | ET::MappingStart => {
            event.anchor = user_string(at(1))?;
            event.tag = user_string(at(2))?;
            event.implicit = flag(at(3));
            event.collection_style = if flag(at(4)) { CollectionStyle::Flow } else { CollectionStyle::Block };
        }
        ET::StreamEnd | ET::SequenceEnd | ET::MappingEnd => {}
    }
    Ok(event)
}

struct YamlEmitter {
    emitter: RefCell<Emitter>,
}

impl ExtObject for YamlEmitter {
    fn type_name(&self) -> &'static str {
        "_yaml_core.Emitter"
    }

    fn methods(&self) -> &'static [&'static str] {
        &["emit"]
    }

    /// `emit(evento)` devolve `(mensagem de erro ou None, [blocos de bytes já descarregados])`.
    fn call_method(&self, _vm: &mut Vm, name: &str, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
        if name != "emit" {
            return Err(exc("AttributeError", format!("'_yaml_core.Emitter' object has no attribute '{name}'")));
        }
        let event = event_from_value(args.first().unwrap_or(&Value::None))?;
        let mut emitter = self.emitter.borrow_mut();
        let outcome = emitter.emit(event);
        let chunks = emitter.take_output().into_iter().map(Value::bytes).collect();
        Ok(Value::tuple(vec![outcome.err().map_or(Value::None, Value::str), Value::list(chunks)]))
    }
}

/// A conversão de `indent` e `width` para `int` de C que o Cython faz no `CEmitter.__init__`:
/// `None` não chama o `yaml_emitter_set_*`; o que não é inteiro levanta `TypeError` e o que não
/// cabe em `int` de C levanta `OverflowError`.
fn c_int(v: &Value) -> PyResult<i64> {
    let n = match v {
        Value::Bool(b) => i64::from(*b),
        Value::Int(n) => *n,
        Value::Big(_) => i64::MAX,
        _ => return Err(type_error("an integer is required")),
    };
    if i32::try_from(n).is_err() {
        return Err(exc("OverflowError", "value too large to convert to int"));
    }
    Ok(n)
}

/// `indent` e `width` opcionais: `None` ou ausente não chama o `yaml_emitter_set_*`.
fn optional_c_int(v: Option<&Value>) -> PyResult<Option<i64>> {
    match v {
        None | Some(Value::None) => Ok(None),
        Some(v) => c_int(v).map(Some),
    }
}

fn new_emitter(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let a = bind("emitter", args, kw, &["canonical", "indent", "width", "unicode", "line_break"], 0)?;
    let mut emitter = Emitter::new();
    emitter.canonical = flag(a[0].as_ref());
    if let Some(indent) = optional_c_int(a[1].as_ref())? {
        emitter.best_indent = if (2..10).contains(&indent) { indent } else { 2 };
    }
    if let Some(width) = optional_c_int(a[2].as_ref())? {
        emitter.best_width = width.max(-1);
    }
    emitter.unicode = flag(a[3].as_ref());
    emitter.line_break = match opt_string(a[4].as_ref()).as_deref() {
        Some("\r") => LineBreak::Cr,
        Some("\n") => LineBreak::Ln,
        Some("\r\n") => LineBreak::CrLn,
        _ => LineBreak::Any,
    };
    Ok(Value::Ext(Rc::new(YamlEmitter { emitter: RefCell::new(emitter) })))
}

/// O módulo interno `_yaml_core`, só para a camada em Python.
pub fn build_core(_vm: &mut Vm) -> Rc<ModuleObj> {
    ModuleBuilder::new("_yaml_core")
        .func("parser_from_bytes", parser_from_bytes)
        .func("parser_from_reader", parser_from_reader)
        .func("emitter", new_emitter)
        .build()
}

/// `yaml._yaml`: o módulo que o `import` entrega no lugar do `.so`. Leva só os nomes públicos da
/// camada em Python; os auxiliares com `_` e os dados do arquivo embutido ficam de fora (o
/// carregador de extensões põe `__file__`, `__package__` e `__doc__` do `.so`).
pub fn build_yaml_native(vm: &mut Vm) -> PyResult<Rc<ModuleObj>> {
    let inner = crate::modules::import_checked(vm, "_yaml_impl")?;
    let hidden = ["__file__", "__cached__", "__package__", "__builtins__", "__doc__", "__spec__", "__loader__"];
    let attrs: BTreeMap<String, Value> = inner
        .attrs
        .borrow()
        .iter()
        .filter(|(name, _)| (!name.starts_with('_') || (name.starts_with("__") && name.ends_with("__"))) && !hidden.contains(&name.as_str()))
        .map(|(name, value)| (name.clone(), value.clone()))
        .collect();
    Ok(Rc::new(ModuleObj { name: crate::object::intern("yaml._yaml"), attrs: RefCell::new(attrs) }))
}

#[cfg(test)]
mod tests;
