//! JPEG do libjpeg-turbo 2.1.5 (o `libjpeg62-turbo` do Debian 13), traduzido sem `unsafe`, para o
//! `PIL._imaging` do pseudo-linus.
//!
//! O decodificador cobre o que o Pillow pede: quadros sequenciais e progressivos com Huffman e
//! amostras de 8 bits, a IDCT inteira lenta (`JDCT_ISLOW`), o aumento de amostragem "fancy" e as
//! conversões de cor do `jdcolor.c`. A saída tem os mesmos pixels do libjpeg-turbo do Debian.
//!
//! Fora do porte: codificação aritmética (`SOF9`..`SOF11`), JPEG sem perdas (`SOF3`), 12 bits,
//! modo de imagem em buffer e saída por quantização de cores, que o Pillow não usa.

mod decode;
mod idct;
mod output;

/// `J_COLOR_SPACE` restrito aos valores que o Pillow usa.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ColorSpace {
    Unknown,
    Grayscale,
    Rgb,
    YCbCr,
    Cmyk,
    Ycck,
}

/// Erros fatais (`ERREXIT`), com o texto do `jerror.h`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    NotJpeg(u8, u8),
    Truncated,
    BadLength,
    BadPrecision(u8),
    EmptyImage,
    BadSampling,
    FractionalSampling,
    BadHuffTable,
    BadQuantTable,
    NoHuffTable(usize),
    NoQuantTable(usize),
    BadComponentId(u8),
    BadProgression(usize, usize, i32, i32),
    SofDuplicate,
    SofUnsupported(u8),
    SosNoSof,
    NoImage,
    BadColorSpace,
    ConversionNotImplemented,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::NotJpeg(a, b) => write!(f, "Not a JPEG file: starts with 0x{a:02x} 0x{b:02x}"),
            Error::Truncated => write!(f, "Premature end of JPEG file"),
            Error::BadLength => write!(f, "Bogus marker length"),
            Error::BadPrecision(p) => write!(f, "Unsupported JPEG data precision {p}"),
            Error::EmptyImage => write!(f, "Empty JPEG image (DNL not supported)"),
            Error::BadSampling | Error::FractionalSampling => write!(f, "Unsupported JPEG process: SOF type 0x00"),
            Error::BadHuffTable => write!(f, "Bogus Huffman table definition"),
            Error::BadQuantTable => write!(f, "Bogus DQT index"),
            Error::NoHuffTable(n) => write!(f, "Huffman table 0x{n:02x} was not defined"),
            Error::NoQuantTable(n) => write!(f, "Quantization table 0x{n:02x} was not defined"),
            Error::BadComponentId(n) => write!(f, "Invalid component ID {n} in SOS"),
            Error::BadProgression(a, b, c, d) => {
                write!(f, "Invalid progressive parameters Ss={a} Se={b} Ah={c} Al={d}")
            }
            Error::SofDuplicate => write!(f, "Invalid JPEG file structure: two SOF markers"),
            Error::SofUnsupported(m) => write!(f, "Unsupported JPEG process: SOF type 0x{m:02x}"),
            Error::SosNoSof => write!(f, "Invalid JPEG file structure: SOS before SOF"),
            Error::NoImage => write!(f, "JPEG datastream contains no image"),
            Error::BadColorSpace => write!(f, "Bogus input colorspace"),
            Error::ConversionNotImplemented => write!(f, "Unsupported color conversion request"),
        }
    }
}

/// Avisos (`WARNMS`): o libjpeg segue decodificando.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Warning {
    HitMarker,
    HuffBadCode,
    MustResync,
    NotSequential,
    BogusProgression(usize, usize),
    ExtraneousData(usize, u8),
}

/// Parâmetros que o Pillow ajusta depois do `jpeg_read_header`.
#[derive(Clone, Copy, Debug)]
pub struct Options {
    /// `jpeg_color_space`; `None` mantém o palpite do `default_decompress_parms`.
    pub jpeg_color_space: Option<ColorSpace>,
    /// `out_color_space`; `None` mantém o padrão (RGB para três componentes, CMYK para quatro).
    pub out_color_space: Option<ColorSpace>,
    /// `do_fancy_upsampling`.
    pub fancy_upsampling: bool,
}

impl Default for Options {
    fn default() -> Options {
        Options { jpeg_color_space: None, out_color_space: None, fancy_upsampling: true }
    }
}

/// O resultado de `decode`: amostras intercaladas, linha a linha.
#[derive(Clone, Debug)]
pub struct Decoded {
    pub width: usize,
    pub height: usize,
    pub components: usize,
    pub data: Vec<u8>,
    pub warnings: Vec<Warning>,
}

/// O cabeçalho que o `jpeg_read_header` deixa no `cinfo`.
#[derive(Clone, Debug)]
pub struct Header {
    pub width: usize,
    pub height: usize,
    pub components: usize,
    pub progressive: bool,
    pub jpeg_color_space: ColorSpace,
}

/// Decodifica o arquivo JPEG inteiro (`jpeg_read_header`, `jpeg_start_decompress` e
/// `jpeg_read_scanlines` até o fim).
pub fn decode(data: &[u8], opts: &Options) -> Result<Decoded, Error> {
    let frame = decode::read(data)?;
    let jcs = opts.jpeg_color_space.unwrap_or_else(|| frame.default_color_space());
    let default_out = match frame.comps.len() {
        1 => ColorSpace::Grayscale,
        3 => ColorSpace::Rgb,
        4 => ColorSpace::Cmyk,
        _ => ColorSpace::Unknown,
    };
    let ocs = opts.out_color_space.unwrap_or(default_out);
    let (pixels, components) = output::output(&frame, jcs, ocs, opts.fancy_upsampling)?;
    Ok(Decoded { width: frame.width, height: frame.height, components, data: pixels, warnings: frame.warnings })
}
