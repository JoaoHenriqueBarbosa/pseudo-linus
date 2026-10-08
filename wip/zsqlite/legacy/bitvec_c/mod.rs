// Mesclado das partes traduzidas de bitvec_c (as partes quebram funções grandes no meio).
#![allow(unused_imports, ambiguous_glob_reexports)]
use crate::prelude::*;

// ---- part_000.rs ----

/// Este arquivo implementa um objeto que representa um bitmap de comprimento
/// fixo. Os bits são numerados começando com 1.
///
/// Um bitmap é usado para registrar quais páginas de um arquivo de banco de
/// dados foram registradas em diário durante uma transação, ou quais páginas
/// têm a propriedade "não escrever". Geralmente poucas páginas atendem a
/// qualquer das condições, então o bitmap costuma ser esparso e de baixa
/// cardinalidade. Mas às vezes (por exemplo, num DROP de uma tabela grande) a
/// maioria ou todas as páginas podem ser registradas; aí o bitmap fica denso.
/// O algoritmo precisa lidar bem com os dois casos.
///
/// O tamanho do bitmap é fixado quando o objeto é criado. Todos os bits
/// começam limpos e podem ser definidos ou limpos um de cada vez.

/// Tamanho da estrutura Bitvec em bytes.
pub const BITVEC_SZ: usize = 512;

/// Tamanho da union arredondado para baixo no limite de ponteiro (8 bytes, o
/// `sizeof(Bitvec*)` do Debian 13 amd64), já que é assim que ela fica alinhada
/// dentro da struct Bitvec: ((512 - 3*4) / 8) * 8 = 496.
pub const BITVEC_USIZE: usize = ((BITVEC_SZ - (3 * 4)) / 8) * 8;

/// Tipo do elemento do array da representação em bitmap.
pub type BitvecTelem = u8;
/// Tamanho, em bits, do elemento do bitmap.
pub const BITVEC_SZELEM: usize = 8;
/// Número de elementos no array do bitmap.
pub const BITVEC_NELEM: usize = BITVEC_USIZE / 1;
/// Número de bits no array do bitmap.
pub const BITVEC_NBIT: usize = BITVEC_NELEM * BITVEC_SZELEM;

/// Número de valores u32 na tabela hash.
pub const BITVEC_NINT: usize = BITVEC_USIZE / 4;
/// Número máximo de entradas na tabela hash antes de subdividir e refazer o hash.
pub const BITVEC_MXHASH: usize = BITVEC_NINT / 2;

/// Função de hash da representação aHash.
#[inline]
pub fn bitvec_hash(x: u32) -> usize {
    (x as usize) % BITVEC_NINT
}

/// Número de ponteiros para sub-bitmaps na representação recursiva.
pub const BITVEC_NPTR: usize = BITVEC_USIZE / 8;

/// A union do C: uma das três representações possíveis do bitmap.
///
/// Se i_size <= BITVEC_NBIT, vale `Bitmap` (bitmap direto, o bit menos
/// significativo é o bit 1). Se i_size > BITVEC_NBIT e i_divisor == 0, vale
/// `Hash` (tabela hash de até BITVEC_MXHASH valores distintos). Caso
/// contrário vale `Sub` (BITVEC_NPTR sub-bitmaps, cada um com i_divisor valores).
#[derive(Clone)]
pub enum BitvecUnion {
    Bitmap(Vec<BitvecTelem>),
    Hash(Vec<u32>),
    Sub(Vec<Option<Box<Bitvec>>>),
}

/// Um bitmap com valores entre 1 e i_size, inclusive.
#[derive(Clone)]
pub struct Bitvec {
    /// Índice de bit máximo.
    pub i_size: u32,
    /// Número de bits definidos, válido só para a representação hash.
    pub n_set: u32,
    /// Número de bits tratados por cada entrada de `Sub`.
    pub i_divisor: u32,
    pub u: BitvecUnion,
}

