//! Motor de alinhamento: decide quais linhas de cada arquivo mudaram.
//!
//! O núcleo é o algoritmo publicado por Eugene W. Myers ("An O(ND) Difference Algorithm and Its
//! Variations", Algorithmica 1986), na versão de espaço linear: busca simultânea do começo pro fim e do
//! fim pro começo até os caminhos se encontrarem, divisão no ponto de encontro e recursão nas duas
//! metades. Em volta dele, as etapas que dão ao resultado a forma que o GNU diffutils produz, escritas a
//! partir do comportamento observado no oráculo (e calibradas pelo corpus aleatório do F08):
//!
//! 1. O prefixo e o sufixo idênticos (byte a byte) ficam fora da comparação, menos `horizon` linhas de
//!    cada lado, que entram pra que o deslizamento final tenha onde se mover.
//! 2. Linhas que não aparecem no outro arquivo são descartadas antes da busca (já se sabe que mudaram);
//!    linhas que aparecem demais no outro arquivo são descartes provisórios, mantidos só no meio de uma
//!    sequência longa de descartes.
//! 3. A busca roda nas linhas que sobraram; nas diagonais, do maior índice pro menor, primeiro o passo
//!    de cima pra baixo e depois o de baixo pra cima. Sem `--minimal`, quando o custo passa de um limite
//!    (raiz aproximada do tamanho, no mínimo 4096) a busca escolhe a diagonal que mais avançou.
//! 4. Cada bloco de mudança desliza: sobe enquanto a linha anterior é igual à última do bloco (juntando
//!    com blocos anteriores), desce enquanto a primeira é igual à seguinte (juntando com os próximos), e
//!    por fim volta pra última posição em que o fim do bloco coincidia com um bloco mudado no outro
//!    arquivo.

/// Parâmetros do motor.
#[derive(Clone, Copy, Debug, Default)]
pub struct Options {
    /// `-d`: sem a heurística de custo.
    pub minimal: bool,
    /// `-H`: heurística extra pra arquivos grandes com mudanças espalhadas.
    pub speed_large_files: bool,
    /// Linhas do prefixo e do sufixo comuns mantidas na comparação (`--horizon-lines`, no mínimo o
    /// contexto do formato).
    pub horizon: usize,
}

/// Vetores `changed` de cada arquivo: `true` = linha apagada (no primeiro) ou inserida (no segundo).
pub struct Alignment {
    pub changed_a: Vec<bool>,
    pub changed_b: Vec<bool>,
}

/// Compara dois arquivos. `la`/`lb` são as linhas cruas (pro prefixo e sufixo exatos); `ca`/`cb` as
/// classes de equivalência sob a normalização escolhida; `nclasses` o total de classes.
pub fn compare(la: &[&[u8]], lb: &[&[u8]], ca: &[u32], cb: &[u32], nclasses: usize, opts: Options) -> Alignment {
    let (n0, n1) = (ca.len(), cb.len());
    let mut changed_a = vec![false; n0];
    let mut changed_b = vec![false; n1];

    // 1. Prefixo e sufixo idênticos.
    let mut prefix = 0usize;
    while prefix < n0 && prefix < n1 && la[prefix] == lb[prefix] {
        prefix += 1;
    }
    let mut suffix = 0usize;
    while suffix < n0 - prefix && suffix < n1 - prefix && la[n0 - 1 - suffix] == lb[n1 - 1 - suffix] {
        suffix += 1;
    }
    let lo = prefix - prefix.min(opts.horizon);
    let tail = suffix - suffix.min(opts.horizon);
    let (hi0, hi1) = (n0 - tail, n1 - tail);
    if lo == hi0 && lo == hi1 {
        return Alignment { changed_a, changed_b };
    }
    let ra = &ca[lo..hi0];
    let rb = &cb[lo..hi1];

    // 2. Descartes.
    let mut count_a = vec![0usize; nclasses];
    let mut count_b = vec![0usize; nclasses];
    for &c in ra {
        count_a[c as usize] += 1;
    }
    for &c in rb {
        count_b[c as usize] += 1;
    }
    let disc_a = discards(ra, &count_b);
    let disc_b = discards(rb, &count_a);

    // 3. Busca nas linhas que sobraram.
    let mut ra_changed = vec![false; ra.len()];
    let mut rb_changed = vec![false; rb.len()];
    let mut xv = Vec::new();
    let mut xidx = Vec::new();
    for (i, &c) in ra.iter().enumerate() {
        if disc_a[i] {
            ra_changed[i] = true;
        } else {
            xv.push(c);
            xidx.push(i);
        }
    }
    let mut yv = Vec::new();
    let mut yidx = Vec::new();
    for (i, &c) in rb.iter().enumerate() {
        if disc_b[i] {
            rb_changed[i] = true;
        } else {
            yv.push(c);
            yidx.push(i);
        }
    }
    let mut search = Search::new(&xv, &yv, opts);
    search.run();
    for (k, &ch) in search.changed_x.iter().enumerate() {
        if ch {
            ra_changed[xidx[k]] = true;
        }
    }
    for (k, &ch) in search.changed_y.iter().enumerate() {
        if ch {
            rb_changed[yidx[k]] = true;
        }
    }

    // 4. Deslizamento dos blocos, primeiro no primeiro arquivo, depois no segundo.
    shift_boundaries(ra, &mut ra_changed, &rb_changed);
    shift_boundaries(rb, &mut rb_changed, &ra_changed);

    changed_a[lo..hi0].copy_from_slice(&ra_changed);
    changed_b[lo..hi1].copy_from_slice(&rb_changed);
    Alignment { changed_a, changed_b }
}

