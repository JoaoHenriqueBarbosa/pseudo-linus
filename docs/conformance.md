# Conformidade do pseudo-linus

Gerado por `cargo run -p pl-conformance --release`: todos os casos da bancada com golden, rodados no kernel real com a tabela completa de programas (152 programas). Estrito: stdout, stderr, exit e arquivos iguais ao Debian 13; leniente: stderr pode divergir.

**Total: 4762/5416 estrito (87.9%), 4888/5416 leniente (90.3%).**

| Ferramenta | Casos | Estrito | Leniente | Sem suporte |
|---|---|---|---|---|
| archive | 343 | 94.5% | 94.8% | 0 |
| awk | 234 | 99.6% | 99.6% | 0 |
| bc | 118 | 0.0% | 0.0% | 118 |
| column | 129 | 100.0% | 100.0% | 0 |
| coreutils | 955 | 80.5% | 91.3% | 34 |
| csv | 36 | 41.7% | 41.7% | 21 |
| date | 264 | 90.5% | 92.0% | 0 |
| diff | 298 | 100.0% | 100.0% | 0 |
| envsubst | 22 | 100.0% | 100.0% | 0 |
| file | 89 | 0.0% | 0.0% | 89 |
| find | 52 | 69.2% | 75.0% | 0 |
| git | 15 | 0.0% | 0.0% | 0 |
| grep | 185 | 100.0% | 100.0% | 0 |
| hexdump | 184 | 100.0% | 100.0% | 0 |
| jq | 194 | 99.5% | 99.5% | 0 |
| patch | 210 | 99.5% | 100.0% | 0 |
| procps | 46 | 100.0% | 100.0% | 0 |
| regex | 1214 | 100.0% | 100.0% | 0 |
| sed | 172 | 100.0% | 100.0% | 0 |
| shell | 108 | 88.0% | 91.7% | 0 |
| smoke | 6 | 83.3% | 83.3% | 0 |
| sqlite | 142 | 98.6% | 98.6% | 0 |
| strings | 161 | 91.9% | 98.1% | 0 |
| tree | 80 | 100.0% | 100.0% | 0 |
| which | 26 | 100.0% | 100.0% | 0 |
| xargs | 28 | 0.0% | 0.0% | 26 |
| xxd | 0 | 0.0% | 0.0% | 0 |
| yq | 105 | 0.0% | 0.0% | 105 |
