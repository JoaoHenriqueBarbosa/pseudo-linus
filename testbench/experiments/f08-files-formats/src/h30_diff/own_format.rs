//! Renderizadores que usam o formatador da própria biblioteca (o "do jeito que vem"), com o front-end
//! nosso em volta. Mede quanto da saída do GNU cada crate entrega sem camada nossa de formatação.

use super::cli::{Opts, Renderer, Style};

fn only_plain(opts: &Opts) -> bool {
    opts.norm.is_identity() && !opts.ignore_blank
}

/// `similar::TextDiff::unified_diff` (Myers), cabeçalho com os rótulos que passamos.
pub struct SimilarUnified;

impl Renderer for SimilarUnified {
    fn name(&self) -> String {
        "similar unified_diff próprio (Myers)".into()
    }

    fn krate(&self) -> (&'static str, &'static str) {
        ("similar", "3.2.0")
    }

    fn render(&self, opts: &Opts, a: &[u8], b: &[u8], header: (&str, &str)) -> Option<(Vec<u8>, bool)> {
        let Style::Unified(n) = opts.style else { return None };
        if !only_plain(opts) {
            return None;
        }
        let diff = similar::TextDiff::configure().algorithm(similar::Algorithm::Myers).diff_lines(a, b);
        let mut out = Vec::new();
        diff.unified_diff().context_radius(n).header(header.0, header.1).to_writer(&mut out).ok()?;
        Some((out, true))
    }
}

/// `diffy::DiffOptions::create_patch_bytes` + `Patch::to_bytes`.
pub struct DiffyUnified;

impl Renderer for DiffyUnified {
    fn name(&self) -> String {
        "diffy create_patch próprio (Myers)".into()
    }

    fn krate(&self) -> (&'static str, &'static str) {
        ("diffy", "0.5.2")
    }

    fn render(&self, opts: &Opts, a: &[u8], b: &[u8], header: (&str, &str)) -> Option<(Vec<u8>, bool)> {
        let Style::Unified(n) = opts.style else { return None };
        if !only_plain(opts) {
            return None;
        }
        // O diffy põe o nome entre aspas e escapa o tab quando o rótulo tem data ("--- \"a\\t2026...\""), então
        // o cabeçalho é trocado pelo nosso e se mede só o corpo.
        let mut o = diffy::DiffOptions::new();
        o.set_context_len(n).set_original_filename("a").set_modified_filename("b");
        let patch = o.create_patch_bytes(a, b);
        Some((replace_header(patch.to_bytes(), &format!("--- {}", header.0), &format!("+++ {}", header.1)), true))
    }
}

/// Funções de formatação do uutils diffutils 0.5 (`diffutilslib`). O cabeçalho que ela gera lê o mtime
/// do disco do host (`std::fs::metadata` + `chrono::Local`), então as duas primeiras linhas são trocadas
/// pelas nossas.
pub struct DiffutilsLib;

fn replace_header(out: Vec<u8>, first: &str, second: &str) -> Vec<u8> {
    let mut lines = out.splitn(3, |&c| c == b'\n');
    let _ = lines.next();
    let _ = lines.next();
    let rest = lines.next().unwrap_or(b"");
    let mut fixed = Vec::new();
    fixed.extend_from_slice(first.as_bytes());
    fixed.push(b'\n');
    fixed.extend_from_slice(second.as_bytes());
    fixed.push(b'\n');
    fixed.extend_from_slice(rest);
    fixed
}

impl Renderer for DiffutilsLib {
    fn name(&self) -> String {
        "uutils diffutils formatadores próprios (normal/unified/context/ed/side-by-side)".into()
    }

    fn krate(&self) -> (&'static str, &'static str) {
        ("diffutils", "0.5.0")
    }

    fn render(&self, opts: &Opts, a: &[u8], b: &[u8], header: (&str, &str)) -> Option<(Vec<u8>, bool)> {
        if !only_plain(opts) {
            return None;
        }
        let mut params = diffutilslib::params::Params {
            executable: "diff".into(),
            from: header.0.split('\t').next().unwrap_or_default().into(),
            to: header.1.split('\t').next().unwrap_or_default().into(),
            ..Default::default()
        };
        let out = match opts.style {
            Style::Normal => {
                params.format = diffutilslib::params::Format::Normal;
                diffutilslib::normal_diff(a, b, &params)
            }
            Style::Unified(n) => {
                params.format = diffutilslib::params::Format::Unified;
                params.context_count = n;
                let raw = diffutilslib::unified_diff(a, b, &params);
                replace_header(raw, &format!("--- {}", header.0), &format!("+++ {}", header.1))
            }
            Style::Context(n) => {
                params.format = diffutilslib::params::Format::Context;
                params.context_count = n;
                let raw = diffutilslib::context_diff(a, b, &params);
                replace_header(raw, &format!("*** {}", header.0), &format!("--- {}", header.1))
            }
            Style::Ed => {
                params.format = diffutilslib::params::Format::Ed;
                diffutilslib::ed_diff(a, b, &params).ok()?
            }
            Style::SideBySide { width, suppress_common } => {
                if suppress_common {
                    return None;
                }
                params.format = diffutilslib::params::Format::SideBySide;
                params.width = width;
                let mut buf = Vec::new();
                let _ = diffutilslib::side_by_side_diff(a, b, &mut buf, &params);
                buf
            }
        };
        Some((out, true))
    }
}

/// `imara_diff::Diff::unified_diff` com o `BasicLineDiffPrinter` (Histogram + `postprocess_lines`, o uso
/// recomendado pela crate). O printer só aceita texto UTF-8 e não escreve cabeçalho de arquivo.
pub struct ImaraUnified;

impl Renderer for ImaraUnified {
    fn name(&self) -> String {
        "imara-diff unified_diff próprio (Histogram)".into()
    }

    fn krate(&self) -> (&'static str, &'static str) {
        ("imara-diff", "0.2.0")
    }

    fn render(&self, opts: &Opts, a: &[u8], b: &[u8], header: (&str, &str)) -> Option<(Vec<u8>, bool)> {
        let Style::Unified(n) = opts.style else { return None };
        if !only_plain(opts) {
            return None;
        }
        let sa = std::str::from_utf8(a).ok()?;
        let sb = std::str::from_utf8(b).ok()?;
        let input = imara_diff::InternedInput::new(sa, sb);
        let mut diff = imara_diff::Diff::compute(imara_diff::Algorithm::Histogram, &input);
        diff.postprocess_lines(&input);
        let mut cfg = imara_diff::UnifiedDiffConfig::default();
        cfg.context_len(n as u32);
        let printer = imara_diff::BasicLineDiffPrinter(&input.interner);
        let body = diff.unified_diff(&printer, cfg, &input).to_string();
        let mut out = format!("--- {}\n+++ {}\n", header.0, header.1).into_bytes();
        out.extend_from_slice(body.as_bytes());
        Some((out, true))
    }
}

pub fn all_own() -> Vec<Box<dyn Renderer>> {
    vec![Box::new(SimilarUnified), Box::new(DiffyUnified), Box::new(DiffutilsLib), Box::new(ImaraUnified)]
}
