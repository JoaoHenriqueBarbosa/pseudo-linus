//! `hb-ot-shaper-hebrew.cc`: formas de apresentação para fontes antigas e a reordenação de marcas.

use crate::buffer::Buffer;
use crate::normalize::compose_unicode;

/// Formas com dagesh de U+05D0..U+05EA (zero onde não há forma codificada).
const DAGESH_FORMS: [u32; 0x05EA - 0x05D0 + 1] = [
    0xFB30, 0xFB31, 0xFB32, 0xFB33, 0xFB34, 0xFB35, 0xFB36, 0x0000, 0xFB38, 0xFB39, 0xFB3A, 0xFB3B, 0xFB3C, 0x0000,
    0xFB3E, 0x0000, 0xFB40, 0xFB41, 0x0000, 0xFB43, 0xFB44, 0x0000, 0xFB46, 0xFB47, 0xFB48, 0xFB49, 0xFB4A,
];

/// `compose_hebrew`: a composição Unicode e, sem `mark` no GPOS, as formas de apresentação
/// excluídas da normalização.
pub fn compose(a: u32, b: u32, has_gpos_mark: bool) -> Option<u32> {
    let found = compose_unicode(a, b);
    if found.is_some() || has_gpos_mark {
        return found;
    }
    let ab = match b {
        0x05B4 if a == 0x05D9 => 0xFB1D,
        0x05B7 if a == 0x05F2 => 0xFB1F,
        0x05B7 if a == 0x05D0 => 0xFB2E,
        0x05B8 if a == 0x05D0 => 0xFB2F,
        0x05B9 if a == 0x05D5 => 0xFB4B,
        0x05BC if (0x05D0..=0x05EA).contains(&a) => DAGESH_FORMS[(a - 0x05D0) as usize],
        0x05BC if a == 0xFB2A => 0xFB2C,
        0x05BC if a == 0xFB2B => 0xFB2D,
        0x05BF if a == 0x05D1 => 0xFB4C,
        0x05BF if a == 0x05DB => 0xFB4D,
        0x05BF if a == 0x05E4 => 0xFB4E,
        0x05C1 if a == 0x05E9 => 0xFB2A,
        0x05C1 if a == 0xFB49 => 0xFB2C,
        0x05C2 if a == 0x05E9 => 0xFB2B,
        0x05C2 if a == 0xFB49 => 0xFB2D,
        _ => 0,
    };
    (ab != 0).then_some(ab)
}

/// `reorder_marks_hebrew`: patah ou qamats, depois sheva ou hiriq, depois meteg ou marca
/// inferior; o meteg passa para antes do sheva/hiriq.
pub fn reorder_marks(buffer: &mut Buffer, start: usize, end: usize) {
    const CCC10: u8 = 22;
    const CCC14: u8 = 23;
    const CCC17: u8 = 20;
    const CCC18: u8 = 21;
    const CCC22: u8 = 25;
    const BELOW: u8 = 220;
    for i in start + 2..end {
        let c0 = buffer.info[i - 2].modified_combining_class();
        let c1 = buffer.info[i - 1].modified_combining_class();
        let c2 = buffer.info[i].modified_combining_class();
        if (c0 == CCC17 || c0 == CCC18) && (c1 == CCC10 || c1 == CCC14) && (c2 == CCC22 || c2 == BELOW) {
            buffer.merge_clusters(i - 1, i + 1);
            buffer.info.swap(i - 1, i);
            break;
        }
    }
}
