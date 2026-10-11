# Fronteira do zjsc com a máquina (inventário de 2026-10-11)

REGRA SUPREMA do projeto: dentro do pseudo-linus o `bun` lê o sistema de arquivos, o ambiente, os relógios, os
processos e a saída DO SANDBOX, nunca os do host. O zjsc tinha 16 arquivos que tocavam a máquina por `std`. Este
documento lista cada ponto e o destino. A fronteira é o trait `crate::runtime::host::Host`, instalado por thread
(`host::install`), como o `ConsoleHost` e o `ModuleFs` já eram instalados pelo hospedeiro.

## Estado (feito em 2026-10-11)

- `src/runtime/host.rs`: o trait `Host` (read_file, stat, create_file, environ/getenv, cwd, pid, cpus, random_fill, kill,
  set_priority, write_stderr, exit), `install`/`uninstall`/`host()` por thread e o `Detached` (máquina vazia) quando
  ninguém instalou nada. Os relógios e o sono ficam em `ul_common::time` (`realtime_nanos`, `monotonic_nanos`, `sleep`),
  que já respondem com o relógio do pseudo-processo.
- Os 16 arquivos da tabela abaixo usam a fronteira; `tests/host_boundary.rs` varre o `src/` sem exceção e falha se
  `std::fs`, `std::net`, `std::env`, `std::process`, `Command`, `Instant::now`, `SystemTime::now`, `thread::sleep`,
  `available_parallelism`, `eprintln!` e afins voltarem.
- O host que lê a máquina real existe só em `tests/common/std_host.rs` (e é incluído por `#[path]` nos testes de unidade);
  `process_bun_golden` o instala porque confere `kill`, `pid` e `/proc` contra o bun desta máquina.
- `crates/ul-bun`: o `SysabiHost` (sobre o `sysio`/`sysabi`), o `FdConsole` (fds 0, 1, 2), o `SandboxFs` (VFS do processo)
  e o programa `bun` (`-e`, `run`, `ARQUIVO`, `--version`). Testes no kernel de teste do sysabi.

Medido por `grep -rnE "std::(fs|net|env|process|thread::sleep|time)|Instant|SystemTime|eprintln!|available_parallelism"`.

## Pontos de acesso e destino

| Arquivo | Função | O que toca | Destino |
|---|---|---|---|
| `runtime/process_shape.rs` | `START`, `elapsed_since_start` | `Instant::now` (base de `uptime`/`hrtime`) | `Host::monotonic_nanos` |
| `runtime/process_shape.rs` | `owner_ids` | `fs::metadata("/proc/self")` para uid e gid | `Host::stat` |
| `runtime/process_shape.rs` | criação do `process` | `std::process::id()` | `Host::pid` |
| `runtime/process_system.rs` | `current_directory` | `env::current_dir` | `Host::cwd` |
| `runtime/process_system.rs` | `chdir_body` | `fs::metadata` do destino | `Host::stat` (errno cru) |
| `runtime/process_system.rs` | `kill` (vivo? entrega) | `Path::exists("/proc/N")`, `std::process::id`, `Command("kill")` | `Host::stat`, `Host::pid`, `Host::kill` |
| `runtime/process_env.rs` | `current_environment` | `env::vars_os` | `Host::environ` |
| `runtime/process_stats.rs` | `stat_field`, `status_value`, `io_value`, `statm_pages` | `fs::read_to_string("/proc/self/...")` | `Host::read_file` |
| `runtime/process_warning.rs` | cabeçalho `(node:PID)` | `std::process::id()` | `Host::pid` |
| `runtime/node_os.rs` | `read_text` (uname, meminfo, cpu, rede, hostname) | `fs::read_to_string` em `/proc` e `/etc` | `Host::read_file` |
| `runtime/node_os.rs` | `set_priority_body` | `Path::exists("/proc/N")`, `process::id`, `Command("renice")` | `Host::stat`, `Host::pid`, `Host::set_priority` |
| `runtime/node_os.rs` | `fib_trie_addresses`, redes | `std::net::Ipv4Addr` | puro (`core::net::Ipv4Addr`, o mesmo tipo, sem acesso à rede) |
| `runtime/crypto.rs` | `random_bytes` | `File::open("/dev/urandom")` | `Host::random_fill` |
| `runtime/crypto_ec.rs` | `fill_random` | `File::open("/dev/urandom")` | `Host::random_fill` |
| `runtime/options.rs` | `kern_tcsm_aware_number_of_processor_cores` | `env::var`, `available_parallelism`, `eprintln!` | `Host::getenv`, `Host::cpus`, `Host::write_stderr` |
| `runtime/gc.rs` | `stress_threshold` | `env::var("ZJSC_GC_STRESS")` | `Host::getenv` |
| `runtime/function_overrides.rs` | `parse_overrides_in_file`, `exit(1)` | `File::open`, `process::exit` | `Host::read_file`, `Host::exit` |
| `runtime/code_cache.rs` | `time_at_last_prune` | `Instant::now` | `Host::monotonic_nanos` |
| `runtime/console_client.rs` | `console.time` | `Instant::now` | `Host::monotonic_nanos` |
| `runtime/timers.rs`, `runtime/bun_global.rs`, `runtime/atomics_object.rs` | espera do laço, `Bun.sleepSync`, `Atomics.wait` | `thread::sleep` | `Host::sleep` |
| `wtf/monotonic_time.rs` | `MonotonicTime::now` | `Instant::now` | `Host::monotonic_nanos` |
| `wtf/data_log.rs` | `setDataFile`, `dataLog` | `File::create`, `io::stderr` | `Host::create_file`, `Host::write_stderr` |
| `parser/source_provider.rs` | `dumpSource` e arquivo temporário | `fs::File::create`, `OpenOptions`, `env::temp_dir`, `process::id`, `SystemTime` | `Host::create_file`, `Host::getenv("TMPDIR")`, `Host::pid`, `date_math::current_time_in_nanoseconds` |
| `parser/parser_part3.rs`, `wasm/wasm_section_parser.rs` | diagnósticos do JSC | `eprintln!` | `Host::write_stderr` |

Já corretos (sem mudança): o relógio de parede (`wtf/date_math.rs` lê `ul_common::time::realtime_nanos`, que usa o
relógio do pseudo-processo), o console (`ConsoleHost`), o sistema de arquivos dos módulos (`ModuleFs`), o fuso
(`ul_common::time::zone`).

## Sem consumidor no zjsc hoje (a fronteira ganha o método quando o primeiro consumidor chegar)

Rede (`node:net`, `dgram`, `dns`), `child_process` e `Bun.spawn` não existem no zjsc: `grep -rn child_process src`
só acha o registro de nomes. Não há `TcpStream`, `UdpSocket` nem `Command::new` além de `kill` e `renice`. Método de
trait sem chamador seria superfície morta e sem teste, então spawn, wait, pipes, sockets e DNS entram no trait na
fatia que portar o módulo que os usa, sobre o `sysabi` (`spawn`, `wait4`, `pipe2`, `tcp_*`, `udp_*`).

## Puro (não é a máquina)

Dados do ICU/CLDR compilados, tzdata embutida do `jiff`, `std::thread::park` do `Atomics.wait` sem prazo, tabelas
de `Intl`. Nenhum lê o host.
