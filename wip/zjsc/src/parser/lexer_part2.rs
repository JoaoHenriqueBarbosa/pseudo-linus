// Parte 2 de `parser/Lexer.cpp` (linhas 900 a 1674): `record16`, `recordUnicodeCodePoint`,
// `parseIdentifier` (as duas especializações, 8 e 16 bits), `parseIdentifierSlowCase`,
// `parseString`, `parseComplexEscape`, `parseStringSlowCase` e `parseTemplateLiteral`.
//
// Incluído por `include!` no fim de `lexer.rs`, então não há `use` nem atributo de módulo aqui.
// Onde o C++ usa `SIMD::find`, o porte varre escalarmente com o mesmo predicado: o resultado
// (primeiro índice que casa, ou o fim) é o mesmo, a vetorização não tem efeito observável.

/// `SIMD::find`: índice do primeiro elemento de `span` para o qual `scalar_match` vale, ou
/// `span.len()` (o fim) quando nenhum casa.
fn simd_find<C: Copy>(span: &[C], scalar_match: impl Fn(C) -> bool) -> usize {
    span.iter().position(|&c| scalar_match(c)).unwrap_or(span.len())
}

/// `characterRequiresParseStringSlowCase` (as duas sobrecargas: `Latin1Character` só olha o
/// controle, `char16_t` também exige o caminho lento fora do Latin1).
fn character_requires_parse_string_slow_case<C: crate::wtf::text::string_impl::CharType>(character: C) -> bool {
    let character: u32 = character.into();
    character < 0xE || (C::SIZE != 1 && character > 0xFF)
}

/// `U16_IS_SURROGATE`.
fn u16_is_surrogate(c: u32) -> bool {
    (c & 0xF800) == 0xD800
}

/// `U16_IS_SURROGATE_LEAD`: só vale para um `c` que já é surrogate.
fn u16_is_surrogate_lead(c: u32) -> bool {
    (c & 0x400) == 0
}

impl<T: crate::wtf::text::string_impl::CharType> Lexer<T> {
    /// O caractere corrente como inteiro de 32 bits (o C++ compara `m_current` com literais de
    /// caractere promovidos a `int`).
    #[inline(always)]
    fn current_unit(&self) -> u32 {
        self.current.into()
    }

    /// `record8(int)`: o chamador garante `isLatin1(c)`.
    #[inline]
    fn record8(&mut self, c: u32) {
        self.buffer8.push(c as u8);
    }

    /// `append8(std::span<const T>)`: o chamador garante que cada unidade é Latin1.
    fn append8(&mut self, span: &[T]) {
        self.buffer8.extend(span.iter().map(|&c| Into::<u32>::into(c) as u8));
    }

    /// `append16(std::span<const Latin1Character>)` e `append16(std::span<const char16_t>)`: as
    /// duas sobrecargas viram esta, genérica sobre `T` (cada unidade é alargada a 16 bits).
    fn append16(&mut self, span: &[T]) {
        self.buffer16.extend(span.iter().map(|&c| c.to_u16()));
    }

    /// `record16(T)` e `record16(int)`: o chamador garante `0 <= c <= USHRT_MAX`.
    #[inline]
    fn record16(&mut self, c: u32) {
        self.buffer16.push(c as u16);
    }

    fn record_unicode_code_point(&mut self, code_point: u32) {
        if code_point <= 0xFFFF {
            self.record16(code_point);
        } else {
            let lead = 0xD7C0 + (code_point >> 10);
            let trail = 0xDC00 | (code_point & 0x3FF);
            self.buffer16.push(lead as u16);
            self.buffer16.push(trail as u16);
        }
    }

    /// `makeIdentifier(m_buffer16.span())`. O buffer sai do `self` durante a chamada porque a
    /// criação do identificador toma `&mut self`; volta intacto (o chamador decide o `shrink`).
    fn make_identifier_from_buffer16(&mut self) -> Identifier {
        let buffer = std::mem::take(&mut self.buffer16);
        let ident = self.make_identifier(&buffer[..]);
        self.buffer16 = buffer;
        ident
    }

