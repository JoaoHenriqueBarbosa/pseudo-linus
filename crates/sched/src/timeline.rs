//! A linha do tempo de uma runqueue CFS: a árvore aumentada (`tasks_timeline`) e a contabilidade do
//! `avg_vruntime` (`zero_vruntime`, `avg_vruntime`, `avg_load`), com as funções do fair.c que só
//! dependem disso: `__enqueue_entity`, `__dequeue_entity`, `avg_vruntime`, `vruntime_eligible`,
//! `cfs_rq_min_slice` e `pick_eevdf`.
//!
//! Fica separada do [`crate::Sched`] pra que testes e benchmarks montem estados arbitrários da árvore
//! sem passar pela máquina de estados das tarefas.
//!
//! # A conta do V
//!
//! `V = Σ v_i * w_i / Σ w_i` é guardado de forma relativa a `v0 = zero_vruntime`:
//! `avg_vruntime = Σ (v_i - v0) * w_i` e `avg_load = Σ w_i`, com `w_i = scale_load_down(peso)`. A
//! entidade que está rodando (`cfs_rq->curr`) não fica na árvore nem nas somas; quem chama passa ela
//! como [`CurrView`] e as funções somam a contribuição dela na hora, como o kernel faz. A divisão só
//! acontece em [`Timeline::avg_vruntime`], com piso pra negativos, e cada chamada move `v0` pro V
//! atual (é o backport da 6.12.64+ que trocou `min_vruntime` por `zero_vruntime`).
//!
//! # Mais de uma entidade rodando
//!
//! No kernel cada `cfs_rq` é de uma CPU e tem no máximo um corrente. O modo de runqueue única do
//! [`crate::Sched`] (uma hierarquia compartilhada por várias CPUs) pode ter várias entidades da mesma
//! fila rodando ao mesmo tempo, então as funções recebem a lista `running` com todas elas (as que estão
//! na fila), e o pick recebe à parte o corrente da CPU que escolhe (`this`) e os candidatos que estão
//! fora da árvore por estarem rodando em outra CPU mas ainda podem receber esta (`extra`, grupos com
//! tarefas prontas sobrando). Com `running` de no máximo um elemento e `extra` vazio, as contas são
//! exatamente as do kernel.

use std::cmp::Ordering;

use rbtree::{Augment, NodeId, RbTree};

use crate::EntityId;
use crate::weight::{div_s64, scale_load_down};

/// `cfs_rq->zero_vruntime` inicial (`init_cfs_rq`): `(u64)(-(1LL << 20))`, pra que a volta do u64
/// apareça cedo.
pub const INITIAL_ZERO_VRUNTIME: u64 = (-(1i64 << 20)) as u64;

/// Deadline virtual como chave da árvore.
///
/// Compara como o `entity_before` do kernel: `(s64)(a - b) < 0`. É uma ordem total dentro de qualquer
/// janela menor que 2^63 ns de tempo virtual, que é o caso de uma runqueue viva; por isso a árvore
/// continua certa quando o u64 dá a volta.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct VDeadline(pub u64);

impl Ord for VDeadline {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.0.wrapping_sub(other.0) as i64).cmp(&0)
    }
}

impl PartialOrd for VDeadline {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// `entity_before(a, b)` sobre deadlines.
#[inline]
pub fn deadline_before(a: u64, b: u64) -> bool {
    (a.wrapping_sub(b) as i64) < 0
}

/// O que a árvore guarda de cada entidade enfileirada: o suficiente pra augmentação e pro pick.
///
/// São cópias dos campos da `sched_entity`. Enquanto a entidade está na árvore o kernel não mexe em
/// `vruntime` nem no peso dela (quem muda isso tira da árvore antes); a fatia de entidades de grupo é
/// atualizada no lugar, com propagação do resumo, como o `min_vruntime_cb_propagate` do kernel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QueuedEntity {
    pub entity: EntityId,
    pub vruntime: u64,
    pub slice: u64,
    /// `se->load.weight` (escalado, nice 0 = 1 << 20).
    pub weight: u64,
}

