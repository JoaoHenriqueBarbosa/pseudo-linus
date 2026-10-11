//! As propriedades CSS que o Blink tem e o stylo do Servo não analisa (no stylo elas são `engine = "gecko"` ou não
//! existem). Sem esta tabela a declaração some do `style` escrito pelo CSSOM e o `getComputedStyle` devolve vazio,
//! duas costuras visíveis (o motion do nextjs.org escreve `transform-box: fill-box`).
//!
//! Cada propriedade tem a gramática do valor e o valor computado inicial medido no oráculo
//! (`testbench/oracle/measured/blink-computed-defaults.html`, no `<body>`). A gramática valida e serializa como o
//! Blink: palavra-chave em minúsculas; cor, comprimento e número passam pelo stylo, analisados como o valor de uma
//! propriedade que ele conhece com a mesma gramática.

/// A gramática de um valor.
#[derive(Clone, Copy)]
enum Grammar {
    /// Uma palavra-chave da lista.
    Keyword(&'static [&'static str]),
    /// Palavra-chave da lista ou o valor da gramática do proxy.
    KeywordOr(&'static [&'static str], Proxy),
    /// Só o valor da gramática do proxy.
    Value(Proxy),
    /// Uma a `n` palavras-chave da lista, sem repetir.
    Keywords(&'static [&'static str], usize),
    /// `none` ou `url(...)`.
    UrlOrNone,
    /// `none` ou uma lista de identificadores.
    IdentsOrNone,
}

/// Uma gramática que o stylo conhece, emprestada da propriedade `property`; `no_percent` recusa porcentagem.
#[derive(Clone, Copy)]
struct Proxy {
    property: &'static str,
    no_percent: bool,
}

const COLOR: Proxy = Proxy { property: "color", no_percent: false };
const LENGTH: Proxy = Proxy { property: "width", no_percent: true };
const LENGTH_PERCENTAGE: Proxy = Proxy { property: "padding-top", no_percent: false };
const LINE_WIDTH: Proxy = Proxy { property: "border-top-width", no_percent: true };
const NUMBER: Proxy = Proxy { property: "flex-grow", no_percent: true };
const OPACITY: Proxy = Proxy { property: "opacity", no_percent: false };
const INTEGER: Proxy = Proxy { property: "order", no_percent: true };

const WRITING_MODES: &[&str] = &["horizontal-tb", "vertical-rl", "vertical-lr", "sideways-rl", "sideways-lr"];
const BREAK_BETWEEN: &[&str] = &["auto", "avoid", "always", "all", "avoid-page", "page", "left", "right", "recto", "verso", "avoid-column", "column", "avoid-region", "region"];
const BORDER_STYLES: &[&str] = &["none", "hidden", "dotted", "dashed", "solid", "double", "groove", "ridge", "inset", "outset"];
const SNAP_ALIGN: &[&str] = &["none", "start", "end", "center"];

