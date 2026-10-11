# Pendências do WPT no ul-chromium (fim da rodada)

Medido contra o oráculo (Chromium 154). Nada aqui está commitado pela metade.

## Feito (commitado em wpt-fix)

- `:has()` e `:nth-child(of S)` (stylo vendorizado com `parse_has` e `parse_nth_child_of`).
- Instrução de processamento, CDATA e XMLDocument, sem vazar a marca de comentário.
- `document.compatMode` pelo doctype (10 casos do oráculo).
- Modo quirks no layout: folha de UA que zera a margem do primeiro e do último filho de body, td e th.

## Falta

1. getComputedStyle (756 subtestes): não iniciado. Começar pelas propriedades de maior contagem e medir o diretório inteiro antes e depois.
2. NOTRUN (1619): ver se um erro de infra do runner derruba muitos arquivos de uma vez.
3. assert_equals genérico (2388): amostrar por diretório.
4. Quirks, o que sobra: altura de linha de tabela e de imagem inline em BackCompat (o documento inteiro sai 2 px mais alto no paint_capture_quirks); unidades e seletores do modo quirks.
5. Parser xhtml/xml e `iframe.contentDocument` para /common/dummy.xml e .xhtml (489 `documentElement` de null): depende do iframe do fork do CDP (embed.rs), já integrado em fd147c2e; remedir.
6. `:dir()` (~150), `:heading()` (~250) e `.foo-*` (15): faltam no enum `NonTSPseudoClass` do stylo servo e no blitz-dom.
7. `DOM.getDocument`, `DOM.querySelector` e `DOM.getOuterHTML` no CDP: não existem; fatia própria.
