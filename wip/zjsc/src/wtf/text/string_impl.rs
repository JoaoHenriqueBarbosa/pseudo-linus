//! Tradução de `WTF/wtf/text/StringImpl.h`: a declaração da classe, os enums, as constantes de
//! flags e as funções inline do cabeçalho. As funções do `StringImpl.cpp` (substring, conversões
//! de caixa, replace, UTF-8 etc.) ficam para a próxima fatia.
//!
//! Modelo (CONVENTIONS, item 1): o buffer é sempre um `Box<[T]>` dono. O buffer interno, o
//! substring e o static do C++ viram cópia; a contagem de referência some porque o `String` é um
//! `Rc<StringImpl>`. O que a contagem de referência do C++ expressava como "estático" vira o campo
//! `is_static`. As flags (`hash_and_flags`) mantêm o mesmo layout de bits do C++.

use std::cell::Cell;
use std::cmp::Ordering;
use std::rc::Rc;

use crate::wtf::text::string_hasher;

/// `Latin1Character`.
pub type LChar = u8;
/// `char16_t`.
pub type UChar = u16;

/// Posição "não encontrado" de `StringCommon.h` (`notFound`).
const NOT_FOUND: usize = usize::MAX;

/// Valor de `sizeof(StringImpl)` do C++ em x86_64 (refCount 4, length 4, ponteiro 8, hashAndFlags 4,
/// com alinhamento 8). Entra só no cálculo de `is_valid_length` e dos limites de substring.
const CPP_SIZE_OF_STRING_IMPL: usize = 24;

/// `StringImpl::tailOffset<T>()` do C++ (offsetof(m_hashAndFlags) + 4 = 20, arredondado ao
/// alinhamento de T), usado em `allocationSize`.
const CPP_TAIL_OFFSET_8: usize = 20;
const CPP_TAIL_OFFSET_POINTER: usize = 24;

// ---------------------------------------------------------------------------------------------
// CharType
// ---------------------------------------------------------------------------------------------

/// O parâmetro de template `CharacterType` do C++ (`LChar` ou `UChar`).
pub trait CharType: Copy + Into<u32> + Eq + Ord + 'static {
    /// `sizeof(CharacterType)`.
    const SIZE: usize;

    /// O caractere como unidade de código de 16 bits.
    fn to_u16(self) -> u16;

    /// Constrói a partir de uma unidade de 16 bits (trunca no tipo de 8 bits, como a conversão
    /// implícita do C++ onde o chamador já garantiu Latin1).
    fn from_u16(character: u16) -> Self;

    /// `StringImpl::span<CharacterType>()`.
    fn span_of(string: &StringImpl) -> &[Self];

    /// `StringImpl::create(std::span<const CharacterType>)`.
    fn create(characters: &[Self]) -> Rc<StringImpl>;
}

impl CharType for u8 {
    const SIZE: usize = 1;

    fn to_u16(self) -> u16 {
        self as u16
    }

    fn from_u16(character: u16) -> Self {
        character as u8
    }

    fn span_of(string: &StringImpl) -> &[u8] {
        string.span8()
    }

    fn create(characters: &[u8]) -> Rc<StringImpl> {
        StringImpl::create(characters)
    }
}

impl CharType for u16 {
    const SIZE: usize = 2;

    fn to_u16(self) -> u16 {
        self
    }

    fn from_u16(character: u16) -> Self {
        character
    }

    fn span_of(string: &StringImpl) -> &[u16] {
        string.span16()
    }

    fn create(characters: &[u16]) -> Rc<StringImpl> {
        StringImpl::create16(characters)
    }
}

// ---------------------------------------------------------------------------------------------
// Enums e constantes de flags
// ---------------------------------------------------------------------------------------------

/// `StringImpl::BufferOwnership`. No porte o buffer é sempre dono; o valor só vive nos bits de
/// flag, como no C++.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum BufferOwnership {
    BufferInternal = 0,
    BufferOwned = 1,
    BufferSubstring = 2,
    BufferExternal = 3,
}

/// `StringImpl::StringKind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum StringKind {
    /// Nem símbolo nem atômica.
    StringNormal = 0,
    /// Atômica.
    StringAtom = S_HASH_FLAG_STRING_KIND_IS_ATOM,
    /// Símbolo, não atômica.
    StringSymbol = S_HASH_FLAG_STRING_KIND_IS_SYMBOL,
}

