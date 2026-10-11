# Baseline de desempenho do zjsc contra o bun 1.4.2

Medição de 2026-10-11, base `e320930e` (branch `intl-causa-raiz`) mais o conserto do `replace` com callback descrito abaixo.
Reproduzir: compilar `cargo build --release --example bench_run` em `wip/zjsc` (container `rust-mold`, `--memory 16g`,
`CARGO_INCREMENTAL=0`) e rodar `wip/zjsc/scripts/bench-js.sh resultado.tsv`; `wip/zjsc/scripts/bench-summary.py resultado.tsv` gera a tabela.

## Método

- Os MESMOS arquivos de `wip/zjsc/bench/js` rodam nos dois: `bench_run` (exemplo do zjsc em perfil `release`, programa
  principal sobre o motor sem o host do sandbox) e o `bun` 1.4.2 da imagem `pl-oracle-bun-check`, cada um no seu container
  com `--memory 16g`, `--rm`.
- Tempo de parede do PROCESSO (`date +%s%N` em volta dele, dentro do container, sem o início do container), 3 execuções,
  mediana. O `empty` (um script vazio) mostra o custo fixo: 23 ms no zjsc, 5 ms no bun.
- A coluna "saída igual" compara o `cksum` do stdout nos dois motores.
- A máquina estava dividida com outros agentes: a variação entre rodadas chegou a 40% (fib: 4,6 s a 6,5 s). Leia as razões como ordem de grandeza.
- Richards é o clássico reduzido do Octane. `deltablue.js` NÃO é o DeltaBlue completo: é um solucionador de restrições
  reduzido, escrito para este baseline (cadeia de igualdades e projeção de escala, sem remoção incremental nem replanejamento).

## Resultado

| Cenário | zjsc (ms) | bun (ms) | razão zjsc/bun | saída igual |
|---|---|---|---|---|
| empty (início do processo) | 23 | 5 | 4,6 | sim |
| richards (150 voltas) | 15003 | 30 | 500 | sim |
| deltablue (reduzido) | 53775 | 67 | 803 | sim |
| fib(32) recursivo | 6456 | 22 | 294 | sim |
| concatenação, split, join e indexOf de 2 MB | 10494 | 456 | 23 | sim |
| Array.sort (300 mil números, 100 mil strings) | 2461 | 97 | 25 | sim |
| JSON.stringify e parse de 1 MB, 5 voltas | 710 | 44 | 16 | sim |
| RegExp (match, matchAll, replace com função, split) sobre 1 MB | 1460 | 55 | 27 | sim |
| 1 milhão de decodeURI e 1 milhão de decodeURIComponent | 6584 | 286 | 23 | sim |
| Playwright `coreBundle.js` (3,4 MB): parse e compilação, 3 vezes | 168 | 96 | 1,8 | sim |
| Playwright `utilsBundle.js` (3 MB): parse e compilação, 3 vezes | 174 | 86 | 2,0 | sim |
| Playwright `coreBundle.js`: `require` | 465 | 245 | 1,9 | não |
| Playwright `utilsBundle.js`: `require` | 168 | 132 | 1,3 | não |

O decodeURI que aparecia solto (1 milhão em 19,9 s) era do perfil de depuração; no `release` são 6,6 s para os dois milhões
de chamadas, 23 vezes o bun.

### O `require` do bundle não chega ao fim no zjsc

No bun, `require('coreBundle.js')` carrega e devolve um objeto com 13 chaves. No zjsc ele pára em
`Cannot find module 'tty'`: o `node:tty` não existe. Os 465 ms são, portanto, o tempo até a primeira falta, não o de
carregar tudo, e a razão desses dois cenários NÃO vale como medida de execução. A lista do que falta depois do `tty` só
aparece corrigindo um módulo de cada vez.

## Achado de correção no caminho (corrigido)

O `regex.js` derrubava o zjsc com pânico (`asObject em objeto que o registro ainda não expõe`): `String.prototype.replace`
com callback sobre mais de 20 mil matches guardava os resultados do `exec` só num `Vec` do código nativo, sem raiz, e uma
coleta durante o callback varria os que ainda não tinham sido lidos. A correção é um `RootScope` sobre `results` em
`reg_exp_replace_generic`, com o teste `tests/regexp_replace_gc_regression.rs` (60 mil matches). O teste passa com a correção;
não rodei a versão sem ela em perfil de depuração para vê-lo vermelho, a evidência da falha é o `release` do baseline.

