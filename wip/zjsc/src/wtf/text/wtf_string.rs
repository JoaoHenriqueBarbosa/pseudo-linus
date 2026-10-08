//! Tradução de `WTF/wtf/text/WTFString.h` e `WTFString.cpp`.
//!
//! Neste módulo o nome `String` é o `WTF::String` do C++ e esconde o `std::string::String` do Rust;
//! onde o Rust é necessário (testes), o caminho completo `std::string::String` aparece explícito.
//!
//! Modelo (CONVENTIONS, item 1): `String` é `Option<Rc<StringImpl>>`; o nulo do C++ é `None`, e
//! nulo e vazio são distintos (`is_null`, `is_empty`). O `CString` do C++ vira `Vec<u8>` (sem o
//! terminador nulo, que o chamador acrescenta se precisar). `bool* ok` vira o `bool` devolvido
//! junto do valor.
//!
//! Dependências não portadas, e como foram tratadas:
//!
//! - `StringView`: os parâmetros `StringView` viram `&String` (o `StringView(const String&)` do C++:
//!   `String` nulo é visão nula, vazio é visão vazia). A lógica de `StringView.h`, `StringCommon.h`
//!   e `StringImpl.cpp` de que o `WTFString` depende (`findCommon`, `findInner`, `reverseFindInner`,
//!   `equalInner`, `startsWith`, `endsWith` e as versões que ignoram caixa ASCII) está traduzida
//!   aqui como funções privadas.
//! - `StringBuilder`: `makeStringByJoining` e `makeStringByRemoving` montam o resultado direto, com
//!   a mesma largura (8 ou 16 bits) que o `StringBuilder`/`makeString` produzem.
//! - `parseDouble` e `parseFixedDouble` vêm de `crate::wtf::fast_float` (o `FastFloat.cpp`).
//! - `AtomString`: `convert_to_*_with_locale` recebem `&String` no lugar do `const AtomString&`.
//!
//! Fora deste porte (dependem de módulos ainda inexistentes): `toExistingAtomString`,
//! `numberToStringFixedPrecision`, `numberToStringFixedWidth`, `number(float)`,
//! `tryCreateUninitialized`, `defaultWritingDirection` (ICU), `show()` (só em depuração) e as
//! sobrecargas que recebem `ASCIILiteral` (o chamador monta um `String` com `from_latin1`).

use std::rc::Rc;

use crate::wtf::ascii_ctype::{
    is_ascii_alpha_caseless_equal, is_unicode_compatible_ascii_whitespace, to_ascii_lower,
};
use crate::wtf::dtoa::{number_to_string_and_size, NumberToStringBuffer};
use crate::wtf::text::string_impl::{
    self, copy_characters_widen, CharType, StringImpl,
};
use crate::wtf::unicode::utf8_conversion::{
    convert_latin1_to_utf8, convert_replacing_invalid_sequences_utf16_to_utf8,
    convert_replacing_invalid_sequences_utf8_to_utf16, convert_utf16_to_utf8, ConversionResultCode,
};

/// `notFound` de `StringCommon.h`.
pub const NOT_FOUND: usize = usize::MAX;

/// `String::MaxLength`.
pub const MAX_LENGTH: u32 = StringImpl::MAX_LENGTH;

/// `ConversionMode` de `wtf/text/ConversionMode.h`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConversionMode {
    LenientConversion,
    StrictConversion,
    StrictConversionReplacingUnpairedSurrogatesWithFFFD,
}

/// `UTF8ConversionError` de `wtf/text/UTF8ConversionError.h` (mesma ordem de variantes).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UTF8ConversionError {
    OutOfMemory,
    Invalid,
}

/// `TrailingZerosPolicy`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrailingZerosPolicy {
    Keep,
    Truncate,
}

/// `TrailingJunkPolicy` de `StringToIntegerConversion.h`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrailingJunkPolicy {
    Disallow,
    Allow,
}

/// `WhitespacePolicy` do `WTFString.cpp`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WhitespacePolicy {
    Skip,
    Preserve,
}

/// `ParseMode` do `WTFString.cpp`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ParseMode {
    General,
    Fixed,
}

// ---------------------------------------------------------------------------------------------
// Visão sobre os caracteres (o par `span8()`/`span16()` do C++)
// ---------------------------------------------------------------------------------------------

