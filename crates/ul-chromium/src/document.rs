//! O documento da página: um `BaseDocument` do Blitz, com o parser do html5ever ligado como o Chromium o liga
//! (scripting ligado, doctype preservado).

use std::sync::{Arc, Mutex};

use blitz_dom::node::NodeData;
use blitz_dom::{BaseDocument, DocumentConfig, DocumentMutator, NodeId};
use blitz_html::DocumentHtmlParser;
use blitz_traits::net::{NetHandler, NetProvider, Request};
use blitz_traits::shell::{ColorScheme, Viewport};
use html5ever::ParseOpts;
use html5ever::tendril::TendrilSink;
use html5ever::tree_builder::TreeBuilderOpts;
use markup5ever::{LocalName, QualName, ns};

/// Parseia `html` num documento novo, com as opções do navegador (scripting ligado: `<noscript>` vira texto).
pub fn parse(html: &str) -> BaseDocument {
    parse_at(html, "about:blank")
}

/// A janela de conteúdo do `chromium --headless`, medida no oráculo: `--window-size` padrão de 800x600, menos a moldura
/// que ele ainda desconta (innerWidth 780, innerHeight 493; outer 780x580).
pub const WINDOW: (u32, u32) = (780, 493);

fn config(base_url: &str) -> DocumentConfig {
    DocumentConfig { base_url: Some(base_url.to_owned()), viewport: Some(Viewport::new(WINDOW.0, WINDOW.1, 1.0, ColorScheme::Light)), ..DocumentConfig::default() }
}

/// [`parse`] com a URL do documento como base (o Blitz resolve por ela os `href` de folhas e imagens). Documento
/// inerte (o do `DOMParser`): não busca subrecursos.
pub fn parse_at(html: &str, base_url: &str) -> BaseDocument {
    parse_with(html, config(base_url))
}

/// O documento da página: as folhas de estilo, imagens e fontes que o Blitz pede ficam na fila de `net`, que o
/// hospedeiro serve pela rede dele ([`QueuedNet::serve`]).
pub fn parse_page(html: &str, base_url: &str, net: Arc<QueuedNet>) -> BaseDocument {
    parse_with(html, DocumentConfig { net_provider: Some(net), ..config(base_url) })
}

fn parse_with(html: &str, config: DocumentConfig) -> BaseDocument {
    let mut doc = BaseDocument::new(config);
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

/// O provedor de rede do Blitz para a página: guarda os pedidos (o Blitz exige `Send + Sync`, e a rede do hospedeiro é
/// de uma thread só) até o hospedeiro servi-los.
#[derive(Default)]
pub struct QueuedNet(Mutex<Vec<(Request, Box<dyn NetHandler>)>>);

impl NetProvider for QueuedNet {
    fn fetch(&self, _doc_id: usize, request: Request, handler: Box<dyn NetHandler>) {
        self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push((request, handler));
    }
}

impl QueuedNet {
    /// Serve os pedidos na fila com `fetch` (corpo de sucesso, ou `None`) e aplica as respostas ao documento. Um recurso
    /// pode pedir outros (`@import`): repete até a fila esvaziar.
    pub fn serve(&self, doc: &mut BaseDocument, fetch: &dyn Fn(&str) -> Option<Vec<u8>>) {
        loop {
            let pending = std::mem::take(&mut *self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner));
            if pending.is_empty() {
                break;
            }
            for (request, handler) in pending {
                // A falha também responde (corpo vazio): uma folha do `<head>` pendente segura o estilo do documento
                // inteiro no Blitz, e no Chromium a folha que não chega só não tem regras.
                let body = fetch(request.url.as_str()).unwrap_or_default();
                handler.bytes(request.url.to_string(), body.into());
            }
            doc.handle_messages();
        }
    }
}

/// A caixa de `id` em coordenadas da janela, depois de estilo e layout: `[x, y, largura, altura]` da caixa de borda
/// e as bordas e preenchimentos (`[topo, direita, base, esquerda]` de cada), e por fim a largura da barra vertical e
/// a altura da horizontal. `None` para nó que não é elemento.
pub fn layout_box(doc: &mut BaseDocument, id: NodeId) -> Option<[f64; 14]> {
    doc.resolve(0.0);
    let node = doc.get_node(id)?;
    if !node.is_element() {
        return None;
    }
    // Sob `display: none` (no próprio nó ou num ancestral) não há caixa: medidas zeradas, sem interseção.
    let mut current = Some(id);
    while let Some(at) = current {
        let ancestor = doc.get_node(at)?;
        if ancestor.primary_styles().is_some_and(|s| s.get_box().display.is_none()) {
            return None;
        }
        current = ancestor.parent;
    }
    // O layout sem o arredondamento do taffy: o Chromium mede em frações (21.44 do margin do h1, não 21).
    let layout = node.unrounded_layout();
    let (mut x, mut y) = (layout.location.x, layout.location.y);
    let mut parent = node.layout_parent.get();
    while let Some(id) = parent {
        let ancestor = doc.get_node(id)?;
        x += ancestor.unrounded_layout().location.x - ancestor.scroll_offset().x as f32;
        y += ancestor.unrounded_layout().location.y - ancestor.scroll_offset().y as f32;
        parent = ancestor.layout_parent.get();
    }
    let (b, p) = (layout.border, layout.padding);
    Some([
        f64::from(x),
        f64::from(y),
        f64::from(layout.size.width),
        f64::from(layout.size.height),
        f64::from(b.top),
        f64::from(b.right),
        f64::from(b.bottom),
        f64::from(b.left),
        f64::from(p.top),
        f64::from(p.right),
        f64::from(p.bottom),
        f64::from(p.left),
        f64::from(layout.scrollbar_size.width),
        f64::from(layout.scrollbar_size.height),
    ])
}

