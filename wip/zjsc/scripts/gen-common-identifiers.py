#!/usr/bin/env python3
"""Gera src/runtime/common_identifiers.rs (o `CommonIdentifiers.h` e o `.cpp` do C++) e
src/runtime/builtin_names/generated.rs (a parte gerada do `BuiltinNames.h`/`.cpp`: os campos, o
construtor e os acessores que o C++ produz expandindo macros).

As listas são as macros X do C++, lidas dos próprios cabeçalhos e expandidas na mesma ordem:
`JSC_COMMON_IDENTIFIERS_EACH_*` e `JSC_PARSER_PRIVATE_NAMES` (CommonIdentifiers.h),
`JSC_COMMON_PRIVATE_IDENTIFIERS_EACH_*` (BuiltinNames.h), `JSC_COMMON_BYTECODE_INTRINSIC_*`
(bytecode/BytecodeIntrinsicRegistry.h) e `JSC_FOREACH_BUILTIN_FUNCTION_NAME`
(derived/JavaScriptCore/JSCBuiltins.h).

Nomes: o campo vira snake_case do nome C++ com o mesmo sufixo (`xxx_keyword`, `xxx_symbol`,
`xxx_private_name`, `xxx_private_field`). Palavra reservada do Rust vira identificador cru
(`r#type`); as que não admitem `r#` ganham `_` no fim. Nomes que colidem depois do snake_case
(`Map` e `map`) recebem `_upper` ou `_dup` e saem listados no stderr.

Uso: scripts/gen-common-identifiers.py (na raiz do crate).
"""
import os
import re
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
JSC = os.path.join(ROOT, "upstream", "JavaScriptCore")

SOURCES = [
    os.path.join(JSC, "runtime", "CommonIdentifiers.h"),
    os.path.join(JSC, "builtins", "BuiltinNames.h"),
    os.path.join(JSC, "bytecode", "BytecodeIntrinsicRegistry.h"),
    os.path.join(ROOT, "derived", "JavaScriptCore", "JSCBuiltins.h"),
]

RUST_KEYWORDS = {
    "as", "break", "const", "continue", "else", "enum", "extern", "false", "fn", "for", "if",
    "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref", "return", "static",
    "struct", "trait", "true", "type", "unsafe", "use", "where", "while", "async", "await", "dyn",
    "abstract", "become", "box", "do", "final", "macro", "override", "priv", "typeof", "unsized",
    "virtual", "yield", "try", "gen",
}
RUST_NO_RAW = {"crate", "self", "Self", "super", "_"}


def read_macros():
    """Todos os `#define NOME(macro) \\` com a lista de itens: `("name", x)` ou `("ref", OUTRA)`."""
    defs = {}
    for path in SOURCES:
        with open(path, encoding="utf-8") as f:
            lines = f.read().split("\n")
        i = 0
        while i < len(lines):
            m = re.match(r"#define (\w+)\(macro\)\s*\\?\s*$", lines[i])
            if not m:
                i += 1
                continue
            items = []
            cont = lines[i].rstrip().endswith("\\")
            i += 1
            while cont and i < len(lines):
                line = lines[i]
                cont = line.rstrip().endswith("\\")
                ref = re.match(r"\s*(\w+)\(macro\)\s*\\?\s*$", line)
                named = re.match(r"\s*macro\((\w+)(?:\)|,)", line)
                if ref:
                    items.append(("ref", ref.group(1)))
                elif named:
                    items.append(("name", named.group(1)))
                i += 1
            defs[m.group(1)] = items
    return defs


def expand(defs, name):
    out = []
    for kind, value in defs[name]:
        if kind == "name":
            out.append(value)
        else:
            out.extend(expand(defs, value))
    return out


def snake(name):
    s = re.sub(r"(.)([A-Z][a-z]+)", r"\1_\2", name)
    s = re.sub(r"([a-z0-9])([A-Z])", r"\1_\2", s)
    return re.sub(r"_+", "_", s).lower()


class Namer:
    """Entrega nomes de campo únicos: a colisão pós-snake_case recebe um sufixo."""

    def __init__(self, reserved=()):
        self.used = set(reserved)

    def take(self, cpp_name, suffix=""):
        base = snake(cpp_name) + suffix
        name = base
        if name in self.used:
            name = base + ("_upper" if cpp_name[:1].isupper() else "_dup")
            print(f"colisão de nome: {cpp_name} -> {name}", file=sys.stderr)
        assert name not in self.used, name
        self.used.add(name)
        return name