/// `StringImplShape::MaxLength`.
pub const MAX_LENGTH: u32 = i32::MAX as u32;

/// Os 6 bits baixos do hash são flags, mas se reservam 8 porque o `StringHash` só tem 24 bits.
pub const S_FLAG_COUNT: u32 = 8;

const S_FLAG_MASK: u32 = (1u32 << S_FLAG_COUNT) - 1;
const S_FLAG_STRING_KIND_COUNT: u32 = 4;
/// `s_hashFlagNeverAtomize` (`USE(BUN_JSC_ADDITIONS)` vale no `cmakeconfig.h`).
const S_HASH_FLAG_NEVER_ATOMIZE: u32 = 1u32 << 6;

const S_HASH_ZERO_VALUE: u32 = 0;
const S_HASH_FLAG_STRING_KIND_IS_ATOM: u32 = 1u32 << S_FLAG_STRING_KIND_COUNT;
const S_HASH_FLAG_STRING_KIND_IS_SYMBOL: u32 = 1u32 << (S_FLAG_STRING_KIND_COUNT + 1);
const S_HASH_MASK_STRING_KIND: u32 = S_HASH_FLAG_STRING_KIND_IS_ATOM | S_HASH_FLAG_STRING_KIND_IS_SYMBOL;
const S_HASH_FLAG_DID_REPORT_COST: u32 = 1u32 << 3;
const S_HASH_FLAG_8BIT_BUFFER: u32 = 1u32 << 2;
const S_HASH_MASK_BUFFER_OWNERSHIP: u32 = (1u32 << 0) | (1u32 << 1);

// ---------------------------------------------------------------------------------------------
// StringImpl
// ---------------------------------------------------------------------------------------------

/// O buffer de caracteres: Latin1 ou UTF-16.
#[derive(Clone, Debug)]
pub enum StringData {
    Latin1(Box<[u8]>),
    Utf16(Box<[u16]>),
}

/// `class StringImpl`. Imutável salvo pelos bits de `hash_and_flags` (o `mutable` do C++).
#[derive(Debug)]
pub struct StringImpl {
    data: StringData,
    /// Hash nos 24 bits altos, flags nos 8 baixos.
    hash_and_flags: Cell<u32>,
    /// O C++ marca a string estática no bit baixo da contagem de referência; sem contagem, é um
    /// campo. Nunca muda depois da construção.
    is_static: bool,
}

thread_local! {
    /// `StringImpl::s_emptyAtomString`: a string vazia, atômica e estática.
    static EMPTY_ATOM_STRING: Rc<StringImpl> = Rc::new(StringImpl::new_static_empty());
}

impl StringImpl {
    pub const MAX_LENGTH: u32 = MAX_LENGTH;
    pub const S_FLAG_COUNT: u32 = S_FLAG_COUNT;

    // ---- construtores internos -----------------------------------------------------------

    fn construct(data: StringData, hash_and_flags: u32, is_static: bool) -> StringImpl {
        StringImpl {
            data,
            hash_and_flags: Cell::new(hash_and_flags),
            is_static,
        }
    }

    /// Normal de 8 bits com buffer interno (`StringImpl(length, Force8Bit)`).
    fn new8(data: Box<[u8]>) -> StringImpl {
        Self::construct(
            StringData::Latin1(data),
            S_HASH_FLAG_8BIT_BUFFER | StringKind::StringNormal as u32 | BufferOwnership::BufferInternal as u32,
            false,
        )
    }

    /// Normal de 16 bits com buffer interno (`StringImpl(length)`).
    fn new16(data: Box<[u16]>) -> StringImpl {
        Self::construct(
            StringData::Utf16(data),
            S_HASH_ZERO_VALUE | StringKind::StringNormal as u32 | BufferOwnership::BufferInternal as u32,
            false,
        )
    }

    /// `StaticStringImpl("", StringAtom)`: hash calculado na construção e custo já reportado.
    fn new_static_empty() -> StringImpl {
        let hash = string_hasher::compute_literal_hash_and_mask_top8_bits::<u8>(&[]);
        Self::construct(
            StringData::Latin1(Box::new([])),
            S_HASH_FLAG_8BIT_BUFFER
                | S_HASH_FLAG_DID_REPORT_COST
                | StringKind::StringAtom as u32
                | BufferOwnership::BufferInternal as u32
                | (hash << S_FLAG_COUNT),
            true,
        )
    }

