# Conformidade do pseudo-linus

Gerado por `cargo run -p pl-conformance --release`: todos os casos da bancada com golden, rodados no kernel real com a tabela completa de programas (195 programas). Estrito: stdout, stderr, exit e arquivos iguais ao Debian 13; leniente: stderr pode divergir.

**Total: 7460/7483 estrito (99.7%), 7460/7483 leniente (99.7%).**

| Ferramenta | Casos | Estrito | Leniente | Sem suporte |
|---|---|---|---|---|
| archive | 350 | 100.0% | 100.0% | 0 |
| awk | 234 | 100.0% | 100.0% | 0 |
| bc | 118 | 100.0% | 100.0% | 0 |
| column | 129 | 100.0% | 100.0% | 0 |
| coreutils | 955 | 100.0% | 100.0% | 0 |
| csv | 36 | 41.7% | 41.7% | 21 |
| date | 264 | 100.0% | 100.0% | 0 |
| diff | 298 | 100.0% | 100.0% | 0 |
| envsubst | 22 | 100.0% | 100.0% | 0 |
| file | 89 | 100.0% | 100.0% | 0 |
| find | 52 | 100.0% | 100.0% | 0 |
| git | 28 | 100.0% | 100.0% | 0 |
| grep | 185 | 100.0% | 100.0% | 0 |
| hexdump | 184 | 100.0% | 100.0% | 0 |
| jq | 194 | 100.0% | 100.0% | 0 |
| libc | 354 | 100.0% | 100.0% | 0 |
| ncurses | 327 | 100.0% | 100.0% | 0 |
| patch | 210 | 100.0% | 100.0% | 0 |
| procps | 117 | 100.0% | 100.0% | 0 |
| regex | 1214 | 100.0% | 100.0% | 0 |
| sed | 172 | 100.0% | 100.0% | 0 |
| shell | 108 | 100.0% | 100.0% | 0 |
| smoke | 6 | 100.0% | 100.0% | 0 |
| sqlite | 142 | 98.6% | 98.6% | 0 |
| strings | 161 | 100.0% | 100.0% | 0 |
| tree | 80 | 100.0% | 100.0% | 0 |
| utillinux | 1204 | 100.0% | 100.0% | 0 |
| which | 26 | 100.0% | 100.0% | 0 |
| xargs | 28 | 100.0% | 100.0% | 0 |
| xxd | 91 | 100.0% | 100.0% | 0 |
| yq | 105 | 100.0% | 100.0% | 0 |
