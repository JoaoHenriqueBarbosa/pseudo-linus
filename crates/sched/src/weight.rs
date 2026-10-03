//! Pesos, tabelas de nice e aritmética de ponto fixo, como em `kernel/sched/core.c`,
//! `kernel/sched/sched.h`, `kernel/sched/fair.c` e `include/linux/math64.h` (Linux 6.12.101, 64 bits).
//!
//! A regra aqui é reproduzir o resultado inteiro do kernel bit a bit, inclusive as perdas de precisão:
//! `__calc_delta` multiplica pelo inverso de 32 bits em vez de dividir, `div_s64` recebe o divisor como
//! `s32` (truncando se for maior) e as somas com sinal dão a volta como no C compilado com
//! `-fno-strict-overflow`. Quem compara com o kernel precisa dos mesmos arredondamentos.

/// `SCHED_FIXEDPOINT_SHIFT`: bits de resolução extra da carga em 64 bits.
pub const SCHED_FIXEDPOINT_SHIFT: u32 = 10;

/// `NICE_0_LOAD_SHIFT` em 64 bits (`SCHED_FIXEDPOINT_SHIFT + SCHED_FIXEDPOINT_SHIFT`).
pub const NICE_0_LOAD_SHIFT: u32 = SCHED_FIXEDPOINT_SHIFT + SCHED_FIXEDPOINT_SHIFT;

/// `NICE_0_LOAD`: carga de uma tarefa nice 0, já escalada (1024 << 10).
pub const NICE_0_LOAD: u64 = 1 << NICE_0_LOAD_SHIFT;

/// `WMULT_CONST`: numerador dos inversos (`~0U`).
pub const WMULT_CONST: u32 = u32::MAX;

/// `WMULT_SHIFT`: os inversos são `2^32 / peso`.
pub const WMULT_SHIFT: u32 = 32;

/// Menor nice (`MIN_NICE`).
pub const MIN_NICE: i32 = -20;

/// Maior nice (`MAX_NICE`).
pub const MAX_NICE: i32 = 19;

/// `sched_prio_to_weight[40]`, de nice -20 a 19. Cada nível muda ~10% de CPU (fator ~1,25 de peso).
pub const SCHED_PRIO_TO_WEIGHT: [u32; 40] = [
    /* -20 */ 88761, 71755, 56483, 46273, 36291, //
    /* -15 */ 29154, 23254, 18705, 14949, 11916, //
    /* -10 */ 9548, 7620, 6100, 4904, 3906, //
    /*  -5 */ 3121, 2501, 1991, 1586, 1277, //
    /*   0 */ 1024, 820, 655, 526, 423, //
    /*   5 */ 335, 272, 215, 172, 137, //
    /*  10 */ 110, 87, 70, 56, 45, //
    /*  15 */ 36, 29, 23, 18, 15, //
];

/// `sched_prio_to_wmult[40]`: inversos pré-calculados (`2^32 / peso`), na mesma ordem.
pub const SCHED_PRIO_TO_WMULT: [u32; 40] = [
    /* -20 */ 48388, 59856, 76040, 92818, 118348, //
    /* -15 */ 147320, 184698, 229616, 287308, 360437, //
    /* -10 */ 449829, 563644, 704093, 875809, 1099582, //
    /*  -5 */ 1376151, 1717300, 2157191, 2708050, 3363326, //
    /*   0 */ 4194304, 5237765, 6557202, 8165337, 10153587, //
    /*   5 */ 12820798, 15790321, 19976592, 24970740, 31350126, //
    /*  10 */ 39045157, 49367440, 61356676, 76695844, 95443717, //
    /*  15 */ 119304647, 148102320, 186737708, 238609294, 286331153, //
];

/// `MIN_SHARES`: menor peso de grupo (sem escala; também é o piso do `calc_group_shares`).
pub const MIN_SHARES: u64 = 1 << 1;

