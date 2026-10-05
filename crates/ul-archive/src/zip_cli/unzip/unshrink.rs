//! O unshrink do unzip (unshrink.c), pro método 1 (shrink do PKZIP 1.x): LZW de 9 a 13 bits com
//! o escape 256 (1 aumenta o tamanho do código, 2 limpa as folhas da árvore), a saída num buffer
//! de 64 KiB despejado quando enche, e as tabelas na mesma área de memória da janela do inflate.

use super::inflate::WSIZE;
use super::Uz;

/// `MAX_BITS` e `HSIZE`: o maior código tem 13 bits.
const MAX_BITS: i32 = 13;
const HSIZE: usize = 1 << MAX_BITS;
/// O escape, também o pai das raízes (`BOGUSCODE`).
const BOGUSCODE: i32 = 256;
/// Bits altos do `parent`: código livre e código com filho (`FREE_CODE`, `HAS_CHILD`).
const CODE_MASK: i32 = HSIZE as i32 - 1;
const FREE_CODE: i32 = HSIZE as i32;
const HAS_CHILD: i32 = (HSIZE << 1) as i32;
/// `OUTBUFSIZ` no Linux (`lenEOL * WSIZE`), igual ao `RAWBUFSIZ` do modo texto.
const OUTBUFSIZ: usize = WSIZE;

/// As tabelas do unshrink (`G.area.shrink`): `Parent` em `int`, `value` e `Stack`, sobrepostas aos
/// primeiros 48 KiB da janela do inflate. O C não inicializa tudo (o `parent[256]` e o `value`
/// dos códigos livres guardam o que estava na área), e o inflate de um membro seguinte enxerga o
/// que o unshrink deixou ali.
struct Shrink {
    parent: Vec<i32>,
    value: Vec<u8>,
    stack: Vec<u8>,
}

const VALUE_AT: usize = HSIZE * 4;
const STACK_AT: usize = VALUE_AT + HSIZE;

impl Shrink {
    fn from_area(area: &[u8]) -> Shrink {
        let parent = area[..VALUE_AT].as_chunks::<4>().0.iter().map(|c| i32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
        Shrink { parent, value: area[VALUE_AT..STACK_AT].to_vec(), stack: area[STACK_AT..STACK_AT + HSIZE].to_vec() }
    }

    fn to_area(&self, area: &mut [u8]) {
        for (i, p) in self.parent.iter().enumerate() {
            area[i * 4..i * 4 + 4].copy_from_slice(&p.to_le_bytes());
        }
        area[VALUE_AT..STACK_AT].copy_from_slice(&self.value);
        area[STACK_AT..STACK_AT + HSIZE].copy_from_slice(&self.stack);
    }

    /// `partial_clear`: solta os códigos que não são pai de ninguém.
    fn partial_clear(&mut self, lastcodeused: i32) {
        for code in BOGUSCODE + 1..=lastcodeused {
            let cparent = self.parent[code as usize] & CODE_MASK;
            if cparent > BOGUSCODE {
                self.parent[cparent as usize] |= HAS_CHILD;
            }
        }
        for code in BOGUSCODE + 1..=lastcodeused {
            let p = &mut self.parent[code as usize];
            if *p & HAS_CHILD != 0 {
                *p &= !HAS_CHILD;
            } else {
                *p = FREE_CODE;
            }
        }
    }
}

/// O leitor de bits do `READBITS` (`G.bitbuf`, `G.bits_left`, `G.zipeof`).
#[derive(Default)]
struct Bits {
    buf: u64,
    left: i32,
    eof: bool,
}

impl Uz {
    /// `READBITS`: quando faltam bits, enche o buffer até 57 bits ou até o fim dos dados; só é fim
    /// (`zipeof`) se não veio byte nenhum.
    fn readbits(&mut self, bits: &mut Bits, n: i32) -> i32 {
        if n > bits.left {
            bits.eof = true;
            while bits.left <= 56 {
                let Some(c) = self.next_byte() else { break };
                bits.buf |= u64::from(c) << bits.left;
                bits.left += 8;
                bits.eof = false;
            }
        }
        let z = (bits.buf as u32 & ((1u32 << n) - 1)) as i32;
        bits.buf >>= n;
        bits.left -= n;
        z
    }

