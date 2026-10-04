# host: estado

Dono: agente host. Daemon multiusuário (`pseudo-linusd`), CLI `osh` e imagem de deploy.

## Funciona (testado)

`cargo test -p host --features fake-backend`: 44 testes unitários e 11 de integração (o binário de
verdade, com supervisor e workers em processos separados, falando HTTP).

- **Autenticação** (`auth.rs`): usuários com papel (`admin`/`user`) e estado (ativo/desativado),
  chaves `plk_<id>_<segredo>` (256 bits), só o SHA-256 no disco, comparação em tempo constante,
  expiração, revogação, último uso gravado em lote. `auth.json` 0600 com escrita atômica e `flock`:
  o comando de admin e o daemon escrevem ao mesmo tempo sem perder atualização, e o daemon enxerga a
  mudança na requisição seguinte. 429 por IP depois de 20 falhas em 60 s.
- **Supervisor e workers** (`supervisor.rs`, `worker.rs`, `ipc.rs`): workers são processos filhos
  (quadros JSON por stdin/stdout, fd de protocolo separado do stdout); usuário fixo num worker enquanto
  tiver sandbox; ping de saúde; worker que cai ou trava (SIGSTOP no teste) é morto e reiniciado com
  backoff; chamadas em andamento recebem `worker_crashed` com o sinal; sandboxes viram `lost` com o
  motivo, ou voltam do último snapshot persistido quando o worker sobe; sessões viram `session_closed`.
- **Quotas e admissão**: por usuário (sandboxes, memória, processos, execs simultâneos, sessões,
  timeout máximo, saída máxima, snapshots persistidos, peso e teto de CPU) e do serviço (sandboxes,
  memória, execs, conexões, corpo HTTP).
- **Métodos**: `whoami`, `sandbox.create/destroy/list/info`, `exec`, `exec.stream`,
  `session.open/exec/exec.stream/close/list`, `fs.read/write/list/stat/mkdir/remove`,
  `snapshot` (com `persist`), `snapshot.delete`, `restore`, `ps`, `kill`, `export`, `import`,
  `admin.users.list/create/update/remove`, `admin.keys.create/list/revoke`, `admin.workers`.
- **Transporte** (`server.rs`): `POST /rpc` (lote, NDJSON nos métodos de streaming), `GET /ws`
  (JSON-RPC multiplexado com notificações `exec.output`), `GET /healthz`; 401/413/429 com erro JSON-RPC;
  cliente que desconecta cancela a execução.
- **exec** (`exec.rs`): timeout de parede mata o grupo e a sessão do comando; limite de saída com
  truncamento, descarte até um teto e então SIGPIPE; escoamento de processo em segundo plano;
  cancelamento.
- **Sessões** (`session.rs`): `bash` persistente com laço de sentinela (`.` de um arquivo por
  comando); timeout reinicia o shell com cwd e variáveis exportadas.
- **tar** (`tarball.rs`): export/import GNU com nomes longos, hardlinks, symlinks, dono, modo e mtime;
  recusa `..`; recuperação limpa a raiz e importa (o que foi apagado continua apagado).
- **Isolamento** (`isolation.rs`): Landlock (ABI v6 pedida, best effort; FS, TCP e scope) e seccomp
  (rede, exec, fork, io_uring, sinais pra fora, ptrace, montagem, módulos, relógio, x32) por thread.
  `pseudo-linusd selftest` confere no host onde roda.
- **Allocator**: `#[global_allocator]` `tracking_allocator::Allocator<MiMalloc>` nos dois binários.
- **osh**: `-c`, script com argumentos, interativo linha a linha (sessão persistente), `--remote` com
  `--key-file`/`OSH_KEY`/`--key`; modo local em processo pelo mesmo trait `Backend`.
- **Imagem**: `Dockerfile` multi-stage (rust 1.98.1 slim trixie com libsqlite3-dev; runtime
  debian trixie-slim com libsqlite3-0, usuário 10001, HEALTHCHECK pelo próprio binário),
  `Dockerfile.dockerignore`, `deploy/config.toml` da VPS, `deploy/docker-compose.yml`, `DEPLOY.md`.

## Falta

- **Backend do kernel**: o adaptador do trait `Backend` pro `crates/kernel` espera o `API.md`. Hoje o
  único backend é o dublê (`fake.rs`, só com a feature `fake-backend` e `PL_ALLOW_FAKE_BACKEND=1`);
  a imagem de produção sobe, mas os workers recusam o backend `kernel` até a integração.
- **Do kernel, pedido ao `main`**: grupos de CPU por usuário (`cpu.weight` e `cpu.max`) na criação
  da sandbox e atualização ao vivo; confirmação de que `spawn` com sessão nova dá pgid = sid = pid;
  `kernel::mem::install_tracker()` pro allocator rastreado.
- **userland**: testes do daemon com os programas reais (`crates/userland`) quando existir; o
  protocolo de sessão depende de `.`, `read -r`, `printf` e `env -0` do bash e do coreutils.
- Testes de integração com o kernel real: os mesmos 11 cenários, mais memória (teto da sandbox) e
  laço sem checkpoint (watchdog).
- `exec` interativo com stdin em streaming pelo WebSocket (hoje o stdin vai inteiro na chamada).