/// Cria um novo bitmap capaz de tratar bits entre 0 e i_size, inclusive.
pub fn bitvec_create(i_size: u32) -> Option<Box<Bitvec>> {
    let u = if (i_size as usize) <= BITVEC_NBIT {
        BitvecUnion::Bitmap(vec![0; BITVEC_NELEM])
    } else {
        BitvecUnion::Hash(vec![0; BITVEC_NINT])
    };
    Some(Box::new(Bitvec {
        i_size,
        n_set: 0,
        i_divisor: 0,
        u,
    }))
}

/// Verifica se o i-ésimo bit está definido, com p garantidamente não nulo.
/// Fora do intervalo retorna 0.
pub fn bitvec_test_not_null(p: &Bitvec, i: u32) -> i32 {
    let mut p = p;
    let mut i = i.wrapping_sub(1);
    if i >= p.i_size {
        return 0;
    }
    while p.i_divisor != 0 {
        let bin = (i / p.i_divisor) as usize;
        i %= p.i_divisor;
        let BitvecUnion::Sub(subs) = &p.u else {
            return 0;
        };
        match subs.get(bin) {
            Some(Some(sub)) => p = sub,
            _ => return 0,
        }
    }
    if (p.i_size as usize) <= BITVEC_NBIT {
        let BitvecUnion::Bitmap(bitmap) = &p.u else {
            return 0;
        };
        ((bitmap[i as usize / BITVEC_SZELEM] & (1u8 << (i as usize & (BITVEC_SZELEM - 1)))) != 0) as i32
    } else {
        let BitvecUnion::Hash(hash) = &p.u else {
            return 0;
        };
        let mut h = bitvec_hash(i);
        i = i.wrapping_add(1);
        while hash[h] != 0 {
            if hash[h] == i {
                return 1;
            }
            h = (h + 1) % BITVEC_NINT;
        }
        0
    }
}

/// Verifica se o i-ésimo bit está definido. Se p é None (bitmap não criado)
/// ou i está fora do intervalo, retorna 0.
pub fn bitvec_test(p: Option<&Bitvec>, i: u32) -> i32 {
    match p {
        Some(p) => bitvec_test_not_null(p, i),
        None => 0,
    }
}

/// Define o i-ésimo bit. Retorna 0 em sucesso e um código de erro se algo
/// der errado. Pode alocar sub-bitmaps. O chamador garante que p é válido e
/// que i está no intervalo do Bitvec.
pub fn bitvec_set(p: Option<&mut Bitvec>, i: u32) -> i32 {
    let Some(mut p) = p else {
        return SQLITE_OK;
    };
    let mut i = i.wrapping_sub(1);
    while (p.i_size as usize) > BITVEC_NBIT && p.i_divisor != 0 {
        let divisor = p.i_divisor;
        let bin = (i / divisor) as usize;
        i %= divisor;
        let BitvecUnion::Sub(subs) = &mut p.u else {
            unreachable!("Bitvec com divisor sem representação Sub");
        };
        if subs[bin].is_none() {
            subs[bin] = bitvec_create(divisor);
            if subs[bin].is_none() {
                return SQLITE_NOMEM_BKPT;
            }
        }
        p = subs[bin].as_deref_mut().unwrap();
    }
    if (p.i_size as usize) <= BITVEC_NBIT {
        let BitvecUnion::Bitmap(bitmap) = &mut p.u else {
            unreachable!("Bitvec pequeno sem representação Bitmap");
        };
        bitmap[i as usize / BITVEC_SZELEM] |= 1u8 << (i as usize & (BITVEC_SZELEM - 1));
        return SQLITE_OK;
    }
    let mut h = bitvec_hash(i);
    i = i.wrapping_add(1);
    let n_set = p.n_set;
    let BitvecUnion::Hash(hash) = &mut p.u else {
        unreachable!("Bitvec grande sem divisor e sem representação Hash");
    };
    // Se não houve colisão de hash e isto não enche a tabela por completo,
    // adiciona direto, sem subdividir nem refazer o hash.
    let mut rehash = false;
    if hash[h] == 0 {
        if n_set >= (BITVEC_NINT as u32 - 1) {
            rehash = true;
        }
    } else {
        // Houve colisão: confere se já está no hash; se não, acha uma vaga.
        loop {
            if hash[h] == i {
                return SQLITE_OK;
            }
            h += 1;
            if h >= BITVEC_NINT {
                h = 0;
            }
            if hash[h] == 0 {
                break;
            }
        }
        // Não achou no hash; h aponta a primeira vaga livre. Vê se isso deixa
        // o hash cheio demais.
        if n_set >= BITVEC_MXHASH as u32 {
            rehash = true;
        }
    }
    if rehash {
        let ai_values: Vec<u32> = hash.clone();
        p.u = BitvecUnion::Sub((0..BITVEC_NPTR).map(|_| None).collect());
        p.i_divisor = ((p.i_size as u64 + BITVEC_NPTR as u64 - 1) / BITVEC_NPTR as u64) as u32;
        let mut rc = bitvec_set(Some(&mut *p), i);
        for &v in ai_values.iter() {
            if v != 0 {
                rc |= bitvec_set(Some(&mut *p), v);
            }
        }
        return rc;
    }
    // bitvec_set_end
    p.n_set += 1;
    hash[h] = i;
    SQLITE_OK
}

