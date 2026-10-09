# CompressionStream brotli e zstd: medições no bun 1.4.2

Script de medição em /tmp (nome único), usando `CompressionStream`/`DecompressionStream` com escritas de um pedaço.

## Compressão (tudo sai num único pedaço, no flush)

Brotli (idêntico a `zlib.brotliCompressSync` padrão, ou seja qualidade 11, lgwin 22):

| entrada | saída hex |
|---|---|
| vazia (escrita de 0 bytes ou nenhuma escrita) | `3b` |
| `a` | `0b00806103` |
| `hello world hello world hello world` (também em 2 pedaços) | `1b2200f88d946ede44558696206c6f350b33b54006543b00` |
| 1000 zeros | `1be703f82700a2b1402034` |

Zstd (nível padrão 3, janela 2 MiB, sem checksum):

| entrada | saída hex |
|---|---|
| escrita vazia | `28b52ffd0058010000` |
| nenhuma escrita (só close) | `28b52ffd2000010000` (tamanho do conteúdo 0 declarado, segmento único) |
| `a` | `28b52ffd005809000061` |
| `hello world hello world hello world` | `28b52ffd00589500006068656c6c6f20776f726c64200100af4b12` |
| 1000 zeros | `28b52ffd00584d00001000000100e32b8005` |

O cabeçalho com descritor de janela (`0x58`) aparece quando houve escrita (o tamanho não é conhecido de antemão);
sem escrita o libzstd declara tamanho 0 em segmento único.

## Decompressão

- um pedaço ou o fluxo quebrado em 2 pedaços: saída única `hello world ...` e fim normal;
- brotli com lixo depois do fim: `TypeError` `Trailing junk found after the end of the compressed stream`,
  código `ERR_TRAILING_JUNK_AFTER_STREAM_END` (leitor e escritor);
- zstd com 3 bytes de lixo depois do quadro: o mesmo erro; zstd com dois quadros concatenados: valem, saída concatenada;
- entrada inválida (`garbage12345`): brotli `TypeError: brotli decode failed` com `code` `ERR__ERROR_FORMAT_PADDING_1`
  (código depende do ponto da falha; só este foi medido); zstd `TypeError: zstd decode failed` sem `code`;
- truncado (5 primeiros bytes): `TypeError: unexpected end of file` sem `code`, nos dois.

## Vetores grandes de brotli (medidos em 2026-10-09)

Entradas geradas por um gerador congruencial (`s = imul(s, 1103515245) + 12345`, byte = `s >>> 24`), idêntico no teste Rust
(`sample_text`, `sample_binary`). Texto: 100000 bytes de palavras gregas (semente 1, `words[next() & 15]` seguido de
espaço, ou `\n` se `next() & 7 == 0`), CRC-32 da entrada `be123bcf`. Binário: 100000 bytes da semente 7, CRC-32 `25143a6a`.
A saída do `CompressionStream` é igual à de `zlib.brotliCompressSync`.

| entrada | saída (bytes) | CRC-32 da saída | pedaços |
|---|---|---|---|
| texto 100 KB | 16859 | `2da4c3c0` | 16859 |
| binário 100 KB | 100005 | `f431d84f` | 65536 e 34469 |

## Entrega em pedaços (streaming)

- Toda descompressão (gzip, deflate, brotli, zstd) entrega a saída em pedaços de 65536 bytes, sobra no último
  (1800000 bytes de texto repetido: 27 pedaços de 65536 e um de 30528). A compressão de brotli também (65536 até o resto);
  gzip e deflate comprimindo entregam um pedaço por escrita, zstd comprimindo pedaços de ~131 KB.
- Brotli decodifica em streaming: a saída sai na escrita que a completa, não no flush. Com o fluxo de 61 bytes cortado
  em 10 escritas, saem pedaços de 4 e 31 bytes já nas escritas 6 e 7 e o resto (todo) na escrita 8; escrevendo byte a byte
  nos 2 primeiros e o resto, tudo sai na terceira escrita.
- Zstd decodifica em streaming por bloco: com a entrada em 10 escritas, os pedaços de 65536 saem 2 por escrita a partir da
  escrita 2; com metade do fluxo, saem 8 pedaços e o `flush` acusa `unexpected end of file`.
- Fluxo de brotli cortado ao meio: `TypeError: unexpected end of file` (sem `code`) lançado no `flush`, e o leitor vê o
  mesmo erro.
- Lixo depois do fim, na mesma escrita do fim: erro `ERR_TRAILING_JUNK_AFTER_STREAM_END` e NENHUMA saída (a saída do
  fluxo válido é descartada); em escrita posterior: a saída do fluxo sai antes e o erro vem na escrita seguinte.

