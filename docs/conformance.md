# Conformidade do pseudo-linus

Gerado por `cargo run -p pl-conformance --release`: todos os casos da bancada com golden, rodados no kernel real com a tabela completa de programas (59 programas). Estrito: stdout, stderr, exit e arquivos iguais ao Debian 13; leniente: stderr pode divergir.

**Total: 1382/1386 estrito (99.7%), 1382/1386 leniente (99.7%).**

| Ferramenta | Casos | Estrito | Leniente | Sem suporte |
|---|---|---|---|---|
| sed | 172 | 97.7% | 97.7% | 0 |
| regex | 1214 | 100.0% | 100.0% | 0 |
