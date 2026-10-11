//! Os textos de ajuda do `bun` 1.4.2, medidos no oráculo (`bun --help`, `bun` sem argumentos, `bun run`), e a tabela
//! de opções que a ajuda descreve (o `cli.rs` a usa para saber qual opção leva valor).
//!
//! A ajuda de nível superior sorteia a cada execução o argumento de exemplo de sete comandos (`x`, `add`, `remove`,
//! `update`, `info`, `why`, `create`); as listas abaixo são as que o oráculo mostrou em 400 execuções.

use sysabi::sys;

const TOP_HEAD: &str = include_str!("help/top-head.txt");
const TOP_FLAGS: &str = include_str!("help/top-flags.txt");
const TOP_TAIL: &str = include_str!("help/top-tail.txt");

/// A ajuda de `bun run` (a mesma de `bun run --help`).
pub const RUN: &str = include_str!("help/run.txt");

/// Largura da coluna do argumento de exemplo na tabela de comandos.
const EXAMPLE_WIDTH: usize = 21;

const PACKAGES: [&str; 10] =
    ["@remix-run/dev", "@zarfjs/zarf", "react", "hono", "tailwindcss", "elysia", "@shumai/shumai", "zod", "@evan/duckdb", "lyra"];

/// O comando e a lista de argumentos de exemplo que a ajuda sorteia para ele.
const EXAMPLES: [(&str, &[&str]); 7] = [
    ("x", &["next", "nuxi", "bun-repl", "eslint", "prettier", "prisma", "vite"]),
    ("add", &PACKAGES),
    ("remove", &["redux", "is-array", "underscore", "webpack", "browserify", "left-pad", "@parcel/core", "jquery", "moment", "backbone", "babel-core"]),
    ("update", &PACKAGES),
    ("info", &PACKAGES),
    ("why", &PACKAGES),
    ("create", &["vite", "svelte", "elysia", "astro", "next-app"]),
];

/// Um número sorteado em `0..len` do `getrandom` do sandbox.
fn pick(len: usize) -> usize {
    let mut bytes = [0u8; 4];
    let _ = sys::current().getrandom(&mut bytes);
    u32::from_le_bytes(bytes) as usize % len
}

/// A ajuda de nível superior: a curta (`bun`, opção inválida) ou a longa (`bun --help`), com os exemplos sorteados.
pub fn top(long: bool) -> String {
    let mut text = String::from(TOP_HEAD);
    if long {
        text.push_str(TOP_FLAGS);
    }
    text.push_str(TOP_TAIL);
    for (command, pool) in EXAMPLES {
        let example = pool[pick(pool.len())];
        text = text.replace(&format!("{{{command}}}"), &format!("{example:<EXAMPLE_WIDTH$}"));
    }
    text
}

/// Uma opção da tabela de opções das ajudas.
pub struct Flag {
    pub short: Option<char>,
    pub long: Option<String>,
    pub takes_value: bool,
}

/// As opções que as ajudas de `bun run` e de nível superior descrevem (`-F, --filter=<val>`, `--silent`, `-i`).
pub fn flags() -> Vec<Flag> {
    let mut table = Vec::new();
    for line in RUN.lines().chain(TOP_FLAGS.lines()) {
        if !line.starts_with("  -") && !line.starts_with("      --") {
            continue;
        }
        let spec = line.trim_start().split("  ").next().unwrap_or("");
        let mut flag = Flag { short: None, long: None, takes_value: spec.contains("=<val>") };
        for part in spec.split(", ") {
            if let Some(long) = part.strip_prefix("--") {
                flag.long = Some(long.split('=').next().unwrap_or(long).to_string());
            } else if let Some(short) = part.strip_prefix('-') {
                flag.short = short.chars().next();
            }
        }
        table.push(flag);
    }
    table
}
