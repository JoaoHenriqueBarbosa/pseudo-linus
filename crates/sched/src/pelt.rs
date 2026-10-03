//! PELT (Per-Entity Load Tracking), como `kernel/sched/pelt.c` e `kernel/sched/sched-pelt.h` da
//! 6.12.101, só com o sinal de **carga** (`load_sum`/`load_avg`).
//!
//! O sinal é uma média geométrica do tempo em que a entidade (ou a fila) teve carga, em períodos de
//! 1024 µs com `y^32 = 1/2`: o que aconteceu há 32 ms vale metade do que acontece agora. As contas são
//! as do kernel, inclusive o tempo medido em unidades de 1024 ns (`delta >>= 10`), o `decay_load` pela
//! tabela `runnable_avg_yN_inv` e o divisor que depende da posição no período corrente
//! (`get_pelt_divider`).
//!
//! Ficam de fora os sinais `runnable` e `util`: no kernel eles alimentam o cpufreq, o EAS e a
//! classificação de grupos do balanceamento, que este crate não porta. A carga é o que entra no
//! `calc_group_shares` (peso de uma entidade de grupo em cada CPU) e no `task_h_load` do balanceamento.

/// `LOAD_AVG_PERIOD`: períodos pra meia-vida.
pub const LOAD_AVG_PERIOD: u64 = 32;

/// `LOAD_AVG_MAX`: soma máxima da série (1024 * Σ y^n).
pub const LOAD_AVG_MAX: u64 = 47742;

/// `PELT_MIN_DIVIDER`.
pub const PELT_MIN_DIVIDER: u64 = LOAD_AVG_MAX - 1024;

/// `runnable_avg_yN_inv[n] = y^n * 2^32`, n de 0 a 31.
const RUNNABLE_AVG_YN_INV: [u32; 32] = [
    0xffffffff, 0xfa83b2da, 0xf5257d14, 0xefe4b99a, 0xeac0c6e6, 0xe5b906e6, 0xe0ccdeeb, 0xdbfbb796, 0xd744fcc9,
    0xd2a81d91, 0xce248c14, 0xc9b9bd85, 0xc5672a10, 0xc12c4cc9, 0xbd08a39e, 0xb8fbaf46, 0xb504f333, 0xb123f581,
    0xad583ee9, 0xa9a15ab4, 0xa5fed6a9, 0xa2704302, 0x9ef5325f, 0x9b8d39b9, 0x9837f050, 0x94f4efa8, 0x91c3d373,
    0x8ea4398a, 0x8b95c1e3, 0x88980e80, 0x85aac367, 0x82cd8698,
];

/// `struct sched_avg`, só a parte de carga.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SchedAvg {
    /// Último instante (relógio PELT) em que a soma foi atualizada; 0 marca "não anexado".
    pub last_update_time: u64,
    pub load_sum: u64,
    /// Parte já contada do período corrente, em unidades de 1024 ns.
    pub period_contrib: u32,
    pub load_avg: u64,
}

/// `decay_load(val, n)`: `val * y^n`.
pub fn decay_load(val: u64, n: u64) -> u64 {
    if n > LOAD_AVG_PERIOD * 63 {
        return 0;
    }
    let mut val = val;
    let mut local_n = n;
    if local_n >= LOAD_AVG_PERIOD {
        val >>= local_n / LOAD_AVG_PERIOD;
        local_n %= LOAD_AVG_PERIOD;
    }
    crate::weight::mul_u64_u32_shr(val, RUNNABLE_AVG_YN_INV[local_n as usize], 32)
}

/// `__accumulate_pelt_segments(periods, d1, d3)`.
fn accumulate_pelt_segments(periods: u64, d1: u32, d3: u32) -> u32 {
    let c1 = decay_load(u64::from(d1), periods) as u32;
    let c2 = (LOAD_AVG_MAX - decay_load(LOAD_AVG_MAX, periods) - 1024) as u32;
    c1.wrapping_add(c2).wrapping_add(d3)
}

