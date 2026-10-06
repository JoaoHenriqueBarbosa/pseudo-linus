//! Tabela do codec `cp437` (IBM PC original), usada por `bytes.decode` e `str.encode`: é a
//! codificação dos nomes de arquivo de um ZIP sem a marca UTF-8.

const HIGH: &str = "ÇüéâäàåçêëèïîìÄÅÉæÆôöòûùÿÖÜ¢£¥₧ƒáíóúñÑªº¿⌐¬½¼¡«»░▒▓│┤╡╢╖╕╣║╗╝╜╛┐└┴┬├─┼╞╟╚╔╩╦╠═╬╧╨╤╥╙╘╒╓╫╪┘┌█▄▌▐▀αßΓπΣσµτΦΘΩδ∞φε∩≡±≥≤⌠⌡÷≈°∙·√ⁿ²■\u{a0}";

pub fn decode_byte(b: u8) -> char {
    if b < 0x80 {
        char::from(b)
    } else {
        HIGH.chars().nth(usize::from(b - 0x80)).unwrap_or('\u{fffd}')
    }
}

pub fn encode_char(c: char) -> Option<u8> {
    if (c as u32) < 0x80 {
        return Some(c as u8);
    }
    HIGH.chars().position(|h| h == c).map(|i| 0x80 + i as u8)
}
