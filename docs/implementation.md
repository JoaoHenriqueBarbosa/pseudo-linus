# Implementação: arquitetura, donos e convenções

Este é o documento de coordenação da implementação do pseudo-linus. O desenho e o porquê de cada
decisão estão em `docs/design.md`; os números que sustentam cada decisão em `docs/bench-report.md`.
Leia os dois antes de começar.

## Camadas

```
host (daemon na VPS, CLI osh)            crates/host
  kernel (processos, fds, pipes, sinais,  crates/kernel
          escalonador, contabilidade)      usa crates/sched e crates/vfs
    vfs (tmpfs, procfs, devfs, hostfs)    crates/vfs
  ---------------------------------------- fronteira: trait sysabi::Syscalls
  sysabi (syscalls, tipos, errno, Ctx)    crates/sysabi
  sysio (fachada com a forma do std)      crates/sysio
  shell (bash)                            crates/shell
  userland (programas)                    crates/ul-*
  userland (agregador: tabela única)      crates/userland
```

Regra de ouro: **de `sysabi` pra baixo (shell, sysio, ul-*) nada toca o host**. Nada de `std::fs`,
`std::process`, `std::env`, `std::net`, `std::io::stdin/stdout/stderr`, `print!`, `println!`,
`eprintln!`, `dbg!`, `std::time::SystemTime::now()`, `std::thread::sleep`. Tudo passa por
`sysabi::sys` (ou pela fachada `sysio`). O tempo vem de `clock_gettime`, a aleatoriedade de
`getrandom`, o fuso de `local_timezone`. Só `crates/kernel`, `crates/vfs` (hostfs) e `crates/host`
falam com o host, e só onde o desenho manda.

## O contrato: `sysabi`

- `sysabi::Syscalls` é a lista de syscalls. O kernel implementa uma por processo e instala na thread do
  processo (modelo A: uma thread do SO por pseudo-processo). Programas chamam `sysabi::sys::*`.
- Caminhos e argumentos são bytes (`&[u8]`, `Vec<u8>`).
- Erros são `sysabi::Errno` com o número do Linux. A mensagem é `Errno::message()` (glibc exata). Nunca
  formate com o `Display` do `std::io::Error` (ele acrescenta " (os error N)").
- `sysabi::sys::exit(code)` termina o processo desenrolando a pilha (payload `ExitUnwind`).
- Um programa embutido é `sysabi::Program { name, dir, main }` com `main: fn(&mut Ctx, &[OsString]) -> i32`.
- **Mudança no `sysabi` só pelo coordenador** (a sessão principal). Precisa de uma syscall ou de um tipo
  novo? Mande mensagem pro `main` com a assinatura proposta e o porquê; não edite o crate.

## Donos

| Crate | Dono | Conteúdo |
|---|---|---|
| `sysabi` | coordenador | contrato, testkit |
| `pl-testing` | coordenador | candidato de conformidade (testkit e, depois, kernel real) |
| `kernel` | agente kernel | processos, threads com tokens de CPU do `sched`, fds, pipes, sinais, wait, spawn, exec, rlimits, contabilidade de memória, net_connect com allowlist |
| `vfs` | agente vfs | tmpfs persistente (imbl), namei, permissões com bypass do root, procfs, devfs, hostfs híbrido, montagens, readdir na ordem do tmpfs |
| `sched`, `rbtree` | já prontos | mudanças só pelo agente kernel, com os testes do E02 verdes |
| `shell` | agente shell | bash (programas `bash` e `sh`) sobre o `brush-parser` |
| `regex-posix` | agente regex | motor BRE/ERE do GNU (parser nosso + regex-automata + ferroni, leftmost-longest) |
| `ul-textproc` | agente regex | `grep`, `egrep`, `fgrep`, `sed` |
| `sysio` | agente coreutils | fachada com a forma do std sobre `sysabi` (sucessor do shim do F06) |
| `ul-coreutils` | agente coreutils | coreutils (fork do uutils sobre o `uucore` portado) |
| `ul-findutils` | agente coreutils | `find`, `xargs` |
| `ul-awk` | agente awk | `awk`, `gawk` (gawk 5.2.1 como alvo) |
| `ul-jq` | agente jq | `jq` (jaq + camada do F04), `yq` |
| `ul-diff`, `ul-archive` | agente arquivos | `diff`, `cmp`, `diff3`, `patch`; `tar`, `gzip`, `gunzip`, `zcat`, `bzip2`, `xz`, `zstd`, `zip`, `unzip`, `lzip` |
| `ul-misc`, `ul-procps` | agente misc | `bc`, `file`, `column`, `tree`, `xxd`, `hexdump`, `strings`, `which`, `envsubst`, `less`/`more` (não interativos); `ps`, `top -b`, `free`, `uptime`, `pgrep`, `pkill`, `pidof`, `killall`, `watch` |
| `ul-net`, `ul-sqlite` | agente net | `curl`, `wget`; `sqlite3` |
| `ul-git` | agente git | `git` (gix-* de baixo nível) |
| `userland` | coordenador | `all_programs()`: junta as tabelas de todos os `ul-*` e do shell |
| `host` | agente host | daemon JSON-RPC, supervisor e workers, CLI `osh`, imagem Docker |

