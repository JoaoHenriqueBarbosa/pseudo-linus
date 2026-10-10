//! O `list.sort` do CPython 3.13 (`Objects/listobject.c`): corridas naturais (`count_run`), inserção binária
//! (`binarysort`), a estratégia de fusão do powersort (`found_new_run`, `merge_force_collapse`), as fusões com
//! galope (`merge_lo`, `merge_hi`, `gallop_left`, `gallop_right`). Só se usa `<`, na mesma ordem e com os mesmos
//! pares do CPython, então um `__lt__` de usuário que conta ou registra as chamadas vê a mesma sequência; e, se a
//! comparação levanta, o que sobra no vetor é a mesma permutação parcial.

use crate::object::Value;
use crate::vm::PyResult;

/// (chave, item): a ordenação compara só a chave e move o par inteiro.
pub(crate) type Pair = (Value, Value);

/// A comparação `<` entre duas chaves.
pub(crate) type Lt<'a> = &'a mut dyn FnMut(&Value, &Value) -> PyResult<bool>;

const MIN_GALLOP: usize = 7;
const MAX_MINRUN: usize = 64;

struct Run {
    base: usize,
    len: usize,
    power: u32,
}

struct MergeState {
    min_gallop: usize,
    pending: Vec<Run>,
    listlen: usize,
}

/// Ordena `a` (estável) pela chave de cada par.
pub(crate) fn sort(a: &mut [Pair], lt: Lt) -> PyResult<()> {
    let n = a.len();
    if n < 2 {
        return Ok(());
    }
    let mut ms = MergeState { min_gallop: MIN_GALLOP, pending: Vec::new(), listlen: n };
    let minrun = compute_minrun(n);
    let mut lo = 0;
    let mut nremaining = n;
    loop {
        let mut run = count_run(&mut a[lo..], lt)?;
        if run < minrun {
            let force = nremaining.min(minrun);
            binarysort(&mut a[lo..lo + force], run, lt)?;
            run = force;
        }
        found_new_run(&mut ms, a, run, lt)?;
        ms.pending.push(Run { base: lo, len: run, power: 0 });
        lo += run;
        nremaining -= run;
        if nremaining == 0 {
            break;
        }
    }
    while ms.pending.len() > 1 {
        let mut i = ms.pending.len() - 2;
        if i > 0 && ms.pending[i - 1].len < ms.pending[i + 1].len {
            i -= 1;
        }
        merge_at(&mut ms, a, i, lt)?;
    }
    Ok(())
}

fn compute_minrun(mut n: usize) -> usize {
    let mut r = 0;
    while n >= MAX_MINRUN {
        r |= n & 1;
        n >>= 1;
    }
    n + r
}

/// Inserção binária estável de `a[ok..]` em `a[..ok]`, que já está ordenado.
fn binarysort(a: &mut [Pair], ok: usize, lt: Lt) -> PyResult<()> {
    let n = a.len();
    for at in ok.max(1)..n {
        let mut l = 0;
        let mut r = at;
        loop {
            let m = (l + r) >> 1;
            if lt(&a[at].0, &a[m].0)? {
                r = m;
            } else {
                l = m + 1;
            }
            if l >= r {
                break;
            }
        }
        a[l..=at].rotate_right(1);
    }
    Ok(())
}

/// O tamanho da corrida que começa em `lo[0]` (todo `lo` é o que resta), já em ordem crescente.
fn count_run(lo: &mut [Pair], lt: Lt) -> PyResult<usize> {
    let nremaining = lo.len();
    let mut n = 1;
    while n < nremaining {
        if lt(&lo[n].0, &lo[n - 1].0)? {
            break;
        }
        n += 1;
    }
    if n == nremaining {
        return Ok(n);
    }
    if n > 1 {
        if lt(&lo[0].0, &lo[n - 1].0)? {
            return Ok(n);
        }
        lo[..n].reverse();
    }
    n += 1;
    // A corrida descendente; as subcorridas de iguais se invertem na hora e a inversão total as restaura.
    let mut neq = 0;
    let reverse_last_neq = |lo: &mut [Pair], n: usize, neq: &mut usize| {
        if *neq > 0 {
            *neq += 1;
            lo[n - *neq..n].reverse();
            *neq = 0;
        }
    };
    while n < nremaining {
        if lt(&lo[n].0, &lo[n - 1].0)? {
            reverse_last_neq(lo, n, &mut neq);
        } else if lt(&lo[n - 1].0, &lo[n].0)? {
            break;
        } else {
            neq += 1;
        }
        n += 1;
    }
    reverse_last_neq(lo, n, &mut neq);
    lo[..n].reverse();
    while n < nremaining {
        if lt(&lo[n].0, &lo[n - 1].0)? {
            break;
        }
        n += 1;
    }
    Ok(n)
}