/// `MAX_SHARES`: maior peso de grupo (sem escala).
pub const MAX_SHARES: u64 = 1 << 18;

/// `CGROUP_WEIGHT_MIN`, `CGROUP_WEIGHT_DFL` e `CGROUP_WEIGHT_MAX` do `cpu.weight` do cgroup v2.
pub const CGROUP_WEIGHT_MIN: u64 = 1;
pub const CGROUP_WEIGHT_DFL: u64 = 100;
pub const CGROUP_WEIGHT_MAX: u64 = 10_000;

/// `sched_weight_from_cgroup`: `cpu.weight` (1 a 10000, padrão 100) pra peso do escalonador (padrão
/// 1024), com arredondamento pro mais próximo (`DIV_ROUND_CLOSEST_ULL(w * 1024, 100)`).
pub const fn sched_weight_from_cgroup(cgroup_weight: u64) -> u64 {
    (cgroup_weight * 1024 + CGROUP_WEIGHT_DFL / 2) / CGROUP_WEIGHT_DFL
}

/// Shares escalados (`tg->shares`) pra um `cpu.weight`, como `cpu_weight_write_u64` +
/// `__sched_group_set_shares` (que limita a `[MIN_SHARES, MAX_SHARES]` escalados).
pub fn shares_from_cgroup_weight(cgroup_weight: u64) -> u64 {
    let w = cgroup_weight.clamp(CGROUP_WEIGHT_MIN, CGROUP_WEIGHT_MAX);
    scale_load(sched_weight_from_cgroup(w)).clamp(scale_load(MIN_SHARES), scale_load(MAX_SHARES))
}

/// Índice nas tabelas pra um nice (`static_prio - MAX_RT_PRIO`). Entra em pânico fora de [-20, 19].
pub fn nice_to_index(nice: i32) -> usize {
    assert!((MIN_NICE..=MAX_NICE).contains(&nice), "nice {nice} fora de [-20, 19]");
    (nice - MIN_NICE) as usize
}

/// `scale_load(w)`: peso visível ao usuário pra carga interna.
pub const fn scale_load(w: u64) -> u64 {
    w << SCHED_FIXEDPOINT_SHIFT
}

/// `scale_load_down(w)`: carga interna pra peso, com piso 2 pra carga não nula.
pub const fn scale_load_down(w: u64) -> u64 {
    if w == 0 {
        0
    } else {
        let d = w >> SCHED_FIXEDPOINT_SHIFT;
        if d < 2 { 2 } else { d }
    }
}

/// `struct load_weight`: carga escalada e o inverso usado pelo `__calc_delta`.
///
/// `inv_weight == 0` significa "ainda não calculado" (é o que `update_load_set` deixa); nesse caso o
/// inverso é derivado na hora como no `__update_inv_weight`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LoadWeight {
    pub weight: u64,
    pub inv_weight: u32,
}

impl LoadWeight {
    /// `set_load_weight` de uma tarefa SCHED_NORMAL: peso e inverso das tabelas.
    pub fn from_nice(nice: i32) -> LoadWeight {
        let i = nice_to_index(nice);
        LoadWeight { weight: scale_load(u64::from(SCHED_PRIO_TO_WEIGHT[i])), inv_weight: SCHED_PRIO_TO_WMULT[i] }
    }

    /// `update_load_set`: peso novo, inverso a recalcular.
    pub fn with_weight(weight: u64) -> LoadWeight {
        LoadWeight { weight, inv_weight: 0 }
    }

    /// Inverso efetivo, como o `__update_inv_weight` deixaria no campo.
    pub fn effective_inv_weight(&self) -> u32 {
        if self.inv_weight != 0 {
            return self.inv_weight;
        }
        let w = scale_load_down(self.weight);
        if w >= u64::from(WMULT_CONST) {
            return 1;
        }
        // Peso zero dá o inverso máximo.
        u64::from(WMULT_CONST).checked_div(w).map_or(WMULT_CONST, |q| q as u32)
    }
}

