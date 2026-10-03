# F01: motor de regex com a semântica POSIX do GNU

## Hipóteses

- **H23** (v2): existe motor de regex em Rust puro com a semântica POSIX do GNU. Critério: concordância
  de casa/não casa, span e submatch com o GNU nas suítes spencer/bre/ere e nas regexes do corpus de
  agente; confirmada se algum candidato concorda em 100% (ou só diverge em extensões raras e
  documentadas).

## Método

**Oráculo.** O GNU de verdade, no container Debian 13 (grep 3.11, sed 4.9, gawk 5.2.1, `LC_ALL=C.UTF-8`).
Cada regex vira até quatro sondas, todas comandos reais:

| sonda | comando | mede |
|---|---|---|
| `grep-n` / `sed-n` | `grep -n -G/-E [-i] -e RE s.txt`, `sed -n '\cREcp'` | casa ou não, linha a linha |
| `grep-o` | `grep -ob ...` | spans de todas as casadas (leftmost-longest, iteração do grep) |
| `sed-g` | `sed -n 's/RE/\x02&\x03/gp'` | spans na iteração do `s///g` (casada vazia colada na anterior é pulada) |
| `sed-sub` | `sed -n 's/RE/\x02&\x03\1\x03\2...\x04/p'` | conteúdo dos grupos da primeira casada (BRE e ERE) |
| `gawk` | `match($0, re, a)` | posição e participação de cada grupo (ERE sem backref, só quando o padrão significa o mesmo no gawk) |

O delimitador do sed é `\x01`, que não aparece em padrão nenhum. Nas suítes upstream, o exit do
`grep -ob` dá o casa/não casa (o oráculo bate com a expectativa das suítes em 329/329 linhas).

**Tradutor nosso** (`src/parse.rs`, `src/emit.rs`). Nenhuma crate entende o dialeto GNU, então todo
motor recebe o mesmo AST, saído de um parser dos quatro dialetos (BRE/ERE do grep e do sed) que segue o
`regcomp.c` do glibc: contexto de `^`, `$` e `*`, `\{` inválido virando literal no `grep -E`,
`\+ \? \|`, `\< \> \b \B \w \W \s \S \` \'`, backref só pra grupo já fechado, colchetes com `]` e `-`
nas pontas, classes, `[[:space:]]` x `[:space:]` (erro do `dfa.c`), faixa com caractere fora do ASCII
(erro em C.UTF-8), escapes do sed (`\t`, `\n`, `\xHH`, `\dNNN`, `\cX`) e a visão do `dfa.c` do grep
(no `-E`, repetição em posição inicial se aplica à âncora anterior: `^*a` casa `a` em qualquer lugar,
mas o `-o` usa o glibc, que pula o `*`). O emissor escreve o AST na sintaxe de cada motor (escapes,
classes, grupos sintéticos com mapa de numeração, `{n,m}` acima de 255 dividido pra ERE POSIX).
Prova de que o tradutor não é o gargalo: nenhuma falha do corpus commitado é comum a todos os motores.

**Leftmost-longest por montagem.** Pra motores leftmost-first, o início da casada é o mesmo do POSIX
(a posição mais à esquerda em que algo casa); o fim mais longo sai de uma segunda busca ancorada
nesse início: DFA com `MatchKind::All` no `regex-automata`, `ONIG_OPTION_FIND_LONGEST` no
`ferroni` e no `rusty_expressions`.

**Isolamento.** Cada motor roda em subprocessos (`--worker`), comparando com o golden caso a caso via
`harness::score`; caso que passa 3 s sem resposta vira timeout e o worker recomeça do seguinte.

**Corpus.**

- `corpus/cases/regex/upstream-{spencer1,bre,ere}.toml`: as suítes do GNU grep 3.11 (baixadas do
  tarball de release pra `corpus/upstream/grep/`). A `spencer2.tests` não existe na v3.11: foi dividida
  em `bre.tests` e `ere.tests` em 2009. Linhas de 4 campos (TO CORRECT) entram com a tag `upstream-todo`.