/// Limpa o i-ésimo bit. O `_p_buf` do C (BITVEC_SZ bytes de área temporária
/// para refazer a tabela hash) não é usado: a cópia dos valores é local.
pub fn bitvec_clear(p: Option<&mut Bitvec>, i: u32, _p_buf: &mut [u8]) {
    let Some(mut p) = p else {
        return;
    };
    let mut i = i.wrapping_sub(1);
    while p.i_divisor != 0 {
        let bin = (i / p.i_divisor) as usize;
        i %= p.i_divisor;
        let BitvecUnion::Sub(subs) = &mut p.u else {
            return;
        };
        match subs[bin].as_deref_mut() {
            Some(sub) => p = sub,
            None => return,
        }
    }
    if (p.i_size as usize) <= BITVEC_NBIT {
        let BitvecUnion::Bitmap(bitmap) = &mut p.u else {
            return;
        };
        bitmap[i as usize / BITVEC_SZELEM] &= !(1u8 << (i as usize & (BITVEC_SZELEM - 1)));
    } else {
        let BitvecUnion::Hash(hash) = &mut p.u else {
            return;
        };
        let ai_values: Vec<u32> = hash.clone();
        hash.fill(0);
        let mut n_set = 0u32;
        for &v in ai_values.iter() {
            if v != 0 && v != i.wrapping_add(1) {
                let mut h = bitvec_hash(v - 1);
                n_set += 1;
                while hash[h] != 0 {
                    h += 1;
                    if h >= BITVEC_NINT {
                        h = 0;
                    }
                }
                hash[h] = v;
            }
        }
        p.n_set = n_set;
    }
}

/// Destrói um bitmap. A memória dos sub-bitmaps é recuperada pelo `Drop`
/// recursivo do `Box`.
pub fn bitvec_destroy(p: Option<Box<Bitvec>>) {
    drop(p);
}

/// Retorna o valor do parâmetro i_size dado na criação do Bitvec.
pub fn bitvec_size(p: &Bitvec) -> u32 {
    p.i_size
}

/// Define o bit I no vetor V (macro SETBIT do C).
#[inline]
pub fn setbit(v: &mut [u8], i: usize) {
    v[i >> 3] |= 1u8 << (i & 7);
}

/// Limpa o bit I no vetor V (macro CLEARBIT do C).
#[inline]
pub fn clearbit(v: &mut [u8], i: usize) {
    v[i >> 3] &= !(1u8 << (i & 7));
}