    /// `parseIdentifier<shouldCreateIdentifier>`: no C++ são duas especializações de template
    /// (`Lexer<Latin1Character>` e `Lexer<char16_t>`); aqui o despacho é pelo tamanho do tipo.
    fn parse_identifier<const SHOULD_CREATE_IDENTIFIER: bool>(
        &mut self,
        token_data: &mut JSTokenData,
        lexer_flags: LexerFlags,
        strict_mode: bool,
    ) -> JSTokenType {
        if T::SIZE == 1 {
            self.parse_identifier_latin1::<SHOULD_CREATE_IDENTIFIER>(token_data, lexer_flags, strict_mode)
        } else {
            self.parse_identifier_utf16::<SHOULD_CREATE_IDENTIFIER>(token_data, lexer_flags, strict_mode)
        }
    }

    /// `Lexer<Latin1Character>::parseIdentifier`.
    fn parse_identifier_latin1<const SHOULD_CREATE_IDENTIFIER: bool>(
        &mut self,
        token_data: &mut JSTokenData,
        lexer_flags: LexerFlags,
        strict_mode: bool,
    ) -> JSTokenType {
        token_data.escaped = false;
        let remaining = (self.code_end - self.code) as isize;
        if remaining >= MAX_TOKEN_LENGTH as isize && !lexer_flags.contains(LexerFlags::IGNORE_RESERVED_WORDS) {
            let keyword = self.parse_keyword::<SHOULD_CREATE_IDENTIFIER>(token_data);
            if keyword != IDENT {
                return if keyword == RESERVED_IF_STRICT && !strict_mode { IDENT } else { keyword };
            }
        }

        let is_private_name = self.current_unit() == '#' as u32;
        let is_builtin_name = self.current_unit() == '@' as u32 && self.parsing_builtin_function;
        let mut is_well_known_symbol = false;
        if is_builtin_name {
            self.shift();
            if self.current_unit() == '@' as u32 {
                is_well_known_symbol = true;
                self.shift();
            }
        }

        let identifier_start = self.current_source_ptr();

        if is_private_name {
            self.shift();
        }

        let source = self.source.clone();

        // A busca vetorial só para em fim de identificador ASCII; o laço escalar abaixo cobre
        // as partes Latin1 não ASCII. O predicado escalar do C++ é `!isIdentPart(c)`.
        let start = self.current_source_ptr();
        let found = start + simd_find(&source[start..self.code_end], |c: T| !is_ident_part(c.into()));
        self.code = found;
        self.current = if found < self.code_end { source[found] } else { T::from_u16(0) };

        // Scalar fallback for non-ASCII Latin1 identifier parts
        while is_ident_part(self.current_unit()) {
            self.shift();
        }

        if self.current_unit() == '\\' as u32 {
            return self.parse_identifier_slow_case::<SHOULD_CREATE_IDENTIFIER>(
                token_data,
                lexer_flags,
                strict_mode,
                identifier_start,
            );
        }

        let mut ident: Option<Identifier> = None;

        if SHOULD_CREATE_IDENTIFIER || self.parsing_builtin_function {
            let identifier_span = &source[identifier_start..self.current_source_ptr()];
            if self.parsing_builtin_function && is_builtin_name {
                let symbol = if is_well_known_symbol {
                    self.vm.property_names.builtin_names().look_up_well_known_symbol(identifier_span)
                } else {
                    self.vm.property_names.builtin_names().look_up_private_name(identifier_span)
                };
                ident = symbol.map(|symbol| self.arena.borrow_mut().make_symbol_identifier(&symbol));
                if ident.is_none() {
                    return INVALID_PRIVATE_NAME_ERRORTOK;
                }
            } else {
                let made = self.make_identifier(identifier_span);
                if self.parsing_builtin_function {
                    // A checagem `isSafeBuiltinIdentifier` só existe sem `USE(BUN_JSC_ADDITIONS)`,
                    // e o Bun a define: o ramo não existe neste porte.
                    if made == self.vm.property_names.undefined_keyword {
                        token_data.ident = Some(self.vm.property_names.undefined_private_name.clone());
                    }
                }
                ident = Some(made);
            }
            token_data.ident = ident.clone();
        } else {
            token_data.ident = None;
        }

        let ident_type = if is_private_name { PRIVATENAME } else { IDENT };
        if remaining < MAX_TOKEN_LENGTH as isize && !lexer_flags.contains(LexerFlags::IGNORE_RESERVED_WORDS) {
            if !is_builtin_name {
                let entry = ident.as_ref().and_then(main_table_lexer_value);
                let Some(token) = entry else {
                    return ident_type;
                };
                return if token != RESERVED_IF_STRICT || strict_mode { token } else { ident_type };
            }
        }

        ident_type
    }

