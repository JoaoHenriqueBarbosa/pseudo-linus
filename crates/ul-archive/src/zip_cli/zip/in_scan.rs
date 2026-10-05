//! Busca de assinaturas "PK??" num buffer (`find_next_signature` e `find_signature` do zipfile.c).

/// A próxima assinatura plausível a partir de `pos` (um "PK" seguido de dois bytes menores que 16);
/// avança `pos` para depois dela. Um "P" que aparece no meio de um começo falso reinicia a busca.
pub fn find_next_signature(data: &[u8], pos: &mut usize) -> Option<[u8; 4]> {
    let mut i = *pos;
    while i < data.len() {
        if data[i] != 0x50 {
            i += 1;
            continue;
        }
        // Achou um P.
        if i + 1 >= data.len() {
            break;
        }
        if data[i + 1] != 0x4b {
            i += 1;
            continue;
        }
        if i + 2 >= data.len() {
            break;
        }
        let m2 = data[i + 2];
        if m2 == 0x50 {
            i += 2;
            continue;
        } else if m2 >= 16 {
            i += 3;
            continue;
        }
        if i + 3 >= data.len() {
            break;
        }
        let m3 = data[i + 3];
        if m3 == 0x50 {
            i += 3;
            continue;
        } else if m3 >= 16 {
            i += 4;
            continue;
        }
        *pos = i + 4;
        return Some([0x50, 0x4b, m2, m3]);
    }
    *pos = data.len();
    None
}

/// `find_signature`: procura uma assinatura específica; deixa `pos` depois dela.
pub fn find_signature(data: &[u8], pos: &mut usize, sig: &[u8; 4]) -> bool {
    loop {
        match find_next_signature(data, pos) {
            None => return false,
            Some(s) => {
                if &s == sig {
                    return true;
                }
            }
        }
    }
}