/// Resumo de subárvore: `se->min_vruntime` e `se->min_slice`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EevdfSummary {
    pub min_vruntime: u64,
    pub min_slice: u64,
}

/// `min_vruntime_update` como augmentação da árvore.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EevdfAugment;

impl Augment<VDeadline, QueuedEntity> for EevdfAugment {
    type Summary = EevdfSummary;

    /// `se->min_vruntime = min(se->vruntime, right->min_vruntime, left->min_vruntime)` com a comparação
    /// circular `vruntime_gt`, e `se->min_slice` com a comparação comum, na ordem direita e esquerda do
    /// kernel.
    #[inline]
    fn summarize(
        _key: &VDeadline,
        value: &QueuedEntity,
        left: Option<&EevdfSummary>,
        right: Option<&EevdfSummary>,
    ) -> EevdfSummary {
        let mut s = EevdfSummary { min_vruntime: value.vruntime, min_slice: value.slice };
        for child in [right, left].into_iter().flatten() {
            if (s.min_vruntime.wrapping_sub(child.min_vruntime) as i64) > 0 {
                s.min_vruntime = child.min_vruntime;
            }
        }
        for child in [right, left].into_iter().flatten() {
            if child.min_slice < s.min_slice {
                s.min_slice = child.min_slice;
            }
        }
        s
    }
}

/// A árvore concreta do EEVDF.
pub type EevdfTree = RbTree<VDeadline, QueuedEntity, EevdfAugment>;

/// Retrato de uma entidade que está rodando e continua na fila (`curr && curr->on_rq`). Quem não está
/// mais na fila (dormiu) não entra, como o kernel faz ao zerar `curr` nas contas.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CurrView {
    pub entity: EntityId,
    pub vruntime: u64,
    pub deadline: u64,
    /// `curr->load.weight` (escalado).
    pub weight: u64,
    pub slice: u64,
    /// `protect_slice(curr)`: `curr->vlag == curr->deadline`.
    pub protected: bool,
}

/// A linha do tempo de uma runqueue.
#[derive(Clone, Debug)]
pub struct Timeline {
    pub(crate) tree: EevdfTree,
    zero_vruntime: u64,
    avg_vruntime: i64,
    avg_load: u64,
}

impl Default for Timeline {
    fn default() -> Self {
        Timeline::new()
    }
}

impl Timeline {
    /// Linha do tempo vazia (`init_cfs_rq`).
    pub fn new() -> Timeline {
        Timeline::with_zero_vruntime(INITIAL_ZERO_VRUNTIME)
    }

    /// Linha do tempo vazia com `zero_vruntime` escolhido (útil pra testes de volta do u64).
    pub fn with_zero_vruntime(zero_vruntime: u64) -> Timeline {
        Timeline { tree: RbTree::new(), zero_vruntime, avg_vruntime: 0, avg_load: 0 }
    }


    /// A árvore, pra navegação.
    pub fn tree(&self) -> &EevdfTree {
        &self.tree
    }

    /// `cfs_rq->zero_vruntime`.
    pub fn zero_vruntime(&self) -> u64 {
        self.zero_vruntime
    }

    /// `cfs_rq->avg_vruntime` (Σ (v_i - v0) * w_i das entidades na árvore).
    pub fn avg_vruntime_sum(&self) -> i64 {
        self.avg_vruntime
    }

    /// `cfs_rq->avg_load` (Σ w_i das entidades na árvore, com `scale_load_down`).
    pub fn avg_load(&self) -> u64 {
        self.avg_load
    }

    /// `entity_key`: vruntime relativo a `zero_vruntime`.
    #[inline]
    pub fn entity_key(&self, vruntime: u64) -> i64 {
        vruntime.wrapping_sub(self.zero_vruntime) as i64
    }

