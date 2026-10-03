# E05: verdade de campo do Linux real

## Hipóteses

- **H14** (v1): as constantes do Linux usadas no design (pipe, PIPE_BUF, ELOOP, PATH_MAX, NAME_MAX,
  pid_max, fatia do escalonador) batem com o sistema real.
- **H15** (v1): errno e strerror da glibc podem ser reproduzidos byte a byte.

## Método

- **No host** (kernel 6.12.101, glibc 2.41): capacidade do pipe com `fcntl(F_GETPIPE_SZ)`;
  atomicidade de escrita (4 escritores em paralelo, blocos de 4096 bytes e de 256 KiB, e o leitor confere
  se algum bloco saiu misturado); cadeias de 40 e 41 symlinks; nomes de 255 e 256 bytes; escrita em
  `/dev/full`; `pid_max`; `sysctl_sched_base_slice` lido do `fair.c` da 6.12.101 (árvore stable, baixado
  e guardado em cache no scratch).
- **Tabelas da glibc**: `strerror` via `std::io::Error` (que chama `strerror_r`) comparado com a tabela
  `errno.errorcode` + `os.strerror` do python3 do host, pros errnos 1 a 133; sinais com nome e descrição
  (`strsignal`).
- **No oráculo** (Debian 13, `src/probe.sh`): `getconf`, `pid_max`, umask, `ulimit -a`, `kill -l`,
  códigos de saída do bash (SIGTERM, comando inexistente, sem permissão, SIGPIPE), mensagens de erro reais
  de cat, mkdir, cd, rmdir, ls, rm, mv, cp, ln e `echo > /dev/full`, root diante de modo 000, formato de
  data do `ls -l` (regra dos 6 meses), e amostras de `/proc`.

Saídas consumíveis pelo kernel: `golden/linux-facts/linux_facts.json` (errno com nome e mensagem, sinais,
limites, códigos de saída do bash, mensagens reais) e `golden/linux-facts/proc/*.txt`.

## Resultado

| Constante | v1 | Real |
|---|---|---|
| capacidade padrão do pipe | 65536 | 65536 |
| escrita de 4096 bytes atômica | sim | sim (0 de 16000 blocos misturados) |
| escrita de 256 KiB atômica | (não afirmado) | não (quase todos os blocos misturados) |
| PATH_MAX / NAME_MAX / PIPE_BUF | 4096 / 255 / 4096 | 4096 / 255 / 4096 |
| 40 symlinks resolvem, 41 dão ELOOP | sim | sim |
| /dev/full dá ENOSPC | sim | sim |
| **pid_max** | **32768** | **4194304** (o default do kernel com 16 CPUs é 32768, mas o systemd do Debian sobe pra 4194304; não é por namespace, o container vê o mesmo) |
| **fatia base do EEVDF** | **0,75 ms** | **0,70 ms** na 6.12.101 (backport), × 4 com 16 CPUs = 2,8 ms |
| exit do bash: SIGTERM, não encontrado, sem permissão, SIGPIPE | 143, 127, 126, 141 | 143, 127, 126, 141 |

- **strerror**: 130 de 130 errnos batem entre o `strerror_r` e a tabela da glibc. 14 de 14 mensagens
  reais de ferramentas terminam exatamente em `": " + strerror(errno)`.
- **Aspas**: em C.UTF-8, o `mkdir` escreve `‘d’` (aspas curvas, função `quote()`), enquanto rm, mv, cp,
  ln e ls escrevem `'d'` (função `quoteaf()`). Quem decide o estilo é a função usada por cada utilitário,
  e o porte precisa manter a mesma.
- **Root ignora permissões**: `cat` de arquivo com modo 000 sai com 0, e escrever num diretório com modo
  000 também. O VFS precisa reproduzir CAP_DAC_OVERRIDE pro root.

## Veredito

- **H14: parcial.** 14 de 16 constantes batem. `pid_max` e a fatia base estavam errados no v1, e o v2 já
  usa os valores medidos.
- **H15: confirmada.** A tabela errno → nome → mensagem sai da glibc real e bate byte a byte com o que
  as ferramentas imprimem. O kernel do pseudo-linus vai gerar `strerror` a partir de
  `golden/linux-facts/linux_facts.json`, em vez de uma tabela digitada.