/// Limite de "muitas ocorrências": 5, dobrado a cada vez que `n / 64` ainda tem bits depois de andar
/// dois pra direita.
fn many_threshold(n: usize) -> usize {
    let mut many = 5usize;
    let mut t = n / 64;
    loop {
        t >>= 2;
        if t == 0 {
            break;
        }
        many *= 2;
    }
    many
}

const KEEP: u8 = 0;
const SURE: u8 = 1;
const MAYBE: u8 = 2;

/// Decide os descartes de um arquivo. `other_count[c]` é quantas vezes a classe `c` aparece no outro.
fn discards(lines: &[u32], other_count: &[usize]) -> Vec<bool> {
    let n = lines.len();
    let many = many_threshold(n);
    let mut mark: Vec<u8> = lines
        .iter()
        .map(|&c| match other_count[c as usize] {
            0 => SURE,
            k if k > many => MAYBE,
            _ => KEEP,
        })
        .collect();

    let mut i = 0usize;
    while i < n {
        match mark[i] {
            MAYBE => {
                // Provisório fora de uma sequência que começa num descarte certo: fica.
                mark[i] = KEEP;
            }
            SURE => refine_run(&mut mark, &mut i),
            _ => {}
        }
        i += 1;
    }
    mark.into_iter().map(|m| m != KEEP).collect()
}

/// Ajusta uma sequência de descartes que começa em `*i` (um descarte certo). Ao voltar, `*i` aponta pra
/// última linha da sequência examinada.
fn refine_run(mark: &mut [u8], i: &mut usize) {
    let n = mark.len();
    let start = *i;
    let mut end = start;
    let mut provisional = 0usize;
    while end < n && mark[end] != KEEP {
        if mark[end] == MAYBE {
            provisional += 1;
        }
        end += 1;
    }
    // Provisórios no fim da sequência não ficam.
    while end > start && mark[end - 1] == MAYBE {
        end -= 1;
        mark[end] = KEEP;
        provisional -= 1;
    }
    let len = end - start;
    if provisional * 4 > len {
        // Provisórios demais: nenhum fica.
        for m in &mut mark[start..end] {
            if *m == MAYBE {
                *m = KEEP;
            }
        }
    } else {
        // Uma sub-sequência longa de provisórios (comprimento mínimo cresce com a sequência) não fica.
        let mut minimum = 1usize;
        let mut t = len >> 2;
        loop {
            t >>= 2;
            if t == 0 {
                break;
            }
            minimum <<= 1;
        }
        minimum += 1;
        let mut consec = 0usize;
        let mut k = 0usize;
        while k < len {
            if mark[start + k] != MAYBE {
                consec = 0;
            } else {
                consec += 1;
                if consec == minimum {
                    // Volta pro começo da sub-sequência pra desfazer ela inteira.
                    k -= consec;
                } else if consec > minimum {
                    mark[start + k] = KEEP;
                }
            }
            k += 1;
        }
        // Das pontas pra dentro, provisórios só ficam depois de três descartes certos seguidos ou de um
        // descarte certo a partir da oitava linha.
        let mut consec = 0usize;
        for k in 0..len {
            let p = start + k;
            if k >= 8 && mark[p] == SURE {
                break;
            }
            match mark[p] {
                MAYBE => {
                    consec = 0;
                    mark[p] = KEEP;
                }
                KEEP => consec = 0,
                _ => consec += 1,
            }
            if consec == 3 {
                break;
            }
        }
        let last = start + len - 1;
        let mut consec = 0usize;
        for k in 0..len {
            let p = last - k;
            if k >= 8 && mark[p] == SURE {
                break;
            }
            match mark[p] {
                MAYBE => {
                    consec = 0;
                    mark[p] = KEEP;
                }
                KEEP => consec = 0,
                _ => consec += 1,
            }
            if consec == 3 {
                break;
            }
        }
    }
    *i = start + len.max(1) - 1;
}

