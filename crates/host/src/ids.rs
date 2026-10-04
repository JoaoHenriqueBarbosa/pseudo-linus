//! Identificadores aleatórios (`sb_…`, `ss_…`, `sn_…`), com 80 bits do `getrandom`.

/// `prefixo_` seguido de 20 dígitos hexadecimais.
pub fn random_id(prefix: &str) -> String {
    let mut b = [0u8; 10];
    // getrandom só falha sem fonte de entropia no kernel, e aí nada do daemon é seguro: melhor parar.
    getrandom::fill(&mut b).expect("getrandom sem entropia");
    format!("{prefix}_{}", hex::encode(b))
}

/// Confere o formato de um id recebido do cliente (evita caminho arbitrário em nomes de arquivo).
pub fn valid_id(prefix: &str, id: &str) -> bool {
    id.strip_prefix(prefix)
        .and_then(|r| r.strip_prefix('_'))
        .is_some_and(|h| h.len() == 20 && h.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
}

#[cfg(test)]
mod tests {
    #[test]
    fn ids() {
        let a = super::random_id("sb");
        assert!(super::valid_id("sb", &a), "{a}");
        assert!(!super::valid_id("ss", &a));
        assert!(!super::valid_id("sb", "sb_../../etc"));
        assert_ne!(a, super::random_id("sb"));
    }
}