/// (nome, gramática, valor computado inicial no oráculo).
const PROPERTIES: &[(&str, Grammar, &str)] = &[
    ("transform-box", Grammar::Keyword(&["content-box", "border-box", "fill-box", "stroke-box", "view-box"]), "view-box"),
    ("writing-mode", Grammar::Keyword(WRITING_MODES), "horizontal-tb"),
    ("-webkit-writing-mode", Grammar::Keyword(WRITING_MODES), "horizontal-tb"),
    ("-webkit-font-smoothing", Grammar::Keyword(&["auto", "none", "antialiased", "subpixel-antialiased"]), "auto"),
    ("-webkit-tap-highlight-color", Grammar::Value(COLOR), "rgba(0, 0, 0, 0.18)"),
    ("-webkit-text-fill-color", Grammar::Value(COLOR), "rgb(0, 0, 0)"),
    ("-webkit-text-stroke-color", Grammar::Value(COLOR), "rgb(0, 0, 0)"),
    ("-webkit-text-stroke-width", Grammar::Value(LINE_WIDTH), "0px"),
    ("accent-color", Grammar::KeywordOr(&["auto"], COLOR), "auto"),
    ("scroll-margin-top", Grammar::Value(LENGTH), "0px"),
    ("scroll-margin-right", Grammar::Value(LENGTH), "0px"),
    ("scroll-margin-bottom", Grammar::Value(LENGTH), "0px"),
    ("scroll-margin-left", Grammar::Value(LENGTH), "0px"),
    ("scroll-margin-block-start", Grammar::Value(LENGTH), "0px"),
    ("scroll-margin-block-end", Grammar::Value(LENGTH), "0px"),
    ("scroll-margin-inline-start", Grammar::Value(LENGTH), "0px"),
    ("scroll-margin-inline-end", Grammar::Value(LENGTH), "0px"),
    ("scroll-padding-top", Grammar::KeywordOr(&["auto"], LENGTH_PERCENTAGE), "auto"),
    ("scroll-padding-right", Grammar::KeywordOr(&["auto"], LENGTH_PERCENTAGE), "auto"),
    ("scroll-padding-bottom", Grammar::KeywordOr(&["auto"], LENGTH_PERCENTAGE), "auto"),
    ("scroll-padding-left", Grammar::KeywordOr(&["auto"], LENGTH_PERCENTAGE), "auto"),
    ("scroll-padding-block-start", Grammar::KeywordOr(&["auto"], LENGTH_PERCENTAGE), "auto"),
    ("scroll-padding-block-end", Grammar::KeywordOr(&["auto"], LENGTH_PERCENTAGE), "auto"),
    ("scroll-padding-inline-start", Grammar::KeywordOr(&["auto"], LENGTH_PERCENTAGE), "auto"),
    ("scroll-padding-inline-end", Grammar::KeywordOr(&["auto"], LENGTH_PERCENTAGE), "auto"),
    ("scroll-snap-align", Grammar::Keywords(SNAP_ALIGN, 2), "none"),
    ("scroll-snap-stop", Grammar::Keyword(&["normal", "always"]), "normal"),
    ("overflow-anchor", Grammar::Keyword(&["auto", "none"]), "auto"),
    ("hyphens", Grammar::Keyword(&["none", "manual", "auto"]), "manual"),
    ("-webkit-line-clamp", Grammar::KeywordOr(&["none"], INTEGER), "none"),
    ("-webkit-box-orient", Grammar::Keyword(&["horizontal", "vertical", "inline-axis", "block-axis"]), "horizontal"),
    ("-webkit-box-align", Grammar::Keyword(&["stretch", "start", "end", "center", "baseline"]), "stretch"),
    ("-webkit-box-pack", Grammar::Keyword(&["start", "end", "center", "justify"]), "start"),
    ("-webkit-box-direction", Grammar::Keyword(&["normal", "reverse"]), "normal"),
    ("-webkit-box-flex", Grammar::Value(NUMBER), "0"),
    ("-webkit-box-ordinal-group", Grammar::Value(INTEGER), "1"),
    ("-webkit-box-decoration-break", Grammar::Keyword(&["slice", "clone"]), "slice"),
    ("-webkit-user-drag", Grammar::Keyword(&["auto", "none", "element"]), "auto"),
    ("-webkit-user-modify", Grammar::Keyword(&["read-only", "read-write", "read-write-plaintext-only"]), "read-only"),
    ("-webkit-rtl-ordering", Grammar::Keyword(&["logical", "visual"]), "logical"),
    ("-webkit-text-combine", Grammar::Keyword(&["none", "horizontal"]), "none"),
    ("-webkit-line-break", Grammar::Keyword(&["auto", "loose", "normal", "strict", "after-white-space", "anywhere"]), "auto"),
    ("paint-order", Grammar::Keywords(&["normal", "fill", "stroke", "markers"], 3), "normal"),
    ("stop-color", Grammar::Value(COLOR), "rgb(0, 0, 0)"),
    ("flood-color", Grammar::Value(COLOR), "rgb(0, 0, 0)"),
    ("lighting-color", Grammar::Value(COLOR), "rgb(255, 255, 255)"),
    ("stop-opacity", Grammar::Value(OPACITY), "1"),
    ("flood-opacity", Grammar::Value(OPACITY), "1"),
    ("shape-rendering", Grammar::Keyword(&["auto", "optimizespeed", "crispedges", "geometricprecision"]), "auto"),
    ("text-anchor", Grammar::Keyword(&["start", "middle", "end"]), "start"),
    ("dominant-baseline", Grammar::Keyword(&["auto", "alphabetic", "ideographic", "middle", "central", "mathematical", "hanging", "text-bottom", "text-top"]), "auto"),
    ("vector-effect", Grammar::Keyword(&["none", "non-scaling-stroke"]), "none"),
    ("color-interpolation", Grammar::Keyword(&["auto", "srgb", "linearrgb"]), "srgb"),
    ("color-interpolation-filters", Grammar::Keyword(&["auto", "srgb", "linearrgb"]), "linearrgb"),
    ("color-rendering", Grammar::Keyword(&["auto", "optimizespeed", "optimizequality"]), "auto"),
    ("clip-rule", Grammar::Keyword(&["nonzero", "evenodd"]), "nonzero"),
    ("buffered-rendering", Grammar::Keyword(&["auto", "dynamic", "static"]), "auto"),
    ("marker-start", Grammar::UrlOrNone, "none"),
    ("marker-mid", Grammar::UrlOrNone, "none"),
    ("marker-end", Grammar::UrlOrNone, "none"),
    ("content-visibility", Grammar::Keyword(&["visible", "auto", "hidden"]), "visible"),
    ("contain-intrinsic-width", Grammar::KeywordOr(&["none"], LENGTH), "none"),
    ("contain-intrinsic-height", Grammar::KeywordOr(&["none"], LENGTH), "none"),
    ("contain-intrinsic-block-size", Grammar::KeywordOr(&["none"], LENGTH), "none"),
    ("contain-intrinsic-inline-size", Grammar::KeywordOr(&["none"], LENGTH), "none"),
    ("container-type", Grammar::Keyword(&["normal", "size", "inline-size", "scroll-state"]), "normal"),
    ("container-name", Grammar::IdentsOrNone, "none"),
    ("break-after", Grammar::Keyword(BREAK_BETWEEN), "auto"),
    ("break-before", Grammar::Keyword(BREAK_BETWEEN), "auto"),
    ("break-inside", Grammar::Keyword(&["auto", "avoid", "avoid-page", "avoid-column", "avoid-region"]), "auto"),
    ("orphans", Grammar::Value(INTEGER), "2"),
    ("widows", Grammar::Value(INTEGER), "2"),
    ("resize", Grammar::Keyword(&["none", "both", "horizontal", "vertical", "block", "inline"]), "none"),
    ("text-wrap-style", Grammar::Keyword(&["auto", "balance", "stable", "pretty"]), "auto"),
    ("print-color-adjust", Grammar::Keyword(&["economy", "exact"]), "economy"),
    ("font-optical-sizing", Grammar::Keyword(&["auto", "none"]), "auto"),
    ("font-synthesis-style", Grammar::Keyword(&["auto", "none"]), "auto"),
    ("font-synthesis-small-caps", Grammar::Keyword(&["auto", "none"]), "auto"),
    ("text-decoration-skip-ink", Grammar::Keyword(&["auto", "none", "all"]), "auto"),
    ("text-combine-upright", Grammar::Keyword(&["none", "all"]), "none"),
    ("overlay", Grammar::Keyword(&["none", "auto"]), "none"),
    ("field-sizing", Grammar::Keyword(&["fixed", "content"]), "fixed"),
    ("interpolate-size", Grammar::Keyword(&["numeric-only", "allow-keywords"]), "numeric-only"),
    ("image-orientation", Grammar::Keyword(&["from-image", "none"]), "from-image"),
    ("ruby-position", Grammar::Keyword(&["over", "under", "alternate", "inter-character"]), "over"),
    ("ruby-align", Grammar::Keyword(&["space-around", "start", "center", "space-between"]), "space-around"),
    ("speak", Grammar::Keyword(&["none", "normal", "spell-out", "digits", "literal-punctuation", "no-punctuation"]), "normal"),
    ("caret-shape", Grammar::Keyword(&["auto", "bar", "block", "underscore"]), "auto"),
    ("caret-animation", Grammar::Keyword(&["auto", "manual"]), "auto"),
    ("column-fill", Grammar::Keyword(&["auto", "balance", "balance-all"]), "balance"),
    ("column-rule-style", Grammar::Keyword(BORDER_STYLES), "none"),
    ("column-rule-width", Grammar::Value(LINE_WIDTH), "3px"),
    ("column-rule-color", Grammar::Value(COLOR), "rgb(0, 0, 0)"),
    ("text-emphasis-color", Grammar::Value(COLOR), "rgb(0, 0, 0)"),
];