def escape(name):
    if name in RUST_NO_RAW:
        return name + "_"
    if name in RUST_KEYWORDS:
        return "r#" + name
    return name


def rust_bytes(text):
    assert text.isascii() and '"' not in text and "\\" not in text, text
    return 'b"' + text + '"'


def gen_builtin_names(defs):
    functions = expand(defs, "JSC_FOREACH_BUILTIN_FUNCTION_NAME")
    properties = expand(defs, "JSC_COMMON_PRIVATE_IDENTIFIERS_EACH_PROPERTY_NAME")
    symbols = expand(defs, "JSC_COMMON_PRIVATE_IDENTIFIERS_EACH_WELL_KNOWN_SYMBOL")
    names = functions + properties
    # Um nome só: `name##PublicName` repetido não compilaria no C++.
    assert len(set(names)) == len(names), "nome repetido entre as listas de BuiltinNames"
    namer = Namer(["dollar_vm", "poly_proto", "stack", "empty_identifier"])
    stems = {n: namer.take(n) for n in names}
    symbol_namer = Namer()
    symbol_stems = {n: symbol_namer.take(n) for n in symbols}

    fields = ["    pub(super) m_empty_identifier: Identifier,"]
    for n in names:
        s = stems[n]
        fields.append(f"    pub(super) m_{s}: Identifier,")
        fields.append(f"    pub(super) m_{s}_private_name: Identifier,")
    for n in symbols:
        s = symbol_stems[n]
        fields.append(f"    pub(super) m_{s}_symbol: Identifier,")
        fields.append(f"    pub(super) m_{s}_symbol_private_identifier: Identifier,")
    fields += [
        "    pub(super) m_dollar_vm_name: Identifier,",
        "    pub(super) m_dollar_vm_private_name: Identifier,",
        "    pub(super) m_poly_proto_private_name: Identifier,",
        "    pub(super) m_stack_private_name: Identifier,",
        "    pub(super) m_private_name_set: PrivateNameSet,",
        "    pub(super) m_well_known_symbols_map: WellKnownSymbolMap,",
    ]

    init = ["            m_empty_identifier: empty_identifier.clone(),"]
    for n in names:
        s = stems[n]
        init.append(f"            m_{s}: public({rust_bytes(n)}),")
        init.append(f"            m_{s}_private_name: private({rust_bytes(n)}),")
    for n in symbols:
        s = symbol_stems[n]
        init.append(f"            m_{s}_symbol: well_known({rust_bytes('Symbol.' + n)}),")
        init.append(f"            m_{s}_symbol_private_identifier: public({rust_bytes(n)}),")
    init += [
        f"            m_dollar_vm_name: public({rust_bytes('$vm')}),",
        f"            m_dollar_vm_private_name: private({rust_bytes('$vm')}),",
        f"            m_poly_proto_private_name: private({rust_bytes('PolyProto')}),",
        f"            m_stack_private_name: private({rust_bytes('stack')}),",
        "            m_private_name_set: PrivateNameSet::new(),",
        "            m_well_known_symbols_map: WellKnownSymbolMap::new(),",
    ]

    privates = ",\n".join(f"            this.m_{stems[n]}_private_name.clone()" for n in names)
    pairs = ",\n".join(
        f"            (this.m_{symbol_stems[n]}_symbol_private_identifier.clone(),"
        f" this.m_{symbol_stems[n]}_symbol.clone())" for n in symbols)

    accessors = []
    for n in names:
        s = stems[n]
        accessors.append(
            f"    /// `{n}PublicName()`.\n"
            f"    pub fn {s}_public_name(&self) -> &Identifier {{\n"
            f"        &self.m_{s}\n    }}\n\n"
            f"    /// `{n}PrivateName()`: `Identifier::fromUid(Symbols::{n}PrivateName)`.\n"
            f"    pub fn {s}_private_name(&self) -> Identifier {{\n"
            f"        self.m_{s}_private_name.clone()\n    }}\n")
    for n in symbols:
        s = symbol_stems[n]
        accessors.append(
            f"    /// `{n}Symbol()`.\n"
            f"    pub fn {s}_symbol(&self) -> &Identifier {{\n"
            f"        &self.m_{s}_symbol\n    }}\n")

    out = f"""//! Gerado por `scripts/gen-common-identifiers.py` a partir de `builtins/BuiltinNames.h`,
//! `bytecode/BytecodeIntrinsicRegistry.h` e `derived/JavaScriptCore/JSCBuiltins.h`. Não editar à
//! mão. É o que o C++ produz expandindo `JSC_FOREACH_BUILTIN_FUNCTION_NAME`,
//! `JSC_COMMON_PRIVATE_IDENTIFIERS_EACH_PROPERTY_NAME` e
//! `JSC_COMMON_PRIVATE_IDENTIFIERS_EACH_WELL_KNOWN_SYMBOL`: campos, construtor e acessores. O que
//! é escrito à mão (as consultas, `appendExternalName`) fica em `builtin_names.rs`.
//!
//! Os `Symbols::xxxPrivateName` do C++ são `StaticSymbolImpl` globais; aqui cada `BuiltinNames`
//! materializa os seus no construtor e os guarda como `Identifier` em `m_xxx_private_name`.

use crate::runtime::builtin_names::{{PrivateNameSet, WellKnownSymbolMap}};
use crate::runtime::identifier::Identifier;
use crate::runtime::vm::VM;
use crate::wtf::text::symbol_impl::{{StaticSymbolImpl, S_FLAG_DEFAULT, S_FLAG_IS_PRIVATE}};

/// `class BuiltinNames`.
#[derive(Debug)]
pub struct BuiltinNames {{
{chr(10).join(fields)}
}}

impl BuiltinNames {{
    /// `BuiltinNames(VM&, CommonIdentifiers*)`: só o `emptyIdentifier` do `CommonIdentifiers` é lido.
    pub fn new(vm: &VM, empty_identifier: &Identifier) -> BuiltinNames {{
        let public = |name: &'static [u8]| Identifier::from_span(vm, name);
        let private = |name: &'static [u8]| {{
            Identifier::from_uid_symbol(&StaticSymbolImpl::new8(name, S_FLAG_IS_PRIVATE).symbol_impl())
        }};
        let well_known = |name: &'static [u8]| {{
            Identifier::from_uid_symbol(&StaticSymbolImpl::new8(name, S_FLAG_DEFAULT).symbol_impl())
        }};
        let mut this = BuiltinNames {{
{chr(10).join(init)}
        }};

        // `m_privateNameSet.reserveInitialCapacity(1024)`.
        this.m_private_name_set.reserve(1024);

        let private_names: Vec<Identifier> = vec![
{privates},
        ];
        for private_name in &private_names {{
            this.insert_private_name(private_name);
        }}
        let well_known_symbols: Vec<(Identifier, Identifier)> = vec![
{pairs},
        ];
        for (key, symbol) in &well_known_symbols {{
            this.add_well_known_symbol(key, symbol);
        }}
        let dollar_vm = this.m_dollar_vm_private_name.clone();
        this.insert_private_name(&dollar_vm);
        this
    }}

{chr(10).join(accessors)}
    /// `dollarVMPublicName()`.
    pub fn dollar_vm_public_name(&self) -> &Identifier {{
        &self.m_dollar_vm_name
    }}

    /// `dollarVMPrivateName()`.
    pub fn dollar_vm_private_name(&self) -> &Identifier {{
        &self.m_dollar_vm_private_name
    }}

    /// `polyProtoName()`.
    pub fn poly_proto_name(&self) -> &Identifier {{
        &self.m_poly_proto_private_name
    }}

    /// `stackPrivateName()`.
    pub fn stack_private_name(&self) -> &Identifier {{
        &self.m_stack_private_name
    }}
}}
"""
    out_path = os.path.join(ROOT, "src", "runtime", "builtin_names", "generated.rs")
    os.makedirs(os.path.dirname(out_path), exist_ok=True)
    with open(out_path, "w", encoding="utf-8") as f:
        f.write(out)
    return stems