    /// `__enqueue_entity`: soma a entidade em `avg_vruntime`/`avg_load` e insere na árvore pela deadline.
    pub fn enqueue(&mut self, entity: QueuedEntity, deadline: u64) -> NodeId {
        let w = scale_load_down(entity.weight);
        let key = self.entity_key(entity.vruntime);
        self.avg_vruntime = self.avg_vruntime.wrapping_add(key.wrapping_mul(w as i64));
        self.avg_load += w;
        self.tree.insert(VDeadline(deadline), entity)
    }

    /// `__dequeue_entity`: tira da árvore e das somas. Devolve a deadline e a entidade.
    pub fn dequeue(&mut self, node: NodeId) -> (u64, QueuedEntity) {
        let (deadline, entity) = self.tree.remove(node);
        let w = scale_load_down(entity.weight);
        let key = self.entity_key(entity.vruntime);
        self.avg_vruntime = self.avg_vruntime.wrapping_sub(key.wrapping_mul(w as i64));
        self.avg_load -= w;
        (deadline.0, entity)
    }

    /// Troca a fatia guardada de uma entidade na árvore e propaga o `min_slice` (o
    /// `se->slice = slice; min_vruntime_cb_propagate(&se->run_node, NULL)` do `enqueue_task_fair`).
    pub fn set_slice(&mut self, node: NodeId, slice: u64) {
        self.tree.update_value(node, |e| e.slice = slice);
    }

    /// Deadline da entidade no nó.
    pub fn deadline(&self, node: NodeId) -> u64 {
        self.tree.key(node).0
    }

    /// `update_zero_vruntime`: move `v0` de `delta` e corrige a soma (`avg_vruntime -= avg_load * delta`).
    fn update_zero_vruntime(&mut self, delta: i64) {
        self.avg_vruntime = self.avg_vruntime.wrapping_sub((self.avg_load as i64).wrapping_mul(delta));
        self.zero_vruntime = self.zero_vruntime.wrapping_add(delta as u64);
    }

    /// Somas da árvore mais a contribuição de quem está rodando: (Σ key * w, Σ w).
    #[inline]
    fn sums_with(&self, running: &[CurrView]) -> (i64, i64) {
        let mut avg = self.avg_vruntime;
        let mut load = self.avg_load as i64;
        for c in running {
            let w = scale_load_down(c.weight) as i64;
            avg = avg.wrapping_add(self.entity_key(c.vruntime).wrapping_mul(w));
            load += w;
        }
        (avg, load)
    }

    /// Quanto `v0` precisa andar pra virar o V atual (a parte de cálculo do `avg_vruntime`).
    fn avg_vruntime_delta(&self, running: &[CurrView]) -> i64 {
        if self.avg_load != 0 {
            let (mut runtime, weight) = self.sums_with(running);
            // O sinal inverte piso e teto da divisão truncada: subtrair (peso - 1) dá o piso.
            if runtime < 0 {
                runtime = runtime.wrapping_sub(weight - 1);
            }
            div_s64(runtime, weight as i32)
        } else if !running.is_empty() {
            if running.len() == 1 {
                // Com um elemento só, ele é a média.
                running[0].vruntime.wrapping_sub(self.zero_vruntime) as i64
            } else {
                // Só pode acontecer com várias CPUs na mesma fila: média das que rodam, com o mesmo piso.
                let (mut runtime, weight) = self.sums_with(running);
                if runtime < 0 {
                    runtime = runtime.wrapping_sub(weight - 1);
                }
                div_s64(runtime, weight as i32)
            }
        } else {
            0
        }
    }

    /// `avg_vruntime(cfs_rq)`: calcula V (com viés pra esquerda, pelo piso) e move `zero_vruntime` pra
    /// ele. Muda o estado, como no kernel; pra só olhar, use [`Timeline::avg_vruntime_peek`].
    pub fn avg_vruntime(&mut self, running: &[CurrView]) -> u64 {
        let delta = self.avg_vruntime_delta(running);
        self.update_zero_vruntime(delta);
        self.zero_vruntime
    }

