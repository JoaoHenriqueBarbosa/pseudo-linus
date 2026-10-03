//! Camadas YAML candidatas do yq. Cada uma lê um stream YAML pra [`Node`] (modelo neutro, com a
//! resolução de escalares que a própria crate faz) e emite YAML a partir de um [`Node`] (pro `-y`),
//! usando o emissor da própria crate. O resto do yq (CLI, filtro, impressão JSON) é igual pra todas.

use std::borrow::Cow;
use std::fmt;

use serde::de::{Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};

/// Valor YAML já resolvido, independente de crate.
#[derive(Clone, Debug, PartialEq)]
pub enum Node {
    Null,
    Bool(bool),
    /// Inteiro em decimal (pode passar de 64 bits).
    Int(String),
    Float(f64),
    Str(String),
    Seq(Vec<Node>),
    /// Pares na ordem do documento (chaves repetidas ficam; o front-end decide).
    Map(Vec<(Node, Node)>),
}

pub trait YamlLayer: Send + Sync {
    /// Nome do candidato (ex.: "jaq + saphyr 0.1").
    fn name(&self) -> &'static str;
    /// Crate da camada YAML (pro depscan) e versão.
    fn package(&self) -> &'static str;
    fn version(&self) -> &'static str;
    /// Lê todos os documentos do stream.
    fn load(&self, text: &str) -> Result<Vec<Node>, String>;
    /// Emite um documento como YAML (sem o separador `---` inicial, que é do front-end).
    fn emit(&self, node: &Node) -> Result<String, String>;
}

pub fn all_layers() -> Vec<Box<dyn YamlLayer>> {
    vec![
        Box::new(JaqFmtsLayer),
        Box::new(SaphyrLayer),
        Box::new(SerdeSaphyrLayer),
        Box::new(YamlRust2Layer),
        Box::new(NoyalibLayer),
    ]
}

/// Tira o `---` que alguns emissores sempre escrevem no começo do documento (normalização mecânica:
/// quem decide os separadores entre documentos é o front-end, como no `yaml.dump_all` do yq).
fn strip_doc_start(s: &str) -> String {
    let s = s.strip_prefix("---\n").or_else(|| s.strip_prefix("--- ")).or_else(|| s.strip_prefix("---")).unwrap_or(s);
    let mut s = s.to_string();
    if !s.ends_with('\n') {
        s.push('\n');
    }
    s
}

// ---------------------------------------------------------------------------------------------
// jaq-fmts (saphyr-parser por baixo, resolução própria do core schema 1.2)

pub struct JaqFmtsLayer;

impl YamlLayer for JaqFmtsLayer {
    fn name(&self) -> &'static str {
        "jaq + jaq-fmts 0.1 (saphyr-parser)"
    }
    fn package(&self) -> &'static str {
        "jaq-fmts"
    }
    fn version(&self) -> &'static str {
        "0.1.1"
    }
    fn load(&self, text: &str) -> Result<Vec<Node>, String> {
        // O leitor do jaq-fmts tem `panic!`/`unwrap` internos em estados que ele julga impossíveis;
        // um panic aqui vira erro do caso, não da bancada.
        let text = text.to_string();
        // Depois de um erro o iterador continua devolvendo o mesmo erro pra sempre: para no primeiro.
        std::panic::catch_unwind(move || {
            let mut docs = Vec::new();
            for r in jaq_fmts::read::yaml::parse_many(&text) {
                docs.push(val_to_node(&r.map_err(|e| e.to_string())?));
            }
            Ok(docs)
        })
        .map_err(|_| "panic no leitor YAML do jaq-fmts".to_string())?
    }
    fn emit(&self, node: &Node) -> Result<String, String> {
        let val = super::yq_front::node_to_val_plain(node);
        let pp = jaq_json::write::Pp { indent: Some("  ".to_string()), sep_space: true, ..Default::default() };
        let mut buf = Vec::new();
        jaq_fmts::write::yaml::write(&mut buf, &pp, 0, &val).map_err(|e| e.to_string())?;
        buf.push(b'\n');
        Ok(String::from_utf8_lossy(&buf).into_owned())
    }
}

/// `Val` do jaq (vindo do jaq-fmts ou do filtro) pra [`Node`].
pub fn val_to_node(v: &jaq_json::Val) -> Node {
    use jaq_json::{Num, Val};
    match v {
        Val::Null => Node::Null,
        Val::Bool(b) => Node::Bool(*b),
        Val::Num(Num::Int(i)) => Node::Int(i.to_string()),
        Val::Num(Num::BigInt(b)) => Node::Int(b.to_string()),
        Val::Num(Num::Float(f)) => Node::Float(*f),
        Val::Num(Num::Dec(s)) => Node::Float(s.parse().unwrap_or(f64::NAN)),
        Val::TStr(b) | Val::BStr(b) => Node::Str(String::from_utf8_lossy(b).into_owned()),
        Val::Arr(a) => Node::Seq(a.iter().map(val_to_node).collect()),
        Val::Obj(o) => Node::Map(o.iter().map(|(k, v)| (val_to_node(k), val_to_node(v))).collect()),
    }
}

