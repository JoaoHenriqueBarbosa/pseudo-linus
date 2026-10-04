# sysio: status

Dono: agente coreutils.

## Funciona

- `fs`, `io`, `env`, `process`, `time`, `thread`, `os::unix::fs`, `os::fd`, `os::unix::process`,
  `path::PathExt`, `unistd`, `users`, `random`, `errno` e as macros de impressão, todos sobre
  `sysabi::sys` (ver README).
- `run`: estado de userland por processo, stdout com buffer no estilo da glibc, descarga no fim e no
  `exit`, `write error` no fim como o `close_stdout`.
- 8 testes de integração no testkit (`cargo test -p sysio`).
- Threads pelo kernel (`spawn_thread`/`join_thread`); CPUs por `sched_getaffinity`.

## Pendências

- **poll**: sem `poll` no contrato; leitura de vários pipes usa thread.
- Ainda não validado sobre o kernel real (não existe).
