#!/usr/bin/env python3
"""Gera src/runtime/options_list.rs (o `OptionsList.h` do C++) a partir de
upstream/JavaScriptCore/runtime/OptionsList.h: a struct `Options` com um campo por opção do
`FOR_EACH_JSC_OPTION` (já com os sub-macros expandidos: `FOR_EACH_JSC_FFI_OPTION` e os outros do
fork do Bun, que valem porque `USE_BUN_JSC_ADDITIONS` está ligado no cmakeconfig.h, e
`FOR_EACH_JSC_WEB_PREFERENCE_OPTION`, que vem de derived/JavaScriptCore/JSCWebPreferenceOptions.h),
o `Default` com os padrões do C++, um acessor e um setter por opção, a tabela `OPTIONS_TABLE`
(nome original, tipo, disponibilidade, descrição) e a tabela de apelidos do `FOR_EACH_JSC_ALIASED_OPTION`.

Padrão que é expressão C++ (`5 * MB`, `computeNumberOfWorkerThreads(3, 2) - 1`, `canUseWasm()`)
é traduzido por regra; chamada de função vira a função escrita à mão em src/runtime/options.rs.
Expressão que nenhuma regra reconhece derruba o script, para nunca sair um padrão chutado.

Uso: scripts/gen-options.py (na raiz do crate).
"""
import os
import re

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

# tipo C++ -> (tipo Rust, variante de OptionType)
TYPES = {
    "Bool": ("bool", "Bool"),
    "Unsigned": ("u32", "Unsigned"),
    "Double": ("f64", "Double"),
    "Int32": ("i32", "Int32"),
    "Size": ("usize", "Size"),
    "OptionRange": ("OptionRange", "OptionRange"),
    "OptionString": ("Option<String>", "OptionString"),
    "GCLogLevel": ("GCLogLevel", "GCLogLevel"),
    "OSLogType": ("OSLogType", "OSLogType"),
}
# tipos que não são Copy: o acessor clona
NOT_COPY = {"OptionRange", "OptionString"}
AVAILABILITY = {"Normal", "Restricted", "Configurable"}
RUST_KEYWORDS = {
    "as", "break", "const", "continue", "crate", "else", "enum", "extern", "false", "fn", "for", "if",
    "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref", "return", "self", "Self",
    "static", "struct", "super", "trait", "true", "type", "unsafe", "use", "where", "while", "async",
    "await", "dyn", "abstract", "become", "box", "do", "final", "macro", "override", "priv", "typeof",
    "unsized", "virtual", "yield", "try",
}
# constantes C++ que aparecem em padrão
CONSTANTS = {
    "KB": "1024",
    "MB": "1048576",
    "UINT_MAX": "u32::MAX",
    "INT32_MAX": "i32::MAX",
    "ASSERT_ENABLED": "assert_enabled()",
    "OSLogType::None": "OSLogType::None",
    "GCLogging::None": "GCLogLevel::None",
    "MAXIMUM_NUMBER_OF_FTL_COMPILER_THREADS": "MAXIMUM_NUMBER_OF_FTL_COMPILER_THREADS",
}
# funções escritas à mão em options.rs que um padrão pode chamar
FUNCTIONS = {
    "defaultTCSMValue",
    "jitEnabledByDefault",
    "ipintEnabledByDefault",
    "computeNumberOfWorkerThreads",
    "computePriorityDeltaOfWorkerThreads",
    "computeNumberOfGCMarkers",
    "defaultQuickDFGTierUpThresholdFactor",
    "defaultRelaxedProfileCoverageFactorForQuickDFGTierUp",
    "defaultQuickFTLTierUpThresholdFactor",
    "canUseWasm",
    "canUseJITCage",
    "hasCapacityToUseLargeGigacage",
}
# constantes do options.rs que o gerado importa
IMPORTED_CONSTANTS = {"MAXIMUM_NUMBER_OF_FTL_COMPILER_THREADS", "assert_enabled()"}


def snake(name):
    """camelCase do C++ para snake_case (`useJITCage` vira `use_jit_cage`)."""
    return re.sub(r"(?<=[a-z0-9])(?=[A-Z])|(?<=[A-Z])(?=[A-Z][a-z])", "_", name).lower()


def match_paren(text, open_index):
    """Índice do `)` que fecha o `(` em open_index, pulando literais de string."""
    depth = 0
    i = open_index
    while i < len(text):
        c = text[i]
        if c == '"':
            i += 1
            while text[i] != '"':
                i += 2 if text[i] == "\\" else 1
        elif c == "(":
            depth += 1
        elif c == ")":
            depth -= 1
            if depth == 0:
                return i
        i += 1
    raise ValueError("parêntese sem par")