fn entry(name: &str) -> Option<&'static (&'static str, Grammar, &'static str)> {
    PROPERTIES.iter().find(|(n, _, _)| *n == name)
}

/// O valor computado inicial de uma propriedade da tabela.
pub fn initial(name: &str) -> Option<&'static str> {
    entry(name).map(|(_, _, initial)| *initial)
}

/// Valida `value` para a propriedade `name` da tabela e devolve a serialização do Blink; `None` quando a
/// propriedade não está na tabela ou o valor é inválido. `proxy` serializa um valor como a propriedade dada
/// (devolve `None` quando o stylo o recusa).
pub fn serialize(name: &str, value: &str, proxy: impl Fn(&str, &str) -> Option<String>) -> Option<String> {
    let (_, grammar, _) = entry(name)?;
    let value = value.trim();
    let lower = value.to_ascii_lowercase();
    // As palavras-chave globais valem em toda propriedade.
    if ["initial", "inherit", "unset", "revert", "revert-layer"].contains(&lower.as_str()) {
        return Some(lower);
    }
    let by_proxy = |p: Proxy| -> Option<String> {
        if p.no_percent && value.contains('%') {
            return None;
        }
        proxy(p.property, value)
    };
    match grammar {
        Grammar::Keyword(list) => list.contains(&lower.as_str()).then_some(lower),
        Grammar::KeywordOr(list, p) => if list.contains(&lower.as_str()) { Some(lower) } else { by_proxy(*p) },
        Grammar::Value(p) => by_proxy(*p),
        Grammar::Keywords(list, max) => {
            let words: Vec<&str> = lower.split_ascii_whitespace().collect();
            let distinct = words.iter().enumerate().all(|(i, w)| !words[..i].contains(w));
            let ok = !words.is_empty() && words.len() <= *max && distinct && words.iter().all(|w| list.contains(w));
            ok.then(|| words.join(" "))
        }
        Grammar::UrlOrNone => (lower == "none" || (lower.starts_with("url(") && lower.ends_with(')'))).then(|| if lower == "none" { lower } else { value.to_owned() }),
        Grammar::IdentsOrNone => {
            let words: Vec<&str> = value.split_ascii_whitespace().collect();
            let ident = |w: &str| w.chars().next().is_some_and(|c| c.is_alphabetic() || c == '_' || c == '-') && w.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_');
            if lower == "none" { Some(lower) } else { (!words.is_empty() && words.iter().all(|w| ident(w) && !w.eq_ignore_ascii_case("none"))).then(|| words.join(" ")) }
        }
    }
}
