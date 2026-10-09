//! `String.prototype.normalize` ponta a ponta. Os valores vêm do padrão Unicode (UAX #15); a conferência
//! contra o bun 1.4.2 fica com quem compila e roda.
use zjsc::api::eval::evaluate_script;

/// Roda um programa e devolve o valor de conclusão como booleano.
fn is_true(source: &str) -> bool {
    let value = evaluate_script(source).unwrap_or_else(|_| panic!("lançou exceção: {source}"));
    value.is_true()
}

#[test]
fn nfd_decomposes_precomposed() {
    assert!(is_true("'\\u00e9'.normalize('NFD').length === 2"));
    assert!(is_true("'\\u00e9'.normalize('NFD') === 'e\\u0301'"));
}

#[test]
fn nfc_composes_and_default_is_nfc() {
    assert!(is_true("'e\\u0301'.normalize('NFC') === '\\u00e9'"));
    assert!(is_true("'e\\u0301'.normalize() === '\\u00e9'"));
    assert!(is_true("'\\u00e9'.normalize('NFC') === '\\u00e9'"));
}

#[test]
fn compatibility_forms() {
    assert!(is_true("'\\ufb01'.normalize('NFKC') === 'fi'"));
    assert!(is_true("'\\ufb01'.normalize('NFKD') === 'fi'"));
    assert!(is_true("'\\ufb01'.normalize('NFC') === '\\ufb01'"));
    assert!(is_true("'\\u1e9b\\u0323'.normalize('NFC') === '\\u1e9b\\u0323'"));
    assert!(is_true("'\\u1e9b\\u0323'.normalize('NFD') === '\\u017f\\u0323\\u0307'"));
    assert!(is_true("'\\u1e9b\\u0323'.normalize('NFKC') === '\\u1e69'"));
    assert!(is_true("'\\u1e9b\\u0323'.normalize('NFKD') === 's\\u0323\\u0307'"));
}

#[test]
fn hangul_composition() {
    assert!(is_true("'\\uac01'.normalize('NFD') === '\\u1100\\u1161\\u11a8'"));
    assert!(is_true("'\\u1100\\u1161\\u11a8'.normalize('NFC') === '\\uac01'"));
}

#[test]
fn lone_surrogates_pass_through() {
    assert!(is_true("'\\ud800e\\u0301'.normalize('NFC') === '\\ud800\\u00e9'"));
    assert!(is_true("'\\udc00'.normalize('NFD') === '\\udc00'"));
}

#[test]
fn ascii_and_invalid_form() {
    assert!(is_true("'abc'.normalize('NFKD') === 'abc'"));
    assert!(is_true("try { 'a'.normalize('x'); false } catch (e) { e instanceof RangeError }"));
    assert!(is_true("try { 'a'.normalize('nfc'); false } catch (e) { e instanceof RangeError }"));
}
