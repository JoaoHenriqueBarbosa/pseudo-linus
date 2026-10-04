# Guia de porte dos utilitários do uutils pro pseudo-linus

Este guia é pra quem porta um utilitário do uutils 0.12 (vendorizado em `vendor/src/uu/<util>`) pro
pseudo-linus. Leia antes: `docs/implementation.md`, `crates/sysio/README.md` e, no README do F06
(`testbench/experiments/f06-coreutils-find/README.md`), as seções "Por que não dá pra só trocar o
std::fs" e "Bloqueios por utilitário".

## Regras do dono

1. Nunca travessão (U+2014) nem meia-risca (U+2013) no que você escrever (código vendorizado de
   terceiros mantém o texto original).
2. Prosa em português com acentuação; identificadores em inglês.
3. Arquivo novo com a tool Write; alteração com Edit. Nada de `sed -i`, heredoc ou script escrevendo
   arquivo. Copiar código de terceiros com `cp` pode.
4. Toda alteração de porte num arquivo vendorizado leva o comentário `Porte pseudo-linus` (em `.rs`
   com `//`, em `.toml` e `.ftl` com `#`), explicando o porquê em uma ou duas linhas.
5. `unsafe_code = "forbid"` em todo crate; o porte elimina unsafe. Nunca `unsafe impl`.
6. Não commite. Não edite crates de outros donos (`sysabi`, `kernel`, `vfs`, `pl-testing`, `shell`,
   testbench fora de `corpus/cases` e `golden`).
7. Licença: o projeto é MIT. Ao corrigir divergência contra o GNU, **não abra o código-fonte do GNU
   coreutils nem do findutils** (GPLv3). Use o manual, o comportamento no oráculo e os testes.
8. Sem rebaixamento: nada de stub, `TODO` ou dado fake entregue como implementação. O que faltar vai
   pro relatório como pendência, com o motivo.

## Arquitetura

- `vendor/src/uucore`: o uucore portado (dono: agente coreutils). Se o seu utilitário precisa de
  mudança no uucore, faça a menor mudança local possível, marcada, releia o arquivo antes de editar
  (outros agentes podem estar mexendo em outros arquivos dele) e liste no relatório final cada
  arquivo do uucore que você mudou. Não mude assinatura pública que outro utilitário usa.
- `vendor/src/uu/<util>`: cópias dos crates do crates.io; o porte é feito em cima delas.
- `src/<grupo>.rs`: a tabela do grupo (`pub(crate) fn programs() -> Vec<Program>`), com uma entrada
  por utilitário via `uu_main!(fn_name, "util", uu_util)` (ver `src/core.rs`). Utilitário que não é
  do uutils (ex.: `rev`) é escrito à mão num módulo próprio, com `sysio::run`.
- `staging/<grupo>`: workspace de preparação do grupo (tem `[workspace]` próprio). O porte em
  andamento compila e testa só ali, pra não quebrar o build de ninguém. A integração no crate
  principal (`Cargo.toml` e `src/lib.rs` do `ul-coreutils`) é feita pelo agente coreutils.
- Target próprio: `CARGO_TARGET_DIR=/home/john/projects/pseudo-linus/target/agents/coreutils-<grupo>`.

## Passo a passo de um utilitário

### 1. Manifesto (`vendor/src/uu/<util>/Cargo.toml`)

- `name = "uu_x"` vira `name = "pl-uu-x"` (o nome da lib continua `uu_x`).
- Em `[lib]`: `test = false` e `doctest = false` (os testes upstream são escritos contra o host).
- `[dependencies.uucore]`: `path = "../../uucore"` e `package = "pl-uucore"`; mantenha as features
  que o utilitário pedia, menos as que não existem mais (`libc`).
- Acrescente `[dependencies.sysio]` com `path = "../../../../../sysio"` (e `sysabi` com
  `path = "../../../../../sysabi"` se usar o contrato direto).
- clap sem a feature `wrap_help` (ela mede o terminal do host com `terminal_size`).
- Tire as dependências que tocam o host: `libc`, `nix`, `rustix`, `walkdir`, `tempfile`, `filetime`,
  `notify`, `ctrlc`, `hostname`, `platform-info`, `utmp-classic`, `memmap2`, `indicatif` (barra de
  progresso no terminal do host), `selinux`, `xattr`, `rand` com `thread_rng`/`OsRng` (use
  `sysio::random`). Dependências puras (memchr, bigdecimal, num-*, unicode-width, nom, bytecount,
  itertools, jiff com `tzdb-bundle-always` e sem tzdb do sistema...) ficam.
- `regex`/`fancy-regex`: onde o GNU usa regex POSIX (csplit, nl, pr, ptx, tac, expr), troque pelo
  `regex-posix` (`crates/regex-posix/API.md`), com a sintaxe que o GNU usa (BRE do GNU na maioria).
- `[lints.rust]` com `unsafe_code = "forbid"`.

### 2. Código

O porte é, na maioria das linhas, trocar o caminho do import:

