//! A extração e o teste dos membros (extract.c): o laço sobre o diretório central em blocos, a
//! checagem de cada cabeçalho local, a descompressão, o CRC e os resumos.

use super::Uz;

/// Entradas do diretório central lidas por bloco antes de ir aos dados (`DIR_BLKSIZ`).
const DIR_BLKSIZ: usize = 16384;

/// Métodos de compressão.
pub const STORED: u16 = 0;
pub const SHRUNK: u16 = 1;
pub const REDUCED1: u16 = 2;
pub const REDUCED4: u16 = 5;
pub const IMPLODED: u16 = 6;
pub const TOKENIZED: u16 = 7;
pub const DEFLATED: u16 = 8;
pub const ENHDEFLATED: u16 = 9;
pub const BZIPPED: u16 = 12;

/// Versão que este unzip extrai (`UNZIP_VERSION`, a do bzip2) e a do VMS.
const UNZIP_VERSION: u8 = 46;
const VMS_UNZIP_VERSION: u8 = 42;

/// Nome de cada método de [`super::list::COMPR_IDS`] nas mensagens de "não suportado".
const COMPR_NAMES: [&str; 17] = [
    "store", "shrink", "reduce", "reduce", "reduce", "reduce", "implode", "tokenize", "deflate", "deflate64",
    "DCL implode", "bzip2", "LZMA", "IBM/Terse", "IBM LZ77", "WavPack", "PPMd",
];

/// Faixas do arquivo já atribuídas a algum componente (`cover_t`), pra recusar membros que se
/// sobrepõem (a defesa contra zip bomb do Debian). Ordenadas, disjuntas e com as adjacentes fundidas.
#[derive(Default)]
pub struct Cover {
    span: Vec<(i64, i64)>,
}

impl Cover {
    /// Índice da primeira faixa que começa depois de `val` (`cover_find`).
    fn find(&self, val: i64) -> usize {
        self.span.partition_point(|&(beg, _)| beg <= val)
    }

    /// `val` cai dentro de alguma faixa (`cover_within`).
    pub fn within(&self, val: i64) -> bool {
        let pos = self.find(val);
        pos > 0 && val < self.span[pos - 1].1
    }

    /// Acrescenta `beg..end` se não sobrepõe nada (`cover_add`): 0 se entrou, 1 se sobrepõe, -1 se
    /// a faixa é vazia ou invertida.
    pub fn add(&mut self, beg: i64, end: i64) -> i32 {
        if beg >= end {
            return -1;
        }
        let pos = self.find(beg);
        if (pos > 0 && beg < self.span[pos - 1].1) || (pos < self.span.len() && end > self.span[pos].0) {
            return 1;
        }
        let prec = pos > 0 && beg == self.span[pos - 1].1;
        let foll = pos < self.span.len() && end == self.span[pos].0;
        match (prec, foll) {
            (true, true) => {
                self.span[pos - 1].1 = self.span[pos].1;
                self.span.remove(pos);
            }
            (true, false) => self.span[pos - 1].1 = end,
            (false, true) => self.span[pos].0 = beg,
            (false, false) => self.span.insert(pos, (beg, end)),
        }
        0
    }

}

/// O que sobrescrever quando o arquivo de saída já existe (`G.overwrite_mode`).
#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub enum Overwrite {
    #[default]
    Query,
    Always,
    Never,
}

/// O resultado do `check_for_newer`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Newer {
    DoesNotExist,
    ExistsAndOlder,
    /// O `EXISTS_AND_NEWER` do C (o existente é igual ou mais novo).
    ExistsNotOlder,
}

/// O estado da extração que o C guarda nos globais.
#[derive(Default)]
pub struct Ex {
    pub cover: Cover,
    pub overwrite_mode: Overwrite,
    /// Já avisou do separador `\` num nome de FAT.
    pub reported_backslash: bool,
    /// 1: o disco encheu e o usuário quis seguir com os outros membros; 2: parar.
    pub disk_full: u8,
    /// CRC dos bytes descomprimidos do membro (`G.crc32val`).
    pub crc: crc32fast::Hasher,
    /// Primeiro `flush` do membro (zera o estado da conversão de texto) e CR no fim do bloco anterior.
    pub newfile: bool,
    pub did_cr_last: bool,
    /// A conversão do texto de registro variável do VMS no `-a` (`G.VMS_line_state`): -1 desligada,
    /// 0 esperando o comprimento, 1 com meio comprimento lido, 2 copiando a linha, 3 pondo o fim de
    /// linha, 4 pulando o byte de alinhamento das linhas ímpares.
    pub vms_line_state: i32,
    pub vms_line_length: u32,
    pub vms_line_pad: bool,
    /// O membro é um link simbólico sendo extraído pro disco (`G.symlnk`).
    pub symlnk: bool,
    /// O arquivo de saída aberto.
    pub outfile: Option<sysabi::Fd>,
    /// Links simbólicos adiados pro fim da extração.
    pub slinks: Vec<super::unix::Slink>,
    /// O diretório de extração (`-d`) com a barra final (`rootpath`), o caminho sendo montado pro
    /// membro (`buildpath`), e as decisões do `mapname`: criar diretórios, criou algum, e o nome
    /// novo dado na pergunta é absoluto.
    pub rootpath: Vec<u8>,
    pub buildpath: Vec<u8>,
    pub create_dirs: bool,
    pub created_dir: bool,
    pub renamed_fullpath: bool,
    /// A última resposta lida (`G.answerbuf`): o `fgets` no fim do stdin não a troca.
    pub answerbuf: Vec<u8>,
    /// Um `read()` do zip falhou no meio de um membro: o C sai na hora com `PK_BADERR`.
    pub read_failed: bool,
    /// A janela do inflate (`slide`): persiste entre membros, como o buffer global do C. É a mesma
    /// área (`G.area`, uma união) das tabelas do unshrink.
    pub slide: Vec<u8>,
    /// Bytes comprimidos que o explode consumiu quando eles não batem com o `csize`.
    pub used_csize: i64,
    /// As tabelas do Huffman fixo do deflate e do deflate64, montadas uma vez.
    pub fixed: [Option<super::inflate::Fixed>; 2],
    /// O modo memória (`G.mem_mode`) do inflate de um bloco do campo extra: a saída e quanto
    /// ainda cabe nela.
    pub mem: Option<(Vec<u8>, usize)>,
    /// A cifra: chaves correntes, a senha que abriu o último membro, primeiro membro cifrado do
    /// zip ainda por vir (`newzip`), e não perguntar mais (`nopwd`).
    pub keys: super::crypt::Keys,
    pub key: Option<Vec<u8>>,
    pub newzip: bool,
    pub nopwd: bool,
    /// O buffer do stdin do stdio: o `fgets` lê um bloco e as perguntas seguintes consomem dele.
    stdin_buf: Vec<u8>,
    stdin_eof: bool,
}

