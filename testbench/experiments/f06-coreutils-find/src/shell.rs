//! Pipeline mínima pros casos `script` que só encadeiam programas da tabela (`find ... | sort`,
//! `find -print0 | xargs -0 wc -c`). Não é o shell do pseudo-linus: aceita palavras com aspas
//! simples, duplas e `\`, separadas por `|`, e recusa qualquer outra construção (expansão,
//! redirecionamento, `;`, `&&`, glob), que vira "unsupported". Os estágios rodam em sequência, cada
//! um com a saída inteira do anterior; pra programas que terminam sozinhos o resultado é o mesmo da
//! pipeline concorrente do bash.

use std::sync::{Arc, Mutex};

use harness::{Invocation, Outcome};
use sysio::process::RunResult;

use crate::exec::{self, Root, Table};
use crate::sandbox::Sandbox;

/// Quebra o script em estágios; `None` quando há construção fora do subconjunto.
pub fn parse(script: &str) -> Option<Vec<Vec<String>>> {
    let mut stages: Vec<Vec<String>> = vec![Vec::new()];
    let mut word = String::new();
    let mut in_word = false;
    let mut chars = script.trim().chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            ' ' | '\t' => {
                if in_word {
                    stages.last_mut()?.push(std::mem::take(&mut word));
                    in_word = false;
                }
            }
            '|' => {
                if chars.peek() == Some(&'|') {
                    return None;
                }
                if in_word {
                    stages.last_mut()?.push(std::mem::take(&mut word));
                    in_word = false;
                }
                if stages.last()?.is_empty() {
                    return None;
                }
                stages.push(Vec::new());
            }
            '\'' => {
                in_word = true;
                loop {
                    match chars.next()? {
                        '\'' => break,
                        ch => word.push(ch),
                    }
                }
            }
            '"' => {
                in_word = true;
                loop {
                    match chars.next()? {
                        '"' => break,
                        '$' | '`' => return None,
                        '\\' => match chars.next()? {
                            ch @ ('"' | '\\') => word.push(ch),
                            '$' | '`' => return None,
                            ch => {
                                word.push('\\');
                                word.push(ch);
                            }
                        },
                        ch => word.push(ch),
                    }
                }
            }
            '\\' => {
                in_word = true;
                word.push(chars.next()?);
            }
            ';' | '&' | '<' | '>' | '$' | '`' | '(' | ')' | '{' | '}' | '*' | '?' | '[' | '\n' | '~' => {
                return None;
            }
            '#' if !in_word => return None,
            ch => {
                in_word = true;
                word.push(ch);
            }
        }
    }
    if in_word {
        stages.last_mut()?.push(word);
    }
    if stages.iter().any(Vec::is_empty) {
        return None;
    }
    Some(stages)
}

/// Os programas da pipeline, se todos estiverem na tabela.
pub fn programs(script: &str) -> Option<Vec<String>> {
    let stages = parse(script)?;
    let names: Vec<String> = stages.iter().map(|s| s[0].clone()).collect();
    names.iter().all(|n| exec::program(n).is_some()).then_some(names)
}

/// Roda a pipeline sobre o sandbox do caso.
pub fn run(inv: &Invocation, script: &str) -> Outcome {
    let Some(stages) = parse(script) else {
        return Outcome::unsupported("script fora do subconjunto de pipeline da bancada");
    };
    let sandbox = Sandbox::for_case(&inv.files, inv.faketime.as_deref());
    let root = Root {
        sandbox: &sandbox,
        inv,
        table: Arc::new(Table::default()),
        stderr: Arc::new(Mutex::new(Vec::new())),
    };
    let mut input = inv.stdin.clone();
    let mut last = RunResult::Exited(0);
    for stage in &stages {
        match root.run(stage, input) {
            Err(why) => return Outcome::unsupported(why),
            Ok((out, result)) => {
                input = out;
                last = result;
                if matches!(last, RunResult::Panicked(_)) {
                    break;
                }
            }
        }
    }
    exec::finish(&root, input, last)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_quoted_pipelines_and_rejects_the_rest() {
        assert_eq!(
            parse("find . -name '*.py' | sort").unwrap(),
            vec![vec!["find", ".", "-name", "*.py"], vec!["sort"]]
        );
        assert_eq!(parse(r#"xargs -0 wc -c"#).unwrap(), vec![vec!["xargs", "-0", "wc", "-c"]]);
        assert_eq!(parse(r#"echo "a b" c\ d"#).unwrap(), vec![vec!["echo", "a b", "c d"]]);
        assert!(parse("ls *.txt | xargs echo").is_none());
        assert!(parse("echo $HOME").is_none());
        assert!(parse("a && b").is_none());
        assert!(parse("sort > out").is_none());
        assert!(parse("a | | b").is_none());
    }
}
