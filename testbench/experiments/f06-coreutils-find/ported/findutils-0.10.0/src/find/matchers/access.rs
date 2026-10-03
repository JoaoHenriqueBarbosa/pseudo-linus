// Copyright 2022 Tavian Barnes
//
// Use of this source code is governed by a MIT-style
// license that can be found in the LICENSE file or at
// https://opensource.org/licenses/MIT.

use super::{Matcher, MatcherIO, WalkEntry};

/// Porte pseudo-linus: a crate `faccess` chama `faccessat(2)` no FS do host. Aqui a regra é a do
/// kernel pro root (que é quem roda todo pseudo-processo da bancada): lê e escreve qualquer coisa
/// que exista; executa se algum bit x estiver ligado ou se for diretório.
trait PathExt {
    fn readable(&self) -> bool;
    fn writable(&self) -> bool;
    fn executable(&self) -> bool;
}

impl PathExt for std::path::Path {
    fn readable(&self) -> bool {
        sysio::fs::exists(self)
    }
    fn writable(&self) -> bool {
        sysio::fs::exists(self)
    }
    fn executable(&self) -> bool {
        use sysio::os::unix::fs::PermissionsExt;
        sysio::fs::metadata(self).is_ok_and(|m| m.is_dir() || m.permissions().mode() & 0o111 != 0)
    }
}

/// Matcher for -{read,writ,execut}able.
pub enum AccessMatcher {
    Readable,
    Writable,
    Executable,
}

impl Matcher for AccessMatcher {
    fn matches(&self, file_info: &WalkEntry, _: &mut MatcherIO) -> bool {
        let path = file_info.path();

        match self {
            Self::Readable => path.readable(),
            Self::Writable => path.writable(),
            Self::Executable => path.executable(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::find::matchers::tests::get_dir_entry_for;
    use crate::find::tests::FakeDependencies;

    #[test]
    fn access_matcher() {
        let file_info = get_dir_entry_for("test_data/simple", "abbbc");
        let deps = FakeDependencies::new();

        assert!(
            AccessMatcher::Readable.matches(&file_info, &mut deps.new_matcher_io()),
            "file should be readable"
        );

        assert!(
            AccessMatcher::Writable.matches(&file_info, &mut deps.new_matcher_io()),
            "file should be writable"
        );

        #[cfg(unix)]
        assert!(
            !AccessMatcher::Executable.matches(&file_info, &mut deps.new_matcher_io()),
            "file should not be executable"
        );
    }
}