impl Uz {
    /// `fgets(buf, n, stdin)`: até `n - 1` bytes ou até o fim da linha, inclusive; `None` no fim do
    /// arquivo sem nada lido.
    pub fn fgets(&mut self, n: usize) -> Option<Vec<u8>> {
        let mut line = Vec::new();
        while line.len() + 1 < n {
            if self.x.stdin_buf.is_empty() {
                if self.x.stdin_eof {
                    break;
                }
                let mut chunk = vec![0u8; 4096];
                match sysabi::sys::read(sysabi::Fd::STDIN, &mut chunk) {
                    Ok(0) | Err(_) => {
                        self.x.stdin_eof = true;
                        break;
                    }
                    Ok(k) => self.x.stdin_buf.extend_from_slice(&chunk[..k]),
                }
            }
            let c = self.x.stdin_buf.remove(0);
            line.push(c);
            if c == b'\n' {
                break;
            }
        }
        if line.is_empty() { None } else { Some(line) }
    }

    /// Resposta a uma pergunta (o `answerbuf` de 10 bytes); vazia no fim do stdin.
    fn read_answer(&mut self) -> Vec<u8> {
        self.fgets(10).unwrap_or_default()
    }
}

/// O `%-22s` do printf sobre bytes: o nome e espaços até 22 bytes.
pub fn pad22(name: &[u8]) -> Vec<u8> {
    let mut v = name.to_vec();
    while v.len() < 22 {
        v.push(b' ');
    }
    v
}

impl Uz {
    /// `"   skipping: NOME  motivo\n"` no stderr, a menos que o silêncio pedido cale (as condições
    /// `(tflag && qflag) || (!tflag && qflag)` do C).
    fn skip_msg(&mut self, why: &str) {
        if (self.o.tflag != 0 && self.o.qflag != 0) || (self.o.tflag == 0 && self.o.qflag != 0) {
            return;
        }
        let mut m = b"   skipping: ".to_vec();
        m.extend(pad22(&super::fileio::fnfilter(&self.filename)));
        m.extend_from_slice(b"  ");
        m.extend_from_slice(why.as_bytes());
        m.push(b'\n');
        self.info(super::MSG_STDERR, m);
    }

    /// Os atributos do membro no formato Unix (`mapattr` do unix.c): os bits altos dos atributos
    /// externos dos sistemas que os usam, senão os do MS-DOS (somente leitura e diretório)
    /// expandidos e filtrados pela umask. Também decide se é link simbólico.
    fn mapattr(&mut self) {
        use super::process::{AMIGA, THEOS, UNIX, VMS, ACORN, ATARI, ATHEOS, BEOS, QDOS, TANDEM, FS_FAT};
        const S_IFMT: u32 = 0o170000;
        const S_IFLNK: u32 = 0o120000;
        let is_lnk = |a: u32| a & S_IFMT == S_IFLNK;
        let mut tmp = self.crec.external_file_attributes;
        let hostnum = self.pinfo.hostnum;
        self.pinfo.file_attr = 0;
        let mut dos = true;
        match hostnum {
            AMIGA => {
                tmp = tmp >> 17 & 7;
                self.pinfo.file_attr = tmp << 6 | tmp << 3 | tmp;
                dos = false;
            }
            THEOS | UNIX | VMS | ACORN | ATARI | ATHEOS | BEOS | QDOS | TANDEM => {
                if hostnum == THEOS {
                    tmp &= 0xF1FF_FFFF;
                    tmp &= if tmp & 0xF000_0000 != 0x4000_0000 { 0x01FF_FFFF } else { 0x41FF_FFFF };
                }
                self.pinfo.file_attr = tmp >> 16;
                if !(self.pinfo.file_attr == 0 && self.extra_field.is_some() && self.ef_has_foreign_perms()) {
                    let unixlike = matches!(hostnum, UNIX | ATARI | ATHEOS | BEOS | VMS);
                    self.pinfo.symlink = is_lnk(self.pinfo.file_attr) && unixlike;
                    return;
                }
                self.pinfo.file_attr = tmp >> 16;
            }
            FS_FAT => self.pinfo.file_attr = tmp >> 16,
            _ => {}
        }
        if dos {
            if tmp & 0x10 == 0 && self.filename.last() == Some(&b'/') {
                tmp |= 0x10;
            }
            tmp = u32::from(tmp & 1 == 0) << 1 | (tmp & 0x10) >> 4;
            if self.pinfo.file_attr & 0o700 == 0o400 | tmp << 6 {
                self.pinfo.symlink = is_lnk(self.pinfo.file_attr) && hostnum == FS_FAT;
                return;
            }
            self.pinfo.file_attr = 0o444 | tmp << 6 | tmp << 3 | tmp;
        }
        let sys = super::sys();
        let mask = sys.umask(0);
        sys.umask(mask);
        self.pinfo.file_attr &= !mask;
    }