    /// `Lexer<char16_t>::parseIdentifier`.
    fn parse_identifier_utf16<const SHOULD_CREATE_IDENTIFIER: bool>(
        &mut self,
        token_data: &mut JSTokenData,
        lexer_flags: LexerFlags,
        strict_mode: bool,
    ) -> JSTokenType {
        token_data.escaped = false;
        let remaining = (self.code_end - self.code) as isize;
        if remaining >= MAX_TOKEN_LENGTH as isize && !lexer_flags.contains(LexerFlags::IGNORE_RESERVED_WORDS) {
            let keyword = self.parse_keyword::<SHOULD_CREATE_IDENTIFIER>(token_data);
            if keyword != IDENT {
                return if keyword == RESERVED_IF_STRICT && !strict_mode { IDENT } else { keyword };
            }
        }

        let is_private_name = self.current_unit() == '#' as u32;
        let identifier_start = self.current_source_ptr();

        if is_private_name {
            self.shift();
        }

        let mut or_all_chars: u16 = 0;

        let source = self.source.clone();

        // Attempt SIMD scan first (predicado escalar do C++)
        let start = self.current_source_ptr();
        let found = start
            + simd_find(&source[start..self.code_end], |c: T| {
                let c: u32 = c.into();
                !crate::wtf::ascii_ctype::is_ascii_alphanumeric(c) && c != '_' as u32 && c != '$' as u32
            });
        self.code = found;
        self.current = if found < self.code_end { source[found] } else { T::from_u16(0) };
        // No need to update orAllChars: all SIMD-matched chars are ASCII, so they don't affect orAllChars & ~0xFF

        // Scalar fallback for non-ASCII identifier parts
        while is_single_character_ident_part(self.current.to_u16()) {
            or_all_chars |= self.current.to_u16();
            self.shift();
        }

        if u16_is_surrogate(self.current_unit()) || self.current_unit() == '\\' as u32 {
            return self.parse_identifier_slow_case::<SHOULD_CREATE_IDENTIFIER>(
                token_data,
                lexer_flags,
                strict_mode,
                identifier_start,
            );
        }

        let is_all_8_bit = (or_all_chars & !0xff) == 0;
        let mut ident: Option<Identifier> = None;

        if SHOULD_CREATE_IDENTIFIER {
            let identifier_span = &source[identifier_start..self.current_source_ptr()];
            let made = if is_all_8_bit {
                self.make_lchar_identifier(identifier_span)
            } else {
                self.make_identifier(identifier_span)
            };
            ident = Some(made);
            token_data.ident = ident.clone();
        } else {
            token_data.ident = None;
        }

        if is_private_name {
            return PRIVATENAME;
        }

        if remaining < MAX_TOKEN_LENGTH as isize && !lexer_flags.contains(LexerFlags::IGNORE_RESERVED_WORDS) {
            let entry = ident.as_ref().and_then(main_table_lexer_value);
            let Some(token) = entry else {
                return IDENT;
            };
            return if token != RESERVED_IF_STRICT || strict_mode { token } else { IDENT };
        }

        IDENT
    }

