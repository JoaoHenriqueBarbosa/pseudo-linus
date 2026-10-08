//! Testes do porte do libyaml: tokens, eventos, erros e a saída do emissor, em documentos pequenos.

use super::emitter::{Emitter, LineBreak};
use super::reader::Parser;
use super::types::{CollectionStyle, Event, Mark, ScalarStyle, YamlError, ET, TT};

fn parser_of(text: &str) -> Parser {
    Parser::from_bytes(text.as_bytes().to_vec())
}

fn describe(e: &Event) -> String {
    let props = |e: &Event| {
        let mut s = String::new();
        if let Some(a) = &e.anchor {
            s.push_str(&format!(" &{a}"));
        }
        if let Some(t) = &e.tag {
            s.push_str(&format!(" <{t}>"));
        }
        s
    };
    match e.ty {
        ET::StreamStart => "+STR".to_string(),
        ET::StreamEnd => "-STR".to_string(),
        ET::DocumentStart => String::from(if e.implicit { "+DOC" } else { "+DOC ---" }),
        ET::DocumentEnd => String::from(if e.implicit { "-DOC" } else { "-DOC ..." }),
        ET::Alias => format!("=ALI *{}", e.anchor.clone().unwrap_or_default()),
        ET::Scalar => {
            let indicator = match e.style {
                ScalarStyle::SingleQuoted => '\'',
                ScalarStyle::DoubleQuoted => '"',
                ScalarStyle::Literal => '|',
                ScalarStyle::Folded => '>',
                _ => ':',
            };
            format!("=VAL{} {}{}", props(e), indicator, e.value.replace('\n', "\\n"))
        }
        ET::SequenceStart => format!("+SEQ{}{}", if e.collection_style == CollectionStyle::Flow { " []" } else { "" }, props(e)),
        ET::SequenceEnd => "-SEQ".to_string(),
        ET::MappingStart => format!("+MAP{}{}", if e.collection_style == CollectionStyle::Flow { " {}" } else { "" }, props(e)),
        ET::MappingEnd => "-MAP".to_string(),
    }
}

/// Todos os eventos, ou o erro `(origem, contexto, problema)` que parou o parser.
fn events(text: &str) -> Result<Vec<String>, (bool, Option<&'static str>, &'static str)> {
    let mut parser = parser_of(text);
    let mut out = Vec::new();
    loop {
        match parser.parse() {
            Ok(Some(e)) => out.push(describe(&e)),
            Ok(None) => return Ok(out),
            Err(YamlError::Marked { from_parser, context, problem, .. }) => return Err((from_parser, context, problem)),
            Err(YamlError::Reader { problem, .. }) => return Err((false, None, problem)),
            Err(YamlError::Py(_)) => panic!("erro do programa"),
        }
    }
}

fn error_of(text: &str) -> (bool, Option<&'static str>, &'static str) {
    events(text).err().expect("esperava um erro")
}

#[test]
fn block_mapping_with_flow_sequence() {
    let got = events("a: 1\nb: [x, y]\n").unwrap();
    assert_eq!(
        got,
        ["+STR", "+DOC", "+MAP", "=VAL :a", "=VAL :1", "=VAL :b", "+SEQ []", "=VAL :x", "=VAL :y", "-SEQ", "-MAP", "-DOC", "-STR"]
    );
}

#[test]
fn tokens_of_a_simple_mapping() {
    let mut parser = parser_of("a: 1");
    let mut kinds = Vec::new();
    while let Ok(Some(t)) = parser.scan() {
        kinds.push(t.ty);
    }
    assert_eq!(
        kinds,
        [TT::StreamStart, TT::BlockMappingStart, TT::Key, TT::Scalar, TT::Value, TT::Scalar, TT::BlockEnd, TT::StreamEnd]
    );
}

#[test]
fn scalar_marks_count_characters() {
    let mut parser = parser_of("ação: é\n");
    parser.scan().ok().flatten();
    parser.scan().ok().flatten();
    parser.scan().ok().flatten();
    let key = parser.scan().ok().flatten().unwrap();
    assert_eq!((key.a.as_str(), key.start.index, key.end.index, key.end.column), ("ação", 0, 4, 4));
}

#[test]
fn anchors_aliases_and_tags() {
    let got = events("- &a x\n- *a\n- !!str 5\n- !local v\n").unwrap();
    assert_eq!(
        got,
        [
            "+STR",
            "+DOC",
            "+SEQ",
            "=VAL &a :x",
            "=ALI *a",
            "=VAL <tag:yaml.org,2002:str> :5",
            "=VAL <!local> :v",
            "-SEQ",
            "-DOC",
            "-STR"
        ]
    );
}

