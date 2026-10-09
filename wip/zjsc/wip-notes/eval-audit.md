# Auditoria de eval e escopo dinâmico

## Golden novo

- `scripts/gen-eval-golden.js` gera `tests/golden/eval_bun.tsv` (8434 programas, medidos no bun 1.4.2) e
  `tests/eval_bun_golden.rs` o consome (mesmo padrão de `function_error_bun_golden.rs`).
- Cada programa roda como script real (`vm.runInThisContext`, sem o invólucro de módulo do bun), num processo novo, e
  grava o resultado em `globalThis.R`. Programas cujo resultado não sai são descartados (107).
- Passou de "cerca de 1500" porque as famílias se combinam (contextos de eval x corpos, formas de nome x nomes
  reservados, erros de sintaxe via eval e `new Function`). O teste exige no mínimo 1500.
- Não duplica os três goldens existentes: `scope_bun.tsv` cobre TDZ e escopo léxico, `annexb_bun.tsv` cobre Annex B,
  `function_error_bun.tsv` cobre toString e stack. Este cobre eval direto e indireto, `with`, `delete`, conflitos com
  let global, `new Function`, `arguments` mapeado, binding imutável de função nomeada e nomes reservados.
- O teste NÃO foi rodado (regra da tarefa: sem cargo). Quem rodar primeiro deve esperar uma lista longa de divergências
  e triar por família.

## Leitura contra o upstream

- `src/interpreter/execute_eval.rs` (`try_execute_eval`) confere linha a linha com `Interpreter::executeEval`
  (`Interpreter.cpp:1502`): mesma ordem de verificações, mesmas mensagens (`Can't create duplicate variable in eval: '`),
  `canDeclareGlobalFunction`/`canDeclareGlobalVar`, `ensureBindingExists`, hoisting de candidatos Annex B.
- `src/runtime/program_executable.rs` (verificação de declarações globais) confere com `ProgramExecutable.cpp`,
  incluindo `Can't create duplicate variable: 'x'` (`createErrorForDuplicateGlobalVariableDeclaration`) e
  `...that shadows a global property: '`.
- Divergência declarada e aceita: `ensureBindingExists` usa `has_property` no `StrictEvalActivation` (o upstream usa
  `hasOwnProperty`); o objeto embrulhado tem protótipo nulo, então o resultado é o mesmo.
- Nenhuma divergência óbvia encontrada, então nenhuma edição no interpretador. Não li ainda os caminhos de `with` no
  bytecompiler (`ResolveScope`/`with_scope`) nem `JSScope::resolveScopeForHoistingFuncDeclInEval`: próximo passo é rodar
  o golden e ler só o que falhar.
