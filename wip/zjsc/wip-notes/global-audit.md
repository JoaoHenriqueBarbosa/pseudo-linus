# Auditoria de semântica de globais

## Golden novo

- `scripts/gen-global-semantics-golden.js` gera `tests/golden/global_semantics_bun.tsv` (4273 programas, medidos no bun 1.4.2, cada um rodado como script via `vm.runInThisContext` em processo novo). Um programa é uma lista de scripts separados por `\n/*--script--*/\n`; o resultado é `<erros por script>#<R>`.
- 13 linhas levam a marca `bunonly` (protótipo do global, `Symbol.toStringTag`, `toString` do global, `isExtensible`, `constructor.name`): o teste Rust as ignora. Medido no bun: `Object.prototype.toString.call(globalThis)` é `[object Object]` e `Symbol.toStringTag in globalThis` é `false`; o protótipo direto do global é `Object.prototype`.
- `tests/global_semantics_bun_golden.rs` roda cada programa com `zjsc::api::eval::evaluate_script_sequence_result` (função nova em `src/api/eval.rs`: vários `evaluate` no mesmo realm, erro de um script não impede o seguinte).
- Cobertura: var/function/let/const/class contra 27 sondas (A), todos os pares de redeclaração em um script, dois scripts e eval direto/indireto (B), let/var/function sobre propriedades existentes e definidas (C), atribuição/delete/leitura em 15 estados iniciais, sloppy e strict (D), `undefined`/`NaN`/`Infinity` (E), TDZ (F), `this`/`globalThis`/descritores (G), `with` (H), Annex B (I), ordem entre scripts, preventExtensions/seal/freeze, protótipo do global (J), getters e setters no global (K), script de topo misto.
- Medido (vale conferir quando o teste rodar): `var x; let x` em scripts distintos dá `Can't create duplicate variable that shadows a global property: 'x'`, não a mensagem curta; a curta (`Can't create duplicate variable: 'x'`) é para let sobre let.
- Não rodei cargo (regra da tarefa): o teste nunca foi compilado nem executado.

## Leitura contra o upstream

`src/runtime/program_executable.rs` (initialize_global_properties) confere com `ProgramExecutable.cpp` passo a passo: checagem de let/const/class contra o registro léxico (incluindo `allowRedeclaringSymbols` e a exceção de const em sloppy), `hasRestrictedGlobalProperty`, var contra léxico, `canDeclareGlobalFunction`, `canDeclareGlobalVar` com estrutura não extensível, Annex B (B.3.2.2), criação de bindings e `bumpGlobalLexicalBindingEpoch`. `js_global_lexical_environment.rs` (`is_empty`, `is_const_variable`) e `can_declare_global_function` em `js_global_object.rs` também conferem. Nenhuma divergência óbvia encontrada, nenhum código de runtime alterado.

Pendente: as slow paths de `resolve_scope`/`put_to_scope` (`llint/slow_paths.rs`) só foram localizadas, não lidas linha a linha; o golden novo é o instrumento para achar divergência ali assim que rodar.