#[test]
fn scalar_styles() {
    let got = events("- 'a''b'\n- \"c\\n\\x41\"\n- |\n  l1\n  l2\n- >\n  f1\n  f2\n").unwrap();
    assert_eq!(got[3], "=VAL 'a'b");
    assert_eq!(got[4], "=VAL \"c\\nA");
    assert_eq!(got[5], "=VAL |l1\\nl2\\n");
    assert_eq!(got[6], "=VAL >f1 f2\\n");
}

#[test]
fn multiple_documents_and_explicit_markers() {
    let got = events("a\n---\nb\n...\n").unwrap();
    assert_eq!(got, ["+STR", "+DOC", "=VAL :a", "-DOC", "+DOC ---", "=VAL :b", "-DOC ...", "-STR"]);
}

#[test]
fn version_and_tag_directives() {
    let mut parser = parser_of("%YAML 1.1\n%TAG !e! tag:example.com,2000:\n---\n!e!x v\n");
    let mut found = None;
    let mut tagged = None;
    while let Ok(Some(e)) = parser.parse() {
        if e.ty == ET::DocumentStart {
            found = Some((e.version, e.tags.clone(), e.implicit));
        }
        if e.ty == ET::Scalar {
            tagged = e.tag.clone();
        }
    }
    assert_eq!(found, Some((Some((1, 1)), vec![("!e!".to_string(), "tag:example.com,2000:".to_string())], false)));
    assert_eq!(tagged.as_deref(), Some("tag:example.com,2000:x"));
}

#[test]
fn empty_values_and_explicit_keys() {
    let got = events("? a\n: \nb:\n").unwrap();
    assert_eq!(got[2..9], ["+MAP", "=VAL :a", "=VAL :", "=VAL :b", "=VAL :", "-MAP", "-DOC"]);
}

#[test]
fn scanner_errors_carry_libyaml_messages() {
    assert_eq!(error_of("a: b: c\n"), (false, None, "mapping values are not allowed in this context"));
    assert_eq!(error_of("a: @b\n"), (false, Some("while scanning for the next token"), "found character that cannot start any token"));
    assert_eq!(error_of("'abc"), (false, Some("while scanning a quoted scalar"), "found unexpected end of stream"));
    assert_eq!(error_of("\"\\q\""), (false, Some("while parsing a quoted scalar"), "found unknown escape character"));
    assert_eq!(error_of("%FOO\n---\na\n"), (false, Some("while scanning a directive"), "found unknown directive name"));
    assert_eq!(error_of("- a\n b: c\n- d\n"), (false, None, "mapping values are not allowed in this context"));
}

#[test]
fn parser_errors_carry_libyaml_messages() {
    assert_eq!(error_of("a: [1, 2"), (true, Some("while parsing a flow sequence"), "did not find expected ',' or ']'"));
    assert_eq!(error_of("{a: 1 b: 2}"), (true, Some("while parsing a flow mapping"), "did not find expected ',' or '}'"));
    assert_eq!(error_of("!x!y z"), (true, Some("while parsing a node"), "found undefined tag handle"));
    assert_eq!(error_of("%YAML 2.0\n---\na\n"), (true, None, "found incompatible YAML document"));
}

#[test]
fn reader_errors_report_offset_and_value() {
    let mut parser = Parser::from_bytes(vec![b'a', 0x00]);
    match parser.scan() {
        Err(YamlError::Reader { problem, offset, value }) => {
            assert_eq!((problem, offset, value), ("control characters are not allowed", 1, 0));
        }
        _ => panic!("esperava erro do leitor"),
    }
    let mut bad = Parser::from_bytes(vec![0xFF, b'a']);
    assert!(matches!(bad.scan(), Err(YamlError::Reader { problem: "invalid leading UTF-8 octet", offset: 0, value: 255 })));
}

#[test]
fn utf16_input_with_bom_is_decoded() {
    let mut data = vec![0xFF, 0xFE];
    for unit in "k: v".encode_utf16() {
        data.extend_from_slice(&unit.to_le_bytes());
    }
    let mut parser = Parser::from_bytes(data);
    let start = parser.scan().ok().flatten().unwrap();
    assert_eq!(start.encoding, super::types::Encoding::Utf16Le);
}

fn emitter_for(width: Option<i64>) -> Emitter {
    let mut e = Emitter::new();
    e.line_break = LineBreak::Ln;
    if let Some(w) = width {
        e.best_width = w;
    }
    e
}

