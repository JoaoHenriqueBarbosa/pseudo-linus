//! O documento da página: um `BaseDocument` do Blitz, com o parser do html5ever ligado como o Chromium o liga
//! (scripting ligado, doctype preservado).

use blitz_dom::{BaseDocument, DocumentConfig};
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_queries() {
        let doc = parse("<!DOCTYPE html><title>T</title><h1 id=h>a</h1><p>x&lt;y</p>");
        let h1 = doc.get_element_by_id("h").unwrap();
        assert_eq!(doc.get_node(h1).unwrap().text_content(), "a");
    }
}