    /// O mesmo V de [`Timeline::avg_vruntime`], sem mover `zero_vruntime`.
    pub fn avg_vruntime_peek(&self, running: &[CurrView]) -> u64 {
        self.zero_vruntime.wrapping_add(self.avg_vruntime_delta(running) as u64)
    }

    /// `vruntime_eligible`: `Σ (v_j - v0) * w_j >= (v - v0) * Σ w_j`, sem divisão (equivale a `v <= V`
    /// exato, sem o erro de arredondamento de V).
    #[inline]
    pub fn vruntime_eligible(&self, running: &[CurrView], vruntime: u64) -> bool {
        let (avg, load) = self.sums_with(running);
        avg >= self.entity_key(vruntime).wrapping_mul(load)
    }

    /// `cfs_rq_min_slice`: a menor fatia entre quem roda e a árvore (`u64::MAX` se não há ninguém).
    pub fn min_slice(&self, running: &[CurrView]) -> u64 {
        let mut min_slice = u64::MAX;
        for c in running {
            min_slice = min_slice.min(c.slice);
        }
        if let Some(root) = self.tree.root() {
            min_slice = min_slice.min(self.tree.summary(root).min_slice);
        }
        min_slice
    }

    /// `pick_eevdf`: entre as entidades elegíveis, a de deadline mais cedo, em O(log n).
    ///
    /// `nr_running` é `cfs_rq->nr_running`; `running`, todas as entidades da fila que estão rodando
    /// (entram nas contas de V); `this`, o corrente da CPU que escolhe, se ainda estiver na fila; `extra`,
    /// candidatos fora da árvore (só no modo de runqueue única). Com uma entidade só, devolve ela sem
    /// testar elegibilidade. O corrente sai da disputa se não for elegível; com RUN_TO_PARITY e a fatia
    /// protegida, ele vence direto. Depois testa o mais à esquerda e, se não for elegível, desce: vai pra
    /// esquerda se a subárvore esquerda tem alguém elegível (pelo `min_vruntime`), senão pega o nó se
    /// ele for elegível, senão vai pra direita. No fim o corrente vence se tiver deadline estritamente
    /// menor.
    pub fn pick_eevdf(
        &self,
        nr_running: u32,
        running: &[CurrView],
        this: Option<&CurrView>,
        extra: &[CurrView],
        run_to_parity: bool,
    ) -> Option<EntityId> {
        let first = self.tree.first();
        if nr_running == 1 {
            return this
                .map(|c| c.entity)
                .or_else(|| first.map(|n| self.tree.value(n).entity))
                .or_else(|| extra.first().map(|c| c.entity));
        }

        let eligible_curr = this.filter(|c| self.vruntime_eligible(running, c.vruntime));
        if run_to_parity && let Some(c) = eligible_curr && c.protected {
            return Some(c.entity);
        }

        let mut best = None;
        match first {
            Some(f) if self.vruntime_eligible(running, self.tree.value(f).vruntime) => best = Some(f),
            _ => {
                let mut node = self.tree.root();
                while let Some(n) = node {
                    if let Some(l) = self.tree.left(n)
                        && self.vruntime_eligible(running, self.tree.summary(l).min_vruntime)
                    {
                        node = Some(l);
                        continue;
                    }
                    if self.vruntime_eligible(running, self.tree.value(n).vruntime) {
                        best = Some(n);
                        break;
                    }
                    node = self.tree.right(n);
                }
            }
        }

        let mut best: Option<(u64, EntityId)> = best.map(|b| (self.tree.key(b).0, self.tree.value(b).entity));
        for c in extra {
            if self.vruntime_eligible(running, c.vruntime) && best.is_none_or(|(d, _)| deadline_before(c.deadline, d)) {
                best = Some((c.deadline, c.entity));
            }
        }

        match (best, eligible_curr) {
            (None, c) => c.map(|c| c.entity),
            (Some((d, _)), Some(c)) if deadline_before(c.deadline, d) => Some(c.entity),
            (Some((_, e)), _) => Some(e),
        }
    }

