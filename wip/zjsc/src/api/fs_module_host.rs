//! `FsModuleHost`: o carregador de módulos ES sobre um [`ModuleFs`], o mesmo trait que o `require` do CommonJS
//! usa. A chave de um módulo é o caminho canônico (`realpath`), então dois caminhos que levam ao mesmo arquivo
//! são a mesma instância. Especificador relativo ou absoluto vai pela sonda de arquivo e diretório
//! ([`resolve_require`]); o resto é nome de pacote, resolvido em `node_modules` dos ancestrais. As falhas têm as
//! mensagens do Bun: `Cannot find module './x' imported from /app/main.mjs` e
//! `Cannot find package 'x' imported from /app/main.mjs`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::api::builtin_modules;
use crate::api::module_probe::{file_url_from_path, file_url_path, import_not_found_message, resolve_require, ModuleFs};
use crate::api::source_transpiler::transpile_source;
use crate::parser::position_map::PositionMap;
use crate::runtime::host_call::Thrown;
use crate::runtime::identifier::Identifier;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_module_loader::ModuleHost;
use crate::runtime::js_value::JSValue;

pub struct FsModuleHost {
    fs: Rc<dyn ModuleFs>,
    /// Os mapas dos módulos que `fetch` passou pelo transpilador de fonte, por chave (o loader os pede depois).
    maps: RefCell<HashMap<String, Rc<PositionMap>>>,
}

impl FsModuleHost {
    pub fn new(fs: Rc<dyn ModuleFs>) -> Self {
        Self { fs, maps: RefCell::new(HashMap::new()) }
    }
}

fn is_path_like(specifier: &str) -> bool {
    specifier.starts_with('/') || specifier.starts_with('\\') || matches!(specifier, "." | "..") || specifier.starts_with("./") || specifier.starts_with("../")
}

impl ModuleHost for FsModuleHost {
    fn resolve(&self, specifier: &str, referrer: Option<&str>) -> Result<String, String> {
        // Embutido do registro (`node:path`, `path`): a chave é `node:<nome>`, e o módulo vem de `builtin_exports`.
        if let Some(resolved) = builtin_modules::resolve_import(specifier) {
            return resolved;
        }
        let from = referrer.unwrap_or("/");
        let importer_dir = from.rsplit_once('/').map_or("", |(directory, _)| directory);
        resolve_require(self.fs.as_ref(), importer_dir, specifier).ok_or_else(|| {
            if let Some(path) = file_url_path(specifier) {
                format!("Cannot find module '{path}' imported from {from}")
            } else if is_path_like(specifier) {
                format!("Cannot find module '{specifier}' imported from {from}")
            } else {
                import_not_found_message(specifier, from)
            }
        })
    }

    fn builtin_exports(&self, global_object: &JSGlobalObject, key: &str) -> Option<Result<(Vec<Identifier>, Vec<JSValue>), Thrown>> {
        builtin_modules::import_exports(global_object, key)
    }

    fn fetch(&self, key: &str) -> Result<String, String> {
        let text = self.fs.read_file(key).ok_or_else(|| format!("Could not open the module '{key}'."))?;
        // O módulo ES reimpresso pelo transpilador de fonte (o CommonJS que um `import` puxa segue como está).
        match transpile_source(key, &text, false) {
            Some(done) if !done.is_commonjs => {
                self.maps.borrow_mut().insert(key.to_owned(), done.map);
                Ok(done.text)
            }
            _ => Ok(text),
        }
    }

    fn position_map(&self, key: &str) -> Option<Rc<PositionMap>> {
        self.maps.borrow().get(key).cloned()
    }

    fn import_meta_url(&self, key: &str) -> String {
        file_url_from_path(key)
    }
}
