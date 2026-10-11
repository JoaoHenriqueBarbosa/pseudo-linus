//! `Loader` + `SideEffects`.
//!
//! Data-only enum + pure predicates. `to_api()` / `from_api()` / `API_NAMES`
//! live in `bun_options_types::LoaderExt` (would back-edge into the schema
//! crate). `to_mime_type` / `from_mime_type` live in `bun_http_types` (would
//! back-edge into `bun_http::MimeType`).

use enum_map::Enum;

/// The max integer value in this enum can only be appended to.
/// It has dependencies in several places:
/// - bun-native-bundler-plugin-api/bundler_plugin.h
/// - src/jsc/bindings/headers-handwritten.h
#[repr(u8)]
#[derive(
    Copy,
    Clone,
    Default,
    Eq,
    PartialEq,
    Debug,
    Hash,
    Enum,
    strum::IntoStaticStr,
    strum::VariantNames,
)]
// The lower_snake names are exposed to JS (HTMLImportManifest
// `"loader":`, BuildArtifact.loader) so the strum serialization must match exactly.
#[strum(serialize_all = "snake_case")]
pub enum Loader {
    Jsx = 0,
    Js = 1,
    Ts = 2,
    Tsx = 3,
    Css = 4,
    #[default]
    File = 5,
    Json = 6,
    Jsonc = 7,
    Toml = 8,
    Wasm = 9,
    Napi = 10,
    Base64 = 11,
    Dataurl = 12,
    Text = 13,
    Bunsh = 14,
    Sqlite = 15,
    SqliteEmbedded = 16,
    Html = 17,
    Yaml = 18,
    Json5 = 19,
    Md = 20,
    Xml = 21,
}

// Crosses FFI as `uint8_t default_loader` / `uint8_t loader` in
// `OnBeforeParseArguments` / `OnBeforeParseResult` (`bundler_plugin.h`); lock
// the discriminant width and the values native plugins observe. NB: the C
// header's `BUN_LOADER_TOML = 7` etc. predate `Jsonc`'s insertion at 7 and are
// known-stale: this enum is the source of truth.
bun_core::assert_ffi_discr!(
    Loader, u8;
    Jsx = 0, Js = 1, Ts = 2, Tsx = 3, Css = 4, File = 5, Json = 6,
    Jsonc = 7, Toml = 8, Wasm = 9, Napi = 10, Base64 = 11, Dataurl = 12,
    Text = 13, Bunsh = 14, Sqlite = 15, SqliteEmbedded = 16, Html = 17,
    Yaml = 18, Json5 = 19, Md = 20, Xml = 21,
);

// E0658: inherent assoc types are nightly-only; lifted to module scope.
pub type LoaderHashTable = bun_collections::StringArrayHashMap<Loader>;

bun_core::comptime_string_map! {
    pub static LOADER_NAMES: Loader = {
        b"js" => Loader::Js,
        b"mjs" => Loader::Js,
        b"cjs" => Loader::Js,
        b"cts" => Loader::Ts,
        b"mts" => Loader::Ts,
        b"jsx" => Loader::Jsx,
        b"ts" => Loader::Ts,
        b"tsx" => Loader::Tsx,
        b"css" => Loader::Css,
        b"file" => Loader::File,
        b"json" => Loader::Json,
        b"jsonc" => Loader::Jsonc,
        b"toml" => Loader::Toml,
        b"yaml" => Loader::Yaml,
        b"json5" => Loader::Json5,
        b"xml" => Loader::Xml,
        b"wasm" => Loader::Wasm,
        b"napi" => Loader::Napi,
        b"node" => Loader::Napi,
        b"dataurl" => Loader::Dataurl,
        b"base64" => Loader::Base64,
        b"txt" => Loader::Text,
        b"text" => Loader::Text,
        b"sh" => Loader::Bunsh,
        b"sqlite" => Loader::Sqlite,
        b"sqlite_embedded" => Loader::SqliteEmbedded,
        b"html" => Loader::Html,
        b"md" => Loader::Md,
        b"markdown" => Loader::Md,
    };
}

impl Loader {
    #[inline]
    pub fn is_css(self) -> bool {
        self == Loader::Css
    }

    #[inline]
    pub fn is_js_like(self) -> bool {
        matches!(self, Loader::Jsx | Loader::Js | Loader::Ts | Loader::Tsx)
    }

    pub fn should_copy_for_bundling(self) -> bool {
        matches!(
            self,
            Loader::File
                | Loader::Napi
                | Loader::Sqlite
                | Loader::SqliteEmbedded
                // TODO: loader for reading bytes and creating module or instance
                | Loader::Wasm
        )
    }

    pub fn handles_empty_file(self) -> bool {
        matches!(self, Loader::Wasm | Loader::File | Loader::Text)
    }

    // `to_mime_type` / `from_mime_type` stay in bun_http_types as extension
    // methods (would back-edge into bun_http::MimeType).

    pub fn can_have_source_map(self) -> bool {
        matches!(self, Loader::Jsx | Loader::Js | Loader::Ts | Loader::Tsx)
    }

    pub fn can_be_run_by_bun(self) -> bool {
        matches!(
            self,
            Loader::Jsx | Loader::Js | Loader::Ts | Loader::Tsx | Loader::Wasm | Loader::Bunsh
        )
    }

