// Mesclado das partes traduzidas de utf_c (as partes quebram funções grandes no meio).
#![allow(unused_imports, ambiguous_glob_reexports)]
use crate::prelude::*;

// ---- part_000.rs ----

/// Tabela de consulta usada para ajudar a decodificar o primeiro byte
/// de um caractere UTF-8 multi-byte.
pub const UTF8_TRANS1: [u8; 64] = [
  0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07,
  0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f,
  0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17,
  0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e, 0x1f,
  0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07,
  0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f,
  0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07,
  0x00, 0x01, 0x02, 0x03, 0x00, 0x01, 0x00, 0x00,
];

/// Macro WRITE_UTF8: acrescenta o ponto de código c, codificado em 1 a 4
/// bytes de UTF-8, ao final de out.
#[inline]
pub fn write_utf8(out: &mut Vec<u8>, c: u32) {
  if c < 0x00080 {
    out.push((c & 0xFF) as u8);
  } else if c < 0x00800 {
    out.push(0xC0u8.wrapping_add(((c >> 6) & 0x1F) as u8));
    out.push(0x80u8.wrapping_add((c & 0x3F) as u8));
  } else if c < 0x10000 {
    out.push(0xE0u8.wrapping_add(((c >> 12) & 0x0F) as u8));
    out.push(0x80u8.wrapping_add(((c >> 6) & 0x3F) as u8));
    out.push(0x80u8.wrapping_add((c & 0x3F) as u8));
  } else {
    out.push(0xF0u8.wrapping_add(((c >> 18) & 0x07) as u8));
    out.push(0x80u8.wrapping_add(((c >> 12) & 0x3F) as u8));
    out.push(0x80u8.wrapping_add(((c >> 6) & 0x3F) as u8));
    out.push(0x80u8.wrapping_add((c & 0x3F) as u8));
  }
}

/// Macro WRITE_UTF16LE: acrescenta o ponto de código c em UTF-16
/// little-endian (2 ou 4 bytes) ao final de out.
#[inline]
pub fn write_utf16le(out: &mut Vec<u8>, c: u32) {
  if c <= 0xFFFF {
    out.push((c & 0x00FF) as u8);
    out.push(((c >> 8) & 0x00FF) as u8);
  } else {
    let d = c.wrapping_sub(0x10000);
    out.push((((c >> 10) & 0x003F).wrapping_add((d >> 10) & 0x00C0)) as u8);
    out.push((0x00D8u32.wrapping_add((d >> 18) & 0x03)) as u8);
    out.push((c & 0x00FF) as u8);
    out.push((0x00DCu32.wrapping_add((c >> 8) & 0x03)) as u8);
  }
}

/// Macro WRITE_UTF16BE: acrescenta o ponto de código c em UTF-16
/// big-endian (2 ou 4 bytes) ao final de out.
#[inline]
pub fn write_utf16be(out: &mut Vec<u8>, c: u32) {
  if c <= 0xFFFF {
    out.push(((c >> 8) & 0x00FF) as u8);
    out.push((c & 0x00FF) as u8);
  } else {
    let d = c.wrapping_sub(0x10000);
    out.push((0x00D8u32.wrapping_add((d >> 18) & 0x03)) as u8);
    out.push((((c >> 10) & 0x003F).wrapping_add((d >> 10) & 0x00C0)) as u8);
    out.push((0x00DCu32.wrapping_add((c >> 8) & 0x03)) as u8);
    out.push((c & 0x00FF) as u8);
  }
}

/// Macro READ_UTF8: lê um caractere UTF-8 de z na posição pos (que avança),
/// sem passar de z_term. Ver as notas sobre UTF-8 inválido em utf8_read.
#[inline]
pub fn read_utf8(z: &[u8], pos: &mut usize, z_term: usize) -> u32 {
  let mut c: u32 = z[*pos] as u32;
  *pos += 1;
  if c >= 0xc0 {
    c = UTF8_TRANS1[(c - 0xc0) as usize] as u32;
    while *pos != z_term && (z[*pos] & 0xc0) == 0x80 {
      c = (c << 6).wrapping_add(0x3f & (z[*pos] as u32));
      *pos += 1;
    }
    if c < 0x80 || (c & 0xFFFFF800) == 0xD800 || (c & 0xFFFFFFFE) == 0xFFFE {
      c = 0xFFFD;
    }
  }
  c
}

