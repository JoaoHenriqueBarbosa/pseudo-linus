# Conformidade do pseudo-linus

Gerado por `cargo run -p pl-conformance --release`: todos os casos da bancada com golden, rodados no kernel real com a tabela completa de programas (152 programas). Estrito: stdout, stderr, exit e arquivos iguais ao Debian 13; leniente: stderr pode divergir.

**Total: 867/1225 estrito (70.8%), 988/1225 leniente (80.7%).**

| Ferramenta | Casos | Estrito | Leniente | Sem suporte |
|---|---|---|---|---|
| coreutils | 955 | 80.3% | 91.2% | 34 |
| date | 264 | 36.0% | 42.4% | 0 |
| cat | 0 | 0.0% | 0.0% | 0 |
| head | 0 | 0.0% | 0.0% | 0 |
| ls | 0 | 0.0% | 0.0% | 0 |
| sort | 0 | 0.0% | 0.0% | 0 |
| tail | 0 | 0.0% | 0.0% | 0 |
| wc | 0 | 0.0% | 0.0% | 0 |
| smoke | 6 | 83.3% | 83.3% | 0 |
