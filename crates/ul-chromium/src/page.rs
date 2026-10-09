//! Uma página carregada: busca a URL, monta o documento e serializa o resultado.

use blitz_dom::BaseDocument;
use sysabi::sys;
use ul_net::net::client;

use crate::{document, encoding};

pub struct Page {
    pub url: String,
    pub doctype: Option<String>,
    pub doc: BaseDocument,
}

impl Page {
    /// Busca `url` (`file:`, `http:` ou `https:`) e monta o documento.
    pub fn load(url: &str) -> Result<Page, String> {
        let (bytes, final_url, content_type) = fetch(url)?;
        let source = encoding::decode(&bytes, content_type.as_deref());
        Ok(Page { url: final_url, doctype: document::sniff_doctype(&source), doc: document::parse(&source) })
    }

    /// O que `--dump-dom` imprime: o doctype, o `<html>` serializado e uma quebra de linha final.
    pub fn dump_dom(&self) -> String {
        let mut out = String::new();
        if let Some(doctype) = &self.doctype {
            out.push_str(doctype);
            out.push('\n');
        }
        out.push_str(&document::outer_html(&self.doc, self.doc.root_element().id));
        out.push('\n');
        out
    }
}

fn fetch(url: &str) -> Result<(Vec<u8>, String, Option<String>), String> {
    if let Some(path) = url.strip_prefix("file://") {
        let bytes = sys::read_file(path.as_bytes()).map_err(|_| "[ERROR:net::ERR_FILE_NOT_FOUND]".to_owned())?;
        return Ok((bytes, url.to_owned(), None));
    }
    let response = client::request(url, &client::RequestSpec::default()).map_err(|e| e.net_error().to_owned())?;
    let final_url = response.url.to_string_with(true);
    let content_type = response.header("Content-Type");
    Ok((response.body, final_url, content_type))
}
