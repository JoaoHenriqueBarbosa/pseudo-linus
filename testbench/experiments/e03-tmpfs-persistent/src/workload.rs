//! Gerador determinístico de imagens base e de alterações por sandbox.

use crate::fs::{Fs, Ino};
use crate::maps::Flavor;
use crate::vfs::Vfs;

/// SplitMix64: determinístico, rápido, sem dependência.
#[derive(Clone)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    pub fn below(&mut self, n: u64) -> u64 {
        self.next_u64() % n.max(1)
    }

    pub fn fill(&mut self, buf: &mut [u8]) {
        for chunk in buf.chunks_mut(8) {
            let x = self.next_u64().to_le_bytes();
            chunk.copy_from_slice(&x[..chunk.len()]);
        }
    }

    /// Tamanho log-uniforme em `[1, max]`: muitos arquivos pequenos, poucos grandes.
    pub fn log_size(&mut self, max: u64) -> u64 {
        let u = (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64;
        ((max as f64).ln() * u).exp().round().clamp(1.0, max as f64) as u64
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ImageSpec {
    pub files: usize,
    pub files_per_dir: usize,
    pub dirs_per_top: usize,
    pub max_file_size: u64,
    /// Profundidade da cadeia `/deep/l1/.../lN`, com um arquivo `f` em cada nível.
    pub deep: usize,
    pub seed: u64,
}

impl ImageSpec {
    pub fn with_files(files: usize) -> ImageSpec {
        ImageSpec { files, files_per_dir: 64, dirs_per_top: 32, max_file_size: 8192, deep: 32, seed: 0xe03 }
    }
}

pub struct Image<F: Flavor> {
    pub fs: Fs<F>,
    /// Caminhos dos arquivos comuns, na ordem de criação.
    pub files: Vec<Vec<u8>>,
    /// `deep_files[d - 1]` é o arquivo na profundidade `d` (contando `/deep` como 1).
    pub deep_files: Vec<Vec<u8>>,
    pub content_bytes: u64,
    pub inodes: usize,
}

/// Monta a imagem: `/d<i>/s<j>/f<k>` com `files_per_dir` arquivos por diretório folha, mais a
/// cadeia funda. Conteúdo pseudoaleatório, tamanho log-uniforme até `max_file_size`.
pub fn build_image<F: Flavor>(spec: &ImageSpec) -> Image<F> {
    let mut vfs = Vfs::<F>::new();
    let mut rng = Rng::new(spec.seed);
    let mut files = Vec::with_capacity(spec.files);
    let mut content_bytes = 0;
    let mut buf = vec![0u8; spec.max_file_size as usize];
    let leaf_dirs = spec.files.div_ceil(spec.files_per_dir);
    for leaf in 0..leaf_dirs {
        let top = leaf / spec.dirs_per_top;
        let sub = leaf % spec.dirs_per_top;
        if sub == 0 {
            vfs.mkdir(format!("/d{top}").as_bytes(), 0o755).expect("mkdir top");
        }
        let dir = format!("/d{top}/s{sub}");
        vfs.mkdir(dir.as_bytes(), 0o755).expect("mkdir leaf");
        let count = spec.files_per_dir.min(spec.files - leaf * spec.files_per_dir);
        for k in 0..count {
            let path = format!("{dir}/f{k}.txt").into_bytes();
            let size = rng.log_size(spec.max_file_size) as usize;
            rng.fill(&mut buf[..size]);
            vfs.create(&path, 0o644).expect("create");
            vfs.write(&path, 0, &buf[..size]).expect("write");
            content_bytes += size as u64;
            files.push(path);
        }
    }
    let mut deep_files = Vec::with_capacity(spec.deep);
    let mut dir = String::from("/deep");
    vfs.mkdir(dir.as_bytes(), 0o755).expect("mkdir deep");
    for level in 1..=spec.deep {
        if level > 1 {
            dir.push_str(&format!("/l{level}"));
            vfs.mkdir(dir.as_bytes(), 0o755).expect("mkdir deep level");
        }
        let path = format!("{dir}/f").into_bytes();
        vfs.create(&path, 0o644).expect("create deep");
        vfs.write(&path, 0, b"profundo\n").expect("write deep");
        content_bytes += 9;
        deep_files.push(path);
    }
    let fs = vfs.into_fs();
    let inodes = fs.inode_count();
    Image { fs, files, deep_files, content_bytes, inodes }
}

/// Diretório com `n` arquivos vazios (`/big/e<k>`), pra medir diretório grande.
pub fn build_big_dir<F: Flavor>(n: usize) -> Fs<F> {
    let mut vfs = Vfs::<F>::new();
    vfs.mkdir(b"/big", 0o755).expect("mkdir big");
    for k in 0..n {
        vfs.create(format!("/big/e{k}").as_bytes(), 0o644).expect("create big");
    }
    vfs.into_fs()
}

/// Imagem com um arquivo `/huge` de `size` bytes (escrito em pedaços de 1 MiB).
pub fn build_huge_file<F: Flavor>(size: usize) -> Fs<F> {
    let mut vfs = Vfs::<F>::new();
    vfs.create(b"/huge", 0o644).expect("create huge");
    let mut rng = Rng::new(7);
    let mut chunk = vec![0u8; 1 << 20];
    let mut off = 0;
    while off < size {
        let n = chunk.len().min(size - off);
        rng.fill(&mut chunk[..n]);
        vfs.write(b"/huge", off as u64, &chunk[..n]).expect("write huge");
        off += n;
    }
    vfs.into_fs()
}

/// Resumo das alterações aplicadas numa sandbox derivada.
#[derive(Clone, Copy, Debug, Default)]
pub struct ChangeStats {
    pub ops: usize,
    pub errors: usize,
}

/// "Poucas alterações" de uma sandbox de agente: um diretório de trabalho com 10 arquivos novos
/// de 1 KiB, 10 escritas de 100 bytes em arquivos da imagem, 2 remoções e 1 rename.
pub fn apply_changes<F: Flavor>(vfs: &mut Vfs<F>, files: &[Vec<u8>], id: usize, rng: &mut Rng) -> ChangeStats {
    let mut st = ChangeStats::default();
    let mut track = |r: bool| {
        st.ops += 1;
        if !r {
            st.errors += 1;
        }
    };
    let work = format!("/work{id}");
    track(vfs.mkdir(work.as_bytes(), 0o755).is_ok());
    let mut data = vec![0u8; 1024];
    for k in 0..10 {
        let p = format!("{work}/n{k}").into_bytes();
        rng.fill(&mut data);
        track(vfs.create(&p, 0o644).is_ok());
        track(vfs.write(&p, 0, &data).is_ok());
    }
    let mut picked: Vec<usize> = Vec::new();
    while picked.len() < 13 {
        let i = rng.below(files.len() as u64) as usize;
        if !picked.contains(&i) {
            picked.push(i);
        }
    }
    for &i in &picked[..10] {
        let size = vfs.stat(&files[i]).map(|s| s.size).unwrap_or(0);
        let off = rng.below(size.max(1));
        track(vfs.write(&files[i], off, &data[..100]).is_ok());
    }
    for &i in &picked[10..12] {
        track(vfs.unlink(&files[i]).is_ok());
    }
    track(vfs.rename(&files[picked[12]], format!("{work}/moved").as_bytes()).is_ok());
    st
}

/// Inodes de um caminho (pra testes).
pub fn ino_of<F: Flavor>(vfs: &Vfs<F>, path: &[u8]) -> Option<Ino> {
    vfs.stat(path).ok().map(|s| s.ino)
}