/// `a[k-1] < key <= a[k]`: onde `key` entra em `a` (ordenado), partindo de `hint`.
fn gallop_left(key: &Value, a: &[Pair], hint: usize, lt: Lt) -> PyResult<usize> {
    let n = a.len() as isize;
    let hint = hint as isize;
    let mut ofs: isize = 1;
    let mut lastofs: isize = 0;
    if lt(&a[hint as usize].0, key)? {
        let maxofs = n - hint;
        while ofs < maxofs {
            if lt(&a[(hint + ofs) as usize].0, key)? {
                lastofs = ofs;
                ofs = (ofs << 1) + 1;
            } else {
                break;
            }
        }
        if ofs > maxofs {
            ofs = maxofs;
        }
        lastofs += hint;
        ofs += hint;
    } else {
        let maxofs = hint + 1;
        while ofs < maxofs {
            if lt(&a[(hint - ofs) as usize].0, key)? {
                break;
            }
            lastofs = ofs;
            ofs = (ofs << 1) + 1;
        }
        if ofs > maxofs {
            ofs = maxofs;
        }
        let k = lastofs;
        lastofs = hint - ofs;
        ofs = hint - k;
    }
    lastofs += 1;
    while lastofs < ofs {
        let m = lastofs + ((ofs - lastofs) >> 1);
        if lt(&a[m as usize].0, key)? {
            lastofs = m + 1;
        } else {
            ofs = m;
        }
    }
    Ok(ofs as usize)
}

/// Como [`gallop_left`], mas `a[k-1] <= key < a[k]`: à direita do último igual.
fn gallop_right(key: &Value, a: &[Pair], hint: usize, lt: Lt) -> PyResult<usize> {
    let n = a.len() as isize;
    let hint = hint as isize;
    let mut ofs: isize = 1;
    let mut lastofs: isize = 0;
    if lt(key, &a[hint as usize].0)? {
        let maxofs = hint + 1;
        while ofs < maxofs {
            if lt(key, &a[(hint - ofs) as usize].0)? {
                lastofs = ofs;
                ofs = (ofs << 1) + 1;
            } else {
                break;
            }
        }
        if ofs > maxofs {
            ofs = maxofs;
        }
        let k = lastofs;
        lastofs = hint - ofs;
        ofs = hint - k;
    } else {
        let maxofs = n - hint;
        while ofs < maxofs {
            if lt(key, &a[(hint + ofs) as usize].0)? {
                break;
            }
            lastofs = ofs;
            ofs = (ofs << 1) + 1;
        }
        if ofs > maxofs {
            ofs = maxofs;
        }
        lastofs += hint;
        ofs += hint;
    }
    lastofs += 1;
    while lastofs < ofs {
        let m = lastofs + ((ofs - lastofs) >> 1);
        if lt(key, &a[m as usize].0)? {
            ofs = m;
        } else {
            lastofs = m + 1;
        }
    }
    Ok(ofs as usize)
}

/// Funde as corridas `i` e `i + 1` da pilha.
fn merge_at(ms: &mut MergeState, a: &mut [Pair], i: usize, lt: Lt) -> PyResult<()> {
    let mut ba = ms.pending[i].base;
    let mut na = ms.pending[i].len;
    let bb = ms.pending[i + 1].base;
    let mut nb = ms.pending[i + 1].len;
    ms.pending[i].len = na + nb;
    if i + 3 == ms.pending.len() {
        ms.pending[i + 1] = Run { base: ms.pending[i + 2].base, len: ms.pending[i + 2].len, power: ms.pending[i + 2].power };
    }
    ms.pending.pop();
    // Onde `b` começa em `a`: o que vem antes já está no lugar.
    let key = a[bb].0.clone();
    let k = gallop_right(&key, &a[ba..ba + na], 0, lt)?;
    ba += k;
    na -= k;
    if na == 0 {
        return Ok(());
    }
    // Onde `a` termina em `b`: o que vem depois já está no lugar.
    let key = a[ba + na - 1].0.clone();
    nb = gallop_left(&key, &a[bb..bb + nb], nb - 1, lt)?;
    if nb == 0 {
        return Ok(());
    }
    if na <= nb {
        merge_lo(ms, a, ba, na, nb, lt)
    } else {
        merge_hi(ms, a, ba, na, nb, lt)
    }
}

