# regex-posix: estado

Motor BRE/ERE do GNU (glibc 2.41). Guia de uso em `API.md`.

## Pronto

- Parser: porte do `regcomp.c` dirigido pelos bits `RE_*` (todas as sintaxes do `regex.h`, mais as
  do grep, sed, gawk e find). Extensões GNU, classes POSIX, `[.x.]`, `[=x=]`, intervalos até
  `RE_DUP_MAX`, referências 1 a 9, `RE_ICASE` como o glibc (padrão em maiúsculas), mensagens do
  glibc.
- Visão do `dfa.c` pro grep: repetição aplicada à âncora no ERE (`^*a`), avisos `* at start of
  expression` e afins, e o erro `[:space:]`.
- Semântica: leftmost-longest; submatches com as regras do `set_regs`/`update_regs` do glibc
  (prioridade por índice de nó, volta vazia de grupo opcional, âncora no fim do caminho, expansão
  de `{n,m}` do `parse_dup_op`); referências com busca exaustiva; `not_bol`/`not_eol`;
  `newline_anchor`; modo linha do grep.
- Execução: `regex-automata` (meta leftmost-first pro início + DFA preguiçoso `MatchKind::All` pro
  fim) sem referência; NFA próprio (Pike) quando o DFA desiste (fronteira de palavra Unicode com
  texto não ASCII) ou com flags; candidatos de início filtrados por um padrão relaxado quando há
  referência. Gancho de checkpoint nos laços do motor próprio.

## Placar

| corpus | sondas | regex | F01 (melhor combinação) |
|---|---|---|---|
| borda (`testbench/corpus/cases/regex`, emulação das sondas sobre a biblioteca) | 1214/1214 leniente, 1195/1214 estrito | 682/682 leniente | 99,27% |
| uso real minerado (4500 regex de agentes, 9291 sondas, golden em cache do F01) | 9291/9291 | 100%, 100% ponderado | 100% |
| sondas de grep do corpus de borda pelo `grep` de verdade (`ul-textproc`) | 879/879 estrito | | |

As 19 diferenças estritas da emulação são só a posição `char N` nas mensagens de erro do sed, que
a emulação não calcula (o `sed` do `ul-textproc` calcula).

Como rodar:

```sh
CARGO_TARGET_DIR=target/agents/regex cargo test -p regex-posix -- --nocapture
# uso real: gere os pares (caso, golden) com o gerador do F01 e aponte a variável
REGEX_POSIX_MINED=/caminho/mined-pairs.json cargo test -p regex-posix --release --test conformance mined -- --nocapture
```

O gerador dos pares minerados é um binário de rascunho que usa `f01_regex::mined` (mesma amostra e
semente do F01) e o golden em `testbench/scratch/f01-regex/mined-golden-*.json`; ele não é
commitado porque o corpus minerado não é.

## Falta / limites conhecidos

- Caracteres de palavra das fronteiras (`\b`, `\<`) seguem o `\w` Unicode do `regex-automata`
  (inclui marcas combinantes); o glibc usa `iswalnum` + `_`. Só difere fora do ASCII.
- Byte inválido de UTF-8 no texto: o glibc o trata como o caractere de mesmo valor pra contexto de
  palavra; aqui ele não é caractere de palavra.
- `[=x=]` e `[.x.]` só com um caractere (C.UTF-8 não tem regras de colação; igual ao glibc).
- Busca com referência é exaustiva: padrão patológico (`\(a*\)*\1` em texto longo) pode demorar,
  como no glibc; o gancho de checkpoint permite interromper.