## Códigos de erro de brotli (decodificação)

O `code` é `ERR_` mais o nome do `BrotliDecoderErrorCode` sem o prefixo `BROTLI_DECODER` (`ERR__ERROR_FORMAT_PADDING_1`);
a mensagem é sempre `brotli decode failed`, lançada na própria escrita (a escrita e o fechamento rejeitam).

| entrada | resultado |
|---|---|
| `ff` (e `ffffffff`) | `ERR__ERROR_FORMAT_PADDING_2` |
| `garbage12345` | `ERR__ERROR_FORMAT_PADDING_1` |
| `hello world, not brotli at all` | `ERR__ERROR_FORMAT_CL_SPACE` |
| cabeçalho gzip `1f8b08000000000000 03` | `ERR__ERROR_FORMAT_CL_SPACE` |
| 8 zeros, `aabbccddee`, `1bffff`, `00`, `05`, `1b2200f8ffffffff` | não é erro de formato: `unexpected end of file` no `flush` |

## Escolha de crates

- brotli: `brotli` 9 (rust-brotli, port do google/brotli) com qualidade 11 e lgwin 22, os parâmetros que reproduzem
  o `brotliCompressSync` padrão. Decodificação por `BrotliDecompressStream` direto (o `DecompressorWriter` esconde o
  código de erro). Os vetores pequenos e os grandes acima viraram testes em `compression_streams.rs`; a conferência
  executada depende de rodar o cargo (não rodado nesta fatia).
- zstd: compressor e descompressor próprios em `src/runtime/zstd/` (portes do libzstd 1.5.7); o `ruzstd` saiu do
  `Cargo.toml` (os testes do compressor conferem a ida e volta com `zstd::decompress::decode_all`). A descompressão
  usa `Decoder::push_frame` (para no fim de cada quadro) com a semântica do `CompressionStreamCoder`: depois de um
  quadro completo, o que não é prefixo de número mágico é `ERR_TRAILING_JUNK_AFTER_STREAM_END`, um prefixo curto
  espera a escrita seguinte (no `flush` vira lixo), quadros concatenados e skippable valem.

## Brotli comprimindo: quando sai a saída (medido em 2026-10-09, processo novo)

O fonte (`CompressionStreamCoder.rs`) chama `BrotliEncoderCompressStream` com `PROCESS` em cada escrita e `FINISH` no
close, num laço com espaço de saída `min(cap - len, folga do Vec)` (a mesma `spare` do zstd: crescimento de 16 KiB), com
`cap = max(highWaterMark, tamanho da escrita)` e, no close, `cap = highWaterMark`. Medido com a sonda
`/tmp/br_probe_c91e.js` (`bun sonda <texto|bin> <n> <passo> [hwm]`, binário semente 3): NENHUM byte sai nas escritas,
nem com uma escrita de 1 MB nem com escritas de 1 KB; toda a saída chega no close, em pedaços de `highWaterMark`:

| entrada, passo | pedaços (todos no close) |
|---|---|
| bin 1024 | 1028 |
| bin 65536 | 65536, 4 |
| bin 200000 | 65536 x3, 3397 |
| bin 300000 (passo 100000) | 65536 x4, 37861 |
| bin 262144 | 65536 x4, 5 |
| bin 524288 | 65536 x8, 5 |
| bin 1000000 (1 ou 1000000 de passo, ou 1024) | 65536 x15, 16965 |
| texto 1000000 | 65536, 13748 |
| texto 200000 em escritas de 1000 | 16842 |
| bin 1000000 com highWaterMark 1000 | 1000 repetido, o último com o resto |
| bin 1000000 com highWaterMark 1000000 | 1000000, 5 |

Conclusão: acumular até o `flush` (o que o porte faz) está certo; o que falta é o tamanho dos pedaços do close seguir o
`highWaterMark` do construtor (hoje `OUTPUT_PIECE` fixo de 65536). O erro do codificador brotli não ocorre com entrada
válida, então o item "erro no meio de uma escrita grande" não se aplica à compressão de brotli (só aos
decodificadores, onde `drive` já entrega os pedaços de `piece_of` antes do erro, ver pendências).

## Pendências

- `BrotliState::new` da crate liga `large_window`; conferir no bun se uma janela grande (`lgwin` > 24) é aceita.
- Erro no meio de uma escrita grande: o bun já entregou os pedaços cheios dos passos anteriores; o porte descarta tudo.
- `highWaterMark` do segundo argumento do construtor (`strategy`) ainda não é lido: o porte usa 65536 fixo.
- Binário 131073 em processo novo e a conferência executada de `pacing.rs` dependem de rodar o cargo.