    /// O campo extra central traz as permissões num formato que o `mapattr` não lê (VMS da PKWARE,
    /// ou o "ASi Unix" curto demais); o bloco ASi legível fica com o modo dele em `file_attr`.
    fn ef_has_foreign_perms(&mut self) -> bool {
        let Some(ef) = self.extra_field.as_ref() else { return false };
        let mut rest = &ef[..ef.len().min(usize::from(self.crec.extra_field_length))];
        while rest.len() >= 4 {
            let id = u16::from_le_bytes([rest[0], rest[1]]);
            let len = usize::from(u16::from_le_bytes([rest[2], rest[3]]));
            if len > rest.len() - 4 {
                break;
            }
            match id {
                0x756e if len >= 6 => {
                    self.pinfo.file_attr = u32::from(u16::from_le_bytes([rest[8], rest[9]]));
                    return false;
                }
                0x756e | 0x000c => return true,
                _ => {}
            }
            rest = &rest[4 + len..];
        }
        false
    }
}

impl Uz {
    /// Guarda o que a extração vai precisar do cabeçalho central, ou recusa o membro (versão,
    /// método) com a mensagem de "skipping" (`store_info`). `false` pula o membro.
    fn store_info(&mut self) -> bool {
        use super::process::VMS;
        let c = &self.crec;
        let p = &mut self.pinfo;
        p.encrypted = c.general_purpose_bit_flag & 1 != 0;
        p.ext_loc_hdr = c.general_purpose_bit_flag & 8 == 8;
        p.textfile = c.internal_file_attributes & 1 != 0;
        p.crc = c.crc32;
        p.compr_size = c.csize;
        p.uncompr_size = c.ucsize;
        p.textmode = match self.o.aflag {
            0 => false,
            1 => p.textfile,
            _ => true,
        };
        let [need, need_os] = c.version_needed_to_extract;
        let method = c.compression_method;
        if need_os == VMS {
            if need > VMS_UNZIP_VERSION {
                self.skip_msg(&format!("need VMS compat. v{}.{} (can do v{}.{})", need / 10, need % 10, VMS_UNZIP_VERSION / 10, VMS_UNZIP_VERSION % 10));
                return false;
            } else if self.o.tflag == 0 && self.x.overwrite_mode != Overwrite::Always {
                let mut q = b"\n".to_vec();
                q.extend(super::fileio::fnfilter(&self.filename));
                q.extend_from_slice(b":  stored in VMS format.  Extract anyway? (y/n) ");
                self.info(super::MSG_STDERR | super::MSG_LNEWLN | 0x80, q);
                let answer = self.read_answer();
                if !matches!(answer.first(), Some(b'y' | b'Y')) {
                    return false;
                }
            }
        } else if need > UNZIP_VERSION {
            let s = UNZIP_VERSION;
            self.skip_msg(&format!("need PK compat. v{}.{} (can do v{}.{})", need / 10, need % 10, s / 10, s % 10));
            return false;
        }
        let unknown = (REDUCED1..=REDUCED4).contains(&method) || method == TOKENIZED || (method > ENHDEFLATED && method != BZIPPED);
        if unknown {
            let idx = super::list::find_compr_idx(method);
            let why = match COMPR_NAMES.get(idx) {
                Some(name) => format!("`{name}' method not supported"),
                None => format!("unsupported compression method {method}"),
            };
            self.skip_msg(&why);
            return false;
        }
        self.pinfo.cfilname = Some(self.filename.clone());
        self.mapattr();
        self.pinfo.diskstart = self.crec.disk_number_start;
        self.pinfo.offset = self.crec.relative_offset_local_header as i64;
        true
    }

    /// Lê o cabeçalho local (`process_local_file_hdr`); com o bit 3 os tamanhos e o CRC vêm do
    /// diretório central, porque os do local ainda não eram conhecidos quando ele foi escrito.
    fn process_local_file_hdr(&mut self) -> i32 {
        use super::process::LREC_SIZE;
        let mut b = [0u8; LREC_SIZE];
        if self.readbuf(&mut b) == 0 {
            return super::PK_EOF;
        }
        let word = |o: usize| u16::from_le_bytes([b[o], b[o + 1]]);
        let long = |o: usize| u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]);
        let mut l = super::process::Lrec {
            version_needed_to_extract: [b[0], b[1]],
            general_purpose_bit_flag: word(2),
            compression_method: word(4),
            last_mod_dos_datetime: long(6),
            crc32: long(10),
            csize: u64::from(long(14)),
            ucsize: u64::from(long(18)),
            filename_length: word(22),
            extra_field_length: word(24),
        };
        if l.general_purpose_bit_flag & 8 != 0 {
            l.crc32 = self.pinfo.crc;
            l.csize = self.pinfo.compr_size;
            l.ucsize = self.pinfo.uncompr_size;
        }
        self.csize = l.csize as i64;
        self.lrec = l;
        super::PK_OK
    }
}

/// O que a segunda volta (`extract_or_test_entrylist`) acumula entre os lotes: membros tentados,
/// os pulados por senha errada, os bytes extras da tentativa de recompensação e os diretórios
/// criados, cujas datas e permissões só são aplicadas no fim.
#[derive(Default)]
struct ListState {
    filnum: u64,
    num_bad_pwd: u64,
    old_extra_bytes: i64,
    dirs: Vec<super::unix::DirAttr>,
}