enum Tail {
    /// Sobra de `A` (ou nada) a copiar para o fim.
    Rest,
    /// Só resta o último de `A`, que vai para o fim da fusão.
    Last,
}

/// Funde `na` de `a[ba..]` com os `nb` seguintes, com `na <= nb`; `A` vai para uma temporária.
fn merge_lo(ms: &mut MergeState, a: &mut [Pair], ba: usize, mut na: usize, mut nb: usize, lt: Lt) -> PyResult<()> {
    let temp: Vec<Pair> = a[ba..ba + na].to_vec();
    let mut dest = ba;
    let mut ia = 0;
    let mut ib = ba + na;
    let tail = (|| -> PyResult<Tail> {
        a[dest] = a[ib].clone();
        dest += 1;
        ib += 1;
        nb -= 1;
        if nb == 0 {
            return Ok(Tail::Rest);
        }
        if na == 1 {
            return Ok(Tail::Last);
        }
        let mut min_gallop = ms.min_gallop;
        loop {
            let mut acount = 0;
            let mut bcount = 0;
            // O trivial até uma corrida vencer sempre.
            loop {
                if lt(&a[ib].0, &temp[ia].0)? {
                    a[dest] = a[ib].clone();
                    dest += 1;
                    ib += 1;
                    bcount += 1;
                    acount = 0;
                    nb -= 1;
                    if nb == 0 {
                        return Ok(Tail::Rest);
                    }
                    if bcount >= min_gallop {
                        break;
                    }
                } else {
                    a[dest] = temp[ia].clone();
                    dest += 1;
                    ia += 1;
                    acount += 1;
                    bcount = 0;
                    na -= 1;
                    if na == 1 {
                        return Ok(Tail::Last);
                    }
                    if acount >= min_gallop {
                        break;
                    }
                }
            }
            min_gallop += 1;
            loop {
                if min_gallop > 1 {
                    min_gallop -= 1;
                }
                ms.min_gallop = min_gallop;
                let key = a[ib].0.clone();
                let k = gallop_right(&key, &temp[ia..ia + na], 0, lt)?;
                acount = k;
                if k > 0 {
                    a[dest..dest + k].clone_from_slice(&temp[ia..ia + k]);
                    dest += k;
                    ia += k;
                    na -= k;
                    if na == 1 {
                        return Ok(Tail::Last);
                    }
                    if na == 0 {
                        return Ok(Tail::Rest);
                    }
                }
                a[dest] = a[ib].clone();
                dest += 1;
                ib += 1;
                nb -= 1;
                if nb == 0 {
                    return Ok(Tail::Rest);
                }
                let key = temp[ia].0.clone();
                let k = gallop_left(&key, &a[ib..ib + nb], 0, lt)?;
                bcount = k;
                if k > 0 {
                    for j in 0..k {
                        a[dest + j] = a[ib + j].clone();
                    }
                    dest += k;
                    ib += k;
                    nb -= k;
                    if nb == 0 {
                        return Ok(Tail::Rest);
                    }
                }
                a[dest] = temp[ia].clone();
                dest += 1;
                ia += 1;
                na -= 1;
                if na == 1 {
                    return Ok(Tail::Last);
                }
                if !(acount >= MIN_GALLOP || bcount >= MIN_GALLOP) {
                    break;
                }
            }
            min_gallop += 1;
            ms.min_gallop = min_gallop;
        }
    })();
    match tail {
        Ok(Tail::Last) => {
            for j in 0..nb {
                a[dest + j] = a[ib + j].clone();
            }
            a[dest + nb] = temp[ia].clone();
            Ok(())
        }
        other => {
            if na > 0 {
                a[dest..dest + na].clone_from_slice(&temp[ia..ia + na]);
            }
            other.map(|_| ())
        }
    }
}