    #[inline]
    pub fn stdin_name(self) -> &'static str {
        match self {
            Loader::Jsx => "input.jsx",
            Loader::Js => "input.js",
            Loader::Ts => "input.ts",
            Loader::Tsx => "input.tsx",
            Loader::Css => "input.css",
            Loader::File => "input",
            Loader::Json => "input.json",
            Loader::Toml => "input.toml",
            Loader::Yaml => "input.yaml",
            Loader::Json5 => "input.json5",
            Loader::Xml => "input.xml",
            Loader::Wasm => "input.wasm",
            Loader::Napi => "input.node",
            Loader::Text => "input.txt",
            Loader::Bunsh => "input.sh",
            Loader::Html => "input.html",
            Loader::Md => "input.md",
            _ => "",
        }
    }

    // `from_js` lives in bundler_jsc as an extension trait.

    pub(crate) const NAMES: &'static __ComptimeStringMap_LOADER_NAMES = &LOADER_NAMES;

    pub fn from_string(slice_: &[u8]) -> Option<Loader> {
        let slice = if !slice_.is_empty() && slice_[0] == b'.' {
            &slice_[1..]
        } else {
            slice_
        };
        Self::NAMES.get(slice).copied().or_else(|| {
            // Exact match missed; retry case-insensitively (keys are lowercase).
            Self::NAMES.get_ascii_case_insensitive(slice).copied()
        })
    }

    #[inline]
    pub fn is_jsx(self) -> bool {
        self == Loader::Jsx || self == Loader::Tsx
    }

    #[inline]
    pub fn is_typescript(self) -> bool {
        self == Loader::Tsx || self == Loader::Ts
    }

    #[inline]
    pub fn is_javascript_like(self) -> bool {
        matches!(self, Loader::Jsx | Loader::Js | Loader::Ts | Loader::Tsx)
    }

    // Os sinônimos de grafia do upstream (`is_type_script`, `is_java_script_like`) são repasse de uma linha para
    // `is_typescript` e `is_javascript_like`; pela regra do repositório o chamador usa o nome canônico.

    pub fn is_javascript_like_or_json(self) -> bool {
        matches!(
            self,
            Loader::Jsx
                | Loader::Js
                | Loader::Ts
                | Loader::Tsx
                | Loader::Json
                | Loader::Jsonc
                // toml, yaml, json5, and xml are included because we can serialize to the same AST as JSON
                | Loader::Toml
                | Loader::Yaml
                | Loader::Json5
                | Loader::Xml
        )
    }

    // `for_file_name` is generic over `anytype` map; callers in bun_bundler
    // implement it locally with their concrete map type.

    pub fn side_effects(self) -> SideEffects {
        match self {
            Loader::Text
            | Loader::Json
            | Loader::Jsonc
            | Loader::Toml
            | Loader::Yaml
            | Loader::Json5
            | Loader::Xml
            | Loader::File
            | Loader::Md => SideEffects::NoSideEffectsPureData,
            _ => SideEffects::HasSideEffects,
        }
    }
}

#[repr(u8)]
#[derive(Copy, Clone, Eq, PartialEq, Debug, Default)]
pub enum SideEffects {
    /// The default value conservatively considers all files to have side effects.
    #[default]
    HasSideEffects,

    /// This file was listed as not having side effects by a "package.json"
    /// file in one of our containing directories with `"sideEffects": false`.
    NoSideEffectsPackageJson,

    /// This file does not match any entry of a "sideEffects" array in a
    /// "package.json" file in one of our containing directories. Unused
    /// imports of this file can still be removed, but other files in the
    /// same package can be side-effectful, so the barrel optimization must
    /// not defer this file's re-exports without resolving them.
    NoSideEffectsPackageJsonArray,

    /// This file is considered to have no side effects because the AST was empty
    /// after parsing finished. This should be the case for ".d.ts" files.
    NoSideEffectsEmptyAst,

    /// This file was loaded using a data-oriented loader (e.g. "text") that is
    /// known to not have side effects.
    NoSideEffectsPureData,
    // /// Same as above but it came from a plugin. We don't want to warn about
    // /// unused imports to these files since running the plugin is a side effect.
    // /// Removing the import would not call the plugin which is observable.
    // NoSideEffectsPureDataFromPlugin,
}

#[cfg(test)]
mod tests {
    use super::Loader;
    use crate::{E, Expr, NodeStore, Store};

    #[test]
    fn from_string_and_names() {
        assert_eq!(Loader::from_string(b".mts"), Some(Loader::Ts));
        assert_eq!(Loader::from_string(b"TSX"), Some(Loader::Tsx));
        assert_eq!(Loader::from_string(b"markdown"), Some(Loader::Md));
        assert_eq!(Loader::from_string(b"exe"), None);
        assert_eq!(<&'static str>::from(Loader::SqliteEmbedded), "sqlite_embedded");
        assert_eq!(Loader::Tsx.stdin_name(), "input.tsx");
    }

    /// `import("./db.sqlite", { with: { type: "sqlite", embed: "true" } })`
    #[test]
    fn import_record_loader_reads_with() {
        let mut store = Store::default();
        let with = store.alloc(E::Object::default());
        E::Object::put_string(&mut store, with, b"type", b"sqlite");
        E::Object::put_string(&mut store, with, b"embed", b"true");
        let options = store.alloc(E::Object::default());
        let with = Expr { loc: crate::Loc::EMPTY, data: crate::ExprData::EObject(with) };
        E::Object::put(&mut store, options, b"with", with);
        let path = store.alloc_str(b"./db.sqlite");
        let import = E::Import {
            expr: Expr::init(&mut store, E::String::init(path), crate::Loc::EMPTY),
            options: Expr { loc: crate::Loc::EMPTY, data: crate::ExprData::EObject(options) },
            import_record_index: 0,
            namespace_ref: crate::Ref::NONE,
        };
        assert_eq!(import.import_record_loader(&store), Some(Loader::SqliteEmbedded));
    }
}