    /// A lambda `fillBuffer` do `parseIdentifierSlowCase`: `identifier_start` é a captura por
    /// referência do C++.
    fn fill_identifier_buffer<const SHOULD_CREATE_IDENTIFIER: bool>(
        &mut self,
        token_data: &mut JSTokenData,
        identifier_start: &mut usize,
        ident_type: JSTokenType,
        is_start: bool,
    ) -> JSTokenType {
        let source = self.source.clone();

        // \uXXXX unicode characters or Surrogate pairs.
        if *identifier_start != self.current_source_ptr() {
            self.append16(&source[*identifier_start..self.current_source_ptr()]);
        }

        if self.current_unit() == '\\' as u32 {
            token_data.escaped = true;
            self.shift();
            if self.current_unit() != 'u' as u32 {
                return if self.at_end() {
                    UNTERMINATED_IDENTIFIER_ESCAPE_ERRORTOK
                } else {
                    INVALID_IDENTIFIER_ESCAPE_ERRORTOK
                };
            }
            self.shift();
            let character = self.parse_unicode_escape();
            if !character.is_valid() {
                return if character.is_incomplete() {
                    UNTERMINATED_IDENTIFIER_UNICODE_ESCAPE_ERRORTOK
                } else {
                    INVALID_IDENTIFIER_UNICODE_ESCAPE_ERRORTOK
                };
            }
            let value = character.value();
            if if is_start { !is_ident_start(value) } else { !is_ident_part(value) } {
                return INVALID_IDENTIFIER_UNICODE_ESCAPE_ERRORTOK;
            }
            if SHOULD_CREATE_IDENTIFIER {
                self.record_unicode_code_point(value);
            }
            *identifier_start = self.current_source_ptr();
            return ident_type;
        }

        if !u16_is_surrogate_lead(self.current_unit()) {
            return INVALID_UNICODE_ENCODING_ERRORTOK;
        }

        let code_point = self.current_code_point();
        if code_point == Self::ERROR_CODE_POINT {
            return INVALID_UNICODE_ENCODING_ERRORTOK;
        }
        if if is_start { !is_non_latin1_ident_start(code_point) } else { !is_non_latin1_ident_part(code_point) } {
            return INVALID_IDENTIFIER_UNICODE_ERRORTOK;
        }
        self.append16(&source[self.code..self.code + 2]);
        self.shift();
        self.shift();
        *identifier_start = self.current_source_ptr();
        ident_type
    }

    fn parse_identifier_slow_case<const SHOULD_CREATE_IDENTIFIER: bool>(
        &mut self,
        token_data: &mut JSTokenData,
        lexer_flags: LexerFlags,
        strict_mode: bool,
        identifier_start: usize,
    ) -> JSTokenType {
        let source = self.source.clone();
        let mut identifier_start = identifier_start;

        let mut ident_chars_start = identifier_start;
        let is_private_name = Into::<u32>::into(source[identifier_start]) == '#' as u32;
        if is_private_name {
            ident_chars_start += 1;
        }

        let ident_type = if is_private_name { PRIVATENAME } else { IDENT };

        let mut type_ = self.fill_identifier_buffer::<SHOULD_CREATE_IDENTIFIER>(
            token_data,
            &mut identifier_start,
            ident_type,
            ident_chars_start == self.current_source_ptr(),
        );
        if type_ & CAN_BE_ERROR_TOKEN_FLAG != 0 {
            return type_;
        }

        loop {
            if is_single_character_ident_part(self.current.to_u16()) {
                self.shift();
                continue;
            }
            if !u16_is_surrogate(self.current_unit()) && self.current_unit() != '\\' as u32 {
                break;
            }

            type_ = self.fill_identifier_buffer::<SHOULD_CREATE_IDENTIFIER>(
                token_data,
                &mut identifier_start,
                ident_type,
                false,
            );
            if type_ & CAN_BE_ERROR_TOKEN_FLAG != 0 {
                return type_;
            }
        }

        let mut ident: Option<Identifier> = None;
        if SHOULD_CREATE_IDENTIFIER {
            if identifier_start != self.current_source_ptr() {
                self.append16(&source[identifier_start..self.current_source_ptr()]);
            }
            let made = self.make_identifier_from_buffer16();
            token_data.ident = Some(made.clone());
            ident = Some(made);
        } else {
            token_data.ident = None;
        }

        self.buffer16.clear();

        if !lexer_flags.contains(LexerFlags::IGNORE_RESERVED_WORDS) {
            let Some(token) = ident.as_ref().and_then(main_table_lexer_value) else {
                return ident_type;
            };
            if token != RESERVED_IF_STRICT || strict_mode {
                return ESCAPED_KEYWORD;
            }
        }

        ident_type
    }