/// Testa o bit I no vetor V (macro TESTBIT do C).
#[inline]
pub fn testbit(v: &[u8], i: usize) -> bool {
    (v[i >> 3] & (1u8 << (i & 7))) != 0
}

/// Executa um teste extensivo do código Bitvec.
///
/// A entrada é um array de inteiros que age como programa: opcodes seguidos
/// de 0, 1 ou 3 operandos.
///
///    0          Parar e retornar o número de erros
///    1 N S X    Definir N bits começando em S e incrementando de X
///    2 N S X    Limpar N bits começando em S e incrementando de X
///    3 N        Definir N bits escolhidos ao acaso
///    4 N        Limpar N bits escolhidos ao acaso
///    5 N S X    Definir N bits de S com incremento X só no array, não no bitvec
///
/// No fim o array linear é comparado com o Bitvec. Se houver diferença
/// retorna erro, senão zero. Se faltar memória, retorna -1.
pub fn bitvec_builtin_test(sz: i32, a_op: &mut [i32]) -> i32 {
    let mut rc: i32 = -1;
    let p_bitvec = bitvec_create(sz as u32);
    let mut p_v = vec![0u8; ((sz as usize + 7) / 8) + 1];
    let mut p_tmp_space = vec![0u8; BITVEC_SZ];
    let mut p_bitvec = match p_bitvec {
        Some(b) => b,
        None => return rc,
    };

    'end: {
        // Testes com Bitvec nulo.
        bitvec_set(None, 1);
        bitvec_clear(None, 1, &mut p_tmp_space);

        // Executa o programa.
        let mut pc: usize = 0;
        let mut i: i32;
        loop {
            let op = a_op[pc];
            if op == 0 {
                break;
            }
            let mut nx: usize;
            match op {
                1 | 2 | 5 => {
                    nx = 4;
                    i = a_op[pc + 2] - 1;
                    a_op[pc + 2] += a_op[pc + 3];
                }
                _ => {
                    nx = 2;
                    let mut buf = [0u8; 4];
                    randomness(&mut buf);
                    i = i32::from_ne_bytes(buf);
                }
            }
            a_op[pc + 1] -= 1;
            if a_op[pc + 1] > 0 {
                nx = 0;
            }
            pc += nx;
            i = (i & 0x7fffffff) % sz;
            if (op & 1) != 0 {
                setbit(&mut p_v, (i + 1) as usize);
                if op != 5 {
                    if bitvec_set(Some(&mut *p_bitvec), (i + 1) as u32) != 0 {
                        break 'end;
                    }
                }
            } else {
                clearbit(&mut p_v, (i + 1) as usize);
                bitvec_clear(Some(&mut *p_bitvec), (i + 1) as u32, &mut p_tmp_space);
            }
        }

        // Confere se o array linear casa exatamente com o Bitvec. Parte da
        // suposição de que casam (rc == 0) e muda rc se achar discrepância.
        rc = bitvec_test(None, 0)
            .wrapping_add(bitvec_test(Some(&*p_bitvec), (sz + 1) as u32))
            .wrapping_add(bitvec_test(Some(&*p_bitvec), 0))
            .wrapping_add((bitvec_size(&p_bitvec) as i32).wrapping_sub(sz));
        let mut k: i32 = 1;
        while k <= sz {
            if (testbit(&p_v, k as usize) as i32) != bitvec_test(Some(&*p_bitvec), k as u32) {
                rc = k;
                break;
            }
            k += 1;
        }
    }

    // bitvec_end: libera as estruturas alocadas.
    bitvec_destroy(Some(p_bitvec));
    rc
}


// ---- part_001.rs ----

// O trecho C correspondente contém apenas o `#endif /* SQLITE_UNTESTABLE */`,
// que fecha o bloco de teste embutido já traduzido em part_000.rs.