impl Uz {
    /// As faixas que nenhum membro pode tocar: bytes antes do zip, o diretório central e os
    /// registros de fim. `Some` é o código de saída quando elas já se sobrepõem.
    fn init_cover(&mut self) -> Option<i32> {
        use super::MSG_STDERR;
        let xb = self.zin.extra_bytes;
        let (off, size) = (self.ecrec.offset_start_central_directory as i64, self.ecrec.size_central_directory as i64);
        let c = &mut self.x.cover;
        c.span.clear();
        if c.add(xb + off, xb + off + size) != 0 {
            self.info(MSG_STDERR, "error: not enough memory for bomb detection\n");
            return Some(super::PK_MEM);
        }
        let e = &self.ecrec;
        let bad = (xb != 0 && c.add(0, xb) != 0) || (e.have_ecr64 && c.add(e.ec64_start, e.ec64_end) != 0) || c.add(e.ec_start, e.ec_end) != 0;
        if bad {
            self.info(MSG_STDERR, "error: invalid zip file with overlapped components (possible zip bomb)\n");
            return Some(super::PK_BOMB);
        }
        None
    }

    /// Nome, campo extra e comentário de um cabeçalho central; `false` encerra a varredura.
    fn read_central_strings(&mut self, error_in_archive: &mut i32) -> bool {
        use super::{MSG_LNEWLN, MSG_STDERR, PK_WARN};
        let c = &self.crec;
        let (fnl, efl, fcl) = (usize::from(c.filename_length), usize::from(c.extra_field_length), usize::from(c.file_comment_length));
        let steps: [(u8, &str); 3] = [(0, "bad filename length (central)"), (1, "bad extra field length (central)"), (2, "bad file comment length")];
        for (which, what) in steps {
            let error = match which {
                0 => self.read_filename(fnl, false),
                1 => self.read_extra_field(efl),
                _ => self.skip_string(fcl),
            };
            if error > *error_in_archive {
                *error_in_archive = error;
            }
            if error > PK_WARN {
                let mut m = super::fileio::fnfilter(&self.filename);
                m.extend_from_slice(b":  ");
                m.extend_from_slice(what.as_bytes());
                m.push(b'\n');
                let flag = if which == 2 { MSG_STDERR | MSG_LNEWLN } else { MSG_STDERR };
                self.info(flag, m);
                return false;
            }
        }
        true
    }
}

impl Uz {
    /// Com `-d` e extraindo pro disco, garante o diretório de extração; `Some` aborta.
    fn prepare_exdir(&mut self) -> Option<i32> {
        use super::unix::{MPN_INF_SKIP, MPN_NOMEM};
        let exdir = self.o.exdir.clone()?;
        if !self.extract_flag {
            return None;
        }
        self.x.create_dirs = !self.o.fflag;
        let error = self.checkdir_root(&exdir);
        (error > MPN_INF_SKIP).then_some(if error == MPN_NOMEM { super::PK_MEM } else { super::PK_ERR })
    }

    /// Depois de todos os membros: cria os links simbólicos adiados e aplica dono, datas e
    /// permissões dos diretórios criados, do mais fundo pro mais raso.
    fn finish_deferred(&mut self, st: &mut ListState, error_in_archive: &mut i32) {
        if !self.x.slinks.is_empty() {
            if self.o.qflag == 0 {
                self.info(0, "finishing deferred symbolic links:\n");
            }
            for s in std::mem::take(&mut self.x.slinks) {
                self.set_deferred_symlink(&s);
            }
        }
        let mut dirs = std::mem::take(&mut st.dirs);
        if dirs.is_empty() {
            return;
        }
        dirs.sort_by(|a, b| b.fname.cmp(&a.fname));
        let mut ndirs_fail = 0u64;
        for d in &dirs {
            let error = self.set_direc_attribs(d);
            if error != super::PK_OK {
                ndirs_fail += 1;
                let mut m = b"warning:  set times/attribs failed for ".to_vec();
                m.extend_from_slice(&d.fname);
                m.push(b'\n');
                self.info(0x201, m);
                if *error_in_archive == super::PK_OK {
                    *error_in_archive = error;
                }
            }
        }
        if self.o.tflag == 0 && self.o.qflag == 0 && ndirs_fail > 0 {
            self.info(0, format!("     failed setting times/attribs for {ndirs_fail} dir entries"));
        }
    }
}

