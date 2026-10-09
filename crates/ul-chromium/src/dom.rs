//! A árvore DOM do navegador: uma arena de nós com identidade por índice, a serialização HTML da especificação e o
//! `TreeSink` do html5ever. O JavaScript enxerga os nós por esses índices (ver `bridge.rs`).

use std::cell::{Ref, RefCell};
use std::rc::Rc;

use html5ever::tendril::{StrTendril, TendrilSink};
use html5ever::tree_builder::{ElementFlags, NodeOrText, QuirksMode, TreeSink};
use html5ever::{Attribute, LocalName, QualName, ns, parse_document, parse_fragment, local_name};

pub type NodeId = usize;

pub const DOCUMENT_NODE: u8 = 9;
pub const ELEMENT_NODE: u8 = 1;
pub const TEXT_NODE: u8 = 3;
pub const COMMENT_NODE: u8 = 8;
pub const PROCESSING_INSTRUCTION_NODE: u8 = 7;
pub const DOCUMENT_TYPE_NODE: u8 = 10;
pub const DOCUMENT_FRAGMENT_NODE: u8 = 11;

#[derive(Clone, Debug)]
pub struct Attr {
    pub name: QualName,
    pub value: String,
}

#[derive(Clone, Debug)]
pub enum NodeData {
    Document,
    Fragment,
    Doctype { name: String, public_id: String, system_id: String },
    Element { name: QualName, attrs: Vec<Attr>, template_contents: Option<NodeId>, script_started: bool },
    Text(String),
    Comment(String),
    ProcessingInstruction { target: String, data: String },
}

#[derive(Clone, Debug)]
pub struct Node {
    pub data: NodeData,
    pub parent: Option<NodeId>,
    pub children: Vec<NodeId>,
}

#[derive(Debug)]
pub struct Dom {
    pub nodes: Vec<Node>,
    pub quirks: bool,
}

pub type DomRef = Rc<RefCell<Dom>>;

const VOID: &[&str] = &[
    "area", "base", "basefont", "bgsound", "br", "col", "embed", "frame", "hr", "img", "input", "keygen", "link", "meta", "param", "source", "track", "wbr",
];
const RAW_TEXT: &[&str] = &["style", "script", "xmp", "iframe", "noembed", "noframes", "plaintext", "noscript"];

impl Default for Dom {
    fn default() -> Self {
        Dom::new()
    }
}

impl Dom {
    /// Um documento vazio no nó 0.
    pub fn new() -> Dom {
        Dom { nodes: vec![Node { data: NodeData::Document, parent: None, children: Vec::new() }], quirks: false }
    }

    pub fn alloc(&mut self, data: NodeData) -> NodeId {
        self.nodes.push(Node { data, parent: None, children: Vec::new() });
        self.nodes.len() - 1
    }

    pub fn node_type(&self, id: NodeId) -> u8 {
        match &self.nodes[id].data {
            NodeData::Document => DOCUMENT_NODE,
            NodeData::Fragment => DOCUMENT_FRAGMENT_NODE,
            NodeData::Doctype { .. } => DOCUMENT_TYPE_NODE,
            NodeData::Element { .. } => ELEMENT_NODE,
            NodeData::Text(_) => TEXT_NODE,
            NodeData::Comment(_) => COMMENT_NODE,
            NodeData::ProcessingInstruction { .. } => PROCESSING_INSTRUCTION_NODE,
        }
    }

    pub fn qname(&self, id: NodeId) -> Option<&QualName> {
        match &self.nodes[id].data {
            NodeData::Element { name, .. } => Some(name),
            _ => None,
        }
    }

    /// O nome local de um elemento HTML (sem prefixo), em minúsculas como o parser entrega.
    pub fn local(&self, id: NodeId) -> Option<&str> {
        self.qname(id).map(|name| &*name.local)
    }

    pub fn is_html(&self, id: NodeId, local: &str) -> bool {
        self.qname(id).is_some_and(|name| name.ns == ns!(html) && &*name.local == local)
    }

    pub fn attrs(&self, id: NodeId) -> &[Attr] {
        match &self.nodes[id].data {
            NodeData::Element { attrs, .. } => attrs,
            _ => &[],
        }
    }