## Zstd comprimindo: vetores grandes e streaming (bun 1.4.2)

Geradores no script de medição: texto `gen(n, semente)` (palavras `alpha `, `beta `, `gamma `, `delta\n`, `epsilon `, `zeta `
escolhidas por `(s >>> 24) % 6`, congruencial `s = imul(s, 1103515245) + 12345`) e binário `bin(n, semente)` (byte = `s >>> 24`).

| entrada | saída (bytes) | SHA-256 da saída |
|---|---|---|
| texto 300000, semente 1 | 42800 | `648a8fe20bd8eb71c0d87c1ec0e04466cf04cdf1b1ffee23adb499b28b86729d` |
| texto 1 MiB, semente 2 | 149402 | `bd07fe953c76f8883d216cc5c9b3a1c1d6e3e2d2340c33405c5830cf0fc5a8fa` |
| binário 1 MiB, semente 3 | 1048609 | `0634a6fb0f53410efeb7a0fb69c37a4f457fc38512b2ccfa7da99a109753dde5` |
| texto 3 MiB, semente 4 | 447437 | `4cb71f3eb728ce996522330042b88c6740b7c8ff4b338e54389fe81734645b65` |

- Os BYTES não dependem de como a entrada foi repartida em escritas (uma só, 1000, 200000, 65536+1+7000): o SHA-256 é o
  mesmo. Por isso a ligação em `compression_streams.rs` comprime a entrada acumulada de uma vez com `runtime::zstd`.
- O que muda com a repartição é o TEMPO da saída (`ZSTD_e_continue`): cada escrita que completa blocos de 128 KiB de
  entrada já entrega os blocos comprimidos (texto 300 KB em escritas de 1000: pedaços 18749, 18640, 5411; uma escrita só:
  37389 e 5411 no fim). Texto 1 MiB numa escrita só: 131072 e 18330. Binário 1 MiB em escritas de 1000: 65536, 65536, 9,
  repetindo (blocos raw de 128 KiB + 3 de cabeçalho, em pedaços de 64 KiB da leitura).
- Gerador conferido em 2026-10-09: o laço em JavaScript (`Math.imul`, `>>> 0`) reproduziu os quatro tamanhos e SHA-256 da
  tabela acima; o teste Rust (`stream.rs`, `text` e `binary`) documenta o mesmo laço.
- Medido (uma escrita de `n` bytes, depois close; `w:` sai na escrita, `end:` no close; texto semente 2, binário semente 3):
  - texto 131072: `w:18757 end:3`; 131073: `w:18757 end:4`; 262144: `w:32768 end:4663`; 524288: `w:65536 end:9250`;
    786432: `w:112062 end:3`; 1048576: `w:131072 end:18330`; 1048577: `w:149399 end:4`; 3145728: `w:447824 end:3`;
  - binário 131072: `w:65536 end:65536 end:12`; 262144: `w:262144 end:15`; 1000000: `w:917531 end:65536 end:16963`;
    1048576: `w:1048576 end:33`; 3145728: `w:3145728 end:81`;
  - escritas de 1000 bytes, binário: cada 128 KiB completado solta 65536, 65536 e 9 (o primeiro; depois 3) na escrita.
  - Regra do tamanho dos pedaços FECHADA (2026-10-09), derivada do código do bun
    (`src/runtime/webcore/CompressionStreamCoder.rs`, `JSCompressionStreamShared.cpp`) e do
    `ZSTD_compressStream_generic` do libzstd 1.5.7; implementada em `src/runtime/zstd/pacing.rs`:
    1. cada escrita é um passo com saída de no máximo `cap = max(65536, tamanho da escrita)`; ao chegar em `cap` o pedaço
       sai inteiro e vem outro passo (`more`); a saída de um passo é UM pedaço;
    2. cada volta chama `ZSTD_compressStream2` com espaço `min(cap - len, folga do Vec)`; a folga vem de
       `try_reserve(min(restante, 16 KiB))`, e o `Vec` dobra: os espaços são 16, 16, 32, 64, 128 KiB...;
    3. o `Vec` de escrita até 128 KiB é o rascunho da VM (a capacidade sobrevive entre passos, até 256 KiB); escrita acima
       de 128 KiB roda em outra thread com `Vec` novo (capacidade 0). Por isso as medições "aquecidas" (várias
       compressões no mesmo processo, as de cima) diferem das de processo novo: binário 131072 num processo novo
       dá `w:16384 end:65536 end:49164`, e `w:65536 end:65536 end:12` com o rascunho já em 64 KiB;
    4. o libzstd só comprime com 128 KiB carregados; se o espaço cabe `ZSTD_compressBound` o bloco vai direto, senão vai
       ao buffer interno e sai aos poucos; o que não coube fica retido até a próxima chamada (escrita ou close), e o
       passo de `e_continue` termina assim que toda a entrada foi consumida, mesmo com saída retida.
    Medições em processo novo (script `/tmp/zs_probe_a7f3.js`, `bun script <texto|bin> <n>`): binário 131072
    `w:16384 end:65536 end:49164`; 262144 `w:262144 end:15`; 65536 `end:65536 end:9`; 100000 `end:65536 end:34473`;
    131073 `w:131073 w:8 end:4`; 1000000 `w:917531 end:65536 end:16963`; texto 262144 `w:32768 end:4663`; 524288
    `w:65536 end:9250`; 786432 `w:112062 end:3`; 1048576 `w:131072 end:18330`; 100000 `end:14335`; 300000
    `w:37428 end:5425`. Conferidos À MÃO contra o modelo: binário 131072, 262144, 100000 e texto 262144; os demais
    ficam para a execução do teste.
    Os vetores de binário viraram o teste `fresh_process_binary_vectors` em `pacing.rs` (não rodado: sem cargo nesta fatia).
  - Gzip, deflate e brotli: o fonte mostra a mesma regra de `cap` (`max(highWaterMark, entrada)`) para todo motor, então
    `piece_of` virou `max(65536, tamanho da entrada)` para tudo menos o zstd comprimindo (que já sai repartido).
    Brotli comprimindo no bun real (`BrotliEncoderCompressStream`) NÃO foi lido nesta fatia: o porte acumula tudo até o
    `flush` (conferido pelas medições), e o encoder real pode soltar saída antes em escritas grandes.