def gen_common_identifiers(defs):
    properties = expand(defs, "JSC_COMMON_IDENTIFIERS_EACH_PROPERTY_NAME")
    private_fields = expand(defs, "JSC_COMMON_IDENTIFIERS_EACH_PRIVATE_FIELD")
    keywords = expand(defs, "JSC_COMMON_IDENTIFIERS_EACH_KEYWORD")
    symbols = expand(defs, "JSC_COMMON_PRIVATE_IDENTIFIERS_EACH_WELL_KNOWN_SYMBOL")
    parser_privates = expand(defs, "JSC_PARSER_PRIVATE_NAMES")
    # Os acessores `xxx_private_name` de BuiltinNames, com o mesmo snake_case do gerador dele.
    builtin_stems = {n: snake(n) for n in parser_privates}

    namer = Namer([
        "null_identifier", "empty_identifier", "underscore_proto", "use_strict_identifier",
        "negative_one_identifier", "using_identifier", "m_builtin_names",
    ])
    f_private = [(n, namer.take(n, "_private_name")) for n in parser_privates]
    f_keyword = [(n, namer.take(n, "_keyword")) for n in keywords]
    f_property = [(n, namer.take(n)) for n in properties]
    f_symbol = [(n, namer.take(n, "_symbol")) for n in symbols]
    f_field = [(n, namer.take(n, "_private_field")) for n in private_fields]

    decl = ["    pub null_identifier: Identifier,", "    pub empty_identifier: Identifier,",
            "    pub underscore_proto: Identifier,", "    pub use_strict_identifier: Identifier,",
            "    pub negative_one_identifier: Identifier,", "    pub using_identifier: Identifier,",
            "    m_builtin_names: BuiltinNames,"]
    init = ["            null_identifier: Identifier::null_identifier(),",
            "            empty_identifier,",
            f"            underscore_proto: Identifier::from_span(vm, {rust_bytes('__proto__')}),",
            f"            use_strict_identifier: Identifier::from_span(vm, {rust_bytes('use strict')}),",
            f"            negative_one_identifier: Identifier::from_span(vm, {rust_bytes('-1')}),",
            "            using_identifier: Identifier::from_span(vm, " + rust_bytes('using') + "),"]
    for n, f in f_private:
        decl.append(f"    pub {escape(f)}: Identifier,")
        init.append(f"            {escape(f)}: m_builtin_names.{builtin_stems[n]}_private_name(),")
    for n, f in f_keyword:
        decl.append(f"    pub {escape(f)}: Identifier,")
        init.append(f"            {escape(f)}: Identifier::from_span(vm, {rust_bytes(n)}),")
    for n, f in f_property:
        decl.append(f"    pub {escape(f)}: Identifier,")
        init.append(f"            {escape(f)}: Identifier::from_span(vm, {rust_bytes(n)}),")
    for n, f in f_symbol:
        decl.append(f"    pub {escape(f)}: Identifier,")
        init.append(f"            {escape(f)}: m_builtin_names.{snake(n)}_symbol().clone(),")
    for n, f in f_field:
        decl.append(f"    pub {escape(f)}: Identifier,")
        init.append(f"            {escape(f)}: Identifier::from_span(vm, {rust_bytes('#' + n)}),")

    # O literal avalia os campos na ordem em que aparecem: o `m_builtin_names` é movido por último,
    # depois de lidos dele os private names e os símbolos.
    init.append("            m_builtin_names,")
    out = f"""//! Gerado por `scripts/gen-common-identifiers.py` a partir de `runtime/CommonIdentifiers.h` (as
//! listas `JSC_COMMON_IDENTIFIERS_EACH_*` e `JSC_PARSER_PRIVATE_NAMES`) e de
//! `runtime/CommonIdentifiers.cpp` (o construtor). Não editar à mão.
//!
//! A ordem dos campos é a do C++: os seis fixos, o `m_builtinNames`, os private names do parser,
//! as palavras-chave, os nomes de propriedade, os símbolos conhecidos e o private field.

use crate::runtime::builtin_names::BuiltinNames;
use crate::runtime::identifier::Identifier;
use crate::runtime::vm::VM;

/// `class CommonIdentifiers`.
#[derive(Debug)]
pub struct CommonIdentifiers {{
{chr(10).join(decl)}
}}

impl CommonIdentifiers {{
    /// `CommonIdentifiers(VM&)`. O `m_builtinNames` é criado primeiro (o
    /// `makeUnique<BuiltinNames>(vm, this)`) e só é movido para o campo na última posição do
    /// literal, depois de lidos os private names e os símbolos que saem dele.
    pub fn new(vm: &VM) -> CommonIdentifiers {{
        let empty_identifier = Identifier::empty_identifier();
        let m_builtin_names = BuiltinNames::new(vm, &empty_identifier);
        CommonIdentifiers {{
{chr(10).join(init)}
        }}
    }}

    /// `builtinNames()`.
    pub fn builtin_names(&self) -> &BuiltinNames {{
        &self.m_builtin_names
    }}

    /// `appendExternalName(publicName, privateName)`.
    pub fn append_external_name(&mut self, public_name: &Identifier, private_name: &Identifier) {{
        self.m_builtin_names.append_external_name(public_name, private_name);
    }}
}}
"""
    with open(os.path.join(ROOT, "src", "runtime", "common_identifiers.rs"), "w",
              encoding="utf-8") as f:
        f.write(out)


def main():
    defs = read_macros()
    gen_builtin_names(defs)
    gen_common_identifiers(defs)


main()