    /// O trecho repetido de `parseString` que desiste do caminho rápido: volta ao início da
    /// string (offset, início de linha, número da linha), zera `m_buffer8` e reparseia pelo
    /// `parseStringSlowCase`.
    fn restart_string_in_slow_case<const SHOULD_BUILD_STRINGS: bool>(
        &mut self,
        token_data: &mut JSTokenData,
        strict_mode: bool,
        starting_offset: i32,
        starting_line_start_offset: i32,
        starting_line_number: i32,
    ) -> StringParseResult {
        self.set_offset(starting_offset, starting_line_start_offset);
        self.set_line_number(starting_line_number);
        self.buffer8.clear();
        self.parse_string_slow_case::<SHOULD_BUILD_STRINGS>(token_data, strict_mode)
    }

    fn parse_string<const SHOULD_BUILD_STRINGS: bool>(
        &mut self,
        token_data: &mut JSTokenData,
        strict_mode: bool,
    ) -> StringParseResult {
        let starting_offset = self.current_offset();
        let starting_line_start_offset = self.current_line_start_offset();
        let starting_line_number = self.line_number();
        let string_quote_character = self.current;
        self.shift();

        let source = self.source.clone();
        let mut string_start = self.current_source_ptr();

        let quote: u32 = string_quote_character.into();
        let scalar_match = |character: T| -> bool {
            let character: u32 = character.into();
            if character == quote {
                return true;
            }
            if character == '\\' as u32 {
                return true;
            }
            if character < 0xE {
                return true;
            }

            if T::SIZE == 1 || !SHOULD_BUILD_STRINGS {
                false
            } else {
                character > 0xFF
            }
        };

        let mut found = string_start + simd_find(&source[string_start..self.code_end], &scalar_match);
        if found == self.code_end {
            return self.restart_string_in_slow_case::<SHOULD_BUILD_STRINGS>(
                token_data,
                strict_mode,
                starting_offset,
                starting_line_start_offset,
                starting_line_number,
            );
        }

        self.code = found;
        self.current = source[found];
        if self.current == string_quote_character {
            if SHOULD_BUILD_STRINGS {
                token_data.ident = Some(self.make_lchar_identifier(&source[string_start..found]));
            } else {
                token_data.ident = None;
            }
            return StringParseResult::StringParsedSuccessfully;
        }

        while self.current != string_quote_character {
            if self.current_unit() == '\\' as u32 {
                if SHOULD_BUILD_STRINGS && string_start != self.current_source_ptr() {
                    self.append8(&source[string_start..self.current_source_ptr()]);
                }
                self.shift();

                let escape = single_escape(self.current_unit());

                // Most common escape sequences first.
                if escape != 0 {
                    if SHOULD_BUILD_STRINGS {
                        self.record8(escape as u32);
                    }
                    self.shift();
                } else if Self::is_line_terminator(self.current) {
                    self.shift_line_terminator();
                } else if self.current_unit() == 'x' as u32 {
                    self.shift();
                    if !crate::wtf::ascii_ctype::is_ascii_hex_digit(self.current_unit())
                        || !crate::wtf::ascii_ctype::is_ascii_hex_digit(Into::<u32>::into(self.peek(1)))
                    {
                        self.lex_error_message = lex_message("\\x can only be followed by a hex character sequence");
                        return if self.at_end()
                            || (crate::wtf::ascii_ctype::is_ascii_hex_digit(self.current_unit())
                                && self.code + 1 == self.code_end)
                        {
                            StringParseResult::StringUnterminated
                        } else {
                            StringParseResult::StringCannotBeParsed
                        };
                    }
                    let prev = self.current_unit();
                    self.shift();
                    if SHOULD_BUILD_STRINGS {
                        self.record8(Self::convert_hex(prev, self.current_unit()) as u32);
                    }
                    self.shift();
                } else {
                    return self.restart_string_in_slow_case::<SHOULD_BUILD_STRINGS>(
                        token_data,
                        strict_mode,
                        starting_offset,
                        starting_line_start_offset,
                        starting_line_number,
                    );
                }
                string_start = self.current_source_ptr();

                // Retry SIMD to skip the next plain segment to an interesting character
                found = string_start + simd_find(&source[string_start..self.code_end], &scalar_match);
                if found == self.code_end {
                    return self.restart_string_in_slow_case::<SHOULD_BUILD_STRINGS>(
                        token_data,
                        strict_mode,
                        starting_offset,
                        starting_line_start_offset,
                        starting_line_number,
                    );
                }
                self.code = found;
                self.current = source[found];

                if character_requires_parse_string_slow_case(self.current) {
                    return self.restart_string_in_slow_case::<SHOULD_BUILD_STRINGS>(
                        token_data,
                        strict_mode,
                        starting_offset,
                        starting_line_start_offset,
                        starting_line_number,
                    );
                }
                continue;
            }

            if character_requires_parse_string_slow_case(self.current) {
                return self.restart_string_in_slow_case::<SHOULD_BUILD_STRINGS>(
                    token_data,
                    strict_mode,
                    starting_offset,
                    starting_line_start_offset,
                    starting_line_number,
                );
            }

            self.shift();
        }

        if SHOULD_BUILD_STRINGS {
            if self.current_source_ptr() != string_start {
                self.append8(&source[string_start..self.current_source_ptr()]);
            }
            let buffer = std::mem::take(&mut self.buffer8);
            token_data.ident = Some(self.make_identifier(&buffer[..]));
            self.buffer8 = buffer;
            self.buffer8.clear();
        } else {
            token_data.ident = None;
        }

        StringParseResult::StringParsedSuccessfully
    }

