//! O subconjunto de `src/bun_core/feature_flags.rs` do upstream que o parser e o printer leem, com os valores do
//! bun 1.4.2. As outras flags são do servidor HTTP, do watcher e do instalador.

/// React will issue warnings in development if there are multiple children
/// without keys and "jsxs" is not used.
/// https://github.com/oven-sh/bun/issues/10733
pub const SUPPORT_JSXS_IN_JSX_TRANSFORM: bool = true;

pub const INLINE_PROPERTIES_IN_TRANSPILER: bool = true;

pub const SAME_TARGET_BECOMES_DESTRUCTURING: bool = true;

pub const RUNTIME_TRANSPILER_CACHE: bool = true;