/// Parseia `xml` (o `application/xml` do `DOMParser`) com o xml5ever, num documento novo.
pub fn parse_xml(xml: &str, base_url: &str) -> BaseDocument {
    let mut doc = BaseDocument::new(config(base_url));
    {
        let mut mutr = doc.mutate();
        DocumentHtmlParser::parse_xml_into_mutator(&mut mutr, xml);
    }
    doc
}

/// Nonce hiding (HTML, "nonce attributes"): com CSP entregue por cabeçalho, o atributo `nonce` de cada elemento fica
/// vazio depois da inserção (a propriedade `nonce` guardaria o valor; o DOM serializado mostra `nonce=""`).
pub fn hide_nonces(doc: &mut BaseDocument) {
    let targets: Vec<NodeId> = doc.query_selector_all("[nonce]").map(|found| found.to_vec()).unwrap_or_default();
    let mut mutr = doc.mutate();
    for id in targets {
        mutr.set_attribute(id, QualName::new(None, markup5ever::ns!(), markup5ever::local_name!("nonce")), "");
    }
}

/// A serialização canônica de um bloco de declarações (`element.style.cssText` depois de uma escrita via CSSOM):
/// valores computados de especificação (`0` vira `0px`), aliases trocados (`word-wrap` vira `overflow-wrap`) e o
/// que não parseia descartado. É o stylo do Blitz.
pub fn normalize_style(text: &str) -> String {
    use style::properties::parse_style_attribute;
    use style::stylesheets::{CssRuleType, UrlExtraData};
    let base = url::Url::parse("about:blank").expect("about:blank");
    let data = UrlExtraData(style::servo_arc::Arc::new(base));
    let text = blink_precision(text);
    let block = parse_style_attribute(&text, &data, None, selectors::matching::QuirksMode::NoQuirks, CssRuleType::Style);
    let mut out = String::new();
    let _ = block.to_css(&mut out);
    out
}

/// O valor computado de `name` no elemento `id`, como o stylo o serializa (o `getComputedStyle`). Shorthand só tem
/// valor quando todas as longhands dela coincidem. `None` para nome desconhecido ou nó sem estilo.
pub fn computed_value(doc: &mut BaseDocument, id: NodeId, name: &str) -> Option<String> {
    use style::properties::{PropertyDeclarationId, PropertyId};
    doc.resolve(0.0);
    let node = doc.get_node(id)?;
    let styles = node.primary_styles()?;
    let property = PropertyId::parse_enabled_for_all_content(name).ok()?;
    match property.as_shorthand() {
        Err(declaration) => Some(styles.computed_value_to_string(declaration)),
        Ok(shorthand) => {
            let mut values = shorthand.longhands().map(|longhand| styles.computed_value_to_string(PropertyDeclarationId::Longhand(longhand)));
            let first = values.next()?;
            Some(if values.all(|value| value == first) { first } else { String::new() })
        }
    }
}

/// Se a media query casa com a janela do documento (o `matchMedia`), pelo parser e avaliador do stylo.
pub fn media_matches(doc: &mut BaseDocument, query: &str) -> bool {
    use style::media_queries::MediaList;
    use style::parser::ParserContext;
    use style::stylesheets::{CssRuleType, CustomMediaEvaluator, Namespaces, Origin, UrlExtraData};
    let base = url::Url::parse("about:blank").expect("about:blank");
    let data = UrlExtraData(style::servo_arc::Arc::new(base));
    let mut context = ParserContext::new(
        Origin::Author,
        &data,
        Some(CssRuleType::Media),
        style_traits::ParsingMode::DEFAULT,
        selectors::matching::QuirksMode::NoQuirks,
        std::borrow::Cow::Owned(Namespaces::default()),
        None,
        None,
        Default::default(),
    );
    let mut input = cssparser::ParserInput::new(query);
    let list = MediaList::parse(&mut context, &mut cssparser::Parser::new(&mut input));
    list.evaluate(doc.stylist_device(), selectors::matching::QuirksMode::NoQuirks, &mut CustomMediaEvaluator::none())
}