def split_top(text):
    """Divide nas vírgulas de nível zero, fora de string e de parênteses."""
    parts = []
    depth = 0
    start = 0
    i = 0
    while i < len(text):
        c = text[i]
        if c == '"':
            i += 1
            while text[i] != '"':
                i += 2 if text[i] == "\\" else 1
        elif c == "(":
            depth += 1
        elif c == ")":
            depth -= 1
        elif c == "," and depth == 0:
            parts.append(text[start:i].strip())
            start = i + 1
        i += 1
    parts.append(text[start:].strip())
    return parts


def macro_body(sources, name):
    """Corpo de `#define name(v)` com as continuações de linha (`\\`) já juntadas."""
    for text in sources:
        m = re.search(r"^#define " + re.escape(name) + r"\(v\)", text, re.M)
        if not m:
            continue
        acc = []
        for line in text[m.end():].split("\n"):
            stripped = line.rstrip()
            if stripped.endswith("\\"):
                acc.append(stripped[:-1])
                continue
            acc.append(line)
            break
        return "\n".join(acc)
    raise KeyError(name)


def entries(sources, name):
    """Os argumentos (texto entre parênteses) de cada `v(...)` do macro, com sub-macros expandidos."""
    body = macro_body(sources, name)
    out = []
    i = 0
    while i < len(body):
        if body[i].isspace():
            i += 1
        elif body.startswith("/*", i):
            i = body.index("*/", i) + 2
        elif body.startswith("v(", i):
            j = match_paren(body, i + 1)
            out.append(body[i + 2:j])
            i = j + 1
        else:
            m = re.match(r"(FOR_EACH_\w+)\(v\)", body[i:])
            if not m:
                raise ValueError(f"{name}: texto inesperado em {body[i:i + 60]!r}")
            out.extend(entries(sources, m.group(1)))
            i += m.end()
    return out


def translate_atom(atom, imports):
    atom = atom.strip()
    if atom in CONSTANTS:
        if CONSTANTS[atom] == "MAXIMUM_NUMBER_OF_FTL_COMPILER_THREADS":
            imports.add("MAXIMUM_NUMBER_OF_FTL_COMPILER_THREADS")
        if CONSTANTS[atom] == "assert_enabled()":
            imports.add("assert_enabled")
        return CONSTANTS[atom]
    if re.fullmatch(r"-?(0x[0-9a-fA-F]+|\d+)", atom):
        return atom
    raise ValueError(f"argumento não reconhecido: {atom!r}")


def translate_default(option_type, expr, option_names, imports):
    """Expressão C++ do padrão para expressão Rust (o contexto `let x: T = ...` fixa o tipo)."""
    expr = " ".join(expr.split())
    if expr == "nullptr":
        if option_type == "OptionRange":
            return "OptionRange::default()"
        if option_type == "OptionString":
            return "None"
        raise ValueError(f"nullptr em {option_type}")
    if expr in ("true", "false"):
        return expr
    # Options::nome(): lê outra opção já inicializada (variável local de mesmo nome)
    m = re.fullmatch(r"Options::(\w+)\(\)", expr)
    if m:
        if m.group(1) not in option_names:
            raise ValueError(f"opção ainda não declarada: {m.group(1)}")
        return snake(m.group(1))
    # c ? a : b
    m = re.fullmatch(r"(\w+)\(\) \? (\d+) : (\d+)", expr)
    if m and m.group(1) in FUNCTIONS:
        imports.add(snake(m.group(1)))
        return f"if {snake(m.group(1))}() {{ {m.group(2)} }} else {{ {m.group(3)} }}"
    # f(args) [- n]
    m = re.fullmatch(r"(\w+)\(([^()]*)\)( - \d+)?", expr)
    if m and m.group(1) in FUNCTIONS:
        imports.add(snake(m.group(1)))
        args = ", ".join(translate_atom(a, imports) for a in split_top(m.group(2))) if m.group(2).strip() else ""
        return f"{snake(m.group(1))}({args}){m.group(3) or ''}"
    if expr in CONSTANTS:
        return translate_atom(expr, imports)
    # aritmética de literais (com KB e MB trocados pelo valor)
    replaced = re.sub(r"\bKB\b", CONSTANTS["KB"], re.sub(r"\bMB\b", CONSTANTS["MB"], expr))
    if re.fullmatch(r"-?[\d\s*/.]+|-?(0x[0-9a-fA-F]+)", replaced):
        if option_type == "Double" and re.fullmatch(r"-?\d+", replaced):
            return replaced + ".0"
        return replaced
    raise ValueError(f"padrão não reconhecido ({option_type}): {expr!r}")


def translate_description(text):
    text = text.strip()
    if text == "nullptr":
        return "None"
    if text.endswith("_s"):
        text = text[:-2]
    if not (text.startswith('"') and text.endswith('"')):
        raise ValueError(f"descrição não reconhecida: {text!r}")
    return f"Some({text})"


