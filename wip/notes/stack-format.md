# Formato de `Error.stack` contra o bun (golden `stack_format_bun`)

Medição `/tmp/now4_stack_format_bun_golden.txt`: 287 de 511 divergências.

## Padrões

| Padrão | Casos | Causa |
|---|---|---|
| Frame do programa saía `at <anonymous> (arq:l:c)`, o bun imprime `at arq:l:c` | 244 | `StackFrame::display_name` em `src/runtime/stack_frame.rs` mapeava `global code` para `<anonymous>` e sempre embrulhava em `nome (local)` |
| Coluna +9 em `getColumnNumber`/`toString` de `var e = new Error('x')` dentro de `prepareStackTrace` | 14 | ver "Aberto" |
| `Error.stackTraceLimit = 4` com recursão `return rec(n - 1)` em modo estrito dá 3 linhas, o bun 5 | 2 | ver "Aberto" |
| `JSON.stringify({ toJSON() { return new Error('x').stack } })`: esperado `""` (R não definido), veio a pilha | 1 | ver "Aberto" |
| Demais (aprox. 26) | | não classificadas dentro do prazo |

Nenhum padrão `at ` vs `@`, filename ou frame async novo: todas as linhas continuam `at`, o nome do arquivo bate.
As edições recentes em `error_info.rs` / `error_messages.rs` (único diff tracked na lista pedida) não tocam a pilha;
`stack_frame.rs`, `unwind.rs` e `stack_visitor.rs` são arquivos novos (não rastreados), sem diff contra o HEAD.

## Correção aplicada

`StackFrame::to_call_site_string`: o frame com `function_name == "global code"` (código de programa) sai só com o
local, `at arq:l:c`. `eval code` e função sem nome continuam `<anonymous> (...)` (o golden confirma:
`at <anonymous> (file:///error_stack_case.js:1:10)` + `at eval (unknown)`). Testes unitários do arquivo ajustados.
Não rodei cargo (restrição da tarefa): confirmar com o golden.

## Coluna +9 (corrigido, sem rodar cargo)

A causa não estava no divot do `NewExprNode` nem no `AssignResolveNode` (o `Parser.cpp` dá `divot = expressionEnd`, o `(`, nos dois
casos, e o `emitBytecode` do `AssignResolveNode` não toca na informação de expressão do `op_construct`). Está no `Bun`:
`src/jsc/bindings/ErrorStackFrame.cpp::getAdjustedPositionForBytecode` recua a posição do `CallSite` pelo `startOffset` do
`ExpressionInfo` quando a instrução é `op_construct`, `op_construct_varargs`, `op_super_construct` ou `op_super_construct_varargs`
(do `(` para o `new`). CORREÇÃO (2026-10-08, medida no bun): a afirmação seguinte, de que o texto de `Error.stack` fica no `(`, está ERRADA. O texto de `stack`
mostra o começo do nome do callee (`o.T()` em `T`, `T()` em `T`, `f()()` no `)` do primeiro `()`, `o[k]()` em `k`, `(o.p)()` em `p`,
`o.a[ 0 ]()` em `0`, `o.a[(0)]()` em `0`, `o.a[o.a.length-1]()` em `1`, `o.a[0+0]()` em `0`, `o  .  p   ()` em `p`, com espaços e quebra de
linha entre as partes). `getAdjustedPositionForBytecode` NÃO existe no `upstream/JavaScriptCore` (é código do fork do bun): a regra
exata não está no fonte disponível. Ver também o relatório do porte: `callee_name_back_offset` é heurística e sai quando a regra for derivada.
Texto antigo: A linha de texto de `Error.stack` continua no `(`, por isso `return new Error().stack` bate e
`getColumnNumber`/`toString` do `CallSite` não batiam. Os 14 casos "coluna +9" eram todos `CallSite`.

Correção: `StackFrame::construct_back_offset` (novo, 0 fora de construção), preenchido em `Interpreter::stack_frame_for`
(`interpreter/unwind.rs`, `construct_back_offset`), e `StackFrame::call_site_column` usado por `call_site_column_number`,
`call_site_json_fields` e `call_site_text`; `to_call_site_string` (texto de `stack`) segue sem recuo. Nota: apliquei a edição de
`stack_frame.rs` por script (Bash) por engano; o resto foi por Edit.

## Recursão com cauda (`stackTraceLimit`), resolvido: era artefato do harness