impl Uz {
    /// Processa os membros de um lote (`extract_or_test_entrylist`): posiciona no cabeçalho local
    /// (com as tentativas de recompensar bytes extras), confere nome e tamanhos contra o central,
    /// decide sobrescrita e extrai ou testa.
    fn extract_or_test_entrylist(&mut self, infos: Vec<super::process::MinInfo>, st: &mut ListState, mut error_in_archive: i32) -> i32 {
        use super::{MSG_STDERR, PK_BOMB, PK_OK};
        for info in infos {
            st.filnum += 1;
            self.pinfo = info;
            let request = self.pinfo.offset + self.zin.extra_bytes;
            if self.x.cover.within(request) {
                self.info(MSG_STDERR, "error: invalid zip file with overlapped components (possible zip bomb)\n");
                return PK_BOMB;
            }
            let Some(request) = self.seek_local_header(request, st, &mut error_in_archive) else { continue };
            match self.check_local_header(st, &mut error_in_archive) {
                Ok(true) => {}
                Ok(false) => continue,
                Err(e) => return e,
            }
            if self.pinfo.encrypted {
                let error = self.decrypt_member();
                if error != PK_OK {
                    if error == super::PK_WARN {
                        self.skip_msg("incorrect password");
                        st.num_bad_pwd += 1;
                    } else {
                        if error > error_in_archive {
                            error_in_archive = error;
                        }
                        let mut m = b"   skipping: ".to_vec();
                        m.extend(pad22(&super::fileio::fnfilter(&self.filename)));
                        m.extend_from_slice(b"  unable to get password\n");
                        self.info(MSG_STDERR, m);
                    }
                    continue;
                }
            }
            if self.o.tflag == 0 && !self.o.cflag && !self.prepare_output(st, &mut error_in_archive) {
                continue;
            }
            self.x.disk_full = 0;
            let error = self.extract_or_test_member();
            if error != PK_OK {
                if error > error_in_archive {
                    error_in_archive = error;
                }
                if self.x.disk_full > 1 {
                    return error_in_archive;
                }
            }
            let end = self.zin.tell();
            match self.x.cover.add(request, end) {
                0 => {}
                r if r < 0 => {
                    self.info(MSG_STDERR, "error: not enough memory for bomb detection\n");
                    return super::PK_MEM;
                }
                _ => {
                    self.info(MSG_STDERR, "error: invalid zip file with overlapped components (possible zip bomb)\n");
                    return PK_BOMB;
                }
            }
        }
        error_in_archive
    }

    /// Vai ao cabeçalho local do membro e lê a assinatura. Um offset negativo ou uma assinatura
    /// errada no primeiro membro de um zip com bytes extras disparam uma nova tentativa sem eles
    /// (e, no membro seguinte, com eles de volta). `None` pula o membro.
    fn seek_local_header(&mut self, mut request: i64, st: &mut ListState, eia: &mut i32) -> Option<i64> {
        use super::{MSG_STDERR, PK_BADERR, PK_ERR, PK_OK};
        const RECOMPENSATE: &str = "  (attempting to re-compensate)\n";
        if request < 0 {
            self.seek_msg();
            *eia = PK_ERR;
            if st.filnum == 1 && self.zin.extra_bytes != 0 {
                self.info(MSG_STDERR, RECOMPENSATE);
                st.old_extra_bytes = self.zin.extra_bytes;
                self.zin.extra_bytes = 0;
                request = self.pinfo.offset;
                if request < 0 {
                    self.seek_msg();
                    *eia = PK_BADERR;
                    return None;
                }
            } else {
                *eia = PK_BADERR;
                return None;
            }
        }
        if !self.zin.seek_raw(request) {
            let bufstart = request - request % super::process::INBUFSIZ as i64;
            self.offset_msg(st.filnum, "lseek", bufstart);
            *eia = PK_BADERR;
            return None;
        }
        let mut sig = [0u8; 4];
        if self.readbuf(&mut sig) == 0 {
            self.offset_msg(st.filnum, "EOF", request);
            *eia = PK_BADERR;
            return None;
        }
        if &sig != super::process::LOCAL_HDR_SIG {
            self.offset_msg(st.filnum, "local header sig", request);
            *eia = PK_ERR;
            let xb = self.zin.extra_bytes;
            if !((st.filnum == 1 && xb != 0) || (xb == 0 && st.old_extra_bytes != 0)) {
                return None;
            }
            self.info(MSG_STDERR, RECOMPENSATE);
            if xb != 0 {
                st.old_extra_bytes = xb;
                self.zin.extra_bytes = 0;
            } else {
                self.zin.extra_bytes = st.old_extra_bytes;
            }
            let error = self.seek_zipf(self.pinfo.offset);
            if error != PK_OK || self.readbuf(&mut sig) == 0 {
                if error != PK_BADERR {
                    self.offset_msg(st.filnum, "EOF", request);
                }
                *eia = PK_BADERR;
                return None;
            }
            if &sig != super::process::LOCAL_HDR_SIG {
                self.offset_msg(st.filnum, "local header sig", request);
                *eia = PK_BADERR;
                return None;
            }
        }
        Some(request)
    }

    /// Lê e confere o cabeçalho local: bit 11 contra o central, nome, campo extra, nome contra o
    /// central (vale o central) e, no STORED, o tamanho descomprimido contra o comprimido. `Ok(false)`
    /// pula o membro; `Err` encerra o lote com aquele código.
    fn check_local_header(&mut self, st: &ListState, eia: &mut i32) -> Result<bool, i32> {
        use super::fileio::fnfilter;
        use super::{PK_ERR, PK_OK, PK_WARN};
        let error = self.process_local_file_hdr();
        if error != PK_OK {
            self.info(0x421, format!("file #{}:  bad local header\n", st.filnum));
            *eia = error;
            return Ok(false);
        }
        if (self.lrec.general_purpose_bit_flag & (1 << 11) != 0) != self.pinfo.gpf_is_utf8 {
            if self.o.qflag == 0 {
                let cname = self.pinfo.cfilname.clone().unwrap_or_default();
                let mut m = format!("file #{} (", st.filnum).into_bytes();
                m.extend(fnfilter(&cname));
                m.extend_from_slice(b"):\n         mismatch between local and central GPF bit 11 (\"UTF-8\"),\n");
                m.extend_from_slice(format!("         continuing with central flag (IsUTF8 = {})\n", i32::from(self.pinfo.gpf_is_utf8)).as_bytes());
                self.info(0x421, m);
            }
            if *eia < PK_WARN {
                *eia = PK_WARN;
            }
        }
        let error = self.read_filename(usize::from(self.lrec.filename_length), true);
        if error != PK_OK {
            if error > *eia {
                *eia = error;
            }
            if error > PK_WARN {
                self.bad_length_msg("filename", "local");
                return Ok(false);
            }
        }
        let error = self.read_extra_field(usize::from(self.lrec.extra_field_length));
        if error != PK_OK {
            if error > *eia {
                *eia = error;
            }
            if error > PK_WARN {
                self.bad_length_msg("extra field", "local");
                return Ok(false);
            }
        }
        // O nome só se compara depois do campo extra, que pode trazer o nome Unicode.
        if let Some(cname) = self.pinfo.cfilname.take()
            && cname != self.filename {
                let mut m = fnfilter(&cname);
                m.extend_from_slice(b":  mismatching \"local\" filename (");
                m.extend(fnfilter(&self.filename));
                m.extend_from_slice(b"),\n         continuing with \"central\" filename version\n");
                self.info(0x401, m);
                self.filename = cname;
                if *eia < PK_WARN {
                    *eia = PK_WARN;
                }
            }
        // E os tamanhos só depois do campo extra, que pode trazer o bloco Zip64.
        if self.lrec.compression_method == STORED {
            let mut csiz = self.lrec.csize;
            if self.pinfo.encrypted {
                if csiz < 12 {
                    self.info(0x401, "\n  error:  invalid compressed data to inflate\n");
                    return Err(PK_ERR);
                }
                csiz -= 12;
            }
            if self.lrec.ucsize != csiz {
                let mut m = fnfilter(&self.filename);
                m.extend_from_slice(format!(":  ucsize {} <> csize {csiz} for STORED entry\n         continuing with \"compressed\" size value\n", self.lrec.ucsize).as_bytes());
                self.info(0x401, m);
                self.lrec.ucsize = csiz;
                if *eia < PK_WARN {
                    *eia = PK_WARN;
                }
            }
        }
        Ok(true)
    }