    pub fn attr(&self, id: NodeId, name: &str) -> Option<&str> {
        self.attrs(id).iter().find(|attr| attr.name.ns == ns!() && &*attr.name.local == name).map(|attr| attr.value.as_str())
    }

    pub fn set_attr(&mut self, id: NodeId, name: &str, value: &str) {
        let NodeData::Element { attrs, .. } = &mut self.nodes[id].data else { return };
        if let Some(existing) = attrs.iter_mut().find(|attr| attr.name.ns == ns!() && &*attr.name.local == name) {
            existing.value = value.to_owned();
        } else {
            attrs.push(Attr { name: QualName::new(None, ns!(), LocalName::from(name)), value: value.to_owned() });
        }
    }

    pub fn remove_attr(&mut self, id: NodeId, name: &str) -> bool {
        let NodeData::Element { attrs, .. } = &mut self.nodes[id].data else { return false };
        let before = attrs.len();
        attrs.retain(|attr| !(attr.name.ns == ns!() && &*attr.name.local == name));
        attrs.len() != before
    }

    pub fn detach(&mut self, id: NodeId) {
        if let Some(parent) = self.nodes[id].parent.take() {
            self.nodes[parent].children.retain(|child| *child != id);
        }
    }

    /// Insere `child` em `parent` antes de `before` (ou no fim), tirando-o do pai antigo.
    pub fn insert(&mut self, parent: NodeId, child: NodeId, before: Option<NodeId>) {
        self.detach(child);
        let index = before.and_then(|b| self.nodes[parent].children.iter().position(|c| *c == b)).unwrap_or(self.nodes[parent].children.len());
        self.nodes[parent].children.insert(index, child);
        self.nodes[child].parent = Some(parent);
    }

    /// Acrescenta texto, juntando ao último filho quando ele é texto.
    pub fn append_text(&mut self, parent: NodeId, text: &str) {
        if let Some(&last) = self.nodes[parent].children.last() {
            if let NodeData::Text(existing) = &mut self.nodes[last].data {
                existing.push_str(text);
                return;
            }
        }
        let node = self.alloc(NodeData::Text(text.to_owned()));
        self.insert(parent, node, None);
    }

    pub fn is_ancestor(&self, ancestor: NodeId, mut node: NodeId) -> bool {
        loop {
            if node == ancestor {
                return true;
            }
            match self.nodes[node].parent {
                Some(parent) => node = parent,
                None => return false,
            }
        }
    }

    /// O `textContent` do nó.
    pub fn text_content(&self, id: NodeId) -> Option<String> {
        match &self.nodes[id].data {
            NodeData::Document | NodeData::Doctype { .. } => None,
            NodeData::Text(text) | NodeData::Comment(text) => Some(text.clone()),
            NodeData::ProcessingInstruction { data, .. } => Some(data.clone()),
            _ => {
                let mut out = String::new();
                self.collect_text(id, &mut out);
                Some(out)
            }
        }
    }

    fn collect_text(&self, id: NodeId, out: &mut String) {
        for &child in &self.nodes[id].children {
            match &self.nodes[child].data {
                NodeData::Text(text) => out.push_str(text),
                NodeData::Element { .. } => self.collect_text(child, out),
                _ => {}
            }
        }
    }

    pub fn clone_node(&mut self, id: NodeId, deep: bool) -> NodeId {
        let source = self.nodes[id].data.clone();
        let data = match source {
            NodeData::Element { name, attrs, template_contents, .. } => {
                let contents = template_contents.map(|contents| self.clone_node(contents, deep));
                NodeData::Element { name, attrs, template_contents: contents, script_started: false }
            }
            other => other,
        };
        let copy = self.alloc(data);
        if deep && !matches!(self.nodes[id].data, NodeData::Element { template_contents: Some(_), .. }) {
            for child in self.nodes[id].children.clone() {
                let child_copy = self.clone_node(child, true);
                self.insert(copy, child_copy, None);
            }
        }
        copy
    }

