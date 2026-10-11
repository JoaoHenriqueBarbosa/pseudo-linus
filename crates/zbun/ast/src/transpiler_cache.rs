//! Runtime transpiler cache.
//!
//! No upstream a struct fica no `bun_ast` para o parser (`Features.runtime_transpiler_cache`) e o printer a
//! nomearem, e o `get`/`put` de verdade vêm do `bun_jsc` por um `link_interface!` (`r#impl`), com a entrada em
//! disco guardada sem tipo em `entry: Option<*mut ()>`. O porte não tem o `bun_jsc` nem o cache em disco: o
//! despacho é o caso `r#impl == None` do upstream (cache desligado, como no build wasm ou com
//! `--no-transpiler-cache`), e saem os campos `entry` e `r#impl`, que só esse despacho lê.

use crate::{ExportsKind, Source};

pub struct RuntimeTranspilerCache {
    pub input_hash: Option<u64>,
    pub input_byte_length: Option<u64>,
    pub features_hash: Option<u64>,
    pub exports_kind: ExportsKind,
    /// Set by `put()` when there is transpiled output to keep.
    pub output_code: Option<Box<[u8]>>,
}

impl Default for RuntimeTranspilerCache {
    fn default() -> Self {
        Self { input_hash: None, input_byte_length: None, features_hash: None, exports_kind: ExportsKind::None, output_code: None }
    }
}

impl RuntimeTranspilerCache {
    /// Sem o despacho do `bun_jsc` nunca há acerto, como no upstream com `r#impl == None`. Os parâmetros ficam os do
    /// upstream (menos as `parser_options` sem tipo, que só o despacho lia).
    pub fn get(&mut self, _source: &Source, _used_jsx: bool) -> bool {
        false
    }

    pub fn put(&mut self, output_code: &[u8], _sourcemap: &[u8], _esm_record: &[u8]) {
        if self.input_hash.is_none() {
            return;
        }
        self.output_code = Some(Box::<[u8]>::from(output_code));
    }
}

#[cfg(test)]
mod tests {
    use super::RuntimeTranspilerCache;

    #[test]
    fn put_keeps_output_only_with_input_hash() {
        let mut cache = RuntimeTranspilerCache::default();
        cache.put(b"x", b"", b"");
        assert!(cache.output_code.is_none());
        cache.input_hash = Some(1);
        cache.put(b"const x = 1;\n", b"", b"");
        assert_eq!(cache.output_code.as_deref(), Some(&b"const x = 1;\n"[..]));
    }
}
