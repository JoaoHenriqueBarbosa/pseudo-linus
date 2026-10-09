# wasm_gc_bun_golden: timeout de 420 s (2026-10-08)

Auditoria por leitura de `src/wasm/` (nenhum cargo rodado). Não achei laço infinito:

- `read_block_type`, `read_u64`, `read_s64` só indexam o slice (pânico, nunca giram sem avançar).
- `is_strict_sub_rtt` desce por profundidade estritamente decrescente; tipos rec/sub são internados por grupo
  (`append_recursion_group`), sem validação recursiva.
- `branch!`/`loop` em `step_frame` avançam `pc` para `label.start`; `parse_nested_blocks_eagerly` sempre soma 1.

Custo ilimitado provado: `array.new`/`array.new_default` aceitam até 2^30 bytes (`MAX_GC_ARRAY_BYTES`), mas cada
elemento ocupa um slot `u64`; `init.repeat(length)` escrevia até 8 GiB para `i8`. Corrigido com `vec![0u64; n]`
quando o valor inicial é zero (calloc preguiçoso). Elementos não nulos (referências null, 1 GiB) seguem preenchidos.

Suspeitos restantes, ainda por medir: linhas 702 a 704 do tsv (`cnt(100000)`, `cnt(1000000)`, `cnt(10000000)`,
laço de 10 milhões de iterações no interpretador) e 713 (`sum(100000)`).

Bisseção: `WASM_GC_LINES=702-704 cargo test --test wasm_gc_bun_golden` roda só a faixa e imprime o tempo de cada
caso em stderr (`linha N: duração`). Sem a variável o teste roda tudo e exige no mínimo 250 programas.

## Caminho quente de `step_frame` (revisão, sem medir)

Achados por instrução ou por desvio que o IPInt do C++ não paga:

- `sync_handlers!` rodava a cada `br`/`br_if`/`end`/`else`. O custo real era pequeno (`handlers.last()` é O(1) e o
  `retain` num vetor vazio é curto), mas em build de teste (debug) o `retain` não inlina. Agora cada metade só roda
  se o vetor correspondente não está vazio. Semântica igual: vetor vazio não tem o que descartar. Não precisou de
  pilha de tratadores por profundidade: `handlers` já é uma pilha ordenada por `label_index`, então o truncamento
  é o `while` do topo, O(1) amortizado.
- `jumps` (consultado em todo `block`/`loop`/`if`/`try`/`try_table`), `delegates`, `catches` e `wide_operands`
  (consultado em todo `drop`/`select`) eram `HashMap`/`HashSet` com SipHash. Viraram `PcMap`, vetor ordenado por
  posição com busca binária, sem hash e sem alocação. O C++ guarda isso em tabela pré-computada na posição.
- `branch!` chamava `stack.drain(height..top)` mesmo com intervalo vazio; agora só quando `height != top`.

Não mudei: `read_u64`/`read_s64` decodificam LEB a cada execução (o IPInt também decodifica LEB na hora, só que
em metadados pré-expandidos para locais e desvios; cachear aqui exigiria reescrever o fluxo de código), nem o
`Label` (Copy, 40 bytes), nem `function.local_offsets[index]` (índice de vetor).

Se `cnt(10000000)` ainda estourar depois disto, o próximo passo é pré-decodificar `local.get`/`local.set`/`i32.const`
e os imediatos de `br`/`br_if` para uma tabela paralela a `code`, e medir com `WASM_GC_LINES=702-704`.
