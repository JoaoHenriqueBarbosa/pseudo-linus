# Auditoria de módulos ES

## O que existia

- `tests/golden/module_bun.tsv` (461 casos), `scripts/gen-module-golden.js`, `tests/module_bun_golden.rs`.
- Porte: `js_module_record.rs`, `abstract_module_record*.rs`, `js_module_namespace_object.rs`,
  `js_module_environment.rs`, `js_module_loader.rs`, `module_map.rs`, `synthetic_module_record.rs`.
- Entrada de teste: `zjsc::api::module::evaluate_module_map(files, "main.mjs")`.

## O que foi acrescentado

- `scripts/gen-module-more-golden.js` gera `tests/golden/module_more_bun.tsv` (1551 casos medidos no bun 1.4.2;
  o bun só carrega módulos do disco, então cada caso vira um diretório temporário e os caminhos são normalizados).
- `tests/module_more_bun_golden.rs` (mesmo formato e mesma regra de erros da camada do bun).
- Famílias: export default anônimo (name `default`), `export *` e `export * as`, namespace (ordem, toStringTag,
  set/delete/define/setPrototypeOf, TDZ por operação), ciclos (TDZ por forma x acesso, hoisting), ambiguidade e
  binding inexistente, live bindings, `import.meta`, `import()` dinâmico, TLA (ordem, ciclo, rejeição), strict,
  `this`, `await` e palavras reservadas, import attributes JSON, erros de parse, 330 grafos aleatórios
  determinísticos (semente fixa, afinados a ~200).
- Caso removido por não determinismo: módulo que faz `import()` do ponto de entrada (trava, timeout no bun).

## Auditoria contra o upstream

- Lidos `JSModuleNamespaceObject.cpp` contra `js_module_namespace_object.rs`: ordenação por ponto de código,
  `@@toStringTag`, `DontDelete` nos exports, TDZ em `getOwnPropertySlot`, `HasProperty` sem `[[Get]]`, caminho
  `ByIndex`, `getOwnPropertyNames` com leitura do binding no modo `Exclude`, namespace adiado: sem divergência
  óbvia. As mensagens de `js_module_record.rs` (`Export named ... not found`, ambiguidade, `export '...' not
  found in`, `Cannot export ... multiple times`) batem com as medidas.
- Sem divergência óbvia que justificasse Edit. Não foi rodado cargo: a primeira execução de
  `module_more_bun_golden` dirá onde o porte diverge; tratar cada falha pela mensagem do teste.
- Não lidos nesta passada (fora do orçamento): `CyclicModuleRecord.cpp` e `JSModuleEnvironment.cpp` contra o porte.

## Segunda passada: ciclo, TLA e avaliação assíncrona

Lido sem cargo, só leitura. Upstream em `upstream/JavaScriptCore/runtime/` (não existe `builtins/ModuleLoader*.js`
nesta versão: o carregador é C++ e microtarefas internas).

- `CyclicModuleRecord.cpp` (`initializeEnvironment`, `link`, `evaluate`, `execute`, `executeAsync`,
  `gatherAvailableAncestors`, `asyncExecutionRejected`, `asyncExecutionFulfilled`), `AbstractModuleRecord.cpp`
  (`innerModuleLinking`, `innerModuleEvaluation`, `importPromiseGatesAsyncDependency`), `JSModuleRecord.cpp`
  (`evaluate`, `execute`) e `JSMicrotask.cpp` (`asyncModuleResolveEvaluation`, `asyncModuleExecutionResume`) contra
  `js_module_record.rs` (linhas 690 a 1450) e `js_microtask.rs` (`async_module_resolve_evaluation`): passo a passo
  iguais, incluindo a extensão do bun (`depInOuterSCC`, `referrerAsyncOrder`, `dynamicImportPromise`), a ordenação
  de `execList` por `asyncEvaluationOrder`, o laço de trabalho em `asyncExecutionRejected` (pais em ordem inversa),
  a guarda `cycleRoot` nulo + `evaluationError` em `gatherAvailableAncestors`, e o `fastAsyncGeneratorSentinel`.
- Lacunas conhecidas e declaradas no porte (não divergência): `attachErrorInfo` (só decora o erro para o host),
  `unwrapContext` do `InternalFieldTuple` em `importPromiseGatesAsyncDependency` (o porte não tem contexto assíncrono,
  então o contexto nunca é tupla), ramo `WebAssemblyModuleRecord`.
