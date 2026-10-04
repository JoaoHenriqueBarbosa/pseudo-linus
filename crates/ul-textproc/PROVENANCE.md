# ul-textproc: proveniência

Registro de onde veio cada parte do código, pra decisão de licença do dono. Licença declarada no
`Cargo.toml`: `GPL-3.0-or-later`.

## grep, egrep, fgrep, rgrep

Fontes lidas (tarball do GNU grep 3.11, **GPL-3.0-or-later**): `src/grep.c`, `src/dfasearch.c`,
`src/kwsearch.c` (`Fexecute`), `src/pcresearch.c` (`Pcompile`, `Pexecute`), `lib/exclude.c`
(gnulib, GPL-3.0+).

| arquivo | origem |
|---|---|
| `src/grep/search.rs` | **porte** do `grep.c`: `reset`, `fillbuf`, `grep()`, `grepbuf`, `prtext`, `prpending`, `prline`, `print_line_head`, `print_line_middle`, `file_must_have_nulls`, `finalize_input` (mesma estrutura de buffer, resíduo, contexto, detecção de binário e `-m`). |
| `src/grep/mod.rs` | segue o `main()` (tabela de opções longas e curtas, ordem das verificações, `-NUM`, `keycc == 0`, códigos de saída), `grepdesc`, `grepfile`, `grepdirent`, `skipped_file`; a validação padrão por padrão segue o `GEAcompile`. Os textos do `--help` e do `-V` (`help.txt`, `VERSION`) são cópia da saída do oráculo. |
| `src/grep/matcher.rs` | o laço do `-w` e o comprimento do `re_match` seguem o `EGexecute` (`dfasearch.c`); o `-P` segue o `Pcompile` (`^(?:...)$`, `(?<!\w)(?:...)(?!\w)`, `\d` ASCII). A montagem sobre o regex-posix e o `fancy-regex` é original. |
| `src/grep/glob.rs` | `fnmatch` original; a regra de decisão de `--include`/`--exclude` segue o `excluded_file_name` do `lib/exclude.c`. |
| `src/grep/help.txt` | saída do `grep --help` do oráculo. |

## sed

Fontes lidas ANTES do aviso de licença do coordenador (GNU sed 4.9, **GPL-3.0-or-later**):
`sed/sed.c` inteiro, `sed/regexp.c` inteiro, `sed/compile.c` do começo até o início do
`normalize_text` (tabela de erros, `bad_prog`, `inchar`, `match_slash`, `snarf_char_class`,
`mark_subst_opts`, `read_label`, `setup_replacement`, `read_text`, `compile_address`,
`compile_program`), e o `do_subst` do `sed/execute.c`. Depois do aviso, nenhum fonte do GNU sed foi
aberto. O sed foi escrito a partir do manual do GNU sed 4.9 (`doc/sed.texi`, GFDL), do comportamento
observado no oráculo (Debian 13) e da testsuite (`misc.pl` como teste); a estrutura é própria. Como
houve leitura prévia, não é sala limpa estrita.

## Comum

| arquivo | origem |
|---|---|
| `src/getopt.rs` | original; reproduz o comportamento documentado do `getopt_long` do glibc (permutação, prefixos, mensagens), sem leitura do fonte nesta sessão. |
| `src/io.rs`, `src/lib.rs` | originais. |
| `tests/conformance.rs` | original. |

Dependências: `regex-posix` (este projeto; ver o PROVENANCE dele), `fancy-regex` (MIT), `sysabi`.