    /// `StringImpl(CreateSymbol, std::span<const Latin1Character>)`: símbolo cujo buffer o C++
    /// compartilha com a descrição (BufferSubstring); aqui é cópia.
    pub(crate) fn new_symbol8(characters: &[u8]) -> StringImpl {
        Self::construct(
            StringData::Latin1(characters.into()),
            S_HASH_FLAG_8BIT_BUFFER | StringKind::StringSymbol as u32 | BufferOwnership::BufferSubstring as u32,
            false,
        )
    }

    /// `StringImpl(CreateSymbol, std::span<const char16_t>)`.
    pub(crate) fn new_symbol16(characters: &[u16]) -> StringImpl {
        Self::construct(
            StringData::Utf16(characters.into()),
            S_HASH_ZERO_VALUE | StringKind::StringSymbol as u32 | BufferOwnership::BufferSubstring as u32,
            false,
        )
    }

    /// `StringImpl(CreateSymbol)`: símbolo nulo (buffer vazio de 8 bits).
    pub(crate) fn new_null_symbol() -> StringImpl {
        Self::new_symbol8(&[])
    }

    // ---- create / empty ------------------------------------------------------------------

    /// `StringImpl::empty()`.
    pub fn empty() -> Rc<StringImpl> {
        EMPTY_ATOM_STRING.with(Rc::clone)
    }

    /// `StringImpl::create(std::span<const Latin1Character>)`.
    pub fn create(characters: &[u8]) -> Rc<StringImpl> {
        if characters.is_empty() {
            return Self::empty();
        }
        Rc::new(Self::new8(characters.into()))
    }

    /// `StringImpl::create(std::span<const char16_t>)`: sem estreitar, como o C++.
    pub fn create16(characters: &[u16]) -> Rc<StringImpl> {
        if characters.is_empty() {
            return Self::empty();
        }
        Rc::new(Self::new16(characters.into()))
    }

    /// `StringImpl::create8BitUnconditionally(std::span<const char16_t>)`: o chamador garantiu
    /// Latin1; trunca cada unidade.
    pub fn create8_bit_unconditionally(characters: &[u16]) -> Rc<StringImpl> {
        if characters.is_empty() {
            return Self::empty();
        }
        let narrowed: Vec<u8> = characters.iter().map(|c| *c as u8).collect();
        Rc::new(Self::new8(narrowed.into_boxed_slice()))
    }

    /// `StringImpl::create8BitIfPossible(std::span<const char16_t>)`.
    pub fn create8_bit_if_possible(characters: &[u16]) -> Rc<StringImpl> {
        if characters.is_empty() {
            return Self::empty();
        }
        if characters.iter().all(|c| *c <= 0xFF) {
            return Self::create8_bit_unconditionally(characters);
        }
        Self::create16(characters)
    }

    /// `StringImpl::create(std::span<const char8_t>)`: UTF-8 para UTF-16, `None` se houver
    /// sequência inválida. Entrada vazia dá a string vazia; ASCII puro dá 8 bits.
    pub fn create_from_utf8(code_units: &[u8]) -> Option<Rc<StringImpl>> {
        if code_units.is_empty() {
            return Some(Self::empty());
        }
        if code_units.is_ascii() {
            return Some(Self::create(code_units));
        }
        let text = std::str::from_utf8(code_units).ok()?;
        let units: Vec<u16> = text.encode_utf16().collect();
        Some(Self::create16(&units))
    }

    /// `StringImpl::createSubstringSharingImpl`. O C++ copia quando a cópia cabe no espaço que o
    /// substring ocuparia (`allocationSize<StringImpl*>(1)`) e, para 16 bits, estreita para 8 bits
    /// quando tudo é Latin1; o resto compartilha o dono, o que aqui também é cópia. Os limites de
    /// cópia ficam para manter `is_8bit` igual ao do C++.
    pub fn create_substring_sharing_impl(rep: &StringImpl, offset: u32, length: u32) -> Rc<StringImpl> {
        let (offset, length) = (offset as usize, length as usize);
        if length == 0 {
            return Self::empty();
        }
        let substring_size = CPP_TAIL_OFFSET_POINTER + std::mem::size_of::<usize>();
        match &rep.data {
            StringData::Latin1(data) => Self::create(&data[offset..offset + length]),
            StringData::Utf16(data) => {
                let span = &data[offset..offset + length];
                if substring_size >= CPP_TAIL_OFFSET_8 + length && span.iter().all(|c| *c <= 0xFF) {
                    return Self::create8_bit_unconditionally(span);
                }
                Self::create16(span)
            }
        }
    }

