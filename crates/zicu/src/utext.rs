//! O `UText` sobre UTF-16 (`utext_openUChars`): índices nativos em unidades de código, e leitura
//! por ponto de código, com os surrogates soltos devolvidos como estão.

/// O `U_SENTINEL` do ICU, devolvido no fim e no começo do texto.
pub const SENTINEL: i32 = -1;

#[derive(Clone, Debug, Default)]
pub struct Utf16Text {
    units: Vec<u16>,
    index: usize,
}

fn is_lead(u: u16) -> bool {
    (0xd800..0xdc00).contains(&u)
}

fn is_trail(u: u16) -> bool {
    (0xdc00..0xe000).contains(&u)
}

impl Utf16Text {
    pub fn new(units: &[u16]) -> Self {
        Self {
            units: units.to_vec(),
            index: 0,
        }
    }

    /// `utext_nativeLength`.
    pub fn len(&self) -> i32 {
        self.units.len() as i32
    }

    pub fn is_empty(&self) -> bool {
        self.units.is_empty()
    }

    /// `utext_getNativeIndex`.
    pub fn index(&self) -> i32 {
        self.index as i32
    }

    /// `utext_setNativeIndex`: preso ao texto e recuado para o começo do ponto de código quando
    /// cai no meio de um par de surrogates.
    pub fn set_index(&mut self, index: i32) {
        let mut i = index.clamp(0, self.len()) as usize;
        if i > 0 && i < self.units.len() && is_trail(self.units[i]) && is_lead(self.units[i - 1]) {
            i -= 1;
        }
        self.index = i;
    }

    /// O ponto de código que começa em `i`, e quantas unidades ele ocupa.
    fn char_at(&self, i: usize) -> (i32, usize) {
        let u = self.units[i];
        if is_lead(u) && i + 1 < self.units.len() && is_trail(self.units[i + 1]) {
            let c = 0x10000 + (((u as i32) - 0xd800) << 10) + ((self.units[i + 1] as i32) - 0xdc00);
            return (c, 2);
        }
        (u as i32, 1)
    }

    /// `utext_current32`.
    pub fn current32(&self) -> i32 {
        if self.index >= self.units.len() {
            return SENTINEL;
        }
        self.char_at(self.index).0
    }

    /// `UTEXT_NEXT32`.
    pub fn next32(&mut self) -> i32 {
        if self.index >= self.units.len() {
            return SENTINEL;
        }
        let (c, width) = self.char_at(self.index);
        self.index += width;
        c
    }

    /// `UTEXT_PREVIOUS32`.
    pub fn previous32(&mut self) -> i32 {
        if self.index == 0 {
            return SENTINEL;
        }
        self.index -= 1;
        let u = self.units[self.index];
        if is_trail(u) && self.index > 0 && is_lead(self.units[self.index - 1]) {
            self.index -= 1;
        }
        self.char_at(self.index).0
    }

    /// `utext_getPreviousNativeIndex`: o começo do ponto de código antes da posição atual.
    pub fn previous_index(&self) -> i32 {
        let mut copy = Self {
            units: Vec::new(),
            index: self.index,
        };
        if copy.index == 0 {
            return 0;
        }
        copy.index -= 1;
        if is_trail(self.units[copy.index]) && copy.index > 0 && is_lead(self.units[copy.index - 1])
        {
            copy.index -= 1;
        }
        copy.index as i32
    }

    /// `utext_moveIndex32` para a frente: avança `n` pontos de código, parando no fim.
    pub fn move_forward32(&mut self, n: usize) {
        for _ in 0..n {
            if self.next32() == SENTINEL {
                break;
            }
        }
    }
}
