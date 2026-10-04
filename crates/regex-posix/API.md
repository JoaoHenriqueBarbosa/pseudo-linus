# regex-posix: guia da API

Motor de regex do GNU (glibc 2.41) pro pseudo-linus. Biblioteca pura: nenhuma E/S, nenhum `unsafe`.
Quem usa: `grep`/`sed` (`ul-textproc`), `awk` (`ul-awk`), `find -regex` (`ul-findutils`), e quem mais
precisar de BRE/ERE com a semântica do GNU.

## Em uma tela

```rust
use regex_posix::{Regex, RegexBuilder, Syntax, ExecFlags};

// Sintaxe por bits RE_* do glibc; as dos programas já vêm prontas.
let re = RegexBuilder::new(Syntax::GNU_AWK)
    .icase(false)
    .build(br"(ab|a)(c|bcd)")?;          // Err(Error) com a mensagem do glibc

re.is_match(b"xabcd");                    // casa ou não (caminho mais rápido)
let m = re.find_at(b"xabcd", 0).unwrap(); // leftmost-longest: m.start, m.end (bytes)
let caps = re.captures_at(b"xabcd", 0).unwrap();
caps.get(1);                              // Option<Match>: grupo 1 (None = não participou)
re.longest_at(b"abcd", 0, ExecFlags::default()); // re_match: fim da casada mais longa ancorada em 0
```

Todo offset é em bytes do haystack (`&[u8]`). Texto com UTF-8 inválido é aceito: o byte inválido
não casa com `.` nem com colchete (como o glibc em C.UTF-8), mas casa com ele mesmo se aparecer
literal no padrão.

## Sintaxes prontas (`Syntax`)

| constante | quem usa | observação |
|---|---|---|
| `Syntax::GREP` | `grep -G` | `RE_SYNTAX_GREP` (newline no padrão é alternação) |
| `Syntax::EGREP` | `grep -E` | `RE_SYNTAX_EGREP` (`{` inválido vira literal, `*` no começo é ignorado) |
| `Syntax::SED_BASIC`, `Syntax::SED_EXTENDED` | `sed`, `sed -E` | os bits que o `sed/regexp.c` liga no modo padrão |
| `Syntax::GNU_AWK` | `gawk` | `RE_SYNTAX_GNU_AWK` |
| `Syntax::POSIX_AWK` | `gawk --posix` | `RE_SYNTAX_POSIX_AWK` |
| `Syntax::AWK` | `gawk --traditional` | `RE_SYNTAX_AWK` |
| `Syntax::EMACS` | `find -regex` (padrão) | `RE_SYNTAX_EMACS` (0) |
| `Syntax::from_regextype(nome)` | `find -regextype` | os 13 nomes do findutils 4.10; `REGEXTYPE_NAMES` lista |
| `Syntax::POSIX_BASIC`, `POSIX_EXTENDED`, `POSIX_MINIMAL_*`, `ED`, `SED` | genéricos | iguais ao `regex.h` |

Os bits são públicos (`Syntax::ICASE`, `Syntax::NO_GNU_OPS`...): monte a sintaxe que o programa usa
com `|` e `.difference()`, exatamente como o código C faz.

## Opções de compilação (`RegexBuilder`)

| método | equivale a | padrão |
|---|---|---|
| `icase(bool)` | `RE_ICASE` | desligado. Como o glibc: padrão e texto em maiúsculas (`grep -i '[Z-a]'` dá "Invalid range end", igual ao GNU) |
| `newline_anchor(bool)` | `re_pattern_buffer.newline_anchor` | **desligado**. O `re_compile_pattern` do glibc liga por padrão: o `find -regex` deve ligar (o findutils não muda); o gawk e o grep desligam; o sed liga só com a flag `M` |
| `no_sub(bool)` | `RE_NO_SUB` | desligado (submatches calculados) |
| `line_separator(Some(b))` | modo linha do grep | o byte nunca casa e as âncoras casam junto dele; serve pra varrer um buffer com muitas linhas |
| `whole_line(bool)` | `grep -x` | `^(...)$` em volta da alternação inteira |
| `dfa_view(bool)` | semântica do `dfa.c` | só o grep usa (escolha de linhas; difere do glibc em `^*` no ERE) |
| `confusing_brackets_error(bool)` | `DFA_CONFUSING_BRACKETS_ERROR` | `[:space:]` vira `Error::ConfusingBrackets` |
| `checkpoint(Arc<dyn Fn()>)` | gancho de preempção | chamado a cada ~16 mil passos nas buscas do motor próprio; ligue em `sysabi::sys::checkpoint` |