    /// `"NOME:  bad <campo> length (local)\n"`.
    fn bad_length_msg(&mut self, what: &str, which: &str) {
        let mut m = super::fileio::fnfilter(&self.filename);
        m.extend_from_slice(format!(":  bad {what} length ({which})\n").as_bytes());
        self.info(0x401, m);
    }

    /// Antes de extrair pro disco: corrige a barra invertida dos zips do FAT, tira o caminho
    /// absoluto, mapeia o nome (criando diretórios) e decide a sobrescrita, perguntando quando
    /// for o caso. `false` pula o membro.
    fn prepare_output(&mut self, st: &mut ListState, eia: &mut i32) -> bool {
        use super::fileio::fnfilter;
        use super::unix::{MPN_CREATED_DIR, MPN_INF_SKIP, MPN_INF_TRUNC, MPN_MASK, MPN_VOL_LABEL};
        use super::{PK_ERR, PK_OK, PK_WARN};
        let mut renamed = false;
        'startover: loop {
            if self.pinfo.hostnum == super::process::FS_FAT && !self.filename.contains(&b'/') {
                for i in 0..self.filename.len() {
                    if self.filename[i] == b'\\' {
                        if !self.x.reported_backslash {
                            let m = [b"warning:  ".as_slice(), &self.zipfn, b" appears to use backslashes as path separators\n"].concat();
                            self.info(0x21, m);
                            self.x.reported_backslash = true;
                            if *eia == PK_OK {
                                *eia = PK_WARN;
                            }
                        }
                        self.filename[i] = b'/';
                    }
                }
            }
            if !renamed && self.filename.first() == Some(&b'/') {
                let mut m = b"warning:  stripped absolute path spec from ".to_vec();
                m.extend(fnfilter(&self.filename));
                m.push(b'\n');
                self.info(0x401, m);
                if *eia == PK_OK {
                    *eia = PK_WARN;
                }
                let n = self.filename.iter().take_while(|&&c| c == b'/').count();
                self.filename.drain(..n);
            }
            let error = self.mapname(renamed);
            let errcode = error & !MPN_MASK;
            if errcode != PK_OK && *eia < errcode {
                *eia = errcode;
            }
            let errcode = error & MPN_MASK;
            if errcode > MPN_INF_TRUNC {
                if errcode == MPN_CREATED_DIR {
                    let d = self.defer_dir_attribs();
                    st.dirs.push(d);
                } else if errcode == MPN_VOL_LABEL {
                    let mut m = b"   skipping: ".to_vec();
                    m.extend(pad22(&fnfilter(&self.filename)));
                    m.extend_from_slice(b"  volume label\n");
                    self.info(0x1, m);
                } else if errcode > MPN_INF_SKIP && *eia < PK_ERR {
                    *eia = PK_ERR;
                }
                return false;
            }
            let query = match self.check_for_newer() {
                Newer::DoesNotExist => {
                    if self.o.fflag && !renamed {
                        return false;
                    }
                    false
                }
                Newer::ExistsAndOlder => {
                    if self.o.b_flag {
                        false
                    } else if self.x.overwrite_mode == Overwrite::Never {
                        return false;
                    } else {
                        self.x.overwrite_mode != Overwrite::Always
                    }
                }
                Newer::ExistsNotOlder => {
                    if (!self.o.b_flag && self.x.overwrite_mode == Overwrite::Never) || (self.o.uflag && !renamed) {
                        return false;
                    }
                    self.x.overwrite_mode != Overwrite::Always && !self.o.b_flag
                }
            };
            if !query {
                return true;
            }
            loop {
                let mut m = b"replace ".to_vec();
                m.extend(fnfilter(&self.filename));
                m.extend_from_slice(b"? [y]es, [n]o, [A]ll, [N]one, [r]ename: ");
                self.info(0x81, m);
                let mut answer = match self.fgets(10) {
                    Some(a) => a,
                    None => {
                        self.info(0x1, " NULL\n(EOF or read error, treating as \"[N]one\" ...)\n");
                        if *eia == PK_OK {
                            *eia = PK_WARN;
                        }
                        // O C só troca o primeiro byte do buffer.
                        let mut a = std::mem::take(&mut self.x.answerbuf);
                        if a.is_empty() {
                            a.push(b'N');
                        } else {
                            a[0] = b'N';
                        }
                        a
                    }
                };
                self.x.answerbuf = answer.clone();
                match answer[0] {
                    b'r' | b'R' => {
                        let name = loop {
                            self.info(0x81, "new name: ");
                            // No fim do stdin o `fgets` do C não toca no buffer: fica o nome atual.
                            let Some(mut n) = self.fgets(super::fileio::FILNAMSIZ) else { break self.filename.clone() };
                            if n.last() == Some(&b'\n') {
                                n.pop();
                            }
                            // Como o `strlen` do C, o nome para no primeiro NUL.
                            n.truncate(n.iter().position(|&c| c == 0).unwrap_or(n.len()));
                            if !n.is_empty() {
                                break n;
                            }
                        };
                        self.filename = name;
                        renamed = true;
                        continue 'startover;
                    }
                    b'A' => {
                        self.x.overwrite_mode = Overwrite::Always;
                        return true;
                    }
                    b'y' | b'Y' => return true,
                    b'N' => {
                        self.x.overwrite_mode = Overwrite::Never;
                        return false;
                    }
                    b'n' => return false,
                    c => {
                        if c == b'\n' || c == b'\r' {
                            answer = b"{ENTER}".to_vec();
                        }
                        if answer.last() == Some(&b'\n') {
                            answer.pop();
                        }
                        let a = &answer[..answer.iter().position(|&c| c == 0).unwrap_or(answer.len())];
                        let m = [b"error:  invalid response [".as_slice(), a, b"]\n"].concat();
                        self.info(0x1, m);
                    }
                }
            }
        }
    }

    /// Se o arquivo de saída já existe e é tão novo quanto o membro (`check_for_newer`). Um link
    /// simbólico conta sempre como mais velho.
    fn check_for_newer(&mut self) -> Newer {
        use super::process::{ef_scan_for_izux, EB_UT_FL_MTIME};
        let symlink_msg = |g: &mut Uz, tail: &[u8]| {
            if g.o.qflag == 0 && g.x.overwrite_mode != Overwrite::Always {
                let mut m = super::fileio::fnfilter(&g.filename);
                m.extend_from_slice(b" exists and is a symbolic link");
                m.extend_from_slice(tail);
                m.extend_from_slice(b".\n");
                g.info(0, m);
            }
        };
        let st = match sysabi::sys::stat(&self.filename) {
            Ok(st) => st,
            Err(_) => {
                if sysabi::sys::lstat(&self.filename).is_ok() {
                    symlink_msg(self, b" with no real file");
                    return Newer::ExistsAndOlder;
                }
                return Newer::DoesNotExist;
            }
        };
        if sysabi::sys::lstat(&self.filename).is_ok_and(|l| l.file_type() == sysabi::FileType::Symlink) {
            symlink_msg(self, b"");
            return Newer::ExistsAndOlder;
        }
        let dos = self.lrec.last_mod_dos_datetime;
        let ut = self.extra_field.as_ref().map(|ef| {
            let ef = &ef[..ef.len().min(usize::from(self.lrec.extra_field_length))];
            ef_scan_for_izux(ef, false, dos, true, false)
        });
        let mtime = st.mtime.sec;
        let (existing, archive) = match ut {
            Some((flags, t, _)) if flags & EB_UT_FL_MTIME != 0 => (mtime, t.mtime),
            _ => (if mtime & 1 != 0 { mtime.saturating_add(1) } else { mtime }, self.dos_to_unix_time(dos)),
        };
        if existing >= archive { Newer::ExistsNotOlder } else { Newer::ExistsAndOlder }
    }

    /// `"error [ZIP]:  attempt to seek before beginning of zipfile\n"` e o pedido de conferir a
    /// transferência.
    fn seek_msg(&mut self) {
        let m = [b"error [".as_slice(), &self.zipfn, b"]:  attempt to seek before beginning of zipfile\n", super::text::REPORT_MSG.as_bytes()].concat();
        self.info(super::MSG_STDERR, m);
    }

    /// `"file #N:  bad zipfile offset (o que falhou):  OFFSET\n"`.
    fn offset_msg(&mut self, filnum: u64, what: &str, off: i64) {
        self.info(super::MSG_STDERR, format!("file #{filnum}:  bad zipfile offset ({what}):  {off}\n"));
    }
}

