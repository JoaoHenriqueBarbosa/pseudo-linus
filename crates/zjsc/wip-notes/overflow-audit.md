# Auditoria de overflow e pânico: wtf e yarr (2026-10-08)

Escopo: `src/wtf/text/{string_impl,wtf_string,string_builder}.rs`, `src/wtf/{dtoa,unicode}` e `src/yarr/*`.
Triagem de 5 minutos, sem cargo. Critério: alcançável por JS ou por regexp hostil.

## Já tratado como no C++ (nada a mudar)

- Quantificador `{n,m}`: `consume_number64` usa `checked_mul`/`checked_add` e devolve o infinito;
  `min == infinito` vira `QuantifierTooLarge`, `min > max` vira `QuantifierOutOfOrder`
  (`/a{4294967295}/`, `/a{99999999999999999999}/`).
- `setup_alternative_offsets` e `setup_disjunction_offsets` (`yarr_pattern_cpp4.rs`) usam `CheckedUint32`
  e viram `OffsetTooLarge`/`FrameTooLarge` ("pattern exceeds string length limits", "too many frame slots").
- `quantify_atom`: `max - min` só roda com `min <= max` (garantido pelo parser, espelha ASSERT).
- `copy_disjunction`/`copy_term`: limite de recursão vira `PatternTooLarge` ("regular expression too large").
- `\u{...}`: o limite `UCHAR_MAX_VALUE` é checado a cada dígito, então o shift nunca estoura.
- `StringBuilder`: `saturating_sum` e `did_overflow` (CRASH ou flag, conforme a política); `append_substring`
  limita `length` a `total - offset`.
- `StringImpl::replace` e `reserve`: checagens de `MaxLength` antes da aritmética; os `panic!` são o `CRASH()` do C++
  (mesmo comportamento observável de abortar), não precisam virar erro tratado aqui.
- `wtf_string.rs` e `string_impl.rs` (conversão UTF-8): `checked_mul(2)`/`checked_mul(3)` no tamanho do buffer.

## Mudado

- `yarr_interpreter_cpp2.rs`: seis `input_position + 1` / `input_offset + 1` viram `wrapping_add(1)`,
  que é a soma de `unsigned` do C++. `input_position` pode chegar a `u32::MAX` pela soma checada
  (por exemplo `{4294967294}`) e `+ 1` entraria em pânico em debug. Linhas 158, 189, 235, 293, 426, 519.

## Invariantes (espelham ASSERT/RELEASE_ASSERT_NOT_REACHED), mantidos

- `expect`/`unreachable!` em `yarr_parser.rs` (pilhas de nomes de grupo, `pop_parenthesis`), `yarr_pattern.rs`
  (acessores de `PatternTerm` por tipo), `yarr_pattern_cpp6.rs:1706`, `yarr_interpreter_cpp3/4/5.rs`
  (tipos de ByteTerm que o ByteCompiler nunca emite, pilha de parênteses).

## Não coberto nesta passada

- `wtf/dtoa/*`, `wtf/unicode/*` (somente varredura por `*=`/`+=` em `utf8_conversion.rs`, sem achados de entrada JS).
- `yarr_interpreter_cpp4/5/6.rs` e `yarr_pattern_cpp5.rs:819` não foram lidos linha a linha.
- `'x'.repeat(2**30)` e `padStart` vivem no runtime, fora de `wtf`/`yarr`.

# Triagem do runtime (array, string, typed array, JSON, Date, Map/Set, Number, Math, RegExp)

Arquivos lidos: `array_prototype.rs`, `string_prototype.rs`, `string_prototype_natives*.rs`,
`string_regexp_support.rs`, `typed_array_*` (prototype, natives, constructors, support, adaptors),
`json_object*.rs`, `json_host.rs`, `date_*`, `map_prototype.rs`, `set_prototype.rs`, `number_prototype.rs`,
`math_object.rs`, `reg_exp_prototype_natives.rs`, `reg_exp_object.rs`, `reg_exp_matches_array.rs`,
`reg_exp_global_data.rs`. (Não existem arquivos `regexp_*`; o nome real é `reg_exp_*`.)

## Mudado

- `string_prototype.rs` `pad`: `max_length` NaN virava 0 no cast e `max_length as usize - units.len()` estourava;
  a guarda agora é `!(max_length > len)`.
- `string_prototype_natives_part2.rs` `replace_using_string_search`: o resultado de `replace`/`replaceAll` passa a
  conferir `MAX_LENGTH` a cada casamento e devolve `Thrown::OutOfMemory` (o `StringBuilder` do C++ marca overflow
  e o chamador lança OutOfMemoryError). Antes o `Vec` crescia sem limite.
- `array_prototype.rs` `join_with_separator`: `separador * (length - 1)` e a soma dos pedaços são conferidos contra
  `MAX_LENGTH` (OutOfMemoryError, como o `StringBuilder` do C++); antes `[].join` com `length` enorme
  acumulava sem limite.
- `array_prototype.rs` `checked_result_length` (toReversed, with, toSpliced, toSorted): acima de
  `MAX_STORAGE_VECTOR_LENGTH` vira OutOfMemoryError (o `tryCreateUninitializedRestricted` do C++ falha), em vez de
  encher um `Vec` de até 2^32 elementos.
