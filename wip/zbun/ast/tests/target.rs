//! `Target` e o `TARGET_MAP` gerado pelo `comptime_string_map!`, como no bun 1.4.2.
use bun_ast::Target;
use bun_ast::target::TARGET_MAP;

#[test]
fn target_map_resolves_aliases() {
    assert_eq!(TARGET_MAP.get(b"macro").copied(), Some(Target::BunMacro));
    assert_eq!(TARGET_MAP.get(b"bun_macro").copied(), Some(Target::BunMacro));
    assert_eq!(TARGET_MAP.get(b"deno"), None);
    assert!(Target::Bun.is_server_side());
    assert_eq!(Target::Node.default_main_fields(), &["main", "module"]);
}