- `initializeEnvironment`: o ramo dos `Indirect` e as mensagens por `Resolution` (inclusive o "Did you mean to
  import default?" e "Missing 'default' export") ficaram em `js_module_record.rs` e `abstract_module_record_resolve.rs`
  já conferidos na passada anterior; o passo de `var`/função e `import.meta` (linhas 640 a 685) segue o upstream.
- `JSModuleEnvironment.cpp` contra `js_module_environment.rs`: não relido por completo no orçamento; a leitura
  parcial não achou desvio, mas continua pendente a conferência de `create` com `jsTDZValue` e do `moduleRecord`.

Simulação à mão (20+ programas de `module_more_bun.tsv`, todos batem com o golden pelo algoritmo do porte):

- Irmãos com `await` (linhas 775, 781, 787, 793, 799, 805, 811): ordem `a1,b1,a2,b2,main` e variantes pelo número
  de `await` de cada irmão; `main` só roda quando `pendingAsyncDependencies` chega a 0.
- TLA atrás de `export *` (822, 828): `b` assíncrono deixa `a` e `main` com pendência; em `asyncExecutionFulfilled`
  o `execList` sai `[a, main]` por ordem de `asyncEvaluationOrder` e `L(v)` imprime `bv`.
- Dependência assíncrona compartilhada (834, 840, 846): `d` com TLA importado por `a` e `b`; `d.parents=[a,b]`,
  saída `d1,d2,a,b,main`.
- Rejeição propagada (853, 860, 866, 874, 880): exceção síncrona em irmão deixa `main` com `evaluationError` e
  `cycleRoot` vazio, e o `gatherAvailableAncestors` posterior pula `main` pela guarda; TLA que rejeita propaga por
  `asyncExecutionRejected` até a cápsula de topo; segundo `import()` do módulo rejeitado devolve a mesma promessa
  (`cycleRoot` próprio com `topLevelCapability`), `c1 E, c2 E`.
- Ciclo com TLA e TDZ (1297): `c -> a -> b -> c`; `b` ganha ordem 1 e executa, `a` fica com pendência 1 (a checagem
  de `asyncEvaluationOrder` vale também para dependência `Evaluating`), `c` ordem 3; ao terminar `b`, `execList`
  `[a, c]`: `b c ReferenceError, b start, b end, a start, a end, c sees a=a, ...` igual ao golden.
- Erro em folha com TLA no meio (1279, 1285): o grafo para na exceção de `e`; `b` nunca executa, `d start,d end,e start`.
- `import()` do próprio módulo em avaliação (766): a avaliação do `import()` roda em microtarefa, quando `a` já está
  `Evaluated`, então o `debug_assert` de `cyclic_evaluate` não é violado e `self` é impresso.
- Não simulados por dependerem da contagem de ticks do carregador (não do algoritmo cíclico): 751 (`o1`/`o2`
  concorrentes por `import()`); a medição é do `module_more_bun_golden`.

Resultado: nenhuma divergência óbvia, nenhum Edit em código.

Ampliação do golden (2026-10-08, `scripts/gen-module-more-golden.js`, seção 15): o `module_more_bun.tsv` foi
regerado inteiro (1736 casos; o arquivo versionado estava defasado em relação ao gerador) e ganhou:

- `import defer * as ns` (o bun aceita; `import defer { x }`, `import defer x`, `export defer *` e `import.defer()`
  entram como erro de parse medido): `typeof ns` e `toString` não avaliam o módulo, `Object.keys`, `in`,
  `ownKeys`, `ns.x`, `ns.nope` avaliam na primeira operação, `@@toStringTag` é `Deferred Module`, dependência
  assíncrona é avaliada antes de `main`, erro de avaliação repete a cada acesso.
- `import source` (o bun rejeita: `Expected "from"`; só `log` é comparado, o erro é da camada do bun).
- `import.meta.url` com subdiretórios, `?query` e `#hash` (módulos distintos por query), nomes string em
  `export`/`import` (`"a-b"`, `""`, `"0"`, `__proto__`, `then`, escapes `ab` duplicando `ab`), `export * as "x"`.
- JSON modules: vazio, BOM, comentário, vírgula sobrando, duplicata, número grande, `-0`, `__proto__`, `default`
  como chave, identidade entre `import` estático e `import()`, mutação do objeto compartilhado.
- Removidos do gerador (medição não determinística ou fora do JSC): `import()` do `main` por um filho com TLA (o bun
  trava e o runner dá timeout) e `Object.getPrototypeOf(deferredNs)` (o bun devolve um objeto com `__esModule`,
  camada do bun; o namespace comum devolve `null`).
- Não rodei o teste Rust (regra da tarefa). Casos novos mais prováveis de divergir: `import defer` (ordem da
  avaliação lazy, `[[OwnPropertyKeys]]` avaliando, `t.mjs` com TLA avaliado antes de `main`) e nomes string com
  escape no `export { q as 'a\x62' }`. Rodar `cargo test --test module_more_bun_golden` e conferir.

Nota: o `tsv` saiu do gerador por redirecionamento de stdout (codegen, exceção da regra Write/Edit).
