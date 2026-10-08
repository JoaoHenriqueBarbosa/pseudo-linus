//! Porte de `WTF/wtf/unicode`.
pub mod character_names;
pub mod utf8_conversion;
pub mod case_mapping;
pub mod case_mapping_tables;
pub mod char_direction_table;

/// `u_charDirection` do ICU: a classe bidirecional de `c`, com os valores do enum
/// `UCharDirection`.
pub fn char_direction(c: u32) -> u8 {
    let runs = char_direction_table::CHAR_DIRECTION_RUNS;
    let i = runs.partition_point(|&(start, _)| start <= c);
    runs[i - 1].1
}
