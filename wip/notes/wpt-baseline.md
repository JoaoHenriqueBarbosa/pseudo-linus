# Baseline do web-platform-tests sobre o ul-chromium

Commit do WPT: `be3ccb802967a8de6f237796c4c4fbe0612ce679` (clone raso fora do repositório).
Base do ul-chromium: `4db26cb8` (branch `chromium-headless`). Meta do M5.4: acima de 90% no recorte.

## Resultado por diretório (por subteste)

| Diretório | Arquivos | Subtestes | Passou | % | Sem resultado |
|---|---|---|---|---|---|
| console | 7 | 24 | 22 | 91.7 | 1 |
| css/cssom | 262 | 3846 | 2291 | 59.6 | 11 |
| css/selectors | 223 | 4000 | 1723 | 43.1 | 11 |
| dom/collections | 11 | 55 | 30 | 54.5 | 0 |
| dom/events | 100 | 737 | 325 | 44.1 | 1 |
| dom/nodes | 370 | 4855 | 1751 | 36.1 | 31 |
| dom/ranges | 64 | 363 | 67 | 18.5 | 25 |
| dom/traversal | 18 | 59 | 12 | 20.3 | 3 |
| encoding | 148 | 1511 | 963 | 63.7 | 103 |
| fetch | 79 | 1053 | 654 | 62.1 | 0 |
| html/dom | 204 | 1365 | 516 | 37.8 | 14 |
| html/semantics/forms | 359 | 4685 | 1362 | 29.1 | 2 |
| html/syntax | 102 | 869 | 592 | 68.1 | 4 |
| html/webappapis | 16 | 25 | 22 | 88.0 | 2 |
| url | 33 | 3444 | 2847 | 82.7 | 5 |
| webstorage | 49 | 172 | 94 | 54.7 | 6 |
| **total** | 2045 | 27063 | 13271 | 49.0 | |


Arquivo que não reportou subteste (travou, estourou o relógio ou caiu) conta como uma unidade reprovada.

## Excluídos (1136):
- reftest (compara renderização): 538
- exige o wptserve (substituição .sub, pipes, handlers python): 304
- exige o WebDriver (testdriver): 288
- exige Worker, WebSocket ou Service Worker: 6

Causas: reftest compara pixels; `.sub`, pipes e handlers python precisam do wptserve; testdriver precisa de WebDriver;
Worker, WebSocket e Service Worker não existem no ul-chromium. Os `.any.js` rodam só na variante window.

## As 10 maiores causas de falha (mensagem normalizada)

```
  2388  FAIL: assert_equals: expected 'X' but got 'X'   (ex.: /dom/nodes/CharacterData-surrogates.html :: Text.replaceData() splitting and creating surrogate pairs)
  1619  NOTRUN:    (ex.: /dom/nodes/Document-characterSet-normalization-1.html :: Name "UTF-8" has label "unicode-1-1-utf-8" (characterSet))
  1008  FAIL: 'X' is not a valid selector.   (ex.: /dom/nodes/DocumentFragment-getElementById.html :: It must return null when there are no matches)
   756  FAIL: assert_equals: 'X' expected 'X' but got 'X'   (ex.: /css/cssom/getComputedStyle-insets-relative.html :: horizontal-tb ltr inside horizontal-tb ltr - Percentages are)
   634  FAIL: Cannot read properties of null (reading 'X')   (ex.: /dom/nodes/Attr-prefix-xhtml.xhtml :: Attr.prefix present)
   533  FAIL: Cannot read properties of undefined (reading 'X')   (ex.: /dom/nodes/Node-appendChild.html :: Appending a document)
   280  FAIL: assert_equals: input.value should be 'X' after change of state expected 'X' but got 'X'   (ex.: /html/semantics/forms/the-input-element/type-change-state-weekmonth.html :: change state from hidden to month)
   271  FAIL: assert_throws_dom: function 'X' did not throw   (ex.: /dom/nodes/CharacterData-deleteData.html :: Text.deleteData() out of bounds)
   256  TIMEOUT: Test timed out   (ex.: /dom/nodes/Comment-constructor.html :: new Comment() should get the correct ownerDocument across gl)
   203  FAIL: assert_equals: expected N but got N   (ex.: /dom/nodes/Element-removeAttribute.html :: removeAttribute should remove the first attribute, irrespect)
```

## Leitura

- Seletores: `'X' is not a valid selector` (1008) é o parser de seletores recusando sintaxe que o Chromium aceita.
- Nós: `Cannot read properties of undefined/null` (1167) vem de interfaces ausentes (`XMLDocument`, `ProcessingInstruction`, `CDATASection`, `HTMLMediaElement`) e de documentos XML (`.xhtml`, `createDocument`).
- Strings e `CharacterData`: `assert_equals` (2388) concentra surrogates, `replaceData` e comparação de valores.
- NOTRUN (1619): arquivos em que o primeiro erro derrubou o resto do teste.
- Timeouts (256) e arquivos sem resultado (98): testes de realm cruzado com iframe e laços longos.
- Formulários: `type-change-state` de week/month/color/selectionDirection (cerca de 500).

## Como reproduzir

Compilar `cargo build --example wpt_worker` em `crates/ul-chromium` (container `rust-mold`, `--memory 16g`, `CARGO_INCREMENTAL=0`) e rodar:

```
python3 crates/ul-chromium/scripts/wpt-run.py --wpt /wpt --worker /target/debug/examples/wpt_worker --out full.jsonl --jobs 12 --timeout 120
python3 crates/ul-chromium/scripts/wpt-run.py --summary full.jsonl
```

O worker serve a raiz do WPT em `http://web-platform.test:8000` (`https` e porta 8443 para `.https.`), usa um
`testharnessreport.js` próprio que imprime o JSON por subteste no console, e roda com tempo virtual de 75 s.