    /// Todos os descendentes em ordem de documento.
    pub fn descendants(&self, id: NodeId) -> Vec<NodeId> {
        let mut out = Vec::new();
        let mut stack: Vec<NodeId> = self.nodes[id].children.iter().rev().copied().collect();
        while let Some(node) = stack.pop() {
            out.push(node);
            stack.extend(self.nodes[node].children.iter().rev().copied());
        }
        out
    }

    pub fn document_element(&self) -> Option<NodeId> {
        self.nodes[0].children.iter().copied().find(|c| matches!(self.nodes[*c].data, NodeData::Element { .. }))
    }

    // ---- serialização ----

    /// `outerHTML` do nó (ou o próprio texto escapado, o comentário, o doctype).
    pub fn serialize_node(&self, id: NodeId, out: &mut String) {
        match &self.nodes[id].data {
            NodeData::Document | NodeData::Fragment => self.serialize_children(id, out),
            NodeData::Doctype { name, .. } => {
                out.push_str("<!DOCTYPE ");
                out.push_str(name);
                out.push('>');
            }
            NodeData::Text(text) => {
                let raw = self.nodes[id].parent.is_some_and(|p| self.qname(p).is_some_and(|n| n.ns == ns!(html) && RAW_TEXT.contains(&&*n.local)));
                if raw { out.push_str(text) } else { escape(text, false, out) }
            }
            NodeData::Comment(text) => {
                out.push_str("<!--");
                out.push_str(text);
                out.push_str("-->");
            }
            NodeData::ProcessingInstruction { target, data } => {
                out.push_str("<?");
                out.push_str(target);
                out.push(' ');
                out.push_str(data);
                out.push('>');
            }
            NodeData::Element { name, attrs, template_contents, .. } => {
                let tag = qualified(name);
                out.push('<');
                out.push_str(&tag);
                for attr in attrs {
                    out.push(' ');
                    out.push_str(&attr_name(&attr.name));
                    out.push_str("=\"");
                    escape(&attr.value, true, out);
                    out.push('"');
                }
                out.push('>');
                if name.ns == ns!(html) && VOID.contains(&&*name.local) {
                    return;
                }
                match template_contents {
                    Some(contents) if name.ns == ns!(html) => self.serialize_children(*contents, out),
                    _ => self.serialize_children(id, out),
                }
                out.push_str("</");
                out.push_str(&tag);
                out.push('>');
            }
        }
    }

    pub fn serialize_children(&self, id: NodeId, out: &mut String) {
        for &child in &self.nodes[id].children {
            self.serialize_node(child, out);
        }
    }
}

fn qualified(name: &QualName) -> String {
    match &name.prefix {
        Some(prefix) if name.ns != ns!(html) && name.ns != ns!(svg) && name.ns != ns!(mathml) => format!("{prefix}:{}", name.local),
        _ => name.local.to_string(),
    }
}

fn attr_name(name: &QualName) -> String {
    if name.ns == ns!() {
        return name.local.to_string();
    }
    if name.ns == ns!(xml) {
        return format!("xml:{}", name.local);
    }
    if name.ns == ns!(xmlns) {
        return if &*name.local == "xmlns" { "xmlns".to_owned() } else { format!("xmlns:{}", name.local) };
    }
    if name.ns == ns!(xlink) {
        return format!("xlink:{}", name.local);
    }
    match &name.prefix {
        Some(prefix) => format!("{prefix}:{}", name.local),
        None => name.local.to_string(),
    }
}

/// Escapa texto (`&`, U+00A0, `<`, `>`) ou valor de atributo (`&`, U+00A0, `"`, e `<`/`>` como o Chromium atual).
pub fn escape(text: &str, attribute: bool, out: &mut String) {
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '\u{a0}' => out.push_str("&nbsp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' if attribute => out.push_str("&quot;"),
            c => out.push(c),
        }
    }
}

// ---- TreeSink ----

/// O `TreeSink` que o html5ever usa sobre a arena compartilhada.
pub struct Sink {
    pub dom: DomRef,
}

impl TreeSink for Sink {
    type Handle = NodeId;
    type Output = ();
    type ElemName<'a> = Ref<'a, QualName>;

    fn finish(self) -> Self::Output {}

