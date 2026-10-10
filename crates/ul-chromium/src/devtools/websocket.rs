//! O enquadramento do WebSocket do lado do servidor (RFC 6455, como o `net::WebSocketEncoder` do Chromium sem
//! extensão): os quadros do cliente chegam mascarados, os do servidor saem sem máscara e inteiros, num quadro só.

/// O que um quadro completo do cliente pede.
#[derive(Debug, PartialEq, Eq)]
pub enum Incoming {
    /// Uma mensagem de texto inteira (fragmentos já juntados).
    Text(String),
    /// `ping` com a carga a devolver no `pong`.
    Ping(Vec<u8>),
    /// O cliente fechou, ou mandou o que o protocolo não aceita.
    Close,
}

/// O leitor de quadros de uma conexão: acumula fragmentos até o `FIN`.
#[derive(Default)]
pub struct Decoder {
    partial: Vec<u8>,
}

impl Decoder {
    /// Tira de `buffer` os quadros completos e devolve as mensagens; os bytes de um quadro pela metade ficam.
    pub fn feed(&mut self, buffer: &mut Vec<u8>) -> Vec<Incoming> {
        let mut out = Vec::new();
        let mut at = 0;
        while let Some((fin, opcode, payload, used)) = frame(&buffer[at..]) {
            at += used;
            match opcode {
                0x0 | 0x1 => {
                    self.partial.extend_from_slice(&payload);
                    if fin {
                        let text = std::mem::take(&mut self.partial);
                        match String::from_utf8(text) {
                            Ok(text) => out.push(Incoming::Text(text)),
                            Err(_) => out.push(Incoming::Close),
                        }
                    }
                }
                0x9 => out.push(Incoming::Ping(payload)),
                0xA => {}
                _ => out.push(Incoming::Close),
            }
            if out.last() == Some(&Incoming::Close) {
                break;
            }
        }
        buffer.drain(..at);
        out
    }
}

/// Um quadro do começo de `bytes`: `(fin, opcode, carga desmascarada, bytes usados)`.
fn frame(bytes: &[u8]) -> Option<(bool, u8, Vec<u8>, usize)> {
    let first = *bytes.first()?;
    let second = *bytes.get(1)?;
    let (length, mut at) = match second & 0x7F {
        126 => (u16::from_be_bytes(bytes.get(2..4)?.try_into().ok()?) as usize, 4),
        127 => (u64::from_be_bytes(bytes.get(2..10)?.try_into().ok()?) as usize, 10),
        n => (n as usize, 2),
    };
    let mask: Option<[u8; 4]> = if second & 0x80 != 0 {
        let mask = bytes.get(at..at + 4)?.try_into().ok()?;
        at += 4;
        Some(mask)
    } else {
        None
    };
    let mut payload = bytes.get(at..at.checked_add(length)?)?.to_vec();
    if let Some(mask) = mask {
        payload.iter_mut().enumerate().for_each(|(i, byte)| *byte ^= mask[i % 4]);
    }
    Some((first & 0x80 != 0, first & 0x0F, payload, at + length))
}

/// Um quadro do servidor: `opcode` com a carga inteira, sem máscara.
pub fn encode(opcode: u8, payload: &[u8]) -> Vec<u8> {
    let mut out = vec![0x80 | opcode];
    match payload.len() {
        n if n < 126 => out.push(n as u8),
        n if n <= u16::MAX as usize => {
            out.push(126);
            out.extend_from_slice(&(n as u16).to_be_bytes());
        }
        n => {
            out.push(127);
            out.extend_from_slice(&(n as u64).to_be_bytes());
        }
    }
    out.extend_from_slice(payload);
    out
}

pub const TEXT: u8 = 0x1;
pub const CLOSE: u8 = 0x8;
pub const PONG: u8 = 0xA;

#[cfg(test)]
mod tests {
    use super::*;

    fn masked(fin: bool, opcode: u8, payload: &[u8]) -> Vec<u8> {
        let mask = [1u8, 2, 3, 4];
        let mut out = encode(opcode, payload);
        if !fin {
            out[0] &= 0x7F;
        }
        let header = out.len() - payload.len();
        out[1] |= 0x80;
        let mut body: Vec<u8> = out.split_off(header);
        body.iter_mut().enumerate().for_each(|(i, b)| *b ^= mask[i % 4]);
        out.extend_from_slice(&mask);
        out.extend(body);
        out
    }

    #[test]
    fn fragments_and_partial_frames() {
        let long = "x".repeat(70_000);
        let mut buffer = masked(false, TEXT, b"{\"id\":");
        buffer.extend(masked(true, 0, b"1}"));
        buffer.extend(masked(true, TEXT, long.as_bytes()));
        let tail = buffer.split_off(buffer.len() - 10);
        let mut decoder = Decoder::default();
        assert_eq!(decoder.feed(&mut buffer), vec![Incoming::Text("{\"id\":1}".into())]);
        buffer.extend(tail);
        assert_eq!(decoder.feed(&mut buffer), vec![Incoming::Text(long)]);
        assert!(buffer.is_empty());
        let mut ping = masked(true, 0x9, b"p");
        ping.extend(masked(true, CLOSE, b""));
        assert_eq!(decoder.feed(&mut ping), vec![Incoming::Ping(b"p".to_vec()), Incoming::Close]);
    }

    #[test]
    fn server_frames_unmasked() {
        assert_eq!(encode(TEXT, b"hi"), vec![0x81, 2, b'h', b'i']);
        assert_eq!(&encode(TEXT, &[0; 300])[..4], &[0x81, 126, 1, 44]);
        assert_eq!(&encode(TEXT, &[0; 70_000])[..10], &[0x81, 127, 0, 0, 0, 0, 0, 1, 0x11, 0x70]);
    }
}
