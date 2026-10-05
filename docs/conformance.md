# Conformidade do pseudo-linus

Gerado por `cargo run -p pl-conformance --release`: todos os casos da bancada com golden, rodados no kernel real com a tabela completa de programas (161 programas). Estrito: stdout, stderr, exit e arquivos iguais ao Debian 13; leniente: stderr pode divergir.

**Total: 5300/5507 estrito (96.2%), 5397/5507 leniente (98.0%).**

| Ferramenta | Casos | Estrito | Leniente | Sem suporte |
|---|---|---|---|---|
| archive | 343 | 97.7% | 97.7% | 0 |
| awk | 234 | 99.6% | 99.6% | 0 |
| bc | 118 | 100.0% | 100.0% | 0 |
| column | 129 | 100.0% | 100.0% | 0 |
| coreutils | 955 | 87.7% | 95.9% | 0 |
| csv | 36 | 41.7% | 41.7% | 21 |
| date | 264 | 90.9% | 92.0% | 0 |
| diff | 298 | 100.0% | 100.0% | 0 |
| envsubst | 22 | 100.0% | 100.0% | 0 |
| file | 89 | 100.0% | 100.0% | 0 |
| find | 52 | 94.2% | 100.0% | 0 |
| git | 15 | 0.0% | 0.0% | 0 |
| grep | 185 | 100.0% | 100.0% | 0 |
| hexdump | 184 | 100.0% | 100.0% | 0 |
| jq | 194 | 100.0% | 100.0% | 0 |
| patch | 210 | 99.5% | 100.0% | 0 |
| procps | 46 | 100.0% | 100.0% | 0 |
| regex | 1214 | 100.0% | 100.0% | 0 |
| sed | 172 | 100.0% | 100.0% | 0 |
| shell | 108 | 98.1% | 100.0% | 0 |
| smoke | 6 | 100.0% | 100.0% | 0 |
| sqlite | 142 | 98.6% | 98.6% | 0 |
| strings | 161 | 91.9% | 98.1% | 0 |
| tree | 80 | 100.0% | 100.0% | 0 |
| which | 26 | 100.0% | 100.0% | 0 |
| xargs | 28 | 100.0% | 100.0% | 0 |
| xxd | 91 | 100.0% | 100.0% | 0 |
| yq | 105 | 100.0% | 100.0% | 0 |
