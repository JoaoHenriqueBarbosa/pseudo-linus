//! Porte de `yarr/YarrUnicodeProperties.h` e `YarrUnicodeProperties.cpp`. As tabelas
//! (`UnicodePatternTables.h`) estão em `unicode_pattern_tables`, geradas por script.

use crate::wtf::text::wtf_string::String;
use crate::yarr::unicode_pattern_tables::{
    unicode_character_class_may_contain_strings, BINARY_PROPERTY_HASH_TABLE, CLASSES,
    GENERAL_CATEGORY_HASH_TABLE, SCRIPT_EXTENSION_HASH_TABLE, SCRIPT_HASH_TABLE,
    SEQUENCE_PROPERTY_HASH_TABLE,
};
use crate::yarr::yarr::BuiltInCharacterClassID;
use crate::yarr::yarr_pattern::{CharacterClass, CharacterClassWidths, CharacterRange, CompileMode};

/// `struct HashIndex`.
pub struct HashIndex {
    pub value: i16,
    pub next: i16,
}

/// `struct HashValue`.
pub struct HashValue {
    pub key: &'static str,
    pub index: i32,
}

/// `struct HashTable`.
pub struct HashTable {
    pub number_of_values: i32,
    pub index_mask: i32,
    pub values: &'static [HashValue],
    pub index: &'static [HashIndex],
}

impl HashTable {
    /// `HashTable::entry`.
    pub fn entry(&self, key: &String) -> i32 {
        let mut index_entry = (key.hash() & self.index_mask as u32) as usize;
        let mut value_index = self.index[index_entry].value;
        if value_index == -1 {
            return -1;
        }
        loop {
            let value = &self.values[value_index as usize];
            if *key == String::from_latin1(value.key.as_bytes()) {
                return value.index;
            }
            let next = self.index[index_entry].next;
            if next == -1 {
                return -1;
            }
            index_entry = next as usize;
            value_index = self.index[index_entry].value;
            debug_assert!(value_index != -1);
        }
    }
}

/// Uma `createCharacterClassN()` do C++: os argumentos do construtor e, nas propriedades de
/// sequência, as strings concatenadas em `.0` com o tamanho de cada uma em `.1`.
pub struct ClassData {
    pub matches8: &'static [u32],
    pub ranges8: &'static [CharacterRange],
    pub matches32: &'static [u32],
    pub ranges32: &'static [CharacterRange],
    pub widths: CharacterClassWidths,
    pub strings: Option<(&'static [u32], &'static [u8])>,
}

fn with_offset(property_index: i32) -> Option<BuiltInCharacterClassID> {
    if property_index == -1 {
        return None;
    }
    Some(BuiltInCharacterClassID(
        (BuiltInCharacterClassID::BaseUnicodePropertyID.0 as i32 + property_index) as u32,
    ))
}

/// `unicodeMatchPropertyValue`.
pub fn unicode_match_property_value(
    unicode_property_name: String,
    unicode_property_value: String,
) -> Option<BuiltInCharacterClassID> {
    let is = |name: &str| unicode_property_name == String::from_latin1(name.as_bytes());
    let property_index = if is("Script") || is("sc") {
        SCRIPT_HASH_TABLE.entry(&unicode_property_value)
    } else if is("Script_Extensions") || is("scx") {
        SCRIPT_EXTENSION_HASH_TABLE.entry(&unicode_property_value)
    } else if is("General_Category") || is("gc") {
        GENERAL_CATEGORY_HASH_TABLE.entry(&unicode_property_value)
    } else {
        -1
    };
    with_offset(property_index)
}

/// `unicodeMatchProperty`.
pub fn unicode_match_property(
    unicode_property_value: String,
    compile_mode: CompileMode,
) -> Option<BuiltInCharacterClassID> {
    let mut property_index = BINARY_PROPERTY_HASH_TABLE.entry(&unicode_property_value);
    if property_index == -1 {
        property_index = GENERAL_CATEGORY_HASH_TABLE.entry(&unicode_property_value);
    }
    if property_index == -1 && compile_mode == CompileMode::UnicodeSets {
        property_index = SEQUENCE_PROPERTY_HASH_TABLE.entry(&unicode_property_value);
    }
    with_offset(property_index)
}

fn property_index(unicode_class_id: BuiltInCharacterClassID) -> u32 {
    unicode_class_id.0 - BuiltInCharacterClassID::BaseUnicodePropertyID.0
}

/// `createUnicodeCharacterClassFor`.
pub fn create_unicode_character_class_for(unicode_class_id: BuiltInCharacterClassID) -> Box<CharacterClass> {
    let data = &CLASSES[property_index(unicode_class_id) as usize];
    let mut class = CharacterClass::with_matches(
        data.matches8,
        data.ranges8,
        data.matches32,
        data.ranges32,
        data.widths,
    );
    if let Some((mut chars, lengths)) = data.strings {
        class.strings.reserve_exact(lengths.len());
        for &length in lengths {
            let (string, rest) = chars.split_at(length as usize);
            class.strings.push(string.to_vec());
            chars = rest;
        }
        class.in_canonical_form = true;
    }
    Box::new(class)
}

/// `characterClassMayContainStrings`.
pub fn character_class_may_contain_strings(unicode_class_id: BuiltInCharacterClassID) -> bool {
    unicode_character_class_may_contain_strings(property_index(unicode_class_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(text: &str) -> String {
        String::from_latin1(text.as_bytes())
    }

    #[test]
    fn lookups() {
        let any = unicode_match_property(s("Any"), CompileMode::Unicode).unwrap();
        assert_eq!(any, BuiltInCharacterClassID::BaseUnicodePropertyID);
        let lu = unicode_match_property(s("Lu"), CompileMode::Unicode).unwrap();
        let upper = unicode_match_property_value(s("gc"), s("Uppercase_Letter")).unwrap();
        assert_eq!(lu, upper);
        assert!(unicode_match_property(s("RGI_Emoji"), CompileMode::Unicode).is_none());
        let emoji = unicode_match_property(s("RGI_Emoji"), CompileMode::UnicodeSets).unwrap();
        assert!(character_class_may_contain_strings(emoji));
        assert_eq!(create_unicode_character_class_for(emoji).strings.len(), 2760);
        assert!(unicode_match_property_value(s("Script"), s("Greek")).is_some());
        assert!(unicode_match_property_value(s("Script"), s("Nope")).is_none());
        let ascii = create_unicode_character_class_for(unicode_match_property(s("ASCII"), CompileMode::Unicode).unwrap());
        assert_eq!(ascii.ranges8, vec![CharacterRange::new(0, 0x7f)]);
    }
}
