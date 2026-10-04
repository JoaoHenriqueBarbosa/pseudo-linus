# Conformidade do pseudo-linus

Gerado por `cargo run -p pl-conformance --release`: todos os casos da bancada com golden, rodados no kernel real com a tabela completa de programas (159 programas). Estrito: stdout, stderr, exit e arquivos iguais ao Debian 13; leniente: stderr pode divergir.

**Total: 299/299 estrito (100.0%), 299/299 leniente (100.0%).**

| Ferramenta | Casos | Estrito | Leniente | Sem suporte |
|---|---|---|---|---|
| yq | 105 | 100.0% | 100.0% | 0 |
| jq | 194 | 100.0% | 100.0% | 0 |
