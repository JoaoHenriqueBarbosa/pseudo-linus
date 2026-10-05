//! Geradores de dados do `zstd -b` sem arquivos: o lorem ipsum do `programs/lorem.c` (padrão) e o
//! gerador de compressibilidade do `programs/datagen.c` (`-P#`). Os dois são determinísticos, com o
//! mesmo gerador pseudoaleatório do C, então produzem os mesmos bytes.

const WORDS: [&str; 255] = [
    "lorem", "ipsum", "dolor", "sit", "amet", "consectetur", "adipiscing", "elit", "sed", "do", "eiusmod", "tempor",
    "incididunt", "ut", "labore", "et", "dolore", "magna", "aliqua", "dis", "lectus", "vestibulum", "mattis",
    "ullamcorper", "velit", "commodo", "a", "lacus", "arcu", "magnis", "parturient", "montes", "nascetur",
    "ridiculus", "mus", "mauris", "nulla", "malesuada", "pellentesque", "eget", "gravida", "in", "dictum", "non",
    "erat", "nam", "voluptat", "maecenas", "blandit", "aliquam", "etiam", "enim", "lobortis", "scelerisque",
    "fermentum", "dui", "faucibus", "ornare", "at", "elementum", "eu", "facilisis", "odio", "morbi", "quis", "eros",
    "donec", "ac", "orci", "purus", "turpis", "cursus", "leo", "vel", "porta", "consequat", "interdum", "varius",
    "vulputate", "aliquet", "pharetra", "nunc", "auctor", "urna", "id", "metus", "viverra", "nibh", "cras", "mi",
    "unde", "omnis", "iste", "natus", "error", "perspiciatis", "voluptatem", "accusantium", "doloremque",
    "laudantium", "totam", "rem", "aperiam", "eaque", "ipsa", "quae", "ab", "illo", "inventore", "veritatis",
    "quasi", "architecto", "beatae", "vitae", "dicta", "sunt", "explicabo", "nemo", "ipsam", "quia", "voluptas",
    "aspernatur", "aut", "odit", "fugit", "consequuntur", "magni", "dolores", "eos", "qui", "ratione", "sequi",
    "nesciunt", "neque", "porro", "quisquam", "est", "dolorem", "adipisci", "numquam", "eius", "modi", "tempora",
    "incidunt", "magnam", "quaerat", "ad", "minima", "veniam", "nostrum", "ullam", "corporis", "suscipit",
    "laboriosam", "nisi", "aliquid", "ex", "ea", "commodi", "consequatur", "autem", "eum", "iure", "voluptate",
    "esse", "quam", "nihil", "molestiae", "illum", "fugiat", "quo", "pariatur", "vero", "accusamus", "iusto",
    "dignissimos", "ducimus", "blanditiis", "praesentium", "voluptatum", "deleniti", "atque", "corrupti", "quos",
    "quas", "molestias", "excepturi", "sint", "occaecati", "cupiditate", "provident", "similique", "culpa",
    "officia", "deserunt", "mollitia", "animi", "laborum", "dolorum", "fuga", "harum", "quidem", "rerum", "facilis",
    "expedita", "distinctio", "libero", "tempore", "cum", "soluta", "nobis", "eligendi", "optio", "cumque",
    "impedit", "minus", "quod", "maxime", "placeat", "facere", "possimus", "assumenda", "repellendus", "temporibus",
    "quibusdam", "officiis", "debitis", "saepe", "eveniet", "voluptates", "repudiandae", "recusandae", "itaque",
    "earum", "hic", "tenetur", "sapiente", "delectus", "reiciendis", "cillum", "maiores", "alias", "perferendis",
    "doloribus", "asperiores", "repellat", "minim", "nostrud", "exercitation", "ullamco", "laboris", "aliquip",
    "duis", "aute", "irure",
];

const WEIGHTS: [usize; 6] = [0, 8, 6, 4, 3, 2];

/// O gerador comum aos dois arquivos do C (`LOREM_rand`/`RDG_rand` sem o deslocamento final).
fn step(seed: &mut u32) -> u32 {
    let r = seed.wrapping_mul(2_654_435_761) ^ 2_246_822_519;
    *seed = r.rotate_left(13);
    *seed
}

/// O estado do `lorem.c`.
struct Lorem {
    out: Vec<u8>,
    max: usize,
    seed: u32,
    distrib: Vec<usize>,
}

impl Lorem {
    fn rand(&mut self, range: u32) -> u32 {
        ((u64::from(step(&mut self.seed)) * u64::from(range)) >> 32) as u32
    }

    fn about(&mut self, target: u32) -> usize {
        (self.rand(target) + self.rand(target) + 1) as usize
    }

    fn write_last(&mut self) {
        let last = self.max - self.out.len();
        if last == 0 {
            return;
        }
        self.out.push(b'.');
        if last > 2 {
            self.out.resize(self.out.len() + last - 2, b' ');
        }
        if last > 1 {
            self.out.push(b'\n');
        }
    }

    fn word(&mut self, word: &str, sep: &str, upcase: bool) {
        if self.out.len() + word.len() + sep.len() > self.max {
            self.write_last();
            return;
        }
        let start = self.out.len();
        self.out.extend_from_slice(word.as_bytes());
        if upcase {
            self.out[start] = self.out[start].wrapping_sub(b'a' - b'A');
        }
        self.out.extend_from_slice(sep.as_bytes());
    }

