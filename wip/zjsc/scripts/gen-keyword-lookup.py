#!/usr/bin/env python3
"""Gera src/parser/keyword_lookup.rs a partir de upstream/JavaScriptCore/parser/Keywords.table, o
mesmo insumo do KeywordLookupGenerator.py do WebKit. O C++ gerado é uma árvore de `if` sobre os
caracteres; o resultado dela é: a palavra-chave `k` casa quando o código começa com `k` e o
caractere seguinte não pode continuar um identificador nem começar um escape. Como nenhuma
palavra-chave é prefixo de outra seguida de caractere que encerra identificador, no máximo uma
casa, e a busca linear pela tabela dá o mesmo resultado que a árvore.
Uso: scripts/gen-keyword-lookup.py (na raiz do crate).
"""
import os

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def main():
    rows = []
    with open(os.path.join(ROOT, "upstream", "JavaScriptCore", "parser", "Keywords.table"),
              encoding="utf-8") as f:
        for line in f:
            line = line.strip()
            if not line or line.startswith("#") or line.startswith("@"):
                continue
            word, token = line.split()
            rows.append((word, token))
    longest = max(len(w) for w, _ in rows)
    table = ",\n".join(
        f'    Keyword {{ word: b"{w}", token: {t}, property_name: "{w}Keyword" }}' for w, t in rows)
    tokens = sorted({t for _, t in rows})
    out = f"""//! Gerado por `scripts/gen-keyword-lookup.py` a partir de `parser/Keywords.table`. Não editar à
//! mão. É o `KeywordLookup.h` do C++ (o `Lexer<T>::parseKeyword`) sem a árvore de `if`: ver o
//! comentário do gerador.

use crate::parser::parser_tokens::{{{", ".join(tokens)}, JSTokenType}};

/// `maxTokenLength`: o chamador só consulta a tabela quando restam pelo menos tantos caracteres.
pub const MAX_TOKEN_LENGTH: usize = {longest};

/// Uma linha do `Keywords.table`, com o nome do `propertyNames->xxxKeyword` que o C++ devolve em
/// `data->ident`.
pub struct Keyword {{
    pub word: &'static [u8],
    pub token: JSTokenType,
    pub property_name: &'static str,
}}

/// As linhas do `Keywords.table`, na ordem do arquivo.
pub static KEYWORDS: &[Keyword] = &[
{table},
];

/// O corpo do `parseKeyword`: a palavra-chave no começo de `code`, se houver, com `next_ends` dizendo
/// se o caractere logo depois dela é um `cannotBeIdentPartOrEscapeStart`. O chamador faz o
/// `internalShift` do tamanho da palavra e preenche `data->ident`.
pub fn match_keyword<C: Copy + Into<u32>>(
    code: &[C],
    next_ends: impl Fn(C) -> bool,
) -> Option<&'static Keyword> {{
    debug_assert!(code.len() >= MAX_TOKEN_LENGTH);
    KEYWORDS.iter().find(|k| {{
        code.len() > k.word.len()
            && k.word.iter().zip(code).all(|(&w, &c)| c.into() == w as u32)
            && next_ends(code[k.word.len()])
    }})
}}
"""
    with open(os.path.join(ROOT, "src", "parser", "keyword_lookup.rs"), "w", encoding="utf-8") as f:
        f.write(out)


main()
