//! A ponte entre o JavaScript da página e o documento: implementa o `DomHost` do zjsc sobre o `BaseDocument` do Blitz.
//! As operações são as mínimas que o prelúdio JavaScript precisa para escrever `Node`, `Element`, `Document` e o resto
//! em cima delas; a identidade dos nós é o `NodeId` do Blitz.

use std::cell::RefCell;

use blitz_dom::node::{Attribute, NodeData};
use blitz_dom::{BaseDocument, DocumentMutator, NodeId};
use markup5ever::{LocalName, Namespace, QualName, ns};
use ul_net::net::client::RequestSpec;
use ul_net::net::url;
use zjsc::runtime::dom_bridge::{DomArg, DomError, DomHost};

use crate::document;
use crate::encoding;
use crate::fetch::Fetcher;

/// O User-Agent do Chromium do oráculo (o Debian 13 traz o 154; o Chromium reduz a versão a `154.0.0.0`).
pub const USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) HeadlessChrome/154.0.0.0 Safari/537.36";

/// Nome local do elemento que emula o `DocumentFragment` (o Blitz não tem esse tipo de nó). Não é nome válido de
/// elemento, então não colide com nada que a página crie.
const FRAGMENT: &str = "#document-fragment";
/// A raiz de um documento criado por `DOMImplementation` (o Blitz só tem um nó de documento por árvore).
const SUBDOCUMENT: &str = "#document";

pub struct Bridge {
    pub doc: RefCell<BaseDocument>,
    pub doctype: Option<String>,
    pub fetcher: Fetcher,
    /// A URL da página (muda com `history.pushState` e `location.hash`, que o prelúdio relata).
    pub url: RefCell<String>,
    /// Navegação pedida por `location.href = ...` (a página nova substitui a atual).
    pub navigation: RefCell<Option<String>>,
    /// Estilo escrito pelo CSSOM em elemento ainda sem atributo `style`: como no Blink, o atributo só nasce (no fim
    /// da lista de atributos) quando alguém o lê, serializa ou casa seletor.
    pub pending_styles: RefCell<Vec<(NodeId, String)>>,
}

/// As operações que leem atributos (e por isso sincronizam o `style` pendente do CSSOM).
const READS_ATTRIBUTES: &[&str] = &["attrs", "getAttr", "clone", "query", "matches", "innerHTML", "outerHTML", "box", "byId"];

fn err(name: &str, message: impl Into<String>) -> DomError {
    DomError { name: name.to_owned(), message: message.into() }
}

/// O que o Chromium lança quando um método de DOM recebe um `this` ou argumento que não é o objeto certo. Os erros
/// internos da ponte (id que não resolve, argumento de tipo errado) chegam à página só assim.
fn illegal() -> DomError {
    err("TypeError", "Illegal invocation")
}

/// Os ids cruzam a ponte como número JavaScript: o `NodeId` do Blitz é um `u64` (índice mais versão do slot), e a
/// versão só sobe quando um slot é reaproveitado, então o valor cabe nos 53 bits exatos de um `f64`.
fn id_arg(args: &[DomArg], index: usize) -> Result<NodeId, DomError> {
    match args.get(index) {
        Some(DomArg::Num(n)) if *n >= 0.0 => Ok(NodeId::from_u64(*n as u64)),
        _ => Err(illegal()),
    }
}

fn str_arg(args: &[DomArg], index: usize) -> Result<&str, DomError> {
    match args.get(index) {
        Some(DomArg::Str(s)) => Ok(s),
        _ => Err(illegal()),
    }
}

fn opt_id(args: &[DomArg], index: usize) -> Option<NodeId> {
    match args.get(index) {
        Some(DomArg::Num(n)) => Some(NodeId::from_u64(*n as u64)),
        _ => None,
    }
}

fn num(id: NodeId) -> DomArg {
    DomArg::Num(id.as_u64() as f64)
}

