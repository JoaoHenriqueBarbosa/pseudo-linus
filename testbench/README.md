# Bancada de testes do pseudo-linus

A bancada existe pra que nenhuma decisão do `docs/design.md` dependa de palpite. Ela responde as 40
hipóteses de `hypotheses.toml` e decide, ferramenta por ferramenta, se uma crate encaixa, se precisa de
fork, ou se o certo é fazer à mão.

## Estrutura

```
testbench/
  Cargo.toml          workspace da infraestrutura (crates/*)
  hypotheses.toml     registro H01..H40: frase, experimento, critério
  oracle/Dockerfile   Debian 13 pinado com as ferramentas GNU de referência
  corpus/cases/<tool>/*.toml   casos (entrada); corpus/upstream e corpus/agent são gitignored
  golden/<tool>/*.json         saída do oráculo pra cada caso (commitado)
  results/<exp>.json           métricas e vereditos (commitado)
  scratch/                     rascunho gitignored
  crates/harness      tipos de caso, MemTree, Candidate, comparação, cliente do oráculo, resultados
  crates/oracle       CLI: build da imagem e do agente, geração do golden
  crates/oracle-agent roda os casos dentro do container
  crates/depscan      acoplamento ao host e unsafe de uma crate e da árvore dela
  crates/runner       agrega results/ e escreve docs/bench-report.md
  experiments/<id>/   um workspace Cargo próprio por experimento
```

## Fluxo

```sh
cd testbench
cargo run -p oracle -- build                 # imagem Docker + oracle-agent (uma vez)
cargo run -p oracle -- gen --tool grep       # roda corpus/cases/grep no oráculo e grava golden/grep
cargo run -p depscan -- --manifest-path experiments/f04-awk-jq/Cargo.toml --package jaq-core
cargo run --release --manifest-path experiments/<id>/Cargo.toml    # grava results/<id>.json
cargo run -p runner                          # valida e escreve ../docs/bench-report.md
```

## Casos

Um arquivo `corpus/cases/<tool>/<nome>.toml`:

```toml
tool = "grep"
source = "agent-style"     # manual, agent-style, upstream:<suíte>

[[case]]
id = "grep-n-basic"        # único no arquivo
argv = ["grep", "-n", "foo", "in.txt"]     # roda direto, sem shell
tags = ["bre", "flags"]
[case.files]
"in.txt" = "foo\nbar\n"
"bin.dat" = { content_b64 = "AAE=" }
"link" = { symlink = "in.txt" }
"sub" = { dir = true, mode = 0o700 }

[[case]]
id = "grep-pipeline"
script = "printf 'a\\nb\\n' | grep -c a"   # roda com bash -c
stdin = "..."              # ou stdin_b64
faketime = "2026-01-15 12:00:00"           # só quando o caso depende do relógio
```

Ambiente fixo nos dois lados: `cwd = /work/case`, `LC_ALL=C.UTF-8`, `TZ=UTC`, `HOME=/root`, usuário
root, umask 022, mtime de todo arquivo de fixture = 2026-01-15T12:00:00Z (`harness::FIXTURE_MTIME`).
Casos não podem depender de coisa não determinística (mtime de arquivo criado durante o caso, pid, data
real); quem precisa de relógio usa `faketime`.

O golden de cada caso guarda stdout, stderr, exit (ou sinal) e o retrato do diretório de trabalho
depois da execução. A comparação é byte a byte: `strict` exige tudo igual, `lenient` aceita stderr
diferente.

## Experimentos

Cada `experiments/<id>/` é um workspace próprio (Cargo.lock e target próprios), pra que um experimento
com dependência quebrada não derrube os outros. Modelo de `Cargo.toml`:

```toml
[package]
name = "<id>"
version = "0.1.0"
edition = "2024"
rust-version = "1.98"
publish = false

[workspace]

[dependencies]
harness = { path = "../../crates/harness" }
depscan = { path = "../../crates/depscan" }

[lints.rust]
unsafe_code = "forbid"
```

O binário principal do experimento roda tudo e grava `results/<id>.json` via
`harness::ExperimentResult`, com um veredito pra cada hipótese que é dele em `hypotheses.toml` e um
`CandidateResult` pra cada candidato testado. Pra rodar um candidato contra o golden, implemente
`harness::Candidate` e chame `harness::score`. Cada experimento tem um `README.md` com Hipóteses,
Método, Candidatos, Resultado e Veredito.

Regras:

- **Nada de unsafe.** `unsafe_code = "forbid"` em todo crate, inclusive testes. Dependência pode ter
  unsafe interno; nunca `unsafe impl` nosso.
- **Hipótese refutada é resultado, não teste quebrado.** `cargo test` só falha por bug do harness ou da
  nossa implementação; o veredito vai pro JSON.
- **Se a crate candidata não serve, procurar outra e testar**; só cravar "fazer à mão" com a evidência
  registrada no README e no JSON.
- Build só em release (`cargo run --release`, `cargo test --release`), pra poupar disco.
- Prosa em português com acentuação, sem travessão nem meia-risca; identificadores em inglês.
- Comandos minerados dos transcripts (`corpus/agent/`) nunca são executados, só parseados.
