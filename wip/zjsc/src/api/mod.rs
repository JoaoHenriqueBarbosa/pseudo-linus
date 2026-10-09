//! API de alto nível (equivalente ao que `JSC::evaluate` oferece a quem embute o motor).
pub mod builtin_modules;
pub mod bun_options;
pub mod eval;
pub mod fs_module_host;
pub mod module;
pub mod module_probe;
pub mod package_exports;
pub mod package_json;