fn ids(list: impl IntoIterator<Item = NodeId>) -> DomArg {
    DomArg::List(list.into_iter().map(num).collect())
}

fn is_fragment(doc: &BaseDocument, id: NodeId) -> bool {
    doc.get_node(id).and_then(|n| n.element_data()).is_some_and(|e| &*e.name.local == FRAGMENT)
}

/// O nome com que a página enxerga o atributo: `prefixo:local` quando há prefixo.
fn attr_matches(name: &QualName, wanted: &str) -> bool {
    document::qualified_attr_name(name) == wanted
}

impl Bridge {
    /// Dá atributo `style` aos elementos com estilo pendente do CSSOM, na ordem em que o estilo foi escrito.
    pub fn flush_styles(&self) {
        let pending = std::mem::take(&mut *self.pending_styles.borrow_mut());
        for (id, text) in pending {
            self.mutate(|m| m.set_attribute(id, QualName::new(None, ns!(), LocalName::from("style")), &text));
        }
    }

    fn has_style_attribute(&self, id: NodeId) -> bool {
        let doc = self.doc.borrow();
        doc.get_node(id).and_then(|n| n.element_data()).is_some_and(|e| e.attrs.iter().any(|a| attr_matches(&a.name, "style")))
    }

    /// Tira da árvore tudo o que vem depois de `script` na ordem do documento (os irmãos seguintes dele e de cada
    /// ancestral), como se o parser ainda não tivesse chegado lá. Devolve o que tirou, por pai, para `restore_following`.
    pub fn detach_following(&self, script: NodeId) -> Vec<(NodeId, Vec<NodeId>)> {
        let mut held = Vec::new();
        let mut doc = self.doc.borrow_mut();
        let root = doc.root_node().id;
        let mut mutr = doc.mutate();
        let mut current = script;
        while let Some(parent) = mutr.parent_id(current) {
            if parent == root {
                break;
            }
            let siblings = mutr.child_ids(parent);
            if let Some(position) = siblings.iter().position(|&s| s == current) {
                let after: Vec<NodeId> = siblings[position + 1..].to_vec();
                for &node in &after {
                    mutr.remove_node(node);
                }
                if !after.is_empty() {
                    held.push((parent, after));
                }
            }
            current = parent;
        }
        held
    }

    /// Devolve à árvore, no fim de cada pai, o que `detach_following` tirou (o que o script acrescentou fica antes).
    pub fn restore_following(&self, held: Vec<(NodeId, Vec<NodeId>)>) {
        self.mutate(|m| {
            for (parent, nodes) in held {
                m.append_children(parent, &nodes);
            }
        });
    }