Construtores: `build(padrão)`, `build_many(&[padrões])` (alternação, cada padrão analisado sozinho,
grupos renumerados em sequência; o erro traz o índice do padrão), `build_literals(&[cadeias])`
(`grep -F`), `check(padrão)` (só análise: erros e diagnósticos, sem compilar).

`MatchKind` não existe: a semântica é sempre a do POSIX/GNU (leftmost-longest).

`RE_NEWLINE` do `regcomp` (o `REG_NEWLINE` da API POSIX) é: `newline_anchor(true)` mais tirar
`Syntax::DOT_NEWLINE` e ligar `Syntax::HAT_LISTS_NOT_NEWLINE`, que é o que o glibc faz.

## Busca

| método | equivale a |
|---|---|
| `is_match(hay)`, `is_match_at(hay, start)` | `re_search(..., NULL)` com resultado ≥ 0 |
| `find_at(hay, start)` | `re_search` de `start` até o fim: casada leftmost-longest que começa em `start` ou depois. O texto antes de `start` vale como contexto (`^` só casa em 0, `\<` olha o caractere anterior) |
| `find_at_with(hay, start, ExecFlags { not_bol, not_eol })` | `REG_NOTBOL`/`REG_NOTEOL` |
| `longest_at(hay, pos, flags)` | `re_match`: fim da casada mais longa ancorada em `pos` |
| `captures_at(hay, start)`, `captures_at_with(...)` | `re_search` com `regs`: grupos com as escolhas do glibc |
| `groups_of(hay, s, e, flags)` | grupos de uma casada já conhecida |
| `find_iter(hay)` | casadas não vazias na ordem do `grep -o` |

`Captures::get(i)` devolve `Option<Match>`; o índice 0 é a casada inteira. `Captures::len()` é
`group_count() + 1`.

Iteração de substituição (`sed s///g`, `gsub` do awk): cada programa tem a sua regra pra casada
vazia; implemente com `find_at` a partir do fim da anterior, como o C faz. O `sed` do `ul-textproc`
é o exemplo (`do_subst`). Um começo no meio de um caractere UTF-8 é empurrado pro fim dele, como o
glibc.

## Diagnósticos

- `Error::Syntax(ErrorCode)`: `message()` é a do glibc ("Unmatched [, [^, [:, [., or [=", "Invalid
  back reference", ...); `code()` é o `REG_*`.
- `Error::ConfusingBrackets`: `CONFUSING_BRACKETS` é a mensagem do `dfa.c`.
- `Regex::warnings()`: avisos do `dfa.c` (`* at start of expression` etc.), na ordem do grep.

## Semântica (o que foi medido)

- Corpus de borda do F01 (682 regex, 1214 sondas contra o GNU real): 100% leniente; o F01 media
  99,27% com o melhor motor.
- Uso real minerado (4500 regex de agentes, 9291 sondas): 100%.
- Submatches como o glibc, que não segue a regra do POSIX: `(a|ab)(c|bcd)(d*)` em `abcd` dá
  `a`/`bcd`/vazio; a volta vazia de grupo opcional mantém o valor anterior (`(a*)*` em `aa` dá
  `aa`); grupo que só contém âncora fica de fora (`(^)*`).
- Classes em C.UTF-8: ASCII exato; fora do ASCII pelas propriedades Unicode equivalentes ao
  LC_CTYPE do glibc (`alpha` inclui dígitos não ASCII, `space` exclui espaços sem quebra).
- Faixa com caractere fora do ASCII dá "Invalid collation character", como no glibc em C.UTF-8.

## Custo

- Sem referência: `regex-automata` (meta + DFA preguiçoso), tempo linear. Fronteira de palavra
  Unicode diante de texto não ASCII, `not_bol`/`not_eol`: NFA próprio, tempo linear (mais lento).
- Submatches: busca em profundidade com memória de estados visitados (polinomial) sobre o trecho
  da casada.
- Referência: busca exaustiva (exponencial no pior caso, como no glibc). Use o gancho de
  checkpoint.

`Regex` é `Clone` (barato, `Arc` por dentro), `Send` e `Sync`.
