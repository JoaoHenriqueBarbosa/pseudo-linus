//! `BuildMessage` com posição, notas e nível, como o bun os expõe (`tests/golden/build_message_bun.tsv`, de
//! `scripts/gen-build-message-golden.js`, rodado no bun 1.4.2): `position` (`lineText`, `file`, `namespace`, `line`,
//! `column`, `length`, `offset`), `line` e `column` de base 0, `level`, `notes` (cada uma um `BuildMessage` de nível
//! `note`), a identidade de `position` e `notes` entre leituras, `toJSON` e `String`. O transpilador é o do zbun.
mod common;
mod zbun_transpiler;

use common::module_golden::check_module_golden;
use zjsc::api::module::LogPoint;

#[test]
fn build_message_position_and_notes_match_bun() {
    zbun_transpiler::install();
    check_module_golden(include_str!("golden/build_message_bun.tsv"), 20, LogPoint::EntrySettled);
}
