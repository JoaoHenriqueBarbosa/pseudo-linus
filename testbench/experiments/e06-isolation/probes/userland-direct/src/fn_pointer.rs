//! `std::fs::read` usado como valor (item de função guardado numa variável), sem chamada direta no
//! ponto em que o caminho aparece.

pub fn read_len(path: &str) -> usize {
    let reader = std::fs::read::<&str>;
    reader(path).map(|bytes| bytes.len()).unwrap_or(0)
}