/// Os caracteres de uma string, Latin1 ou UTF-16. Uma string nula é um `Latin1` vazio.
#[derive(Clone, Copy)]
enum Chars<'a> {
    Latin1(&'a [u8]),
    Utf16(&'a [u16]),
}

/// Despacha uma expressão genérica sobre os dois pares de largura de caractere.
macro_rules! dispatch2 {
    ($a:expr, $b:expr, |$x:ident, $y:ident| $body:expr) => {
        match ($a, $b) {
            (Chars::Latin1($x), Chars::Latin1($y)) => $body,
            (Chars::Latin1($x), Chars::Utf16($y)) => $body,
            (Chars::Utf16($x), Chars::Latin1($y)) => $body,
            (Chars::Utf16($x), Chars::Utf16($y)) => $body,
        }
    };
}

impl<'a> Chars<'a> {
    fn of(string: &'a StringImpl) -> Chars<'a> {
        if string.is_8bit() {
            Chars::Latin1(string.span8())
        } else {
            Chars::Utf16(string.span16())
        }
    }

    fn len(self) -> usize {
        match self {
            Chars::Latin1(span) => span.len(),
            Chars::Utf16(span) => span.len(),
        }
    }

    fn at(self, index: usize) -> u16 {
        match self {
            Chars::Latin1(span) => span[index] as u16,
            Chars::Utf16(span) => span[index],
        }
    }

    /// `WTF::find(span, character, start)`.
    fn find_character(self, character: u16, start: usize) -> usize {
        match self {
            Chars::Latin1(span) => string_impl::find(span, |c| c == character, start),
            Chars::Utf16(span) => string_impl::find(span, |c| c == character, start),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Funções de comparação e busca sobre spans (StringCommon.h)
// ---------------------------------------------------------------------------------------------

/// `equal(const CharType* a, std::span<const OtherType> b)`: compara os `b.len()` primeiros
/// caracteres de `a` com `b`. O chamador garante `a.len() >= b.len()`.
fn equal_prefix<A: CharType, B: CharType>(a: &[A], b: &[B]) -> bool {
    a.iter().zip(b.iter()).all(|(x, y)| x.to_u16() == y.to_u16()) && a.len() >= b.len()
}

/// `equalIgnoringASCIICaseWithLength`.
fn equal_ignoring_ascii_case_with_length<A: CharType, B: CharType>(a: &[A], b: &[B], length_to_check: usize) -> bool {
    (0..length_to_check).all(|i| to_ascii_lower(a[i].to_u16()) == to_ascii_lower(b[i].to_u16()))
}

/// `equalLettersIgnoringASCIICaseWithLength`.
fn equal_letters_ignoring_ascii_case_with_length<T: CharType>(
    characters: &[T],
    lowercase_letters: &[u8],
    length: usize,
) -> bool {
    (0..length).all(|i| is_ascii_alpha_caseless_equal(characters[i].to_u16(), lowercase_letters[i]))
}

/// `findInner` de `StringCommon.h`: hash corrente das janelas, `equal` só quando o hash casa.
fn find_inner<S: CharType, M: CharType>(search_characters: &[S], match_characters: &[M], index: usize) -> usize {
    // delta is the number of additional times to test; delta == 0 means test only once.
    let delta = search_characters.len() - match_characters.len();

    let mut search_hash: u32 = 0;
    let mut match_hash: u32 = 0;

    for i in 0..match_characters.len() {
        search_hash = search_hash.wrapping_add(search_characters[i].to_u16() as u32);
        match_hash = match_hash.wrapping_add(match_characters[i].to_u16() as u32);
    }

    let mut i = 0;
    // keep looping until we match
    while search_hash != match_hash || !equal_prefix(&search_characters[i..], match_characters) {
        if i == delta {
            return NOT_FOUND;
        }
        search_hash = search_hash.wrapping_add(search_characters[i + match_characters.len()].to_u16() as u32);
        search_hash = search_hash.wrapping_sub(search_characters[i].to_u16() as u32);
        i += 1;
    }
    index + i
}

/// `reverseFindInner` de `StringCommon.h`.
fn reverse_find_inner<S: CharType, M: CharType>(
    search_characters: &[S],
    match_characters: &[M],
    start: usize,
) -> usize {
    if search_characters.len() < match_characters.len() {
        return NOT_FOUND;
    }

    // delta is the number of additional times to test; delta == 0 means test only once.
    let mut delta = std::cmp::min(start, search_characters.len() - match_characters.len());

    let mut search_hash: u32 = 0;
    let mut match_hash: u32 = 0;
    for i in 0..match_characters.len() {
        search_hash = search_hash.wrapping_add(search_characters[delta + i].to_u16() as u32);
        match_hash = match_hash.wrapping_add(match_characters[i].to_u16() as u32);
    }

    // keep looping until we match
    while search_hash != match_hash || !equal_prefix(&search_characters[delta..], match_characters) {
        if delta == 0 {
            return NOT_FOUND;
        }
        delta -= 1;
        search_hash = search_hash.wrapping_sub(search_characters[delta + match_characters.len()].to_u16() as u32);
        search_hash = search_hash.wrapping_add(search_characters[delta].to_u16() as u32);
    }
    delta
}

/// `WTF::findIgnoringASCIICase(span, span, startOffset)`.
fn find_ignoring_ascii_case_spans<S: CharType, M: CharType>(
    source: &[S],
    match_characters: &[M],
    start_offset: usize,
) -> usize {
    let mut offset = start_offset;
    while offset <= source.len() && source.len() - offset >= match_characters.len() {
        if equal_ignoring_ascii_case_with_length(&source[offset..], match_characters, match_characters.len()) {
            return offset;
        }
        offset += 1;
    }
    NOT_FOUND
}

/// `findCommon(StringView haystack, StringView needle, unsigned start)` de `StringView.h`.
fn find_common(haystack: Chars, needle: Chars, start: usize) -> usize {
    let needle_length = needle.len();

    if needle_length == 1 {
        return haystack.find_character(needle.at(0), start);
    }

    if start > haystack.len() {
        return NOT_FOUND;
    }

    if needle_length == 0 {
        return start;
    }

    let search_length = haystack.len() - start;
    if needle_length > search_length {
        return NOT_FOUND;
    }

    dispatch2!(haystack, needle, |h, n| find_inner(&h[start..], n, start))
}

/// `findIgnoringASCIICase(StringView source, StringView stringToFind, unsigned start)`.
fn find_ignoring_ascii_case(source: Chars, string_to_find: Chars, start: usize) -> usize {
    let source_string_length = source.len();
    let match_length = string_to_find.len();
    if match_length == 0 {
        return std::cmp::min(start, source_string_length);
    }

    // Check start & matchLength are in range.
    if start > source_string_length {
        return NOT_FOUND;
    }
    let search_length = source_string_length - start;
    if match_length > search_length {
        return NOT_FOUND;
    }

    dispatch2!(source, string_to_find, |s, f| find_ignoring_ascii_case_spans(s, f, start))
}

/// `startsWith(StringView reference, StringView prefix)`.
fn starts_with_chars(reference: Chars, prefix: Chars) -> bool {
    if prefix.len() > reference.len() {
        return false;
    }
    dispatch2!(reference, prefix, |r, p| equal_prefix(r, p))
}

/// `startsWithIgnoringASCIICase(StringView reference, StringView prefix)`.
fn starts_with_ignoring_ascii_case_chars(reference: Chars, prefix: Chars) -> bool {
    if prefix.len() > reference.len() {
        return false;
    }
    dispatch2!(reference, prefix, |r, p| equal_ignoring_ascii_case_with_length(r, p, p.len()))
}

/// `endsWith(StringView reference, StringView suffix)`.
fn ends_with_chars(reference: Chars, suffix: Chars) -> bool {
    let suffix_length = suffix.len();
    let reference_length = reference.len();
    if suffix_length > reference_length {
        return false;
    }

    let start_offset = reference_length - suffix_length;
    dispatch2!(reference, suffix, |r, s| equal_prefix(&r[start_offset..], s))
}

/// `endsWithIgnoringASCIICase(StringView reference, StringView suffix)`.
fn ends_with_ignoring_ascii_case_chars(reference: Chars, suffix: Chars) -> bool {
    let suffix_length = suffix.len();
    let reference_length = reference.len();
    if suffix_length > reference_length {
        return false;
    }

    let start_offset = reference_length - suffix_length;
    dispatch2!(reference, suffix, |r, s| equal_ignoring_ascii_case_with_length(
        &r[start_offset..],
        s,
        s.len()
    ))
}

/// `equalInner(const StringImpl&, unsigned start, StringView)` do `StringImpl.cpp`.
fn equal_inner(string: &StringImpl, start: u32, match_string: &String) -> bool {
    let start = start as usize;
    let length = string.length() as usize;
    let match_chars = match_string.chars();
    if start > length {
        return false;
    }
    if match_chars.len() > length {
        return false;
    }
    if match_chars.len() + start > length {
        return false;
    }

    dispatch2!(Chars::of(string), match_chars, |s, m| equal_prefix(&s[start..], m))
}

/// `StringImpl::find(StringView matchString)`.
fn impl_find(string: &StringImpl, match_string: &String) -> usize {
    // Check for null string to match against
    if match_string.is_null() {
        return NOT_FOUND;
    }
    let match_chars = match_string.chars();
    let match_length = match_chars.len();
    let haystack = Chars::of(string);

    // Optimization 1: fast case for strings of length 1.
    if match_length == 1 {
        return haystack.find_character(match_chars.at(0), 0);
    }

    // Check matchLength is in range.
    if match_length > string.length() as usize {
        return NOT_FOUND;
    }

    // Check for empty string to match against
    if match_length == 0 {
        return 0;
    }

    dispatch2!(haystack, match_chars, |h, m| find_inner(h, m, 0))
}

/// `StringImpl::reverseFind(StringView matchString, size_t start)`.
fn impl_reverse_find(string: &StringImpl, match_string: &String, start: usize) -> usize {
    // Check for null or empty string to match against
    if match_string.is_null() {
        return NOT_FOUND;
    }
    let match_chars = match_string.chars();
    if match_chars.len() == 0 {
        return std::cmp::min(start, string.length() as usize);
    }

    let haystack = Chars::of(string);

    // Optimization 1: fast case for strings of length 1.
    if match_chars.len() == 1 {
        return match haystack {
            Chars::Latin1(span) => string_impl::reverse_find_8_char16(span, match_chars.at(0), start),
            Chars::Utf16(span) => string_impl::reverse_find(span, match_chars.at(0), start),
        };
    }

    // Check start & matchLength are in range.
    if match_chars.len() > string.length() as usize {
        return NOT_FOUND;
    }

    dispatch2!(haystack, match_chars, |h, m| reverse_find_inner(h, m, start))
}

/// `equalCommon(a, b)` entre dois `StringImpl` já não nulos: mesmo tamanho e mesmos caracteres.
fn equal_common(a: &StringImpl, b: &StringImpl) -> bool {
    if a.length() != b.length() {
        return false;
    }
    dispatch2!(Chars::of(a), Chars::of(b), |x, y| equal_prefix(x, y))
}

/// `equal(const StringImpl&, const StringImpl&)`: o hash já calculado serve de atalho.
pub fn equal_string_impl(a: &StringImpl, b: &StringImpl) -> bool {
    let a_hash = a.existing_hash();
    let b_hash = b.existing_hash();
    if a_hash != b_hash && a_hash != 0 && b_hash != 0 {
        return false;
    }
    equal_common(a, b)
}

/// `equal(const StringImpl*, const StringImpl*)`: igualdade de ponteiro, nulo só casa com nulo.
pub fn equal_impl(a: Option<&Rc<StringImpl>>, b: Option<&Rc<StringImpl>>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => Rc::ptr_eq(a, b) || equal_string_impl(a, b),
        _ => false,
    }
}

// ---------------------------------------------------------------------------------------------
// UTF-8 (StringImpl.cpp / StringImpl.h)
// ---------------------------------------------------------------------------------------------

/// `isValidCapacityForVector<char8_t>`: `capacity <= (UINT32_MAX >> 1) / sizeof(T)`.
fn is_valid_capacity_for_vector(capacity: usize, element_size: usize) -> bool {
    capacity <= (u32::MAX >> 1) as usize / element_size
}

/// `simdutf::utf8_length_from_utf16le`: cada unidade de substituto conta como 2 bytes.
fn utf8_length_from_utf16(characters: &[u16]) -> usize {
    characters
        .iter()
        .map(|&word| {
            if word <= 0x7F {
                1
            } else if word <= 0x7FF {
                2
            } else if word <= 0xD7FF || word >= 0xE000 {
                3
            } else {
                2
            }
        })
        .sum()
}

/// `true` se o UTF-16 é bem formado, o único caso em que a conversão do `simdutf` tem sucesso.
fn is_well_formed_utf16(characters: &[u16]) -> bool {
    let mut i = 0;
    while i < characters.len() {
        let unit = characters[i];
        if (0xD800..0xDC00).contains(&unit) {
            if i + 1 < characters.len() && (0xDC00..0xE000).contains(&characters[i + 1]) {
                i += 2;
                continue;
            }
            return false;
        }
        if (0xDC00..0xE000).contains(&unit) {
            return false;
        }
        i += 1;
    }
    true
}

/// `StringImpl::tryGetUTF8ForCharacters` para Latin1 (o caso `utf8ForCharacters(span8)`).
fn utf8_for_characters_latin1(characters: &[u8]) -> Result<Vec<u8>, UTF8ConversionError> {
    if characters.is_empty() {
        return Ok(Vec::new());
    }

    // Allocate a buffer big enough to hold all the characters
    // (an individual Latin1Character can only expand to 2 UTF-8 bytes).
    let capacity = match characters.len().checked_mul(2) {
        Some(capacity) if is_valid_capacity_for_vector(capacity, 1) => capacity,
        _ => return Err(UTF8ConversionError::OutOfMemory),
    };

    let mut buffer = vec![0u8; capacity];
    let converted = convert_latin1_to_utf8(characters, &mut buffer).buffer.len();
    buffer.truncate(converted);
    Ok(buffer)
}

/// `StringImpl::tryGetUTF8ForCharacters` para UTF-16 (`utf8ForCharacters(span16, mode)`).
fn utf8_for_characters_utf16(characters: &[u16], mode: ConversionMode) -> Result<Vec<u8>, UTF8ConversionError> {
    if characters.is_empty() {
        return Ok(Vec::new());
    }

    let utf8_length = utf8_length_from_utf16(characters);
    if !is_valid_capacity_for_vector(utf8_length, 1) {
        return Err(UTF8ConversionError::OutOfMemory);
    }

    // O caminho do simdutf só tem sucesso em UTF-16 bem formado.
    if is_well_formed_utf16(characters) {
        let mut buffer = vec![0u8; utf8_length];
        let converted = convert_utf16_to_utf8(characters, &mut buffer).buffer.len();
        buffer.truncate(converted);
        return Ok(buffer);
    }

    let buffer_size = match characters.len().checked_mul(3) {
        Some(size) if is_valid_capacity_for_vector(size, 1) => size,
        _ => return Err(UTF8ConversionError::OutOfMemory),
    };

    let mut buffer = vec![0u8; buffer_size];
    let (code, converted) = match mode {
        ConversionMode::StrictConversion => {
            let result = convert_utf16_to_utf8(characters, &mut buffer);
            (result.code, result.buffer.len())
        }
        // FIXME do C++: Lenient is exactly the same as "replacing unpaired surrogates with FFFD".
        ConversionMode::StrictConversionReplacingUnpairedSurrogatesWithFFFD | ConversionMode::LenientConversion => {
            let result = convert_replacing_invalid_sequences_utf16_to_utf8(characters, &mut buffer);
            (result.code, result.buffer.len())
        }
    };
    if code == ConversionResultCode::SourceInvalid {
        return Err(UTF8ConversionError::Invalid);
    }
    buffer.truncate(converted);
    Ok(buffer)
}

// ---------------------------------------------------------------------------------------------
// Números (IntegerToStringConversion.h)
// ---------------------------------------------------------------------------------------------

/// `numberToStringImpl`: o buffer do C++ tem `sizeof(UnsignedIntegerType) * 3 + 1` posições (25 no
/// maior tipo, o suficiente para todos).
fn integer_to_string(mut number: u64, negative: bool) -> String {
    let mut buffer = [0u8; 25];
    let mut index = buffer.len();
    loop {
        index -= 1;
        buffer[index] = ((number % 10) as u8) + b'0';
        number /= 10;
        if number == 0 {
            break;
        }
    }

    if negative {
        index -= 1;
        buffer[index] = b'-';
    }

    String::from_latin1(&buffer[index..])
}

// ---------------------------------------------------------------------------------------------
// Conversão de texto em número (WTFString.cpp e FastFloat.cpp)
// ---------------------------------------------------------------------------------------------

fn is_integer_unit(unit: u16) -> bool {
    (0x30..=0x39).contains(&unit)
}


/// `toDoubleType<CharacterType, trailingJunkPolicy, whitespacePolicy, mode>`: devolve o número, o
/// `ok` e o `parsedLength`.
fn to_double_type<T: CharType>(
    data: &[T],
    trailing_junk_policy: TrailingJunkPolicy,
    whitespace_policy: WhitespacePolicy,
    mode: ParseMode,
) -> (f64, bool, usize) {
    let mut leading_spaces_length = 0;
    if whitespace_policy == WhitespacePolicy::Skip {
        while leading_spaces_length < data.len()
            && is_unicode_compatible_ascii_whitespace(data[leading_spaces_length].to_u16())
        {
            leading_spaces_length += 1;
        }
    }

    let (number, mut parsed_length) = if mode == ParseMode::Fixed {
        { let mut n = 0; (crate::wtf::fast_float::parse_fixed_double(&data[leading_spaces_length..], &mut n), n) }
    } else {
        { let mut n = 0; (crate::wtf::fast_float::parse_double(&data[leading_spaces_length..], &mut n), n) }
    };

    if parsed_length == 0 {
        return (0.0, false, 0);
    }

    parsed_length += leading_spaces_length;
    let ok = trailing_junk_policy == TrailingJunkPolicy::Allow || parsed_length == data.len();
    (number, ok, parsed_length)
}

/// `charactersToDouble(span, ok)`: devolve o valor e o `ok`.
pub fn characters_to_double<T: CharType>(data: &[T]) -> (f64, bool) {
    let (number, ok, _) = to_double_type(data, TrailingJunkPolicy::Disallow, WhitespacePolicy::Skip, ParseMode::General);
    (number, ok)
}

/// `charactersToFixedDouble(span, ok)`.
pub fn characters_to_fixed_double<T: CharType>(data: &[T]) -> (f64, bool) {
    let (number, ok, _) =
        to_double_type(data, TrailingJunkPolicy::Disallow, WhitespacePolicy::Preserve, ParseMode::Fixed);
    (number, ok)
}

/// `doubleToFloatCheckingOverflow`.
fn double_to_float_checking_overflow(number: f64, is_valid: bool) -> (f32, bool) {
    let result = number as f32;
    if is_valid && number.is_finite() && !result.is_finite() {
        return (result, false);
    }
    (result, is_valid)
}

/// `charactersToFloat(span, ok)`.
pub fn characters_to_float<T: CharType>(data: &[T]) -> (f32, bool) {
    let (number, ok, _) = to_double_type(data, TrailingJunkPolicy::Disallow, WhitespacePolicy::Skip, ParseMode::General);
    double_to_float_checking_overflow(number, ok)
}

/// `charactersToFloat(span, size_t& parsedLength)`: aceita lixo no fim e devolve o comprimento
/// consumido.
pub fn characters_to_float_parsed_length<T: CharType>(data: &[T]) -> (f32, usize) {
    let (number, _, parsed_length) =
        to_double_type(data, TrailingJunkPolicy::Allow, WhitespacePolicy::Skip, ParseMode::General);
    (number as f32, parsed_length)
}

// ---------------------------------------------------------------------------------------------
// trim e simplifyWhiteSpace (StringImpl.cpp)
// ---------------------------------------------------------------------------------------------

/// `StringImpl::trimMatchedCharacters<CharacterType>`.
fn trim_matched_characters<T: CharType>(string: &Rc<StringImpl>, predicate: &impl Fn(u16) -> bool) -> Rc<StringImpl> {
    let span = string.span::<T>();
    if span.is_empty() {
        return Rc::clone(string);
    }

    let mut start = 0;
    let mut end = span.len() - 1;

    // skip matched characters from start
    while start <= end && predicate(span[start].to_u16()) {
        start += 1;
    }

    // only matched characters
    if start > end {
        return StringImpl::empty();
    }

    // skip matched characters from end
    while end != 0 && predicate(span[end].to_u16()) {
        end -= 1;
    }

    if start == 0 && end == span.len() - 1 {
        return Rc::clone(string);
    }
    T::create(&span[start..end + 1])
}

/// `StringImpl::simplifyMatchedCharactersToSpace<CharacterType>`.
fn simplify_matched_characters_to_space<T: CharType>(
    string: &Rc<StringImpl>,
    predicate: &impl Fn(u16) -> bool,
) -> Rc<StringImpl> {
    let from = string.span::<T>();
    let mut to: Vec<T> = Vec::with_capacity(from.len());
    let space = T::from_u16(' ' as u16);
    let mut index = 0;
    let mut changed_to_space = false;

    loop {
        while index < from.len() && predicate(from[index].to_u16()) {
            if from[index].to_u16() != ' ' as u16 {
                changed_to_space = true;
            }
            index += 1;
        }
        while index < from.len() && !predicate(from[index].to_u16()) {
            to.push(from[index]);
            index += 1;
        }
        if index < from.len() {
            to.push(space);
        } else {
            break;
        }
    }

    if let Some(last) = to.last() {
        if last.to_u16() == ' ' as u16 {
            to.pop();
        }
    }

    if to.len() == string.length() as usize && !changed_to_space {
        return Rc::clone(string);
    }

    StringImpl::adopt(to)
}

// ---------------------------------------------------------------------------------------------
// class String
// ---------------------------------------------------------------------------------------------

/// `class String`. Nulo (`None`) é distinguível de vazio.
#[derive(Clone, Default, Debug)]
pub struct String {
    pub(crate) m_impl: Option<Rc<StringImpl>>,
}

/// `String::MaxLength`, também como constante associada.
impl String {
    pub const MAX_LENGTH: u32 = MAX_LENGTH;
}

impl From<Rc<StringImpl>> for String {
    /// `String(Ref<StringImpl>&&)`.
    fn from(string: Rc<StringImpl>) -> String {
        String { m_impl: Some(string) }
    }
}

impl From<Option<Rc<StringImpl>>> for String {
    /// `String(RefPtr<StringImpl>&&)`.
    fn from(string: Option<Rc<StringImpl>>) -> String {
        String { m_impl: string }
    }
}

impl PartialEq for String {
    /// `operator==(const String&, const String&)`: `equal(a.impl(), b.impl())`.
    fn eq(&self, other: &String) -> bool {
        equal_impl(self.m_impl.as_ref(), other.m_impl.as_ref())
    }
}

impl Eq for String {}

/// `DefaultHash<String>`: o hash da WTF sobre o conteúdo (o da string nula é 0), coerente com a
/// igualdade por conteúdo acima.
impl std::hash::Hash for String {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write_u32(self.m_impl.as_ref().map_or(0, |s| s.hash()));
    }
}

impl String {
    // ---- construção ----------------------------------------------------------------------

    /// `String(std::span<const char16_t>)`: dados UTF-16, sem estreitar.
    pub fn from_utf16(characters: &[u16]) -> String {
        String::from(StringImpl::create16(characters))
    }

    /// `String(std::span<const Latin1Character>)`, `String(std::span<const char>)` e
    /// `String::fromLatin1`: dados Latin1.
    pub fn from_latin1(characters: &[u8]) -> String {
        String::from(StringImpl::create(characters))
    }

    /// `String(std::span<const char8_t>)` e `String::fromUTF8`: nula se houver UTF-8 inválido.
    pub fn from_utf8(code_units: &[u8]) -> String {
        String::from(StringImpl::create_from_utf8(code_units))
    }

    /// `String::fromUTF8ReplacingInvalidSequences`.
    pub fn from_utf8_replacing_invalid_sequences(string: &[u8]) -> String {
        // RELEASE_ASSERT(string.size() <= String::MaxLength)
        assert!(string.len() <= MAX_LENGTH as usize);

        if string.is_empty() {
            return empty_string();
        }

        if string.is_ascii() {
            return String::from(StringImpl::create(string));
        }

        let mut buffer = vec![0u16; string.len()];
        let result = convert_replacing_invalid_sequences_utf8_to_utf16(string, &mut buffer);
        if result.code != ConversionResultCode::Success {
            return String::default();
        }

        assert!(result.buffer.len() <= string.len());
        String::from(StringImpl::create16(result.buffer))
    }

    /// `String::fromUTF8WithLatin1Fallback`: tenta UTF-8 e cai para Latin1 se for inválido.
    pub fn from_utf8_with_latin1_fallback(string: &[u8]) -> String {
        let utf8 = String::from_utf8(string);
        if utf8.is_null() {
            // Do this assertion before chopping the size_t down to unsigned.
            assert!(string.len() <= MAX_LENGTH as usize);
            return String::from_latin1(string);
        }
        utf8
    }

    /// `String::fromCodePoint`: o `U16_APPEND` do ICU só falha acima de U+10FFFF (substituto isolado
    /// entra como unidade única).
    pub fn from_code_point(code_point: u32) -> String {
        if code_point <= 0xFFFF {
            return String::from_utf16(&[code_point as u16]);
        }
        if code_point <= 0x10FFFF {
            let offset = code_point - 0x10000;
            let lead = 0xD800 + (offset >> 10) as u16;
            let trail = 0xDC00 + (offset & 0x3FF) as u16;
            return String::from_utf16(&[lead, trail]);
        }
        String::default()
    }

    /// `String::adopt(Vector&&)` e `String::adopt(StringBuffer&&)`.
    pub fn adopt<T: CharType>(vector: Vec<T>) -> String {
        String::from(StringImpl::adopt(vector))
    }

    /// `String::createUninitialized(length, data)`: `fill` escreve os caracteres.
    pub fn create_uninitialized<T: CharType>(length: usize, fill: impl FnOnce(&mut [T])) -> String {
        String::from(StringImpl::create_uninitialized(length, fill))
    }

    /// `String::make8Bit`: o chamador garante Latin1.
    pub fn make_8bit(source: &[u16]) -> String {
        String::from(StringImpl::create8_bit_unconditionally(source))
    }

    /// `String::convertTo16Bit`.
    pub fn convert_to_16bit(&mut self) {
        if self.is_null() || !self.is_8bit() {
            return;
        }
        let converted = String::create_uninitialized::<u16>(self.length() as usize, |destination| {
            copy_characters_widen(destination, self.span8());
        });
        *self = converted;
    }

    // ---- consultas básicas ---------------------------------------------------------------

    /// Os caracteres da string; nula dá um span Latin1 vazio.
    fn chars(&self) -> Chars<'_> {
        match &self.m_impl {
            Some(string) => Chars::of(string),
            None => Chars::Latin1(&[]),
        }
    }

    pub fn is_null(&self) -> bool {
        self.m_impl.is_none()
    }

    pub fn is_empty(&self) -> bool {
        match &self.m_impl {
            Some(string) => string.is_empty(),
            None => true,
        }
    }

    /// `impl()`.
    pub fn impl_(&self) -> Option<&Rc<StringImpl>> {
        self.m_impl.as_ref()
    }

    /// `releaseImpl()`: deixa a string nula.
    pub fn release_impl(&mut self) -> Option<Rc<StringImpl>> {
        self.m_impl.take()
    }

    pub fn length(&self) -> u32 {
        match &self.m_impl {
            Some(string) => string.length(),
            None => 0,
        }
    }

    pub fn span8(&self) -> &[u8] {
        match &self.m_impl {
            Some(string) => string.span8(),
            None => &[],
        }
    }

    pub fn span16(&self) -> &[u16] {
        match &self.m_impl {
            Some(string) => string.span16(),
            None => &[],
        }
    }

    /// `span<CharacterType>()`.
    pub fn span<T: CharType>(&self) -> &[T] {
        match &self.m_impl {
            Some(string) => string.span::<T>(),
            None => &[],
        }
    }

    pub fn is_8bit(&self) -> bool {
        match &self.m_impl {
            Some(string) => string.is_8bit(),
            None => true,
        }
    }

    pub fn size_in_bytes(&self) -> u32 {
        match &self.m_impl {
            Some(string) => string.length() * if self.is_8bit() { 1 } else { 2 },
            None => 0,
        }
    }

    /// `codeUnitAt(index)` (e `operator[]`): zero fora do intervalo.
    pub fn code_unit_at(&self, index: u32) -> u16 {
        match &self.m_impl {
            Some(string) if index < string.length() => string.char_at(index),
            _ => 0,
        }
    }

    /// `codePointAt(i)`.
    pub fn code_point_at(&self, i: u32) -> u32 {
        match &self.m_impl {
            Some(string) if i < string.length() => string.code_point_at(i),
            _ => 0,
        }
    }

    pub fn contains_only_ascii(&self) -> bool {
        match &self.m_impl {
            Some(string) => string.contains_only_ascii(),
            None => true,
        }
    }

    pub fn contains_only_latin1(&self) -> bool {
        match &self.m_impl {
            Some(string) => string.contains_only_latin1(),
            None => true,
        }
    }

    /// `containsOnly<isSpecialCharacter>()`.
    pub fn contains_only(&self, is_special_character: fn(u16) -> bool) -> bool {
        match &self.m_impl {
            Some(string) => string.contains_only(is_special_character),
            None => true,
        }
    }

    pub fn hash(&self) -> u32 {
        match &self.m_impl {
            Some(string) => string.hash(),
            None => 0,
        }
    }

    pub fn existing_hash(&self) -> u32 {
        match &self.m_impl {
            Some(string) => string.existing_hash(),
            None => 0,
        }
    }

    /// `clearImplIfNotShared()`: deixa a string nula se mais ninguém referencia o `StringImpl`.
    pub fn clear_impl_if_not_shared(&mut self) {
        if let Some(string) = &self.m_impl {
            if Rc::strong_count(string) == 1 {
                self.m_impl = None;
            }
        }
    }

    /// `operator==(const String&, ASCIILiteral)`: `equal(a.impl(), literal)`. O literal nulo do C++
    /// é `None`.
    pub fn equals_latin1(&self, literal: Option<&[u8]>) -> bool {
        match (&self.m_impl, literal) {
            (None, literal) => literal.is_none(),
            (Some(_), None) => false,
            (Some(string), Some(literal)) => {
                string.length() as usize == literal.len()
                    && (literal.is_empty() || equal_prefix_chars(Chars::of(string), literal))
            }
        }
    }

    // ---- conversões para bytes -----------------------------------------------------------

    /// `ascii()`: os caracteres ASCII imprimíveis (32 a 127) e o nulo ficam, o resto vira `?`.
    pub fn ascii(&self) -> Vec<u8> {
        // Printable ASCII characters 32..127 and the null character are
        // preserved, characters outside of this range are converted to '?'.
        if self.is_empty() {
            return Vec::new();
        }

        let convert = |character: u16| -> u8 {
            if character != 0 && (character < 0x20 || character > 0x7f) {
                b'?'
            } else {
                character as u8
            }
        };

        match self.chars() {
            Chars::Latin1(characters) => characters.iter().map(|&c| convert(c as u16)).collect(),
            Chars::Utf16(characters) => characters.iter().map(|&c| convert(c)).collect(),
        }
    }

    /// `latin1()`: 0..255 ficam, o resto vira `?`.
    pub fn latin1(&self) -> Vec<u8> {
        // Basic Latin1 (ISO) encoding - Unicode characters 0..255 are
        // preserved, characters outside of this range are converted to '?'.
        if self.is_empty() {
            return Vec::new();
        }

        match self.chars() {
            Chars::Latin1(characters) => characters.to_vec(),
            Chars::Utf16(characters) => characters.iter().map(|&c| if c > 0xFF { b'?' } else { c as u8 }).collect(),
        }
    }

    /// `tryGetUTF8(mode)`.
    pub fn try_get_utf8(&self, mode: ConversionMode) -> Result<Vec<u8>, UTF8ConversionError> {
        match &self.m_impl {
            None => Ok(Vec::new()),
            Some(string) => {
                if string.is_8bit() {
                    utf8_for_characters_latin1(string.span8())
                } else {
                    utf8_for_characters_utf16(string.span16(), mode)
                }
            }
        }
    }

    /// `utf8(mode)`: o C++ faz `RELEASE_ASSERT` no resultado de `tryGetUTF8`.
    pub fn utf8(&self, mode: ConversionMode) -> Vec<u8> {
        match self.try_get_utf8(mode) {
            Ok(converted) => converted,
            Err(error) => panic!("String::utf8: conversão falhou: {:?}", error),
        }
    }

    /// `charactersWithoutNullTermination()`.
    pub fn characters_without_null_termination(&self) -> Result<Vec<u16>, UTF8ConversionError> {
        let mut result: Vec<u16> = Vec::new();
        if self.m_impl.is_none() {
            return Ok(result);
        }

        if !is_valid_capacity_for_vector(self.length() as usize + 1, 2) {
            return Err(UTF8ConversionError::OutOfMemory);
        }
        result.reserve_exact(self.length() as usize + 1);

        match self.chars() {
            Chars::Latin1(characters) => result.extend(characters.iter().map(|&c| c as u16)),
            Chars::Utf16(characters) => result.extend_from_slice(characters),
        }

        Ok(result)
    }

    /// `charactersWithNullTermination()`.
    pub fn characters_with_null_termination(&self) -> Result<Vec<u16>, UTF8ConversionError> {
        let mut result = self.characters_without_null_termination()?;
        result.push(0);
        Ok(result)
    }

    // ---- números -------------------------------------------------------------------------

    /// `number(int)`.
    pub fn number_i32(number: i32) -> String {
        integer_to_string(number.unsigned_abs() as u64, number < 0)
    }

    /// `number(unsigned)`.
    pub fn number_u32(number: u32) -> String {
        integer_to_string(number as u64, false)
    }

    /// `number(long)` e `number(long long)` (ambos de 64 bits no Linux x86_64).
    pub fn number_i64(number: i64) -> String {
        integer_to_string(number.unsigned_abs(), number < 0)
    }

    /// `number(unsigned long)` e `number(unsigned long long)`.
    pub fn number_u64(number: u64) -> String {
        integer_to_string(number, false)
    }

    /// `number(double)`: `numberToStringAndSize` (a tradução do `wtf/dtoa`).
    pub fn number_f64(number: f64) -> String {
        let mut buffer: NumberToStringBuffer = [0; 124];
        String::from_latin1(number_to_string_and_size(number, &mut buffer))
    }

    // ---- busca ---------------------------------------------------------------------------

    /// `find(char16_t / Latin1Character / char, start)`.
    pub fn find_character(&self, character: u16, start: u32) -> usize {
        match &self.m_impl {
            Some(string) => string.find_character(character, start as usize),
            None => NOT_FOUND,
        }
    }

    /// `find(StringView)`.
    pub fn find(&self, match_string: &String) -> usize {
        match &self.m_impl {
            Some(string) => impl_find(string, match_string),
            None => NOT_FOUND,
        }
    }

    /// `find(StringView, start)`.
    pub fn find_from(&self, match_string: &String, start: u32) -> usize {
        match &self.m_impl {
            // Check for null or empty string to match against
            Some(_) if match_string.is_null() => NOT_FOUND,
            Some(string) => find_common(Chars::of(string), match_string.chars(), start as usize),
            None => NOT_FOUND,
        }
    }

    /// `find(CodeUnitMatchFunction, start)`.
    pub fn find_matching(&self, match_function: impl Fn(u16) -> bool, start: u32) -> usize {
        match &self.m_impl {
            Some(string) => string.find_matching(match_function, start as usize),
            None => NOT_FOUND,
        }
    }

    /// `findIgnoringASCIICase(StringView)`.
    pub fn find_ignoring_ascii_case(&self, match_string: &String) -> usize {
        self.find_ignoring_ascii_case_from(match_string, 0)
    }

    /// `findIgnoringASCIICase(StringView, start)`.
    pub fn find_ignoring_ascii_case_from(&self, match_string: &String, start: u32) -> usize {
        match &self.m_impl {
            Some(_) if match_string.is_null() => NOT_FOUND,
            Some(string) => find_ignoring_ascii_case(Chars::of(string), match_string.chars(), start as usize),
            None => NOT_FOUND,
        }
    }

    /// `reverseFind(char16_t, start)`; o `start` padrão do C++ é `MaxLength`.
    pub fn reverse_find_character(&self, character: u16, start: u32) -> usize {
        match &self.m_impl {
            Some(string) => match Chars::of(string) {
                Chars::Latin1(span) => string_impl::reverse_find_8_char16(span, character, start as usize),
                Chars::Utf16(span) => string_impl::reverse_find(span, character, start as usize),
            },
            None => NOT_FOUND,
        }
    }

    /// `reverseFind(StringView, start)`; o `start` padrão do C++ é `MaxLength`.
    pub fn reverse_find(&self, match_string: &String, start: u32) -> usize {
        match &self.m_impl {
            Some(string) => impl_reverse_find(string, match_string, start as usize),
            None => NOT_FOUND,
        }
    }

    /// `contains(char16_t)`.
    pub fn contains_character(&self, character: u16) -> bool {
        self.find_character(character, 0) != NOT_FOUND
    }

    /// `contains(StringView)`.
    pub fn contains(&self, match_string: &String) -> bool {
        self.find(match_string) != NOT_FOUND
    }

    /// `contains(CodeUnitMatchFunction)`.
    pub fn contains_matching(&self, match_function: impl Fn(u16) -> bool) -> bool {
        self.find_matching(match_function, 0) != NOT_FOUND
    }

    /// `containsIgnoringASCIICase(StringView)`.
    pub fn contains_ignoring_ascii_case(&self, match_string: &String) -> bool {
        self.find_ignoring_ascii_case(match_string) != NOT_FOUND
    }

    /// `containsIgnoringASCIICase(StringView, start)`.
    pub fn contains_ignoring_ascii_case_from(&self, match_string: &String, start: u32) -> bool {
        self.find_ignoring_ascii_case_from(match_string, start) != NOT_FOUND
    }

    /// `startsWith(StringView)`: a string nula só começa com o vazio.
    pub fn starts_with(&self, prefix: &String) -> bool {
        match &self.m_impl {
            Some(string) => prefix.is_null() || starts_with_chars(Chars::of(string), prefix.chars()),
            None => prefix.is_empty(),
        }
    }

    /// `startsWithIgnoringASCIICase(StringView)`.
    pub fn starts_with_ignoring_ascii_case(&self, prefix: &String) -> bool {
        match &self.m_impl {
            Some(string) => {
                !prefix.is_null() && starts_with_ignoring_ascii_case_chars(Chars::of(string), prefix.chars())
            }
            None => prefix.is_empty(),
        }
    }

    /// `startsWith(char16_t)`.
    pub fn starts_with_character(&self, character: u16) -> bool {
        match &self.m_impl {
            Some(string) => string.length() != 0 && string.char_at(0) == character,
            None => false,
        }
    }

    /// `hasInfixStartingAt(prefix, start)`.
    pub fn has_infix_starting_at(&self, prefix: &String, start: u32) -> bool {
        match &self.m_impl {
            Some(string) => !prefix.is_null() && equal_inner(string, start, prefix),
            None => false,
        }
    }

    /// `endsWith(StringView)`: a string nula só termina com o vazio.
    pub fn ends_with(&self, suffix: &String) -> bool {
        match &self.m_impl {
            Some(string) => !suffix.is_null() && ends_with_chars(Chars::of(string), suffix.chars()),
            None => suffix.is_empty(),
        }
    }

    /// `endsWithIgnoringASCIICase(StringView)`.
    pub fn ends_with_ignoring_ascii_case(&self, suffix: &String) -> bool {
        match &self.m_impl {
            Some(string) => {
                !suffix.is_null() && ends_with_ignoring_ascii_case_chars(Chars::of(string), suffix.chars())
            }
            None => suffix.is_empty(),
        }
    }

    /// `endsWith(char16_t)`.
    pub fn ends_with_character(&self, character: u16) -> bool {
        match &self.m_impl {
            Some(string) => string.length() != 0 && string.char_at(string.length() - 1) == character,
            None => false,
        }
    }

    /// `hasInfixEndingAt(suffix, end)`.
    pub fn has_infix_ending_at(&self, suffix: &String, end: u32) -> bool {
        match &self.m_impl {
            Some(string) => {
                !suffix.is_null()
                    && end >= suffix.length()
                    && equal_inner(string, end - suffix.length(), suffix)
            }
            None => false,
        }
    }

    // ---- substrings ----------------------------------------------------------------------

    /// `substring(position, length)`; o `length` padrão do C++ é `MaxLength`.
    pub fn substring(&self, position: u32, length: u32) -> String {
        let Some(string) = &self.m_impl else {
            return String::default();
        };

        if position == 0 && length >= string.length() {
            return self.clone();
        }

        String::from(string.substring(position, length))
    }

    /// `substringSharingImpl(position, length)`; o `length` padrão do C++ é `MaxLength`.
    pub fn substring_sharing_impl(&self, offset: u32, length: u32) -> String {
        // FIXME: We used to check against a limit of Heap::minExtraCost / sizeof(char16_t).
        let string_length = self.length();
        let offset = std::cmp::min(offset, string_length);
        let length = std::cmp::min(length, string_length - offset);

        match &self.m_impl {
            Some(string) if !(offset == 0 && length == string_length) => {
                String::from(StringImpl::create_substring_sharing_impl(string, offset, length))
            }
            _ => self.clone(),
        }
    }

    /// `left(length)`.
    pub fn left(&self, length: u32) -> String {
        self.substring(0, length)
    }

    /// `right(length)`: a conta do C++ é em `unsigned`, com volta.
    pub fn right(&self, length: u32) -> String {
        self.substring(self.length().wrapping_sub(length), length)
    }

    // ---- caixa e limpeza -----------------------------------------------------------------

    /// Aplica `convert` ao `StringImpl` se a string não é nula (`m_impl ? ... : String { }`).
    fn map_impl(&self, convert: impl FnOnce(&Rc<StringImpl>) -> Rc<StringImpl>) -> String {
        match &self.m_impl {
            Some(string) => String::from(convert(string)),
            None => String::default(),
        }
    }

    pub fn convert_to_ascii_lowercase(&self) -> String {
        self.map_impl(|s| s.convert_to_ascii_lowercase())
    }

    pub fn convert_to_ascii_uppercase(&self) -> String {
        self.map_impl(|s| s.convert_to_ascii_uppercase())
    }

    pub fn convert_to_lowercase_without_locale(&self) -> String {
        self.map_impl(|s| s.convert_to_lowercase_without_locale())
    }

    pub fn convert_to_lowercase_without_locale_starting_at_failing_index8_bit(&self, failing_index: u32) -> String {
        self.map_impl(|s| s.convert_to_lowercase_without_locale_starting_at_failing_index8_bit(failing_index))
    }

    pub fn convert_to_lowercase_without_locale_starting_at_failing_index16_bit(&self, failing_index: u32) -> String {
        self.map_impl(|s| s.convert_to_lowercase_without_locale_starting_at_failing_index16_bit(failing_index))
    }

    pub fn convert_to_uppercase_without_locale(&self) -> String {
        self.map_impl(|s| s.convert_to_uppercase_without_locale())
    }

    pub fn convert_to_uppercase_without_locale_starting_at_failing_index8_bit(&self, failing_index: u32) -> String {
        self.map_impl(|s| s.convert_to_uppercase_without_locale_starting_at_failing_index8_bit(failing_index))
    }

    pub fn convert_to_uppercase_without_locale_starting_at_failing_index16_bit(&self, failing_index: u32) -> String {
        self.map_impl(|s| s.convert_to_uppercase_without_locale_starting_at_failing_index16_bit(failing_index))
    }

    /// `convertToLowercaseWithLocale(const AtomString&)`; o átomo nulo se comporta como vazio.
    pub fn convert_to_lowercase_with_locale(&self, locale_identifier: &String) -> String {
        let locale = locale_identifier.m_impl.clone().unwrap_or_else(StringImpl::empty);
        self.map_impl(|s| s.convert_to_lowercase_with_locale(&locale))
    }

    /// `convertToUppercaseWithLocale(const AtomString&)`.
    pub fn convert_to_uppercase_with_locale(&self, locale_identifier: &String) -> String {
        let locale = locale_identifier.m_impl.clone().unwrap_or_else(StringImpl::empty);
        self.map_impl(|s| s.convert_to_uppercase_with_locale(&locale))
    }

    /// `foldCase()`.
    pub fn fold_case(&self) -> String {
        self.map_impl(|s| s.fold_case())
    }

    /// `trim(CodeUnitMatchFunction)`.
    pub fn trim(&self, predicate: impl Fn(u16) -> bool) -> String {
        self.map_impl(|s| {
            if s.is_8bit() {
                trim_matched_characters::<u8>(s, &predicate)
            } else {
                trim_matched_characters::<u16>(s, &predicate)
            }
        })
    }

    /// `simplifyWhiteSpace(CodeUnitMatchFunction)`.
    pub fn simplify_white_space(&self, is_white_space: impl Fn(u16) -> bool) -> String {
        self.map_impl(|s| {
            if s.is_8bit() {
                simplify_matched_characters_to_space::<u8>(s, &is_white_space)
            } else {
                simplify_matched_characters_to_space::<u16>(s, &is_white_space)
            }
        })
    }

    /// `removeCharacters(predicate)`.
    pub fn remove_characters(&self, find_match: impl Fn(u16) -> bool) -> String {
        self.map_impl(|s| s.remove_characters(find_match))
    }

    // ---- divisão -------------------------------------------------------------------------

    /// `splitInternal<allowEmptyEntries>(char16_t, functor)`.
    fn split_internal_with<const ALLOW_EMPTY_ENTRIES: bool>(
        &self,
        separator: u16,
        mut functor: impl FnMut(&String),
    ) {
        let mut start_pos: u32 = 0;
        loop {
            let end_pos = self.find_character(separator, start_pos);
            if end_pos == NOT_FOUND {
                break;
            }
            if ALLOW_EMPTY_ENTRIES || start_pos as usize != end_pos {
                functor(&view_substring(self, start_pos, end_pos as u32 - start_pos));
            }
            start_pos = end_pos as u32 + 1;
        }
        if ALLOW_EMPTY_ENTRIES || start_pos != self.length() {
            functor(&view_substring(self, start_pos, MAX_LENGTH));
        }
    }

    /// `splitInternal<allowEmptyEntries>(StringView separator)`. Um separador vazio não avança e
    /// não termina, como no C++.
    fn split_internal_string<const ALLOW_EMPTY_ENTRIES: bool>(&self, separator: &String) -> Vec<String> {
        let mut result: Vec<String> = Vec::new();

        let mut start_pos: u32 = 0;
        loop {
            let end_pos = self.find_from(separator, start_pos);
            if end_pos == NOT_FOUND {
                break;
            }
            if ALLOW_EMPTY_ENTRIES || start_pos as usize != end_pos {
                result.push(self.substring(start_pos, end_pos as u32 - start_pos));
            }
            start_pos = (end_pos as u32).wrapping_add(separator.length());
        }
        if ALLOW_EMPTY_ENTRIES || start_pos != self.length() {
            result.push(self.substring(start_pos, MAX_LENGTH));
        }

        result
    }

    /// `splitInternal<allowEmptyEntries>(char16_t)`.
    fn split_internal<const ALLOW_EMPTY_ENTRIES: bool>(&self, separator: u16) -> Vec<String> {
        let mut result: Vec<String> = Vec::new();
        self.split_internal_with::<ALLOW_EMPTY_ENTRIES>(separator, |item| result.push(item.clone()));
        result
    }

    /// `split(char16_t, functor)`.
    pub fn split_with(&self, separator: u16, functor: impl FnMut(&String)) {
        self.split_internal_with::<false>(separator, functor);
    }

    /// `split(char16_t)`.
    pub fn split(&self, separator: u16) -> Vec<String> {
        self.split_internal::<false>(separator)
    }

    /// `split(StringView)`.
    pub fn split_string(&self, separator: &String) -> Vec<String> {
        self.split_internal_string::<false>(separator)
    }

    /// `splitAllowingEmptyEntries(char16_t, functor)`.
    pub fn split_allowing_empty_entries_with(&self, separator: u16, functor: impl FnMut(&String)) {
        self.split_internal_with::<true>(separator, functor);
    }

    /// `splitAllowingEmptyEntries(char16_t)`.
    pub fn split_allowing_empty_entries(&self, separator: u16) -> Vec<String> {
        self.split_internal::<true>(separator)
    }

    /// `splitAllowingEmptyEntries(StringView)`.
    pub fn split_allowing_empty_entries_string(&self, separator: &String) -> Vec<String> {
        self.split_internal_string::<true>(separator)
    }

    // ---- números a partir do texto -------------------------------------------------------

    /// `toDouble(bool* ok)`: o valor e o `ok`. A string nula dá `(0.0, false)`.
    pub fn to_double(&self) -> (f64, bool) {
        match self.chars() {
            _ if self.m_impl.is_none() => (0.0, false),
            Chars::Latin1(data) => characters_to_double(data),
            Chars::Utf16(data) => characters_to_double(data),
        }
    }

    /// `toFloat(bool* ok)`.
    pub fn to_float(&self) -> (f32, bool) {
        match self.chars() {
            _ if self.m_impl.is_none() => (0.0, false),
            Chars::Latin1(data) => characters_to_float(data),
            Chars::Utf16(data) => characters_to_float(data),
        }
    }

    // ---- cópia entre threads -------------------------------------------------------------

    /// `isolatedCopy() const &`.
    pub fn isolated_copy(&self) -> String {
        self.map_impl(|s| s.isolated_copy())
    }

    /// `isSafeToSendToAnotherThread()`: um átomo não é seguro, porque o destrutor tentaria
    /// removê-lo da tabela da thread errada.
    pub fn is_safe_to_send_to_another_thread(&self) -> bool {
        match &self.m_impl {
            None => true,
            Some(string) => string.is_empty() || (Rc::strong_count(string) == 1 && !string.is_atom()),
        }
    }

    /// `isolatedCopy() &&`: reaproveita o `StringImpl` quando é seguro.
    pub fn into_isolated_copy(self) -> String {
        if self.is_safe_to_send_to_another_thread() {
            // Since we know that our string is a temporary that will be destroyed
            // we can just steal the m_impl from it, thus avoiding a copy.
            return self;
        }
        self.map_impl(|s| s.isolated_copy())
    }
}

/// `StringView::substring(start, length)` aplicado à visão de um `String`: fora do intervalo dá a
/// visão vazia (não nula); o intervalo inteiro devolve a própria visão.
fn view_substring(string: &String, start: u32, length: u32) -> String {
    if start >= string.length() {
        return empty_string();
    }
    let max_length = string.length() - start;
    let mut length = length;

    if length >= max_length {
        if start == 0 {
            return string.clone();
        }
        length = max_length;
    }

    let (start, length) = (start as usize, length as usize);
    match string.chars() {
        Chars::Latin1(span) => String::from(StringImpl::create(&span[start..start + length])),
        Chars::Utf16(span) => String::from(StringImpl::create16(&span[start..start + length])),
    }
}

/// `equal(span, span)` para o `ASCIILiteral`: tamanhos já conferidos pelo chamador.
fn equal_prefix_chars(characters: Chars, literal: &[u8]) -> bool {
    match characters {
        Chars::Latin1(span) => equal_prefix(span, literal),
        Chars::Utf16(span) => equal_prefix(span, literal),
    }
}

// ---------------------------------------------------------------------------------------------
// Funções livres do cabeçalho e do .cpp
// ---------------------------------------------------------------------------------------------

/// `nullString()`.
pub fn null_string() -> String {
    String::default()
}

/// `emptyString()`: a string vazia atômica e estática.
pub fn empty_string() -> String {
    String::from(StringImpl::empty())
}

/// `equalIgnoringASCIICase(const String&, const String&)`: ponteiros iguais, ou ambos não nulos
/// com o mesmo tamanho e os mesmos caracteres sem distinguir a caixa ASCII.
pub fn equal_ignoring_ascii_case_string(a: &String, b: &String) -> bool {
    match (a.impl_(), b.impl_()) {
        (None, None) => true,
        (Some(x), Some(y)) => {
            Rc::ptr_eq(x, y)
                || (x.length() == y.length()
                    && dispatch2!(Chars::of(x), Chars::of(y), |p, q| equal_ignoring_ascii_case_with_length(
                        p,
                        q,
                        q.len()
                    )))
        }
        _ => false,
    }
}

/// `equalLettersIgnoringASCIICase(const String&, ASCIILiteral)`: `lowercase_letters` já em
/// minúsculas (letra, dígito ou pontuação de 0x21 a 0x3F).
pub fn equal_letters_ignoring_ascii_case(string: &String, lowercase_letters: &[u8]) -> bool {
    match string.impl_() {
        None => false,
        Some(string) => {
            string.length() as usize == lowercase_letters.len()
                && match Chars::of(string) {
                    Chars::Latin1(span) => {
                        equal_letters_ignoring_ascii_case_with_length(span, lowercase_letters, lowercase_letters.len())
                    }
                    Chars::Utf16(span) => {
                        equal_letters_ignoring_ascii_case_with_length(span, lowercase_letters, lowercase_letters.len())
                    }
                }
        }
    }
}

/// `startsWithLettersIgnoringASCIICase(const String&, ASCIILiteral)`.
pub fn starts_with_letters_ignoring_ascii_case(string: &String, lowercase_letters: &[u8]) -> bool {
    match string.impl_() {
        None => false,
        Some(string) => {
            if lowercase_letters.is_empty() {
                return true;
            }
            if (string.length() as usize) < lowercase_letters.len() {
                return false;
            }
            match Chars::of(string) {
                Chars::Latin1(span) => {
                    equal_letters_ignoring_ascii_case_with_length(span, lowercase_letters, lowercase_letters.len())
                }
                Chars::Utf16(span) => {
                    equal_letters_ignoring_ascii_case_with_length(span, lowercase_letters, lowercase_letters.len())
                }
            }
        }
    }
}

/// `equalIgnoringNullity(const String&, const String&)`: nulo e vazio contam como iguais.
pub fn equal_ignoring_nullity(a: &String, b: &String) -> bool {
    if a.is_null() && !b.is_null() && b.length() == 0 {
        return true;
    }
    if b.is_null() && !a.is_null() && a.length() == 0 {
        return true;
    }
    equal_impl(a.impl_(), b.impl_())
}

/// `equalIgnoringNullity(const Vector<char16_t>&, const String&)`.
pub fn equal_ignoring_nullity_utf16(a: &[u16], b: &String) -> bool {
    let Some(string) = b.impl_() else {
        return a.is_empty();
    };
    if a.len() != string.length() as usize {
        return false;
    }
    match Chars::of(string) {
        Chars::Latin1(span) => a.iter().zip(span.iter()).all(|(x, y)| *x == *y as u16),
        Chars::Utf16(span) => a == span,
    }
}

/// `makeStringByReplacingAll(string, target, replacement)`.
pub fn make_string_by_replacing_all(string: &String, target: u16, replacement: u16) -> String {
    let Some(impl_) = string.impl_() else {
        return string.clone();
    };
    if target == replacement {
        return string.clone();
    }

    // find() devolve notFound para um alvo fora de Latin1 numa string de 8 bits.
    let replaced = match Chars::of(impl_) {
        Chars::Latin1(characters) => {
            let i = string_impl::find(characters, |c| c == target, 0);
            if i == NOT_FOUND {
                return string.clone();
            }
            StringImpl::create_by_replacing_in_characters8(characters, target, replacement, i)
        }
        Chars::Utf16(characters) => {
            let i = string_impl::find(characters, |c| c == target, 0);
            if i == NOT_FOUND {
                return string.clone();
            }
            StringImpl::create_by_replacing_in_characters16(characters, target, replacement, i)
        }
    };
    String::from(replaced)
}

/// `makeStringByRemoving(string, position, lengthToRemove)`: o `makeString(left, rest)` do C++
/// guarda a largura da string de origem.
pub fn make_string_by_removing(string: &String, position: u32, length_to_remove: u32) -> String {
    if length_to_remove == 0 {
        return string.clone();
    }
    let length = string.length();
    if position >= length {
        return string.clone();
    }
    let length_to_remove = std::cmp::min(length_to_remove, length - position);
    let (position, end_of_removed) = (position as usize, (position + length_to_remove) as usize);

    match string.chars() {
        Chars::Latin1(span) => {
            let mut result: Vec<u8> = span[..position].to_vec();
            result.extend_from_slice(&span[end_of_removed..]);
            String::from(StringImpl::create(&result))
        }
        Chars::Utf16(span) => {
            let mut result: Vec<u16> = span[..position].to_vec();
            result.extend_from_slice(&span[end_of_removed..]);
            String::from(StringImpl::create16(&result))
        }
    }
}

/// `StringBuilder::append(const String&)`: nulo e vazio não mudam nada; um pedaço de 16 bits
/// não vazio tira o acumulado dos 8 bits.
fn append_to_builder(result: &mut Vec<u16>, is_8bit: &mut bool, piece: &String) {
    if piece.length() == 0 {
        return;
    }
    match piece.chars() {
        Chars::Latin1(span) => result.extend(span.iter().map(|&c| c as u16)),
        Chars::Utf16(span) => {
            *is_8bit = false;
            result.extend_from_slice(span);
        }
    }
}

/// `makeStringByJoining(strings, separator)`: o `StringBuilder` do C++ só põe o separador depois de
/// o acumulado deixar de ser vazio, e fica de 8 bits até entrar um pedaço não vazio de 16 bits.
pub fn make_string_by_joining(strings: &[String], separator: &String) -> String {
    let mut result: Vec<u16> = Vec::new();
    let mut is_8bit = true;

    for string in strings {
        if result.is_empty() {
            append_to_builder(&mut result, &mut is_8bit, string);
        } else {
            append_to_builder(&mut result, &mut is_8bit, separator);
            append_to_builder(&mut result, &mut is_8bit, string);
        }
    }

    if result.is_empty() {
        return empty_string();
    }
    if is_8bit {
        return String::from(StringImpl::create8_bit_unconditionally(&result));
    }
    String::from(StringImpl::create16(&result))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Constrói uma string a partir de texto Rust (UTF-8).
    fn s(text: &str) -> String {
        String::from_utf8(text.as_bytes())
    }

    /// O conteúdo Latin1 como texto Rust, para comparar nas asserções.
    fn text(string: &String) -> std::string::String {
        std::string::String::from_utf8_lossy(&string.latin1()).into_owned()
    }

    #[test]
    fn null_and_empty_are_distinct() {
        let null = String::default();
        let empty = empty_string();
        assert!(null.is_null() && null.is_empty());
        assert!(!empty.is_null() && empty.is_empty());
        assert_ne!(null, empty);
        assert_eq!(null, null_string());
        assert_eq!(null.length(), 0);
        assert_eq!(null.code_unit_at(0), 0);
    }

    #[test]
    fn equality_across_widths() {
        assert_eq!(String::from_latin1(b"abc"), String::from_utf16(&[97, 98, 99]));
        assert_ne!(String::from_latin1(b"abc"), String::from_latin1(b"abd"));
        assert_ne!(String::from_latin1(b"abc"), String::from_latin1(b"ab"));
        assert!(s("abc").equals_latin1(Some(b"abc")));
        assert!(!s("abc").equals_latin1(Some(b"abd")));
        assert!(String::default().equals_latin1(None));
        assert!(!s("abc").equals_latin1(None));
    }

    #[test]
    fn code_unit_and_code_point() {
        let string = String::from_utf16(&[0x61, 0xD83D, 0xDE00]);
        assert_eq!(string.code_unit_at(0), 0x61);
        assert_eq!(string.code_unit_at(3), 0);
        assert_eq!(string.code_point_at(1), 0x1F600);
        assert_eq!(string.code_point_at(5), 0);
        assert_eq!(string.length(), 3);
        assert_eq!(string.size_in_bytes(), 6);
        assert_eq!(s("abc").size_in_bytes(), 3);
    }

    #[test]
    fn find_family() {
        let hello = s("hello world");
        assert_eq!(hello.find(&s("o w")), 4);
        assert_eq!(hello.find(&s("xyz")), NOT_FOUND);
        assert_eq!(hello.find(&s("")), 0);
        assert_eq!(hello.find(&String::default()), NOT_FOUND);
        assert_eq!(hello.find(&s("w")), 6);
        assert_eq!(hello.find_from(&s("o"), 5), 7);
        assert_eq!(hello.find_from(&s("o"), 100), NOT_FOUND);
        assert_eq!(hello.find_from(&s(""), 3), 3);
        assert_eq!(hello.find_character('l' as u16, 0), 2);
        assert_eq!(hello.find_character('l' as u16, 4), 9);
        assert_eq!(hello.find_character(0x20AC, 0), NOT_FOUND);
        assert_eq!(hello.find_matching(|c| c == 'w' as u16, 0), 6);
        assert!(hello.contains(&s("lo wo")));
        assert!(hello.contains_character('d' as u16));
        assert!(hello.contains_matching(|c| c == 'h' as u16));
        assert_eq!(String::default().find(&s("a")), NOT_FOUND);
        // Haystack de 16 bits com agulha de 8 bits e o contrário.
        let wide = String::from_utf16(&[0x20AC, 0x61, 0x62, 0x63]);
        assert_eq!(wide.find(&s("bc")), 2);
        assert_eq!(hello.find(&String::from_utf16(&[0x6F, 0x20])), 4);
    }

    #[test]
    fn find_ignoring_ascii_case_family() {
        let hello = s("Hello World");
        assert_eq!(hello.find_ignoring_ascii_case(&s("WORLD")), 6);
        assert_eq!(hello.find_ignoring_ascii_case(&s("")), 0);
        assert_eq!(hello.find_ignoring_ascii_case_from(&s(""), 100), 11);
        assert_eq!(hello.find_ignoring_ascii_case_from(&s("o"), 5), 7);
        assert_eq!(hello.find_ignoring_ascii_case(&s("xyz")), NOT_FOUND);
        assert!(hello.contains_ignoring_ascii_case(&s("hELLO")));
        assert!(equal_ignoring_ascii_case_string(&s("AbC"), &s("aBc")));
        assert!(!equal_ignoring_ascii_case_string(&s("AbC"), &s("aBcd")));
        assert!(equal_ignoring_ascii_case_string(&String::default(), &String::default()));
        assert!(!equal_ignoring_ascii_case_string(&String::default(), &empty_string()));
    }

    #[test]
    fn reverse_find_family() {
        let hello = s("hello world");
        assert_eq!(hello.reverse_find(&s("o"), MAX_LENGTH), 7);
        assert_eq!(hello.reverse_find(&s("o"), 6), 4);
        assert_eq!(hello.reverse_find(&s("lo"), MAX_LENGTH), 3);
        assert_eq!(hello.reverse_find(&s("xyz"), MAX_LENGTH), NOT_FOUND);
        assert_eq!(hello.reverse_find(&s(""), 3), 3);
        assert_eq!(hello.reverse_find(&s(""), MAX_LENGTH), 11);
        assert_eq!(hello.reverse_find_character('l' as u16, MAX_LENGTH), 9);
        assert_eq!(hello.reverse_find_character(0x20AC, MAX_LENGTH), NOT_FOUND);
    }

    #[test]
    fn starts_and_ends_with() {
        let hello = s("hello world");
        assert!(hello.starts_with(&s("hello")));
        assert!(!hello.starts_with(&s("world")));
        assert!(hello.starts_with(&String::default()));
        assert!(hello.ends_with(&s("world")));
        assert!(!hello.ends_with(&String::default()));
        assert!(hello.starts_with_ignoring_ascii_case(&s("HELLO")));
        assert!(hello.ends_with_ignoring_ascii_case(&s("WORLD")));
        assert!(hello.starts_with_character('h' as u16));
        assert!(hello.ends_with_character('d' as u16));
        assert!(hello.has_infix_starting_at(&s("wor"), 6));
        assert!(!hello.has_infix_starting_at(&s("wor"), 7));
        assert!(hello.has_infix_ending_at(&s("hello"), 5));
        assert!(!hello.has_infix_ending_at(&s("hello"), 4));
        assert!(String::default().starts_with(&empty_string()));
        assert!(!String::default().starts_with(&s("a")));
        assert!(starts_with_letters_ignoring_ascii_case(&s("Content-Type"), b"content"));
        assert!(equal_letters_ignoring_ascii_case(&s("TEXT"), b"text"));
        assert!(!equal_letters_ignoring_ascii_case(&s("TEXTS"), b"text"));
    }

    #[test]
    fn substring_left_right() {
        let hello = s("hello world");
        assert_eq!(text(&hello.substring(6, MAX_LENGTH)), "world");
        assert_eq!(text(&hello.substring(0, 5)), "hello");
        assert_eq!(text(&hello.left(3)), "hel");
        assert_eq!(text(&hello.right(5)), "world");
        assert!(hello.substring(50, 3).is_empty());
        assert!(!hello.substring(50, 3).is_null());
        assert!(String::default().substring(0, 3).is_null());
        assert_eq!(text(&hello.substring_sharing_impl(6, 100)), "world");
        assert_eq!(hello.substring(0, MAX_LENGTH), hello);
    }

    #[test]
    fn utf8_conversion_modes() {
        let lone_surrogate = String::from_utf16(&[0xD800]);
        assert_eq!(lone_surrogate.utf8(ConversionMode::LenientConversion), vec![0xEF, 0xBF, 0xBD]);
        assert_eq!(
            lone_surrogate.utf8(ConversionMode::StrictConversionReplacingUnpairedSurrogatesWithFFFD),
            vec![0xEF, 0xBF, 0xBD]
        );
        assert_eq!(
            lone_surrogate.try_get_utf8(ConversionMode::StrictConversion),
            Err(UTF8ConversionError::Invalid)
        );
        assert_eq!(String::from_latin1(&[0xE9]).utf8(ConversionMode::StrictConversion), vec![0xC3, 0xA9]);
        assert_eq!(
            String::from_code_point(0x1F600).utf8(ConversionMode::StrictConversion),
            vec![0xF0, 0x9F, 0x98, 0x80]
        );
        assert_eq!(s("abc").utf8(ConversionMode::LenientConversion), b"abc".to_vec());
        assert_eq!(String::default().utf8(ConversionMode::LenientConversion), Vec::<u8>::new());
        assert_eq!(empty_string().utf8(ConversionMode::StrictConversion), Vec::<u8>::new());
    }

    #[test]
    fn from_utf8_variants() {
        assert!(String::from_utf8(&[0xFF, 0x41]).is_null());
        assert_eq!(String::from_utf8("é".as_bytes()).length(), 1);
        assert!(String::from_utf8(b"").is_empty() && !String::from_utf8(b"").is_null());
        let replaced = String::from_utf8_replacing_invalid_sequences(&[0x41, 0xFF, 0x42]);
        assert_eq!(replaced, String::from_utf16(&[0x41, 0xFFFD, 0x42]));
        assert_eq!(String::from_utf8_replacing_invalid_sequences(b"abc"), s("abc"));
        assert!(String::from_utf8_replacing_invalid_sequences(b"").is_empty());
        let fallback = String::from_utf8_with_latin1_fallback(&[0xFF, 0x41]);
        assert_eq!(fallback, String::from_latin1(&[0xFF, 0x41]));
        assert_eq!(String::from_utf8_with_latin1_fallback("é".as_bytes()), String::from_utf16(&[0xE9]));
    }

    #[test]
    fn from_code_point_cases() {
        assert_eq!(String::from_code_point(0x41), String::from_utf16(&[0x41]));
        assert_eq!(String::from_code_point(0x1F600), String::from_utf16(&[0xD83D, 0xDE00]));
        assert!(String::from_code_point(0x110000).is_null());
        assert_eq!(String::from_code_point(0xD800), String::from_utf16(&[0xD800]));
    }

    #[test]
    fn ascii_and_latin1() {
        let string = String::from_utf16(&[0x41, 0x20AC, 0x0A, 0x00, 0x7F, 0x80]);
        assert_eq!(string.ascii(), vec![b'A', b'?', b'?', 0, 0x7F, b'?']);
        assert_eq!(string.latin1(), vec![b'A', b'?', 0x0A, 0, 0x7F, 0x80]);
        assert_eq!(String::from_latin1(&[0x41, 0xE9]).ascii(), vec![b'A', b'?']);
        assert_eq!(String::from_latin1(&[0x41, 0xE9]).latin1(), vec![0x41, 0xE9]);
    }

    #[test]
    fn characters_with_null_termination() {
        let string = s("ab");
        assert_eq!(string.characters_without_null_termination(), Ok(vec![97, 98]));
        assert_eq!(string.characters_with_null_termination(), Ok(vec![97, 98, 0]));
        assert_eq!(String::default().characters_without_null_termination(), Ok(vec![]));
    }

    #[test]
    fn integer_numbers() {
        assert_eq!(text(&String::number_i32(-123)), "-123");
        assert_eq!(text(&String::number_i32(0)), "0");
        assert_eq!(text(&String::number_i32(i32::MIN)), "-2147483648");
        assert_eq!(text(&String::number_u32(u32::MAX)), "4294967295");
        assert_eq!(text(&String::number_i64(i64::MIN)), "-9223372036854775808");
        assert_eq!(text(&String::number_u64(u64::MAX)), "18446744073709551615");
    }

    #[test]
    fn double_numbers() {
        assert_eq!(text(&String::number_f64(1.5)), "1.5");
        assert_eq!(text(&String::number_f64(100.0)), "100");
    }

    #[test]
    fn to_double_parsing() {
        assert_eq!(s("12.5").to_double(), (12.5, true));
        assert_eq!(s("  12.5").to_double(), (12.5, true));
        assert_eq!(s("12.5x").to_double(), (12.5, false));
        assert_eq!(s("1e3").to_double(), (1000.0, true));
        assert_eq!(s("1e").to_double(), (1.0, false));
        assert_eq!(s(".5").to_double(), (0.5, true));
        assert_eq!(s("5.").to_double(), (5.0, true));
        assert_eq!(s("+3").to_double(), (3.0, true));
        assert_eq!(s("-7.25e-1").to_double(), (-0.725, true));
        assert_eq!(s("abc").to_double(), (0.0, false));
        assert_eq!(s("").to_double(), (0.0, false));
        assert_eq!(s(".").to_double(), (0.0, false));
        assert_eq!(String::default().to_double(), (0.0, false));
        let (negative_zero, ok) = s("-0").to_double();
        assert!(ok && negative_zero == 0.0 && negative_zero.is_sign_negative());
        let (infinity, ok) = s("1e999").to_double();
        assert!(ok && infinity.is_infinite());
        assert_eq!(s("inf").to_double(), (0.0, false));
        assert_eq!(String::from_utf16(&[0x31, 0x2E, 0x35]).to_double(), (1.5, true));
        assert_eq!(characters_to_fixed_double(b"1.5"), (1.5, true));
        assert_eq!(characters_to_fixed_double(b" 1.5"), (0.0, false));
        assert_eq!(characters_to_fixed_double(b"1e3"), (1.0, false));
        assert_eq!(characters_to_fixed_double(b"+1.5"), (0.0, false));
    }

    #[test]
    fn to_float_parsing() {
        assert_eq!(s("1.5").to_float(), (1.5f32, true));
        assert_eq!(s("1e300").to_float().1, false);
        assert_eq!(s("x").to_float(), (0.0f32, false));
        assert_eq!(characters_to_float_parsed_length(b"  2.5abc"), (2.5f32, 5));
    }

    #[test]
    fn split_family() {
        let comma = ',' as u16;
        let items = |strings: Vec<String>| strings.iter().map(text).collect::<Vec<_>>();
        assert_eq!(items(s("a,b,,c").split(comma)), vec!["a", "b", "c"]);
        assert_eq!(items(s("a,b,,c").split_allowing_empty_entries(comma)), vec!["a", "b", "", "c"]);
        assert_eq!(items(s(",a,").split_allowing_empty_entries(comma)), vec!["", "a", ""]);
        assert_eq!(items(s("a--b-c").split_string(&s("--"))), vec!["a", "b-c"]);
        assert_eq!(items(s("a--b----c").split_allowing_empty_entries_string(&s("--"))), vec!["a", "b", "", "c"]);
        let mut seen = Vec::new();
        s("x;y").split_with(';' as u16, |piece| seen.push(text(piece)));
        assert_eq!(seen, vec!["x", "y"]);
        assert_eq!(s("").split(comma).len(), 0);
        assert_eq!(s("").split_allowing_empty_entries(comma).len(), 1);
    }

    #[test]
    fn trim_and_simplify() {
        let is_space = |c: u16| c == 0x20 || c == 0x09 || c == 0x0A;
        assert_eq!(text(&s("  hi  ").trim(is_space)), "hi");
        assert!(s("   ").trim(is_space).is_empty());
        assert_eq!(text(&s("hi").trim(is_space)), "hi");
        assert_eq!(text(&s(" a \t b\n\nc ").simplify_white_space(is_space)), "a b c");
        assert_eq!(text(&s("a  b").simplify_white_space(is_space)), "a b");
        assert!(String::default().trim(is_space).is_null());
        let wide = String::from_utf16(&[0x20, 0x20AC, 0x20]);
        assert_eq!(wide.trim(is_space), String::from_utf16(&[0x20AC]));
    }

    #[test]
    fn case_conversions() {
        assert_eq!(text(&s("HeLLo").convert_to_ascii_lowercase()), "hello");
        assert_eq!(text(&s("HeLLo").convert_to_ascii_uppercase()), "HELLO");
        assert_eq!(text(&s("HeLLo").convert_to_lowercase_without_locale()), "hello");
        assert_eq!(text(&s("HeLLo").convert_to_uppercase_without_locale()), "HELLO");
        assert_eq!(text(&s("HeLLo").fold_case()), "hello");
        assert!(String::default().convert_to_ascii_lowercase().is_null());
    }

    #[test]
    fn replace_remove_join() {
        assert_eq!(text(&make_string_by_replacing_all(&s("a-b-c"), '-' as u16, '+' as u16)), "a+b+c");
        assert_eq!(make_string_by_replacing_all(&s("abc"), 'x' as u16, 'y' as u16), s("abc"));
        assert_eq!(text(&make_string_by_removing(&s("hello"), 1, 3)), "ho");
        assert_eq!(text(&make_string_by_removing(&s("hello"), 3, 100)), "hel");
        assert_eq!(make_string_by_removing(&s("hello"), 9, 1), s("hello"));
        assert_eq!(text(&make_string_by_joining(&[s("a"), s("b"), s("c")], &s(", "))), "a, b, c");
        assert!(make_string_by_joining(&[], &s(",")).is_empty());
        // O StringBuilder só põe o separador depois de o acumulado deixar de ser vazio.
        assert_eq!(text(&make_string_by_joining(&[s(""), s("b")], &s(","))), "b");
    }

    #[test]
    fn width_conversions() {
        let mut string = s("abc");
        assert!(string.is_8bit());
        string.convert_to_16bit();
        assert!(!string.is_8bit());
        assert_eq!(string, s("abc"));
        assert!(String::make_8bit(&[0x61, 0xE9]).is_8bit());
        let mut null = String::default();
        null.convert_to_16bit();
        assert!(null.is_null());
    }

    #[test]
    fn ownership_helpers() {
        let mut string = s("abc");
        let shared = string.clone();
        string.clear_impl_if_not_shared();
        assert!(!string.is_null());
        drop(shared);
        string.clear_impl_if_not_shared();
        assert!(string.is_null());
        assert!(s("abc").is_safe_to_send_to_another_thread());
        assert!(!empty_string().clone().is_null());
        assert_eq!(s("abc").isolated_copy(), s("abc"));
        assert_eq!(s("abc").into_isolated_copy(), s("abc"));
    }

    #[test]
    fn equality_ignoring_nullity() {
        assert!(equal_ignoring_nullity(&String::default(), &empty_string()));
        assert!(equal_ignoring_nullity(&empty_string(), &String::default()));
        assert!(!equal_ignoring_nullity(&String::default(), &s("a")));
        assert!(equal_ignoring_nullity_utf16(&[0x61], &s("a")));
        assert!(equal_ignoring_nullity_utf16(&[], &String::default()));
        assert!(!equal_ignoring_nullity_utf16(&[0x62], &s("a")));
    }
}