/// O Blink guarda os números de CSS em double e os serializa com seis algarismos significativos; o stylo guarda em f32,
/// que muda o sexto algarismo (`55.319148...%` sai `55.3192%`). Os números decimais do texto chegam ao stylo já
/// arredondados como o Blink os escreveria. Strings, `url(...)` e números colados a identificadores ficam intactos.
fn blink_precision(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    let mut quote: Option<u8> = None;
    let mut in_url = false;
    while i < bytes.len() {
        let c = bytes[i];
        if let Some(q) = quote {
            if c == b'\\' && i + 1 < bytes.len() {
                out.push_str(&text[i..i + 2]);
                i += 2;
                continue;
            }
            if c == q {
                quote = None;
            }
        } else if in_url {
            if c == b')' {
                in_url = false;
            }
        } else if c == b'"' || c == b'\'' {
            quote = Some(c);
        } else if text[i..].len() >= 4 && text[i..i + 4].eq_ignore_ascii_case("url(") {
            in_url = true;
        } else if (c.is_ascii_digit() || c == b'.') && !bytes[..i].last().is_some_and(|p| p.is_ascii_alphanumeric() || matches!(p, b'_' | b'#' | b'\\') || *p >= 0x80) {
            let end = i + bytes[i..].iter().take_while(|b| b.is_ascii_digit() || **b == b'.').count();
            let number = &text[i..end];
            let digits = number.bytes().filter(u8::is_ascii_digit).skip_while(|d| *d == b'0').count();
            match number.parse::<f64>() {
                Ok(value) if number.contains('.') && digits > 6 => out.push_str(&format!("{:.5e}", value).parse::<f64>().unwrap_or(value).to_string()),
                _ => out.push_str(number),
            }
            i = end;
            continue;
        }
        let len = text[i..].chars().next().map_or(1, char::len_utf8);
        out.push_str(&text[i..i + len]);
        i += len;
    }
    out
}

/// O nome do atributo como a serialização o escreve: os de `xml`, `xmlns` e `xlink` levam o prefixo.
pub fn qualified_attr_name(name: &QualName) -> String {
    let local: &str = &name.local;
    match &*name.ns {
        "http://www.w3.org/XML/1998/namespace" => format!("xml:{local}"),
        "http://www.w3.org/2000/xmlns/" if local == "xmlns" => "xmlns".to_owned(),
        "http://www.w3.org/2000/xmlns/" => format!("xmlns:{local}"),
        "http://www.w3.org/1999/xlink" => format!("xlink:{local}"),
        _ => match &name.prefix {
            Some(prefix) => format!("{prefix}:{local}"),
            None => local.to_owned(),
        },
    }
}

/// Desliga os filhos de `parent` sem descartá-los: no DOM, um nó removido continua existindo enquanto a página o
/// referencia (e pode voltar à árvore).
pub fn detach_children(mutr: &mut DocumentMutator<'_>, parent: NodeId) {
    for child in mutr.child_ids(parent).to_vec() {
        mutr.remove_node(child);
    }
}

/// Parseia `html` como fragmento no contexto de `context` e põe o resultado como filhos dele (substitui o que havia).
/// Mesmas opções do documento: scripting ligado, então o conteúdo de `<noscript>` vira texto como no Chromium.
pub fn set_inner_html(mutr: &mut DocumentMutator<'_>, context: NodeId, html: &str) {
    detach_children(mutr, context);
    // A raiz de sombra (na ponte, um elemento sem namespace) não é elemento HTML: o contexto do parse é o host, que
    // (como todo host de `attachShadow`) põe o parser no modo de corpo. Sem isso o html5ever trata o fragmento como
    // conteúdo estrangeiro, sem namespace.
    let is_element = mutr.doc.get_node(context).and_then(|n| n.element_data()).is_some_and(|e| e.name.ns != ns!());
    let parse_context = if is_element { context } else { mutr.create_element(QualName::new(None, ns!(html), LocalName::from("div")), Vec::new()) };
    let sink = DocumentHtmlParser::new(mutr);
    let opts = ParseOpts {
        tree_builder: TreeBuilderOpts { scripting_enabled: true, drop_doctype: true, ..Default::default() },
        ..Default::default()
    };
    html5ever::driver::parse_fragment_for_element(sink, opts, parse_context, false, None).from_utf8().read_from(&mut html.as_bytes()).unwrap();
    // O html5ever cria uma raiz de fragmento sob o documento: os filhos dela passam ao contexto e ela some.
    let document_id = mutr.doc.root_node().id;
    let fragment_root = mutr.last_child_id(document_id).unwrap();
    let children = mutr.child_ids(fragment_root);
    mutr.append_children(context, &children);
    mutr.remove_and_drop_node(fragment_root);
    if !is_element {
        mutr.remove_and_drop_node(parse_context);
    }
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
            // O Chromium (como a especificação atual) escapa `<` e `>` também nos valores de atributo.
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
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
                out.push_str(&qualified_attr_name(&attr.name));
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
