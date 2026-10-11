//! Golden do modelo de propriedades contra o JavaScriptCore do bun: `tests/golden/object_model_bun.tsv` sai de
//! `scripts/gen-object-model-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON) e o texto da variável
//! global `R` que ele grava (resultado serializado, ou `ERR Nome: mensagem`). Cobre `Object.defineProperty` e
//! `defineProperties` (todas as combinações de descritor, redefinição, mensagens de TypeError), arrays (`length`,
//! índices, limite 2**32-1), ordem de chaves, herança de getters e setters, put em frozen/sealed/non-extensible
//! (sloppy e strict), ciclos de protótipo, `arguments` mapeado, funções, bound functions e classes.
mod common;

const GOLDEN: &str = include_str!("golden/object_model_bun.tsv");
const PRELUDES: &str = include_str!("golden/object_model.preludes.json");

#[test]
fn object_model_matches_bun() {
    common::run_mapped_golden(GOLDEN, PRELUDES, 1500, "object_model_case.js");
}
