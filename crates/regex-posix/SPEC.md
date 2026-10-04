# Especificação comportamental: regex do GNU e grep

Comportamento medido no oráculo (Debian 13: glibc 2.41, grep 3.11, sed 4.9, `LC_ALL=C.UTF-8`), escrito
pra permitir reimplementação em sala limpa. Rede de segurança: `testbench/corpus/cases/regex` (682
regex, 1214 sondas), o corpus minerado do F01 (9291 sondas) e `testbench/corpus/cases/grep` (185).

## 1. Sintaxe

Controlada pelos bits `RE_*` do `regex.h` (interface pública). Regras observáveis:

- `\` seguido de: `|` alternação se `!LIMITED_OPS && !NO_BK_VBAR`; `1`..`9` referência se
  `!NO_BK_REFS`; `< > b B \` '` âncoras e `w W s S` classes se `!NO_GNU_OPS`; `( )` grupo se
  `!NO_BK_PARENS`; `+ ?` operadores se `BK_PLUS_QM && !LIMITED_OPS`; `{ }` intervalo se
  `INTERVALS && !NO_BK_BRACES`; qualquer outro: o caractere literal.
- Sem barra: newline alterna com `NEWLINE_ALT`; `|` com `NO_BK_VBAR`; `*` sempre operador; `+ ?`
  sem `BK_PLUS_QM`; `{ }` com `INTERVALS && NO_BK_BRACES`; `( )` com `NO_BK_PARENS`.
- `^` é âncora no começo do padrão, depois de abrir grupo, depois de alternação, depois de newline
  com `NEWLINE_ALT`, ou sempre com `CONTEXT_INDEP_ANCHORS`; senão literal. `$` é âncora no fim, antes
  de fechar grupo ou de alternação, ou sempre com `CONTEXT_INDEP_ANCHORS`.
- Âncora não aceita repetição: `^*` é `^` seguido do que `*` for naquele contexto.
- Operador de repetição no começo de expressão: erro "Invalid preceding regular expression" com
  `CONTEXT_INVALID_OPS`; ignorado com `CONTEXT_INDEP_OPS` (`*a` = `a` no egrep); senão literal.
  `\{` no começo com `CONTEXT_INVALID_DUP` (sed BRE): erro. No sed BRE, `**` é erro.
- `{n,m}`: `{,m}` = `{0,m}`; `{}` erro "Invalid content of \{\}"; `m<n` idem; maior que 32767:
  "Regular expression too big"; malformado: erro, ou literal com `INVALID_INTERVAL_ORD` (egrep:
  `a{1` casa o texto `a{1`). `{0}` remove o elemento.
- `)` sem par: literal com `UNMATCHED_RIGHT_PAREN_ORD` (egrep), senão "Unmatched ) or \)".
- Referência só a grupo já fechado no mesmo ramo da alternação: `(a)|\1` é "Invalid back
  reference".
- Colchetes: `]` logo depois de `[` ou `[^` é literal; `-` na ponta é literal; `---` é um hífen; `[`
  sozinho dá "Invalid regular expression"; não fechado: "Unmatched [, [^, [:, [., or [=". Faixa com
  ponta fora do ASCII: "Invalid collation character"; invertida: "Invalid range end". `[:nome:]`
  desconhecido: "Invalid character class name". `[=x=]` e `[.x.]` só com um caractere ASCII. Com
  `HAT_LISTS_NOT_NEWLINE`, `[^...]` não casa newline. `\` dentro de colchete é literal, salvo com
  `BACKSLASH_ESCAPE_IN_LISTS` (awk).
- `RE_ICASE`: o padrão (fora dos escapes e dos nomes de classe) vai pra maiúsculas antes de ser
  analisado (`[Z-a]` com `-i` é "Invalid range end"); `[:upper:]`/`[:lower:]` viram `[:alpha:]`; um
  caractere casa se a maiúscula dele casa. `\a` com `-i` (o `a` minúsculo escapado) não casa nada.

## 2. Semântica

- Casada: a mais à esquerda; entre as que começam ali, a mais longa (inclusive com referências).
- `.` e colchetes não casam byte inválido de UTF-8; um byte inválido no padrão casa ele mesmo.
- Classes em C.UTF-8: ASCII como no locale C; fora: `alpha` = Alphabetic mais dígitos não ASCII,
  `digit` só 0-9, `space` = espaços Unicode menos U+00A0, U+2007, U+202F, `punct` = gráfico que não é
  alnum. `\w` = `[_[:alnum:]]`.
- Submatches (difere do POSIX): entre os caminhos que produzem exatamente a casada escolhida, vale
  o primeiro numa ordem em que alternação prefere o ramo da esquerda (um primeiro ramo vazio perde
  pro segundo), repetição prefere mais uma volta, e `e{n,m}` são n cópias seguidas de `m-n` opcionais
  em que a primeira opcional a ser pulada é a mais à esquerda. Um grupo repetido fica com a última
  volta não vazia (`(a*)*` em `aa`: `aa`; em `b`: vazio em 0). Se algum caminho termina sem passar
  por âncora depois do último caractere, só esses valem (`(^)*` deixa o grupo de fora). Exemplos de
  referência: `(a|ab)(c|bcd)(d*)` em `abcd` = `a`,`bcd`,``; `(a|ab)(bc|c)?` em `ab` = `ab`, sem grupo
  2; `\(a*\)\(ab\)*\(b*\)` em `abab` = ``,`ab`,``; `(.*)(.*)` em `abc` = `abc`,``.
- `newline_anchor`: `^` também depois de `\n`, `$` também antes. `\`` e `\'` só nos extremos.
- Começo de busca no meio de um caractere UTF-8 avança pro próximo caractere.