A hipótese anterior ("cauda recursiva não colapsa") está ERRADA. Medido no bun com `vm.runInThisContext` em modo estrito: a
cauda colapsa sempre, recursiva ou não (`rec(8)`, `k -> g -> f`, `a <-> b`, método `o.rec`): só sobram `rec` e o código global.
O `rec(n - 1)` sem cauda (`var r = rec(n-1); return r`) mostra todos os frames. Os "5 linhas" do golden com limite 4 eram
`rec`, o código do programa, `runInThisContext` e `go` do runner do gerador (`scripts/gen-stack-format-golden.js`): o limite
truncava no runner, e o teste `depth 0 vs 3` não detecta isso. O porte roda o fonte direto, sem esses frames, então 3 linhas é o certo.
Correção: o gerador pula `LIMIT = 4` nas duas sondas `rec` de cauda e as duas linhas saíram de `tests/golden/stack_format_bun.tsv`
(`sed`, arquivo gerado). `is_in_tail_call` não mudou, o caso `g -> f` segue colapsando.

## Classificação do restante (sobre o golden antigo, depois de descontar o padrão `<anonymous>` já corrigido)

| Grupo | Casos | Exemplos |
|---|---|---|
| Nome de função inferido faltando (`<anonymous>` no lugar de chave computada, símbolo, getter/setter, `name` definido) | ~14 | `[Symbol.iterator]`, `[d]`, `get [d]`, `dyn`, `[sy]`, `renamed`, `comp` |
| Frames de função nativa mais fundos ausentes (`repeat`, `defineProperty`, `Symbol`, `BigInt`, `set`, `construct`, `Proxy`, `decodeURIComponent`, `resolve`, `parse` com `<parse> (:0)`) | ~11 | erros lançados por builtins nativos |
| Construtor padrão derivado (`new B (unknown:1:28)`), `new Function`/`eval` com `anonymous (file:///...:3:17)` | ~6 | |
| Cabeçalho / `name` / `message` do erro (`captureStackTrace` com `Error` fixo, subclasse de `AggregateError`, `cause`) | ~12 | |
| `CallSite` em `prepareStackTrace` (índices 792-828) | ~8 | |
| `toJSON` retornando `new Error().stack` (esperado vazio) | 1 | |
| `new <anonymous>` de classe anônima (esperado `<anonymous>`) | 1 | |

Os dois primeiros grupos foram corrigidos (sem rodar cargo, confirmar com o golden):

- **Nome inferido.** O C++ (`StackFrame::functionName`) não usa o `inferredName` do `CodeBlock` e sim o callee:
  `getCalculatedDisplayName`; o bun (`ErrorStackTrace.cpp functionName(vm, global, object)`) tenta antes a propriedade própria
  `name` de dados. `JSFunction::stack_frame_name` (novo, `runtime/js_function_reify.rs`) faz isso: `name` reificada
  (`defineProperty`, `setFunctionName` de chave computada/símbolo) ou a preguiçosa (`originalName`, com `get `/`set `), e senão
  `calculated_display_name`. `Frame::stack_function_name` (`interpreter/stack_visitor.rs`) o usa para frame de função e cai no
  `function_name` sem callee função; `stack_frame_for` passou a chamá-lo.
- **Frames nativos no topo.** `get_stack_trace` pulava todo frame nativo de índice 0 (o do construtor `Error`), e o `unwind`
  captura a exceção no frame JS, depois de a função de host sair da pilha. Agora `Interpreter::invoke_native` captura a pilha da
  exceção logo ao voltar da função de host, com o frame dela ainda de pé (`VM::throwException` do C++), e passa
  `include_top_native = true` (novo parâmetro de `get_stack_trace`, `capture_frames` e `capture_stack_for_exception`), o que dá
  `at repeat (unknown)`, `at defineProperty (unknown)`. Construtores de erro e `captureStackTrace` passam `false`.
  `capture_stack_for_exception` agora recebe `&JSGlobalObject`.

### `<parse> (:0)` (corrigido, sem rodar cargo)

Medido no bun 1.4.2: não é frame de `JSON.parse`. TODO `ErrorInstance` de tipo `SyntaxError` abre o texto de `stack` com
`    at <parse> (:0)`, inclusive `new SyntaxError('x')`, subclasse de `SyntaxError`, `Error.captureStackTrace(syntaxError)`, `eval`,
`new RegExp('(')`, `BigInt('x')`, `JSON.parse`. A linha fica FORA do `stackTraceLimit` (limite 1 dá `<parse>` mais 1 frame), não
aparece na lista de `CallSite` do `prepareStackTrace` (é só texto) e não vale para `captureStackTrace` num objeto comum. `new Function`
mostra `(:N)` com a linha do erro no fonte embrulhado (4 para `{`, 5 para `let a;\nlet a`, 2 para erro na lista de parâmetros),
que o porte antes não guardava. Correção: `stack_text(header, frames, parse_line: Option<i32>)` em `error_instance.rs`.