    /// `fetch` e `XMLHttpRequest` da página: `[status, reason, url final, cabeçalhos (nome, valor, ...), corpo]`, ou
    /// `Null` quando o navegador falharia o pedido (rede, esquema não permitido, bloqueio de CORS).
    fn fetch(&self, args: &[DomArg]) -> Result<DomArg, DomError> {
        let method = str_arg(args, 0)?;
        let target = str_arg(args, 1)?;
        let mut headers: Vec<(String, String)> = Vec::new();
        if let Some(DomArg::List(flat)) = args.get(2) {
            for pair in flat.chunks(2) {
                if let [DomArg::Str(name), DomArg::Str(value)] = pair {
                    headers.push((name.clone(), value.clone()));
                }
            }
        }
        let body = match args.get(3) {
            Some(DomArg::Str(text)) => text.as_bytes().to_vec(),
            _ => Vec::new(),
        };
        let page_url = self.url.borrow().clone();
        let page_origin = origin_of(&page_url);
        let target_origin = origin_of(target);
        let Some(target_origin) = target_origin else { return Ok(DomArg::Null) };
        let same_origin = page_origin.as_deref() == Some(target_origin.as_str());
        for (name, value) in [("User-Agent", USER_AGENT), ("Accept", "*/*"), ("Accept-Language", "en-US,en;q=0.9")] {
            if !headers.iter().any(|(existing, _)| existing.eq_ignore_ascii_case(name)) {
                headers.push((name.to_owned(), value.to_owned()));
            }
        }
        if !same_origin || !matches!(method, "GET" | "HEAD") {
            headers.push(("Origin".to_owned(), page_origin.clone().unwrap_or_else(|| "null".to_owned())));
        }
        let spec = RequestSpec { method, headers: &headers, body: &body, ..RequestSpec::default() };
        let Ok(response) = (self.fetcher)(target, &spec) else { return Ok(DomArg::Null) };
        let final_url = response.url.to_string_with(false);
        if origin_of(&final_url).as_deref() != page_origin.as_deref() {
            let allowed = response.header("Access-Control-Allow-Origin");
            let page = page_origin.as_deref().unwrap_or("null");
            if !allowed.is_some_and(|value| value == "*" || value == page) {
                return Ok(DomArg::Null);
            }
        }
        // O corpo cruza a ponte cru, um byte por unidade de código (latin1): binários (wasm, imagens) chegam intactos, e
        // o JavaScript decodifica texto pelo charset que este último item diz (o rótulo do encoding_rs).
        let content_type = response.header("Content-Type");
        let charset = encoding::label(content_type.as_deref());
        let raw: String = response.body.iter().map(|&b| char::from(b)).collect();
        let mut flat = Vec::new();
        for (name, value) in &response.headers {
            flat.push(DomArg::Str(String::from_utf8_lossy(name).into_owned()));
            flat.push(DomArg::Str(String::from_utf8_lossy(value).into_owned()));
        }
        Ok(DomArg::List(vec![DomArg::Num(f64::from(response.status)), DomArg::Str(response.reason), DomArg::Str(final_url), DomArg::List(flat), DomArg::Str(raw), DomArg::Str(charset)]))
    }