/// `fls`: posição (a partir de 1) do bit mais significativo ligado; 0 pra 0.
pub const fn fls(x: u32) -> u32 {
    32 - x.leading_zeros()
}

/// `mul_u64_u32_shr`: `(a * mul) >> shift` com produto de 128 bits.
pub const fn mul_u64_u32_shr(a: u64, mul: u32, shift: u32) -> u64 {
    ((a as u128 * mul as u128) >> shift) as u64
}

/// `div_s64(s64 dividend, s32 divisor)`: divisão com truncamento pra zero, divisor de 32 bits.
///
/// Quem chama passa o divisor já convertido pra `i32`, como a conversão implícita do C faz (o kernel
/// passa `long` e `unsigned long` aqui; valores acima de 2^31 seriam truncados lá também).
pub const fn div_s64(dividend: i64, divisor: i32) -> i64 {
    dividend.wrapping_div(divisor as i64)
}

/// `__calc_delta(delta_exec, weight, lw)`: `delta_exec * weight / lw.weight` em ponto fixo, com o
/// inverso de 32 bits e os deslocamentos que mantêm o produto dentro de 64 bits.
pub fn calc_delta(delta_exec: u64, weight: u64, lw: &LoadWeight) -> u64 {
    let mut fact = scale_load_down(weight);
    let mut fact_hi = (fact >> 32) as u32;
    let mut shift = WMULT_SHIFT;

    if fact_hi != 0 {
        let fs = fls(fact_hi);
        shift -= fs;
        fact >>= fs;
    }

    fact = u64::from(fact as u32) * u64::from(lw.effective_inv_weight());

    fact_hi = (fact >> 32) as u32;
    if fact_hi != 0 {
        let fs = fls(fact_hi);
        shift -= fs;
        fact >>= fs;
    }

    mul_u64_u32_shr(delta_exec, fact as u32, shift)
}