def main():
    with open(os.path.join(ROOT, "upstream", "JavaScriptCore", "runtime", "OptionsList.h"), encoding="utf-8") as f:
        options_list = f.read()
    with open(os.path.join(ROOT, "derived", "JavaScriptCore", "JSCWebPreferenceOptions.h"), encoding="utf-8") as f:
        web_preferences = f.read()
    sources = [options_list, web_preferences]

    options = []
    names = set()
    imports = set()
    for entry in entries(sources, "FOR_EACH_JSC_OPTION"):
        parts = split_top(entry)
        assert len(parts) == 5, entry
        option_type, name, default, availability, description = parts
        assert option_type in TYPES, option_type
        assert availability in AVAILABILITY, availability
        assert name not in names, f"opção repetida: {name}"
        rust_name = snake(name)
        if rust_name in RUST_KEYWORDS:
            rust_name += "_"
        default_rs = translate_default(option_type, default, names, imports)
        names.add(name)
        options.append((option_type, name, rust_name, default_rs, availability, translate_description(description)))
    assert len({o[2] for o in options}) == len(options), "dois nomes viraram o mesmo snake_case"

    aliases = []
    for entry in entries([options_list], "FOR_EACH_JSC_ALIASED_OPTION"):
        old, new, equivalence = split_top(entry)
        assert equivalence in ("SameOption", "InvertedOption"), equivalence
        aliases.append((old, new, equivalence == "InvertedOption"))

    fields = "\n".join(
        f"    pub {rn}: {TYPES[t][0]}," for t, _, rn, _, _, _ in options
    )
    lets = "\n".join(
        f"        let {rn}: {TYPES[t][0]} = {d};" for t, _, rn, d, _, _ in options
    )
    init = ", ".join(rn for _, _, rn, _, _, _ in options)
    accessors = []
    for t, name, rn, _, _, _ in options:
        rust_type = TYPES[t][0]
        clone = ".clone()" if t in NOT_COPY else ""
        accessors.append(
            f"    /// Opção `{name}`.\n"
            f"    pub fn {rn}() -> {rust_type} {{\n"
            f"        Options::with(|options| options.{rn}{clone})\n"
            f"    }}\n\n"
            f"    /// `Options::{name}() = value`.\n"
            f"    pub fn set_{rn}(value: {rust_type}) {{\n"
            f"        Options::with_mut(|options| options.{rn} = value);\n"
            f"    }}\n"
        )
    table = ",\n".join(
        f'    OptionInfo {{ name: "{name}", option_type: OptionType::{TYPES[t][1]}, '
        f"availability: Availability::{av}, description: {desc} }}"
        for t, name, _, _, av, desc in options
    )
    alias_table = ",\n".join(
        f'    OptionAlias {{ name: "{old}", target: "{new}", inverted: {str(inv).lower()} }}'
        for old, new, inv in aliases
    )
    import_list = ", ".join(sorted(imports))
    out = f"""//! Gerado por `scripts/gen-options.py` a partir de `upstream/JavaScriptCore/runtime/OptionsList.h`
//! (e de `derived/JavaScriptCore/JSCWebPreferenceOptions.h`). Não editar à mão.

use super::options::{{
    Availability, GCLogLevel, OSLogType, OptionAlias, OptionInfo, OptionRange, OptionType,
}};
use super::options::{{{import_list}}};

/// `NumberOfOptions`.
pub const NUMBER_OF_OPTIONS: usize = {len(options)};

/// `OptionsStorage`: um campo por opção do `FOR_EACH_JSC_OPTION`, na ordem da lista.
#[derive(Clone, Debug)]
pub struct Options {{
{fields}
}}

impl Default for Options {{
    /// Os padrões do `FOR_EACH_JSC_OPTION`, na ordem da lista (um padrão pode ler opção anterior).
    fn default() -> Self {{
{lets}
        Options {{ {init} }}
    }}
}}

impl Options {{
{chr(10).join(accessors)}}}

/// Nome original, tipo, disponibilidade e descrição de cada opção, na ordem de `Options`.
pub static OPTIONS_TABLE: [OptionInfo; NUMBER_OF_OPTIONS] = [
{table},
];

/// `FOR_EACH_JSC_ALIASED_OPTION`: nome antigo, opção que ele aponta e se o valor se inverte.
pub static OPTIONS_ALIASES: [OptionAlias; {len(aliases)}] = [
{alias_table},
];
"""
    os.makedirs(os.path.join(ROOT, "src", "runtime"), exist_ok=True)
    with open(os.path.join(ROOT, "src", "runtime", "options_list.rs"), "w", encoding="utf-8") as f:
        f.write(out)


main()
