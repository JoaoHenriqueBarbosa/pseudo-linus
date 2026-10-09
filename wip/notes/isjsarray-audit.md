# Auditoria de `JSArray::from_value` / `from_cell_id` (estrito vs por classe)

Regra: `from_*` é `isJSArray` (só `ArrayType`); `from_*_by_class` é `jsDynamicCast<JSArray*>`/`inherits<JSArray>`/`downcast<JSArray>` (`ArrayType` ou `DerivedArrayType`, o `Array.prototype`).

| Rust (arquivo:linha) | C++ correspondente | Veredito |
|---|---|---|
| runtime/iterator_operations.rs:300 | IteratorOperations.cpp:505/631 `isJSArray(iterable)` (FastArray) | estrito, ok |
| bytecode/array_allocation_profile.rs:45,49 | ArrayAllocationProfile `m_lastArray` (JSArray recém-criado) | estrito, ok |
| runtime/js_array_iterator.rs:165,185 | JSArrayIteratorInlines.h:36 `downcast<JSArray>(iteratedObject())` | DIVERGIA, corrigido para `from_value_by_class` |
| runtime/js_array_iterator.rs:228 | teste | ok |
| runtime/json_host.rs:171 | `toLength` (JSArrayInlines.h:216 `isJSArray`) | estrito, ok |
| runtime/array_prototype.rs:194 | `toLength` (JSArrayInlines.h:216) | estrito, ok |
| runtime/array_prototype.rs:279, 307, 408 | caminhos rápidos do Rust (`isJSArray` no C++: ArrayPrototype.cpp:836 etc.); o genérico cobre o `Array.prototype` | estrito, ok |
| runtime/array_prototype.rs:366 | ArrayPrototypeInlines.h:147 `setLength` com `isJSArray(obj)` (o resto vai a `put("length")`) | estrito, ok |
| runtime/array_prototype.rs:790 | ArrayPrototype.cpp:546 `isJSArray(thisValue)` (pop) | estrito, ok |
| runtime/array_prototype.rs:809 | ArrayPrototype.cpp:584 `isJSArray(thisValue)` (push) | estrito, ok |
| runtime/array_prototype.rs:925 | sem equivalente direto; o RangeError vem de `put("length")`. `Array.prototype` só ultrapassa 2^32 se o length for posto, e isso já lança | estrito, ok |
| runtime/array_prototype.rs:1205 | otimização do porte (sem C++); estrito só desliga a otimização | ok |
| runtime/array_prototype.rs:2001 | resultado recém-criado de `toSorted` | ok |
| runtime/structured_clone.rs:351 | SerializedScriptValue usa `isJSArray` (WebCore não está em upstream/; não verificável aqui) | estrito, mantido |
| runtime/js_promise_combinators_context.rs:104 | `values` criado pelo próprio combinador | ok |
| runtime/string_regexp_support.rs:66,79 | caminho rápido do porte; `isJSArray` no C++ | estrito, ok |
| runtime/promise_constructor.rs:871 | JSPromiseConstructor.cpp:191 `isJSArray(iterable)` | estrito, ok |
| runtime/js_array.rs:312, 947 | definição de `isJSArray` | ok |
| llint/varargs.rs:186 | Interpreter.cpp:375 `isJSArray(object)` e `toLength` | estrito, ok |
| llint/dispatch_ext.rs:427 | LLIntSlowPaths.cpp:913 `isJSArray(baseValue)` | estrito, ok |
| llint/handlers_iterator.rs:290 | CommonSlowPaths.cpp:842 `downcast<JSArray>(...)` com `ASSERT(isJSArray)` | DIVERGIA no release, corrigido para `from_value_by_class` |
| llint/handlers_iterator.rs:400,411 | getIterationMode: `isJSArray(iterable)` | estrito, ok |
| llint/handlers_iterator.rs:430 | `uncheckedDowncast<JSArray>(arrayResult)`, array recém-criado | ok |