/// `calc_delta_fair(delta, se)`: converte tempo real em tempo virtual (`delta * NICE_0_LOAD / w`).
/// Pra nice 0 devolve o próprio delta, sem passar pela multiplicação.
pub fn calc_delta_fair(delta: u64, lw: &LoadWeight) -> u64 {
    if lw.weight != NICE_0_LOAD { calc_delta(delta, NICE_0_LOAD, lw) } else { delta }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_endpoints() {
        assert_eq!(SCHED_PRIO_TO_WEIGHT[nice_to_index(0)], 1024);
        assert_eq!(SCHED_PRIO_TO_WEIGHT[nice_to_index(-20)], 88761);
        assert_eq!(SCHED_PRIO_TO_WEIGHT[nice_to_index(19)], 15);
        assert_eq!(LoadWeight::from_nice(0).weight, NICE_0_LOAD);
    }

    /// Cada inverso da tabela é `2^32 / peso` com erro menor que 1. A tabela do kernel não segue uma
    /// regra só: a maioria é arredondada pro mais próximo, mas pesos como 56, 45, 29 e 23 estão
    /// truncados. Por isso o teste aceita o piso ou o teto, e o código usa a tabela, não a fórmula.
    #[test]
    fn wmult_is_inverse_within_one() {
        for (w, m) in SCHED_PRIO_TO_WEIGHT.iter().zip(SCHED_PRIO_TO_WMULT) {
            let w = u64::from(*w);
            let floor = (1u64 << 32) / w;
            let ceil = (1u64 << 32).div_ceil(w);
            assert!((floor..=ceil).contains(&u64::from(m)), "peso {w}: {m} fora de [{floor}, {ceil}]");
        }
    }

    /// Pesos vizinhos diferem por um fator entre 1,2 e 1,3 (o "10% por nível").
    #[test]
    fn neighbouring_weights_ratio() {
        for pair in SCHED_PRIO_TO_WEIGHT.windows(2) {
            let r = f64::from(pair[0]) / f64::from(pair[1]);
            assert!((1.19..=1.31).contains(&r), "razão {r} entre {} e {}", pair[0], pair[1]);
        }
    }

    #[test]
    fn scale_load_down_floor() {
        assert_eq!(scale_load_down(0), 0);
        assert_eq!(scale_load_down(1), 2);
        assert_eq!(scale_load_down(scale_load(15)), 15);
        assert_eq!(scale_load_down(NICE_0_LOAD), 1024);
    }

    /// `__update_inv_weight` usa `~0U / w`, que difere em 1 do valor da tabela pra nice 0.
    #[test]
    fn computed_inverse_differs_from_table() {
        let lw = LoadWeight::with_weight(NICE_0_LOAD);
        assert_eq!(lw.effective_inv_weight(), 4_194_303);
        assert_eq!(LoadWeight::from_nice(0).effective_inv_weight(), 4_194_304);
    }

    /// Valores feitos à mão seguindo o `__calc_delta` passo a passo.
    ///
    /// nice 5: w = 335, inv = 12820798. fact = 1024 * 12820798 = 13128497152, fact_hi = 3,
    /// fls(3) = 2, shift = 30, fact >>= 2 = 3282124288. Pra delta = 1_000_000:
    /// 1e6 * 3282124288 >> 30 = 3056716 (a divisão exata dá 3056716,4).
    ///
    /// nice 19: w = 15, inv = 286331153. fact = 1024 * 286331153 = 293203100672; fact_hi = 68,
    /// fls(68) = 7, shift = 25, fact >>= 7 = 2290649224. Pra delta = 2_800_000:
    /// 2.8e6 * 2290649224 >> 25 = 191146666 (exato: 2.8e6 * 1024 / 15 = 191146666,67).
    ///
    /// nice -20: w = 88761, inv = 48388. fact = 1024 * 48388 = 49549312 (< 2^32), shift = 32.
    /// Pra delta = 4_000_000: 4e6 * 49549312 >> 32 = 46146 (exato: 46146,39).
    #[test]
    fn calc_delta_fair_by_hand() {
        assert_eq!(calc_delta_fair(1_000_000, &LoadWeight::from_nice(0)), 1_000_000);
        assert_eq!(calc_delta_fair(1_000_000, &LoadWeight::from_nice(5)), 3_056_716);
        assert_eq!(calc_delta_fair(2_800_000, &LoadWeight::from_nice(19)), 191_146_666);
        assert_eq!(calc_delta_fair(4_000_000, &LoadWeight::from_nice(-20)), 46_146);
    }

    #[test]
    fn div_s64_truncates_toward_zero_and_divisor_is_32_bits() {
        assert_eq!(div_s64(-7, 2), -3);
        assert_eq!(div_s64(7, 2), 3);
        // Divisor de 2^32 + 3 vira 3 depois da conversão pra s32.
        assert_eq!(div_s64(9, ((1u64 << 32) + 3) as i32), 3);
    }

    /// `cpu.weight` 100 vira 1024; 300 vira 3072; 50 vira 512; 1 vira 10,24 arredondado pra 10.
    #[test]
    fn cgroup_weight_mapping() {
        assert_eq!(sched_weight_from_cgroup(100), 1024);
        assert_eq!(sched_weight_from_cgroup(300), 3072);
        assert_eq!(sched_weight_from_cgroup(50), 512);
        assert_eq!(sched_weight_from_cgroup(1), 10);
        assert_eq!(sched_weight_from_cgroup(10_000), 102_400);
        assert_eq!(shares_from_cgroup_weight(100), NICE_0_LOAD);
        assert_eq!(shares_from_cgroup_weight(0), scale_load(10));
    }

    #[test]
    fn fls_matches_definition() {
        assert_eq!(fls(0), 0);
        assert_eq!(fls(1), 1);
        assert_eq!(fls(3), 2);
        assert_eq!(fls(68), 7);
        assert_eq!(fls(u32::MAX), 32);
    }
}