    /// `StringImpl::createByReplacingInCharacters(span<Latin1>, ...)`.
    pub fn create_by_replacing_in_characters8(
        characters: &[u8],
        target: u16,
        replacement: u16,
        index_of_first_target_character: usize,
    ) -> Rc<StringImpl> {
        if replacement <= 0xFF {
            let old_char = target as u8;
            let new_char = replacement as u8;
            let mut data: Vec<u8> = characters[..index_of_first_target_character].to_vec();
            for &character in &characters[index_of_first_target_character..] {
                data.push(if character == old_char { new_char } else { character });
            }
            return Self::create(&data);
        }

        let data: Vec<u16> = characters
            .iter()
            .map(|&character| if character as u16 == target { replacement } else { character as u16 })
            .collect();
        Self::create16(&data)
    }

    /// `StringImpl::createByReplacingInCharacters(span<char16_t>, ...)`.
    pub fn create_by_replacing_in_characters16(
        characters: &[u16],
        target: u16,
        replacement: u16,
        index_of_first_target_character: usize,
    ) -> Rc<StringImpl> {
        let mut data: Vec<u16> = characters[..index_of_first_target_character].to_vec();
        for &character in &characters[index_of_first_target_character..] {
            data.push(if character == target { replacement } else { character });
        }
        Self::create16(&data)
    }

    /// `StringImpl::adopt(Vector&&)`.
    pub fn adopt<T: CharType>(vector: Vec<T>) -> Rc<StringImpl> {
        T::create(&vector)
    }

    // ---- flags estáticas -----------------------------------------------------------------

    pub const fn flag_is_8bit() -> u32 {
        S_HASH_FLAG_8BIT_BUFFER
    }

    pub const fn flag_is_atom() -> u32 {
        S_HASH_FLAG_STRING_KIND_IS_ATOM
    }

    pub const fn flag_is_symbol() -> u32 {
        S_HASH_FLAG_STRING_KIND_IS_SYMBOL
    }

    pub const fn mask_string_kind() -> u32 {
        S_HASH_MASK_STRING_KIND
    }

    /// `StringImpl::isValidLength<CharacterType>`.
    pub fn is_valid_length<T: CharType>(length: usize) -> bool {
        let max = std::cmp::min(
            MAX_LENGTH as usize,
            (u32::MAX as usize - CPP_SIZE_OF_STRING_IMPL) / T::SIZE,
        );
        length <= max
    }

    // ---- acesso ao conteúdo --------------------------------------------------------------

