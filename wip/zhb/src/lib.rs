//! Shaping OpenType do HarfBuzz 10.2.0, traduzido para Rust sem `unsafe`.

pub mod arabic;
mod arabic_table;
pub mod bidi;
mod bidi_table;
pub mod buffer;
pub mod common;
pub mod fallback;
pub mod font;
pub mod gpos;
pub mod gsub;
pub mod gsubgpos;
pub mod hangul;
pub mod hebrew;
pub mod indic;
mod indic_machine;
mod indic_table;
pub mod khmer;
#[allow(dead_code)]
mod khmer_machine;
pub mod lang;
mod lang_table;
pub mod map;
pub mod myanmar;
#[allow(dead_code)]
mod myanmar_machine;
pub mod normalize;
pub mod ot;
pub mod props;
pub mod raqm;
pub mod shape;
pub mod thai;
pub mod universal;
mod use_machine;
mod use_table;
mod ucd_table;
pub mod unicode;
mod vowel_table;
mod would;
