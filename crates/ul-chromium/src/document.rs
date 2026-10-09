//! O documento da página: um `BaseDocument` do Blitz, com o parser do html5ever ligado como o Chromium o liga
//! (scripting ligado, doctype preservado).

use blitz_dom::node::NodeData;
use blitz_dom::{BaseDocument, DocumentConfig, NodeId};
use blitz_html::DocumentHtmlParser;
use html5ever::ParseOpts;
use html5ever::tendril::TendrilSink;
use html5ever::tree_builder::TreeBuilderOpts;

/// Parseia `html` num documento novo, com as opções do navegador (scripting ligado: `<noscript>` vira texto).
pub fn parse(html: &str) -> BaseDocument {
    let mut doc = BaseDocument::new(DocumentConfig::default());
    {
        let mut mutr = doc.mutate();
        let sink = DocumentHtmlParser::new(&mut mutr);
        let opts = ParseOpts {
            tree_builder: TreeBuilderOpts { scripting_enabled: true, drop_doctype: false, ..Default::default() },
            ..Default::default()
        };
        html5ever::parse_document(sink, opts).from_utf8().read_from(&mut html.as_bytes()).unwrap();
    }
    doc
}

/// Elementos sem conteúdo nem tag de fechamento.
const VOID: &[&str] =
    &["area", "base", "basefont", "bgsound", "br", "col", "embed", "frame", "hr", "img", "input", "keygen", "link", "meta", "param", "source", "track", "wbr"];
/// Elementos cujo texto sai sem escape (`noscript` entra porque o scripting está ligado).
const RAW_TEXT: &[&str] = &["style", "script", "xmp", "iframe", "noembed", "noframes", "plaintext", "noscript"];

fn escape(out: &mut String, text: &str, attribute: bool) {
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '\u{a0}' => out.push_str("&nbsp;"),
            '"' if attribute => out.push_str("&quot;"),
            '<' if !attribute => out.push_str("&lt;"),
            '>' if !attribute => out.push_str("&gt;"),
            c => out.push(c),
        }
    }
}

/// `outerHTML` do nó `id`, pela serialização de fragmento da especificação.
pub fn outer_html(doc: &BaseDocument, id: NodeId) -> String {
    let mut out = String::new();
    write_node(doc, id, &mut out);
    out
}

/// `innerHTML` do nó `id`.
pub fn inner_html(doc: &BaseDocument, id: NodeId) -> String {
    let mut out = String::new();
    write_children(doc, id, &mut out);
    out
}

fn write_children(doc: &BaseDocument, id: NodeId, out: &mut String) {
    let Some(node) = doc.get_node(id) else { return };
    let raw = node.element_data().is_some_and(|e| RAW_TEXT.contains(&&*e.name.local));
    for &child in &node.children {
        if raw {
            if let Some(text) = doc.get_node(child).and_then(|c| c.text_data()) {
                out.push_str(&text.content);
                continue;
            }
        }
        write_node(doc, child, out);
    }
}

fn write_node(doc: &BaseDocument, id: NodeId, out: &mut String) {
    let Some(node) = doc.get_node(id) else { return };
    match &node.data {
        NodeData::Element(element) => {
            let name: &str = &element.name.local;
            out.push('<');
            out.push_str(name);
            for attr in element.attrs.iter() {
                out.push(' ');
                out.push_str(&attr.name.local);
                out.push_str("=\"");
                escape(out, &attr.value, true);
                out.push('"');
            }
            out.push('>');
            if !VOID.contains(&name) {
                write_children(doc, id, out);
                out.push_str("</");
                out.push_str(name);
                out.push('>');
            }
        }
        NodeData::Text(text) => escape(out, &text.content, false),
        NodeData::Comment { contents } => {
            out.push_str("<!--");
            out.push_str(contents);
            out.push_str("-->");
        }
        NodeData::Document(_) => write_children(doc, id, out),
        NodeData::AnonymousBlock(_) => {}
    }
}

/// O doctype do texto de origem, já na forma que o Chromium serializa (`<!DOCTYPE html>`), ou `None`.
/// O Blitz não tem nó de doctype, então ele é lido do próprio texto.
pub fn sniff_doctype(source: &str) -> Option<String> {
    let mut rest = source.trim_start_matches('\u{feff}');
    loop {
        rest = rest.trim_start_matches([' ', '\t', '\n', '\r', '\x0c']);
        if let Some(after) = rest.strip_prefix("<!--") {
            rest = &after[after.find("-->")? + 3..];
        } else {
            break;
        }
    }
    let head = rest.get(..9)?;
    if !head.eq_ignore_ascii_case("<!doctype") {
        return None;
    }
    let body = &rest[9..];
    let body = &body[..body.find('>')?];
    let mut words = DoctypeWords(body.trim());
    let name = words.next_word().unwrap_or("").to_ascii_lowercase();
    let mut out = format!("<!DOCTYPE {name}");
    match words.next_word().map(str::to_ascii_uppercase).as_deref() {
        Some("PUBLIC") => {
            let public = words.next_quoted().unwrap_or_default();
            out.push_str(&format!(" PUBLIC \"{public}\""));
            if let Some(system) = words.next_quoted() {
                out.push_str(&format!(" \"{system}\""));
            }
        }
        Some("SYSTEM") => {
            let system = words.next_quoted().unwrap_or_default();
            out.push_str(&format!(" \"\" \"{system}\""));
        }
        _ => {}
    }
    out.push('>');
    Some(out)
}

struct DoctypeWords<'a>(&'a str);

impl<'a> DoctypeWords<'a> {
    fn next_word(&mut self) -> Option<&'a str> {
        let s = self.0.trim_start();
        if s.is_empty() {
            return None;
        }
        let end = s.find(|c: char| c.is_ascii_whitespace()).unwrap_or(s.len());
        self.0 = &s[end..];
        Some(&s[..end])
    }

    fn next_quoted(&mut self) -> Option<&'a str> {
        let s = self.0.trim_start();
        let quote = s.chars().next().filter(|c| *c == '"' || *c == '\'')?;
        let inner = &s[1..];
        let end = inner.find(quote)?;
        self.0 = &inner[end + 1..];
        Some(&inner[..end])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_like_the_spec() {
        let doc = parse("<!DOCTYPE html><title>T</title><p a='x\"y&amp;'>x&lt;y<br><!--c--></p><script>if(a<b){}</script>");
        let root = doc.root_element().id;
        assert_eq!(
            outer_html(&doc, root),
            "<html><head><title>T</title></head><body><p a=\"x&quot;y&amp;\">x&lt;y<br><!--c--></p><script>if(a<b){}</script></body></html>"
        );
        assert_eq!(sniff_doctype("  <!doctype HTML>"), Some("<!DOCTYPE html>".into()));
        assert_eq!(sniff_doctype("<p>"), None);
    }

    #[test]
    fn parses_and_queries() {
        let doc = parse("<!DOCTYPE html><title>T</title><h1 id=h>a</h1><p>x&lt;y</p>");
        let h1 = doc.get_element_by_id("h").unwrap();
        assert_eq!(doc.get_node(h1).unwrap().text_content(), "a");
    }
}
