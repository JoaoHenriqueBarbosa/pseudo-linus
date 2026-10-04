# Conformidade do pseudo-linus

Gerado por `cargo run -p pl-conformance --release`: todos os casos da bancada com golden, rodados no kernel real com a tabela completa de programas (58 programas). Estrito: stdout, stderr, exit e arquivos iguais ao Debian 13; leniente: stderr pode divergir.

**Total: 3228/5416 estrito (59.6%), 3258/5416 leniente (60.2%).**

| Ferramenta | Casos | Estrito | Leniente | Sem suporte |
|---|---|---|---|---|
| archive | 343 | 93.3% | 93.6% | 0 |
| awk | 234 | 98.7% | 98.7% | 0 |
| bc | 118 | 0.0% | 0.0% | 118 |
| column | 129 | 100.0% | 100.0% | 0 |
| coreutils | 955 | 12.0% | 12.1% | 806 |
| csv | 36 | 13.9% | 13.9% | 31 |
| date | 264 | 0.0% | 0.0% | 264 |
| diff | 298 | 100.0% | 100.0% | 0 |
| envsubst | 22 | 100.0% | 100.0% | 0 |
| file | 89 | 0.0% | 0.0% | 89 |
| find | 52 | 65.4% | 73.1% | 0 |
| git | 15 | 0.0% | 0.0% | 0 |
| grep | 185 | 100.0% | 100.0% | 0 |
| hexdump | 184 | 100.0% | 100.0% | 0 |
| jq | 194 | 3.6% | 7.7% | 0 |
| patch | 210 | 99.5% | 100.0% | 0 |
| procps | 46 | 100.0% | 100.0% | 0 |
| regex | 1214 | 80.4% | 80.4% | 238 |
| sed | 172 | 0.0% | 0.0% | 172 |
| shell | 108 | 66.7% | 71.3% | 0 |
| smoke | 6 | 16.7% | 16.7% | 3 |
| sqlite | 142 | 98.6% | 98.6% | 0 |
| strings | 161 | 91.9% | 98.1% | 0 |
| tree | 80 | 100.0% | 100.0% | 0 |
| which | 26 | 100.0% | 100.0% | 0 |
| xargs | 28 | 0.0% | 0.0% | 26 |
| xxd | 0 | 0.0% | 0.0% | 0 |
| yq | 105 | 0.0% | 0.0% | 105 |
