//! Golden de `TypedArray.prototype` e `ArrayBuffer.prototype` contra o JavaScriptCore do bun:
//! `tests/golden/typedarray_proto_bun.tsv` sai de `scripts/gen-typedarray-proto-golden.js`, rodado no bun 1.4.2. Cada
//! linha é um programa (JSON) e o texto da variável global `R` que ele grava. Cobre `set` com sobreposição de buffer
//! entre tipos, `subarray`, `sort`/`toSorted` com comparadores hostis, `toReversed`/`with`, `fill`/`copyWithin` com
//! índices negativos, `from`/`of`, BigInt64Array, ArrayBuffer redimensionável com length-tracking, detach via
//! `transfer`, Float16Array e `slice`/`resize`/`transfer`/`transferToFixedLength`/`detached` do ArrayBuffer.
//! O prelúdio comum das linhas fica em `tests/golden/typedarray_proto.preludes.json` (ver `tests/common/mod.rs`).

mod common;


const GOLDEN: &str = include_str!("golden/typedarray_proto_bun.tsv");
const PRELUDES: &str = include_str!("golden/typedarray_proto.preludes.json");

/// Pilha da thread do golden, igual à do golden de escopo (comparadores reentrantes de sort e grades de resize).
const THREAD_STACK_BYTES: usize = 256 * 1024 * 1024;

#[test]
fn typedarray_proto_matches_bun() {
    std::thread::Builder::new()
        .stack_size(THREAD_STACK_BYTES)
        .spawn(|| {
            zjsc::runtime::vm::VM::set_thread_stack_budget(THREAD_STACK_BYTES - 16 * 1024 * 1024);
            common::check(GOLDEN, PRELUDES, 800, |source| common::guarded_units(|| common::EvalMode::RunInThisContext.evaluate(source, "", "R")));
        })
        .expect("thread")
        .join()
        .unwrap_or_else(|panic| std::panic::resume_unwind(panic));
}
