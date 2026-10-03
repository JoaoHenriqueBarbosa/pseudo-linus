# E08: corpus real de comandos de agente

## Hipótese

**H22** (v1): agentes escrevem bash sofisticado, mas o conjunto de comandos e recursos é concentrado o
bastante pra priorizar o que implementar.

Critério: confirmada se até 60 nomes de comando cobrem 95% das ocorrências e o `brush-parser` aceita
pelo menos 95% das chamadas.

## Método

1. Lê todos os transcripts locais do Claude Code (`~/.claude/projects/**/*.jsonl`, inclusive os de
   subagentes) e extrai o `input.command` de cada chamada da tool `Bash`. Só leitura.
2. Agrupa por texto exato e parseia cada comando único com o `brush-parser` 0.4 (feature `serde`). O AST
   vira JSON e é percorrido de forma genérica: comandos simples, pipelines, listas, compostos,
   redirecionamentos, heredocs, e as palavras são re-parseadas com `brush_parser::word::parse` pra achar
   `$(...)`, crase, aritmética e cada forma de `${...}`. Comandos dentro de substituição também são
   parseados (recursão).
3. Embrulhadores (`timeout`, `xargs`, `sudo`, `env`, `nohup`...) contam como ocorrência própria e o
   comando embrulhado também entra, marcado com `via`.
4. Estatística ponderada pela quantidade de chamadas: cobertura por nomes distintos, flags por comando,
   recursos de shell, comprimento de pipeline, e quanto das chamadas usaria só ferramentas do plano v2
   (lista em `src/catalog.rs`).

**Privacidade.** Os comandos completos e os padrões de grep/sed/awk/jq ficam só em
`testbench/corpus/agent/` (gitignored) e nunca são executados. O JSON de resultado tem só agregados;
caminhos de script local viram `<path-script>`, nomes com `$` viram `<dynamic>`, flags perdem o valor
depois de `=`.

Saídas locais consumidas por outros experimentos:

- `corpus/agent/commands.jsonl`: um comando único por linha (`command`, `count`, `projects`), usado pelo
  F15 (brush-parser contra `bash -n`).
- `corpus/agent/patterns.jsonl`: padrões por ferramenta (`grep`, `rg`, `sed`, `awk`, `jq`), usados pelo F01.

## Resultado (rodada de 2026-10-02)

- 757 transcripts, 32 projetos, **54.702 chamadas Bash** (47.390 únicas; 72% vieram de subagentes).
- **221.751 ocorrências de comando simples**, média de 4,06 por chamada; 84% das chamadas têm mais de um
  comando.
- **Cobertura**: 5 nomes cobrem 50% das ocorrências, **24 cobrem 90%**, **37 cobrem 95%**, 98 cobrem 99%,
  de 580 distintos.
- **Parse**: o `brush-parser` aceita 99,97% das chamadas (16 rejeições em 47 mil únicas: 15 de
  tokenização, 1 de parse).
- **Mais usados**: grep, cd, head, echo, git, sed, rg, tail, cat, ssh, ls, python3, curl, cargo, find, wc,
  kubectl, sleep, timeout, cut, gh, sort, docker, awk, tr, xargs, date, printf, base64.
- **Recursos de shell** (fração das chamadas): pipeline 55,3%, `&&` 49,1%, `2>&1` 23,8%, `$var` 13,1%,
  `2>` 12,7%, glob 11,5%, `VAR=valor` 10%, `$(...)` 6,4%, `for` 4,4%, `>` 3,1%, heredoc 3,1%, `||` 2,4%,
  brace expansion 1,6%; `if`, `while`, `case`, `[[ ]]`, arrays e `<(...)` ficam abaixo de 0,5% cada.
- **Pipelines**: 45% das chamadas não têm pipe, 41% têm 2 estágios, 11% têm 3, 2% têm 4, e há casos até 10.
- **Flags que dominam**: `grep -n`, `-rn`, `-v`, `--include`, `-E`, `-i`; `sed -n` (13,5 mil: leitura de
  trechos de arquivo com `sed -n 'A,Bp'`), `sed -i`; `head -N` e `tail -N` na forma antiga; `git log
  --oneline`; `find -name/-iname/-o/-path/-type`; `sort -u/-rn`; `cut -c1-N`; `curl -s -H -o --max-time -w`.
- **Plano v2 cobre 68,0% das chamadas** inteiras. O que falta, por grupo: rede (ssh, scp, rsync, dig:
  5,3 mil chamadas), python (3,6 mil), rust/cargo (2,1 mil), containers (1,9 mil), forges/gh (1,1 mil),
  javascript (1,1 mil), e ferramentas próprias do dono.

## Veredito

**H22: confirmada.** 24 nomes cobrem 90% e 37 cobrem 95% das ocorrências, e o parser aceita 99,97%. O
"sofisticado" é sobretudo composição (pipeline e `&&` em metade das chamadas, `2>&1` em um quarto), não
controle de fluxo: `if`, `while`, `case` e `[[ ]]` são raros. Prioridade que sai daqui pro userland: grep
(com `-rn`, `--include`, `-E`), head/tail na forma `-N`, sed `-n 'A,Bp'` e `-i`, git (log, status, diff,
show), cat, ls, find, wc, sort, cut, tr, awk, xargs, curl, date, base64. Pro shell: pipeline, listas,
redirecionamentos de fd, expansão de variável, glob e `$(...)` antes de qualquer outra coisa.