    fn mutate<T>(&self, f: impl FnOnce(&mut DocumentMutator<'_>) -> T) -> T {
        let mut doc = self.doc.borrow_mut();
        let mut mutr = doc.mutate();
        f(&mut mutr)
    }

    /// Cópia rasa de um nó (sem filhos), para `cloneNode(false)` e como base da cópia profunda.
    fn shallow_clone(m: &mut DocumentMutator<'_>, id: NodeId) -> NodeId {
        let data = m.doc.get_node(id).map(|n| n.data.clone());
        match data {
            Some(NodeData::Element(element)) => m.create_element(element.name.clone(), element.attrs.iter().cloned().collect()),
            Some(NodeData::Text(text)) => m.create_text_node(&text.content),
            Some(NodeData::Comment { contents }) => m.create_comment_node(&contents),
            _ => m.create_comment_node(""),
        }
    }

    fn clone_node(m: &mut DocumentMutator<'_>, id: NodeId, deep: bool) -> NodeId {
        let copy = Self::shallow_clone(m, id);
        if deep {
            let children = m.child_ids(id);
            let copies: Vec<NodeId> = children.iter().map(|&child| Self::clone_node(m, child, true)).collect();
            m.append_children(copy, &copies);
        }
        copy
    }

    /// Tira `child` do pai antigo (se houver) e, se for fragmento, devolve os filhos dele no lugar.
    fn detach_for_insert(m: &mut DocumentMutator<'_>, child: NodeId) -> Vec<NodeId> {
        if is_fragment(m.doc, child) {
            let kids: Vec<NodeId> = m.child_ids(child).to_vec();
            for &kid in &kids {
                m.remove_node(kid);
            }
            return kids;
        }
        if m.parent_id(child).is_some() {
            m.remove_node(child);
        }
        vec![child]
    }
}

/// Copia a subárvore `id` de outro documento para o da página, nó a nó.
fn copy_across(from: &BaseDocument, id: NodeId, m: &mut DocumentMutator<'_>) -> Option<NodeId> {
    let node = from.get_node(id)?;
    let copy = match &node.data {
        NodeData::Element(element) => m.create_element(element.name.clone(), element.attrs.iter().cloned().collect()),
        NodeData::Text(text) => m.create_text_node(&text.content),
        NodeData::Comment { contents } => m.create_comment_node(contents),
        _ => return None,
    };
    for &child in &node.children {
        if let Some(child_copy) = copy_across(from, child, m) {
            m.append_children(copy, &[child_copy]);
        }
    }
    Some(copy)
}

/// A origem (`esquema://host[:porta]`) de uma URL absoluta; `None` para o que não tem origem de rede.
pub fn origin_of(address: &str) -> Option<String> {
    let parsed = url::parse(address, false).ok()?;
    if parsed.scheme != "http" && parsed.scheme != "https" {
        return None;
    }
    let default = url::default_port(&parsed.scheme);
    Some(match parsed.port {
        Some(port) if Some(port) != default => format!("{}://{}:{port}", parsed.scheme, parsed.host_for_url()),
        _ => format!("{}://{}", parsed.scheme, parsed.host_for_url()),
    })
}

impl DomHost for Bridge {
    fn call(&self, op: &str, args: &[DomArg]) -> Result<DomArg, DomError> {
        if READS_ATTRIBUTES.contains(&op) && !self.pending_styles.borrow().is_empty() {
            self.flush_styles();
        }
        // Escrever ou apagar o atributo `style` direto descarta o que o CSSOM tinha pendente.
        if matches!(op, "setAttr" | "removeAttr") && args.get(1).is_some_and(|a| matches!(a, DomArg::Str(n) if n.eq_ignore_ascii_case("style"))) {
            let id = id_arg(args, 0)?;
            self.pending_styles.borrow_mut().retain(|(node, _)| *node != id);
        }
        match op {
            // A escrita do CSSOM: no atributo que já existe, no lugar; sem atributo, fica pendente até uma leitura.
            "setStyle" => {
                let id = id_arg(args, 0)?;
                let text = str_arg(args, 1)?.to_owned();
                if self.has_style_attribute(id) {
                    self.mutate(|m| m.set_attribute(id, QualName::new(None, ns!(), LocalName::from("style")), &text));
                } else {
                    let mut pending = self.pending_styles.borrow_mut();
                    match pending.iter_mut().find(|(node, _)| *node == id) {
                        Some(entry) => entry.1 = text,
                        None => pending.push((id, text)),
                    }
                }
                Ok(DomArg::Undefined)
            }
            // O texto do estilo em linha, pendente ou no atributo, sem sincronizar.
            "getStyle" => {
                let id = id_arg(args, 0)?;
                if let Some((_, text)) = self.pending_styles.borrow().iter().find(|(node, _)| *node == id) {
                    return Ok(DomArg::Str(text.clone()));
                }
                let doc = self.doc.borrow();
                let node = doc.get_node(id).ok_or_else(illegal)?;
                Ok(node.element_data().and_then(|e| e.attrs.iter().find(|a| attr_matches(&a.name, "style")).map(|a| a.value.to_string())).map_or(DomArg::Null, DomArg::Str))
            }
            "pageUrl" => Ok(DomArg::Str(self.url.borrow().clone())),
            "navigate" => {
                *self.navigation.borrow_mut() = Some(str_arg(args, 0)?.to_owned());
                Ok(DomArg::Undefined)
            }
            "fetch" => self.fetch(args),
            "root" => Ok(num(self.doc.borrow().root_node().id)),
            "docElement" => {
                let doc = self.doc.borrow();
                let root = match opt_id(args, 0) {
                    Some(id) => doc.get_node(id).ok_or_else(illegal)?,
                    None => doc.root_node(),
                };
                Ok(root.children.iter().copied().find(|&c| doc.get_node(c).is_some_and(|n| n.is_element())).map_or(DomArg::Null, num))
            }
            "doctype" => Ok(self.doctype.clone().map_or(DomArg::Null, DomArg::Str)),
            "kind" => {
                let doc = self.doc.borrow();
                let node = doc.get_node(id_arg(args, 0)?).ok_or_else(illegal)?;
                Ok(DomArg::Num(match &node.data {
                    NodeData::Element(e) if &*e.name.local == FRAGMENT => 11.0,
                    NodeData::Element(e) if &*e.name.local == SUBDOCUMENT => 9.0,
                    NodeData::Element(_) | NodeData::AnonymousBlock(_) => 1.0,
                    NodeData::Text(_) => 3.0,
                    NodeData::Comment { .. } => 8.0,
                    NodeData::Document(_) => 9.0,
                }))
            }
            "name" => {
                let doc = self.doc.borrow();
                let node = doc.get_node(id_arg(args, 0)?).ok_or_else(illegal)?;
                Ok(DomArg::Str(match &node.data {
                    NodeData::Element(e) => e.name.local.to_string(),
                    NodeData::Text(_) => "#text".to_owned(),
                    NodeData::Comment { .. } => "#comment".to_owned(),
                    _ => "#document".to_owned(),
                }))
            }
            "ns" => {
                let doc = self.doc.borrow();
                let node = doc.get_node(id_arg(args, 0)?).ok_or_else(illegal)?;
                Ok(node.element_data().map_or(DomArg::Null, |e| DomArg::Str(e.name.ns.to_string())))
            }
            "parent" => {
                let doc = self.doc.borrow();
                Ok(doc.get_node(id_arg(args, 0)?).and_then(|n| n.parent).map_or(DomArg::Null, num))
            }
            "children" => {
                let doc = self.doc.borrow();
                Ok(doc.get_node(id_arg(args, 0)?).map_or(DomArg::List(Vec::new()), |n| ids(n.children.iter().copied())))
            }
            "attrs" => {
                let doc = self.doc.borrow();
                let node = doc.get_node(id_arg(args, 0)?).ok_or_else(illegal)?;
                let mut flat = Vec::new();
                if let Some(element) = node.element_data() {
                    for attr in element.attrs.iter() {
                        flat.push(DomArg::Str(document::qualified_attr_name(&attr.name)));
                        flat.push(DomArg::Str(attr.value.clone()));
                    }
                }
                Ok(DomArg::List(flat))
            }
            "getAttr" => {
                let doc = self.doc.borrow();
                let wanted = str_arg(args, 1)?;
                let node = doc.get_node(id_arg(args, 0)?).ok_or_else(illegal)?;
                Ok(node
                    .element_data()
                    .and_then(|e| e.attrs.iter().find(|a| attr_matches(&a.name, wanted)))
                    .map_or(DomArg::Null, |a| DomArg::Str(a.value.clone())))
            }
            "setAttr" => {
                let id = id_arg(args, 0)?;
                let (name, value) = (str_arg(args, 1)?, str_arg(args, 2)?);
                // Mantém o QualName do atributo que já existe (com o namespace do `xlink:href`); senão, nome local puro.
                let existing = {
                    let doc = self.doc.borrow();
                    doc.get_node(id).and_then(|n| n.element_data()).and_then(|e| e.attrs.iter().find(|a| attr_matches(&a.name, name)).map(|a| a.name.clone()))
                };
                let qname = existing.unwrap_or_else(|| QualName::new(None, ns!(), LocalName::from(name)));
                self.mutate(|m| m.set_attribute(id, qname, value));
                Ok(DomArg::Undefined)
            }
            "box" => {
                let id = id_arg(args, 0)?;
                let found = document::layout_box(&mut self.doc.borrow_mut(), id);
                Ok(found.map_or(DomArg::Null, |values| DomArg::List(values.into_iter().map(DomArg::Num).collect())))
            }
            "setDefined" => {
                let id = id_arg(args, 0)?;
                self.doc.borrow_mut().get_node_mut(id).ok_or_else(illegal)?.set_defined();
                Ok(DomArg::Undefined)
            }
            "removeAttr" => {
                let id = id_arg(args, 0)?;
                let name = str_arg(args, 1)?;
                let existing = {
                    let doc = self.doc.borrow();
                    doc.get_node(id).and_then(|n| n.element_data()).and_then(|e| e.attrs.iter().find(|a| attr_matches(&a.name, name)).map(|a| a.name.clone()))
                };
                if let Some(qname) = existing {
                    self.mutate(|m| m.clear_attribute(id, qname));
                }
                Ok(DomArg::Undefined)
            }
            "data" => {
                let doc = self.doc.borrow();
                let node = doc.get_node(id_arg(args, 0)?).ok_or_else(illegal)?;
                Ok(DomArg::Str(match &node.data {
                    NodeData::Text(t) => t.content.clone(),
                    NodeData::Comment { contents } => contents.clone(),
                    _ => String::new(),
                }))
            }
            "setData" => {
                let id = id_arg(args, 0)?;
                let value = str_arg(args, 1)?;
                let mut doc = self.doc.borrow_mut();
                if let Some(node) = doc.get_node_mut(id) {
                    match &mut node.data {
                        NodeData::Text(t) => t.content = value.to_owned(),
                        NodeData::Comment { contents } => *contents = value.to_owned(),
                        _ => {}
                    }
                }
                Ok(DomArg::Undefined)
            }
            "createElement" => {
                let (local, namespace) = (str_arg(args, 0)?, str_arg(args, 1)?);
                let qname = QualName::new(None, Namespace::from(namespace), LocalName::from(local));
                Ok(num(self.mutate(|m| m.create_element(qname, Vec::<Attribute>::new()))))
            }
            // `DOMParser.parseFromString`: o texto é parseado num documento à parte (html5ever ou xml5ever) e a árvore
            // copiada para cá sob uma raiz de documento secundário.
            "parseDocument" => {
                let text = str_arg(args, 0)?;
                let xml = matches!(args.get(1), Some(DomArg::Bool(true)));
                let base = self.url.borrow().clone();
                let parsed = if xml { document::parse_xml(text, &base) } else { document::parse_at(text, &base) };
                Ok(num(self.mutate(|m| {
                    let root = m.create_element(QualName::new(None, ns!(), LocalName::from(SUBDOCUMENT)), Vec::new());
                    for &child in &parsed.root_node().children {
                        if let Some(copy) = copy_across(&parsed, child, m) {
                            m.append_children(root, &[copy]);
                        }
                    }
                    root
                })))
            }
            "createFragment" => {
                let qname = QualName::new(None, ns!(), LocalName::from(FRAGMENT));
                Ok(num(self.mutate(|m| m.create_element(qname, Vec::<Attribute>::new()))))
            }
            // `DOMImplementation.createHTMLDocument`: um documento à parte, com `html`, `head`, `body` e o `title`
            // pedido. A raiz é um elemento marcado, como o fragmento.
            "createDocument" => {
                let title = match args.first() {
                    Some(DomArg::Str(t)) => Some(t.clone()),
                    _ => None,
                };
                Ok(num(self.mutate(|m| {
                    let html_name = |local: &str| QualName::new(None, ns!(html), LocalName::from(local));
                    let root = m.create_element(QualName::new(None, ns!(), LocalName::from(SUBDOCUMENT)), Vec::new());
                    let html = m.create_element(html_name("html"), Vec::new());
                    let head = m.create_element(html_name("head"), Vec::new());
                    let body = m.create_element(html_name("body"), Vec::new());
                    if let Some(title) = &title {
                        let element = m.create_element(html_name("title"), Vec::new());
                        let text = m.create_text_node(title);
                        m.append_children(element, &[text]);
                        m.append_children(head, &[element]);
                    }
                    m.append_children(html, &[head, body]);
                    m.append_children(root, &[html]);
                    root
                })))
            }
            "createText" => Ok(num(self.mutate(|m| m.create_text_node(str_arg(args, 0).unwrap_or(""))))),
            "createComment" => Ok(num(self.mutate(|m| m.create_comment_node(str_arg(args, 0).unwrap_or(""))))),
            "append" => {
                let (parent, child) = (id_arg(args, 0)?, id_arg(args, 1)?);
                self.mutate(|m| {
                    let kids = Self::detach_for_insert(m, child);
                    m.append_children(parent, &kids);
                });
                Ok(DomArg::Undefined)
            }
            "insertBefore" => {
                let (parent, child) = (id_arg(args, 0)?, id_arg(args, 1)?);
                let reference = opt_id(args, 2);
                self.mutate(|m| {
                    let kids = Self::detach_for_insert(m, child);
                    match reference {
                        Some(anchor) => m.insert_nodes_before(anchor, &kids),
                        None => m.append_children(parent, &kids),
                    }
                });
                Ok(DomArg::Undefined)
            }
            "remove" => {
                let id = id_arg(args, 0)?;
                self.mutate(|m| {
                    if m.parent_id(id).is_some() {
                        m.remove_node(id);
                    }
                });
                Ok(DomArg::Undefined)
            }
            "clone" => {
                let id = id_arg(args, 0)?;
                let deep = matches!(args.get(1), Some(DomArg::Bool(true)));
                Ok(num(self.mutate(|m| Self::clone_node(m, id, deep))))
            }
            "byId" => Ok(self.doc.borrow().get_element_by_id(str_arg(args, 0)?).map_or(DomArg::Null, num)),
            "query" => {
                let scope = id_arg(args, 0)?;
                let selector = str_arg(args, 1)?;
                let all = matches!(args.get(2), Some(DomArg::Bool(true)));
                let doc = self.doc.borrow();
                let invalid = |_| err("SyntaxError", format!("'{selector}' is not a valid selector."));
                if all {
                    Ok(ids(doc.query_selector_all_in(scope, selector).map_err(invalid)?))
                } else {
                    Ok(doc.query_selector_in(scope, selector).map_err(invalid)?.map_or(DomArg::Null, num))
                }
            }
            "matches" => {
                let id = id_arg(args, 0)?;
                let selector = str_arg(args, 1)?;
                let doc = self.doc.borrow();
                let mut top = id;
                while let Some(parent) = doc.get_node(top).and_then(|n| n.parent) {
                    top = parent;
                }
                let found = doc.query_selector_all_in(top, selector).map_err(|_| err("SyntaxError", format!("'{selector}' is not a valid selector.")))?;
                Ok(DomArg::Bool(found.contains(&id)))
            }
            "normalizeStyle" => Ok(DomArg::Str(document::normalize_style(str_arg(args, 0)?))),
            "innerHTML" => Ok(DomArg::Str(document::inner_html(&self.doc.borrow(), id_arg(args, 0)?))),
            "outerHTML" => Ok(DomArg::Str(document::outer_html(&self.doc.borrow(), id_arg(args, 0)?))),
            "setInnerHTML" => {
                let id = id_arg(args, 0)?;
                let html = str_arg(args, 1)?;
                self.mutate(|m| document::set_inner_html(m, id, html));
                Ok(DomArg::Undefined)
            }
            "textContent" => {
                let doc = self.doc.borrow();
                Ok(DomArg::Str(doc.get_node(id_arg(args, 0)?).map(|n| n.text_content()).unwrap_or_default()))
            }
            "setTextContent" => {
                let id = id_arg(args, 0)?;
                let text = str_arg(args, 1)?;
                self.mutate(|m| {
                    document::detach_children(m, id);
                    if !text.is_empty() {
                        let node = m.create_text_node(text);
                        m.append_children(id, &[node]);
                    }
                });
                Ok(DomArg::Undefined)
            }
            _ => Err(illegal()),
        }
    }
}
