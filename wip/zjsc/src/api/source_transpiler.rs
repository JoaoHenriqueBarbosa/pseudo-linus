//! O ponto de encaixe do transpilador de fonte: o bun reimprime todo arquivo JavaScript e TypeScript pelo impressor
//! dele antes de entregá-lo ao JSC, e remapeia as posições de `Error.stack` pelo mapa que o impressor emite. O zjsc
//! não conhece o transpilador (que vive em `wip/zbun/transpile`): o hospedeiro instala um [`SourceTranspiler`] e o
//! carregador de arquivos (o principal, os módulos ES e o `require` de CommonJS) passa o fonte por ele. O texto
//! reimpresso vira o fonte do `SourceProvider` (é o que `Function.prototype.toString` e as mensagens com trecho do
//! fonte citam) e o mapa de posições (`PositionMap::from_segments`) leva as posições de volta ao fonte original.
//!
//! Sem transpilador instalado nada muda: o fonte segue direto ao motor, como nos goldens que já guardam o texto
//! reimpresso do bun.

use std::cell::RefCell;
use std::rc::Rc;

use crate::parser::position_map::{MapSegment, PositionMap};

/// O que o transpilador devolve para um arquivo.
pub struct TranspiledSource {
    /// O texto que o motor recebe. Para CommonJS é o programa inteiro, com o cabeçalho do wrapper na primeira linha.
    pub text: String,
    /// O mapa do impressor, ordenado por posição gerada.
    pub segments: Vec<MapSegment>,
    /// O transpilador classificou o arquivo como CommonJS: `text` é a função de wrapper, não um módulo ES.
    pub is_commonjs: bool,
}

/// O transpilador que o hospedeiro instala.
pub trait SourceTranspiler {
    /// Transpila o arquivo `path` (o loader sai da extensão). `None` quando o caminho não é de JavaScript nem de
    /// TypeScript (o texto segue como está); `Some(Err)` quando o fonte não parseia (o motor o recebe cru e relata
    /// o próprio `SyntaxError`).
    /// `compile_body`: o texto é o corpo de `module._compile`, que o chamador embrulha (sem função de wrapper no
    /// texto devolvido).
    fn transpile(&self, path: &str, source: &str, compile_body: bool) -> Option<Result<TranspiledSource, String>>;
}

thread_local! {
    static TRANSPILER: RefCell<Option<Rc<dyn SourceTranspiler>>> = const { RefCell::new(None) };
}

/// Instala (ou, com `None`, tira) o transpilador da thread: o motor roda numa thread só.
pub fn install_source_transpiler(transpiler: Option<Rc<dyn SourceTranspiler>>) {
    TRANSPILER.with(|slot| *slot.borrow_mut() = transpiler);
}

/// O resultado de [`transpile_source`]: o texto, o mapa de posições e se é CommonJS.
pub struct Transpiled {
    pub text: String,
    pub map: Rc<PositionMap>,
    pub is_commonjs: bool,
}

/// Passa `source` do arquivo `path` pelo transpilador instalado. `None` sem transpilador, para o que ele não trata e
/// para o fonte que não parseia: o chamador segue com o texto original.
pub fn transpile_source(path: &str, source: &str, compile_body: bool) -> Option<Transpiled> {
    let transpiler = TRANSPILER.with(|slot| slot.borrow().clone())?;
    let done = transpiler.transpile(path, source, compile_body)?.ok()?;
    Some(Transpiled { text: done.text, map: Rc::new(PositionMap::from_segments(done.segments)), is_commonjs: done.is_commonjs })
}

/// Há transpilador instalado nesta thread.
pub fn has_source_transpiler() -> bool {
    TRANSPILER.with(|slot| slot.borrow().is_some())
}