    /// Mesma decisão do [`Timeline::pick_eevdf`], por força bruta: percorre a árvore inteira em ordem e
    /// pega a primeira entidade elegível (que é a de deadline mais cedo, com empate pra quem chegou
    /// antes). Serve de oráculo nos testes e de linha de base O(n) nos benchmarks.
    pub fn pick_linear(
        &self,
        nr_running: u32,
        running: &[CurrView],
        this: Option<&CurrView>,
        extra: &[CurrView],
        run_to_parity: bool,
    ) -> Option<EntityId> {
        if nr_running == 1 {
            return this
                .map(|c| c.entity)
                .or_else(|| self.tree.first().map(|n| self.tree.value(n).entity))
                .or_else(|| extra.first().map(|c| c.entity));
        }
        let eligible_curr = this.filter(|c| self.vruntime_eligible(running, c.vruntime));
        if run_to_parity && let Some(c) = eligible_curr && c.protected {
            return Some(c.entity);
        }
        let mut best: Option<(u64, EntityId)> = self
            .tree
            .iter()
            .find(|(_, _, e)| self.vruntime_eligible(running, e.vruntime))
            .map(|(_, d, e)| (d.0, e.entity));
        for c in extra {
            if self.vruntime_eligible(running, c.vruntime) && best.is_none_or(|(d, _)| deadline_before(c.deadline, d)) {
                best = Some((c.deadline, c.entity));
            }
        }
        match (best, eligible_curr) {
            (None, c) => c.map(|c| c.entity),
            (Some((d, _)), Some(c)) if deadline_before(c.deadline, d) => Some(c.entity),
            (Some((_, e)), _) => Some(e),
        }
    }