| antes | depois |
|---|---|
| `std::fs::{File, OpenOptions, metadata, read_dir...}` | `sysio::fs::...` |
| `std::io::{stdin, stdout, stderr, Stdin, StdoutLock, IsTerminal...}` | `sysio::io::...` (o resto de `std::io` passa por `sysio::io` também) |
| `std::env::{var, args_os, current_dir...}` | `sysio::env::...` |
| `std::process::{exit, Command, Stdio, Child}` | `sysio::process::...` |
| `std::os::unix::fs::{MetadataExt, PermissionsExt, symlink...}` | `sysio::os::unix::fs::...` |
| `std::os::fd::{AsFd, AsRawFd, OwnedFd...}` | `sysio::os::fd::...` |
| `std::thread::{spawn, sleep}`, `std::time::{SystemTime::now, Instant}` | `sysio::thread::...`, `sysio::time::{now, Instant}` |
| `print!`, `println!`, `eprint!`, `eprintln!` | `sysio::print!`... (importe as macros: `use sysio::{println, eprintln};`) |
| `p.exists()`, `p.is_dir()`, `p.metadata()`, `p.canonicalize()` (métodos de `Path`) | `p.sys_exists()`... com `use sysio::path::PathExt;` (o compilador não avisa: método inerente ganha) |
| `libc::getuid`, `nix::unistd::*`, `rustix::*` | `sysio::users`, `sysio::unistd`, `sysio::fs`, ou `sysio::sysabi` direto |
| `getpwuid`/`getgrgid` | `uucore::entries` (já portado) ou `sysio::users` |
| `TimeZone::system()`, `Zoned::now()`, `chrono::Local` | `uucore::time::process_time_zone()` e `sysio::time::now()` |
| `static X: OnceLock/LazyLock` com ambiente, locale, argv ou estado do processo | `sysio::proc::proc_local(\|\| ...)` (estado por pseudo-processo); se o valor não é `Send + Sync`, um `thread_local` indexado por `sysio::proc::frame_id()` (ver `uucore/src/lib/mods/locale.rs`) |
| `std::process::exit` dentro do clap (`get_matches`, `Error::exit`, `print_help`) | `try_get_matches_from` + o tratamento do uucore; ajuda com `render_help()` escrita no stdout do sysio |
| `splice`, `copy_file_range`, `fadvise`, `mmap` | leitura e escrita comuns |
| sinais com handler (`sigaction`) | `uucore::signals::install_signal_handler(sig, fn)` + `run_pending_handlers()` nos laços, ou `sysio::unistd::signal(sig, SigDisposition::Catch)` + `take_caught_signals()` |

Erros: a mensagem de um `io::Error` com errno sai de `sysio::errno::strerror(&e)` (glibc 2.41). O
`UIoError` do uucore já faz isso. Nunca use o `Display` do `io::Error` em mensagem pro usuário.

Saída: o stdout do sysio tem buffer igual ao stdio da glibc (bloco de 4096 em pipe/arquivo). Se o
GNU escreve direto com `write(2)` naquele ponto (cat, tee, yes, dd, tail -f), chame `flush()` onde o
GNU escreveria, pra que a ordem com o stderr fique igual.

### 3. Auditoria

```sh
cd crates/ul-coreutils/staging/<grupo>
CARGO_TARGET_DIR=... cargo clippy 2>&1 | grep -B2 -A6 disallowed
```

O `crates/ul-coreutils/clippy.toml` proíbe as chamadas que tocam o host (fs, env, io, process, time,
thread, métodos de `Path`, macros de impressão do std). Zero avisos `disallowed` no código do
utilitário (o `build.rs` do uucore é tempo de compilação e pode). Também: `grep -rn "unsafe\|libc::\|nix::\|rustix::" vendor/src/uu/<util>/src`
só pode achar código atrás de `cfg` de outra plataforma.

### 4. Conformidade

```sh
cd crates/ul-coreutils/staging/<grupo>
CARGO_TARGET_DIR=... CONF_UTILS=cut,tr cargo test --test conformance -- --nocapture
```

- Roda os casos de `testbench/corpus/cases/coreutils/*.toml` do utilitário contra o golden do GNU
  (Debian 13, coreutils 9.7) no kernel de teste (`sysabi::testkit`). `CONF_TRACE=1` mostra cada
  caso. Casos `script` precisam de `bash` e ficam como "sem shell" enquanto o shell não existe.
- Kernel real: `cargo test --features kernel-tests` com `CONF_KERNEL=1` (threads de verdade, pipes
  concorrentes, sinais; o kernel de teste roda filho e thread de forma síncrona, então `timeout`,
  pipelines e produtor/consumidor só valem de verdade no kernel real). O kernel é de outro agente e
  pode estar em obra: se não compilar, meça no testkit e registre.
- Meta: igualar ou superar o uutils original e chegar no GNU, inclusive stderr e código de saída.
  As mensagens de erro do uutils divergem do GNU em muitos pontos (aspas: em C.UTF-8 o GNU usa
  `‘x’` curvas em umas mensagens e `'x'` retas em outras, às vezes no mesmo programa; textos
  diferentes; `Try 'x --help'`). O golden é a verdade.
- Utilitário sem casos suficientes: crie casos novos num arquivo seu em
  `testbench/corpus/cases/coreutils/<nome>.toml` (formato no `testbench/README.md`) e gere o golden
  com `cd testbench && cargo run -q -p oracle -- gen --tool coreutils --missing-only` (sempre com
  `--missing-only`). Caso tem que ser determinístico e independente da máquina.
- O oráculo também serve pra perguntar como o GNU se comporta: escreva o caso, gere o golden, leia.

### 5. Relatório

Por utilitário: placar estrito/leniente (testkit e, se der, kernel), o que diverge e por quê, o que
ficou de fora, arquivos do uucore alterados.
