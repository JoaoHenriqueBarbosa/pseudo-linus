# Plano: ML-KEM, ML-DSA e SubtleCrypto.supports no zjsc

Medido no bun 1.4.2 (`bun`), em 2026-10-09, só medição e plano. Nada de `src/` foi alterado. Os scripts de medição estão em
`/tmp/pq_measure_*.js` (descartáveis); os casos viraram linhas novas em `scripts/gen-crypto-golden.js` (bloco "ML-KEM (FIPS 203) e
ML-DSA (FIPS 204)", antes do IIFE final). O golden `tests/golden/crypto_bun.tsv` NÃO foi regenerado: a geração foi conferida
só para `/tmp` (3596 linhas, nenhuma `<undefined>`).

Nota do integrador: `fn unported` (panic! LACUNA) em `src/runtime/crypto.rs` ainda é chamada em generateKey, importKey, encrypt e no
match perto da linha 2322. Panic é proibido. Ao portar, cada caminho restante devolve exatamente o que o bun devolve (tabelas abaixo)
e `unported` é removida. Não apagar as linhas vizinhas (o cabeçalho de `fn not_supported` já foi perdido uma vez).

## 1. Achados que mudam o escopo

1. **O bun 1.4.2 NÃO suporta ML-KEM-512.** Toda operação com `ML-KEM-512` (generateKey, importKey, encapsulate*, supports) responde como
   algoritmo desconhecido: `NotSupportedError|Unrecognized algorithm name|9|DOMException`, e `SubtleCrypto.supports(op, 'ML-KEM-512')` é
   `false` para todas as operações. Ou seja: o porte NÃO implementa o parâmetro 512, só trata o nome como inexistente. Só 768 e 1024.
2. ML-DSA tem os três (44, 65, 87).
3. `SubtleCrypto.supports` é **estático** (`SubtleCrypto.supports`, não está em `SubtleCrypto.prototype` nem em `crypto.subtle`),
   síncrono, devolve `boolean` (não promessa).
4. `crypto.subtle.getPublicKey(privateKey, usages)` existe e funciona para ML-KEM e ML-DSA (já existe no porte para outros
   algoritmos; só falta estender).
5. `encapsulateBits`/`decapsulateBits` existem como métodos de `SubtleCrypto.prototype`, junto com `encapsulateKey`/`decapsulateKey`.
6. O `sign` do ML-DSA no bun é **hedged (aleatório)**: duas assinaturas da mesma mensagem com a mesma chave diferem. O `encapsulate`
   também é aleatório. Logo não há vetor de assinatura/ciphertext fixo: a validação contra o bun é "o porte verifica/decapsula o que
   o bun produziu" (vetores gerados na hora da geração do golden, ver seção 8). Determinísticos são: chave pública, spki, pkcs8, jwk
   a partir do `raw-seed`, e o `decapsulate` de um ciphertext qualquer (rejeição implícita).

## 2. Forma das funções (descritores)

Todas em `SubtleCrypto.prototype`, `writable: true, enumerable: true, configurable: true`:

| método | name | length |
|---|---|---|
| encapsulateBits | encapsulateBits | 2 |
| encapsulateKey | encapsulateKey | 5 |
| decapsulateBits | decapsulateBits | 3 |
| decapsulateKey | decapsulateKey | 6 |
| getPublicKey | getPublicKey | 2 |

Ordem de `Object.getOwnPropertyNames(SubtleCrypto.prototype)`: `constructor,encrypt,decrypt,sign,verify,digest,generateKey,deriveKey,
deriveBits,importKey,exportKey,wrapKey,unwrapKey,getPublicKey,encapsulateBits,encapsulateKey,decapsulateBits,decapsulateKey`.
`Object.getOwnPropertyNames(SubtleCrypto)` = `length,name,prototype,supports`. `SubtleCrypto.supports`: name `supports`, length 2,
`{writable:true, enumerable:true, configurable:true}`. `typeof crypto.subtle.supports` é `undefined`. `new SubtleCrypto()` lança
`TypeError|Illegal constructor|ERR_ILLEGAL_CONSTRUCTOR`.

Erros comuns (formato `nome|mensagem|code|classe`):

- argumentos faltando: `TypeError|Not enough arguments|ERR_MISSING_ARGS|TypeError` (encapsulateBits com 0 ou 1 arg, encapsulateKey com
  menos de 5, decapsulateBits com menos de 3, decapsulateKey com menos de 6, getPublicKey com 1, `supports` com 0 ou 1).
- `this` errado em `supports`: `TypeError|Value of "this" must be of type SubtleCrypto constructor|ERR_INVALID_THIS|TypeError` (com
  `call(undefined)`, `call({})`, `call(null)`, e extraída como `var f = SubtleCrypto.supports; f(...)`).

## 3. generateKey

Retorno: `{publicKey, privateKey}` (CryptoKeyPair), `publicKey.type = 'public'`, `privateKey.type = 'private'`, `algorithm = {name}` (só
`name`, sem outros campos; `{name:'ML-KEM-768', foo:1}` é aceito e a saída é igual), nome normalizado para a grafia canônica
(`ml-kem-768` vira `ML-KEM-768`).

`extractable` do `generateKey(n, e, usages)`: com `e = false` a medição devolveu `publicKey.extractable = true` e
`privateKey.extractable = false`, ou seja **a pública é sempre extraível** e a privada segue o argumento. Os usos:

ML-KEM-768 e ML-KEM-1024, `generateKey(n, true, [...])`:

| usos pedidos | publicKey.usages | privateKey.usages |
|---|---|---|
| encapsulateBits, decapsulateBits, encapsulateKey, decapsulateKey | `["encapsulateKey","encapsulateBits"]` | `["decapsulateKey","decapsulateBits"]` |
| só `decapsulateBits` | `[]` | `["decapsulateBits"]` |
| só `encapsulateBits` (ou só encapsulate*) | `SyntaxError\|Usages cannot be empty when creating a key.\|undefined\|SyntaxError` | |
| `[]` | `SyntaxError\|Usages cannot be empty when creating a key.` | |
| repetidos `[encapsulateBits, encapsulateBits]` | `SyntaxError\|Usages cannot be empty when creating a key.` | |
| contém `sign` | `SyntaxError\|Unsupported key usage for an ML-KEM-768 key\|undefined\|SyntaxError` (nome real na mensagem, `an ML-KEM-1024 key` no 1024) | |
| contém `foo` | `TypeError\|value must be enumeration (string)\|undefined\|TypeError` | |
| `generateKey(n, true)` sem usages | `TypeError\|Not enough arguments\|ERR_MISSING_ARGS\|TypeError` | |

Regra derivada: o conjunto de usos da privada é o que o chamador pediu entre `decapsulate*`; da pública é o que pediu entre
`encapsulate*`; **se a privada ficar vazia dá `Usages cannot be empty when creating a key.`**; se houver qualquer uso fora de
`{encapsulateBits, encapsulateKey, decapsulateBits, decapsulateKey}` dá `Unsupported key usage` (checado antes do vazio). A ordem
dos usos nas chaves é fixa: `encapsulateKey, encapsulateBits` e `decapsulateKey, decapsulateBits`.

ML-DSA-44/65/87:

| usos pedidos | publicKey.usages | privateKey.usages |
|---|---|---|
| `[sign, verify]` | `["verify"]` | `["sign"]` |
| só `sign` | `[]` | `["sign"]` |
| só `verify` | `SyntaxError\|Usages cannot be empty when creating a key.` | |
| `[]` | `SyntaxError\|Usages cannot be empty when creating a key.` | |
| `encapsulateBits` ou `[sign, encapsulateBits]` | `SyntaxError\|Unsupported key usage for an ML-DSA-44 key` | |

Nome inexistente (`ML-KEM-999`, `ML-KEM-512`): `NotSupportedError|Unrecognized algorithm name|9|DOMException`.

## 4. exportKey

Formatos para ML-KEM (768 e 1024) e ML-DSA (44, 65, 87). `raw`, `raw-secret` não existem. Tamanhos medidos:

| formato | chave | ML-KEM-768 | ML-KEM-1024 | ML-DSA-44 | ML-DSA-65 | ML-DSA-87 |
|---|---|---|---|---|---|---|
| raw-public | pública | 1184 | 1568 | 1312 | 1952 | 2592 |
| spki | pública | 1206 | 1590 | 1334 | 1974 | 2614 |
| raw-seed | privada | 64 | 64 | 32 | 32 | 32 |
| pkcs8 | privada | 86 | 86 | 54 | 54 | 54 |
| jwk pública `pub` (base64url) | pública | 1579 | 2091 | 1750 | 2603 | 3456 |
| jwk privada `priv` | privada | 86 | 86 | 43 | 43 | 43 |

Retorno é `ArrayBuffer` (jwk é objeto). Combinações inválidas:

- `NotSupportedError|Unable to export ML-KEM-768 private key using spki format|9|DOMException` (privada em `spki` ou `raw-public`).
- `NotSupportedError|Unable to export ML-KEM-768 public key using pkcs8 format|9|DOMException` (pública em `pkcs8` ou `raw-seed`).
- `raw` e `raw-secret`, pública ou privada: `Unable to export ML-KEM-768 public key using raw format` / `private key using raw-secret
  format`, mesma classe e code 9.
- a mensagem usa o nome da chave (`ML-DSA-65`) e a palavra `public` ou `private`.
- chave não extraível: `InvalidAccessError|key is not extractable|15|DOMException`. Vale para a privada gerada com `extractable
  false` ou importada com `false`; a pública do `generateKey` é sempre extraível; `getPublicKey` de privada não extraível devolve
  pública **extraível** (`[true,"public"]`).

### Estrutura DER (constante, só varia o OID e o tamanho)

OIDs (todos sob `2.16.840.1.101.3.4`; os 9 bytes finais `0609 6086480165030404` ou `...0403` etc.):

- ML-KEM-768 `...0402` (nist KEMs 4.2), ML-KEM-1024 `...0403`. (512 seria 0401, inexistente no bun.)
- ML-DSA-44 `...0311`, ML-DSA-65 `...0312`, ML-DSA-87 `...0313` (nist sigAlgs 3.17, 3.18, 3.19).

`pkcs8` (privada, só a semente, sem parâmetros no AlgorithmIdentifier): `30 LL 02 01 00 30 0b 06 09 <OID9> 04 NN 80 SS <seed>`.

- ML-KEM-768: `3054020100300b06096086480165030404020442 8040 <64 bytes>` (86 bytes no total).
- ML-KEM-1024: `3054020100300b06096086480165030404030442 8040 <64 bytes>`.
- ML-DSA-44: `3034020100300b0609608648016503040311 0422 8020 <32 bytes>` (54 bytes). 65 usa `...0312`, 87 `...0313`.

`spki`: `30 82 LLLL 30 0b 06 09 <OID9> 03 82 LLLL 00 <raw-public>`; prefixos medidos:

- ML-KEM-768: `308204b2300b0609608648016503040402038204a100` + 1184 bytes.
- ML-KEM-1024: `30820632300b06096086480165030404030382062100` (tag 03, comprimento 0x0621, byte 00) + 1568.
- ML-DSA-44: `30820532300b06096086480165030403110382052100` + 1312.
- ML-DSA-65: `308207b2300b0609608648016503040312038207a100` + 1952. ML-DSA-87: `30820a32300b060960864801650304031303820a2100` + 2592.

`jwk`: `kty: "AKP"` (Algorithm Key Pair, draft JOSE PQ), chaves em **ordem alfabética**: pública `{alg, ext, key_ops, kty, pub}`,
privada `{alg, ext, key_ops, kty, priv, pub}`. `alg` é o nome (`ML-KEM-768`), `ext` o `extractable`, `key_ops` os usos da chave
na ordem do objeto (`["encapsulateKey","encapsulateBits"]`, `["decapsulateBits"]` se importada só com esse, `["verify"]`, `["sign"]`),
`pub` é a chave pública bruta em base64url sem padding, `priv` é a **semente** em base64url (a rampa 0..63 dá
`AAECAwQFBgcICQoLDA0O...`; 0..31 dá `AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8`). Sem `use`. O SHA-256 do JSON inteiro por chave está
no golden.

## 5. importKey

Algoritmo aceito como string, como `{name}`, e em qualquer caixa (`ml-kem-768`); `algorithm` da chave sai canônico `{name}`.
Formatos: `raw-seed`, `raw-public`, `spki`, `pkcs8`, `jwk`. `raw` e `raw-secret`:
`NotSupportedError|Unable to import ML-KEM-768 using raw format|9|DOMException` (idem `raw-secret`).

Retorno `[type, extractable, algorithm, usages]`; o `extractable` é o argumento (sem forçar). Usos na chave importada: os pedidos,
**na ordem do pedido** (`["decapsulateKey","decapsulateBits"]` ficou assim quando pedido assim; `spki` com
`["encapsulateKey","encapsulateBits"]` voltou nessa ordem).

Regras (mesmas mensagens para 768 e 1024, trocando o nome; ML-DSA troca `ML-KEM` por `ML-DSA` e os usos válidos por `sign`/`verify`):

| entrada | resultado |
|---|---|
| raw-seed 64 bytes (32 no DSA), usos `[decapsulateBits]` (`[sign]`) | privada |
| raw-seed com usos `[]` | `SyntaxError\|Usages cannot be empty when importing a private key.\|undefined\|SyntaxError` |
| raw-seed/pkcs8 com `encapsulateBits` (`verify` no DSA) | `SyntaxError\|Unsupported key usage for a ML-KEM-768 key` (note o **`a`**, não `an`, na importação; na geração é `an`) |
| raw-public com `[encapsulateBits]`, `[]` | pública (usos vazios são aceitos na pública) |
| raw-public/spki com `decapsulateBits` | `SyntaxError\|Unsupported key usage for a ML-KEM-768 key` |
| spki com `[encapsulateKey, encapsulateBits]` ou `[]` | pública |
| pkcs8 com `[decapsulateKey, decapsulateBits]` | privada; com `[]` o erro de "private key" acima |
| tamanho errado (seed 63/65, raw-public 1183, spki/pkcs8 truncado ou trocado) | `DataError\|Invalid keyData\|0\|DOMException` |
| spki de outra família ou de outro parâmetro (DSA-65 spki como DSA-44, KEM-768 pkcs8 como 1024, KEM como DSA) | `DataError\|Invalid key type\|0\|DOMException` |
| jwk `{}` | `DataError\|Invalid keyData\|0\|DOMException` |

JWK (`kty: AKP`), ordem dos erros medida:

- sem `alg`: `DataError|Invalid keyData`. `alg` errado: `DataError|JWK "alg" Parameter and algorithm name mismatch|0`.
- sem `kty`: `Invalid keyData`. `kty: 'OKP'`: `DataError|Invalid JWK "kty" Parameter|0`.
- sem `pub`: `Invalid keyData`. `priv` inválida (`'AA'`): `Invalid keyData`. `priv` trocada por outra semente com `pub` original:
  `Invalid keyData` (o `pub` é checado contra a derivação da semente; ML-DSA com `pub` de outra chave: `Invalid keyData`).
- `ext: false` com `extractable: true`: `DataError|JWK "ext" Parameter and extractable mismatch|0`.
- `key_ops` que não contém os usos pedidos (`['sign']` num KEM, `[]`) ou jwk pública pedindo `decapsulateBits`:
  `DataError|Key operations and usage mismatch|0`.
- sem `priv` (jwk só pública, usos `decapsulateBits`): `SyntaxError|Unsupported key usage for a ML-KEM-768 key`
  (vira importação de pública e o uso é de privada).
- `use: 'enc'` é ignorado (importa normal).

## 6. getPublicKey(privateKey, usages)

Retorna a pública com `extractable: true` (mesmo se a privada era não extraível), `algorithm` igual, `usages` os pedidos:
`getPublicKey(pk, ['encapsulateBits'])` → `["encapsulateBits"]`; `[]` → `[]` (aceito); usos de privada (`['decapsulateBits']`,
`['sign']`) → `SyntaxError|Unsupported key usage for a ML-KEM-768 key`. Passar uma pública:
`InvalidAccessError|key must be a private key|15|DOMException`. `getPublicKey(pk)` com 1 arg: `Not enough arguments`.

## 7. Operações

### encapsulateBits(algorithm, encapsulationKey)

Retorno: objeto simples (`Object.prototype`) com as chaves **nesta ordem: `sharedKey`, `ciphertext`**, ambos `ArrayBuffer`;
`sharedKey` 32 bytes; `ciphertext` 1088 (768) ou 1568 (1024). Algoritmo como string ou `{name}`.

Erros (nesta ordem de avaliação, medida):

- `encapsulateBits()` ou 1 arg: `TypeError|Not enough arguments|ERR_MISSING_ARGS`.
- nome desconhecido (`'foo'`, `'AES-GCM'`, `'ML-KEM-512'`): `NotSupportedError|Unrecognized algorithm name|9|DOMException`.
- chave que não é CryptoKey (`{}`, `null`): `TypeError|Argument 2 ('encapsulationKey') to SubtleCrypto.encapsulateBits must be an instance of CryptoKey|ERR_INVALID_ARG_TYPE|TypeError`.
- chave sem o uso (privada, pública sem `encapsulateBits`, pública de usos `[]`):
  `InvalidAccessError|encapsulationKey does not have encapsulateBits usage|15|DOMException`.
- tamanho diferente (nome `ML-KEM-1024` com chave 768): `InvalidAccessError|key algorithm mismatch|15|DOMException` (note: em
  minúsculas, diferente do `Key algorithm mismatch` do `sign`).
- chave de ML-DSA com nome ML-KEM: `InvalidAccessError|key algorithm mismatch|15`.

### decapsulateBits(algorithm, decapsulationKey, ciphertext)

Retorna `ArrayBuffer` de 32 bytes. `ciphertext` aceita ArrayBuffer ou view (Uint8Array).

- ciphertext com tamanho errado (0, 10, +1 byte): `OperationError|The operation failed for an operation-specific reason|0|DOMException`.
- ciphertext do tamanho certo e adulterado: **sem erro**, devolve 32 bytes diferentes e determinísticos (rejeição implícita FIPS 203).
- ciphertext string ou `undefined`: `TypeError|Type error|undefined|TypeError`.
- chave sem uso: `InvalidAccessError|decapsulationKey does not have decapsulateBits usage|15|DOMException`.
- demais erros iguais aos do encapsulateBits (`key algorithm mismatch`, `Unrecognized algorithm name`, `Not enough arguments`).

### encapsulateKey(algorithm, encapsulationKey, sharedKeyAlgorithm, extractable, keyUsages)

Retorno: objeto com chaves na ordem **`ciphertext`, `sharedKey`** (invertida em relação ao encapsulateBits), `sharedKey` é CryptoKey
`secret`. Uso exigido na chave: `encapsulateKey` (`InvalidAccessError|encapsulationKey does not have encapsulateKey usage|15`).
Algoritmo resultante (os 32 bytes do segredo viram o material da chave; **o `length` pedido é ignorado**, sai sempre 256):

| sharedKeyAlgorithm | algorithm da chave | observação |
|---|---|---|
| `AES-GCM`, `{name:'AES-GCM',length:128}` (e 192, 100) | `{name:"AES-GCM",length:256}` | length ignorado, mesmo inválido |
| `AES-CBC` | `{name:"AES-CBC",length:256}` | idem AES-CTR, AES-KW (`{name,length:256}`) |
| `{name:'HMAC',hash:'SHA-256'}` | `{name:"HMAC",hash:{name:"SHA-256"},length:256}` | |
| `HKDF`, `PBKDF2` | `{name:"HKDF"}`, `{name:"PBKDF2"}`, usos `deriveBits` | extractable `false` obrigatório nesses (o bun aceitou com `false`) |
| `ChaCha20-Poly1305` | `{name:"ChaCha20-Poly1305"}` | |
| ECDSA etc. (não simétrico) | `NotSupportedError\|The algorithm is not supported\|9\|DOMException` | |
| nome desconhecido | `NotSupportedError\|Unrecognized algorithm name\|9` | |

Usos inválidos: `SyntaxError|Unsupported key usage for an AES key` (`['sign']` em AES-GCM); usos `[]`:
`SyntaxError|Usages cannot be empty when importing a secret key.`. 4 argumentos: `Not enough arguments`. A chave devolvida usa a mesma
validação do `importKey('raw-secret', ...)` desse algoritmo (reaproveitar o caminho que `unwrapKey('raw-secret')` já usa em crypto.rs).

### decapsulateKey(algorithm, decapsulationKey, ciphertext, sharedKeyAlgorithm, extractable, keyUsages)

Retorna só a CryptoKey `secret` (round-trip: `exportKey('raw', ...)` do `sharedKey` do encapsulate bate com o da decapsulate; `type`
`secret`, `extractable` o do argumento). Uso exigido: `decapsulateKey` (`InvalidAccessError|decapsulationKey does not have
decapsulateKey usage|15`; com a chave pública o mesmo erro). Ciphertext de tamanho errado: `OperationError|The operation failed for an
operation-specific reason|0`. 5 argumentos: `Not enough arguments`.

## 8. ML-DSA sign/verify

`sign(algorithm, privateKey, data)` → `ArrayBuffer` de 2420 (44), 3309 (65), 4627 (87) bytes. `data` ArrayBuffer ou view; string:
`TypeError|Type error|undefined|TypeError`. Algoritmo: string ou `{name, context?}`.

- `context`: `Uint8Array` de 0 a 255 bytes ok; 256 bytes: `OperationError|The operation failed for an operation-specific reason|0|DOMException`
  (no `sign` e no `verify`); `context: undefined` = vazio; `context: 'abc'` (string): `TypeError|Type error`.
- chave pública no `sign`: `InvalidAccessError|Unable to use this key to sign|15|DOMException`; privada no `verify`:
  `InvalidAccessError|Unable to use this key to verify|15`; pública importada com usos `[]` no `verify`: o mesmo erro do `verify`.
- nome diferente do da chave (`ML-DSA-65` com chave 44), `'Ed25519'`, `'ML-KEM-768'` sobre chave DSA: `InvalidAccessError|Key algorithm mismatch|15|DOMException`
  (maiúsculo em K, ao contrário do encapsulate).
- `sign(n, k)` sem data: `Not enough arguments`. `verify` com 3 args: igual.
- `verify` devolve `boolean`: assinatura válida `true`; mensagem diferente, assinatura adulterada (1 bit), tamanho errado (5, 0,
  +1) ou toda zero: `false` (nunca lança). Contexto diferente: `false`. `sig` string: `TypeError|Type error`.

## 9. SubtleCrypto.supports(operation, algorithm[, extra])

Estático e síncrono, devolve `boolean`. Nunca devolve promessa. Forma medida:

- 0 ou 1 argumento: `TypeError|Not enough arguments|ERR_MISSING_ARGS`. Primeiro argumento não-string, nome de operação em caixa
  errada (`'GENERATEKEY'`, `'generatekey'`) ou desconhecida (`'foo'`): `false`. Segundo argumento `null`, `undefined`, `{}`, `[]`,
  `5`, `{name:5}`: `false`; `Symbol()` lança `TypeError|Cannot convert a symbol to a string|undefined|TypeError`.
- O 2º argumento aceita string ou dicionário (normalizado como o do `generateKey`/`importKey`...). **Para operações que exigem
  parâmetros o string puro dá `false`**: ver tabela.
- Operações reconhecidas: `generateKey, importKey, exportKey, sign, verify, digest, encrypt, decrypt, deriveBits, deriveKey, wrapKey,
  unwrapKey, encapsulateBits, encapsulateKey, decapsulateBits, decapsulateKey, getPublicKey, get key length`.

### Resultado por algoritmo (operações `true`), forma string / forma dicionário completo

| algoritmo | string `{name}` | dicionário com os parâmetros usuais |
|---|---|---|
| ML-KEM-768, ML-KEM-1024 | generateKey, importKey, exportKey, encapsulateBits, decapsulateBits, getPublicKey | idem |
| ML-KEM-512, ML-KEM-999, foo | nenhuma | nenhuma |
| ML-DSA-44/65/87 | generateKey, importKey, exportKey, sign, verify, getPublicKey | idem |
| `ml-kem-768` (minúsculo) | igual ao 768 | |
| Ed25519 | generateKey, importKey, exportKey, sign, verify, getPublicKey | idem |
| X25519 | generateKey, importKey, exportKey, getPublicKey | idem |
| ChaCha20-Poly1305 | generateKey, importKey, exportKey | + encrypt, decrypt (dict com `iv`) |
| RSASSA-PKCS1-v1_5 | sign, verify | + generateKey, importKey, exportKey, getPublicKey (dict com hash, modulusLength, publicExponent) |
| RSA-PSS | nenhuma | generateKey, importKey, exportKey, sign, verify, getPublicKey |
| RSA-OAEP | encrypt, decrypt | + generateKey, importKey, exportKey, getPublicKey |
| ECDSA | nenhuma | generateKey, importKey, exportKey, sign, verify, getPublicKey (dict com namedCurve e hash) |
| ECDH | nenhuma | generateKey, importKey, exportKey, getPublicKey |
| AES-CTR, AES-KW | importKey, exportKey | + generateKey (dict com length) |
| AES-CBC, AES-GCM, AES-CFB-8 | importKey, exportKey | + generateKey, encrypt, decrypt (dict com length e iv) |
| HMAC | sign, verify | + generateKey, importKey, exportKey (dict com hash) |
| HKDF, PBKDF2 | importKey | importKey (dict) |
| SHA-1, SHA-256, SHA-384, SHA-512, SHA3-256 | digest | digest |
| cSHAKE128, Argon2id, KMAC128, AES-OCB, SLH-DSA-*, TurboSHAKE128, KT128 | nenhuma | nenhuma |

Observações de comportamento que a tabela esconde (todas no golden, `SUPPORT_CALLS`):

- `deriveBits`, `deriveKey`, `encapsulateKey`, `decapsulateKey`, `get key length`, `wrapKey`, `unwrapKey` **só aparecem como `true` com o
  terceiro argumento adequado** (ou nunca). `supports('deriveBits', HKDFdict, 256)` é `true`; sem o terceiro, ou com `0`, `7`, `9`, `null`, é `false`
  (`8`, `16`, `256`, `1048576` são `true`: múltiplo de 8 e maior que zero). `supports('deriveBits', 'X25519', 256)` e com ECDH:
  `false`. PBKDF2 com 256: `true`.
- `deriveKey`: HKDF dict + `{name:'AES-GCM',length:256}` é `true`; `length:100` é `false`; `'AES-GCM'` em string `false`; HMAC dict `true`;
  ML-KEM como alvo `false`; PBKDF2 + HMAC `true`; PBKDF2 + HKDF `false`; X25519 + AES-GCM `false`.
- `encapsulateKey`/`decapsulateKey` precisam do 3º argumento: `supports('encapsulateKey','ML-KEM-768')` (e com `undefined`/`null`) é
  `false`; com `'AES-GCM'`, `{name:'AES-GCM',length:256}`, **`{name:'AES-GCM',length:100}`** (!), `{name:'HMAC',hash:'SHA-256'}`,
  `'HKDF'` é `true`; com `ECDSA` ou `'foo'` `false`; com ML-KEM-512 ou ML-DSA no 2º argumento `false`.
- `get key length` e `wrapKey`/`unwrapKey` dão `false` em todas as combinações medidas (inclusive `AES-KW`).
- `supports('importKey', 'AES-GCM', 5)` e `(..., 'foo')`: `true` (3º argumento ignorado fora das operações acima);
  `supports('generateKey','ML-KEM-768', {})`, `5`, `null`: `true`.
- Dicionários incompletos: `generateKey` com `{name:'AES-GCM'}` (sem length), `{name:'HMAC'}` (sem hash), HMAC com `length:0`,
  `{name:'ECDSA'}` (sem curva), RSA sem `modulusLength`: `false`. `generateKey` AES-GCM `length:100`: **`true`** (não valida o valor), ECDSA
  `namedCurve:'P-1'`: **`true`** (não valida a curva).
- `sign`: `'ECDSA'` em string `false`, `{name:'ECDSA',hash:'SHA-256'}` `true`; `'HMAC'`, `'Ed25519'` string `true`; ML-DSA-65 com
  `context` de 300 bytes `true` (não valida), com `context: 'x'` (string) `false`.
- `digest`: `'sha-256'` (minúsculo) `true`; `{name:'SHA-256', foo:1}` `true`; `MD5` `false`; `cSHAKE128` com outputLength `false`;
  `SHA3-256` `true`.
- `encrypt`: AES-GCM em string `false`, em dicionário (com `iv`/`length`) `true`; RSA-OAEP string `true`.
- `exportKey` e `getPublicKey` em AES-GCM: `exportKey` `true`, `getPublicKey` `false`.

Esta função hoje não existe no porte. Implementar a tabela como um `match` sobre (operação, algoritmo normalizado, 3º argumento) com os
mesmos normalizadores do `generateKey`/`importKey` (se o normalizador falha, o resultado é `false` e nunca erro, exceto o `Symbol`).

## 10. Vetores determinísticos (registrar e conferir)

Semente = rampa `0,1,2,...` (64 bytes no ML-KEM, 32 no ML-DSA). Todos vindos de `importKey('raw-seed', ramp, n, true, [...])`.

| algoritmo | tamanho raw-public | SHA-256 do raw-public | SHA-256 do spki | SHA-256 do pkcs8 |
|---|---|---|---|---|
| ML-KEM-768 | 1184 | `0b7934c83125c788995e2ba6bd761e33046b3e40571be53e023309a29f398cc9` | `c23e23dd3d485a9256cda09358a4a286e00b373db10761eadf99f710649ca31c` | `67d4dc57d8f9e28816f4a3495abc48dd416b0da9bd1e1de4716ad0883b9ad6b8` |
| ML-KEM-1024 | 1568 | `c7b8fa0aa471d5ae18922d6ccad5b31e1d84f92ae723abfd13747018740a8530` | `d2b7480ae006a14b37c1e8a8e45ea39022e32a9b1a7b90d594c84d869c6ac420` | `2c19d9d89d8c36ffb9eab0f3bb18dd90a47aaca3653bede67953173514d61234` |
| ML-DSA-44 | 1312 | `9f107644c1084526af3bc8098680b05499a2325a644e388fb4f970e058d19d46` | `837832708c5236d951581f1fddf2b79991b3424a0486d16da1ddad0fd69701be` | `c823cb6a31172daa8af670a22c0f049af972bf1cb39a4a95971aa8c0c659dff4` |
| ML-DSA-65 | 1952 | `d666806e11cee19a7c989f7445f90dd419cf4d2d51db8c0fdb4c0f0a542238c9` | `b8b62131bfbe84433efb2273d7f5b87f7a22854a2cfd366fc2aead86d837c52d` | `af965903772933b6acc59764f335fcad9b5c61cdab2b368eabf224e7c29e31ac` |
| ML-DSA-87 | 2592 | `91dc389cfaa01470b7f66eee45a4ae9026d154817c754dfe22298b3fa241ffcd` | `07e57c4f14dbad1267f621ec3777b4e2e6c4fbc4c22fbb87510ff8e0b3c6a642` | `72cc4260a8d3d7622801ea98636123866d00e236d5f77221039c862325e02754` |

Primeiros 32 bytes do raw-public (rampa): ML-KEM-768 `298aa10d423c8dda069d02bc59e6cdf03a096b8b3da4cab9b80ca4a14907672c`, últimos 32
`5e43481c3eeb397eb192505229b67a201ea893c3e2cb32da8bc342fa4dea0578`; ML-KEM-1024 primeiros `4b94c29450111191823b3514c9ac1ea3d9825ccb86393a2dfb04654fa2192d37`,
últimos `44b6c66984a868aa92fa02227a086950eb0c8701ed58dc628776b983882e1175`; ML-DSA-44 primeiros `d7b2b47254aae0db45e7930d4a98d2c97d8f1397d1789dafa17024b316e9bec9`,
últimos `2202766151f16a965f9f81ece76cc070b55869e4db9784cf05c830b3242c8312`; ML-DSA-65 primeiros `48683d91978e31eb3dddb8b0473482d2b88a5f625949fd8f58a561e696bd4c27`,
últimos `ad18bd13fca55059dd9b185f79f9c47196a4e81b2104bc460a051e02f2e8444f`; ML-DSA-87 primeiros `9792bcec2f2430686a82fccf3c2f5ff665e771d7ab41b90258cfa7e90ec97124`,
últimos `caa4a9cbe885f786fa86e55be062222f8ba90a974073326b31212aece0a34a60`.

Semente toda zero: SHA-256 do raw-public ML-KEM-768 `f95c185fe5b2335d2fc938dd889c6425944acd74376b6952bf1130f720f6ba99` (primeiros 32
`254a797885c63b1440aa389c65340ef33520cc039aa8d749ae7095ba8485a244`), ML-KEM-1024 `29e3692e1c08422f548ca7e683e89015482c09a5442f8d2ead471c3931a5ee76`
(primeiros 32 `b1572c900b8b8202357437819c129e3cd66d21d7af55c5682b951deff475df1b`), ML-DSA-44 `eb4e7302842153b0fa19e8620739ad258af4929c26dd89079a7ec7d4282208e1`,
ML-DSA-65 `085ba380ff386dd52e42349c6eb88489d6058ea541a4e3fb0dce9a3fd1f7a911`, ML-DSA-87 `1d4a461707fc50a7ec93a9c02454778a8b82321ca460eea345e7bbfaff38a3aa`.

`decapsulateBits` de um ciphertext todo zero (rejeição implícita, determinístico, é o segredo derivado de `z` e do ciphertext):

- ML-KEM-768, semente rampa: `c8fbeddafdacef2ffeb8b354ea644f11b5c150f3e2c4a74ce38abba8f854ae16`; semente zero: `0419c6fa226891f68d792ae5e5a2929f2876ecf86dc7bb27ee0b3563320adb8f`.
- ML-KEM-1024, semente rampa: `294cb5b6b0f37a047745a2a69991f66ac85936e1b077e717eaeb26bb0ba42c68`; semente zero: `5643649eb481a76ef527ac38bfa5bd4baa060f7f302968705319f2689dda0801`.

`getPublicKey` e `importKey('raw-seed')` seguem FIPS 203 (`d` = bytes 0..31, `z` = bytes 32..63 da semente de 64 bytes) e FIPS 204 (`xi` = 32
bytes). Se o crate gerar a mesma chave pública que estes hashes, a conformidade da geração está provada; o `decapsulate` zero prova o PRF
implícito. Vetores válidos de ciphertext/assinatura (não determinísticos) são gerados pelo bun na hora e embutidos no golden por
`pqKnownAnswers` em `scripts/gen-crypto-golden.js`: o porte decapsula o `ciphertext` e tem de achar o `sharedKey` do bun; verifica a
assinatura (e a com `context [7,8]`) feita pelo bun.

## 11. Crates

Registry local (`~/.cargo/registry/src/*/`, 889 diretórios): **nenhum crate de ML-KEM nem de ML-DSA** (nem `ml-kem`, `ml-dsa`,
`pqcrypto*`, `libcrux*`, `fips203/204`, `oqs`, `kyber`, `dilithium`). Presentes só as primitivas vizinhas: `sha3-0.12.0`, `keccak-0.2.2`,
`tiny-keccak-2.0.2`, `hybrid-array-0.4.15`, `rand_core-0.10.1`, `digest-0.11.3`, `subtle-2.6.1`, `zeroize-1.9.1`, `signature-2.2.0`,
`pkcs8-0.10.2`, `spki-0.7.3`, `der-0.7.10`. No cache de `.crate` só `shake-0.1.0`.

Escolha (confirmada em crates.io em 2026-10-09, RustCrypto, Rust puro, `no_std`, MSRV 1.85):

- **`ml-kem` 0.3.2** (10 M de downloads; deps: `hybrid-array ^0.4.8`, `kem ^0.3`, `module-lattice ^0.2.3`, `rand_core ^0.10`,
  `sha3 ^0.11`; opcionais `zeroize`, `pkcs8`, `const-oid`). Expõe `MlKem768`, `MlKem1024`, `DecapsulationKey`, `EncapsulationKey`;
  feature `hazmat` dá `encapsulate_deterministic` (útil só para teste).
- **`ml-dsa` 0.1.1** (3,6 M de downloads; deps: `crypto-common ^0.2`, `ctutils ^0.4`, `hybrid-array ^0.4`, `module-lattice ^0.2.3`,
  `shake ^0.1`, `signature ^3`; opcionais `zeroize`, `pkcs8`, `const-oid`). Expõe `MlDsa44/65/87`, `SigningKey`, `VerifyingKey`.
- Features: **não** ligar `pkcs8`/`const-oid` (o DER do pkcs8/spki é fixo e simples, ver seção 4; fazer à mão em `crypto_pq.rs` evita a
  árvore `pkcs8 0.11`, e mantém o formato bit a bit igual ao do bun). Ligar `zeroize` só se o projeto já zera chaves.
- Dependência nova de rede: `cargo fetch` baixa ~10 crates (kem, module-lattice, ctutils, shake, signature 3, crypto-common 0.2, sha3 0.11 além
  do 0.12 local). Isso é ação do integrador (agentes não rodam cargo). `rand_core 0.10` já está no registry local; a geração aleatória
  deve usar semente de 64/32 bytes pedida ao mesmo gerador que `generateKey` já usa em `crypto_ec.rs` e as APIs de semente
  (`from_seed`/`FromSeed`/`KeyInit`), evitando ligar um `CryptoRng` 0.10 ao `rand` 0.8 do projeto.
- `#![forbid(unsafe_code)]` vale para o código do zjsc; os crates RustCrypto são dependências externas (como `sha2`, `rsa`,
  `ed25519-dalek` já usados). Conferir o `unsafe` deles uma vez com `grep -rn unsafe` no fonte baixado (a leitura do `lib.rs` mostra
  `#![no_std]`; o crate `ml-dsa` não declara `forbid(unsafe_code)` no topo).
- Alternativa descartada: escrever NTT/Keccak próprios (milhares de linhas, risco de erro numérico) ao lado de um crate auditado e com
  vetores NIST ACVP. Só cair nisso se o `cargo fetch` for impossível; nesse caso o plano é Keccak via `sha3 0.12` local mais NTT próprio,
  fatias bem maiores.

Pontos a confirmar na primeira compilação (a API não pôde ser lida offline): (a) construir a chave de decapsulação a partir da semente de 64
bytes (`from_seed` ou `DecapsulationKey::from_seed`) e extrair a semente de volta; (b) `encapsulate` com um RNG injetável (para usar o RNG
do porte) e `decapsulate` em forma que NÃO falhe com ciphertext adulterado (rejeição implícita); o crate tem tipo de ciphertext de
tamanho fixo, então o tamanho errado é checado ANTES (`OperationError`); (c) no ML-DSA, assinatura com `context` (`sign_with_context` ou
equivalente) e **modo hedged** (o bun assina aleatório; o crate tem variante determinística e a randomizada, usar a randomizada alimentada
com 32 bytes do RNG do porte); (d) o codificador de `raw-public` (`VerifyingKey::encode`) e `from_seed` (`xi`) do ML-DSA.

## 12. Fatias de 5 minutos (cada uma só com Read/Write/Edit; o integrador compila)

Arquivo novo `src/runtime/crypto_pq.rs` (nunca crypto.rs inteiro), e o integrador liga no `crypto.rs` ao final. Cada fatia lista o
critério de saída medido pelo golden.

1. **Cargo.toml**: acrescentar `ml-kem = { version = "0.3", default-features = false }` e `ml-dsa = { version = "0.1", default-features = false }`
   (integrador faz o fetch e confirma que compila só com `use ml_kem::MlKem768;`). Saída: `cargo check` verde.
2. **Tipos e tabelas** em `crypto_pq.rs`: `enum PqAlgorithm {MlKem768, MlKem1024, MlDsa44, MlDsa65, MlDsa87}`, nome canônico, OID
   de 9 bytes, tamanhos (seed, raw-public, ciphertext, assinatura), conjuntos de usos. Nome reconhecido sem caixa; `ML-KEM-512`
   fica FORA (cai em `Unrecognized algorithm name`). Teste unitário das tabelas da seção 4.
3. **DER à mão**: `encode_spki`, `decode_spki`, `encode_pkcs8`, `decode_pkcs8` (prefixos fixos, comprimento checado, erro
   `Invalid keyData` ou `Invalid key type` conforme a seção 5). Teste com os prefixos hex da seção 4.
4. **Chaves a partir da semente** (ML-KEM 64 bytes, ML-DSA 32): `public_from_seed`. Conferir os SHA-256 da seção 10 (rampa e zero).
5. **generateKey** (CryptoKeyPair): seleção de usos, `Usages cannot be empty when creating a key.`, `Unsupported key usage for an ML-KEM-768 key`
   (com `an`), pública sempre extraível, ordem fixa de usos. Golden: o programa de `generateKey` de cada nome.
6. **exportKey**: `raw-public`, `raw-seed`, `spki`, `pkcs8`, jwk `AKP` em base64url, mensagens `Unable to export ... using ... format`,
   `key is not extractable`. Golden: o programa de export (SHA-256 de cada formato).
7. **importKey** (`raw-seed`, `raw-public`, `spki`, `pkcs8`): validação de usos (`a ML-KEM-768 key`, com `a`), `Invalid keyData`, `Invalid key type`,
   `raw`/`raw-secret` não suportados.
8. **importKey jwk** + **getPublicKey**: ordem dos erros do jwk (`alg`, `kty`, `ext`, `key_ops`), checagem do `pub` contra a derivação da semente,
   `getPublicKey` com `extractable: true`, `key must be a private key`.
9. **ML-KEM encapsulateBits/decapsulateBits**: ordem das chaves do objeto (`sharedKey`, `ciphertext`), erros na ordem da seção 7, tamanho do
   ciphertext antes de decapsular, rejeição implícita (vetores zero da seção 10) e o `pqKnownAnswers` do golden.
10. **encapsulateKey/decapsulateKey**: ordem `ciphertext`, `sharedKey`, mapeamento de algoritmo da tabela (length ignorado, HKDF/PBKDF2 sem
    `length`, ChaCha), reutilizando a importação `raw-secret` simétrica existente.
11. **ML-DSA sign/verify** (hedged), `context` (0 a 255), erros `Unable to use this key to sign/verify`, `Key algorithm mismatch`, `verify` sempre `boolean`
    (nunca lança em tamanho ou assinatura ruim). Golden: o programa de sign/verify + `pqKnownAnswers`.
12. **SubtleCrypto.supports**: estático, tabela da seção 9, `Not enough arguments`, `ERR_INVALID_THIS`, `Symbol` lança. Golden: todas as linhas `SUPPORT_*`.
13. **Registro em SubtleCrypto.prototype** dos 4 métodos novos e de `supports` estático, com length/name/ordem de `getOwnPropertyNames` da seção 2;
    `new SubtleCrypto()` mantém `Illegal constructor`. Remover as chamadas a `unported` e `unported` em si; o `match` perto da linha 2322 do
    crypto.rs passa a despachar esses algoritmos (nenhum `panic!`).
14. **Conformidade**: regerar o golden (`bun scripts/gen-crypto-golden.js > tests/golden/crypto_bun.tsv`) e rodar `cargo test` com o
    filtro do crypto (integrador). Diferenças linha a linha viram correção; nunca exclusão de caso.

Dependência entre fatias: 2 → 3 → 4 → (5, 6, 7) → 8 → 9 → 10; 11 depende de 2, 4; 12 independente; 13 ao final; 14 fecha.
