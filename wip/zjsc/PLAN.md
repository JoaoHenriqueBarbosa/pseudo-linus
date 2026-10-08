# zjsc: plano vivo do porte (retomar daqui depois de compactação)

Branch `wip-javascriptcore`. Roda de 5 Sonnets (só escrevem, nunca compilam); eu integro com
`cargo build` em segundo plano dentro de `wip/zjsc`. Fatia: no máximo 5 minutos de agente
(hoje, cerca de 400 a 800 linhas de C++); ajustar pelo tempo medido de cada agente.

## Camada 0: WTF

| Fatia | Origem | Destino | Estado |
|---|---|---|---|
| utils+ieee | dtoa/utils.h, dtoa/ieee.h | src/wtf/dtoa/{utils,ieee}.rs | roda |
| bignum | dtoa/bignum.{h,cc} | src/wtf/dtoa/bignum.rs | roda |
| diy_fp+cached_powers | dtoa/diy-fp.*, cached-powers.* | src/wtf/dtoa/{diy_fp,cached_powers}.rs | roda |
| fast_dtoa | dtoa/fast-dtoa.* | src/wtf/dtoa/fast_dtoa.rs | roda |
| ascii_ctype+fixed_dtoa | wtf/ASCIICType.h, dtoa/fixed-dtoa.* | src/wtf/ascii_ctype.rs, src/wtf/dtoa/fixed_dtoa.rs | roda |
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