/// `get_pelt_divider`.
pub fn get_pelt_divider(sa: &SchedAvg) -> u64 {
    PELT_MIN_DIVIDER + u64::from(sa.period_contrib)
}

/// `accumulate_sum` (só carga). Devolve quantos períodos foram cruzados.
fn accumulate_sum(mut delta: u64, sa: &mut SchedAvg, load: u64) -> u64 {
    let mut contrib = delta as u32;
    delta += u64::from(sa.period_contrib);
    let periods = delta / 1024;
    if periods != 0 {
        sa.load_sum = decay_load(sa.load_sum, periods);
        delta %= 1024;
        if load != 0 {
            contrib = accumulate_pelt_segments(periods, 1024 - sa.period_contrib, delta as u32);
        }
    }
    sa.period_contrib = delta as u32;
    if load != 0 {
        sa.load_sum += load * u64::from(contrib);
    }
    periods
}

/// `___update_load_sum(now, sa, load, ...)`: acumula até `now`. Devolve `true` se cruzou período (só
/// então a média precisa ser recalculada).
pub fn update_load_sum(now: u64, sa: &mut SchedAvg, load: u64) -> bool {
    let delta = now.wrapping_sub(sa.last_update_time);
    if (delta as i64) < 0 {
        sa.last_update_time = now;
        return false;
    }
    // Unidade de 1024 ns, aproximação de 1 µs que o kernel usa.
    let delta = delta >> 10;
    if delta == 0 {
        return false;
    }
    sa.last_update_time += delta << 10;
    accumulate_sum(delta, sa, load) != 0
}

/// `___update_load_avg(sa, load)`: `load_avg = load * load_sum / divider`.
pub fn update_load_avg(sa: &mut SchedAvg, load: u64) {
    let divider = get_pelt_divider(sa);
    sa.load_avg = load * sa.load_sum / divider;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decay_halves_every_32_periods() {
        assert_eq!(decay_load(1 << 20, 0), (1 << 20) - 1);
        assert_eq!(decay_load(1 << 20, 32), (1 << 19) - 1);
        assert_eq!(decay_load(1 << 20, 64), (1 << 18) - 1);
        assert_eq!(decay_load(12345, 32 * 63 + 1), 0);
        // y^16 = 1/sqrt(2).
        let half_life = decay_load(1_000_000, 16) as f64 / 1_000_000.0;
        assert!((half_life - std::f64::consts::FRAC_1_SQRT_2).abs() < 1e-6);
    }

    /// Carga contínua converge pro peso; sem carga, decai pela metade em 32 ms.
    #[test]
    fn always_runnable_converges_to_weight() {
        let mut sa = SchedAvg { last_update_time: 1 << 30, ..SchedAvg::default() };
        let mut now = sa.last_update_time;
        for _ in 0..400 {
            now += 1_000_000;
            if update_load_sum(now, &mut sa, 1) {
                update_load_avg(&mut sa, 1024);
            }
        }
        assert!((1000..=1024).contains(&sa.load_avg), "{}", sa.load_avg);
        let before = sa.load_avg;
        for _ in 0..32 {
            now += 1024 * 1024;
            if update_load_sum(now, &mut sa, 0) {
                update_load_avg(&mut sa, 1024);
            }
        }
        let ratio = sa.load_avg as f64 / before as f64;
        assert!((0.45..=0.55).contains(&ratio), "{ratio}");
    }

    /// Menos de 1024 ns não mexe em nada; tempo pra trás só realinha.
    #[test]
    fn tiny_and_negative_deltas() {
        let mut sa = SchedAvg { last_update_time: 10_000, ..SchedAvg::default() };
        assert!(!update_load_sum(10_500, &mut sa, 1));
        assert_eq!(sa.last_update_time, 10_000);
        assert!(!update_load_sum(5_000, &mut sa, 1));
        assert_eq!(sa.last_update_time, 5_000);
    }
}