## highWaterMark do construtor e erro no meio de uma escrita do decodificador (bun 1.4.2, 2026-10-09)
- O segundo argumento de `new CompressionStream(f, s)` e `new DecompressionStream(f, s)` segue `parseCodecHighWaterMark`
  (`JSCompressionStreamShared.cpp`): `convertQueuingStrategyDict` e `extractHighWaterMark` com padrão 65536. Medido:
  `undefined`, `null`, `{}`, `{highWaterMark: undefined}` valem 65536; `0`, `null`, `'5'`, `true`, `{valueOf}` convertem
  com `ToNumber` e passam; `-1`, `NaN`, `-Infinity`, `'x'`, `{}` dão `RangeError: The queuing strategy's highWaterMark must be
  a non-negative, non-NaN number`; `Infinity` e `1e30` valem `usize::MAX` (nunca reparte); fracionário é truncado
  (`2.9` vira 2, `0.5` vira 0) e o mínimo é 1 (`CompressionStreamCoder__create` faz `.max(1)`). Estratégia que não é objeto
  (`1`, `'a'`, `true`, `Symbol`, `10n`) dá `TypeError: The queuing strategy must be an object`, função vale como objeto;
  `{highWaterMark: 10n}` dá `TypeError: Conversion from 'BigInt' to 'number' is not allowed.` e `Symbol()` dá `Cannot
  convert a symbol to a number`; getter que lança propaga o erro dele. A ordem é formato primeiro, depois o hwm.
- O teto de cada passo é `max(hwm, entrada)` na escrita e `hwm` no close, para todos os formatos, inclusive o zstd
  comprimindo (`PacedEncoder::new(hwm)` em `pacing.rs`; o `cap` de `step` usa `self.high_water_mark`). Gzip de 50000 bytes
  aleatórios: hwm 1 dá `49174,1,1,...` (865 pedaços), 10 dá `49174,10,...`, 40000 e `Infinity` dão `49174,864`, 2.9 dá 2.
- Erro no meio de uma escrita do decodificador: cada passo é enfileirado antes de o seguinte rodar, então os passos que
  já encheram o teto saem antes do erro e o que o passo que falhou produzia (menos que o teto) se perde. Gzip de 300000
  bytes com rodapé corrompido: hwm 1000 (teto 5697 = a entrada) entrega 52 pedaços de 5697 e depois o erro; teto de 1 MiB
  não entrega nenhum; erro logo no começo também não entrega. Implementado: `Decompressor::settle` guarda a saída parcial e
  `drive` enfileira `floor(parcial / teto)` pedaços cheios antes de lançar o erro.
- Não medido: zstd e brotli com erro depois de saída já produzida (o corrompimento medido cai antes da primeira saída:
  0 pedaços). No zstd o libzstd escreve o bloco antes de reconhecer o erro, então o tamanho exato do parcial pode diferir.
  Medir com um quadro de vários blocos corrompido no meio.
- Casos novos no fim de `scripts/gen-streams-golden.js` (antes de "Execução"), não regenerados no tsv.