impl Uz {
    /// Extrai ou testa os membros (`extract_or_test_files`): lê o diretório central em lotes, vai
    /// aos dados de cada lote, depois termina links simbólicos e diretórios adiados e faz o resumo.
    pub fn extract_or_test_files(&mut self) -> i32 {
        use super::{MSG_STDERR, PK_BADERR, PK_BOMB, PK_FIND, PK_OK, PK_WARN, IZ_UNSUP, IZ_BADPWD};
        let mut error_in_archive = PK_OK;
        if let Some(r) = self.prepare_exdir() {
            return r;
        }
        if let Some(r) = self.init_cover() {
            return r;
        }
        self.x.newzip = true;
        self.x.reported_backslash = false;
        let sepc = self.o.w_flag.then_some(b'/');
        let mut fn_matched = vec![false; self.pfnames.len()];
        let mut xn_matched = vec![false; self.pxnames.len()];
        let mut st = ListState::default();
        let mut members_processed: u64 = 0;
        let mut num_skipped: u64 = 0;
        let mut blknum: u64 = 0;
        let mut no_endsig_found = false;
        let mut reached_end = false;
        while !reached_end {
            let mut infos: Vec<super::process::MinInfo> = Vec::new();
            while infos.len() < DIR_BLKSIZ {
                let mut sig = [0u8; 4];
                if self.readbuf(&mut sig) == 0 {
                    error_in_archive = super::PK_EOF;
                    reached_end = true;
                    break;
                }
                self.sig = sig;
                if &sig != super::process::CENTRAL_HDR_SIG {
                    let mask = if self.ecrec.have_ecr64 { u64::MAX } else { 0xffff };
                    if members_processed & mask == self.ecrec.total_entries_central_dir {
                        no_endsig_found = !self.at_end_sig();
                    } else {
                        let n = infos.len() as u64 + blknum * DIR_BLKSIZ as u64 + 1;
                        self.info(MSG_STDERR, format!("error:  expected central file header signature not found (file #{n}).\n"));
                        self.info(MSG_STDERR, super::text::REPORT_MSG);
                        error_in_archive = PK_BADERR;
                    }
                    reached_end = true;
                    break;
                }
                let error = self.process_cdir_file_hdr();
                if error != PK_OK {
                    error_in_archive = error;
                    reached_end = true;
                    break;
                }
                if !self.read_central_strings(&mut error_in_archive) {
                    reached_end = true;
                    break;
                }
                let mut do_this_file = true;
                if !self.process_all_files {
                    if !self.pfnames.is_empty() {
                        do_this_file = false;
                        for (i, p) in self.pfnames.iter().enumerate() {
                            if super::matching::matches(&self.filename, p, self.o.c_flag, sepc) {
                                do_this_file = true;
                                fn_matched[i] = true;
                                break;
                            }
                        }
                    }
                    if do_this_file {
                        for (i, p) in self.pxnames.iter().enumerate() {
                            if super::matching::matches(&self.filename, p, self.o.c_flag, sepc) {
                                do_this_file = false;
                                xn_matched[i] = true;
                                break;
                            }
                        }
                    }
                }
                if do_this_file {
                    if self.store_info() {
                        infos.push(self.pinfo.clone());
                    } else {
                        num_skipped += 1;
                    }
                }
                members_processed += 1;
            }
            let (cd_bufstart, cd_inptr, cd_incnt) = (self.zin.bufstart, self.zin.inptr, self.zin.incnt);
            let error = self.extract_or_test_entrylist(infos, &mut st, error_in_archive);
            if error != PK_OK {
                if error > error_in_archive {
                    error_in_archive = error;
                }
                if self.x.disk_full > 1 || error == PK_BOMB {
                    reached_end = false;
                    break;
                }
            }
            self.zin.restore(cd_bufstart, cd_inptr, cd_incnt);
            blknum += 1;
        }
        self.finish_deferred(&mut st, &mut error_in_archive);
        if reached_end {
            for (i, hit) in fn_matched.iter().enumerate() {
                if !hit {
                    let m = [super::text::FILENAME_NOT_MATCHED.as_bytes(), &self.pfnames[i], b"\n"].concat();
                    self.info(0x1, m);
                    if error_in_archive <= PK_WARN {
                        error_in_archive = PK_FIND;
                    }
                }
            }
            for (i, hit) in xn_matched.iter().enumerate() {
                if !hit {
                    let m = [super::text::EXCL_FILENAME_NOT_MATCHED.as_bytes(), &self.pxnames[i], b"\n"].concat();
                    self.info(MSG_STDERR, m);
                }
            }
        }
        if !reached_end {
            return error_in_archive;
        }
        if no_endsig_found {
            self.info(MSG_STDERR, "\nnote:  didn't find end-of-central-dir signature at end of central dir.\n");
            self.info(MSG_STDERR, super::text::REPORT_MSG);
            if error_in_archive == PK_OK {
                error_in_archive = PK_WARN;
            }
        }
        let (filnum, num_bad_pwd) = (st.filnum, st.num_bad_pwd);
        let plural = |n: u64| if n == 1 { "" } else { "s" };
        if self.o.tflag != 0 && self.o.qflag < 2 {
            let num = filnum - num_bad_pwd;
            let z = |pre: &str, post: &str| [pre.as_bytes(), &self.zipfn, post.as_bytes()].concat();
            let line = if error_in_archive != PK_OK {
                let w = if error_in_archive == PK_WARN { "warning-" } else { "" };
                z(&format!("At least one {w}error was detected in "), ".\n")
            } else if num == 0 {
                z("Caution:  zero files tested in ", ".\n")
            } else if self.process_all_files && num_skipped + num_bad_pwd == 0 {
                z("No errors detected in compressed data of ", ".\n")
            } else {
                z("No errors detected in ", &format!(" for the {num} file{} tested.\n", plural(num)))
            };
            self.info(0, line);
            if num_skipped > 0 {
                self.info(0, format!("{num_skipped} file{} skipped because of unsupported compression or encoding.\n", plural(num_skipped)));
            }
            if num_bad_pwd > 0 {
                self.info(0, format!("{num_bad_pwd} file{} skipped because of incorrect password.\n", plural(num_bad_pwd)));
            }
        }
        if filnum == 0 && error_in_archive <= PK_WARN {
            error_in_archive = if num_skipped > 0 { IZ_UNSUP } else { PK_FIND };
        } else if filnum == num_bad_pwd && error_in_archive <= PK_WARN {
            error_in_archive = IZ_BADPWD;
        } else if num_skipped > 0 && error_in_archive <= PK_WARN {
            error_in_archive = IZ_UNSUP;
        } else if num_bad_pwd > 0 && error_in_archive == PK_OK {
            error_in_archive = PK_WARN;
        }
        error_in_archive
    }
}

impl Uz {}
