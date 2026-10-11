# Auditoria de Atomics e SharedArrayBuffer

## Golden

- `scripts/gen-atomics-golden.js` mede no bun 1.4.2 e gera `tests/golden/atomics_bun.tsv` (3456 programas, saída estável em duas rodadas).
- `tests/atomics_bun_golden.rs` roda o golden (padrão de `function_error_bun_golden.rs`). Não foi compilado nem executado nesta tarefa (sem cargo).
- Cobre: forma do objeto e descritores, todas as operações em todos os tipos inteiros e nos dois buffers, wrap-around, coerção de valor e de índice, ordem de avaliação, buffer detached/redimensionado, tipos inválidos com a mensagem exata, `isLockFree`, `wait`/`waitAsync`/`notify` sem bloquear, `pause`, e SharedArrayBuffer (construtor, `slice`, `grow`, species, subclasses, `structuredClone`).
- O gerador evita `Atomics.wait` com prazo infinito (trava a thread principal no bun e no porte).
- Fatos medidos que o porte precisa cumprir: `Atomics.add(a, -1, 1)` lança `RangeError: accessIndex cannot be negative`; `Atomics.load` em objeto não tipado lança `TypeError: Argument needs to be a typed array.`; `Atomics.wait` em ArrayBuffer comum lança `Typed array for wait/waitAsync/notify must wrap a SharedArrayBuffer.`; `Atomics.add(a, 0, 1n)` em Int32Array lança `Conversion from 'BigInt' to 'number' is not allowed.`.

## Leitura de `src/runtime/atomics_object.rs` contra `upstream/JavaScriptCore/runtime/AtomicsObject.cpp`

Sem divergência óbvia encontrada; nenhuma edição feita no código. Conferido: `validateAtomicAccess` (uint32 rápido, `toIndex`, RangeError), mensagens de `validateIntegerTypedArray` nos dois modos, ordem em `wait` (tipo, `isShared`, índice, valor esperado, prazo), `notify` (índice, count, `isShared` devolve 0), `isLockFree` (`toInt32`), `store` (`toIntegerOrInfinity`, retorna o valor coerçado) e `pause`.

Pontos a vigiar quando o golden rodar:

- `notify` com `count` grande: o C++ satura em `UINT_MAX`; o porte repassa `f64` (infinito). Equivalente, desde que `notify_waiters` trate o `f64` sem converter para inteiro com estouro.
- `Atomics.wait` sem prazo continua `Unported` (o bun trava), por isso o golden não o inclui.
- Mensagem `Atomics.wait cannot be called from the current thread.`: o bun principal permite o wait, e o porte assume sempre permitido, então não há caso.