    fn sentence(&mut self, nb_words: usize) {
        let comma = self.about(9);
        let comma2 = comma + self.about(7);
        let end = if self.rand(11) == 7 { "? " } else { ". " };
        for i in 0..nb_words {
            let r = self.rand(self.distrib.len() as u32) as usize;
            let id = self.distrib[r];
            let sep = if i == nb_words - 1 {
                end
            } else if i == comma || i == comma2 {
                ", "
            } else {
                " "
            };
            self.word(WORDS[id], sep, i == 0);
        }
    }

    fn paragraph(&mut self, nb: usize) {
        for _ in 0..nb {
            let n = self.about(11);
            self.sentence(n);
        }
        for _ in 0..2 {
            if self.out.len() < self.max {
                self.out.push(b'\n');
            }
        }
    }
}

/// `LOREM_genBuffer(size, seed)`.
pub fn lorem(size: usize, seed: u32) -> Vec<u8> {
    let mut distrib = Vec::new();
    for (w, word) in WORDS.iter().enumerate() {
        let n = WEIGHTS[word.len().min(WEIGHTS.len() - 1)];
        distrib.extend(std::iter::repeat_n(w, n));
    }
    let mut g = Lorem { out: Vec::with_capacity(size), max: size, seed, distrib };
    for (i, word) in WORDS.iter().enumerate().take(18) {
        let sep = if i == 4 || i == 7 { ", " } else { " " };
        g.word(word, sep, i == 0);
    }
    g.word(WORDS[18], ". ", false);
    while g.out.len() < g.max {
        let n = g.about(7);
        g.paragraph(n);
    }
    g.out
}

const LTSIZE: usize = 1 << 13;

fn rdg_rand(seed: &mut u32) -> u32 {
    step(seed) >> 5
}

fn rdg_rand_length(seed: &mut u32) -> u32 {
    if rdg_rand(seed) & 7 != 0 {
        return rdg_rand(seed) & 0xF;
    }
    (rdg_rand(seed) & 0x1FF) + 0xF
}

/// `RDG_genBuffer(size, matchProba, litProba, seed)`.
pub fn datagen(size: usize, match_proba: f64, lit_proba: f64, seed: u32) -> Vec<u8> {
    let mut seed = seed;
    let lit = if lit_proba <= 0.0 { match_proba / 4.5 } else { lit_proba };
    let ld = (lit * 256.0 + 0.001) as u32;
    // `RDG_fillLiteralDistrib`.
    let mut ldt = [b'0'; LTSIZE];
    let (first, last, mut ch) = if ld == 0 { (0u8, 255u8, 0u8) } else { (b'(', b'}', b'0') };
    let mut u = 0usize;
    while u < LTSIZE {
        let weight = (((LTSIZE - u) as u32 * ld) >> 8) as usize + 1;
        let end = (u + weight).min(LTSIZE);
        ldt[u..end].fill(ch);
        u = end;
        ch = if ch >= last { first } else { ch + 1 };
    }
    let gen_char = |seed: &mut u32| ldt[rdg_rand(seed) as usize & (LTSIZE - 1)];
    let mut buf = vec![0u8; size];
    let mut pos = 0usize;
    if match_proba >= 1.0 {
        loop {
            let mut size0 = 1usize << (16 + (rdg_rand(&mut seed) as usize & 3) * 2);
            size0 += rdg_rand(&mut seed) as usize & (size0 - 1);
            if size < pos + size0 {
                return buf;
            }
            pos += size0;
            buf[pos - 1] = gen_char(&mut seed);
        }
    }
    let proba32 = (32768.0 * match_proba) as u32;
    if size > 0 {
        buf[0] = gen_char(&mut seed);
        pos = 1;
    }
    let mut prev_offset = 1usize;
    while pos < size {
        if rdg_rand(&mut seed) & 0x7FFF < proba32 {
            let length = rdg_rand_length(&mut seed) as usize + 4;
            let d = (pos + length).min(size);
            let repeat = rdg_rand(&mut seed) & 15 == 2;
            let rand_offset = (rdg_rand(&mut seed) & 0x7FFF) as usize + 1;
            let offset = if repeat { prev_offset } else { rand_offset.min(pos) };
            let mut m = pos - offset;
            while pos < d {
                buf[pos] = buf[m];
                pos += 1;
                m += 1;
            }
            prev_offset = offset;
        } else {
            let length = rdg_rand_length(&mut seed) as usize;
            let d = (pos + length).min(size);
            while pos < d {
                buf[pos] = gen_char(&mut seed);
                pos += 1;
            }
        }
    }
    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lorem_starts_with_the_classic_sentence_and_fills_exactly() {
        let v = lorem(10_000, 0);
        assert_eq!(v.len(), 10_000);
        assert!(v.starts_with(b"Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do"));
    }

    #[test]
    fn datagen_has_exact_size_and_is_deterministic() {
        let a = datagen(100_000, 0.5, 0.0, 0);
        assert_eq!(a.len(), 100_000);
        assert_eq!(a, datagen(100_000, 0.5, 0.0, 0));
    }
}
