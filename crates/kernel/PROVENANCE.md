# Procedência do código: crate `kernel`

Inventário pedido pelo coordenador. Nenhum arquivo deste crate tem texto copiado do Linux nem da glibc, e
nenhum foi traduzido linha a linha. Mas também não é sala limpa: quem escreveu (o agente kernel) conhece o
código do Linux, e em alguns pontos a ordem das checagens e a divisão em funções seguem a do kernel de
memória. Os comentários citam os nomes das funções do kernel como referência de comportamento. Abaixo,
arquivo por arquivo, de onde veio cada coisa e quão perto do código do kernel ele fica.

Legenda: **original** = desenho próprio a partir do design v2, dos experimentos da bancada e das man pages;
**comportamento** = reproduz comportamento documentado (man pages) e medido (bancada, host, oráculo), com a
estrutura escrita por nós; **perto do kernel** = o algoritmo segue o do kernel passo a passo, reconstruído de
memória.

| Arquivo | Classe | Fontes |
|---|---|---|
| `src/lib.rs`, `src/config.rs`, `src/kernel.rs` | original | design v2, E01 |
| `src/park.rs` | original | E01 (modelo A) |
| `src/proc.rs` | comportamento | wait(2), credentials(7), proc(5) (pid_max medido no E05); rlimits padrão do `/proc/self/limits` do oráculo |
| `src/spawn.rs` | comportamento | fork(2), posix_spawn(3), execve(2), pthread semantics; desenho do modelo A (E01) |
| `src/signal.rs` | comportamento | signal(7), sigaction(2), kill(2): ignorado descartado na geração, SIGCONT retoma, SIGKILL primeiro; os nomes `prepare_signal`/`get_signal` nos comentários são só referência |
| `src/sys.rs` | comportamento | man pages de cada syscall; `uname` do host (6.12.101); EINTR conforme signal(7) |
| `src/fd.rs` | original | open(2), dup(2), fcntl(2) |
| `src/pipe.rs` | comportamento, encontro de FIFO perto do kernel | pipe(7), fifo(7) e E05 (65536, PIPE_BUF 4096); o encontro de leitor e escritor com contadores e a exceção do pipe anônimo reaberto (`is_pipe`) seguem o `fifo_open` de `fs/pipe.c` de memória |
| `src/dev.rs` | comportamento | mem(4), random(4), tty(4) |
| `src/exec.rs` | `parse_shebang` perto do kernel; resto comportamento | a análise do `#!` segue o `load_script` de `fs/binfmt_script.c` passo a passo (fim de linha, corte de espaços, terminador, regra do nome truncado); E2BIG pelos limites de execve(2); cabeçalho ELF64 pelo elf(5) |
| `src/procinfo.rs`, `src/sandbox.rs`, `src/hostio.rs`, `src/image.rs` | original | API pedida pelo coordenador e pelo host |
| `tests/integration.rs` | original | |

## Dados de terceiros (`image/`)

Copiados do container Debian 13 do oráculo (`docker cp`), sem alteração exceto `ID_LIKE=debian` no
os-release (pedido do dono):

| Arquivo | Pacote Debian | Licença do pacote |
|---|---|---|
| `usr/lib/os-release`, `etc/debian_version`, `etc/issue`, `etc/issue.net`, `etc/host.conf` | base-files | GPL-2+ |
| `root/.bashrc`, `root/.profile` (cópias de `/usr/share/base-files/dot.*`), `etc/profile`, `etc/motd`, `etc/fstab`, `etc/shells` (gerados na instalação pelo base-files e pelo debianutils) | base-files | GPL-2+ |
| `etc/passwd`, `etc/group`, `etc/shadow`, `etc/gshadow` (gerados do `passwd.master`/`group.master`) | base-passwd | GPL-2 / domínio público |
| `etc/bash.bashrc`, `etc/skel/.bashrc`, `etc/skel/.profile`, `etc/skel/.bash_logout` | bash | GPL-3+ |
| `etc/nsswitch.conf`, `etc/environment` | gerados na instalação (libc-bin, pam) | LGPL-2.1+ (libc-bin) |

Os arquivos do bash (GPL-3+) são texto embutido no binário via `include_bytes!`; se a licença final do
binário não puder conter GPL-3, eles saem da imagem (ou viram conteúdo nosso equivalente).

## Recomendação

O crate fica MIT por enquanto (decisão do dono). Se o dono quiser sala limpa estrita, os pontos a reescrever
a partir só de man pages e testes são `exec.rs::parse_shebang` e o encontro de FIFO em `pipe.rs`; os testes
de integração e o diferencial do VFS contra o host servem de especificação.
