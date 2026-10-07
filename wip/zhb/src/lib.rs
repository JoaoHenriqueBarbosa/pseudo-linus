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
pub mod lang;
mod lang_table;
pub mod map;
pub mod normalize;
pub mod ot;
pub mod props;
pub mod raqm;
pub mod shape;
mod ucd_table;
pub mod unicode;
