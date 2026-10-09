#!/usr/bin/env python3
"""Gera src/runtime/builtins_source.rs e src/runtime/builtins_combined.js a partir do
`derived/JavaScriptCore/JSCBuiltins.cpp` e `JSCBuiltins.h`, que são a saída do gerador do próprio
JavaScriptCore (`Scripts/generate-js-builtins.py --combined`) sobre `builtins/*.js`.

O que o C++ faz com essa saída, e onde cada parte cai no porte:
- `s_JSCCombinedCode` (os fontes de todos os builtins, um após o outro, cada um embrulhado em
  `(function (...) { ... })\\n`) vira `builtins_combined.js`, que o Rust embute com `include_str!`;
- `JSC_FOREACH_BUILTIN_CODE` vira o enum `BuiltinCodeIndex` (a mesma ordem, que também indexa a
  tabela de metadados) e a tabela `BUILTIN_CODES`: nome da função, nome sobrescrito, deslocamento e
  comprimento no fonte combinado, `ConstructAbility`, `ConstructorKind`, `ImplementationVisibility`,
  `InlineAttribute`, `Intrinsic` e o `s_JSCBuiltinSourceMetadata`;
- `functionName##PublicName()` do `BuiltinExecutables::xxxExecutable` vira `public_name`, um `match`
  sobre os acessores `xxx_public_name` de `BuiltinNames` (os mesmos nomes que
  `scripts/gen-common-identifiers.py` gera, reaproveitando o mesmo `Namer`).

Uso: scripts/gen-builtins.py (na raiz do crate). Regerar quando `derived/` mudar.
"""
import os
import re
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
DERIVED = os.path.join(ROOT, "derived", "JavaScriptCore")
OUT_RS = os.path.join(ROOT, "src", "runtime", "builtins_source.rs")
OUT_JS = os.path.join(ROOT, "src", "runtime", "builtins_combined.js")


def load_common_identifiers():
    """Executa `gen-common-identifiers.py` sem o `main()` do fim, para reusar `read_macros`,
    `expand` e `Namer` (os nomes dos acessores têm que sair idênticos aos de `BuiltinNames`)."""
    path = os.path.join(ROOT, "scripts", "gen-common-identifiers.py")
    with open(path, encoding="utf-8") as f:
        source = f.read()
    head = source.split("\ndef main():")[0]
    namespace = {"__file__": path, "__name__": "gen_common_identifiers"}
    exec(compile(head, path, "exec"), namespace)
    return namespace


def read(path):
    with open(path, encoding="utf-8") as f:
        return f.read()


def accessor_stems():
    """`{nome C++: stem Rust}` na ordem e com as colisões de `gen_builtin_names`."""
    ns = load_common_identifiers()
    defs = ns["read_macros"]()
    expand = ns["expand"]
    functions = expand(defs, "JSC_FOREACH_BUILTIN_FUNCTION_NAME")
    properties = expand(defs, "JSC_COMMON_PRIVATE_IDENTIFIERS_EACH_PROPERTY_NAME")
    namer = ns["Namer"](["dollar_vm", "poly_proto", "stack", "empty_identifier"])
    return {n: namer.take(n) for n in functions + properties}


def parse_header(header):
    """As linhas `macro(codeName, functionName, overriddenName, s_...Length)` do
    `JSC_FOREACH_BUILTIN_CODE`, na ordem do enum `BuiltinCodeIndex`."""
    start = header.index("#define JSC_FOREACH_BUILTIN_CODE(macro)")
    end = header.index("\n\n", start)
    entries = []
    pattern = re.compile(r'^\s*macro\((\w+), (\w+), (ASCIILiteral\(\)|"([^"]*)"_s), s_\w+Length\)')
    for line in header[start:end].split("\n")[1:]:
        m = pattern.match(line)
        assert m, f"linha de JSC_FOREACH_BUILTIN_CODE ilegível: {line!r}"
        entries.append({"code_name": m.group(1), "function_name": m.group(2), "overridden": m.group(4)})
    return entries


