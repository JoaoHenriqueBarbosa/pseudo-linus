//! O módulo `math` do `bun_ast` do upstream (`src/ast/nodes.rs`, "from bun_jsc::math"). No upstream o `pow` é a FFI
//! `Bun__JSC__operationMathPow`, que chama o `operationMathPow` do JavaScriptCore; aqui é o mesmo `operationMathPow`
//! em Rust, que o zjsc também usa, então mora em `ul_common::libm` e este módulo só o reexporta com o nome do upstream.

/// `Math.pow` do JavaScriptCore (`operationMathPow`): os casos de NaN, ±∞ e ±0 que o `powf` do Rust trata de outro
/// jeito, e o `pow` do glibc no resto.
pub use ul_common::libm::operation_math_pow as pow;
