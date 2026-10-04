# Conformidade do pseudo-linus

Gerado por `cargo run -p pl-conformance --release`: todos os casos da bancada com golden, rodados no kernel real com a tabela completa de programas (156 programas). Estrito: stdout, stderr, exit e arquivos iguais ao Debian 13; leniente: stderr pode divergir.

**Total: 1018/1177 estrito (86.5%), 1122/1177 leniente (95.3%).**

| Ferramenta | Casos | Estrito | Leniente | Sem suporte |
|---|---|---|---|---|
| coreutils | 955 | 83.4% | 94.2% | 6 |
| xargs | 28 | 100.0% | 100.0% | 0 |
| jq | 194 | 100.0% | 100.0% | 0 |
