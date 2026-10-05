# ul-procps: estado

Ferramentas portadas do procps-ng 4.0.4 e do psmisc 23.7 do Debian 13. Os dados vêm do `/proc`, lido
como a libproc2 lê; a ausência de um arquivo vira o mesmo valor padrão do original.

## O que existe

| Ferramenta | Origem | Módulo |
| --- | --- | --- |
| `free`, `kill`, `pgrep`, `pkill`, `pidwait`, `uptime` | procps-ng 4.0.4 | `free.rs`, `kill.rs`, `pgrep.rs`, `uptime.rs` |
| `pidof` | sysvinit-utils 3.14 | `pidof.rs` |
| `ps` | procps-ng 4.0.4 | `ps/` (parser, sortformat, display, output, proc, table, help) |
| `top` (modo batch) | procps-ng 4.0.4 | `top/` (args, data, fields, frame, text) |
| `watch` | procps-ng 4.0.4 | `watch.rs` |
| `killall` | psmisc 23.7 | `killall.rs` |

Casos de conformidade novos em `testbench/corpus/cases/procps/` (`ps.toml`, `top.toml`, `watch.toml`,
`killall.toml`). Os arquivos golden não foram gerados: isso é do coordenador, pelo harness do oráculo.

## ps

Porte módulo a módulo de `parser.c`, `sortformat.c`, `display.c`, `select.c`, `output.c` e `help.c`:
opções SysV, BSD e GNU longas, `-o` e `-O` (275 colunas, tabela gerada de `global.c`), formatos
predefinidos (`aux`, `-ef`, `-l`, `u`, `j`...), `--sort`, `-C`, `-p`, `-u`, `--no-headers`, floresta
(`f`, `-H`), threads (`-L`, `-T`, `H`, `m`), `PS_PERSONALITY` e `PS_FORMAT`, e todas as mensagens de
erro e de uso. Diferenças conhecidas: as colunas do systemd (`unit`, `slice`, `lsession`...) mostram
`-`, e `numa` mostra `-1`.

## top

Só o modo batch (`-b`). Opções: `-b -c -d -E -e -H -i -n -O -o -p -S -s -U -u -V -w -1 -h`, com o
`getopt_long` da glibc e o remendo `GETOPTFIX` do próprio top (a palavra seguinte vira argumento de
qualquer opção). O resumo é fiel ao `summary_show`: a linha de carga, as tarefas, as CPUs e a
memória, incluindo o par de linhas que o `double_up` padrão faz com `-1` (a segunda CPU e a linha de
swap somem cortadas na largura da tela, como no original). `%CPU` usa o histórico de ticks entre duas
leituras (a primeira passa por duas leituras com 100 ms de pausa, como o `frame_make`).

Diferenças conhecidas e itens em aberto:

- Sem `-b` o top não funciona: o original precisa de um terminal e o sandbox não tem. Com `TERM`
  ausente ou sem terminfo sai a mensagem do ncurses, sem terminal no stdin sai `failed tty get`, e com
  terminal sai `interactive mode is not available, use -b`.
- Os arquivos `~/.toprc`, `~/.config/procps/toprc` e `/etc/toprc` não são lidos (campos, janelas e
  cores guardados neles não valem). Sem esses arquivos o comportamento é o do oráculo.
- Os sinais (`SIGINT`, `SIGTERM`, `SIGWINCH`) não têm o tratamento do original, que imprime uma quebra
  de linha final ao terminar por sinal.
- `fatal_proc_unmounted` (o aviso de `/proc` não montado) não é verificado.
- A coluna `NU` (nó NUMA) vale `-1`, como o procps sem libnuma.
- Só os 12 campos padrão são mostrados (`PID USER PR NI VIRT RES SHR S %CPU %MEM TIME+ COMMAND`); os
  outros 66 existem para `-o` (ordenação), com o mesmo valor que a libproc2 daria.

## watch

Opções `-b -c -C -d -e -g -q -n -p -r -t -w -x -h -v` e o cabeçalho; sem terminal (`TERM` ausente ou
sem terminfo) falha com `Error opening terminal: unknown.` e saída 1. Com `TERM=dumb` o desenho é o do
ncurses: o primeiro quadro escreve largura por altura espaços e os seguintes só as células que mudam.

## killall

`-e -I -g -y -o -i -r -s -u -v -w -n -Z`, o `getopt_long_only` da glibc escrito à mão (permutação,
ambiguidade silenciosa e os remendos `-ve`, `-INT`, `-9`) e as mensagens de `no process found`.
Divergência conhecida: o original usa `pidfd` para esperar (`-w`) e conferir o processo antes do sinal,
o que o sandbox não oferece; aqui a espera é por consulta ao `/proc`.

## Não verificado

Nada deste crate foi compilado nem testado: o código foi conferido só por leitura contra o fonte do
original e contra as saídas medidas no oráculo (`pseudo-linus-oracle:719900900623`). Os testes em
`crates/ul-procps/tests/` cobrem `free`, `kill`, `pgrep`, `pidof` e `uptime`; `ps`, `top`, `watch` e
`killall` ainda não têm teste de fixture (as fixtures `basic*.expected.json` já trazem as saídas reais
do `ps` e do `top` para quem for ligá-los).