// ---------------------------------------------------------------------------------------------
// saphyr 0.1 (loader + emissor próprios)

pub struct SaphyrLayer;

fn saphyr_to_node(y: &saphyr::Yaml) -> Result<Node, String> {
    use saphyr::{Scalar, Yaml};
    Ok(match y {
        Yaml::Value(Scalar::Null) => Node::Null,
        Yaml::Value(Scalar::Boolean(b)) => Node::Bool(*b),
        Yaml::Value(Scalar::Integer(i)) => Node::Int(i.to_string()),
        Yaml::Value(Scalar::FloatingPoint(f)) => Node::Float(f.into_inner()),
        Yaml::Value(Scalar::String(s)) => Node::Str(s.to_string()),
        Yaml::Representation(s, _, _) => Node::Str(s.to_string()),
        Yaml::Sequence(items) => Node::Seq(items.iter().map(saphyr_to_node).collect::<Result<_, _>>()?),
        Yaml::Mapping(map) => Node::Map(
            map.iter()
                .map(|(k, v)| Ok((saphyr_to_node(k)?, saphyr_to_node(v)?)))
                .collect::<Result<_, String>>()?,
        ),
        Yaml::Tagged(_, inner) => saphyr_to_node(inner)?,
        Yaml::Alias(_) => return Err("alias não resolvido pelo saphyr".into()),
        Yaml::BadValue => return Err("valor YAML inválido (BadValue)".into()),
    })
}

fn node_to_saphyr(n: &Node) -> saphyr::Yaml<'static> {
    use saphyr::{Scalar, Yaml};
    match n {
        Node::Null => Yaml::Value(Scalar::Null),
        Node::Bool(b) => Yaml::Value(Scalar::Boolean(*b)),
        Node::Int(s) => match s.parse::<i64>() {
            Ok(i) => Yaml::Value(Scalar::Integer(i)),
            Err(_) => Yaml::Value(Scalar::String(Cow::Owned(s.clone()))),
        },
        Node::Float(f) => Yaml::Value(Scalar::FloatingPoint((*f).into())),
        Node::Str(s) => Yaml::Value(Scalar::String(Cow::Owned(s.clone()))),
        Node::Seq(items) => Yaml::Sequence(items.iter().map(node_to_saphyr).collect()),
        Node::Map(pairs) => {
            let mut map = saphyr::Mapping::new();
            for (k, v) in pairs {
                map.insert(node_to_saphyr(k), node_to_saphyr(v));
            }
            Yaml::Mapping(map)
        }
    }
}

impl YamlLayer for SaphyrLayer {
    fn name(&self) -> &'static str {
        "jaq + saphyr 0.1"
    }
    fn package(&self) -> &'static str {
        "saphyr"
    }
    fn version(&self) -> &'static str {
        "0.1.0"
    }
    fn load(&self, text: &str) -> Result<Vec<Node>, String> {
        use saphyr::LoadableYamlNode;
        let docs = saphyr::Yaml::load_from_str(text).map_err(|e| e.to_string())?;
        docs.iter().map(saphyr_to_node).collect()
    }
    fn emit(&self, node: &Node) -> Result<String, String> {
        let doc = node_to_saphyr(node);
        let mut out = String::new();
        let mut emitter = saphyr::YamlEmitter::new(&mut out);
        emitter.multiline_strings(true);
        emitter.dump(&doc).map_err(|e| format!("{e:?}"))?;
        Ok(strip_doc_start(&out))
    }
}

// ---------------------------------------------------------------------------------------------
// yaml-rust2 0.13

pub struct YamlRust2Layer;

fn yr2_to_node(y: &yaml_rust2::Yaml) -> Result<Node, String> {
    use yaml_rust2::Yaml;
    Ok(match y {
        Yaml::Null => Node::Null,
        Yaml::Boolean(b) => Node::Bool(*b),
        Yaml::Integer(i) => Node::Int(i.to_string()),
        Yaml::Real(_) => Node::Float(y.as_f64().ok_or_else(|| "real inválido".to_string())?),
        Yaml::String(s) => Node::Str(s.clone()),
        Yaml::Array(items) => Node::Seq(items.iter().map(yr2_to_node).collect::<Result<_, _>>()?),
        Yaml::Hash(map) => Node::Map(
            map.iter().map(|(k, v)| Ok((yr2_to_node(k)?, yr2_to_node(v)?))).collect::<Result<_, String>>()?,
        ),
        Yaml::Alias(_) => return Err("alias não resolvido pelo yaml-rust2".into()),
        Yaml::BadValue => return Err("valor YAML inválido (BadValue)".into()),
    })
}