    /// `unshrink`: 0, `PK_ERR` em dado inválido, ou o erro do `flush`.
    pub fn unshrink(&mut self) -> i32 {
        let mut area = std::mem::take(&mut self.x.slide);
        area.resize(WSIZE, 0);
        let mut sh = Shrink::from_area(&area);
        let r = self.unshrink_codes(&mut sh);
        sh.to_area(&mut area);
        self.x.slide = area;
        r
    }

    fn unshrink_codes(&mut self, sh: &mut Shrink) -> i32 {
        let mut bits = Bits::default();
        let mut codesize = 9;
        let mut lastfreecode = BOGUSCODE;
        for code in 0..BOGUSCODE as usize {
            sh.value[code] = code as u8;
            sh.parent[code] = BOGUSCODE;
        }
        for p in &mut sh.parent[BOGUSCODE as usize + 1..] {
            *p = FREE_CODE;
        }
        let mut out = vec![0u8; OUTBUFSIZ];
        let mut outcnt = 0usize;
        let mut oldcode = self.readbits(&mut bits, codesize);
        if bits.eof {
            return super::PK_OK;
        }
        let mut finalval = oldcode as u8;
        out[outcnt] = finalval;
        outcnt += 1;
        let stacktop = HSIZE as isize - 1;
        loop {
            let mut code = self.readbits(&mut bits, codesize);
            if bits.eof {
                break;
            }
            if code == BOGUSCODE {
                code = self.readbits(&mut bits, codesize);
                if bits.eof {
                    break;
                }
                if code == 1 {
                    codesize += 1;
                    if codesize > MAX_BITS {
                        return super::PK_ERR;
                    }
                } else if code == 2 {
                    sh.partial_clear(lastfreecode);
                    lastfreecode = BOGUSCODE;
                }
                continue;
            }
            // A cadeia do código até a raiz, escrita de trás pra frente na pilha.
            let mut newstr = stacktop;
            let curcode = code;
            if sh.parent[code as usize] == FREE_CODE {
                sh.stack[newstr as usize] = finalval;
                newstr -= 1;
                code = oldcode;
            }
            while code != BOGUSCODE {
                if newstr < 0 {
                    return super::PK_ERR;
                }
                if sh.parent[code as usize] == FREE_CODE {
                    sh.stack[newstr as usize] = finalval;
                    newstr -= 1;
                    code = oldcode;
                } else {
                    sh.stack[newstr as usize] = sh.value[code as usize];
                    newstr -= 1;
                    code = sh.parent[code as usize] & CODE_MASK;
                }
            }
            let len = (stacktop - newstr) as usize;
            newstr += 1;
            let start = newstr as usize;
            finalval = sh.stack[start];
            for i in start..start + len {
                out[outcnt] = sh.stack[i];
                outcnt += 1;
                if outcnt == OUTBUFSIZ {
                    let r = self.flush(&out[..outcnt]);
                    if r != 0 {
                        return r;
                    }
                    outcnt = 0;
                }
            }
            // A folha nova (primeiro byte da cadeia) entra como filha do código anterior.
            code = lastfreecode + 1;
            while (code as usize) < HSIZE && sh.parent[code as usize] != FREE_CODE {
                code += 1;
            }
            lastfreecode = code;
            if code as usize >= HSIZE {
                return super::PK_ERR;
            }
            sh.value[code as usize] = finalval;
            sh.parent[code as usize] = oldcode;
            oldcode = curcode;
        }
        if outcnt > 0 {
            let r = self.flush(&out[..outcnt]);
            if r != 0 {
                return r;
            }
        }
        super::PK_OK
    }
}