/// Comprimento de snake que conta como "grande" pra heurística do `-H`.
const SNAKE_LIMIT: isize = 20;

struct Partition {
    xmid: isize,
    ymid: isize,
    lo_minimal: bool,
    hi_minimal: bool,
}

/// Estado da busca de Myers sobre as linhas não descartadas.
struct Search<'a> {
    xv: &'a [u32],
    yv: &'a [u32],
    /// Vetores das diagonais (índice deslocado por `off`): x mais avançado de cada diagonal.
    fd: Vec<isize>,
    bd: Vec<isize>,
    off: isize,
    too_expensive: isize,
    heuristic: bool,
    minimal: bool,
    changed_x: Vec<bool>,
    changed_y: Vec<bool>,
    steps: u64,
}

impl<'a> Search<'a> {
    fn new(xv: &'a [u32], yv: &'a [u32], opts: Options) -> Search<'a> {
        let (n, m) = (xv.len(), yv.len());
        let size = n + m + 3;
        let mut too_expensive: isize = 1;
        let mut diags = n + m + 3;
        while diags != 0 {
            too_expensive <<= 1;
            diags >>= 2;
        }
        Search {
            xv,
            yv,
            fd: vec![0; size],
            bd: vec![0; size],
            off: m as isize + 1,
            too_expensive: too_expensive.max(4096),
            heuristic: opts.speed_large_files,
            minimal: opts.minimal,
            changed_x: vec![false; n],
            changed_y: vec![false; m],
            steps: 0,
        }
    }

    fn fd(&self, d: isize) -> isize {
        self.fd[(d + self.off) as usize]
    }

    fn set_fd(&mut self, d: isize, v: isize) {
        let k = (d + self.off) as usize;
        self.fd[k] = v;
    }

    fn bd(&self, d: isize) -> isize {
        self.bd[(d + self.off) as usize]
    }

    fn set_bd(&mut self, d: isize, v: isize) {
        let k = (d + self.off) as usize;
        self.bd[k] = v;
    }

    fn eq(&self, x: isize, y: isize) -> bool {
        self.xv[x as usize] == self.yv[y as usize]
    }

    fn tick(&mut self) {
        self.steps += 1;
        if self.steps % 4096 == 0 {
            sysabi::sys::checkpoint();
        }
    }

    fn run(&mut self) {
        let (n, m) = (self.xv.len() as isize, self.yv.len() as isize);
        let minimal = self.minimal;
        // Pilha explícita no lugar da recursão: (xoff, xlim, yoff, ylim, find_minimal).
        let mut stack: Vec<(isize, isize, isize, isize, bool)> = vec![(0, n, 0, m, minimal)];
        while let Some((mut xoff, mut xlim, mut yoff, mut ylim, find_minimal)) = stack.pop() {
            self.tick();
            while xoff < xlim && yoff < ylim && self.eq(xoff, yoff) {
                xoff += 1;
                yoff += 1;
            }
            while xlim > xoff && ylim > yoff && self.eq(xlim - 1, ylim - 1) {
                xlim -= 1;
                ylim -= 1;
            }
            if xoff == xlim {
                for y in yoff..ylim {
                    self.changed_y[y as usize] = true;
                }
            } else if yoff == ylim {
                for x in xoff..xlim {
                    self.changed_x[x as usize] = true;
                }
            } else {
                let p = self.diag(xoff, xlim, yoff, ylim, find_minimal);
                // A metade de cima é processada primeiro (fica no topo da pilha).
                stack.push((p.xmid, xlim, p.ymid, ylim, p.hi_minimal));
                stack.push((xoff, p.xmid, yoff, p.ymid, p.lo_minimal));
            }
        }
    }

    /// Acha o ponto de divisão do trecho: o fim do snake de cima pra baixo ou o começo do snake de baixo
    /// pra cima em que os dois caminhos se encontram.
    fn diag(&mut self, xoff: isize, xlim: isize, yoff: isize, ylim: isize, find_minimal: bool) -> Partition {
        let dmin = xoff - ylim;
        let dmax = xlim - yoff;
        let fmid = xoff - yoff;
        let bmid = xlim - ylim;
        let (mut fmin, mut fmax) = (fmid, fmid);
        let (mut bmin, mut bmax) = (bmid, bmid);
        let odd = (fmid - bmid) & 1 != 0;
        self.set_fd(fmid, xoff);
        self.set_bd(bmid, xlim);
        let mut c: isize = 1;
        loop {
            self.tick();
            let mut big_snake = false;
            // Um passo de cima pra baixo em cada diagonal.
            if fmin > dmin {
                fmin -= 1;
                self.set_fd(fmin - 1, -1);
            } else {
                fmin += 1;
            }
            if fmax < dmax {
                fmax += 1;
                self.set_fd(fmax + 1, -1);
            } else {
                fmax -= 1;
            }
            let mut d = fmax;
            while d >= fmin {
                let tlo = self.fd(d - 1);
                let thi = self.fd(d + 1);
                let x0 = if tlo >= thi { tlo + 1 } else { thi };
                let mut x = x0;
                let mut y = x0 - d;
                while x < xlim && y < ylim && self.eq(x, y) {
                    x += 1;
                    y += 1;
                }
                if x - x0 > SNAKE_LIMIT {
                    big_snake = true;
                }
                self.set_fd(d, x);
                if odd && bmin <= d && d <= bmax && self.bd(d) <= x {
                    return Partition { xmid: x, ymid: y, lo_minimal: true, hi_minimal: true };
                }
                d -= 2;
            }
            // Um passo de baixo pra cima.
            if bmin > dmin {
                bmin -= 1;
                self.set_bd(bmin - 1, isize::MAX);
            } else {
                bmin += 1;
            }
            if bmax < dmax {
                bmax += 1;
                self.set_bd(bmax + 1, isize::MAX);
            } else {
                bmax -= 1;
            }
            let mut d = bmax;
            while d >= bmin {
                let tlo = self.bd(d - 1);
                let thi = self.bd(d + 1);
                let x0 = if tlo < thi { tlo } else { thi - 1 };
                let mut x = x0;
                let mut y = x0 - d;
                while x > xoff && y > yoff && self.eq(x - 1, y - 1) {
                    x -= 1;
                    y -= 1;
                }
                if x0 - x > SNAKE_LIMIT {
                    big_snake = true;
                }
                self.set_bd(d, x);
                if !odd && fmin <= d && d <= fmax && x <= self.fd(d) {
                    return Partition { xmid: x, ymid: y, lo_minimal: true, hi_minimal: true };
                }
                d -= 2;
            }

            if find_minimal {
                c += 1;
                continue;
            }

            // Heurística do `-H`: depois de 200 passos, se houve um snake grande, aceita a diagonal que
            // mais avançou desde que ela tenha um snake longo o bastante.
            if self.heuristic && c > 200 && big_snake {
                if let Some(p) = self.big_snake_split(xoff, xlim, yoff, ylim, fmin, fmax, bmin, bmax, c) {
                    return p;
                }
            }

            // Custo alto demais: corta na diagonal que mais avançou.
            if c >= self.too_expensive {
                let mut fxybest = -1isize;
                let mut fxbest = 0isize;
                let mut d = fmax;
                while d >= fmin {
                    let mut x = self.fd(d).min(xlim);
                    let mut y = x - d;
                    if ylim < y {
                        x = ylim + d;
                        y = ylim;
                    }
                    if fxybest < x + y {
                        fxybest = x + y;
                        fxbest = x;
                    }
                    d -= 2;
                }
                let mut bxybest = isize::MAX;
                let mut bxbest = 0isize;
                let mut d = bmax;
                while d >= bmin {
                    let mut x = self.bd(d).max(xoff);
                    let mut y = x - d;
                    if y < yoff {
                        x = yoff + d;
                        y = yoff;
                    }
                    if x + y < bxybest {
                        bxybest = x + y;
                        bxbest = x;
                    }
                    d -= 2;
                }
                return if (xlim + ylim) - bxybest < fxybest - (xoff + yoff) {
                    Partition { xmid: fxbest, ymid: fxybest - fxbest, lo_minimal: true, hi_minimal: false }
                } else {
                    Partition { xmid: bxbest, ymid: bxybest - bxbest, lo_minimal: false, hi_minimal: true }
                };
            }
            c += 1;
        }
    }

    /// Heurística de snake grande (`-H`): procura, nas duas buscas, uma diagonal que avançou bem mais que
    /// a média e termina num snake de pelo menos `SNAKE_LIMIT` linhas.
    #[allow(clippy::too_many_arguments)]
    fn big_snake_split(
        &self,
        xoff: isize,
        xlim: isize,
        yoff: isize,
        ylim: isize,
        fmin: isize,
        fmax: isize,
        bmin: isize,
        bmax: isize,
        c: isize,
    ) -> Option<Partition> {
        let mut best = 0isize;
        let mut part = None;
        let mut d = fmax;
        while d >= fmin {
            let dd = d - (xoff - yoff);
            let x = self.fd(d);
            let y = x - d;
            let v = (x - xoff) + (y - yoff) - dd;
            if v > 12 * (c + dd.abs()) && v > best && xoff + SNAKE_LIMIT <= x && x < xlim && yoff + SNAKE_LIMIT <= y && y < ylim {
                let mut k = 1;
                while k <= SNAKE_LIMIT && self.eq(x - k, y - k) {
                    k += 1;
                }
                if k > SNAKE_LIMIT {
                    best = v;
                    part = Some(Partition { xmid: x, ymid: y, lo_minimal: true, hi_minimal: false });
                }
            }
            d -= 2;
        }
        if part.is_some() {
            return part;
        }
        best = 0;
        let mut d = bmax;
        while d >= bmin {
            let dd = d - (xlim - ylim);
            let x = self.bd(d);
            let y = x - d;
            let v = (xlim - x) + (ylim - y) + dd;
            if v > 12 * (c + dd.abs()) && v > best && xoff < x && x <= xlim - SNAKE_LIMIT && yoff < y && y <= ylim - SNAKE_LIMIT {
                let mut k = 0;
                while k < SNAKE_LIMIT && self.eq(x + k, y + k) {
                    k += 1;
                }
                if k == SNAKE_LIMIT {
                    best = v;
                    part = Some(Partition { xmid: x, ymid: y, lo_minimal: false, hi_minimal: true });
                }
            }
            d -= 2;
        }
        part
    }
}