/// Traduz um único caractere UTF-8 e devolve o valor Unicode. Avança *pz
/// para o próximo byte não lido. A string é considerada terminada em zero:
/// o fim da fatia equivale ao byte 0x00.
///
/// Notas sobre UTF-8 inválido:
/// - Nunca deixa um caractere de 7 bits (0x00 a 0x7f) ser codificado em
///   vários bytes: tal codificação vira 0xfffd.
/// - Nunca deixa um substituto UTF-16 ser codificado: valor entre 0xd800 e
///   0xe000 vira 0xfffd.
/// - Bytes de 0x80 a 0xbf como primeiro byte são lidos como caracteres de
///   um byte, renderizados como eles mesmos.
/// - Aceita codificações longas demais para valores 0x80 ou maiores, sem
///   trocá-las por 0xfffd.
pub fn utf8_read(pz: &mut &[u8]) -> u32 {
  let mut c: u32 = pz.first().copied().unwrap_or(0) as u32;
  if !pz.is_empty() {
    *pz = &pz[1..];
  }
  if c >= 0xc0 {
    c = UTF8_TRANS1[(c - 0xc0) as usize] as u32;
    while let Some(&b) = pz.first() {
      if (b & 0xc0) != 0x80 {
        break;
      }
      c = (c << 6).wrapping_add(0x3f & (b as u32));
      *pz = &pz[1..];
    }
    if c < 0x80 || (c & 0xFFFFF800) == 0xD800 || (c & 0xFFFFFFFE) == 0xFFFE {
      c = 0xFFFD;
    }
  }
  c
}

/// Lê um caractere UTF-8 de z[], sem ler mais de n bytes. z[] não é
/// terminado em zero. Devolve o número de bytes usados (de 1 a 4) e grava o
/// valor em *pi_out. Nenhum esforço para detectar UTF-8 inválido.
pub fn utf8_read_limited(z: &[u8], n: i32, pi_out: &mut u32) -> i32 {
  let mut n = n;
  let mut i: i32 = 1;
  assert!(n > 0);
  let mut c: u32 = z[0] as u32;
  if c >= 0xc0 {
    c = UTF8_TRANS1[(c - 0xc0) as usize] as u32;
    if n > 4 {
      n = 4;
    }
    while i < n && (z[i as usize] & 0xc0) == 0x80 {
      c = (c << 6).wrapping_add(0x3f & (z[i as usize] as u32));
      i += 1;
    }
  }
  *pi_out = c;
  i
}

/// Converte UTF-16 (little ou big endian, conforme big_endian) em UTF-8,
/// acrescentando a out. Ramo de UTF-16 para UTF-8 de vdbe_mem_translate,
/// sem SQLITE_REPLACE_INVALID_UTF. z_term é par.
fn utf16_units_to_utf8(z: &[u8], z_term: usize, big_endian: bool, out: &mut Vec<u8>) {
  let mut z_in = 0usize;
  let read_unit = |z_in: &mut usize| -> u32 {
    let a = z[*z_in] as u32;
    let b = z[*z_in + 1] as u32;
    *z_in += 2;
    if big_endian {
      (a << 8) + b
    } else {
      a + (b << 8)
    }
  };
  while z_in < z_term {
    let mut c = read_unit(&mut z_in);
    if c >= 0xd800 && c < 0xe000 && z_in < z_term {
      let c2 = read_unit(&mut z_in);
      c = (c2 & 0x03FF)
        .wrapping_add((c & 0x003F) << 10)
        .wrapping_add(((c & 0x03C0).wrapping_add(0x0040)) << 10);
    }
    write_utf8(out, c);
  }
}

/// Transforma a codificação de texto interna de p_mem para desired_enc. É
/// erro a string já estar na codificação desejada ou p_mem não conter uma
/// string. O texto de Mem.z é um Vec<u8> dono do buffer.
pub fn vdbe_mem_translate(p_mem: &mut Mem, desired_enc: u8) -> i32 {
  assert!((p_mem.flags & MEM_STR) != 0);
  assert!(p_mem.enc != desired_enc);
  assert!(p_mem.enc != 0);
  assert!(p_mem.n >= 0);

  // Entre UTF-16 little e big endian basta trocar a ordem dos bytes.
  if p_mem.enc != SQLITE_UTF8 && desired_enc != SQLITE_UTF8 {
    let rc = vdbe_mem_make_writeable(p_mem);
    if rc != SQLITE_OK {
      assert!(rc == SQLITE_NOMEM);
      return SQLITE_NOMEM_BKPT;
    }
    let z_term = (p_mem.n & !1) as usize;
    let mut z_in = 0usize;
    while z_in < z_term {
      p_mem.z.swap(z_in, z_in + 1);
      z_in += 2;
    }
    p_mem.enc = desired_enc;
    return SQLITE_OK;
  }

  // len é o máximo de bytes necessários no buffer de saída.
  let len: i64;
  if desired_enc == SQLITE_UTF8 {
    // De UTF-16, o maior crescimento é 2 bytes virarem 4 de UTF-8, mais o
    // byte terminador.
    p_mem.n &= !1;
    len = 2 * (p_mem.n as i64) + 1;
  } else {
    // De UTF-8 para UTF-16, o maior crescimento é 1 byte virar 2, mais dois
    // bytes do terminador.
    len = 2 * (p_mem.n as i64) + 2;
  }

  let z_term = p_mem.n as usize;
  let mut z_out: Vec<u8> = Vec::with_capacity(len as usize);
  let n_new: usize;

  {
    let input = &p_mem.z[..z_term];
    if p_mem.enc == SQLITE_UTF8 {
      let mut z_in = 0usize;
      if desired_enc == SQLITE_UTF16LE {
        // UTF-8 -> UTF-16 little-endian
        while z_in < z_term {
          let c = read_utf8(input, &mut z_in, z_term);
          write_utf16le(&mut z_out, c);
        }
      } else {
        assert!(desired_enc == SQLITE_UTF16BE);
        // UTF-8 -> UTF-16 big-endian
        while z_in < z_term {
          let c = read_utf8(input, &mut z_in, z_term);
          write_utf16be(&mut z_out, c);
        }
      }
      n_new = z_out.len();
      z_out.push(0);
    } else {
      assert!(desired_enc == SQLITE_UTF8);
      // UTF-16 little ou big endian -> UTF-8
      utf16_units_to_utf8(input, z_term, p_mem.enc != SQLITE_UTF16LE, &mut z_out);
      n_new = z_out.len();
    }
  }
  z_out.push(0);
  assert!(
    (n_new as i64) + (if desired_enc == SQLITE_UTF8 { 1 } else { 2 }) <= len
  );

  let flags = MEM_STR | MEM_TERM | (p_mem.flags & (MEM_AFFMASK | MEM_SUBTYPE));
  p_mem.n = n_new as i32;
  vdbe_mem_release(p_mem);
  p_mem.flags = flags;
  p_mem.enc = desired_enc;
  p_mem.z = z_out;
  p_mem.sz_malloc = p_mem.z.len() as _;
  SQLITE_OK
}