- `array_prototype.rs` `copy_range` (slice/splice) e `ConcatSink::Elements` (concat): sem `ArrayStorage` esparso, o
  `Vec` de buracos é limitado a `MAX_STORAGE_VECTOR_LENGTH` e o excedente é a lacuna `unported` já usada em
  `new_array`, não crescimento sem teto.
- `number_prototype.rs` `toFixed`, `toExponential`, `toPrecision` e `extract_to_string_radix_argument`
  (`toString(radix)`, também usado por BigInt): usavam `to_integer_or_infinity()` sem o `RETURN_IF_EXCEPTION`
  do C++. Um `Symbol` ou `valueOf` que lança devolvia string normal ou RangeError por cima da exceção pendente;
  agora `to_integer_or_infinity_checked()?`.
- `reg_exp_prototype_natives.rs` `@@replace`: `(result_length - 1) as u32` truncava `length` acima de 2^32 de um
  `exec` hostil; agora satura em `u32::MAX`.

## Vistas e já seguras (mantidas)

- Aritmética de índice em `u64` no Array (length até 2^53-1 não estoura `u64`; `length + arg_count` é conferido
  contra 2^53-1 antes de `push`/`unshift`/`splice`/`concat`); `index_u32` devolve erro (nunca pânico) acima de
  `MAX_ARRAY_INDEX`; `clamp_relative`/`argument_clamped_index_from_start_or_end` não recebem NaN (`toIntegerOrInfinity`).
- Subtrações `length - start`, `length - k - 1`, `length - to.max(from)`, `final_index - from` têm o operando
  menor garantido pelo clamp ou por guarda explícita (`copyWithin`, `splice`, `toSpliced`, `reverse`, `lastIndexOf`).
- `String.prototype`: `starts_with`/`ends_with`/`substr`/`slice`/`split`/`concat`/`repeat` (`checked_mul`) conferidos;
  `repeat(NaN)` cai em 0 sem pânico.
- Typed arrays: `can_access_range_quickly` usa `checked_add`; `set_from_array_like` usa `checked_add`;
  `create` usa `ConstructionContext::with_length` (`None` vira OutOfMemory); `copyWithin` reconfere o comprimento
  após a conversão dos argumentos (buffer redimensionável).
- JSON: `gap` (clamp em 10, NaN vira 0), `index`/`size` em `u32` com `length > u32::MAX` virando OutOfMemory,
  pilhas do walker (`expect`) são invariantes do laço de estados.
- Date: tudo em `f64`/`i32` já protegido por `ok = years.abs() <= max_year` e `time_clip`; sem pânico por entrada JS.
- Set/Map: `get_set_size_as_int` usa cast saturante; `forEach` com `expect` está só em teste.
- `unwrap`/`expect`/`unreachable!`/`panic!` restantes (typed_array_type, typed_array_adaptors, typed_array_realm,
  json_host, date_prototype_natives, array_prototype testes) espelham `ASSERT`/`RELEASE_ASSERT_NOT_REACHED`
  (tipos que o chamador já filtrou, realm sempre presente).

## Pendências resolvidas (2026-10-08)

- `make_string_by_joining` é fiel ao WTF (`makeStringByJoining` também usa `builder.isEmpty()`), então não foi
  alterado; o bug estava em quem o chamava: `Array.prototype.join`/`toLocaleString` e `Iterator.prototype.join`
  usam o `JSStringJoiner` no C++, que põe o separador por índice. Novo `join_runs_with_separator` em `wtf_string.rs`
  (n - 1 separadores), usado por `array_prototype.rs` e `iterator_prototype.rs`. `['', 'a'].join(',')` volta a dar `,a`.
- Buracos e `undefined`/`null` seguidos no `join` viram uma corrida `(pedaço, repetições)`: sem uma `String` vazia por
  elemento.
- Typed array gigante: medido no bun 1.4.2, `new Uint8Array(2**53)` é `RangeError: length larger than (2 ** 53) - 1`,
  `new Float64Array(2**40)` é `RangeError: Out of memory`, `new Uint8Array(-1)` é `RangeError: length cannot be
  negative`. O porte já dá exatamente isso (`to_index` e `Thrown::OutOfMemory` é o RangeError "Out of memory");
  "Invalid typed array length" não existe nessa versão do bun, então nada mudou.
- Sem teste novo rodado (cargo proibido nesta tarefa); falta um teste de `join_runs_with_separator` e o caso
  `['', 'a'].join(',')`.

## Pendências antigas (histórico)

- `wtf_string.rs` `make_string_by_joining` (fora do escopo): usa `result.is_empty()` para decidir se põe o
  separador, então `[ '', 'a' ].join(',')` perde a vírgula do começo; parece bug de corretude, não de overflow.
- `join` e `toLocaleString` com `length` ~2^32 de buracos ainda empilham um `WtfString` vazio por elemento (o C++
  também percorre, mas sem alocar por elemento).
- Typed array com comprimento gigante devolve OutOfMemory onde o C++ lança `RangeError: Invalid typed array length`.
