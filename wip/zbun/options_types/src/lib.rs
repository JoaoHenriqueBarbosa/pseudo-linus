//! `bun_options_types`: os tipos de opção que o parser e o printer leem (`jsx`, `bundle_enums`), portados do bun
//! 1.4.2. Os outros módulos do crate do upstream (`context`, `compile_target`, `command_tag` e afins) são da CLI e
//! do bundler e ficam fora.

#![allow(non_snake_case, non_camel_case_types, non_upper_case_globals)]

pub mod bundle_enums;
pub mod jsx;
pub mod schema;

pub use jsx as JSX;

pub use bundle_enums::{
    BuiltInModule, BundlePackage, ForceNodeEnv, Format, LOADER_API_NAMES, LoaderExt, ModuleType, TargetExt,
    WindowsOptions,
};

#[cfg(test)]
mod tests;
