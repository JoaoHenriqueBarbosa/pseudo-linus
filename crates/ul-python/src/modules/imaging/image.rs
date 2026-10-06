//! Armazenamento de imagem do `_imaging`, no layout da libImaging do Pillow 11.1.0 (`Storage.c`):
//! uma linha por `linesize` bytes, 1 byte por pixel nos modos de uma banda de 8 bits (`1`, `L`, `P`),
//! 2 nos `I;16*` e 4 bytes nos demais (inclusive `RGB`, `LA` e `PA`, que guardam o pixel em 32 bits
//! com bytes sobrando, como o `image32` do original).
//!
//! Portado da libImaging (MIT-CMU: Copyright © 1997-2011 Secret Labs AB, © 1995-2011 Fredrik Lundh
//! e colaboradores, © 2010 Jeffrey A. Clark e colaboradores).

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PixType {
    Uint8,
    Int32,
    Float32,
    Special,
}

#[derive(Clone, Debug)]
pub struct Image {
    pub mode: String,
    pub xsize: i32,
    pub ysize: i32,
    pub bands: i32,
    pub pixelsize: i32,
    pub linesize: i32,
    pub kind: PixType,
    pub data: Vec<u8>,
    /// Paleta dos modos `P` e `PA`: modo da paleta (`RGB` ou `RGBA`) e 1024 bytes (256 cores RGBA).
    pub palette: Option<Palette>,
}

#[derive(Clone, Debug)]
pub struct Palette {
    pub mode: String,
    pub size: usize,
    pub colors: Vec<u8>,
}

impl Palette {
    /// `ImagingPaletteNew`: 256 cores pretas e opacas, com tamanho 0.
    pub fn new(mode: &str) -> Palette {
        let mut colors = vec![0u8; 1024];
        for i in 0..256 {
            colors[i * 4 + 3] = 255;
        }
        Palette { mode: mode.to_string(), size: 0, colors }
    }
}

/// `(bands, pixelsize, tipo)` de cada modo aceito; `None` = `unrecognized image mode`.
pub fn mode_layout(mode: &str) -> Option<(i32, i32, PixType)> {
    Some(match mode {
        "1" | "P" | "L" => (1, 1, PixType::Uint8),
        "PA" | "LA" | "La" => (2, 4, PixType::Uint8),
        "F" => (1, 4, PixType::Float32),
        "I" => (1, 4, PixType::Int32),
        "I;16" | "I;16L" | "I;16B" | "I;16N" => (1, 2, PixType::Special),
        "RGB" | "YCbCr" | "LAB" | "HSV" => (3, 4, PixType::Uint8),
        "RGBX" | "RGBA" | "RGBa" | "CMYK" => (4, 4, PixType::Uint8),
        _ => return None,
    })
}

impl Image {
    /// `ImagingNew`: imagem zerada.
    pub fn new(mode: &str, xsize: i32, ysize: i32) -> Option<Image> {
        let (bands, pixelsize, kind) = mode_layout(mode)?;
        let linesize = xsize * pixelsize;
        let palette = matches!(mode, "P" | "PA").then(|| Palette::new("RGB"));
        Some(Image {
            mode: mode.to_string(),
            xsize,
            ysize,
            bands,
            pixelsize,
            linesize,
            kind,
            data: vec![0u8; (linesize.max(0) as usize) * (ysize.max(0) as usize)],
            palette,
        })
    }

    /// Imagem de 8 bits por pixel (o `image8` do original, que também vale pros `I;16`).
    pub fn is8(&self) -> bool {
        self.pixelsize < 4
    }

    pub fn is_i16(&self) -> bool {
        self.mode.starts_with("I;16")
    }

    #[inline]
    pub fn offset(&self, x: i32, y: i32) -> usize {
        (y * self.linesize + x * self.pixelsize) as usize
    }

    pub fn line(&self, y: i32) -> &[u8] {
        let s = (y * self.linesize) as usize;
        &self.data[s..s + self.linesize as usize]
    }

    pub fn line_mut(&mut self, y: i32) -> &mut [u8] {
        let s = (y * self.linesize) as usize;
        let n = self.linesize as usize;
        &mut self.data[s..s + n]
    }

    /// `ImagingFill`: a cor já empacotada em 4 bytes (`getink`).
    pub fn fill(&mut self, ink: [u8; 4]) {
        if self.linesize == 0 || self.ysize == 0 {
            return;
        }
        if self.kind == PixType::Special {
            for y in 0..self.ysize {
                for x in 0..self.xsize {
                    self.put_pixel(x, y, ink);
                }
            }
            return;
        }
        if self.pixelsize == 4 && ink != [0; 4] {
            for px in self.data.chunks_exact_mut(4) {
                px.copy_from_slice(&ink);
            }
        } else {
            self.data.fill(ink[0]);
        }
    }

    /// O `get_pixel` do `Access.c`: os bytes do pixel nas primeiras posições.
    pub fn get_pixel(&self, x: i32, y: i32) -> [u8; 4] {
        let o = self.offset(x, y);
        let mut p = [0u8; 4];
        match self.pixelsize {
            1 => p[0] = self.data[o],
            2 => {
                if self.mode == "I;16B" {
                    // O acesso devolve o valor nativo (little-endian) mesmo no big-endian.
                    p[0] = self.data[o + 1];
                    p[1] = self.data[o];
                } else {
                    p[0] = self.data[o];
                    p[1] = self.data[o + 1];
                }
            }
            _ => p.copy_from_slice(&self.data[o..o + 4]),
        }
        // LA e PA guardam o alfa no 4º byte; o acesso devolve (L, A).
        if self.bands == 2 && self.pixelsize == 4 {
            p = [p[0], p[3], 0, 0];
        }
        p
    }

    /// O `put_pixel` do `Access.c`.
    pub fn put_pixel(&mut self, x: i32, y: i32, ink: [u8; 4]) {
        let o = self.offset(x, y);
        match self.pixelsize {
            1 => self.data[o] = ink[0],
            2 => {
                if self.mode == "I;16B" {
                    self.data[o] = ink[1];
                    self.data[o + 1] = ink[0];
                } else {
                    self.data[o] = ink[0];
                    self.data[o + 1] = ink[1];
                }
            }
            // `put_pixel_32`: os 4 bytes como vieram (no LA o `getink` já repete o L nos três).
            _ => self.data[o..o + 4].copy_from_slice(&ink),
        }
    }
}

/// `DIV255` do `ImagingUtils.h`.
#[inline]
pub fn div255(a: u32) -> u32 {
    let t = a + 128;
    ((t >> 8) + t) >> 8
}

/// `BLEND(mask, in1, in2)`.
#[inline]
pub fn blend(mask: u32, in1: u32, in2: u32) -> u8 {
    div255(in1 * (255 - mask) + in2 * mask) as u8
}

/// `CLIP8`.
#[inline]
pub fn clip8(v: i64) -> u8 {
    v.clamp(0, 255) as u8
}