    fn parse_complex_escape<const SHOULD_BUILD_STRINGS: bool>(&mut self, strict_mode: bool) -> StringParseResult {
        if self.current_unit() == 'x' as u32 {
            self.shift();
            if !crate::wtf::ascii_ctype::is_ascii_hex_digit(self.current_unit())
                || !crate::wtf::ascii_ctype::is_ascii_hex_digit(Into::<u32>::into(self.peek(1)))
            {
                // For raw template literal syntax, we consume `NotEscapeSequence`.
                //
                // NotEscapeSequence ::
                //     x [lookahread not one of HexDigit]
                //     x HexDigit [lookahread not one of HexDigit]
                if crate::wtf::ascii_ctype::is_ascii_hex_digit(self.current_unit()) {
                    self.shift();
                }

                self.lex_error_message = lex_message("\\x can only be followed by a hex character sequence");
                return if self.at_end() {
                    StringParseResult::StringUnterminated
                } else {
                    StringParseResult::StringCannotBeParsed
                };
            }

            let prev = self.current_unit();
            self.shift();
            if SHOULD_BUILD_STRINGS {
                self.record16(Self::convert_hex(prev, self.current_unit()) as u32);
            }
            self.shift();

            return StringParseResult::StringParsedSuccessfully;
        }

        if self.current_unit() == 'u' as u32 {
            self.shift();

            let character = self.parse_unicode_escape();
            if character.is_valid() {
                if SHOULD_BUILD_STRINGS {
                    self.record_unicode_code_point(character.value());
                }
                return StringParseResult::StringParsedSuccessfully;
            }

            self.lex_error_message = lex_message("\\u can only be followed by a Unicode character sequence");
            return if self.at_end() {
                StringParseResult::StringUnterminated
            } else {
                StringParseResult::StringCannotBeParsed
            };
        }

        if strict_mode {
            if crate::wtf::ascii_ctype::is_ascii_digit(self.current_unit()) {
                // The only valid numeric escape in strict mode is '\0', and this must not be followed by a decimal digit.
                let character1 = self.current_unit();
                self.shift();
                if character1 != '0' as u32 || crate::wtf::ascii_ctype::is_ascii_digit(self.current_unit()) {
                    // For raw template literal syntax, we consume `NotEscapeSequence`.
                    //
                    // NotEscapeSequence ::
                    //     0 DecimalDigit
                    //     DecimalDigit but not 0
                    if character1 == '0' as u32 {
                        self.shift();
                    }

                    self.lex_error_message = lex_message("The only valid numeric escape in strict mode is '\\0'");
                    return if self.at_end() {
                        StringParseResult::StringUnterminated
                    } else {
                        StringParseResult::StringCannotBeParsed
                    };
                }
                if SHOULD_BUILD_STRINGS {
                    self.record16(0);
                }
                return StringParseResult::StringParsedSuccessfully;
            }
        } else if crate::wtf::ascii_ctype::is_ascii_octal_digit(self.current_unit()) {
            // Octal character sequences
            let character1 = self.current_unit();
            self.shift();
            if crate::wtf::ascii_ctype::is_ascii_octal_digit(self.current_unit()) {
                // Two octal characters
                let character2 = self.current_unit();
                self.shift();
                if character1 >= '0' as u32
                    && character1 <= '3' as u32
                    && crate::wtf::ascii_ctype::is_ascii_octal_digit(self.current_unit())
                {
                    if SHOULD_BUILD_STRINGS {
                        self.record16(
                            (character1 - '0' as u32) * 64 + (character2 - '0' as u32) * 8 + self.current_unit()
                                - '0' as u32,
                        );
                    }
                    self.shift();
                } else if SHOULD_BUILD_STRINGS {
                    self.record16((character1 - '0' as u32) * 8 + character2 - '0' as u32);
                }
            } else if SHOULD_BUILD_STRINGS {
                self.record16(character1 - '0' as u32);
            }
            return StringParseResult::StringParsedSuccessfully;
        }

        if !self.at_end() {
            if SHOULD_BUILD_STRINGS {
                self.record16(self.current_unit());
            }
            self.shift();
            return StringParseResult::StringParsedSuccessfully;
        }

        self.lex_error_message = lex_message("Unterminated string constant");
        StringParseResult::StringUnterminated
    }

