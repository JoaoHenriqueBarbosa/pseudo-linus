//! O DevTools remoto do Chromium (`--remote-debugging-port`): o servidor HTTP dos comandos `/json/*`, o WebSocket
//! e o protocolo CDP sobre ele. O laço do navegador (`server`) fica na thread principal com os alvos `browser` e as
//! sessões; cada página roda numa thread própria (`target`), com o seu programa JavaScript, e conversa com o laço
//! por um par de pipes.

pub mod http;
pub mod json;
pub mod protocol;
pub mod registry;
pub mod server;
pub mod target;
pub mod websocket;

fn random_bytes<const N: usize>() -> [u8; N] {
    let mut bytes = [0u8; N];
    if let Some(sys) = sysabi::sys::try_current() {
        let _ = sys.getrandom(&mut bytes);
    }
    bytes
}

/// Os ids de alvo, sessão, contexto de navegação e carregador: 128 bits aleatórios em hexadecimal maiúsculo, como o
/// `base::UnguessableToken::ToString()`.
pub fn token() -> String {
    random_bytes::<16>().iter().map(|b| format!("{b:02X}")).collect()
}

/// O id do alvo `browser`: um UUID versão 4 em minúsculas (`base::Uuid::GenerateRandomV4`).
pub fn uuid() -> String {
    let mut bytes = random_bytes::<16>();
    bytes[6] = (bytes[6] & 0x0F) | 0x40;
    bytes[8] = (bytes[8] & 0x3F) | 0x80;
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!("{}-{}-{}-{}-{}", &hex[..8], &hex[8..12], &hex[12..16], &hex[16..20], &hex[20..])
}

/// Um `int64` aleatório como o V8 escreve (o id do isolate e as metades do `uniqueId` de contexto).
pub fn random_i64() -> i64 {
    i64::from_le_bytes(random_bytes::<8>())
}

/// O relógio monotônico em segundos, o dos `timestamp` de `Page.*` e `Network.*` (`base::TimeTicks`).
pub fn monotonic_seconds() -> f64 {
    ul_common::time::monotonic_nanos() as f64 / 1e9
}

/// A hora do sistema em milissegundos, a dos `timestamp` de `Runtime.*` (`base::Time::Now().InMillisecondsFSinceUnixEpoch`).
pub fn wall_millis() -> f64 {
    zjsc::wtf::date_math::current_time_in_nanoseconds() as f64 / 1e6
}

#[cfg(test)]
mod tests {
    #[test]
    fn id_formats() {
        let token = super::token();
        assert_eq!(token.len(), 32);
        assert!(token.bytes().all(|b| b.is_ascii_digit() || (b'A'..=b'F').contains(&b)));
        let uuid = super::uuid();
        assert_eq!(uuid.len(), 36);
        assert_eq!(&uuid[14..15], "4");
        assert!(uuid.bytes().all(|b| b == b'-' || b.is_ascii_digit() || (b'a'..=b'f').contains(&b)));
    }
}