fn node_to_yr2(n: &Node) -> yaml_rust2::Yaml {
    use yaml_rust2::Yaml;
    match n {
        Node::Null => Yaml::Null,
        Node::Bool(b) => Yaml::Boolean(*b),
        Node::Int(s) => match s.parse::<i64>() {
            Ok(i) => Yaml::Integer(i),
            Err(_) => Yaml::Real(s.clone()),
        },
        Node::Float(f) => Yaml::Real(super::jqout::python_repr(*f)),
        Node::Str(s) => Yaml::String(s.clone()),
        Node::Seq(items) => Yaml::Array(items.iter().map(node_to_yr2).collect()),
        Node::Map(pairs) => {
            let mut map = yaml_rust2::yaml::Hash::new();
            for (k, v) in pairs {
                map.insert(node_to_yr2(k), node_to_yr2(v));
            }
            Yaml::Hash(map)
        }
    }
}

impl YamlLayer for YamlRust2Layer {
    fn name(&self) -> &'static str {
        "jaq + yaml-rust2 0.13"
    }
    fn package(&self) -> &'static str {
        "yaml-rust2"
    }
    fn version(&self) -> &'static str {
        "0.13.0"
    }
    fn load(&self, text: &str) -> Result<Vec<Node>, String> {
        let docs = yaml_rust2::YamlLoader::load_from_str(text).map_err(|e| e.to_string())?;
        docs.iter().map(yr2_to_node).collect()
    }
    fn emit(&self, node: &Node) -> Result<String, String> {
        let doc = node_to_yr2(node);
        let mut out = String::new();
        let mut emitter = yaml_rust2::YamlEmitter::new(&mut out);
        emitter.multiline_strings(true);
        emitter.dump(&doc).map_err(|e| format!("{e:?}"))?;
        Ok(strip_doc_start(&out))
    }
}

// ---------------------------------------------------------------------------------------------
// serde-saphyr 1.3 (granit-parser; serde, com merge key e opções de compatibilidade)

pub struct SerdeSaphyrLayer;

impl<'de> Deserialize<'de> for Node {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Node, D::Error> {
        d.deserialize_any(NodeVisitor)
    }
}

struct NodeVisitor;

impl<'de> Visitor<'de> for NodeVisitor {
    type Value = Node;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("qualquer valor YAML")
    }
    fn visit_bool<E>(self, v: bool) -> Result<Node, E> {
        Ok(Node::Bool(v))
    }
    fn visit_i64<E>(self, v: i64) -> Result<Node, E> {
        Ok(Node::Int(v.to_string()))
    }
    fn visit_u64<E>(self, v: u64) -> Result<Node, E> {
        Ok(Node::Int(v.to_string()))
    }
    fn visit_i128<E>(self, v: i128) -> Result<Node, E> {
        Ok(Node::Int(v.to_string()))
    }
    fn visit_u128<E>(self, v: u128) -> Result<Node, E> {
        Ok(Node::Int(v.to_string()))
    }
    fn visit_f64<E>(self, v: f64) -> Result<Node, E> {
        Ok(Node::Float(v))
    }
    fn visit_str<E>(self, v: &str) -> Result<Node, E> {
        Ok(Node::Str(v.to_string()))
    }
    fn visit_string<E>(self, v: String) -> Result<Node, E> {
        Ok(Node::Str(v))
    }
    fn visit_bytes<E>(self, v: &[u8]) -> Result<Node, E> {
        Ok(Node::Str(String::from_utf8_lossy(v).into_owned()))
    }
    fn visit_unit<E>(self) -> Result<Node, E> {
        Ok(Node::Null)
    }
    fn visit_none<E>(self) -> Result<Node, E> {
        Ok(Node::Null)
    }
    fn visit_some<D: Deserializer<'de>>(self, d: D) -> Result<Node, D::Error> {
        Node::deserialize(d)
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Node, A::Error> {
        let mut items = Vec::new();
        while let Some(item) = seq.next_element::<Node>()? {
            items.push(item);
        }
        Ok(Node::Seq(items))
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Node, A::Error> {
        let mut pairs = Vec::new();
        while let Some((k, v)) = map.next_entry::<Node, Node>()? {
            pairs.push((k, v));
        }
        Ok(Node::Map(pairs))
    }
}