    fn parse_error(&self, _msg: std::borrow::Cow<'static, str>) {}

    fn get_document(&self) -> NodeId {
        0
    }

    fn elem_name<'a>(&'a self, target: &'a NodeId) -> Ref<'a, QualName> {
        Ref::map(self.dom.borrow(), |dom| dom.qname(*target).expect("elem_name de um nó que não é elemento"))
    }

    fn create_element(&self, name: QualName, attrs: Vec<Attribute>, flags: ElementFlags) -> NodeId {
        let mut dom = self.dom.borrow_mut();
        let template_contents = flags.template.then(|| dom.alloc(NodeData::Fragment));
        let attrs = attrs.into_iter().map(|attr| Attr { name: attr.name, value: attr.value.to_string() }).collect();
        dom.alloc(NodeData::Element { name, attrs, template_contents, script_started: false })
    }

    fn create_comment(&self, text: StrTendril) -> NodeId {
        self.dom.borrow_mut().alloc(NodeData::Comment(text.to_string()))
    }

    fn create_pi(&self, target: StrTendril, data: StrTendril) -> NodeId {
        self.dom.borrow_mut().alloc(NodeData::ProcessingInstruction { target: target.to_string(), data: data.to_string() })
    }

    fn append(&self, parent: &NodeId, child: NodeOrText<NodeId>) {
        let mut dom = self.dom.borrow_mut();
        match child {
            NodeOrText::AppendNode(node) => dom.insert(*parent, node, None),
            NodeOrText::AppendText(text) => dom.append_text(*parent, &text),
        }
    }

    fn append_based_on_parent_node(&self, element: &NodeId, prev_element: &NodeId, child: NodeOrText<NodeId>) {
        if self.dom.borrow().nodes[*element].parent.is_some() {
            self.append_before_sibling(element, child);
        } else {
            self.append(prev_element, child);
        }
    }

    fn append_doctype_to_document(&self, name: StrTendril, public_id: StrTendril, system_id: StrTendril) {
        let mut dom = self.dom.borrow_mut();
        let node = dom.alloc(NodeData::Doctype { name: name.to_string(), public_id: public_id.to_string(), system_id: system_id.to_string() });
        dom.insert(0, node, None);
    }

    fn mark_script_already_started(&self, node: &NodeId) {
        if let NodeData::Element { script_started, .. } = &mut self.dom.borrow_mut().nodes[*node].data {
            *script_started = true;
        }
    }

    fn get_template_contents(&self, target: &NodeId) -> NodeId {
        match &self.dom.borrow().nodes[*target].data {
            NodeData::Element { template_contents: Some(contents), .. } => *contents,
            _ => panic!("get_template_contents de um elemento que não é template"),
        }
    }

    fn same_node(&self, x: &NodeId, y: &NodeId) -> bool {
        x == y
    }

    fn set_quirks_mode(&self, mode: QuirksMode) {
        self.dom.borrow_mut().quirks = mode == QuirksMode::Quirks;
    }

    fn append_before_sibling(&self, sibling: &NodeId, new_node: NodeOrText<NodeId>) {
        let mut dom = self.dom.borrow_mut();
        let Some(parent) = dom.nodes[*sibling].parent else { return };
        match new_node {
            NodeOrText::AppendNode(node) => dom.insert(parent, node, Some(*sibling)),
            NodeOrText::AppendText(text) => {
                let index = dom.nodes[parent].children.iter().position(|c| c == sibling).unwrap_or(0);
                if index > 0 {
                    let previous = dom.nodes[parent].children[index - 1];
                    if let NodeData::Text(existing) = &mut dom.nodes[previous].data {
                        existing.push_str(&text);
                        return;
                    }
                }
                let node = dom.alloc(NodeData::Text(text.to_string()));
                dom.insert(parent, node, Some(*sibling));
            }
        }
    }

    fn add_attrs_if_missing(&self, target: &NodeId, new_attrs: Vec<Attribute>) {
        let mut dom = self.dom.borrow_mut();
        let NodeData::Element { attrs, .. } = &mut dom.nodes[*target].data else { return };
        for attr in new_attrs {
            if !attrs.iter().any(|existing| existing.name == attr.name) {
                attrs.push(Attr { name: attr.name, value: attr.value.to_string() });
            }
        }
    }

    fn remove_from_parent(&self, target: &NodeId) {
        self.dom.borrow_mut().detach(*target);
    }

    fn reparent_children(&self, node: &NodeId, new_parent: &NodeId) {
        let mut dom = self.dom.borrow_mut();
        let children = std::mem::take(&mut dom.nodes[*node].children);
        for child in children {
            dom.nodes[child].parent = Some(*new_parent);
            dom.nodes[*new_parent].children.push(child);
        }
    }
}

/// Monta o documento de `html` (texto já decodificado).
pub fn parse_html(html: &str) -> DomRef {
    let dom = Rc::new(RefCell::new(Dom::new()));
    let parser = parse_document(Sink { dom: dom.clone() }, html5ever::ParseOpts::default());
    parser.one(html);
    dom
}

/// Analisa `html` como fragmento no contexto do elemento `context` e devolve os nós novos (soltos, sem pai).
pub fn parse_fragment_into(dom: &DomRef, context: NodeId, html: &str) -> Vec<NodeId> {
    let (name, attrs) = {
        let borrowed = dom.borrow();
        let name = borrowed.qname(context).cloned().unwrap_or_else(|| QualName::new(None, ns!(html), local_name!("body")));
        let attrs: Vec<Attribute> = borrowed.attrs(context).iter().map(|a| Attribute { name: a.name.clone(), value: StrTendril::from(a.value.as_str()) }).collect();
        (name, attrs)
    };
    // O fragmento nasce numa arena própria (o html5ever cria o contexto e o nó raiz nela) e depois é copiado.
    let scratch = Rc::new(RefCell::new(Dom::new()));
    let parser = parse_fragment(Sink { dom: scratch.clone() }, html5ever::ParseOpts::default(), name, attrs, true);
    parser.one(html);
    let scratch = scratch.borrow();
    // Raiz do fragmento: o único filho do documento (o elemento `html` do contexto).
    let root = scratch.nodes[0].children.first().copied();
    let mut out = Vec::new();
    if let Some(root) = root {
        for &child in &scratch.nodes[root].children {
            out.push(copy_between(&scratch, child, &mut dom.borrow_mut()));
        }
    }
    out
}

fn copy_between(from: &Dom, id: NodeId, into: &mut Dom) -> NodeId {
    let data = match &from.nodes[id].data {
        NodeData::Element { name, attrs, template_contents, .. } => {
            let contents = template_contents.map(|contents| copy_between(from, contents, into));
            NodeData::Element { name: name.clone(), attrs: attrs.clone(), template_contents: contents, script_started: false }
        }
        other => other.clone(),
    };
    let copy = into.alloc(data);
    for &child in &from.nodes[id].children {
        let child_copy = copy_between(from, child, into);
        into.insert(copy, child_copy, None);
    }
    copy
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dump(html: &str) -> String {
        let dom = parse_html(html);
        let mut out = String::new();
        dom.borrow().serialize_node(0, &mut out);
        out
    }

    #[test]
    fn serializes_the_normalized_document() {
        assert_eq!(dump("<p>a<br>b</p>"), "<html><head></head><body><p>a<br>b</p></body></html>");
        assert_eq!(dump("<!DOCTYPE html><title>t</title>x&lt;y"), "<!DOCTYPE html><html><head><title>t</title></head><body>x&lt;y</body></html>");
    }

    #[test]
    fn script_text_is_raw_and_attributes_are_escaped() {
        assert_eq!(
            dump("<script>if (a<b) x()</script><a href='q&amp;\"'>"),
            "<html><head><script>if (a<b) x()</script></head><body><a href=\"q&amp;&quot;\"></a></body></html>"
        );
    }

    #[test]
    fn fragment_parsing_uses_the_context() {
        let dom = parse_html("<table></table>");
        let table = dom.borrow().descendants(0).into_iter().find(|n| dom.borrow().is_html(*n, "table")).unwrap();
        let nodes = parse_fragment_into(&dom, table, "<tr><td>1</td></tr>");
        let mut out = String::new();
        for n in nodes {
            dom.borrow().serialize_node(n, &mut out);
        }
        assert_eq!(out, "<tbody><tr><td>1</td></tr></tbody>");
    }
}