fn output(e: &mut Emitter) -> String {
    String::from_utf8(e.take_output().concat()).unwrap()
}

/// Reemite o que o parser leu, como o `yaml.emit` faria.
fn roundtrip(text: &str) -> String {
    let mut parser = parser_of(text);
    let mut emitter = emitter_for(None);
    while let Some(event) = parser.parse().ok().flatten() {
        emitter.emit(event).unwrap();
    }
    output(&mut emitter)
}

#[test]
fn emitter_roundtrips_mappings_and_sequences() {
    assert_eq!(roundtrip("a: 1\nb: [x, y]\n"), "a: 1\nb: [x, y]\n");
    assert_eq!(roundtrip("- a\n- b:\n    c: d\n"), "- a\n- b:\n    c: d\n");
}

fn scalar_event(value: &str, style: ScalarStyle) -> Event {
    let mark = Mark::default();
    let mut e = Event::new(ET::Scalar, mark, mark);
    e.value = value.to_string();
    e.plain_implicit = true;
    e.quoted_implicit = true;
    e.style = style;
    e
}

fn dump_root(value: &str, style: ScalarStyle, width: Option<i64>) -> String {
    let mark = Mark::default();
    let mut emitter = emitter_for(width);
    let mut doc = Event::new(ET::DocumentStart, mark, mark);
    doc.implicit = true;
    let mut end = Event::new(ET::DocumentEnd, mark, mark);
    end.implicit = true;
    for event in [Event::new(ET::StreamStart, mark, mark), doc, scalar_event(value, style), end, Event::new(ET::StreamEnd, mark, mark)] {
        emitter.emit(event).unwrap();
    }
    output(&mut emitter)
}

#[test]
fn emitter_picks_scalar_styles() {
    assert_eq!(dump_root("abc", ScalarStyle::Plain, None), "abc\n");
    assert_eq!(dump_root("a: b", ScalarStyle::Plain, None), "'a: b'\n");
    assert_eq!(dump_root("tab\there", ScalarStyle::Plain, None), "\"tab\\there\"\n");
    assert_eq!(dump_root("é", ScalarStyle::Plain, None), "\"\\xE9\"\n");
    assert_eq!(dump_root("a\nb\n", ScalarStyle::Literal, None), "|\n  a\n  b\n");
}

#[test]
fn emitter_folds_long_plain_scalars_at_80_columns() {
    let words = |n: usize| vec!["word"; n].join(" ");
    let text = words(30);
    assert_eq!(dump_root(&text, ScalarStyle::Plain, None), format!("{}\n  {}\n", words(17), words(13)));
}

#[test]
fn emitter_reports_state_errors() {
    let mark = Mark::default();
    let mut emitter = emitter_for(None);
    assert_eq!(emitter.emit(scalar_event("x", ScalarStyle::Plain)).err(), Some("expected STREAM-START"));
    let mut fresh = emitter_for(None);
    fresh.emit(Event::new(ET::StreamStart, mark, mark)).unwrap();
    assert_eq!(fresh.emit(scalar_event("x", ScalarStyle::Plain)).err(), Some("expected DOCUMENT-START or STREAM-END"));
}

#[test]
fn emitter_options_convert_like_cython_int() {
    use crate::object::Value;
    let mut vm = crate::vm::Vm::new();
    let make = |vm: &mut crate::vm::Vm, indent: Value, width: Value| {
        super::new_emitter(vm, vec![Value::Bool(false), indent, width, Value::Bool(false), Value::None], Vec::new())
    };
    assert!(make(&mut vm, Value::None, Value::None).is_ok());
    assert!(make(&mut vm, Value::Int(4), Value::Int(-1)).is_ok());
    assert_eq!(make(&mut vm, Value::str("x"), Value::None).err().map(|e| e.msg), Some("an integer is required".to_string()));
    assert_eq!(make(&mut vm, Value::None, Value::str("x")).err().map(|e| e.msg), Some("an integer is required".to_string()));
    assert_eq!(make(&mut vm, Value::Int(1 << 40), Value::None).err().map(|e| e.msg), Some("value too large to convert to int".to_string()));
}

#[test]
fn module_shape() {
    let mut vm = crate::vm::Vm::new();
    let core = super::build_core(&mut vm);
    let attrs = core.attrs.borrow();
    for name in ["parser_from_bytes", "parser_from_reader", "emitter"] {
        assert!(attrs.contains_key(name), "{name}");
    }
}
