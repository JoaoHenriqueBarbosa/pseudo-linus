//! Mensagens de erro compartilhadas de `runtime/JSObject.cpp` (`ReadonlyPropertyWriteError` etc.).

/// `JSObject.cpp:70`: `const ASCIILiteral ReadonlyPropertyWriteError`.
pub const READONLY_PROPERTY_WRITE_ERROR: &str = "Attempted to assign to readonly property.";
