# Dados do Unicode Character Database

Arquivos oficiais do Unicode, sem alteração, lidos pelo módulo `unicodedata` do interpretador
(`src/modules/ucd.rs`). Eles são a fonte primária: nenhum dado vem de outra implementação.

- `unicode-15.1.0/`: a versão que o `unicodedata` do CPython 3.13 expõe (`unidata_version`).
  Origem: https://www.unicode.org/Public/15.1.0/ucd/ (`Unihan_NumericValues.txt` sai do
  `Unihan.zip` do mesmo diretório).
- `unicode-3.2.0/`: a versão de `unicodedata.ucd_3_2_0`, usada pelo `stringprep` e pelo codec
  `idna` (RFC 3454 e 3490). Origem: https://www.unicode.org/Public/3.2-Update/

Licença: Unicode License V3, cópia em `LICENSE` de cada diretório.

Para atualizar, baixe os mesmos arquivos da versão nova e confira com o oráculo da bancada
(`testbench/corpus/cases/python/unicodedata.toml`).

SHA-256 dos arquivos:

```
2fc713e6a31a87c4850a37fe2caffa4218180fadb5de86b43a143ddb4581fb86  unicode-15.1.0/UnicodeData.txt
b08191401dc125f4e84ef262a95754faae6b737c79538e17ea9664a63434e94e  unicode-15.1.0/EastAsianWidth.txt
59d2d9e3dfdf0a999cf9dae11d594f053631222679a2f5710315ea07f7fe82af  unicode-15.1.0/CompositionExclusions.txt
fbf0e640bab36e165c4da5b6a98bdd963fcb4f923b5097f26f6f7f18b9678698  unicode-15.1.0/NameAliases.txt
7700f03419912fc58c26962b6252e2fcac135240a26925440aff6a4c8b714795  unicode-15.1.0/NamedSequences.txt
e1254413a6d686eb473c85b0d14b1f7350eaa30858238b51ea01c7bc87afc472  unicode-15.1.0/Unihan_NumericValues.txt
5e444028b6e76d96f9dc509609c5e3222bf609056f35e5fcde7e6fb8a58cd446  unicode-3.2.0/UnicodeData-3.2.0.txt
ce19f35ffca911bf492aab6c0d3f6af3d1932f35d2064cf2fe14e10be29534cb  unicode-3.2.0/EastAsianWidth-3.2.0.txt
1d3a450d0f39902710df4972ac4a60ec31fbcb54ffd4d53cd812fc1200c732cb  unicode-3.2.0/CompositionExclusions-3.2.0.txt
```
