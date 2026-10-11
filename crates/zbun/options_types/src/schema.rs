//! Subconjunto do `schema.rs` do upstream: só os tipos de `api` que `jsx.rs` e `bundle_enums.rs` usam (`Target`,
//! `Loader`, `JsxRuntime`, `Jsx`). O resto do esquema (opções de transformação, registro do npm, `BunInstall`) é da
//! CLI e do instalador, que o transpilador não lê.

#[allow(non_camel_case_types, non_upper_case_globals)]
pub mod api {
    #[repr(u8)]
    #[derive(Copy, Clone, Eq, PartialEq, Debug, Default)]
    pub enum Target {
        #[default]
        _none = 0,
        browser = 1,
        node = 2,
        bun = 3,
        bun_macro = 4,
    }

    impl Target {
        // Sinônimos em PascalCase: `runtime/cli/Arguments.rs` escreve `api::Target::Bun`, e o corpo do enum fica com
        // as etiquetas em snake_case que o `bundle_enums.rs` compara.
        pub const Browser: Self = Self::browser;
        pub const Node: Self = Self::node;
        pub const Bun: Self = Self::bun;
        pub const BunMacro: Self = Self::bun_macro;
    }

    #[repr(u8)]
    #[derive(Copy, Clone, Eq, PartialEq, Debug, Default)]
    pub enum Loader {
        #[default]
        _none = 254,
        jsx = 1,
        js = 2,
        ts = 3,
        tsx = 4,
        css = 5,
        file = 6,
        json = 7,
        jsonc = 8,
        toml = 9,
        wasm = 10,
        napi = 11,
        base64 = 12,
        dataurl = 13,
        text = 14,
        bunsh = 15,
        sqlite = 16,
        sqlite_embedded = 17,
        html = 18,
        yaml = 19,
        json5 = 20,
        md = 21,
        xml = 22,
    }

    impl Loader {
        /// Converte um discriminante cru no `Loader` do esquema. Discriminantes desconhecidos viram `_none`, como o
        /// `BundleEnums::Loader::from_api` já protege a cauda aberta.
        #[inline]
        pub const fn from_raw(n: u8) -> Loader {
            match n {
                1 => Loader::jsx,
                2 => Loader::js,
                3 => Loader::ts,
                4 => Loader::tsx,
                5 => Loader::css,
                6 => Loader::file,
                7 => Loader::json,
                8 => Loader::jsonc,
                9 => Loader::toml,
                10 => Loader::wasm,
                11 => Loader::napi,
                12 => Loader::base64,
                13 => Loader::dataurl,
                14 => Loader::text,
                15 => Loader::bunsh,
                16 => Loader::sqlite,
                17 => Loader::sqlite_embedded,
                18 => Loader::html,
                19 => Loader::yaml,
                20 => Loader::json5,
                21 => Loader::md,
                22 => Loader::xml,
                _ => Loader::_none,
            }
        }
    }

    #[repr(u8)]
    #[derive(Copy, Clone, Eq, PartialEq, Debug, Default)]
    pub enum JsxRuntime {
        #[default]
        _none = 0,
        Automatic = 1,
        Classic = 2,
        Solid = 3,
    }

    /// Configuração da transformação do JSX (fábrica, fragmento, runtime e afins).
    #[derive(Clone, Debug, Default)]
    pub struct Jsx {
        pub factory: Box<[u8]>,
        pub runtime: JsxRuntime,
        pub fragment: Box<[u8]>,
        pub development: bool,
        pub import_source: Box<[u8]>,
        pub side_effects: bool,
    }
}