- `corpus/cases/regex/agent-style.toml`: 251 regexes no estilo de agente (logs, IPs, e-mail, URL,
  classes POSIX, `\b`, `\w`, `\s`, âncoras, alternação, quantificadores, backrefs em BRE, extensões GNU,
  regra de subexpressão, UTF-8, erros de sintaxe, dialeto do sed), escritas em
  `data/agent_style.toml`.
- Os dois acima somam 682 regexes e 1214 sondas, gerados pelo binário `f01-gen-cases` (derivados
  mecânicos; a fonte editável é a especificação e as suítes) e com golden em `golden/regex/`.
- **Corpus minerado** (não commitado): `corpus/agent/patterns.jsonl` do E08, só `tool = grep` (BRE/ERE
  do GNU) e `tool = sed`; `rg` fica de fora. As flags (`-E`, `-F`, `-P`, `-i`, `sed -E`) foram
  recuperadas parseando `commands.jsonl` (nada é executado). Amostra: os 2000 padrões de grep e os 1000
  de sed mais frequentes, mais 1000 e 500 sorteados com semente fixa: 4500 regexes, 9281 sondas, 16269
  das 32811 ocorrências. Linhas de teste: as linhas do corpus à mão mais 6 amostras geradas pela própria
  regex. O oráculo roda na hora (cache em `scratch/f01-regex`); no JSON entram só agregados.

## Candidatos

| candidato | semântica declarada |
|---|---|
| `regex` 1.13.1 | leftmost-first, sem backref |
| `regex-automata` 0.4.18 (montagem nossa) | início leftmost + fim mais longo por DFA `MatchKind::All`; grupos leftmost-first dentro do span |
| `fancy-regex` 0.19.2 | leftmost-first com backtracking, backref |
| `revera` 0.2.1 | ERE POSIX.1-2024, leftmost-longest, subexpressões POSIX; sem BRE, backref, `\b` |
| `posix-regex` 0.1.4 | BRE/ERE do relibc, só ASCII |
| `regast` 0.1.0 | leftmost-longest com desambiguação POSIX por derivadas |
| `rusty_expressions` 0.2.2 (montagem nossa) | Oniguruma em Rust, com FIND_LONGEST ancorado |
| `ferroni` 1.8.1 (montagem nossa) | Oniguruma em Rust, com FIND_LONGEST ancorado; também a variante com as sintaxes GREP/POSIX_EXTENDED do próprio Oniguruma, sem o nosso tradutor |
| `resharp` 0.7.5 | derivadas simbólicas, leftmost-longest, lookaround, grupos experimentais |
| `red-sed` 1.0.2 (o motor do `red`) | parser próprio do dialeto GNU, sem tradutor |

Examinados e não rodados: `eregex` 0.1.5 (POSIX matching só planejado), `regex-lite` (mesma semântica do
`regex`), `derivre` (sem busca com spans), `ere`/`ere-core` (macro de compilação). `pcre2`, `onig`,
`tre-regex`, `minrx`, `gnurx-sys` e `regex-rs` são C ou FFI pra libc: só referência.

## Resultado

Concordância no nível da regex (todas as sondas iguais ao GNU) e por aspecto no corpus commitado;
"ponderado" pesa cada regex minerada pela frequência real. Categoria do depscan (a: não toca o host).

| candidato | cat. | commitado: regex | casa | span | grupos | minerado: regex | ponderado |
|---|---|---|---|---|---|---|---|
| regex | a | 95,60% | 95,62% | 95,75% | 89,68% | 99,16% | 98,89% |
| regex-automata-longest | a | 95,75% | 95,22% | 96,19% | 89,68% | 99,98% | 99,99% |
| fancy-regex | b | 99,12% | 100% | 99,41% | 98,22% | 99,16% | 98,89% |
| revera | a | 90,18% | 87,65% | 90,76% | 82,21% | 99,98% | 99,99% |
| posix-regex | a | 87,98% | 92,83% | 90,18% | 82,21% | 98,82% | 98,33% |
| regast (minerado: 1/10) | a | 90,03% | 87,65% | 91,50% | 80,43% | 98,64% | 96,48% |
| rusty_expressions-longest | a | 98,24% | 97,61% | 98,97% | 95,02% | 99,96% | 99,93% |
| **ferroni-longest** | a | **99,27%** | 100% | 100% | 97,15% | **100%** | **100%** |
| ferroni-native-syntax | a | 88,42% | 87,25% | 89,15% | 93,95% | 99,42% | 99,62% |
| resharp | b | 88,56% | 93,63% | 94,57% | 64,77% | 99,76% | 99,37% |
| red-sed-regex | b | 86,07% | 93,23% | 86,80% | 96,09% | 98,93% | 99,34% |
| combo regex-automata + ferroni (backref) | - | 99,27% | 99,60% | 99,85% | 98,22% | 99,98% | 99,99% |
| combo regex-automata (span) + ferroni (grupos, backref) | - | 99,12% | 99,60% | 99,85% | 97,15% | 100% | 100% |
| combo revera + ferroni (backref, `\b`) | - | 97,80% | 98,01% | 98,53% | 97,15% | 99,98% | 99,99% |