impl serde::Serialize for Node {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::{SerializeMap, SerializeSeq};
        match self {
            Node::Null => s.serialize_unit(),
            Node::Bool(b) => s.serialize_bool(*b),
            Node::Int(i) => match i.parse::<i64>() {
                Ok(v) => s.serialize_i64(v),
                Err(_) => match i.parse::<i128>() {
                    Ok(v) => s.serialize_i128(v),
                    Err(_) => s.serialize_str(i),
                },
            },
            Node::Float(f) => s.serialize_f64(*f),
            Node::Str(v) => s.serialize_str(v),
            Node::Seq(items) => {
                let mut seq = s.serialize_seq(Some(items.len()))?;
                for item in items {
                    seq.serialize_element(item)?;
                }
                seq.end()
            }
            Node::Map(pairs) => {
                let mut map = s.serialize_map(Some(pairs.len()))?;
                for (k, v) in pairs {
                    map.serialize_entry(k, v)?;
                }
                map.end()
            }
        }
    }
}

impl YamlLayer for SerdeSaphyrLayer {
    fn name(&self) -> &'static str {
        "jaq + serde-saphyr 1.3"
    }
    fn package(&self) -> &'static str {
        "serde-saphyr"
    }
    fn version(&self) -> &'static str {
        "1.3.0"
    }
    fn load(&self, text: &str) -> Result<Vec<Node>, String> {
        // Configurada pra ficar o mais perto possível do yq 3.4.3: só true/false são booleanos, octal
        // legado (012) vale, chave repetida fica com o último valor, .inf/.nan aceitos, merge key ligada.
        let mut options = serde_saphyr::Options::default();
        options.with_snippet = false;
        options.strict_booleans = true;
        options.legacy_octal_numbers = true;
        options.duplicate_keys = serde_saphyr::DuplicateKeyPolicy::LastWins;
        options.reject_non_finite_typeless_float = false;
        serde_saphyr::from_multiple_with_options::<Node>(text, options).map_err(|e| e.to_string())
    }
    fn emit(&self, node: &Node) -> Result<String, String> {
        // Mais perto do PyYAML: lista indentada sob a chave e sem estilos de bloco (`|`, `>`).
        let mut options = serde_saphyr::SerializerOptions::default();
        options.compact_list_indent = false;
        options.prefer_block_scalars = false;
        serde_saphyr::to_string_with_options(node, options).map(|s| strip_doc_start(&s)).map_err(|e| e.to_string())
    }
}

// ---------------------------------------------------------------------------------------------
// noyalib 0.0.51 (o motor YAML do yqr)

pub struct NoyalibLayer;

pub fn yqr_to_node(v: &yqr::Value) -> Node {
    match v {
        yqr::Value::Null => Node::Null,
        yqr::Value::Bool(b) => Node::Bool(*b),
        yqr::Value::Int(i) => Node::Int(i.to_string()),
        yqr::Value::Float(f) => Node::Float(*f),
        yqr::Value::String(s) => Node::Str(s.clone()),
        yqr::Value::Sequence(items) => Node::Seq(items.iter().map(yqr_to_node).collect()),
        yqr::Value::Mapping(map) => Node::Map(map.iter().map(|(k, v)| (yqr_to_node(k), yqr_to_node(v))).collect()),
    }
}

pub fn node_to_yqr(n: &Node) -> yqr::Value {
    match n {
        Node::Null => yqr::Value::Null,
        Node::Bool(b) => yqr::Value::Bool(*b),
        Node::Int(s) => match s.parse::<i64>() {
            Ok(i) => yqr::Value::Int(i),
            Err(_) => yqr::Value::Float(s.parse().unwrap_or(f64::NAN)),
        },
        Node::Float(f) => yqr::Value::Float(*f),
        Node::Str(s) => yqr::Value::String(s.clone()),
        Node::Seq(items) => yqr::Value::Sequence(items.iter().map(node_to_yqr).collect()),
        Node::Map(pairs) => {
            yqr::Value::Mapping(pairs.iter().map(|(k, v)| (node_to_yqr(k), node_to_yqr(v))).collect())
        }
    }
}

/// Lê todos os documentos com o noyalib, na mesma configuração que o yqr usa.
pub fn noyalib_load(text: &str) -> Result<Vec<yqr::Value>, String> {
    let config = noyalib::ParserConfig::new().alias_anchor_ratio(None);
    let docs = noyalib::document::load_all_with_config(text, &config).map_err(|e| e.to_string())?;
    docs.map(|d| d.map(yqr::Value::from).map_err(|e| e.to_string())).collect()
}

impl YamlLayer for NoyalibLayer {
    fn name(&self) -> &'static str {
        "jaq + noyalib 0.0.51"
    }
    fn package(&self) -> &'static str {
        "noyalib"
    }
    fn version(&self) -> &'static str {
        "0.0.51"
    }
    fn load(&self, text: &str) -> Result<Vec<Node>, String> {
        Ok(noyalib_load(text)?.iter().map(yqr_to_node).collect())
    }
    fn emit(&self, node: &Node) -> Result<String, String> {
        let v = noyalib::Value::from(&node_to_yqr(node));
        noyalib::to_string_value(&v).map(|s| strip_doc_start(&s)).map_err(|e| e.to_string())
    }
}