    fn parse_string_slow_case<const SHOULD_BUILD_STRINGS: bool>(
        &mut self,
        token_data: &mut JSTokenData,
        strict_mode: bool,
    ) -> StringParseResult {
        let string_quote_character = self.current;
        self.shift();

        let source = self.source.clone();
        let mut string_start = self.current_source_ptr();

        while self.current != string_quote_character {
            if self.current_unit() == '\\' as u32 {
                if SHOULD_BUILD_STRINGS && string_start != self.current_source_ptr() {
                    self.append16(&source[string_start..self.current_source_ptr()]);
                }
                self.shift();

                let escape = single_escape(self.current_unit());

                // Most common escape sequences first
                if escape != 0 {
                    if SHOULD_BUILD_STRINGS {
                        self.record16(escape as u32);
                    }
                    self.shift();
                } else if Self::is_line_terminator(self.current) {
                    self.shift_line_terminator();
                } else {
                    let result = self.parse_complex_escape::<SHOULD_BUILD_STRINGS>(strict_mode);
                    if result != StringParseResult::StringParsedSuccessfully {
                        return result;
                    }
                }

                string_start = self.current_source_ptr();
                continue;
            }
            // Fast check for characters that require special handling.
            // Catches 0, \n, and \r as efficiently as possible, and lets through all common ASCII characters.
            if self.current_unit() < 0xE {
                // New-line or end of input is not allowed
                if self.at_end() || self.current_unit() == '\r' as u32 || self.current_unit() == '\n' as u32 {
                    self.lex_error_message = lex_message("Unexpected EOF");
                    return if self.at_end() {
                        StringParseResult::StringUnterminated
                    } else {
                        StringParseResult::StringCannotBeParsed
                    };
                }
                // Anything else is just a normal character
            }
            self.shift();
        }

        if SHOULD_BUILD_STRINGS {
            if self.current_source_ptr() != string_start {
                self.append16(&source[string_start..self.current_source_ptr()]);
            }
            token_data.ident = Some(self.make_identifier_from_buffer16());
        } else {
            token_data.ident = None;
        }

        self.buffer16.clear();
        StringParseResult::StringParsedSuccessfully
    }