## 3. Visão do dfa (só seleção de linhas do grep, sem referência)

- No ERE, repetição depois de âncora se aplica à âncora: `^*a` casa `a` em qualquer lugar (o `-o`
  continua usando a seção 2, que lê `^a`). Repetição no começo de expressão se aplica ao vazio.
- No grep -E, avisos `grep: warning: * at start of expression` (e `?`, `+`, `{...}`) quando o
  operador aparece no começo do padrão, depois de `(` ou `|`, ou depois só de âncoras desde então.
  O Debian não emite os avisos de barra solta.
- `[:alpha:]` fora de colchetes (corpo começa e termina com `:`, tem outro caractere, sem faixa nem
  classe): no grep é erro "character class syntax is [[:space:]], not [:space:]"; no sed também
  (sai com 4, mensagem sem posição).

## 4. grep

- Opções: `getopt_long` com permutação; `-NUM` é contexto (último grupo de dígitos seguidos vale);
  `-E/-F/-G/-P` diferentes juntos: "conflicting matchers specified"; erros de valor: "invalid
  context length argument", "invalid max count", "unknown devices method", "unknown binary-files
  type"; `--directories` inválido sai com 1 e lista os valores com aspas curvas; `--color=xyz`
  imprime a ajuda e sai com 0; `-u` avisa que é obsoleto.
- Padrões: `-e` e `-f` acumulam (newline separa padrões); repetidos são ignorados; `-f` vazio não
  casa nada (com `-v`, tudo); padrão do operando que começa com `\-` perde a barra. Erros de sintaxe:
  um por padrão, `grep: ARQUIVO:LINHA: msg` quando veio de `-f`, saída 2.
- `-x`: casada leftmost-longest cobre a linha. `-w`: a partir de cada casada, tenta encurtar no
  mesmo início (com o fim do texto contando como "não é fim de linha") e depois o próximo byte; aceita
  quando antes e depois não há caractere de palavra (`_` ou alnum).
- Binário: NUL no primeiro bloco lido (96 KiB) ou em bloco seguinte desliga a saída de linhas a
  partir dali e, se houver casada, imprime `grep: ARQ: binary file matches`; NULs viram fim de linha
  pra contagem (`printf 'a\0a\n' | grep -c a` dá 2). Linha com UTF-8 inválido não é impressa
  (`-o` só verifica o trecho casado) e gera a mesma mensagem no fim. `-a` desliga; `-I` pula.
- Contexto: `--` entre grupos não adjacentes (inclusive entre arquivos) sempre que `-A`, `-B` ou
  `-C` foi dado (até com 0); `-m N` para na N-ésima linha selecionada mas ainda imprime o contexto
  posterior; com `-v`, `-o` imprime as casadas das linhas de contexto.
- `-c`, `-l`, `-L`, `-q`: `-q` vence `-l/-L`, que vencem `-c`; saída 0 se alguma linha foi
  selecionada, 1 se não, 2 se houve erro (com `-q` e casada, 0). `-L` sai 0 quando algum arquivo
  tinha casada.
- Recursão: ordem do readdir; symlink abaixo da raiz é pulado com `-r` e seguido com `-R`; sem
  operando, procura em `.` e omite `./`; `--include/--exclude` comparam o nome base (na linha de
  comando, o caminho e cada sufixo depois de `/`), vale o último padrão que casa, e sem nenhum, o
  contrário do primeiro; dispositivos abaixo da raiz são pulados.
- stdin: depois de parar por `-m`, o offset fica logo depois da última linha usada.
- `-P`: um padrão só ("the -P option only supports a single pattern"), `-x` = `^(?:p)$`, `-w` =
  `(?<!\w)(?:p)(?!\w)`, `\d` só ASCII.