Linha do `new Function` (corrigido, sem rodar cargo): medido no bun, só `new Function` tem N diferente de 0 (`new Function("a b")` dá 3,
`"\n\na b"` dá 5, `("a","b","return +")` dá 4); `eval` (mesmo na linha 3), `new RegExp('(')` e `JSON.parse` dão 0. N é a linha do
erro no fonte `(function anonymous(\n) {\n...\n})`, a mesma que `ParserError::toErrorObject` grava via `addErrorInfo`. Só
`UnlinkedFunctionExecutable::fromGlobalCode` (`bytecode/unlinked_function_executable.rs`) a copia para `ErrorData::parse_frame_line`
(`ErrorInstance::set_parse_frame_line`); `eval` também grava `line` no erro mas não esse campo, por isso fica `(:0)`.
Confirmar com o golden.

### `Symbol`/`BigInt`/`Proxy` (corrigido, sem rodar cargo)

Bun: `at Symbol (unknown)` (de `new Symbol()`), `at BigInt (unknown)` (de `BigInt(1.5)`, `new BigInt(1)`, `BigInt('x')` depois
do `<parse>`), `at Proxy (unknown)` (de `Proxy({}, {})`). O nome já saía certo por `InternalFunction::calculated_display_name`. O que
faltava era `new Symbol()`: `handle_host_call` (`llint/dispatch.rs`) lançava o erro por um atalho antes de pôr o callee no frame e sem
capturar a pilha com o frame nativo; agora põe o callee e chama `capture_stack_for_exception(.., include_top_native = true)`.

Nenhum dos demais grupos foi alterado: precisam de medição própria.

## Medição `/tmp/now6_stack_format_bun_golden.txt`: 56 de 509 divergem

Classificação por padrão:

| Padrão | Casos | Estado |
|---|---|---|
| Acessor (`get g`/`set g`, estático ou literal) sai com prefixo, o bun imprime só `g` | 7 | corrigido em `JSFunction::stack_frame_name` (tira `get `/`set `), sem rodar cargo |
| `with`, `new Promise(cb)`, `Promise.resolve().then(cb)` e os 22 casos de `CallSite` dentro de `eval`/`new Function` com aspas simples aninhadas: esperado `<undefined>` e "o programa lançou exceção" | ~25 | esperado e obtido são ambos "lançou"; o golden compara mal (ou o `with`/Promise lançam de verdade no porte). Investigar o gerador antes de mexer no motor |
| `prepareStackTrace` retornando string: esperado `""` (o bun descarta o retorno string de `cs.map(...).join`) e veio o texto | 10 | aberto: o bun só usa o retorno se `prepareStackTrace` foi definido antes de o erro nascer; o porte aplica na leitura |
| Default constructor derivado `new B (unknown:1:28)` e `new Function` `anonymous (file:///arq:3:17)` (frame sintético com URL/linha) | 6 | aberto: frame precisa de `source_url` sintético |
| `new <anonymous>` de função anônima chamada com `new`: esperado `<anonymous>` | 1 | corrigido em `format_call_site` |
| Frame nativo sem nome (Proxy chamado): esperado `at unknown` | 1 | corrigido em `format_call_site` |
| Artefatos do teste e do gerador (`<undefined>`, `prepareStackTrace` devolvendo string, frames sintéticos de `new Function` e do construtor derivado) | ~41 | corrigido (sem rodar cargo, confirmar com o golden): os ~25 `<undefined>` eram do teste: `R` nunca gravado dá `ReferenceError` na leitura solta de `R`, e o runner do gerador engole a exceção e lê `globalThis.R`. `tests/stack_format_bun_golden.rs` passou a usar `evaluate_script_sequence_result(.., "globalThis.R")`. Os 10 de `prepareStackTrace` devolvendo string eram artefato do gerador: o filtro descartava a linha inteira por conter `runInThisContext` (o `join('|')` junta os frames do programa e do runner numa linha só), então o esperado `""` era falso. O bun consulta o gancho na primeira leitura de `stack` (medido: `n` 0 na criação, 1 na leitura; erro criado antes do gancho também o usa), igual ao porte. Gerador corrigido (`stripRunnerLine`) e 9 linhas do TSV regravadas com a medição do bun; a 10ª (`isAsync`) já tinha esperado correto (`false|true`), segue aberta. Frames sintéticos: `new Function` passa a levar a URL da origem do chamador como `sourceURL` (`construct_function_for_caller`), o construtor padrão derivado sai com `unknown` (`stack_frame_for`, o fonte é `(function (...args) { super(...args); })`, `super(` na coluna 28), e `CallSite.toString` sempre escreve a coluna (`:3:1`), só o texto de `stack` omite a coluna 1. |
| `cause`: o bun anexa a pilha da causa à de `stack` do erro externo | 2 | corrigido: era artefato do gerador (ver seção abaixo), o bun NÃO anexa a causa |
| `captureStackTrace(o, fn)` com `fn` nativo/`eval`/gerador, `Promise.any` vazio (frame `any (unknown)`), `isAsync` | 6 | corrigido (ver seção abaixo) |
| `toJSON` com `new Error().stack` (esperado vazio) | 1 | corrigido: artefato do gerador (ver seção abaixo) |
| Cabeçalho `AggregateError` de subclasse (`Error: m`) | 1 | corrigido: `sanitizedNameString` (ver seção abaixo) |

