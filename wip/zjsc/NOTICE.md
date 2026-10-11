# Avisos de terceiros

O `zjsc` embute código derivado do Bun (`oven-sh/bun`, tag `bun-v1.4.2`, commit
`744846f844374847c902b5e7fd59b4342a51ef99`), licenciado sob a MIT. O `LICENSE.md` desta pasta é o do upstream, copiado
sem alteração. O upstream não traz linha de copyright própria; o crédito é dos autores do Bun (`oven-sh/bun`).

| Arquivo | Origem no upstream |
|---|---|
| `src/api/bun_wrap.js` | `src/runtime.js`, copiado sem alteração (o módulo interno `bun:wrap`) |
| `src/api/node_events.js` | `src/js/node/events.ts`, portado para JavaScript comum |
| `src/api/node_async_hooks.js` | `src/js/node/async_hooks.ts`, portado para JavaScript comum |
| `src/api/node_timers.js` | `src/js/node/timers.ts`, portado para JavaScript comum |
| `src/api/node_timers_promises.js` | `src/js/node/timers.promises.ts`, portado para JavaScript comum |
| `src/api/node_querystring.js` | `src/js/node/querystring.ts`, portado para JavaScript comum |
| `src/api/node_stream_modules.js` | gerado por `scripts/gen-node-stream.js` a partir dos fontes do `stream` do upstream |
| `src/api/node_fs_streams.js` | gerado por `scripts/gen-node-fs-streams.js` a partir de `src/js/internal/fs/streams.ts` |
| `src/runtime/node_fs.js` | escrito para o porte sobre o comportamento medido do `node:fs` do bun; o `FileHandle` segue `src/js/node/fs.promises.ts` e o `cp` e o `glob` (quando entrarem) vêm de `src/js/internal/fs/` |
| `src/api/node_url.js` | gerado por `scripts/gen-node-url.js` a partir de `src/js/node/url.ts` e `src/js/internal/url.ts` |
| `src/api/node_util.js` | gerado por `scripts/gen-node-util.js` a partir de `src/js/node/util.ts` e dos internos que ele usa |
| `src/api/node_assert.js` | gerado por `scripts/gen-node-assert.js` a partir de `src/js/node/assert.ts` e dos internos que ele usa |
| `src/api/node_internal.js` | reescrita sobre o `internal/errors`, os validadores e o `internal/primordials` do upstream |

## JavaScriptCore (WebKit)

| Arquivo | Origem no upstream |
|---|---|
| `src/runtime/builtins_combined.js` | gerado por `scripts/gen-builtins.py` a partir de `derived/JavaScriptCore/JSCBuiltins.cpp`, a saída do gerador do próprio JavaScriptCore sobre `upstream/JavaScriptCore/builtins/*.js` (WebKit) |

Os fontes de `upstream/JavaScriptCore/builtins/*.js` trazem, cada um, o cabeçalho de licença BSD de duas cláusulas abaixo; o `builtins_combined.js` é a concatenação dos corpos das funções desses arquivos, e a licença continua valendo para ele. Os copyrights, como o upstream os traz:

- Copyright (C) 2014 Apple Inc. All rights reserved.
- Copyright (C) 2014-2017 Apple Inc. All rights reserved.
- Copyright (C) 2015 Apple Inc. All rights reserved.
- Copyright (C) 2015 Jordan Harband. All rights reserved.
- Copyright (C) 2015 Yusuke Suzuki <utatane.tea@gmail.com>.
- Copyright (C) 2015, 2016 Apple Inc. All rights reserved.
- Copyright (C) 2015-2016 Apple Inc. All rights reserved.
- Copyright (C) 2015-2016 Yusuke Suzuki <utatane.tea@gmail.com>.
- Copyright (C) 2015-2022 Apple Inc. All rights reserved.
- Copyright (C) 2016 Apple Inc. All rights reserved.
- Copyright (C) 2016 Oleksandr Skachkov <gskachkov@gmail.com>.
- Copyright (C) 2016 Yusuke Suzuki <utatane.tea@gmail.com>.
- Copyright (C) 2016-2017 Yusuke Suzuki <utatane.tea@gmail.com>.
- Copyright (C) 2016-2024 Apple Inc. All rights reserved.
- Copyright (C) 2018 Yusuke Suzuki <yusukesuzuki@slowstart.org>.
- Copyright (C) 2021 Igalia S.L.
- Copyright (C) 2022 Apple Inc. All rights reserved.
- Copyright (C) 2022 Jarred Sumner. All rights reserved.
- Copyright (C) 2023 Devin Rousso <webkit@devinrousso.com>.
- Copyright (C) 2024 Apple Inc. All rights reserved.
- Copyright (C) 2024 Sosuke Suzuki <aosukeke@gmail.com>.
- Copyright (C) 2025 Sosuke Suzuki <aosukeke@gmail.com>.

Condições, copiadas do cabeçalho de `upstream/JavaScriptCore/builtins/PromiseConstructor.js` sem alteração (um dos arquivos traz a variante "APPLE INC. AND ITS CONTRIBUTORS"):

```
Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions
are met:
1. Redistributions of source code must retain the above copyright
   notice, this list of conditions and the following disclaimer.
2. Redistributions in binary form must reproduce the above copyright
   notice, this list of conditions and the following disclaimer in the
   documentation and/or other materials provided with the distribution.

THIS SOFTWARE IS PROVIDED BY APPLE INC. ``AS IS'' AND ANY
EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR
PURPOSE ARE DISCLAIMED.  IN NO EVENT SHALL APPLE INC. OR
CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL,
EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO,
PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR
PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY
OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
(INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
 
```
