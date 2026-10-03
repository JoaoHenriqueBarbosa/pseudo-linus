//! Entradas determinísticas da matriz de compressão: geradas aqui, sem ler nada do host.

/// Gerador xorshift64* semeado: mesma sequência em toda execução.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed.max(1))
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    pub fn below(&mut self, n: u64) -> u64 {
        self.next_u64() % n
    }
}

/// Uma entrada da matriz.
pub struct Input {
    pub name: &'static str,
    pub data: Vec<u8>,
}

pub const TEXT_LEN: usize = 1_000_000;
pub const BINARY_LEN: usize = 256 * 1024;
pub const RANDOM_LEN: usize = 64 * 1024;

/// empty, one, text (log de 1 MB), binary (registros de 256 KiB), random (64 KiB incompressível).
pub fn inputs() -> Vec<Input> {
    vec![
        Input { name: "empty", data: Vec::new() },
        Input { name: "one", data: b"a".to_vec() },
        Input { name: "text", data: log_text(TEXT_LEN) },
        Input { name: "binary", data: records(BINARY_LEN) },
        Input { name: "random", data: random_bytes(RANDOM_LEN, 0x5eed_0003) },
    ]
}

const WORDS: &[&str] = &[
    "request", "accepted", "user", "session", "cache", "miss", "hit", "timeout", "retry", "upstream",
    "connection", "closed", "opened", "bytes", "latency", "queue", "worker", "started", "finished",
    "error", "warning", "GET", "POST", "/api/v1/items", "/healthz", "/login", "token", "expired",
    "database", "query", "rows", "commit", "rollback", "lock", "acquired", "released", "shard", "replica",
];

const SERVICES: &[&str] = &["api", "auth", "billing", "search", "worker", "gateway"];
const LEVELS: &[&str] = &["INFO", "INFO", "INFO", "DEBUG", "WARN", "ERROR"];

/// Texto tipo log: timestamps crescentes, serviços, palavras de um vocabulário e números.
pub fn log_text(len: usize) -> Vec<u8> {
    let mut rng = Rng::new(0x5eed_0001);
    let mut out = Vec::with_capacity(len + 256);
    let mut ms: u64 = 0;
    while out.len() < len {
        ms += rng.below(900) + 1;
        let (h, m, s, frac) = (12 + ms / 3_600_000 % 12, ms / 60_000 % 60, ms / 1000 % 60, ms % 1000);
        let svc = SERVICES[rng.below(SERVICES.len() as u64) as usize];
        let lvl = LEVELS[rng.below(LEVELS.len() as u64) as usize];
        let mut line = format!(
            "2026-01-15T{h:02}:{m:02}:{s:02}.{frac:03}Z {lvl:<5} {svc}[{}]:",
            1000 + rng.below(64)
        );
        for _ in 0..(3 + rng.below(8)) {
            line.push(' ');
            line.push_str(WORDS[rng.below(WORDS.len() as u64) as usize]);
        }
        line.push_str(&format!(" id={} ms={}\n", rng.below(100_000), rng.below(5000)));
        out.extend_from_slice(line.as_bytes());
    }
    out.truncate(len);
    out
}

/// Registros binários de 32 bytes: id crescente, timestamp, valor, flags e um rótulo curto.
pub fn records(len: usize) -> Vec<u8> {
    let mut rng = Rng::new(0x5eed_0002);
    let mut out = Vec::with_capacity(len + 32);
    let mut id: u32 = 0;
    let mut ts: u64 = 1_768_478_400_000;
    while out.len() < len {
        id += 1;
        ts += rng.below(50);
        out.extend_from_slice(&id.to_le_bytes());
        out.extend_from_slice(&ts.to_le_bytes());
        let value = (rng.below(1_000_000) as f64 / 100.0).to_le_bytes();
        out.extend_from_slice(&value);
        out.extend_from_slice(&(rng.below(4) as u32).to_le_bytes());
        let label = SERVICES[rng.below(SERVICES.len() as u64) as usize].as_bytes();
        let mut tag = [0u8; 8];
        tag[..label.len().min(8)].copy_from_slice(&label[..label.len().min(8)]);
        out.extend_from_slice(&tag);
    }
    out.truncate(len);
    out
}

pub fn random_bytes(len: usize, seed: u64) -> Vec<u8> {
    let mut rng = Rng::new(seed);
    let mut out = Vec::with_capacity(len + 8);
    while out.len() < len {
        out.extend_from_slice(&rng.next_u64().to_le_bytes());
    }
    out.truncate(len);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inputs_are_deterministic_and_sized() {
        let a = inputs();
        let b = inputs();
        for (x, y) in a.iter().zip(&b) {
            assert_eq!(x.data, y.data, "{}", x.name);
        }
        assert_eq!(a[2].data.len(), TEXT_LEN);
        assert_eq!(a[3].data.len(), BINARY_LEN);
        assert_eq!(a[4].data.len(), RANDOM_LEN);
        assert!(a[2].data.ends_with(b"\n") || a[2].data.len() == TEXT_LEN);
    }
}