def parse_cpp(cpp):
    m = re.search(r"s_JSCCombinedCode\[\] = \{ ([0-9, ]*) \};", cpp)
    assert m, "s_JSCCombinedCode não encontrado"
    combined = bytes(int(x) for x in m.group(1).split(", "))
    m = re.search(r"s_JSCCombinedCodeLength = (\d+);", cpp)
    assert m and int(m.group(1)) == len(combined), "s_JSCCombinedCodeLength não bate com o fonte combinado"

    def by_name(pattern, group_count=2):
        result = {}
        for found in re.finditer(pattern, cpp):
            assert found.group(1) not in result, f"{found.group(1)} repetido"
            result[found.group(1)] = found.group(2)
        return result

    attributes = {
        "construct_ability": by_name(r"s_(\w+)ConstructAbility = JSC::ConstructAbility::(\w+);"),
        "constructor_kind": by_name(r"s_(\w+)ConstructorKind = JSC::ConstructorKind::(\w+);"),
        "visibility": by_name(r"s_(\w+)ImplementationVisibility = JSC::ImplementationVisibility::(\w+);"),
        "inline_attribute": by_name(r"s_(\w+)InlineAttribute = JSC::InlineAttribute::(\w+);"),
        "length": by_name(r"constinit const int s_(\w+)Length = (\d+);"),
        "intrinsic": by_name(r"static constinit const JSC::Intrinsic s_(\w+)Intrinsic = JSC::(\w+);"),
        "offset": by_name(r"constinit const char\* const s_(\w+) =\ns_JSCCombinedCode \+ (\d+)\n;"),
    }

    metadata = {}
    table = re.search(r"s_JSCBuiltinSourceMetadata\[JSC::numberOfBuiltinCodes\] = \{\n(.*?)\n\};", cpp, re.S)
    assert table, "s_JSCBuiltinSourceMetadata não encontrado"
    for row in table.group(1).split("\n"):
        m = re.match(r"\s*/\* (\w+) \*/ \{ ([^}]*) \},", row)
        assert m, f"linha de metadados ilegível: {row!r}"
        fields = [x.strip() for x in m.group(2).split(",")]
        assert len(fields) == 10, row
        metadata[m.group(1)] = fields
    return combined, attributes, metadata


def pascal(code_name):
    return code_name[:1].upper() + code_name[1:]


def rust_str(text):
    assert text.isascii() and '"' not in text and "\\" not in text, text
    return '"' + text + '"'