Achados:

- **Erros de sintaxe: 100% em todos**, porque quem decide é o nosso parser. Mensagem, exit e o que é
  erro em cada dialeto são trabalho nosso, não do motor.
- **O GNU não segue a regra de subexpressão do POSIX.** Em `(a|ab)(c|bcd)(d*)` sobre `abcd` o glibc dá
  `a`/`bcd`/vazio; os motores POSIX corretos (`revera`, `regast`) dão `ab`/`c`/`d`. Ser POSIX correto
  não ajuda a reproduzir o GNU.
- **Leftmost-first erra em uso real**: `^#|^##|^###`, `^test |test result`, `ab|a` (o agente escreve a
  alternativa curta primeiro e o GNU pega a longa). `regex` e `fancy-regex` erram 1,1% do uso ponderado.
- **`ferroni` + tradutor + FIND_LONGEST ancorado** iguala o GNU nas 4500 regexes mineradas e erra 5 das
  682 do corpus de borda, todas de submatch com repetição de grupo (`(a*)*`, `(a*)+`, `(x*)*y`,
  `\(a*\)*\1`, `(^)*`): o glibc atribui o grupo à última iteração não vazia, o backtracking à última
  iteração vazia.
- **As sintaxes nativas GREP/POSIX_EXTENDED do Oniguruma não são o dialeto GNU** (88% no corpus de
  borda): sem o nosso parser não serve.
- `rusty_expressions` 0.2.2 tem defeito de busca: `\[[a-z]+\]` não casa com `[db]` (teste em
  `src/engines.rs`). `regast` busca tentando todo par (início, fim), O(n³) por linha, e no minerado roda
  em 1 de cada 10 regexes. `posix-regex` trava (3 timeouts) e erra a casada mais à esquerda.
- O depscan marca `ferroni` como (c) pela build-dependency opcional `cc` (feature `ffi`, desligada); o
  grafo resolvido não tem C, então a categoria efetiva é (a) (o JSON guarda as duas).

## Veredito

**H23: parcial.** Não existe crate que, sozinha, entenda BRE/ERE do GNU e reproduza a semântica: as
falhas sem o nosso tradutor (88% no melhor caso) e dos motores leftmost-first estão no núcleo. Com
peças prontas mais código nosso, chega-se a 100% do uso real medido:

- **Fazer à mão:** o parser dos dialetos do grep e do sed (este, ~700 linhas, já cobre as suítes), as
  mensagens de erro, a montagem leftmost-longest (início pela busca normal, fim pela busca mais longa
  ancorada), a iteração do `grep -o` e do `s///g`, e, se for preciso fidelidade total de grupos, a regra
  de subexpressão do glibc em repetição de grupo.
- **Aproveitar:** `ferroni` (Oniguruma em Rust, backtracking, backref) como motor geral, e
  `regex-automata` (DFA, tempo linear) como caminho rápido pra padrões sem backref, que no uso real são
  quase todos; a combinação dos dois dá 99,27% no corpus de borda e 99,99% do uso ponderado.

## Como rodar

```sh
cargo run --release --bin f01-gen-cases                  # regenera corpus/cases/regex
cd ../.. && cargo run -q -p oracle -- gen --tool regex   # golden
cd experiments/f01-regex && cargo run --release          # results/f01-regex.json, ~2,5 min
cargo run --release -- --try ferroni-longest grep-ere '(a|ab)(c|bcd)' abcd   # depuração
```