// ---- part_001.rs ----

/// Verifica a marca de ordem de bytes (BOM) no início da string UTF-16
/// guardada em p_mem. Se houver, ela é removida e a codificação do Mem é
/// ajustada. Não troca bytes de lugar, só acerta Mem.enc.
///
/// A alocação e a codificação do Mem podem mudar por esta função.
pub fn vdbe_mem_handle_bom(p_mem: &mut Mem) -> i32 {
  let mut rc = SQLITE_OK;
  let mut bom: u8 = 0;

  assert!(p_mem.n >= 0);
  if p_mem.n > 1 {
    let b1 = p_mem.z[0];
    let b2 = p_mem.z[1];
    if b1 == 0xFE && b2 == 0xFF {
      bom = SQLITE_UTF16BE;
    }
    if b1 == 0xFF && b2 == 0xFE {
      bom = SQLITE_UTF16LE;
    }
  }

  if bom != 0 {
    rc = vdbe_mem_make_writeable(p_mem);
    if rc == SQLITE_OK {
      p_mem.n -= 2;
      let n = p_mem.n as usize;
      p_mem.z.copy_within(2..(2 + n), 0);
      p_mem.z[n] = 0;
      p_mem.z[n + 1] = 0;
      p_mem.flags |= MEM_TERM;
      p_mem.enc = bom;
    }
  }
  rc
}

/// z_in é uma string UTF-8. Se n_byte for negativo, devolve o número de
/// caracteres Unicode até o primeiro byte 0x00 (exclusive). Se não for
/// negativo, devolve o número de caracteres nos primeiros n_byte bytes (ou
/// até o primeiro 0x00, o que vier antes). O fim da fatia vale como 0x00.
pub fn utf8_char_len(z_in: &[u8], n_byte: i32) -> i32 {
  let mut r = 0;
  let z_term: usize = if n_byte >= 0 { n_byte as usize } else { usize::MAX };
  let mut i = 0usize;
  assert!(i <= z_term);
  while i < z_in.len() && z_in[i] != 0 && i < z_term {
    skip_utf8(z_in, &mut i);
    r += 1;
  }
  r
}

/// Converte uma string UTF-16 na codificação enc em UTF-8. Devolve None se
/// houver erro de alocação.
pub fn utf16_to_8(db: &Rc<RefCell<sqlite3>>, z: &[u8], n_byte: i32, enc: u8) -> Option<Vec<u8>> {
  let mut m = Mem::default();
  m.db = Some(Rc::downgrade(db));
  vdbe_mem_set_str(&mut m, z, n_byte, enc, SQLITE_STATIC);
  vdbe_change_encoding(&mut m, SQLITE_UTF8);
  let malloc_failed = db.borrow().malloc_failed != 0;
  if malloc_failed {
    vdbe_mem_release(&mut m);
    m.z = Vec::new();
  }
  assert!((m.flags & MEM_TERM) != 0 || malloc_failed);
  assert!((m.flags & MEM_STR) != 0 || malloc_failed);
  assert!(!m.z.is_empty() || malloc_failed);
  if m.z.is_empty() {
    None
  } else {
    Some(m.z)
  }
}

/// z_in é uma string UTF-16 com pelo menos n_char caracteres. Devolve o
/// número de bytes nos primeiros n_char caracteres. n_char não é negativo.
pub fn utf16_byte_len(z_in: &[u8], n_char: i32) -> i32 {
  let native_le = SQLITE_UTF16NATIVE == SQLITE_UTF16LE;
  let mut z: usize = if native_le { 1 } else { 0 };
  let mut n = 0;

  while n < n_char {
    let c = z_in[z];
    z += 2;
    if c >= 0xd8 && c < 0xdc && z_in[z] >= 0xdc && z_in[z] < 0xe0 {
      z += 2;
    }
    n += 1;
  }
  (z as i32) - (if native_le { 1 } else { 0 })
}