Cada agente só edita os próprios crates. O workspace usa `members = ["crates/*"]`: um manifesto
quebrado, ou um `Cargo.toml` sem `src/lib.rs`, derruba o build de todo mundo. Em crate novo, crie o
`src/lib.rs` **antes** do `Cargo.toml`, e rode `cargo metadata --format-version 1 >/dev/null` depois de
mexer em dependência. Se o seu build falhar por causa do crate de outro, espere e tente de novo (avise o
`main` se passar de alguns minutos); não edite o crate do outro.

Código C com callback em Rust (ex.: VFS do sqlite): um `ExitUnwind`/`KillUnwind` levantado dentro do
callback atravessaria `extern "C"` e abortaria o processo host. Capture o payload no callback, devolva
erro pro C e relance com `std::panic::resume_unwind` quando o controle voltar pro Rust.

## Convenções

- `Cargo.toml` de crate: `edition.workspace = true`, `rust-version.workspace = true`,
  `license.workspace = true`, `publish.workspace = true`, `[lints] workspace = true`.
  `unsafe_code = "forbid"` vale pra todo crate, sem exceção. Dependência pode ter unsafe interno com API
  segura; nunca `unsafe impl` nosso.
- Cada programa exporta `pub fn programs() -> Vec<sysabi::Program>` no crate dele.
- Fidelidade é byte a byte contra o Debian 13 (bash 5.2.37, coreutils 9.7, gawk 5.2.1, grep 3.11, sed
  4.9, jq 1.7.1, findutils 4.10, diffutils 3.10, tar 1.35...). Mensagens de erro, códigos de saída e
  formatos têm que bater. Ambiente padrão: `LC_ALL=C.UTF-8`, `TZ=UTC`, usuário root, umask 022.
- Laço que pode rodar muito sem syscall (interpretadores, ordenação, regex) chama
  `sysabi::sys::checkpoint()` periodicamente (a cada salto pra trás, a cada N iterações).
- Tamanho vindo de entrada do usuário: `try_reserve`, nunca `with_capacity` direto.
- Recursão que depende da entrada (parsers, avaliadores): `stacker::maybe_grow` + limite de
  profundidade com o erro que o programa real daria.
- Locks: `parking_lot`, ou tratar `PoisonError`. Nada de `lock().unwrap()` em `std::Mutex` no kernel.
- Prosa (comentários, docs, mensagens de commit) em português com acentuação; identificadores em
  inglês. Nunca travessão nem meia-risca.
- Arquivo se cria com a tool Write e se altera com Edit (código vendorizado de terceiros pode ser
  copiado com `cp`, e a alteração em cima dele é com Edit).

## Testes

- Unitários dentro de cada crate.
- Programa isolado: `sysabi` com a feature `testkit` (`sysabi::testkit::TestKit`): FS em memória,
  fds, stdin e captura de stdout/stderr, `spawn` síncrono. Serve pra testar um programa antes do kernel
  real.
- Conformidade: `pl-testing` (dev-dependency) roda os casos de `testbench/corpus/cases/<tool>` contra o
  golden de `testbench/golden/<tool>` e dá o placar. Primeiro sobre o testkit; quando o kernel real
  estiver pronto, sobre ele. Casos novos de conformidade entram em `testbench/corpus/cases/<tool>/` e o
  golden se gera com `cd testbench && cargo run -q -p oracle -- gen --tool <tool>`.
- Cada agente usa um target próprio pra não esperar o lock dos outros:
  `CARGO_TARGET_DIR=/home/john/projects/pseudo-linus/target/agents/<seu-nome>`. Disco é compartilhado:
  sem `--release` à toa, e `cargo clean` no seu target se passar de ~5 GiB.

## Coordenação

- O coordenador é a sessão principal (`main`). Agentes mandam mensagem pro `main` pra: pedir mudança no
  `sysabi`, avisar que uma entrega intermediária ficou pronta, reportar bloqueio, ou pedir algo de outro
  agente. O coordenador repassa.
- Commits: só o coordenador commita. Agentes deixam o trabalho no disco, com testes verdes.
- Progresso: cada agente mantém `crates/<crate>/STATUS.md` curto (o que funciona, o que falta, placar
  de conformidade).