/// Desliza os blocos de mudança de um arquivo (ver o cabeçalho do módulo). `other` é o vetor do outro
/// arquivo, usado pra manter a correspondência entre as linhas iguais.
fn shift_boundaries(eq: &[u32], changed: &mut [bool], other: &[bool]) {
    let n = changed.len();
    let on = other.len();
    // Acesso com sentinelas: fora do vetor é "não mudou".
    let ch = |v: &[bool], i: isize| -> bool { i >= 0 && (i as usize) < v.len() && v[i as usize] };
    let mut i: isize = 0;
    let mut j: isize = 0;
    let end = n as isize;
    loop {
        // Acha o começo do próximo bloco, acompanhando a linha correspondente no outro arquivo.
        while i < end && !changed[i as usize] {
            while ch(other, j) {
                j += 1;
            }
            j += 1;
            i += 1;
        }
        if i >= end {
            break;
        }
        let mut start = i;
        i += 1;
        while ch(changed, i) {
            i += 1;
        }
        while ch(other, j) {
            j += 1;
        }
        let mut corresponding;
        loop {
            let runlength = i - start;
            // Sobe enquanto a linha anterior é igual à última do bloco.
            while start > 0 && eq[(start - 1) as usize] == eq[(i - 1) as usize] {
                start -= 1;
                changed[start as usize] = true;
                i -= 1;
                changed[i as usize] = false;
                while ch(changed, start - 1) {
                    start -= 1;
                }
                j -= 1;
                while ch(other, j) {
                    j -= 1;
                }
            }
            corresponding = if ch(other, j - 1) { i } else { end };
            // Desce enquanto a primeira linha do bloco é igual à seguinte.
            while i != end && eq[start as usize] == eq[i as usize] {
                changed[start as usize] = false;
                start += 1;
                changed[i as usize] = true;
                i += 1;
                while ch(changed, i) {
                    i += 1;
                }
                j += 1;
                while ch(other, j) {
                    corresponding = i;
                    j += 1;
                }
            }
            if runlength == i - start {
                break;
            }
        }
        // Volta pra posição em que o fim do bloco coincidia com um bloco do outro arquivo.
        while corresponding < i {
            start -= 1;
            changed[start as usize] = true;
            i -= 1;
            changed[i as usize] = false;
            j -= 1;
            while ch(other, j) {
                j -= 1;
            }
        }
        let _ = on;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(a: &[u32], b: &[u32]) -> (Vec<bool>, Vec<bool>) {
        let la: Vec<Vec<u8>> = a.iter().map(|c| format!("{c}\n").into_bytes()).collect();
        let lb: Vec<Vec<u8>> = b.iter().map(|c| format!("{c}\n").into_bytes()).collect();
        let ra: Vec<&[u8]> = la.iter().map(|v| v.as_slice()).collect();
        let rb: Vec<&[u8]> = lb.iter().map(|v| v.as_slice()).collect();
        let n = a.iter().chain(b).max().map(|m| *m as usize + 1).unwrap_or(0);
        let al = compare(&ra, &rb, a, b, n, Options::default());
        (al.changed_a, al.changed_b)
    }

    fn valid(a: &[u32], b: &[u32], c0: &[bool], c1: &[bool]) -> bool {
        let ka: Vec<u32> = a.iter().zip(c0).filter(|(_, c)| !**c).map(|(x, _)| *x).collect();
        let kb: Vec<u32> = b.iter().zip(c1).filter(|(_, c)| !**c).map(|(x, _)| *x).collect();
        ka == kb
    }

    #[test]
    fn simple_change_is_minimal_and_valid() {
        let a = [0u32, 1, 2, 3];
        let b = [0u32, 4, 2, 3, 5];
        let (c0, c1) = run(&a, &b);
        assert!(valid(&a, &b, &c0, &c1));
        assert_eq!(c0, vec![false, true, false, false]);
        assert_eq!(c1, vec![false, true, false, false, true]);
    }

    #[test]
    fn duplicate_deletion_slides_down() {
        // "x x" contra "x": o GNU apaga a segunda linha (2d1).
        let (c0, c1) = run(&[0, 0], &[0]);
        assert_eq!(c0, vec![false, true]);
        assert!(c1.iter().all(|c| !c));
    }

    #[test]
    fn random_pairs_are_valid_and_minimal_ish() {
        let mut s = 12345u64;
        let mut next = || {
            s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (s >> 33) as u32
        };
        for _ in 0..300 {
            let la = (next() % 30) as usize;
            let lb = (next() % 30) as usize;
            let a: Vec<u32> = (0..la).map(|_| next() % 4).collect();
            let b: Vec<u32> = (0..lb).map(|_| next() % 4).collect();
            let (c0, c1) = run(&a, &b);
            assert!(valid(&a, &b, &c0, &c1), "{a:?} {b:?}");
        }
    }
}