    pub fn length(&self) -> u32 {
        match &self.data {
            StringData::Latin1(data) => data.len() as u32,
            StringData::Utf16(data) => data.len() as u32,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.length() == 0
    }

    pub fn is_8bit(&self) -> bool {
        self.hash_and_flags.get() & S_HASH_FLAG_8BIT_BUFFER != 0
    }

    /// `span8()`: o C++ exige `is8Bit()`; numa string de 16 bits devolve fatia vazia.
    pub fn span8(&self) -> &[u8] {
        match &self.data {
            StringData::Latin1(data) => data,
            StringData::Utf16(_) => &[],
        }
    }

    /// `span16()`: o C++ exige `!is8Bit() || isEmpty()`; numa string de 8 bits devolve fatia vazia.
    pub fn span16(&self) -> &[u16] {
        match &self.data {
            StringData::Utf16(data) => data,
            StringData::Latin1(_) => &[],
        }
    }

    /// `span<CharacterType>()`.
    pub fn span<T: CharType>(&self) -> &[T] {
        T::span_of(self)
    }

    /// `StringImpl::at(i)` (`operator[]`).
    pub fn char_at(&self, i: u32) -> u16 {
        match &self.data {
            StringData::Latin1(data) => data[i as usize] as u16,
            StringData::Utf16(data) => data[i as usize],
        }
    }

    // ---- custo ---------------------------------------------------------------------------

    /// `StringImpl::cost()`. O substring do C++ devolve o custo da base; aqui o símbolo tem cópia
    /// própria e conta o seu.
    pub fn cost(&self) -> usize {
        let flags = self.hash_and_flags.get();
        if flags & S_HASH_FLAG_DID_REPORT_COST != 0 {
            return 0;
        }

        self.hash_and_flags.set(flags | S_HASH_FLAG_DID_REPORT_COST);
        let mut result = self.length() as usize;
        if !self.is_8bit() {
            result <<= 1;
        }
        result
    }

    /// `StringImpl::costDuringGC()`. O C++ divide pela contagem de referência; ela vive no `Rc`,
    /// então o chamador passa `Rc::strong_count`.
    pub fn cost_during_gc(&self, ref_count: usize) -> usize {
        if self.is_static() {
            return 0;
        }

        let mut result = self.length() as usize;
        if !self.is_8bit() {
            result <<= 1;
        }
        result.div_ceil(ref_count)
    }

    // ---- espécie da string ---------------------------------------------------------------

    pub fn is_symbol(&self) -> bool {
        self.hash_and_flags.get() & S_HASH_FLAG_STRING_KIND_IS_SYMBOL != 0
    }

    pub fn is_atom(&self) -> bool {
        self.hash_and_flags.get() & S_HASH_FLAG_STRING_KIND_IS_ATOM != 0
    }

    pub fn set_is_atom(&self, is_atom: bool) {
        let flags = self.hash_and_flags.get();
        if is_atom {
            self.hash_and_flags.set(flags | S_HASH_FLAG_STRING_KIND_IS_ATOM);
        } else {
            self.hash_and_flags.set(flags & !S_HASH_FLAG_STRING_KIND_IS_ATOM);
        }
    }

    pub fn is_external(&self) -> bool {
        self.buffer_ownership() == BufferOwnership::BufferExternal
    }

    pub fn is_sub_string(&self) -> bool {
        self.buffer_ownership() == BufferOwnership::BufferSubstring
    }

    pub fn is_static(&self) -> bool {
        self.is_static
    }

    pub fn buffer_ownership(&self) -> BufferOwnership {
        match self.hash_and_flags.get() & S_HASH_MASK_BUFFER_OWNERSHIP {
            0 => BufferOwnership::BufferInternal,
            1 => BufferOwnership::BufferOwned,
            2 => BufferOwnership::BufferSubstring,
            _ => BufferOwnership::BufferExternal,
        }
    }

    /// `canBecomeAtom()` (adição do Bun).
    pub fn can_become_atom(&self) -> bool {
        self.hash_and_flags.get() & S_HASH_FLAG_NEVER_ATOMIZE == 0
    }

    /// `setNeverAtomize()` (adição do Bun).
    pub fn set_never_atomize(&self) {
        self.hash_and_flags.set(self.hash_and_flags.get() | S_HASH_FLAG_NEVER_ATOMIZE);
    }

    // ---- hash ----------------------------------------------------------------------------

    /// Os bits altos de `hash` são sempre vazios; as flags ficam nos baixos, que é mais barato de
    /// acessar. Por isso se desloca ao guardar e ao ler.
    fn set_hash(&self, hash: u32) {
        self.hash_and_flags.set(self.hash_and_flags.get() | (hash << S_FLAG_COUNT));
    }

    fn raw_hash(&self) -> u32 {
        self.hash_and_flags.get() >> S_FLAG_COUNT
    }

    pub fn has_hash(&self) -> bool {
        self.raw_hash() != 0
    }

    pub fn existing_hash(&self) -> u32 {
        self.raw_hash()
    }

    pub fn hash(&self) -> u32 {
        if self.has_hash() {
            self.raw_hash()
        } else {
            self.hash_slow_case()
        }
    }

    fn hash_slow_case(&self) -> u32 {
        self.set_hash(self.concurrent_hash());
        self.existing_hash()
    }

    /// `StringImpl::concurrentHash()`: calcula sem gravar.
    pub fn concurrent_hash(&self) -> u32 {
        match &self.data {
            StringData::Latin1(data) => string_hasher::compute_hash_and_mask_top8_bits::<u8>(data),
            StringData::Utf16(data) => string_hasher::compute_hash_and_mask_top8_bits::<u16>(data),
        }
    }

    // ---- consultas -----------------------------------------------------------------------

    pub fn contains_only_ascii(&self) -> bool {
        match &self.data {
            StringData::Latin1(data) => data.is_ascii(),
            StringData::Utf16(data) => data.iter().all(|c| *c <= 0x7F),
        }
    }

    pub fn contains_only_latin1(&self) -> bool {
        match &self.data {
            StringData::Latin1(_) => true,
            StringData::Utf16(data) => data.iter().all(|c| *c <= 0xFF),
        }
    }

    /// `StringImpl::containsOnly<isSpecialCharacter>()`.
    pub fn contains_only(&self, is_special_character: fn(u16) -> bool) -> bool {
        match &self.data {
            StringData::Latin1(data) => contains_only(&data[..], is_special_character),
            StringData::Utf16(data) => contains_only(&data[..], is_special_character),
        }
    }

    /// `find(Latin1Character / char / char16_t, start)`.
    pub fn find_character(&self, character: u16, start: usize) -> usize {
        match &self.data {
            StringData::Latin1(data) => find(&data[..], |c| c == character, start),
            StringData::Utf16(data) => find(&data[..], |c| c == character, start),
        }
    }

    /// `find(CodeUnitMatchFunction, start)`.
    pub fn find_matching(&self, match_function: impl Fn(u16) -> bool, start: usize) -> usize {
        match &self.data {
            StringData::Latin1(data) => find(&data[..], match_function, start),
            StringData::Utf16(data) => find(&data[..], match_function, start),
        }
    }

    /// `StringImpl::removeCharacters(predicate)`: devolve a própria string se nada casa.
    pub fn remove_characters(self: &Rc<Self>, find_match: impl Fn(u16) -> bool) -> Rc<StringImpl> {
        match &self.data {
            StringData::Latin1(data) => remove_characters_impl(self, &data[..], &find_match),
            StringData::Utf16(data) => remove_characters_impl(self, &data[..], &find_match),
        }
    }

    /// `StringImpl::isolatedCopy()`: cópia independente (no C++ a cópia protege contra o uso entre
    /// threads; aqui é sempre uma string nova com o mesmo conteúdo).
    pub fn isolated_copy(&self) -> Rc<StringImpl> {
        match &self.data {
            StringData::Latin1(data) => Self::create(data),
            StringData::Utf16(data) => Self::create16(data),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// copyCharacters
// ---------------------------------------------------------------------------------------------

/// `StringImpl::copyCharacters(span<CharacterType>, span<const CharacterType>)`.
pub fn copy_characters<T: Copy>(destination: &mut [T], source: &[T]) {
    destination[..source.len()].copy_from_slice(source);
}

/// `StringImpl::copyCharacters(span<char16_t>, span<const Latin1Character>)`: alarga.
pub fn copy_characters_widen(destination: &mut [u16], source: &[u8]) {
    for (to, from) in destination.iter_mut().zip(source) {
        *to = *from as u16;
    }
}

/// `StringImpl::copyCharacters(span<Latin1Character>, span<const char16_t>)`: o chamador garante
/// que tudo é Latin1; estreita por truncamento.
pub fn copy_characters_narrow(destination: &mut [u8], source: &[u16]) {
    for (to, from) in destination.iter_mut().zip(source) {
        *to = *from as u8;
    }
}

// ---------------------------------------------------------------------------------------------
// Funções livres inline do cabeçalho
// ---------------------------------------------------------------------------------------------

/// `WTF::containsOnly<isSpecialCharacter>(span)`.
pub fn contains_only<T: CharType>(characters: &[T], is_special_character: fn(u16) -> bool) -> bool {
    characters.iter().all(|c| is_special_character(c.to_u16()))
}

/// `WTF::find(span, matchFunction, start)`.
pub fn find<T: CharType>(characters: &[T], match_function: impl Fn(u16) -> bool, start: usize) -> usize {
    let mut start = start;
    while start < characters.len() {
        if match_function(characters[start].to_u16()) {
            return start;
        }
        start += 1;
    }
    NOT_FOUND
}

/// `WTF::reverseFindLineTerminator`.
pub fn reverse_find_line_terminator<T: CharType>(characters: &[T], start: usize) -> usize {
    if characters.is_empty() {
        return NOT_FOUND;
    }
    let mut start = start;
    if start >= characters.len() {
        start = characters.len() - 1;
    }
    let mut character: u32 = characters[start].into();
    while character != '\n' as u32 && character != '\r' as u32 {
        if start == 0 {
            return NOT_FOUND;
        }
        start -= 1;
        character = characters[start].into();
    }
    start
}

/// `WTF::reverseFind(span, matchCharacter, start)`; `start` padrão no C++ é `MaxLength`.
pub fn reverse_find<T: CharType>(characters: &[T], match_character: T, start: usize) -> usize {
    if characters.is_empty() {
        return NOT_FOUND;
    }
    let mut start = start;
    if start >= characters.len() {
        start = characters.len() - 1;
    }
    let search_length = start + 1;
    match characters[..search_length].iter().rposition(|c| *c == match_character) {
        Some(index) => index,
        None => NOT_FOUND,
    }
}

/// `WTF::reverseFind(span<const char16_t>, Latin1Character, start)`.
pub fn reverse_find_16_latin1(characters: &[u16], match_character: u8, start: usize) -> usize {
    reverse_find(characters, match_character as u16, start)
}

/// `WTF::reverseFind(span<const Latin1Character>, char16_t, start)`.
pub fn reverse_find_8_char16(characters: &[u8], match_character: u16, start: usize) -> usize {
    if match_character > 0xFF {
        return NOT_FOUND;
    }
    reverse_find(characters, match_character as u8, start)
}

/// `WTF::codePointCompare`: ordem lexicográfica por unidade de código, depois por tamanho.
pub fn code_point_compare<T1: CharType, T2: CharType>(characters1: &[T1], characters2: &[T2]) -> Ordering {
    let common_length = std::cmp::min(characters1.len(), characters2.len());
    for position in 0..common_length {
        let a: u32 = characters1[position].into();
        let b: u32 = characters2[position].into();
        if a != b {
            return if a > b { Ordering::Greater } else { Ordering::Less };
        }
    }
    characters1.len().cmp(&characters2.len())
}

fn remove_characters_impl<T: CharType>(
    this: &Rc<StringImpl>,
    characters: &[T],
    find_match: &impl Fn(u16) -> bool,
) -> Rc<StringImpl> {
    // Supõe o caso comum de não remover nada.
    let first = match characters.iter().position(|c| find_match(c.to_u16())) {
        Some(index) => index,
        None => return Rc::clone(this),
    };

    let mut data: Vec<T> = characters[..first].to_vec();
    for &character in &characters[first..] {
        if !find_match(character.to_u16()) {
            data.push(character);
        }
    }

    StringImpl::adopt(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flag_constants_match_cpp() {
        assert_eq!(StringImpl::flag_is_8bit(), 4);
        assert_eq!(StringImpl::flag_is_atom(), 16);
        assert_eq!(StringImpl::flag_is_symbol(), 32);
        assert_eq!(StringImpl::mask_string_kind(), 48);
        assert_eq!(S_HASH_FLAG_NEVER_ATOMIZE, 64);
        assert_eq!(S_HASH_FLAG_DID_REPORT_COST, 8);
    }

    #[test]
    fn create_and_access() {
        let s = StringImpl::create(b"abc");
        assert!(s.is_8bit());
        assert_eq!(s.length(), 3);
        assert_eq!(s.span8(), b"abc");
        assert_eq!(s.char_at(1), b'b' as u16);
        let w = StringImpl::create16(&[0x61, 0x20AC]);
        assert!(!w.is_8bit());
        assert_eq!(w.char_at(1), 0x20AC);
        assert_eq!(w.span::<u16>().len(), 2);
    }

    #[test]
    fn empty_is_static_atom_with_hash() {
        let e = StringImpl::empty();
        assert!(e.is_atom() && e.is_static() && e.is_8bit() && e.is_empty());
        assert!(e.has_hash());
        assert_eq!(e.hash(), string_hasher::compute_hash_and_mask_top8_bits::<u8>(&[]));
        assert!(Rc::ptr_eq(&e, &StringImpl::create(b"")));
        assert_eq!(e.cost(), 0);
    }

    #[test]
    fn hash_is_lazy_and_stable() {
        let s = StringImpl::create(b"length");
        assert!(!s.has_hash());
        let h = s.hash();
        assert!(s.has_hash());
        assert_eq!(s.existing_hash(), h);
        assert_eq!(h, string_hasher::compute_hash_and_mask_top8_bits::<u8>(b"length"));
        // O hash não mexe nas flags.
        assert!(s.is_8bit() && !s.is_atom());
        // Latin1 em 16 bits tem o mesmo hash que em 8 bits.
        let w = StringImpl::create16(&[b'l' as u16, b'e' as u16, b'n' as u16, b'g' as u16, b't' as u16, b'h' as u16]);
        assert_eq!(w.hash(), h);
    }

    #[test]
    fn atom_and_cost_flags() {
        let s = StringImpl::create16(&[0x100, 0x101]);
        s.set_is_atom(true);
        assert!(s.is_atom());
        s.set_is_atom(false);
        assert!(!s.is_atom());
        assert_eq!(s.cost(), 4);
        assert_eq!(s.cost(), 0);
        assert!(s.can_become_atom());
        s.set_never_atomize();
        assert!(!s.can_become_atom());
    }

    #[test]
    fn utf8_and_substring_rules() {
        assert!(StringImpl::create_from_utf8(&[0xFF]).is_none());
        let s = StringImpl::create_from_utf8("aé".as_bytes()).unwrap();
        assert!(!s.is_8bit());
        assert_eq!(s.span16(), &[0x61, 0xE9]);
        let wide = StringImpl::create16(&[0x61, 0x62, 0x63, 0x100]);
        let sub = StringImpl::create_substring_sharing_impl(&wide, 0, 3);
        assert!(sub.is_8bit());
        let sub2 = StringImpl::create_substring_sharing_impl(&wide, 2, 2);
        assert!(!sub2.is_8bit());
    }

    #[test]
    fn find_remove_and_compare() {
        let s = StringImpl::create(b"a-b-c");
        assert_eq!(s.find_character('-' as u16, 0), 1);
        assert_eq!(s.find_character('-' as u16, 2), 3);
        assert_eq!(s.find_character('x' as u16, 0), NOT_FOUND);
        let r = s.remove_characters(|c| c == '-' as u16);
        assert_eq!(r.span8(), b"abc");
        let same = s.remove_characters(|c| c == 'z' as u16);
        assert!(Rc::ptr_eq(&s, &same));
        assert_eq!(reverse_find(b"a\nb\r".as_slice(), b'\n', usize::MAX), 1);
        assert_eq!(reverse_find_line_terminator(b"ab\ncd".as_slice(), usize::MAX), 2);
        assert_eq!(code_point_compare(b"ab".as_slice(), &[0x61u16, 0x62, 0x63]), Ordering::Less);
        assert_eq!(code_point_compare(&[0x100u16], b"z".as_slice()), Ordering::Greater);
    }

    #[test]
    fn valid_length_limits() {
        assert!(StringImpl::is_valid_length::<u8>(MAX_LENGTH as usize));
        assert!(!StringImpl::is_valid_length::<u8>(MAX_LENGTH as usize + 1));
    }

    #[test]
    fn replace_in_characters() {
        let r = StringImpl::create_by_replacing_in_characters8(b"a.b.", '.' as u16, '-' as u16, 1);
        assert_eq!(r.span8(), b"a-b-");
        let w = StringImpl::create_by_replacing_in_characters8(b"a.", '.' as u16, 0x20AC, 1);
        assert_eq!(w.span16(), &[0x61, 0x20AC]);
    }
}

/// Chave de identidade de um `UniquedStringImpl*` do C++: o `StringImpl` internado (átomo ou
/// símbolo) comparado e espalhado pelo endereço, como o C++ compara ponteiros.
#[derive(Clone, Debug)]
pub struct UniquedKey(pub std::rc::Rc<StringImpl>);

impl PartialEq for UniquedKey {
    fn eq(&self, other: &Self) -> bool {
        std::rc::Rc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for UniquedKey {}

impl std::hash::Hash for UniquedKey {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        std::ptr::hash(std::rc::Rc::as_ptr(&self.0), state);
    }
}
