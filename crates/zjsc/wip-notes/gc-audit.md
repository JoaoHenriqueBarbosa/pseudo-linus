# Auditoria de ciclo de vida das células e desenho de coletor (2026-10-08)

## 1. Quando uma célula sai do registro

`cell_registry` é um `thread_local` `Vec<Option<CellEntry>>`; `insert`/`reserve` só empurram, o índice
nunca é reaproveitado. O único chamador de `remove` em `src/` é `ordered_hash_table_storage.rs:145`
(o iterador de Map/Set que terminou). `heap.rs` só guarda size classes (e
`decrement_deferral_depth_and_gc_if_needed` não coleta); `vm.rs` não tem coleta. Conclusão: toda célula
JS vive até o fim da thread. `JSValue::Cell(usize)` guarda só o id, então o dono forte de qualquer célula
é o registro (mais clones de `Rc` temporários na pilha Rust).

## 2. Lixo cíclico

Não é só o cíclico: **todo** objeto alocado vaza, cíclico ou não (`for(i<1e7){let o={}}` já cresce
sem limite). Ciclos (`o.self=o`, closures, Map apontando para si) não mudam nada, porque o ciclo é
por id e o registro segura tudo. Memória cresce linearmente com alocações. (Não medi RSS: sem cargo,
é leitura de código.)

## 3. Weak*

- `WeakRef` (`js_weak_object_ref.rs`): `Cell<JSValue>` com o alvo, forte por construção; `deref` sempre
  devolve o alvo.
- `WeakMap`/`WeakSet` (`js_weak_map.rs`, `js_weak_set.rs`): `OrderedTable` comum indexada por `cell_id`
  da chave; entrada vive até `delete`. Não é efêmero.
- `FinalizationRegistry`: `register`/`unregister` existem, o callback nunca roda.
- `Weak<T>` de Rust só aparece em `inferred_value.rs`.

Oráculo bun 1.4.2 (medido): `typeof Bun.gc === "function"`, `typeof globalThis.gc === "undefined"`
(sem `--expose-gc`). `let w=new WeakRef({}); Bun.gc(true); w.deref()` dá `undefined`.
`FinalizationRegistry` com alvo `{}` descartado imprime o callback depois de `Bun.gc(true)`
(a callback é assíncrona: roda numa tarefa posterior, não dentro do `Bun.gc`). `Bun.gc(true)` devolve
um número (bytes, 137267 na medição) e `Bun.gc(false)` também. O porte pode oferecer `Bun.gc(sync)` e
`gc()` condicionado a flag, devolvendo o tamanho do heap.

## 4. Desenho: mark-sweep sobre o registro

Como o grafo é por id e o registro é o único dono, o coletor é seguro sem unsafe:

1. **Raízes** (todas enumeráveis em código seguro):
   - pilha de frames do interpretador (registradores `JSValue::Cell`, callee, scope, this);
   - objetos globais e `VM` (structures, prototypes, `CommonIdentifiers`, símbolos registrados);
   - handles: `Vec<JSValue>` em `Strong`/`Handle` explícito do VM (valores temporários de host
     functions, argumentos de `HostCall`); hoje o conceito não existe e é a peça crítica;
   - `STORAGES` de iteradores e outras tabelas `thread_local` com ids;
   - microtasks/promise jobs pendentes e timers.
2. **Marcação**: `visit_children(&CellEntry, &mut Marker)` por tipo de `CellEntry`, empilhando ids
   (worklist, `Vec<usize>` mais um `Vec<bool>`/bitset indexado pelo índice). Reusa-se a enumeração de
   campos que o C++ tem em `visitChildren`; é por tipo de célula (muitos tipos, trabalho repetitivo).
3. **Fracos**: `WeakRef` e chaves de `WeakMap`/`WeakSet` não marcam. Ephemeron: iterar ponto fixo,
   um valor de WeakMap só é marcado se a chave está marcada; repetir até não marcar nada novo.
   Depois: `WeakRef` com alvo morto vira `Empty`; entradas de WeakMap com chave morta saem;
   `FinalizationRegistry` com alvo morto enfileira o callback como tarefa (holdings são raiz forte
   até lá).
4. **Sweep**: para cada índice não marcado, `CELLS[i] = None` (e remove do `STORAGES`). Como o
   `Rc` do registro era o dono, o `drop` libera. Id velho continua inválido (índice não reaproveitado)
   mas a `Vec` cresce de `None`; trocar por free list com geração no id quando isso pesar.
5. **Rc fortes fora do registro**: células que os campos de uma célula seguram por `Rc` direto
   (structures, executables, scopes `Rc<JSScope>`) não são células JS pelo id; elas somem quando o
   dono é solto, mas **ciclo Rc entre elas vaza** (ex.: scope capturando função que captura scope se
   ambos forem `Rc` diretos, não ids). Auditar: onde dois tipos com `Rc` se apontam, um lado vira id ou
   `Weak`. Esse é o risco principal do desenho, medir antes de prometer.
6. **Quando coletar**: contador de alocações desde a última coleta (`live_cell_count()` mais limite
   adaptativo 2x do pós-coleta), só em pontos seguros (entrada de função/backedge), nunca com
   `DeferGC` ativo (`heap.is_deferred()` já existe).

Custos: marcação O(vivos), sweep O(registro); pausa total (stop-the-world, sem geracional). Esforço
real está em (a) enumeração de raízes e handles e (b) `visit_children` por tipo. Risco: raiz
esquecida vira use-after-free lógico (`get` devolve `None`, panic/`TypeError` estranho), não UB, graças
ao registro seguro: falha barulhenta, ótima para teste.

## 5. Fatia pequena e segura

1. FEITO: `cell_registry::live_cell_count()` (conta entradas preenchidas).
2. FEITO (revisado): sem `size_in_cells()` no `Heap`, seria só repasse; o tamanho se lê direto de
   `cell_registry::live_cell_count()`. `"Bun"` não existe como global no porte (grep em `src` vazio), então
   `Bun.gc` NÃO foi criado (REGRA SUPREMA: o porte espelha o JSC puro). Medido no bun 1.4.2:
   `typeof Bun` é `object`, `typeof Bun.gc` é `function`, `Bun.gc.length` é 1, `typeof Bun.gc(true)` é
   `number`. A coleta fica só na API Rust: `VM::collect_garbage()` (no-op documentado, devolve 0).
   Teste: `tests/heap_live_cells.rs` (crescimento com 10000 objetos; o de lixo cíclico está `#[ignore]`
   até haver coletor). Nada foi compilado nem rodado nesta fatia.
3. Teste de vazamento em `tests/` (marcar `#[ignore]` até haver coletor): cria 1e6 objetos cíclicos,
   chama `gc`, confere `live_cell_count()` próximo do valor inicial. Variante de WeakRef:
   `new WeakRef({}).deref()` após `gc(true)` deve ser `undefined` (golden contra bun).
4. Coletor só com raízes de pilha + globais, sem Weak*; depois ephemerons e FinalizationRegistry.
