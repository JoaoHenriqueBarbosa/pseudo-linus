//! Entrada de um utilitário do uutils portado: o que a macro `uucore::bin!` fazia num executável do
//! host (localização do utilitário), dentro do `sysio::run` (estado de userland do pseudo-processo,
//! descarga do stdout no fim).

use std::ffi::OsString;

/// Roda `main` (o `uumain` do utilitário) com os argumentos do processo.
pub(crate) fn run_uu(util: &str, args: &[OsString], main: impl FnOnce(std::vec::IntoIter<OsString>) -> i32) -> i32 {
    let args = args.to_vec();
    // O `bin!` do uucore carrega as mensagens do utilitário canônico: `[` é o `test`, e `dir` e
    // `vdir` são o `ls` (um só arquivo de mensagens pros três).
    let locale_util = match util {
        "[" => "test",
        "dir" | "vdir" => "ls",
        other => other,
    };
    sysio::run(move || {
        if let Err(err) = uucore::locale::setup_localization(locale_util) {
            // Mesma mensagem e código do `bin!` do uucore.
            match err {
                uucore::locale::LocalizationError::ParseResource { error: err_msg, snippet } => {
                    sysio::eprintln!("Localization parse error at {snippet}: {err_msg:?}");
                }
                other => sysio::eprintln!("Could not init the localization system: {other}"),
            }
            return 99;
        }
        main(args.into_iter())
    })
}

/// Declara a função de entrada (`sysabi::Main`) de um utilitário do uutils.
///
/// `uu_main!(cat_main, "cat", uu_cat);` gera `fn cat_main(ctx, args) -> i32`, que chama
/// `uu_cat::uumain` com a localização de `cat`.
macro_rules! uu_main {
    ($fn_name:ident, $util:literal, $krate:ident) => {
        fn $fn_name(_ctx: &mut sysabi::Ctx, args: &[std::ffi::OsString]) -> i32 {
            $crate::run::run_uu($util, args, |it| $krate::uumain(it))
        }
    };
}

pub(crate) use uu_main;