/// Como [`merge_lo`], com `na >= nb`; `B` vai para a temporária e a fusão corre de trás para a frente.
fn merge_hi(ms: &mut MergeState, a: &mut [Pair], ba: usize, mut na: usize, mut nb: usize, lt: Lt) -> PyResult<()> {
    let bb = ba + na;
    let temp: Vec<Pair> = a[bb..bb + nb].to_vec();
    // Os índices descem até -1: `isize`.
    let mut dest = (bb + nb) as isize - 1;
    let mut ia = (ba + na) as isize - 1;
    let mut ib = nb as isize - 1;
    let tail = (|| -> PyResult<Tail> {
        a[dest as usize] = a[ia as usize].clone();
        dest -= 1;
        ia -= 1;
        na -= 1;
        if na == 0 {
            return Ok(Tail::Rest);
        }
        if nb == 1 {
            return Ok(Tail::Last);
        }
        let mut min_gallop = ms.min_gallop;
        loop {
            let mut acount = 0;
            let mut bcount = 0;
            loop {
                if lt(&temp[ib as usize].0, &a[ia as usize].0)? {
                    a[dest as usize] = a[ia as usize].clone();
                    dest -= 1;
                    ia -= 1;
                    acount += 1;
                    bcount = 0;
                    na -= 1;
                    if na == 0 {
                        return Ok(Tail::Rest);
                    }
                    if acount >= min_gallop {
                        break;
                    }
                } else {
                    a[dest as usize] = temp[ib as usize].clone();
                    dest -= 1;
                    ib -= 1;
                    bcount += 1;
                    acount = 0;
                    nb -= 1;
                    if nb == 1 {
                        return Ok(Tail::Last);
                    }
                    if bcount >= min_gallop {
                        break;
                    }
                }
            }
            min_gallop += 1;
            loop {
                if min_gallop > 1 {
                    min_gallop -= 1;
                }
                ms.min_gallop = min_gallop;
                let key = temp[ib as usize].0.clone();
                let k = na - gallop_right(&key, &a[ba..ba + na], na - 1, lt)?;
                acount = k;
                if k > 0 {
                    dest -= k as isize;
                    ia -= k as isize;
                    for j in (1..=k).rev() {
                        a[dest as usize + j] = a[ia as usize + j].clone();
                    }
                    na -= k;
                    if na == 0 {
                        return Ok(Tail::Rest);
                    }
                }
                a[dest as usize] = temp[ib as usize].clone();
                dest -= 1;
                ib -= 1;
                nb -= 1;
                if nb == 1 {
                    return Ok(Tail::Last);
                }
                let key = a[ia as usize].0.clone();
                let k = nb - gallop_left(&key, &temp[..nb], nb - 1, lt)?;
                bcount = k;
                if k > 0 {
                    dest -= k as isize;
                    ib -= k as isize;
                    for j in 1..=k {
                        a[dest as usize + j] = temp[ib as usize + j].clone();
                    }
                    nb -= k;
                    if nb == 1 {
                        return Ok(Tail::Last);
                    }
                    if nb == 0 {
                        return Ok(Tail::Rest);
                    }
                }
                a[dest as usize] = a[ia as usize].clone();
                dest -= 1;
                ia -= 1;
                na -= 1;
                if na == 0 {
                    return Ok(Tail::Rest);
                }
                if !(acount >= MIN_GALLOP || bcount >= MIN_GALLOP) {
                    break;
                }
            }
            min_gallop += 1;
            ms.min_gallop = min_gallop;
        }
    })();
    match tail {
        Ok(Tail::Last) => {
            // O primeiro de `B` vai para o começo da fusão; os `na` de `A` descem uma casa cada.
            for j in (0..na).rev() {
                a[(dest + 1 - na as isize) as usize + j] = a[(ia + 1 - na as isize) as usize + j].clone();
            }
            dest -= na as isize;
            a[dest as usize] = temp[ib as usize].clone();
            Ok(())
        }
        other => {
            if nb > 0 {
                let start = (dest - (nb as isize - 1)) as usize;
                a[start..start + nb].clone_from_slice(&temp[..nb]);
            }
            other.map(|_| ())
        }
    }
}

/// A corrida nova, de tamanho `n2`, acaba de ser achada: funde as da pilha de poder maior (powersort).
fn found_new_run(ms: &mut MergeState, a: &mut [Pair], n2: usize, lt: Lt) -> PyResult<()> {
    let Some(top) = ms.pending.last() else { return Ok(()) };
    let power = powerloop(top.base, top.len, n2, ms.listlen);
    while ms.pending.len() > 1 && ms.pending[ms.pending.len() - 2].power > power {
        let i = ms.pending.len() - 2;
        merge_at(ms, a, i, lt)?;
    }
    if let Some(top) = ms.pending.last_mut() {
        top.power = power;
    }
    Ok(())
}

/// O "poder" da primeira de duas corridas vizinhas: a profundidade dela na árvore de fusão conceitual.
fn powerloop(s1: usize, n1: usize, n2: usize, n: usize) -> u32 {
    let mut result = 0;
    let mut a = 2 * s1 + n1;
    let mut b = a + n1 + n2;
    loop {
        result += 1;
        if a >= n {
            a -= n;
            b -= n;
        } else if b >= n {
            break;
        }
        a <<= 1;
        b <<= 1;
    }
    result
}
