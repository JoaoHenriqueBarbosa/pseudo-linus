# zjsc: plano vivo do porte (retomar daqui depois de compactação)

Branch `wip-javascriptcore`. Roda de 5 Sonnets (só escrevem, nunca compilam); eu integro com
`cargo build` em segundo plano dentro de `wip/zjsc`. Fatia: no máximo 5 minutos de agente
(hoje, cerca de 400 a 800 linhas de C++); ajustar pelo tempo medido de cada agente.

## Camada 0: WTF

| Fatia | Origem | Destino | Estado |
|---|---|---|---|
| utils+ieee | dtoa/utils.h, dtoa/ieee.h | src/wtf/dtoa/{utils,ieee}.rs | feito |
| bignum | dtoa/bignum.{h,cc} | src/wtf/dtoa/bignum.rs | feito |
| diy_fp+cached_powers | dtoa/diy-fp.*, cached-powers.* | src/wtf/dtoa/{diy_fp,cached_powers}.rs | feito |
| fast_dtoa | dtoa/fast-dtoa.* | src/wtf/dtoa/fast_dtoa.rs | feito |
| ascii_ctype+fixed_dtoa | wtf/ASCIICType.h, dtoa/fixed-dtoa.* | src/wtf/ascii_ctype.rs, src/wtf/dtoa/fixed_dtoa.rs | feito |
| bignum_dtoa | dtoa/bignum-dtoa.* | src/wtf/dtoa/bignum_dtoa.rs | fila |
| strtod | dtoa/strtod.* | src/wtf/dtoa/strtod.rs | fila |
| double_conversion (1/2) | double-conversion.h + .cc até ToShortest/ToFixed | src/wtf/dtoa/double_conversion.rs | fila |
| double_conversion (2/2) | resto do .cc (StringToDouble) | idem | fila |
| text: StringImpl | wtf/text/StringImpl.{h,cpp} | src/wtf/text/string_impl.rs | fila |
| text: WTFString, StringBuilder, AtomString | wtf/text/* | src/wtf/text/*.rs | fila |
| unicode do lexer | ICU usado em parser/Lexer.cpp | src/wtf/unicode.rs | fila |

## Camadas seguintes

Ver `CONVENTIONS.md`, "Ordem de fechamento". A fila da camada 1 (parser) se fatia quando a camada 0
estiver compilando.

## Tempos medidos por agente

(anotar aqui: fatia, linhas de C++, minutos)
- lote 1 (09:24): utils+ieee 386+404 linhas 1,5 min; bignum 916 linhas 2,1 min; diy_fp+cached_powers 424 linhas 1,0 min; fast_dtoa 753 linhas 1,5 min; ascii_ctype+fixed_dtoa 757 linhas 2,0 min. Conclusão: fatias podem crescer para cerca de 1500 linhas.
- integrado e verde (41 testes): utils, ieee, diy_fp, cached_powers, bignum, fast_dtoa, fixed_dtoa, ascii_ctype.
- lote 2: Nodes.h 1-1205 (+construtores) levou 7,5 min: acima do teto. Fatias do parser caem para cerca de 800 linhas.
- dtoa.cpp + Dragonbox + golden: 12 min (escopo cresceu sozinho com o Dragonbox).

## Estado em 2026-10-08, fim da manhã

Feito e verde (151 testes + goldens de números, hash, caixa, identificadores): WTF dtoa inteiro
(com Dragonbox e numberToString), ascii_ctype, unicode (UTF-8, CharacterNames, case mapping, bidi,
ID_Start/ID_Continue, categoria geral; tabelas do UCD 17 por scripts/gen-*.py), StringImpl,
StringHasher, WTFString, AtomString, SymbolImpl, runtime::Identifier, PrivateName, VM (esqueleto),
yarr flags/erros/canonicalize UCS2, bytecode::opcode (gerado), parser tokens/modes/error,
VariableEnvironment, ParserArena, ResultType.

Escrito e fora da compilação (falta dependência): parser::nodes (+part2, part3) espera
source_code, module_scope_data, runtime::constructor_kind, runtime::implementation_visibility,
bytecode::bytecode_intrinsic_registry.

Fila (ordem): SourceCode/SourceProvider/UnlinkedSourceCode + ModuleScopeData + ConstructorKind +
ImplementationVisibility; KeywordLookup (gerar de parser/Keywords.table com script próprio) e
Lexer.lut.h (gerar); Lexer.cpp 1675-fim (números, lex principal); fast_float restante
(decimal_to_binary, bigint, digit_comparison, parse_number) e trocar o str::parse do WTFString;
StringBuilder; Parser.h e Parser.cpp em fatias de 800 linhas; ASTBuilder; SyntaxChecker; yarr
parser/pattern.cpp/interpreter; locale tr/lt/el no case mapping.

Dívida anotada: wtf_string make_string_by_joining aproxima a largura do StringBuilder;
UTF8ConversionError e ConversionMode duplicados em string_impl e wtf_string; U16_* duplicados.

## Fila acrescentada (tarde de 2026-10-08)

- runtime/OptionsList.h + Options.{h,cpp}: 586 opções, defaults literais e calculados (o lexer usa
  `exposePrivateIdentifiers`); o Bun liga opções no início (conferir em `.bun-src/src/bun.js`).
- CommonIdentifiers (gerar das macros) + BuiltinNames (nomes privados e símbolos) para
  `vm.property_names`.
- JSBigInt: o núcleo de parse/toString de que o parser precisa (`makeBigIntDecimalIdentifier`),
  depois a célula inteira.
- URLParser da WTF (o `wtf/url.rs` atual é parcial, não canoniza).
- VM: `DeferTermination`, `TopExceptionScope`.
- FEITO (0fe4fc19): YarrUnicodeProperties, tabelas por `scripts/gen-yarr-unicode-tables.py`. Na roda: StringView, ParseInt+Math, StringBuilder, CommonIdentifiers+BuiltinNames, JSBigInt fatia 1.

## Estado em 2026-10-08, noite

Verde e medido contra o bun: Yarr inteiro (parser, YarrPattern em seis fatias, interpretador em seis
fatias), com goldens `regexp-syntax` (990 casos) e `regexp-exec` (93 casos); JSBigInt; números;
canonicalização Unicode. Parser (Parser.h/.cpp, ASTBuilder partes 1 a 4, SyntaxChecker,
TreeBuilder) registrado em `parser/mod.rs`; falta fechar a compilação (cerca de 100 erros, lista em
`scripts` não: reproduzir com `cargo build --message-format short`). Faltam, para o parser fechar:
SourceProviderCache(+Item), ParseHash, DebuggerParseData, ClassElementDefinition (UnlinkedFunctionExecutable),
ProgramNode/EvalNode/ModuleProgramNode/FunctionNode (Nodes.h de 2035 em diante), Nodes.cpp,
NodeConstructors.h, FixedVector, MonotonicTime.

Bytecompiler escrito e FORA da compilação (nada registrado): register_id, label, label_scope,
static_property_*, bytecode_generator (.h inteiro em três arquivos) e .cpp em cpp1..cpp6,
bytecode_generator_base, nodes_codegen_cpp1/cpp1b/cpp2 (NodesCodegen.cpp: feito 1 a 2200 em curso;
faltam 2200 a 6473). Duplicatas conhecidas: JSGeneratorTraits (label.rs e bytecode_generator.rs),
BytecodeGenerator reexportado do part2, `new_label_scope_impl`.

Lições: `include!` não divide um `impl` de trait (usar `macro_rules!` expandida dentro do impl);
`continue` dentro de macro em `for` interno pega o laço errado (rótulo + macros definidas dentro do
laço); subtração `unsigned` do C++ pede `wrapping_*`.