    fn parse_template_literal(
        &mut self,
        token_data: &mut JSTokenData,
        raw_strings_build_mode: RawStringsBuildMode,
    ) -> StringParseResult {
        let build_raw_strings = raw_strings_build_mode == RawStringsBuildMode::BuildRawStrings;
        let mut parse_cooked_failed = false;
        let source = self.source.clone();
        let mut string_start = self.current_source_ptr();
        let mut raw_string_start = self.current_source_ptr();

        while self.current_unit() != '`' as u32 {
            if self.current_unit() == '\\' as u32 {
                if string_start != self.current_source_ptr() {
                    self.append16(&source[string_start..self.current_source_ptr()]);
                }
                self.shift();

                let escape = single_escape(self.current_unit());

                // Most common escape sequences first.
                if escape != 0 {
                    self.record16(escape as u32);
                    self.shift();
                } else if Self::is_line_terminator(self.current) {
                    // Normalize <CR>, <CR><LF> to <LF>.
                    if self.current_unit() == '\r' as u32 {
                        if build_raw_strings {
                            self.buffer_for_raw_template_string16
                                .extend(source[raw_string_start..self.current_source_ptr()].iter().map(|&c| c.to_u16()));
                            self.buffer_for_raw_template_string16.push('\n' as u16);
                        }

                        self.shift_line_terminator();
                        raw_string_start = self.current_source_ptr();
                    } else {
                        self.shift_line_terminator();
                    }
                } else {
                    let strict_mode = true;
                    let result = self.parse_complex_escape::<true>(strict_mode);
                    if result != StringParseResult::StringParsedSuccessfully {
                        if build_raw_strings && result == StringParseResult::StringCannotBeParsed {
                            parse_cooked_failed = true;
                        } else {
                            return result;
                        }
                    }
                }

                string_start = self.current_source_ptr();
                continue;
            }

            if self.current_unit() == '$' as u32 && Into::<u32>::into(self.peek(1)) == '{' as u32 {
                break;
            }

            // Fast check for characters that require special handling.
            // Catches 0, \n, \r, 0x2028, and 0x2029 as efficiently
            // as possible, and lets through all common ASCII characters.
            if (self.current_unit().wrapping_sub(0xE) & 0x2000) != 0 {
                // End of input is not allowed.
                // Unlike String, line terminator is allowed.
                if self.at_end() {
                    self.lex_error_message = lex_message("Unexpected EOF");
                    return StringParseResult::StringUnterminated;
                }

                if Self::is_line_terminator(self.current) {
                    if self.current_unit() == '\r' as u32 {
                        // Normalize <CR>, <CR><LF> to <LF>.
                        if string_start != self.current_source_ptr() {
                            self.append16(&source[string_start..self.current_source_ptr()]);
                        }
                        if raw_string_start != self.current_source_ptr() && build_raw_strings {
                            self.buffer_for_raw_template_string16
                                .extend(source[raw_string_start..self.current_source_ptr()].iter().map(|&c| c.to_u16()));
                        }

                        self.record16('\n' as u32);
                        if build_raw_strings {
                            self.buffer_for_raw_template_string16.push('\n' as u16);
                        }
                        self.shift_line_terminator();
                        string_start = self.current_source_ptr();
                        raw_string_start = self.current_source_ptr();
                    } else {
                        self.shift_line_terminator();
                    }
                    continue;
                }
                // Anything else is just a normal character
            }

            self.shift();
        }

        let is_tail = self.current_unit() == '`' as u32;

        if self.current_source_ptr() != string_start {
            self.append16(&source[string_start..self.current_source_ptr()]);
        }
        if raw_string_start != self.current_source_ptr() && build_raw_strings {
            self.buffer_for_raw_template_string16
                .extend(source[raw_string_start..self.current_source_ptr()].iter().map(|&c| c.to_u16()));
        }

        if !parse_cooked_failed {
            token_data.cooked = Some(self.make_identifier_from_buffer16());
        } else {
            token_data.cooked = None;
        }

        // Line terminator normalization (e.g. <CR> => <LF>) should be applied to both the raw and cooked representations.
        if build_raw_strings {
            let raw_buffer = std::mem::take(&mut self.buffer_for_raw_template_string16);
            token_data.raw = Some(self.make_identifier(&raw_buffer[..]));
            self.buffer_for_raw_template_string16 = raw_buffer;
        } else {
            token_data.raw = None;
        }

        token_data.is_tail = is_tail;

        self.buffer16.clear();
        self.buffer_for_raw_template_string16.clear();

        if is_tail {
            // Skip `
            self.shift();
        } else {
            // Skip $ and {
            self.shift();
            self.shift();
        }

        StringParseResult::StringParsedSuccessfully
    }
}

/// As mensagens de erro do lexer são ASCII puro: o `String` da WTF nasce Latin1.
fn lex_message(message: &str) -> crate::wtf::text::wtf_string::String {
    crate::wtf::text::wtf_string::String::from_latin1(message.as_bytes())
}