## O pior cenário e onde o tempo vai

O pior é o `deltablue` (razão 800), seguido de `richards` (500) e `fib` (290): código com chamadas, acesso a propriedade e
aritmética inteira, onde o bun usa o JIT. Os cenários de biblioteca (sort, json, regex, decodeURI, string) ficam em 16 a 27
vezes. Parse e compilação do bundle do driver ficam em 2 vezes.

O `perf` não pôde ser usado (o `kernel.perf_event_paranoid` do hospedeiro bloqueia o evento, e mudar isso é mudar o host).
O perfil é por contagem de instruções com o `callgrind` (valgrind) sobre o `release` com tabelas de linha, em cargas reduzidas
(`fib(25)`: 1,76 bilhão de instruções, cerca de 7200 por chamada de função; `deltablue` com 2 voltas: 15,6 bilhões).

| Onde | fib(25) | deltablue |
|---|---|---|
| `llint_execute` (o laço do interpretador) | 28% | 21% |
| Registro de células (`RefCell<Registry>`, `get_for_marking`, clone e drop de `CellEntry`, init do thread local, `as_js_object`) | 6% | 22% |
| Decodificação de operandos a cada execução (`read_operand`, `from_operands`, `opcode_id_enum`) e `SlowPathFrame::get` | 14% | 10% |
| Hash SipHash do `RandomState` (`hash_one`, `Sip13Rounds::write`) em escopo e tabela de propriedades | 4% | 7% |
| Resolução de escopo e import (`resolve_scope`, `get_from_scope`, `resolve_import_binding`, `symbol_table_get`) | 9% | pouco |
| `get_by_id` lento (`slow_path_get_by_id`, `get_property`, `get_property_slot`, `PropertyTable::find`) | pouco | 10% |

Leitura: o interpretador não tem cache inline (cada `get_by_id` e `get_from_scope` refaz a busca completa), decodifica o
bytecode a cada execução, passa pelo slow path até para `add` e `sub` de inteiros, e cada acesso a célula atravessa um
`RefCell` global com clone de `Rc`. Os percentuais são parcelas de instruções, não de tempo de parede.

## As 3 maiores otimizações propostas (NÃO implementadas)

Os ganhos são estimativas a partir das parcelas acima, não medições.

1. **Cache inline para `get_by_id`, `put_by_id` e `get_from_scope`/`resolve_scope`, e troca do SipHash.** Guardar no
   metadado da instrução o id da estrutura e o deslocamento (propriedade) ou o ambiente e o índice (escopo), e usar um
   hasher rápido nas tabelas que sobrarem. Ataca cerca de 17% (deltablue) e 13% (fib) das instruções. Ganho esperado: 1,2 a 1,5 vez.
2. **Instruções pré-decodificadas e caminhos rápidos de inteiro no interpretador.** Decodificar os operandos uma vez (ou
   gerar o `enum` tipado por instrução) e resolver `add`, `sub`, `lt`, `inc` e `jless` de int32 dentro do laço, sem slow path.
   Ataca 10% a 14% das instruções e a maior parte do `SlowPathFrame::get`. Ganho esperado: 1,2 a 1,4 vez.
3. **Acesso a célula sem o `RefCell` global.** O registro é consultado a cada leitura de objeto, com clone e drop de `Rc`
   (22% das instruções no `deltablue`). Entregar referência emprestada ou índice direto de uma arena, sem contar referência
   por acesso. Ganho esperado: 1,2 a 1,3 vez, e é o que libera as outras duas de ficarem baratas.

Juntas, as três provavelmente dão 2 a 3 vezes. Isso leva `richards` e `deltablue` de 500 a 800 vezes para algo na casa de
150 a 400 vezes: continua longe do bun, porque a distância restante é a de um interpretador contra um JIT. Se o M5.6 precisar
de código quente do React perto do bun, o caminho é um JIT de linha de base (ou, antes, medir uma página React real e ver se o
quente é o laço do interpretador ou as bibliotecas, que estão em 16 a 27 vezes), e essa decisão pede a medição de uma página
de verdade antes de investir.
