# E06: isolamento em três camadas

O design v2 troca a promessa do v1 ("`disallowed_methods` no crate `userland` garante o isolamento em
tempo de compilação") por três camadas: lint no nosso código, scanner de dependências e defesa em
profundidade em runtime (Landlock e seccomp por thread). Este experimento põe cada camada à prova.

```sh
cd testbench/experiments/e06-isolation
cargo run --release            # refaz tudo (cerca de 1 a 3 minutos) e grava results/e06-isolation.json
cargo run --release -- --quick # mesma coisa com a medição de overhead encurtada
cargo test --release           # testes do próprio experimento (sondas, parser, subprocesso)
```

## Hipóteses

| Id | Frase | Critério |
|---|---|---|
| H19 | `disallowed_methods` garante isolamento em tempo de compilação | Refutada se uma dependência que chama `std::fs` passa pelo clippy do crate que a usa. |
| H20 | `forbid(unsafe_code)` pega todo unsafe nosso, inclusive gerado por macro | Testa unsafe gerado por `macro_rules!` e por proc macro de outro crate, com controle positivo. |
| H21 | Landlock e seccomp por thread isolam o pseudo-processo sem afetar o resto do host | Thread restrita leva EACCES/EPERM, vizinha não, filha herda, diretório montado continua acessível; overhead por syscall medido. |

## Método

Tudo é refeito pelo binário. As crates-sonda ficam em `probes/`, um workspace à parte (algumas existem
pra falhar a compilação), com target em `target/probes`. O binário chama `cargo clippy` e `cargo build`
com `--message-format=json` e lê os diagnósticos; o ambiente do cargo filho é limpo de `RUSTFLAGS`,
wrappers e `CLIPPY_CONF_DIR`.

### H19: clippy e depscan

- `probes/clippy.toml` proíbe `std::fs`, `std::process`, `std::net`, `std::env` e o stdio do host. O
  clippy não aceita curinga de módulo, então a lista enumera 41 funções em `disallowed-methods`, 13 tipos
  em `disallowed-types` e as 5 macros de impressão em `disallowed-macros`.
- `userland`: só chama `fs_reader::read_host_file` (sonda que faz `std::fs::read`) e `walkdir::WalkDir`
  (crates.io, usa `std::fs::read_dir`). As duas dependências ficam fora do workspace das sondas, como
  qualquer dependência de terceiros, e por isso nunca passam pelo clippy.
- `userland-direct`: um módulo por forma de tocar o host direto (controle), mais `handle_from_dep`, que
  recebe um `std::fs::File` de uma dependência e lê pelo trait `Read` sem escrever o nome do tipo.
- `cargo clippy --release -p <crate> -- -D warnings` em cada um; os diagnósticos são atribuídos ao
  módulo pelo arquivo do span.
- `depscan::scan` (biblioteca, por path) nos dois crates.

### H20: forbid(unsafe_code) e macros

`forbid-consumer` tem `#![forbid(unsafe_code)]` e `[lints.rust] unsafe_code = "forbid"`, e nenhum
`unsafe` escrito à mão. Cada caso é uma feature que liga um módulo que só invoca uma macro de outra crate;
o binário compila um caso por vez (`cargo build --release --features <caso>`) e classifica pelo código
do diagnóstico. Toda macro gera uma operação que exige `unsafe` de verdade (`from_utf8_unchecked`,
`unsafe impl Send` pra tipo com ponteiro cru, `#[unsafe(no_mangle)]`), então nenhum caso compila "por
acaso". As crates-sonda de macro (`unsafe-macros`, `unsafe-proc-macro`) também têm `forbid`: o corpo de
uma macro só vira código em quem a expande.

O controle positivo não usa unsafe escrito à mão (regra do projeto): é uma proc macro que gera o token
`unsafe` com o span da entrada, o que para o compilador equivale a o usuário ter escrito o `unsafe` ali.

### H21: Landlock e seccomp por thread

Nada é aplicado no processo principal. O binário roda `<exe> child <modo>` e lê um JSON do stdout;
dentro do subprocesso, só threads específicas recebem restrição. Arquivos de teste ficam em
`testbench/scratch/e06`; fora dele, o experimento só lê (`/etc/os-release`, `/`, `/proc/self/status`).

- **Landlock** (`landlock` 0.4.7): ruleset que trata todos os acessos de FS da ABI v6 e libera tudo
  embaixo de `scratch/e06/landlock/mounted` (o "diretório montado"). Sondas na thread restrita, numa filha
  criada por ela, numa thread de pool criada antes da restrição, numa vizinha criada depois e na thread
  principal. A ABI vem do `RestrictionStatus` (o crate lê `LANDLOCK_CREATE_RULESET_VERSION`). O TSYNC é
  sondado só no builder (`all_threads(true)` com `HardRequirement`), sem aplicar nada.
- **seccomp** (`seccompiler` 0.5.0): dois filtros por thread, porque o crate tem uma ação por filtro.
  1. EPERM pra `socket`, `connect`, `execve`, `execveat`, `fork`, `vfork`, `clone` sem `CLONE_THREAD`
     e `io_uring_setup/enter/register`, mais as variantes x32 dessas syscalls.
  2. ENOSYS pra `clone3`. Os argumentos do `clone3` ficam numa struct na memória, que o BPF não lê; o
     ENOSYS faz a glibc cair no `clone`, onde o primeiro filtro olha o `CLONE_THREAD`.

  O io_uring entra na lista porque `IORING_OP_SOCKET/CONNECT/OPENAT` não passam pelo seccomp, e no host
  `io_uring_disabled = 0`. O `execve` testado aponta pra um caminho inexistente: com filtro volta EPERM,
  sem filtro chega ao kernel e volta ENOENT, o que separa as duas coisas sem risco de trocar a imagem do
  processo. O caminho `clone3 -> clone` é conferido rodando o modo `seccomp-spawn` sob `strace -f`.
- **Overhead**: cinco configurações em threads separadas, ordem girada a cada rodada, 15 rodadas:
  - `getppid` × 1M;
  - `pread` de 1 byte × 500k (offset 0, sempre uma syscall);
  - `openat`+`close` × 200k de um arquivo dentro do diretório montado (caminho absoluto, 11 componentes);
  - criar thread, aplicar a configuração e dar join, × 400.

  A quinta configuração soma ao perfil seccomp um filtro com condição sobre argumento nas syscalls do
  laço, que impede o cache de ação constante do kernel (5.11+) de pular o BPF. Mede-se também o custo de
  montar um ruleset e o de criar thread a partir de uma thread já restrita.

## Candidatos

| Crate | Versão | Papel | depscan | Encaixe |
|---|---|---|---|---|
| `landlock` | 0.4.7 | FS do host por thread | (b), 17 unsafe, 34 pontos de host | serve |
| `seccompiler` | 0.5.0 | syscalls por thread | (b), 2 unsafe, 17 pontos de host | serve |

As duas têm API segura (o unsafe é interno, nas syscalls). Categoria (b) é esperada: são justamente as
crates que falam com o kernel do host, e rodam do lado do kernel do pseudo-linus, não no userland.

## Resultado

Rodada de 2026-10-02, Debian 13, kernel 6.12.101, Ryzen 7 5700 (16 threads), Rust 1.98.1, máquina
compartilhada com outros builds.

### H19

| Crate | `cargo clippy -D warnings` | Diagnósticos `disallowed_*` |
|---|---|---|
| `userland` (só dependências) | exit 0 | 0 |
| `userland-direct` | exit 101 | 10 |

| Módulo de `userland-direct` | Pego por |
|---|---|
| `direct_fs` (`std::fs::read`) | `disallowed_methods` |
| `direct_process` (`Command`, `exit`) | `disallowed_methods`, `disallowed_types` |
| `direct_net` (`TcpStream::connect`) | `disallowed_methods`, `disallowed_types` |
| `direct_env` (`std::env::var`) | `disallowed_methods` |
| `direct_stdout` (`std::io::stdout`) | `disallowed_methods` |
| `direct_print_macro` (`println!`) | `disallowed_macros` |
| `file_type_path` (`File::open`) | `disallowed_types` |
| `fn_pointer` (`std::fs::read` como valor) | `disallowed_methods` |
| `handle_from_dep` (`File` vindo de dependência, lido por `Read`) | **nada** |

O depscan acerta onde o clippy erra. No `userland`, 0 pontos de host no crate e a árvore em categoria (b),
com `fs-reader` (3), `walkdir` (15), `same-file` (13) e `winapi-util` (8). No `userland-direct`, 9 pontos
no próprio crate. O `winapi-util` só existe no Windows: o depscan conta código atrás de `cfg` de outra
plataforma (limitação já documentada nele), o que aqui só superestima.

### H20

| Caso | Resultado |
|---|---|
| (a) `macro_rules!` de outra crate gerando bloco `unsafe` | **compila** |
| (b) proc macro, span `call_site` | **compila** |
| (c) proc macro, span `mixed_site` | **compila** |
| proc macro, contexto `call_site` com linha e coluna da entrada (`located_at`) | **compila** |
| (d) `macro_rules!` gerando `unsafe impl Send` pra tipo com ponteiro cru | **compila** |
| (d) `#[derive(Pod, Zeroable)]` do bytemuck (`unsafe impl` por proc macro real) | **compila** |
| `macro_rules!` gerando `#[unsafe(no_mangle)]` | **compila** |
| (d) `intrusive_collections::intrusive_adapter!` | E0453 em `adapter.rs:203` |
| `macro_rules!` gerando `#[allow(unsafe_code)]` + bloco `unsafe` | E0453 |
| controle: proc macro com o span da entrada | lint `unsafe_code` |

O lint `unsafe_code` não disparou em nenhum token que vem de macro de outra crate. O único caso pego é
o controle, em que o token `unsafe` tem o contexto de sintaxe do próprio consumidor; o caso `located_at`,
com linha e coluna do consumidor mas contexto da expansão, passa. Isso bate com a regra do rustc de
silenciar lint cujo span está em expansão de macro externa (só lints marcados com
`report_in_external_macro` escapam dela), e o `unsafe_code` não tem essa marca. Os dois casos barrados não foram detectados como unsafe: caíram no
E0453 porque a macro emitia `#[allow(unsafe_code)]` e o `forbid` não deixa rebaixar. O
`intrusive_adapter!` real faz exatamente isso no `unsafe impl Adapter`; sob `deny` em vez de `forbid` ele
compilaria, e os seus `unsafe impl Send/Sync` sem `allow` passariam de qualquer jeito.

O depscan conta 0 unsafe no consumidor e acha o unsafe nas crates de macro (tokens `unsafe` dentro de
`macro_rules!` e de `quote!`): `unsafe-macros` 4, `unsafe-proc-macro` 4, `bytemuck_derive` 7,
`intrusive-collections` 385, `bytemuck` 332, além de `syn`, `proc-macro2` e `unicode-ident`, que só
rodam em tempo de compilação. Uma proc macro que monte o `unsafe` a partir de string escaparia do depscan
também; a regra útil é tratar crate de proc macro e `macro_rules!` exportada com unsafe como suspeitas.

### H21: comportamento

Landlock: `fully_enforced`, `no_new_privs`, ABI efetiva **v6**. O TSYNC com requisito duro foi recusado
pelo crate ("unsupported syscall flag: AllThreads set to true"), coerente com a ABI v6.

| Thread | Dentro do montado | Fora do montado, `/etc/os-release`, `/` |
|---|---|---|
| restrita | ok (ler, criar, listar) | EACCES (ler, criar, listar) |
| filha da restrita | ok (ler, criar) | EACCES (ler) |
| pool criado antes da restrição | n/a | ok (leu a pedido da restrita) |
| vizinha criada depois | n/a | ok |
| principal | n/a | ok |

A restrita também leva EACCES em `/proc/self/status` e ao reabrir um fd por `/proc/self/fd/N`, mas lê
normalmente um fd aberto antes do `restrict_self`.

seccomp: filtro de 117 instruções BPF, mais 15 do `clone3`.

| Thread | socket | connect | execve, execveat | `Command::spawn` | `IoUring::new` | criar thread | ler arquivo |
|---|---|---|---|---|---|---|---|
| filtrada | EPERM | EPERM (socket novo e socket criado antes) | EPERM | EPERM | EPERM | ok | ok |
| filha da filtrada | EPERM | EPERM (socket novo) | EPERM | EPERM | EPERM | n/a | ok |
| vizinha criada depois | ok | ok (socket novo e socket criado antes) | ENOENT (chegou ao kernel) | ok | ok | n/a | ok |
| principal | ok | n/a | n/a | ok | n/a | n/a | n/a |

O strace confirma o mecanismo na thread filtrada. Pro `posix_spawn`: `clone3(... CLONE_VFORK ...) = -1
ENOSYS`, seguido de `clone(... CLONE_VM|CLONE_VFORK|SIGCHLD) = -1 EPERM`. Pra thread: `clone3` com
ENOSYS e depois `clone(... CLONE_THREAD ...)` com sucesso. Efeito colateral global: a glibc guarda o
ENOSYS num estático e, a partir daí, a thread principal (sem filtro) também cria threads por `clone`,
enquanto o `posix_spawn` dela continua usando `clone3`. É inócuo, mas é estado do processo que uma
thread filtrada muda.

### H21: overhead

Melhor de 15 rodadas. As medianas estão no JSON, mas nesta máquina compartilhada a mediana do
`open+close` sem filtro variou de 2981 a 3871 ns entre execuções completas do experimento; o mínimo é o
estimador estável e é aproximadamente aditivo entre as camadas (+353 e +85 contra +485 com as duas).

| Configuração | `getppid` | `pread` 1 B | `openat`+`close` | criar thread + aplicar + join |
|---|---|---|---|---|
| sem filtro | 126 ns | 468 ns | 2788 ns | 29,5 µs |
| Landlock | -3 ns | +0 ns | **+353 ns (+12,7%)** | +8 µs |
| seccomp | +29 ns | +37 ns | +85 ns (+3,1%) | **+97 µs** |
| Landlock + seccomp | +30 ns | +37 ns | +485 ns (+17,4%) | +95 µs |
| seccomp com BPF forçado no laço | +126 ns | +144 ns | +322 ns (+11,5%) | +114 µs |

- Landlock só custa no `open`, porque é ali que ele resolve o caminho contra as regras; `read` e
  syscalls fora do FS não pagam nada.
- seccomp custa cerca de 30 ns em toda syscall mesmo quando o cache de ação constante pula o BPF. A
  diferença pra configuração com BPF forçado (de 97 a 107 ns a mais por syscall) é o que esse cache
  economiza, e mostra por que o filtro não deve ter condição por argumento em syscall quente.
- Aplicar os dois filtros seccomp custa cerca de 95 µs por thread (trabalho do kernel ao instalar cada
  filtro), três vezes o custo de criar a thread. Mas uma thread criada por outra que já está restrita
  herda os dois domínios de graça: **28,9 µs**, contra 29,5 µs de uma thread sem restrição.
- Montar um ruleset com um diretório custa 6,6 µs (7,5 µs de mediana).
- O `getppid` de 126 ns sem filtro já é alto pra uma syscall vazia; é o custo das mitigações de CPU
  deste host, igual em todas as configurações.

## Veredito

- **H19: refutada.** O `cargo clippy -D warnings` passa limpo (exit 0, zero diagnósticos) num crate que
  lê o host inteiro por duas dependências, e também deixa passar I/O direto feito num `File` que chegou
  por dependência. Com a lista completa ele pega as 8 formas de chamada direta testadas, inclusive
  `println!` (só via `disallowed-macros`) e função usada como valor. O depscan pega as dependências.
- **H20: refutada.** 7 de 9 formas de `unsafe` gerado por macro de outra crate compilam sob
  `forbid(unsafe_code)`, inclusive `unsafe impl Send` e `#[unsafe(no_mangle)]`; o lint só dispara quando
  o token `unsafe` tem o contexto de sintaxe do próprio crate. O `forbid` ainda vale contra unsafe escrito
  à mão e contra `allow(unsafe_code)` emitido por macro (E0453).
- **H21: confirmada.** Landlock (ABI v6) e seccomp aplicados por thread restringem só a thread e as
  filhas dela; vizinhas, a thread principal e threads que já existiam seguem livres, e o diretório
  montado continua acessível. Overhead: +353 ns por `open` com Landlock, cerca de 30 ns por syscall com
  seccomp, e custo zero de aplicação quando a thread nasce de uma thread já restrita.

### Recomendação de camadas

1. **Lint no nosso código**, como higiene, não como garantia: `unsafe_code = "forbid"` no workspace e o
   `clippy.toml` desta pasta (com `disallowed-macros`, sem o qual `println!` passa) nos crates de
   userland.
2. **depscan como porteiro de dependência**, porque é a única camada estática que enxerga I/O de host e
   unsafe dentro de dependências e de macros. Dependência nova de userland com I/O de host, `unsafe`
   em corpo de `macro_rules!` exportada ou crate de proc macro que gere `unsafe` reprova até revisão.
3. **Runtime por thread, com uma thread "spawner" por sandbox**: ela aplica Landlock (montagens da
   sandbox) e seccomp uma vez, e cria as threads dos pseudo-processos, que herdam tudo de graça. Regras
   que isso impõe ao kernel do pseudo-linus:
   - nenhum trabalho de pseudo-processo vai pra pool criado fora da sandbox (rayon, tokio globais): a
     thread do pool não é restrita e faz I/O de host a pedido;
   - rede (ureq do F12) roda numa thread do kernel fora do filtro, ou o seccomp troca a negação de
     `socket`/`connect` por regras de rede do Landlock (ABI v4+);
   - montagem nova depois que a sandbox nasceu não estende o domínio já aplicado (Landlock é
     irreversível por thread): ela passa por fd aberto pelo kernel, que o Landlock não revoga;
   - `/proc` do host some pra thread restrita (EACCES), o que é desejável, mas qualquer código que leia
     `/proc/self` precisa rodar do lado do kernel;
   - o filtro seccomp é lista de negação e precisa cobrir io_uring e as variantes x32, como aqui.

Limite das três camadas: as threads dividem o espaço de endereçamento, então isso protege contra código
seguro que tenta fazer I/O de host (o buraco do H19), não contra corrupção de memória numa dependência
com unsafe. Contra esta, a camada é o depscan e a escolha de dependências.
