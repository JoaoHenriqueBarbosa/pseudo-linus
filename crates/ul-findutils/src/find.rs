//! Entrada do `find`: o fork do uutils findutils (`vendor/findutils`) dentro do `sysio::run`, que
//! abre o estado de userland do pseudo-processo e descarrega o stdout no fim.

use std::ffi::OsString;

pub(crate) fn main(_ctx: &mut sysabi::Ctx, args: &[OsString]) -> i32 {
    let args = args.to_vec();
    sysio::run(move || {
        // O parser do findutils trabalha com `&str`: argumento que não é UTF-8 válido chega com o
        // caractere de substituição (o GNU trata bytes; nomes assim são raros na linha de comando).
        let strs: Vec<String> = args.iter().map(|a| a.to_string_lossy().into_owned()).collect();
        let refs: Vec<&str> = strs.iter().map(String::as_str).collect();
        let deps = findutils::find::StandardDependencies::new();
        findutils::find::find_main(&refs, &deps)
    })
}
