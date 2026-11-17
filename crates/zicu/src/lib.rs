//! Os iteradores de quebra do ICU4C 78.2, no fork do Chromium que o engine do Flutter usa,
//! traduzidos sem `unsafe`: as fronteiras de caractere (UAX #29), de palavra e de linha (UAX #14)
//! saem iguais às do ICU para as mesmas tabelas binárias (`.brk`) e os mesmos dicionários.
//!
//! As tabelas não vêm embutidas: quem usa passa os bytes de um `.brk` (por exemplo, os do pacote de
//! dados do ICU que acompanha o programa) a [`RbbiData::from_bytes`], e os de cada `.dict` a
//! [`DictionaryMatcher::from_bytes`], com o motor do script ([`DictionaryBreakEngine::thai`] e os
//! outros três).

mod bytes_trie;
mod dictbe;
mod dictionary;
mod rbbi;
mod rbbi_data;
mod sets;
mod ucptrie;
mod utext;

pub use dictbe::DictionaryBreakEngine;
pub use dictionary::{DictionaryMatcher, Matches};
pub use rbbi::{DONE, LanguageBreakEngine, RuleBasedBreakIterator};
pub use rbbi_data::RbbiData;
pub use ucptrie::{CodePointTrie, ValueWidth};
pub use utext::Utf16Text;
