# Conformidade do pseudo-linus

Gerado por `cargo run -p pl-conformance --release`: todos os casos da bancada com golden, rodados no kernel real com a tabela completa de programas (400 programas). Estrito: stdout, stderr, exit e arquivos iguais ao Debian 13; leniente: stderr pode divergir.

**Total: 9264/9507 estrito (97.4%), 9280/9507 leniente (97.6%).**

| Ferramenta | Casos | Estrito | Leniente | Sem suporte |
|---|---|---|---|---|
| archive | 435 | 98.9% | 98.9% | 0 |
| awk | 260 | 100.0% | 100.0% | 0 |
| bc | 144 | 100.0% | 100.0% | 0 |
| binutils | 204 | 64.7% | 64.7% | 0 |
| column | 129 | 100.0% | 100.0% | 0 |
| coreutils | 1065 | 99.2% | 99.2% | 0 |
| csv | 36 | 97.2% | 97.2% | 0 |
| curl | 1 | 100.0% | 100.0% | 0 |
| date | 264 | 100.0% | 100.0% | 0 |
| diff | 298 | 100.0% | 100.0% | 0 |
| dpkg | 210 | 50.0% | 55.7% | 0 |
| envsubst | 22 | 100.0% | 100.0% | 0 |
| faketime | 8 | 87.5% | 87.5% | 0 |
| file | 89 | 100.0% | 100.0% | 0 |
| find | 52 | 100.0% | 100.0% | 0 |
| git | 46 | 100.0% | 100.0% | 0 |
| glibc | 43 | 95.3% | 95.3% | 0 |
| grep | 185 | 100.0% | 100.0% | 0 |
| hexdump | 184 | 100.0% | 100.0% | 0 |
| iconv | 45 | 100.0% | 100.0% | 0 |
| initscripts | 9 | 100.0% | 100.0% | 0 |
| jq | 194 | 100.0% | 100.0% | 0 |
| less | 42 | 100.0% | 100.0% | 0 |
| libc | 354 | 100.0% | 100.0% | 0 |
| ncurses | 429 | 100.0% | 100.0% | 0 |
| patch | 210 | 100.0% | 100.0% | 0 |
| procps | 288 | 97.9% | 97.9% | 0 |
| python | 13 | 100.0% | 100.0% | 0 |
| regex | 1214 | 100.0% | 100.0% | 0 |
| scripts | 65 | 84.6% | 90.8% | 0 |
| sed | 172 | 100.0% | 100.0% | 0 |
| shadow | 41 | 100.0% | 100.0% | 0 |
| shell | 108 | 100.0% | 100.0% | 0 |
| smoke | 6 | 100.0% | 100.0% | 0 |
| sqlite | 142 | 100.0% | 100.0% | 0 |
| strings | 161 | 100.0% | 100.0% | 0 |
| tree | 80 | 100.0% | 100.0% | 0 |
| utillinux | 1970 | 98.5% | 98.5% | 21 |
| wget | 1 | 100.0% | 100.0% | 0 |
| which | 26 | 100.0% | 100.0% | 0 |
| xargs | 28 | 100.0% | 100.0% | 0 |
| xxd | 91 | 100.0% | 100.0% | 0 |
| yq | 105 | 100.0% | 100.0% | 0 |
| zdump | 18 | 100.0% | 100.0% | 0 |
| zic | 20 | 90.0% | 90.0% | 0 |
