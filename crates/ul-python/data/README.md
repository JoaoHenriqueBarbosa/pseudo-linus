# Dados do Unicode Character Database

Arquivos oficiais do Unicode, sem alteração, lidos pelo módulo `unicodedata` do interpretador
(`src/modules/ucd.rs`). Eles são a fonte primária: nenhum dado vem de outra implementação.

- `unicode-15.1.0/`: a versão que o `unicodedata` do CPython 3.13 expõe (`unidata_version`).
  Origem: https://www.unicode.org/Public/15.1.0/ucd/ (`Unihan_NumericValues.txt` sai do
  `Unihan.zip` do mesmo diretório).
- `unicode-3.2.0/`: a versão de `unicodedata.ucd_3_2_0`, usada pelo `stringprep` e pelo codec
  `idna` (RFC 3454 e 3490). Origem: https://www.unicode.org/Public/3.2-Update/

Do 15.1.0, `CaseFolding.txt`, `SpecialCasing.txt` e `DerivedCoreProperties.txt` alimentam os
mapeamentos de caixa e as propriedades Cased, Case_Ignorable, Lowercase, Uppercase, XID_Start e
XID_Continue do `str`; `PropList.txt` fica como referência (o `makeunicodedata.py` não o usa para isso).

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
4e55acfdc32825a22e87670e9056a3bf94ad7c5400065778e9e10f8314372bcf  unicode-15.1.0/CaseFolding.txt
55a477efd933a52cd27e6a9bf70265bb2d8814af31aab07767abc8eb421f27ef  unicode-15.1.0/SpecialCasing.txt
f55d0db69123431a7317868725b1fcbf1eab6b265d756d1bd7f0f6d9f9ee108b  unicode-15.1.0/DerivedCoreProperties.txt
5e444028b6e76d96f9dc509609c5e3222bf609056f35e5fcde7e6fb8a58cd446  unicode-3.2.0/UnicodeData-3.2.0.txt
ce19f35ffca911bf492aab6c0d3f6af3d1932f35d2064cf2fe14e10be29534cb  unicode-3.2.0/EastAsianWidth-3.2.0.txt
1d3a450d0f39902710df4972ac4a60ec31fbcb54ffd4d53cd812fc1200c732cb  unicode-3.2.0/CompositionExclusions-3.2.0.txt
```

# Docstrings dos módulos em C do CPython

`cpython-docs/runtime.tsv` tem as docstrings que o CPython 3.13 do Debian 13 expõe em tempo de
execução e que o fonte em Python do disco não dá: tudo dos módulos escritos em C (`sys`,
`itertools`, `_socket`, `_ssl`...) e, nos módulos com `.py`, o que vem de um acelerador em C
(`bisect.bisect` é do `_bisect`, `heapq.heappush` do `_heapq`). O `src/modules/cpydocs.rs` monta as
docstrings dos módulos embutidos a partir do `.py` do CPython na imagem e desta tabela, nunca do
fonte embutido.

A tabela é a única fonte de `__doc__` dos objetos nativos: além dos módulos, o módulo `builtins`
guarda os tipos embutidos e os métodos deles (`str.upper`, `int.__add__`, `dict.fromkeys`), com os
métodos especiais, e os tipos de `types` que o CPython chama pelo próprio nome (`function`,
`NoneType`...). Ela é gerada no oráculo da bancada por `cpython-docs/extract.py` (o comando está
no cabeçalho do script, a lista de módulos em `cpython-docs/modules.txt`); para atualizar, rode de
novo e confira o diff.
