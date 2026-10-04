# regex-posix: proveniência

Registro de onde veio cada parte do código, pra decisão de licença do dono. Fontes lidas durante a
escrita (tarballs do grep 3.11, que traz o regex e o dfa do gnulib, sincronizados com o glibc 2.41):

- `lib/regcomp.c`, `lib/regexec.c`, `lib/regex.h` (glibc/gnulib, **LGPL-2.1-or-later**);
- `lib/dfa.c` (gnulib, **GPL-3.0-or-later**).

Licença declarada no `Cargo.toml`: `LGPL-2.1-or-later AND GPL-3.0-or-later`.

| arquivo | origem |
|---|---|
| `src/syntax.rs` | valores dos bits `RE_*` e das sintaxes `RE_SYNTAX_*` copiados do `regex.h` (constantes de interface); nomes do `-regextype` do findutils (lista pública, documentada no manual). O resto é original. |
| `src/ast.rs` | original (o formato da árvore segue a ideia da árvore do `regcomp.c`, sem código traduzido). |
| `src/error.rs` | mensagens de erro copiadas do glibc (`__re_error_msgid`) e do `dfa.c`; são saída observável, necessária pro byte a byte. Estrutura original. |
| `src/parse.rs` | **tradução** das funções do `regcomp.c`: `peek_token`, `peek_token_bracket`, `parse`, `parse_reg_exp`, `parse_branch`, `parse_expression`, `parse_sub_exp`, `parse_dup_op`, `fetch_number`, `parse_bracket_exp`, `parse_bracket_element`, `parse_bracket_symbol`, `build_range_exp` (mesmos testes de bits e mesma ordem). Do `dfa.c`: `colon_state` (o `colon_warning_state` do `parse_bracket_exp`) e a regra de `laststart` que decide os avisos e a visão do dfa. Ponto de partida foi o parser do experimento F01 (`testbench/experiments/f01-regex/src/parse.rs`), que já seguia o `regcomp.c`. `to_upper`, a tradução do padrão com `RE_ICASE` e os testes são originais (a regra foi medida no oráculo). |
| `src/charclass.rs` | original. As definições das classes em C.UTF-8 seguem a documentação do LC_CTYPE do glibc e foram conferidas no oráculo. |
| `src/hir.rs` | original (tradução da AST pro `regex-syntax`). |
| `src/nfa.rs` | `Prog::dfs` no modo de grupos e `update_regs` reproduzem a lógica do `set_regs`, `proceed_next_node` e `update_regs` do `regexec.c` (prioridade pelo menor índice de nó, regra do `eps_via_nodes`, volta vazia de grupo opcional, parada com referências). `Compiler::repeat` segue a expansão do `parse_dup_op`; a ordem dos ramos vazios da alternação e a regra do "nó final sem âncora" (`GroupsPlainHalt`) foram deduzidas do `calc_first`/`link_nfa_nodes`/`duplicate_node_closure` do `regcomp.c`. O resto (programa, simulação de Pike leftmost-longest, busca exaustiva com referências, memória de estados, decodificação UTF-8) é original. |
| `src/regex.rs` | original (montagem `regex-automata` + NFA, API). |
| `src/lib.rs`, `API.md`, `STATUS.md`, `SPEC.md` | originais. |
| `tests/conformance.rs` | original; a emulação das sondas segue o `probe.rs` do F01, e a iteração do `s///g` reproduz o comportamento do `do_subst` do sed (lido no `execute.c`, GPL-3.0+). |

Dependências: `regex-automata` e `regex-syntax` (MIT/Apache-2.0), `bitflags` (MIT/Apache-2.0).

Reescrita em sala limpa: `SPEC.md` descreve o comportamento medido, e os corpus da bancada (borda,
minerado) são a rede de segurança.