def main():
    combined, attributes, metadata = parse_cpp(read(os.path.join(DERIVED, "JSCBuiltins.cpp")))
    entries = parse_header(read(os.path.join(DERIVED, "JSCBuiltins.h")))
    stems = accessor_stems()

    assert combined.isascii(), "o fonte combinado tem byte fora do ASCII (include_str! exigiria UTF-8)"
    text = combined.decode("ascii")
    names = [e["code_name"] for e in entries]
    assert len(set(names)) == len(names)
    assert list(metadata) == names, "a ordem de s_JSCBuiltinSourceMetadata difere da de JSC_FOREACH_BUILTIN_CODE"
    for key, table in attributes.items():
        assert set(table) == set(names), f"{key}: nomes divergem de JSC_FOREACH_BUILTIN_CODE"

    expected_offset = 0
    rows = []
    for e in entries:
        name = e["code_name"]
        offset = int(attributes["offset"][name])
        length = int(attributes["length"][name])
        assert offset == expected_offset, f"{name}: o fonte combinado não é contíguo"
        expected_offset += length
        source = text[offset:offset + length]
        assert source.startswith(("(function (", "(async function (")) and source.endswith("\n"), name
        meta = metadata[name]
        assert int(meta[0]) == length, f"{name}: sourceLength diverge do comprimento"
        assert e["function_name"] in stems, f"{e['function_name']}: sem acessor em BuiltinNames"
        rows.append((e, offset, length, meta))
    assert expected_offset == len(combined)

    out = []
    out.append("//! Gerado por `scripts/gen-builtins.py` a partir de `derived/JavaScriptCore/JSCBuiltins.cpp` e")
    out.append("//! `JSCBuiltins.h` (a saída do gerador de builtins do JavaScriptCore sobre `builtins/*.js`). Não")
    out.append("//! editar à mão. É o que o C++ declara em `s_JSCCombinedCode`, `JSC_FOREACH_BUILTIN_CODE`,")
    out.append("//! `s_xxxCodeConstructAbility` e companhia, e `s_JSCBuiltinSourceMetadata`.")
    out.append("")
    out.append("use crate::runtime::builtin_executables::BuiltinSourceMetadata;")
    out.append("use crate::runtime::builtin_names::BuiltinNames;")
    out.append("use crate::runtime::construct_ability::ConstructAbility;")
    out.append("use crate::runtime::constructor_kind::ConstructorKind;")
    out.append("use crate::runtime::identifier::Identifier;")
    out.append("use crate::runtime::implementation_visibility::ImplementationVisibility;")
    out.append("use crate::runtime::inline_attribute::InlineAttribute;")
    out.append("use crate::runtime::intrinsic::Intrinsic;")
    out.append("")
    out.append("/// `s_JSCCombinedCode`: os fontes de todos os builtins, um atrás do outro (`builtins_combined.js`).")
    out.append('pub const COMBINED_CODE: &str = include_str!("builtins_combined.js");')
    out.append("")
    out.append("/// `numberOfBuiltinCodes`.")
    out.append(f"pub const NUMBER_OF_BUILTIN_CODES: usize = {len(rows)};")
    out.append("")
    out.append("/// `enum class BuiltinCodeIndex`.")
    out.append("#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]")
    out.append("#[repr(usize)]")
    out.append("pub enum BuiltinCodeIndex {")
    for e, *_ in rows:
        out.append(f"    {pascal(e['code_name'])},")
    out.append("}")
    out.append("")
    out.append("/// Uma linha de `JSC_FOREACH_BUILTIN_CODE` com os `s_xxxCode*` e o metadado do fonte.")
    out.append("#[derive(Clone, Copy, Debug)]")
    out.append("pub struct BuiltinCode {")
    out.append("    /// `codeName` (`arrayPrototypeAtCode`).")
    out.append("    pub code_name: &'static str,")
    out.append("    /// `functionName` (`at`).")
    out.append("    pub function_name: &'static str,")
    out.append("    /// `overriddenName` (`ASCIILiteral()` é `None`).")
    out.append("    pub overridden_name: Option<&'static str>,")
    out.append("    /// `s_xxxCode - s_JSCCombinedCode`.")
    out.append("    pub offset: usize,")
    out.append("    /// `s_xxxCodeLength`.")
    out.append("    pub length: usize,")
    out.append("    pub construct_ability: ConstructAbility,")
    out.append("    pub constructor_kind: ConstructorKind,")
    out.append("    pub implementation_visibility: ImplementationVisibility,")
    out.append("    pub inline_attribute: InlineAttribute,")
    out.append("    pub intrinsic: Intrinsic,")
    out.append("    /// `s_JSCBuiltinSourceMetadata[index]`.")
    out.append("    pub metadata: BuiltinSourceMetadata,")
    out.append("}")
    out.append("")
    out.append("/// Indexada por `BuiltinCodeIndex`.")
    out.append("pub static BUILTIN_CODES: [BuiltinCode; NUMBER_OF_BUILTIN_CODES] = [")
    for e, offset, length, meta in rows:
        name = e["code_name"]
        overridden = "None" if e["overridden"] is None else f"Some({rust_str(e['overridden'])})"
        out.append("    BuiltinCode {")
        out.append(f"        code_name: {rust_str(name)},")
        out.append(f"        function_name: {rust_str(e['function_name'])},")
        out.append(f"        overridden_name: {overridden},")
        out.append(f"        offset: {offset},")
        out.append(f"        length: {length},")
        out.append(f"        construct_ability: ConstructAbility::{attributes['construct_ability'][name]},")
        out.append(f"        constructor_kind: ConstructorKind::{attributes['constructor_kind'][name]},")
        out.append(f"        implementation_visibility: ImplementationVisibility::{attributes['visibility'][name]},")
        out.append(f"        inline_attribute: InlineAttribute::{attributes['inline_attribute'][name]},")
        out.append(f"        intrinsic: Intrinsic::{attributes['intrinsic'][name]},")
        out.append("        metadata: BuiltinSourceMetadata {")
        for field, value in zip(
            ["source_length", "parameters_start", "parameter_count", "line_count", "end_column",
             "offset_of_last_newline", "position_before_last_newline_line_start_offset",
             "close_brace_offset_from_end", "is_async_function", "is_in_strict_context"], meta):
            out.append(f"            {field}: {value},")
        out.append("        },")
        out.append("    },")
    out.append("];")
    out.append("")
    out.append("/// `functionName##PublicName()` do `xxxExecutable` do C++.")
    out.append("pub fn public_name(names: &BuiltinNames, index: BuiltinCodeIndex) -> &Identifier {")
    out.append("    match index {")
    for e, *_ in rows:
        out.append(f"        BuiltinCodeIndex::{pascal(e['code_name'])} => names.{stems[e['function_name']]}_public_name(),")
    out.append("    }")
    out.append("}")
    out.append("")

    with open(OUT_JS, "wb") as f:
        f.write(combined)
    with open(OUT_RS, "w", encoding="utf-8") as f:
        f.write("\n".join(out))
    print(f"{len(rows)} builtins, {len(combined)} bytes de fonte -> {OUT_RS}, {OUT_JS}")


main()