    /// Confere a árvore (invariantes rubro-negras e `min_vruntime`/`min_slice`) e se `avg_vruntime` e
    /// `avg_load` batem com as somas recalculadas do zero.
    pub fn check_invariants(&self) -> Result<(), String> {
        self.tree.check_invariants().map_err(|e| format!("árvore: {e}"))?;
        let mut sum = 0i64;
        let mut load = 0u64;
        for (_, _, e) in self.tree.iter() {
            let w = scale_load_down(e.weight);
            sum = sum.wrapping_add(self.entity_key(e.vruntime).wrapping_mul(w as i64));
            load += w;
        }
        if sum != self.avg_vruntime {
            return Err(format!("avg_vruntime {} mas a soma recalculada dá {sum}", self.avg_vruntime));
        }
        if load != self.avg_load {
            return Err(format!("avg_load {} mas a soma recalculada dá {load}", self.avg_load));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::weight::NICE_0_LOAD;

    fn ent(id: u32, vruntime: u64, weight: u64) -> QueuedEntity {
        QueuedEntity { entity: EntityId::from_index(id), vruntime, slice: 2_800_000, weight }
    }

    fn curr(id: u32, vruntime: u64, deadline: u64, protected: bool) -> CurrView {
        CurrView { entity: EntityId::from_index(id), vruntime, deadline, weight: NICE_0_LOAD, slice: 2_800_000, protected }
    }

    /// Exemplo da documentação do módulo, feito à mão.
    ///
    /// v0 = 1000. A: v = 990, w = 1024 (nice 0). B: v = 995, w = 335 (nice 5).
    /// avg = (-10 * 1024) + (-5 * 335) = -11915; W = 1359. Negativo, então runtime = -11915 - 1358 =
    /// -13273, e -13273 / 1359 = -9,77 truncado = -9 = piso(-8,77). V = 991.
    /// Depois de mover v0 pra 991: avg = -11915 - 1359 * (-9) = 316 = (-1 * 1024) + (4 * 335).
    #[test]
    fn avg_vruntime_floor_by_hand() {
        let mut tl = Timeline::with_zero_vruntime(1000);
        tl.enqueue(ent(0, 990, NICE_0_LOAD), 10_000);
        tl.enqueue(ent(1, 995, 335 << 10), 20_000);
        assert_eq!(tl.avg_vruntime_sum(), -11915);
        assert_eq!(tl.avg_load(), 1359);
        assert_eq!(tl.avg_vruntime_peek(&[]), 991);
        assert_eq!(tl.avg_vruntime(&[]), 991);
        assert_eq!(tl.zero_vruntime(), 991);
        assert_eq!(tl.avg_vruntime_sum(), 316);
        tl.check_invariants().expect("invariantes");
    }

    /// Elegibilidade é exata: V real = 990 + 5*335/1359 = 991,23. v = 991 é elegível, 992 não.
    /// A comparação `avg >= key * load` não sofre o piso de V.
    #[test]
    fn eligibility_is_exact() {
        let mut tl = Timeline::with_zero_vruntime(1000);
        tl.enqueue(ent(0, 990, NICE_0_LOAD), 10_000);
        tl.enqueue(ent(1, 995, 335 << 10), 20_000);
        assert!(tl.vruntime_eligible(&[], 990));
        assert!(tl.vruntime_eligible(&[], 991));
        assert!(!tl.vruntime_eligible(&[], 992));
        assert!(!tl.vruntime_eligible(&[], 995));
    }

    /// Com o corrente fora da árvore, a conta inclui ele na hora.
    #[test]
    fn curr_counts_in_average() {
        let mut tl = Timeline::with_zero_vruntime(0);
        tl.enqueue(ent(0, 100, NICE_0_LOAD), 1_000);
        let c = curr(9, 300, 500, false);
        // V = (100 + 300) / 2 = 200.
        assert_eq!(tl.avg_vruntime_peek(&[c]), 200);
        assert!(tl.vruntime_eligible(&[c], 200));
        assert!(!tl.vruntime_eligible(&[c], 201));
        // Sozinho, o corrente é a média.
        let empty = Timeline::with_zero_vruntime(0);
        assert_eq!(empty.avg_vruntime_peek(&[c]), 300);
        // Dois rodando e árvore vazia (modo de runqueue única): média dos dois.
        let d = curr(8, 101, 600, false);
        assert_eq!(empty.avg_vruntime_peek(&[c, d]), 200);
    }

    /// pick à mão: três entidades nice 0 com v = 0, 10, 20 (V = 10) e deadlines 30, 15, 25.
    /// Elegíveis: v <= 10, ou seja, as de v = 0 e v = 10. Entre elas a deadline mais cedo é 15 (1).
    /// Trocando a deadline da 1 pra 40: elegíveis 0 (d 30) e 1 (d 40); o mais à esquerda (d 25, 2) não
    /// é elegível, a descida acha a 0.
    #[test]
    fn pick_by_hand() {
        let mut tl = Timeline::with_zero_vruntime(0);
        tl.enqueue(ent(0, 0, NICE_0_LOAD), 30);
        tl.enqueue(ent(1, 10, NICE_0_LOAD), 15);
        tl.enqueue(ent(2, 20, NICE_0_LOAD), 25);
        assert_eq!(tl.pick_eevdf(3, &[], None, &[], true), Some(EntityId::from_index(1)));

        let mut tl = Timeline::with_zero_vruntime(0);
        tl.enqueue(ent(0, 0, NICE_0_LOAD), 30);
        tl.enqueue(ent(1, 10, NICE_0_LOAD), 40);
        tl.enqueue(ent(2, 20, NICE_0_LOAD), 25);
        assert_eq!(tl.pick_eevdf(3, &[], None, &[], true), Some(EntityId::from_index(0)));
        assert_eq!(tl.pick_linear(3, &[], None, &[], true), Some(EntityId::from_index(0)));
    }

    /// RUN_TO_PARITY: corrente elegível e protegido vence mesmo com deadline pior; desprotegido, perde.
    #[test]
    fn run_to_parity_keeps_protected_curr() {
        let mut tl = Timeline::with_zero_vruntime(0);
        tl.enqueue(ent(0, 10, NICE_0_LOAD), 20);
        let mut c = curr(5, 10, 1_000, true);
        let pick = |tl: &Timeline, c: &CurrView, rtp| tl.pick_eevdf(2, &[*c], Some(c), &[], rtp);
        assert_eq!(pick(&tl, &c, true), Some(EntityId::from_index(5)));
        assert_eq!(pick(&tl, &c, false), Some(EntityId::from_index(0)));
        c.protected = false;
        assert_eq!(pick(&tl, &c, true), Some(EntityId::from_index(0)));
        // Corrente com deadline estritamente menor vence a árvore.
        c.deadline = 19;
        assert_eq!(pick(&tl, &c, true), Some(EntityId::from_index(5)));
        // Empate de deadline: a árvore vence (entity_before é estrito).
        c.deadline = 20;
        assert_eq!(pick(&tl, &c, true), Some(EntityId::from_index(0)));
    }

    /// Candidato de fora da árvore (modo de runqueue única) entra na disputa pela deadline.
    #[test]
    fn extra_candidates_compete_by_deadline() {
        let mut tl = Timeline::with_zero_vruntime(0);
        tl.enqueue(ent(0, 10, NICE_0_LOAD), 50);
        let other = curr(7, 10, 30, false);
        let running = [other];
        assert_eq!(tl.pick_eevdf(2, &running, None, &[other], true), Some(EntityId::from_index(7)));
        assert_eq!(tl.pick_linear(2, &running, None, &[other], true), Some(EntityId::from_index(7)));
        // Sem ser candidato, ele só pesa na média.
        assert_eq!(tl.pick_eevdf(2, &running, None, &[], true), Some(EntityId::from_index(0)));
    }

    /// A comparação circular mantém a ordem certa quando o u64 dá a volta.
    #[test]
    fn deadline_order_survives_wraparound() {
        let mut tl = Timeline::with_zero_vruntime(u64::MAX - 100);
        tl.enqueue(ent(0, u64::MAX - 50, NICE_0_LOAD), 10); // deadline depois da volta
        tl.enqueue(ent(1, u64::MAX - 60, NICE_0_LOAD), u64::MAX - 5); // antes da volta
        let first = tl.tree().first().expect("não vazia");
        assert_eq!(tl.tree.value(first).entity, EntityId::from_index(1));
        tl.check_invariants().expect("invariantes");
        // min_vruntime circular: u64::MAX - 60 é menor que u64::MAX - 50.
        let root = tl.tree().root().expect("não vazia");
        assert_eq!(tl.tree().summary(root).min_vruntime, u64::MAX - 60);
    }

    #[test]
    fn min_slice_tracks_tree_and_curr() {
        let mut tl = Timeline::with_zero_vruntime(0);
        assert_eq!(tl.min_slice(&[]), u64::MAX);
        let mut e = ent(0, 0, NICE_0_LOAD);
        e.slice = 500_000;
        let n = tl.enqueue(e, 100);
        tl.enqueue(ent(1, 0, NICE_0_LOAD), 200);
        assert_eq!(tl.min_slice(&[]), 500_000);
        let mut c = curr(2, 0, 0, false);
        c.slice = 100_000;
        assert_eq!(tl.min_slice(&[c]), 100_000);
        tl.set_slice(n, 3_000_000);
        tl.check_invariants().expect("resumo propagado");
        assert_eq!(tl.min_slice(&[]), 2_800_000);
    }
}