## Medição `/tmp/now8_stack_format_bun_golden.txt`: 11 de 509 divergem

Nove são das famílias com outro agente (`cause` 2, `captureStackTrace(o, fn)` 3, `Promise.any` 1, `isAsync` 1, `toJSON` 1, `AggregateError` de subclasse 1). Os outros dois, acessor de chave computada (`get [d]`, `get dyn`), foram corrigidos (sem rodar cargo, confirmar com o golden): o bun tira o `get `/`set ` só do acessor de chave literal (nome preguiçoso); o de chave computada tem o `name` reificado por `setFunctionName` e imprime com o prefixo. `JSFunction::stack_frame_name` só remove o prefixo quando o nome não foi reificado.

## As seis famílias abertas (sem rodar cargo, confirmar com o golden)

Tudo medido no bun 1.4.2.

- **`cause` e `toJSON` eram artefatos do gerador.** O bun não anexa a pilha da causa (`e.stack` do `new Error('in', { cause })` tem só os frames do próprio erro).
  O `e.stack + '||' + e.cause.stack` terminava a pilha externa com um frame do runner, e o `||TypeError: c` caía na mesma linha, que o
  `stripRunnerLine` descartava inteira; o `JSON.stringify` de uma pilha também vira uma linha só com `runInThisContext`, daí o `""`. Os
  três programas passaram a separar com `'\n||'` e a ler `JSON.parse(JSON.stringify(...))`; três linhas do TSV regravadas com a saída do
  gerador (diff do TSV inteiro: só essas três, mais uma linha que o gerador descarta de forma instável).
- **`Promise.any` vazio.** O `AggregateError` criado dentro do `Promise.any` tem o frame nativo no topo (`at any (unknown)`), igual a `repeat`
  num erro lançado por função de host. `HostCall::capture_stack_frames_with_native` (novo) e o parâmetro `frames` de
  `promise_constructor::create_aggregate_error`, preenchido em `Combinator::finish` (caminhos rápido e lento). Os rejeitadores de elemento
  rodam numa microtask e continuam sem pilha: `e.stack` é `undefined` com todas rejeitadas, como no bun.
- **`isAsync`.** `StackFrame::is_async`, marcado nos frames de `get_async_stack_trace`; `CallSite.isAsync` o lê.
- **Cabeçalho de subclasse.** `ErrorInstance::sanitizedNameString` (upstream `ErrorInstance.cpp`) procura `name` só no objeto e no protótipo
  direto (2 níveis, só valor, não acessor), e `sanitizedMessageString` só no objeto. Portanto `class T extends TypeError {}` e
  `class A extends AggregateError {}` dão `Error: m` (o `name` de `TypeError.prototype` está a 3 níveis), `new TypeError('m')` dá
  `TypeError: m`, `T.prototype.name = 'XX'` dá `XX: m`. `error_header_of_object(.., invoke_getters = false)` faz essa busca.
- **`captureStackTrace(o, fn)`** (`Interpreter::get_stack_trace`, `unwind.rs`): (1) o frame nativo de partida nunca casa com `caller`
  (`captureStackTrace(o, Error.captureStackTrace)` dá `Error` sem frames, antes casava no próprio frame de partida e deixava passar tudo);
  (2) `fn = eval`: a função nativa `eval` não tem frame no percurso, vem logo depois do frame de código de `eval`, então o código de `eval`
  casa se `caller` é o `eval` do realm; (3) `fn` gerador/async: o frame em execução tem como callee o corpo (`@generatorNext`), outra função
  que o wrapper `f` cria, então `is_body_of_wrapper` aceita o callee cujo executável de corpo está entre as funções filhas do código de `f`.
