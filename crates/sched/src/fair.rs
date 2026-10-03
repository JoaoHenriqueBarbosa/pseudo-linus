//! As funções do `kernel/sched/fair.c` (6.12.101) sobre a hierarquia de filas e entidades.
//!
//! Os nomes e a ordem dos passos são os do kernel. Onde o kernel usa `cfs_rq->curr`, aqui há duas
//! leituras: "o corrente desta CPU" (pick, `set_next_entity`, `put_prev_entity`, RUN_TO_PARITY) e
//! "quem está rodando nesta fila" (contas de V, `enqueue_entity`, `dequeue_entity`, banda). No modelo de
//! uma runqueue por CPU as duas coincidem; no modo de runqueue única a segunda pode ter várias
//! entidades.
//!
//! Aproximações em relação ao kernel, todas restritas ao PELT:
//!
//! - só o sinal de carga é mantido (sem `runnable` e `util`);
//! - não há propagação de carga de filho pra entidade de grupo (`propagate_entity_load_avg`): a carga
//!   da entidade de grupo é a média do tempo em que ela esteve na fila, com o peso dela. A carga das
//!   filas, que é o que entra no `calc_group_shares`, é exata (média de `load.weight` mais as cargas
//!   anexadas e destacadas nas migrações);
//! - a remoção de carga na migração é feita na hora (o kernel adia pra próxima atualização da fila de
//!   origem, `cfs_rq->removed`);
//! - o relógio do PELT não é escalado por capacidade nem frequência (CPU sempre na frequência máxima).
//!
//! A carga bloqueada (de filas que esvaziaram) decai como no kernel, pelo
//! `update_blocked_averages` que o balanceamento chama ([`crate::balance`]).

use crate::clock::Clock;
use crate::pelt::{get_pelt_divider, update_load_avg as pelt_update_avg, update_load_sum, PELT_MIN_DIVIDER};
use crate::sched::{
    CfsId, CurrSlot, DEQUEUE_DELAYED, DEQUEUE_SLEEP, DEQUEUE_SPECIAL, DO_ATTACH, DO_DETACH, ENQUEUE_DELAYED,
    ENQUEUE_INITIAL, ENQUEUE_MIGRATED, ENQUEUE_WAKEUP, Kind, Running, SKIP_AGE_LOAD, Sched, UPDATE_TG, WF_FORK,
};
use crate::timeline::{CurrView, QueuedEntity, deadline_before};
use crate::weight::{LoadWeight, MIN_SHARES, calc_delta_fair, div_s64, scale_load_down};
use crate::{EntityId, GroupId, TaskId};

impl<C: Clock> Sched<C> {
    // ---------------------------------------------------------------------------------------------
    // Navegação na hierarquia
    // ---------------------------------------------------------------------------------------------

    /// `cfs_rq_of(se)`.
    pub(crate) fn cfs_rq_of(&self, se: EntityId) -> CfsId {
        self.e(se).cfs_rq
    }

    /// `parent_entity(se)`.
    pub(crate) fn parent_entity(&self, se: EntityId) -> Option<EntityId> {
        self.e(se).parent
    }

    /// `group_cfs_rq(se)` (`se->my_q`).
    pub(crate) fn group_cfs_rq(&self, se: EntityId) -> Option<CfsId> {
        match self.e(se).kind {
            Kind::Group { my_q, .. } => Some(my_q),
            Kind::Task(_) => None,
        }
    }

    pub(crate) fn entity_is_task(&self, se: EntityId) -> bool {
        matches!(self.e(se).kind, Kind::Task(_))
    }

    /// Índice do slot da CPU na fila.
    fn slot_index(&self, cfs: CfsId, cpu: usize) -> Option<usize> {
        self.c(cfs).curr.iter().position(|s| s.cpu == cpu)
    }

    /// O corrente da CPU nesta fila (`cfs_rq->curr` visto pela CPU).
    pub(crate) fn curr_of(&self, cfs: CfsId, cpu: usize) -> Option<EntityId> {
        self.slot_index(cfs, cpu).map(|i| self.c(cfs).curr[i].ent)
    }

    /// Diz se a entidade está rodando nesta fila em alguma CPU (`cfs_rq->curr == se`).
    pub(crate) fn is_running(&self, cfs: CfsId, se: EntityId) -> bool {
        self.c(cfs).curr.iter().any(|s| s.ent == se)
    }

    /// Em quantas CPUs a entidade está no caminho de quem roda.
    fn running_count(&self, cfs: CfsId, se: EntityId) -> u32 {
        self.c(cfs).curr.iter().filter(|s| s.ent == se).count() as u32
    }

    /// `cfs_rq->curr != NULL`.
    pub(crate) fn has_curr(&self, cfs: CfsId) -> bool {
        !self.c(cfs).curr.is_empty()
    }

    fn view(&self, se: EntityId) -> CurrView {
        let e = self.e(se);
        CurrView {
            entity: se,
            vruntime: e.vruntime,
            deadline: e.deadline,
            weight: e.load.weight,
            slice: e.slice,
            protected: e.vlag as u64 == e.deadline,
        }
    }

    /// Quem está rodando nesta fila e continua nela (`curr && curr->on_rq`), sem repetir entidade.
    pub(crate) fn running(&self, cfs: CfsId) -> Running {
        let mut r = Running::default();
        for s in &self.c(cfs).curr {
            if self.e(s.ent).on_rq {
                r.push(self.view(s.ent));
            }
        }
        r
    }

    /// O corrente da CPU nesta fila, se continuar na fila.
    fn this_view(&self, cfs: CfsId, cpu: usize) -> Option<CurrView> {
        self.curr_of(cfs, cpu).filter(|&se| self.e(se).on_rq).map(|se| self.view(se))
    }

    /// Candidatos fora da árvore no modo de runqueue única: entidades de grupo rodando em outra CPU que
    /// ainda têm tarefa pronta sem CPU.
    fn extra_views(&self, cfs: CfsId, cpu: usize) -> Vec<CurrView> {
        let mut out: Vec<CurrView> = Vec::new();
        if !self.shared() {
            return out;
        }
        let this = self.curr_of(cfs, cpu);
        for s in &self.c(cfs).curr {
            if Some(s.ent) == this || out.iter().any(|v| v.entity == s.ent) || !self.e(s.ent).on_rq {
                continue;
            }
            if self.has_spare(cfs, s.ent) {
                out.push(self.view(s.ent));
            }
        }
        out
    }

    /// Diz se uma entidade de grupo tem mais tarefas prontas do que CPUs passando por ela.
    fn has_spare(&self, cfs: CfsId, se: EntityId) -> bool {
        match self.group_cfs_rq(se) {
            Some(q) => self.c(q).h_nr_runnable > self.running_count(cfs, se),
            None => false,
        }
    }

    /// Diz se a CPU pode escolher a entidade agora (no modo de runqueue única, não pode pegar quem já
    /// roda em outra CPU, a não ser grupo com tarefa sobrando).
    fn pickable(&self, cfs: CfsId, se: EntityId, cpu: usize) -> bool {
        if !self.e(se).on_rq {
            return false;
        }
        if self.e(se).node.is_some() || self.curr_of(cfs, cpu) == Some(se) {
            return true;
        }
        self.is_running(cfs, se) && self.has_spare(cfs, se)
    }

    // ---------------------------------------------------------------------------------------------
    // V, elegibilidade, lag, deadline
    // ---------------------------------------------------------------------------------------------

    /// `avg_vruntime(cfs_rq)` (move `zero_vruntime`).
    pub(crate) fn avg_vruntime(&mut self, cfs: CfsId) -> u64 {
        let r = self.running(cfs);
        self.c_mut(cfs).timeline.avg_vruntime(r.as_slice())
    }

    /// V sem mexer no estado.
    pub(crate) fn avg_vruntime_peek_cfs(&self, cfs: CfsId) -> u64 {
        let r = self.running(cfs);
        self.c(cfs).timeline.avg_vruntime_peek(r.as_slice())
    }

    /// `entity_eligible(cfs_rq, se)`.
    pub(crate) fn entity_eligible(&self, cfs: CfsId, se: EntityId) -> bool {
        let r = self.running(cfs);
        self.c(cfs).timeline.vruntime_eligible(r.as_slice(), self.e(se).vruntime)
    }

    /// `cfs_rq_min_slice`.
    pub(crate) fn cfs_rq_min_slice(&self, cfs: CfsId) -> u64 {
        let r = self.running(cfs);
        self.c(cfs).timeline.min_slice(r.as_slice())
    }

    /// `pick_eevdf(cfs_rq)` visto pela CPU.
    pub(crate) fn pick_eevdf(&self, cfs: CfsId, cpu: usize) -> Option<EntityId> {
        let r = self.running(cfs);
        let this = self.this_view(cfs, cpu);
        let extra = self.extra_views(cfs, cpu);
        let c = self.c(cfs);
        c.timeline.pick_eevdf(c.nr_running, r.as_slice(), this.as_ref(), &extra, self.cfg.features.run_to_parity)
    }

    /// A mesma escolha por força bruta (oráculo).
    pub(crate) fn pick_linear(&self, cfs: CfsId, cpu: usize) -> Option<EntityId> {
        let r = self.running(cfs);
        let this = self.this_view(cfs, cpu);
        let extra = self.extra_views(cfs, cpu);
        let c = self.c(cfs);
        c.timeline.pick_linear(c.nr_running, r.as_slice(), this.as_ref(), &extra, self.cfg.features.run_to_parity)
    }

    /// `entity_lag`: `V - v`, limitado a `calc_delta_fair(max(2 * slice, TICK_NSEC))`.
    pub(crate) fn entity_lag(&self, avruntime: u64, se: EntityId) -> i64 {
        let e = self.e(se);
        let vlag = avruntime.wrapping_sub(e.vruntime) as i64;
        let limit = calc_delta_fair((2 * e.slice).max(self.cfg.tunables.tick_nsec), &e.load) as i64;
        vlag.clamp(-limit, limit)
    }

    /// `update_entity_lag`.
    fn update_entity_lag(&mut self, cfs: CfsId, se: EntityId) {
        let v = self.avg_vruntime(cfs);
        let lag = self.entity_lag(v, se);
        self.e_mut(se).vlag = lag;
    }

    /// `set_protect_slice`: guarda a deadline do pick em `vlag`.
    fn set_protect_slice(&mut self, se: EntityId) {
        let e = self.e_mut(se);
        e.vlag = e.deadline as i64;
    }

    /// `protect_slice`.
    pub(crate) fn protect_slice(&self, se: EntityId) -> bool {
        let e = self.e(se);
        e.vlag as u64 == e.deadline
    }

    /// `cancel_protect_slice`.
    fn cancel_protect_slice(&mut self, se: EntityId) {
        if self.protect_slice(se) {
            let e = self.e_mut(se);
            e.vlag = e.deadline.wrapping_add(1) as i64;
        }
    }

    /// `update_deadline`: quando o vruntime alcança a deadline, renova a fatia
    /// (`deadline = vruntime + calc_delta_fair(slice)`) e devolve `true` (pedido de reescalonamento).
    pub(crate) fn update_deadline(&mut self, cfs: CfsId, se: EntityId) -> bool {
        let e = self.e(se);
        if (e.vruntime.wrapping_sub(e.deadline) as i64) < 0 {
            return false;
        }
        let base = self.base_slice;
        let e = self.e_mut(se);
        if !e.custom_slice {
            e.slice = base;
        }
        e.deadline = e.vruntime.wrapping_add(calc_delta_fair(e.slice, &e.load));
        self.avg_vruntime(cfs);
        true
    }

    /// `did_preempt_short`.
    fn did_preempt_short(&self, cfs: CfsId, curr: EntityId) -> bool {
        if !self.cfg.features.preempt_short {
            return false;
        }
        if self.protect_slice(curr) {
            return false;
        }
        !self.entity_eligible(cfs, curr)
    }

    /// `do_preempt_short(cfs_rq, pse, se)`.
    fn do_preempt_short(&self, cfs: CfsId, pse: EntityId, se: EntityId) -> bool {
        if !self.cfg.features.preempt_short {
            return false;
        }
        if self.e(pse).slice >= self.e(se).slice {
            return false;
        }
        if !self.entity_eligible(cfs, pse) {
            return false;
        }
        if deadline_before(self.e(pse).deadline, self.e(se).deadline) {
            return true;
        }
        if !self.entity_eligible(cfs, se) {
            return true;
        }
        false
    }

    /// `update_curr`: cobra de quem roda nesta fila o tempo desde o último acerto, avança o vruntime
    /// pelo peso, renova a deadline se a fatia acabou, desconta do runtime de banda e pede
    /// reescalonamento quando cabe.
    pub(crate) fn update_curr(&mut self, cfs: CfsId) {
        let n = self.c(cfs).curr.len();
        for i in 0..n {
            let slot = self.c(cfs).curr[i];
            let now = self.now;
            let delta_exec = now.wrapping_sub(slot.exec_start) as i64;
            if delta_exec <= 0 {
                continue;
            }
            self.c_mut(cfs).curr[i].exec_start = now;
            let se = slot.ent;
            let e = self.e_mut(se);
            e.exec_start = now;
            e.sum_exec_runtime += delta_exec as u64;
            e.vruntime = e.vruntime.wrapping_add(calc_delta_fair(delta_exec as u64, &e.load));
            let resched = self.update_deadline(cfs, se);
            self.account_cfs_rq_runtime(cfs, delta_exec as u64);
            if self.c(cfs).nr_running == 1 {
                continue;
            }
            if resched || self.did_preempt_short(cfs, se) {
                self.resched_cpu(slot.cpu);
                self.clear_buddies(cfs, se);
            }
        }
    }

    // ---------------------------------------------------------------------------------------------
    // Árvore e contagem
    // ---------------------------------------------------------------------------------------------

    /// `__enqueue_entity`.
    fn enqueue_in_tree(&mut self, cfs: CfsId, se: EntityId) {
        let e = self.e(se);
        let q = QueuedEntity { entity: se, vruntime: e.vruntime, slice: e.slice, weight: e.load.weight };
        let deadline = e.deadline;
        let node = self.c_mut(cfs).timeline.enqueue(q, deadline);
        self.e_mut(se).node = Some(node);
    }

    /// `__dequeue_entity`.
    fn dequeue_from_tree(&mut self, cfs: CfsId, se: EntityId) {
        let node = self.e_mut(se).node.take().expect("entidade fora da árvore");
        self.c_mut(cfs).timeline.dequeue(node);
    }

    /// `se->slice = slice`, propagando o `min_slice` se a entidade estiver na árvore.
    fn set_entity_slice(&mut self, cfs: CfsId, se: EntityId, slice: u64) {
        self.e_mut(se).slice = slice;
        if let Some(node) = self.e(se).node {
            self.c_mut(cfs).timeline.set_slice(node, slice);
        }
    }

    /// `account_entity_enqueue`.
    fn account_entity_enqueue(&mut self, cfs: CfsId, se: EntityId) {
        let w = self.e(se).load.weight;
        let c = self.c_mut(cfs);
        c.load += w;
        c.nr_running += 1;
    }

    /// `account_entity_dequeue`.
    fn account_entity_dequeue(&mut self, cfs: CfsId, se: EntityId) {
        let w = self.e(se).load.weight;
        let c = self.c_mut(cfs);
        c.load -= w;
        c.nr_running -= 1;
    }

    // ---------------------------------------------------------------------------------------------
    // place_entity, enqueue_entity, dequeue_entity
    // ---------------------------------------------------------------------------------------------

    /// `place_entity`: posiciona quem entra na fila. Com PLACE_LAG, o lag guardado é inflado por
    /// `(W + w) / W` pra sobreviver ao deslocamento de V causado pela própria entrada, e
    /// `v = V - lag`. Com PLACE_REL_DEADLINE e deadline relativa guardada, ela é restaurada; senão
    /// `deadline = v + vslice`, com meia fatia pra tarefa nova (PLACE_DEADLINE_INITIAL).
    pub(crate) fn place_entity(&mut self, cfs: CfsId, se: EntityId, flags: u32) {
        let vruntime = self.avg_vruntime(cfs);
        let mut lag: i64 = 0;
        let base = self.base_slice;
        let e = self.e_mut(se);
        if !e.custom_slice {
            e.slice = base;
        }
        let mut vslice = calc_delta_fair(e.slice, &e.load);

        if self.cfg.features.place_lag && self.c(cfs).nr_running > 0 {
            lag = self.e(se).vlag;
            let mut load = self.c(cfs).timeline.avg_load();
            for c in self.running(cfs).as_slice() {
                load += scale_load_down(c.weight);
            }
            lag = lag.wrapping_mul((load + scale_load_down(self.e(se).load.weight)) as i64);
            if load == 0 {
                load = 1;
            }
            lag = div_s64(lag, load as i32);
        }

        let features = self.cfg.features;
        let e = self.e_mut(se);
        e.vruntime = vruntime.wrapping_sub(lag as u64);

        if features.place_rel_deadline && e.rel_deadline {
            e.deadline = e.deadline.wrapping_add(e.vruntime);
            e.rel_deadline = false;
            return;
        }

        if features.place_deadline_initial && flags & ENQUEUE_INITIAL != 0 {
            vslice /= 2;
        }
        e.deadline = e.vruntime.wrapping_add(vslice);
    }

    /// `enqueue_entity`.
    pub(crate) fn enqueue_entity(&mut self, cfs: CfsId, se: EntityId, flags: u32) {
        let curr = self.is_running(cfs, se);
        // O corrente precisa ser renormalizado antes do update_curr.
        if curr {
            self.place_entity(cfs, se, flags);
        }
        self.update_curr(cfs);
        self.update_load_avg(cfs, se, UPDATE_TG | DO_ATTACH);
        self.update_cfs_group(se);
        if !curr {
            self.place_entity(cfs, se, flags);
        }
        self.account_entity_enqueue(cfs, se);
        if flags & ENQUEUE_MIGRATED != 0 {
            self.e_mut(se).exec_start = 0;
        }
        if !curr {
            self.enqueue_in_tree(cfs, se);
        }
        self.e_mut(se).on_rq = true;
        if self.c(cfs).nr_running == 1 {
            self.check_enqueue_throttle(cfs);
        }
    }

    /// `set_delayed`.
    fn set_delayed(&mut self, se: EntityId) {
        self.e_mut(se).sched_delayed = true;
        if !self.entity_is_task(se) {
            return;
        }
        let mut s = Some(se);
        while let Some(x) = s {
            let cfs = self.cfs_rq_of(x);
            let c = self.c_mut(cfs);
            c.h_nr_runnable -= 1;
            c.h_nr_delayed += 1;
            if self.cfs_rq_throttled(cfs) {
                break;
            }
            s = self.parent_entity(x);
        }
    }

    /// `clear_delayed`.
    fn clear_delayed(&mut self, se: EntityId) {
        self.e_mut(se).sched_delayed = false;
        if !self.entity_is_task(se) {
            return;
        }
        let mut s = Some(se);
        while let Some(x) = s {
            let cfs = self.cfs_rq_of(x);
            let c = self.c_mut(cfs);
            c.h_nr_runnable += 1;
            c.h_nr_delayed -= 1;
            if self.cfs_rq_throttled(cfs) {
                break;
            }
            s = self.parent_entity(x);
        }
    }

    /// `finish_delayed_dequeue_entity`: com DELAY_ZERO, lag positivo vira zero.
    fn finish_delayed_dequeue_entity(&mut self, se: EntityId) {
        self.clear_delayed(se);
        let delay_zero = self.cfg.features.delay_zero;
        let e = self.e_mut(se);
        if delay_zero && e.vlag > 0 {
            e.vlag = 0;
        }
    }

    /// `dequeue_entity`. Devolve `false` quando o dequeue foi atrasado: a entidade dorme sem ser
    /// elegível e fica na árvore, marcada, até ser escolhida ou acordar.
    pub(crate) fn dequeue_entity(&mut self, cfs: CfsId, se: EntityId, flags: u32) -> bool {
        let sleep = flags & DEQUEUE_SLEEP != 0;
        self.update_curr(cfs);
        self.clear_buddies(cfs, se);

        if flags & DEQUEUE_DELAYED == 0 {
            let mut delay = sleep;
            // Estados especiais não toleram wakeup espúrio, então não atrasam.
            if flags & DEQUEUE_SPECIAL != 0 {
                delay = false;
            }
            if self.cfg.features.delay_dequeue && delay && !self.entity_eligible(cfs, se) {
                self.update_load_avg(cfs, se, 0);
                self.set_delayed(se);
                return false;
            }
        }

        let mut action = UPDATE_TG;
        if let Kind::Task(d) = &self.e(se).kind
            && d.migrating
        {
            action |= DO_DETACH;
        }
        self.update_load_avg(cfs, se, action);

        self.update_entity_lag(cfs, se);
        if self.cfg.features.place_rel_deadline && !sleep {
            let e = self.e_mut(se);
            e.deadline = e.deadline.wrapping_sub(e.vruntime);
            e.rel_deadline = true;
        }

        if !self.is_running(cfs, se) {
            self.dequeue_from_tree(cfs, se);
        }
        self.e_mut(se).on_rq = false;
        self.account_entity_dequeue(cfs, se);

        // Devolve o runtime que sobrou quando a fila esvazia.
        self.return_cfs_rq_runtime(cfs);

        self.update_cfs_group(se);

        if flags & DEQUEUE_DELAYED != 0 {
            self.finish_delayed_dequeue_entity(se);
        }
        true
    }

    /// `requeue_delayed_entity`: quem acorda ainda na fila deixa de ser atrasado; com DELAY_ZERO e lag
    /// positivo, é recolocado com lag zero (sai e volta pra árvore no V atual).
    pub(crate) fn requeue_delayed_entity(&mut self, se: EntityId) {
        let cfs = self.cfs_rq_of(se);
        if self.cfg.features.delay_zero {
            self.update_entity_lag(cfs, se);
            if self.e(se).vlag > 0 {
                self.c_mut(cfs).nr_running -= 1;
                let curr = self.is_running(cfs, se);
                if !curr {
                    self.dequeue_from_tree(cfs, se);
                }
                self.e_mut(se).vlag = 0;
                self.place_entity(cfs, se, 0);
                if !curr {
                    self.enqueue_in_tree(cfs, se);
                }
                self.c_mut(cfs).nr_running += 1;
            }
        }
        self.update_load_avg(cfs, se, 0);
        self.clear_delayed(se);
    }

    // ---------------------------------------------------------------------------------------------
    // enqueue_task_fair e dequeue_entities (a hierarquia)
    // ---------------------------------------------------------------------------------------------

    /// `enqueue_task_fair`: enfileira a tarefa e sobe pela hierarquia enfileirando cada entidade de
    /// grupo que ainda não estava na fila, até achar uma que estava (ou uma fila estrangulada). Depois
    /// atualiza os níveis de cima (PELT, peso do grupo, fatia, contagens).
    pub(crate) fn enqueue_task_fair(&mut self, p: TaskId, mut flags: u32) {
        let task_se = p.entity();
        if flags & ENQUEUE_DELAYED != 0 {
            self.requeue_delayed_entity(task_se);
            return;
        }
        let task_new = flags & ENQUEUE_WAKEUP == 0;
        let h_nr_delayed: u32 = if task_new { u32::from(self.e(task_se).sched_delayed) } else { 0 };
        let mut slice = 0u64;
        let mut se = Some(task_se);
        let rq_slot = self.rq_slot_of_cfs(self.cfs_rq_of(task_se));

        while let Some(s) = se {
            if self.e(s).on_rq {
                if self.e(s).sched_delayed {
                    self.requeue_delayed_entity(s);
                }
                break;
            }
            let cfs = self.cfs_rq_of(s);
            // A fatia de uma entidade de grupo é a menor fatia da fila dela.
            if slice != 0 {
                let e = self.e_mut(s);
                e.slice = slice;
                e.custom_slice = true;
            }
            self.enqueue_entity(cfs, s, flags);
            slice = self.cfs_rq_min_slice(cfs);

            let c = self.c_mut(cfs);
            if h_nr_delayed == 0 {
                c.h_nr_runnable += 1;
            }
            c.h_nr_queued += 1;
            c.h_nr_delayed += h_nr_delayed;

            if self.cfs_rq_throttled(cfs) {
                return;
            }
            flags = ENQUEUE_WAKEUP;
            se = self.parent_entity(s);
        }

        while let Some(s) = se {
            let cfs = self.cfs_rq_of(s);
            self.update_load_avg(cfs, s, UPDATE_TG);
            self.update_cfs_group(s);
            self.set_entity_slice(cfs, s, slice);
            slice = self.cfs_rq_min_slice(cfs);

            let c = self.c_mut(cfs);
            if h_nr_delayed == 0 {
                c.h_nr_runnable += 1;
            }
            c.h_nr_queued += 1;
            c.h_nr_delayed += h_nr_delayed;

            if self.cfs_rq_throttled(cfs) {
                return;
            }
            se = self.parent_entity(s);
        }

        // add_nr_running
        self.cpus[rq_slot].nr_running += 1;
    }

    /// `dequeue_entities`: -1 se atrasou, 0 se parou numa fila estrangulada, 1 se completou. Sobe pela
    /// hierarquia tirando entidades de grupo que ficaram vazias; se a tarefa dorme e a fila dela
    /// continua com carga, marca o grupo como "next" (`set_next_buddy`). No dequeue de uma entidade
    /// atrasada (vindo do pick), termina o bloqueio da tarefa (`__block_task`).
    pub(crate) fn dequeue_entities(&mut self, se0: EntityId, mut flags: u32) -> i32 {
        let task_sleep = flags & DEQUEUE_SLEEP != 0;
        let task_delayed = flags & DEQUEUE_DELAYED != 0;
        let mut p: Option<TaskId> = None;
        let mut h_nr_queued = 0u32;
        let mut h_nr_delayed = 0u32;
        let mut slice = 0u64;
        let mut ret = 0;
        let rq_slot = self.rq_slot_of_cfs(self.cfs_rq_of(se0));

        if self.entity_is_task(se0) {
            p = Some(TaskId::from_index(se0.index()));
            h_nr_queued = 1;
            if !task_sleep && !task_delayed {
                h_nr_delayed = u32::from(self.e(se0).sched_delayed);
            }
        }

        let mut se = Some(se0);
        let mut done = false;
        while let Some(s) = se {
            let cfs = self.cfs_rq_of(s);
            if !self.dequeue_entity(cfs, s, flags) {
                if p.is_some() && s == se0 {
                    return -1;
                }
                slice = self.cfs_rq_min_slice(cfs);
                break;
            }
            let c = self.c_mut(cfs);
            if h_nr_delayed == 0 {
                c.h_nr_runnable -= h_nr_queued;
            }
            c.h_nr_queued -= h_nr_queued;
            c.h_nr_delayed -= h_nr_delayed;

            if self.cfs_rq_throttled(cfs) {
                done = true;
                break;
            }

            // Não tira o pai se ele tem outras entidades além desta.
            if self.c(cfs).load != 0 {
                slice = self.cfs_rq_min_slice(cfs);
                se = self.parent_entity(s);
                if task_sleep
                    && let Some(parent) = se
                    && !self.throttled_hierarchy(cfs)
                {
                    self.set_next_buddy(parent);
                }
                break;
            }
            flags |= DEQUEUE_SLEEP;
            flags &= !(DEQUEUE_DELAYED | DEQUEUE_SPECIAL);
            se = self.parent_entity(s);
        }

        if !done {
            while let Some(s) = se {
                let cfs = self.cfs_rq_of(s);
                self.update_load_avg(cfs, s, UPDATE_TG);
                self.update_cfs_group(s);
                self.set_entity_slice(cfs, s, slice);
                slice = self.cfs_rq_min_slice(cfs);

                let c = self.c_mut(cfs);
                if h_nr_delayed == 0 {
                    c.h_nr_runnable -= h_nr_queued;
                }
                c.h_nr_queued -= h_nr_queued;
                c.h_nr_delayed -= h_nr_delayed;

                if self.cfs_rq_throttled(cfs) {
                    done = true;
                    break;
                }
                se = self.parent_entity(s);
            }
            if !done {
                // sub_nr_running
                self.cpus[rq_slot].nr_running -= h_nr_queued;
                ret = 1;
            }
        }

        if let Some(task) = p
            && task_delayed
        {
            self.cpus[rq_slot].stats.nr_delayed_dequeues += 1;
            self.finish_block_task(task);
        }
        ret
    }

    /// `dequeue_task_fair`: `false` se o dequeue ficou atrasado.
    pub(crate) fn dequeue_task_fair(&mut self, p: TaskId, flags: u32) -> bool {
        self.dequeue_entities(p.entity(), flags) >= 0
    }

    // ---------------------------------------------------------------------------------------------
    // pick, set_next, put_prev
    // ---------------------------------------------------------------------------------------------

    /// `set_next_entity`: o escolhido sai da árvore (quem roda não fica nela), ganha a proteção de fatia
    /// e começa a contar tempo agora.
    pub(crate) fn set_next_entity(&mut self, cfs: CfsId, se: EntityId, cpu: usize) {
        self.clear_buddies(cfs, se);
        if self.e(se).on_rq {
            if self.e(se).node.is_some() {
                self.dequeue_from_tree(cfs, se);
            }
            self.update_load_avg(cfs, se, UPDATE_TG);
            self.set_protect_slice(se);
        }
        let now = self.now;
        let e = self.e_mut(se);
        e.exec_start = now;
        e.prev_sum_exec_runtime = e.sum_exec_runtime;
        debug_assert!(self.slot_index(cfs, cpu).is_none(), "a CPU já tem corrente nesta fila");
        self.c_mut(cfs).curr.push(CurrSlot { cpu, ent: se, exec_start: now });
    }

    /// `put_prev_entity`: se o anterior continua na fila, cobra o tempo dele, testa a banda da fila e
    /// devolve ele à árvore.
    pub(crate) fn put_prev_entity(&mut self, cfs: CfsId, prev: EntityId, cpu: usize) {
        if self.e(prev).on_rq {
            self.update_curr(cfs);
        }
        // Estrangula a fila que gastou o runtime.
        self.check_cfs_rq_runtime(cfs);
        let i = self.slot_index(cfs, cpu).expect("put_prev sem corrente");
        debug_assert_eq!(self.c(cfs).curr[i].ent, prev);
        self.c_mut(cfs).curr.swap_remove(i);
        if self.e(prev).on_rq && !self.is_running(cfs, prev) {
            self.enqueue_in_tree(cfs, prev);
            self.update_load_avg(cfs, prev, 0);
        }
    }

    /// `pick_next_entity`: o "next" se for elegível, senão o `pick_eevdf`. Se o escolhido é uma
    /// entidade atrasada, ela sai da fila de vez e o pick recomeça (`None`).
    fn pick_next_entity(&mut self, cfs: CfsId, cpu: usize) -> Result<EntityId, ()> {
        if self.cfg.features.pick_buddy
            && let Some(n) = self.c(cfs).next
            && self.pickable(cfs, n, cpu)
            && self.entity_eligible(cfs, n)
        {
            return Ok(n);
        }
        let Some(se) = self.pick_eevdf(cfs, cpu) else { return Err(()) };
        if self.e(se).sched_delayed {
            self.dequeue_entities(se, DEQUEUE_SLEEP | DEQUEUE_DELAYED);
            return Err(());
        }
        Ok(se)
    }

    /// `pick_task_fair`: desce da raiz até uma tarefa. Em cada nível cobra o corrente e testa a banda;
    /// se a fila foi estrangulada ou o escolhido era atrasado, recomeça da raiz.
    pub(crate) fn pick_task_fair(&mut self, cpu: usize) -> Option<TaskId> {
        let root = self.cpus[cpu].root;
        'again: loop {
            if self.c(root).nr_running == 0 {
                return None;
            }
            let mut cfs = root;
            loop {
                if let Some(c) = self.curr_of(cfs, cpu)
                    && self.e(c).on_rq
                {
                    self.update_curr(cfs);
                }
                if self.check_cfs_rq_runtime(cfs) {
                    continue 'again;
                }
                let se = match self.pick_next_entity(cfs, cpu) {
                    Ok(se) => se,
                    Err(()) => {
                        if self.shared() && self.c(cfs).nr_running > 0 && self.pick_eevdf(cfs, cpu).is_none() {
                            // Todas as tarefas desta fila já estão em outras CPUs.
                            return None;
                        }
                        continue 'again;
                    }
                };
                match self.group_cfs_rq(se) {
                    None => return Some(TaskId::from_index(se.index())),
                    Some(q) => cfs = q,
                }
            }
        }
    }

    /// `pick_next_task_fair`: escolhe e troca só o que mudou entre o caminho do anterior e o do próximo
    /// (put/set na profundidade em que os dois se encontram). Sem tarefa, tenta puxar trabalho de outra
    /// CPU (`sched_balance_newidle`) e, se não houver, deixa a CPU ociosa.
    pub(crate) fn pick_next_task_fair(&mut self, cpu: usize, prev: Option<TaskId>) -> Option<TaskId> {
        loop {
            let Some(p) = self.pick_task_fair(cpu) else {
                if self.cfg.balance.enabled && self.cfg.balance.newidle && self.newidle_balance(cpu) {
                    continue;
                }
                if let Some(pr) = prev {
                    self.put_prev_task_fair(cpu, pr);
                }
                return None;
            };
            match prev {
                None => {
                    self.set_next_task_fair(cpu, p);
                }
                Some(pr) if pr != p => {
                    let mut se = p.entity();
                    let mut pse = pr.entity();
                    loop {
                        let same = self.cfs_rq_of(se) == self.cfs_rq_of(pse);
                        if same {
                            break;
                        }
                        let se_depth = self.e(se).depth;
                        let pse_depth = self.e(pse).depth;
                        if se_depth <= pse_depth {
                            let c = self.cfs_rq_of(pse);
                            self.put_prev_entity(c, pse, cpu);
                            pse = self.parent_entity(pse).expect("caminho do anterior");
                        }
                        if se_depth >= pse_depth {
                            let c = self.cfs_rq_of(se);
                            self.set_next_entity(c, se, cpu);
                            se = self.parent_entity(se).expect("caminho do próximo");
                        }
                    }
                    let c = self.cfs_rq_of(se);
                    self.put_prev_entity(c, pse, cpu);
                    self.set_next_entity(c, se, cpu);
                }
                Some(_) => {}
            }
            return Some(p);
        }
    }

    /// `put_prev_task_fair`: put_prev em todos os níveis.
    pub(crate) fn put_prev_task_fair(&mut self, cpu: usize, prev: TaskId) {
        let mut se = Some(prev.entity());
        while let Some(s) = se {
            let cfs = self.cfs_rq_of(s);
            self.put_prev_entity(cfs, s, cpu);
            se = self.parent_entity(s);
        }
    }

    /// `set_next_task_fair`: set_next em todos os níveis.
    pub(crate) fn set_next_task_fair(&mut self, cpu: usize, p: TaskId) {
        let mut se = Some(p.entity());
        while let Some(s) = se {
            let cfs = self.cfs_rq_of(s);
            self.set_next_entity(cfs, s, cpu);
            self.account_cfs_rq_runtime(cfs, 0);
            se = self.parent_entity(s);
        }
    }

    /// `set_next_buddy`.
    fn set_next_buddy(&mut self, se: EntityId) {
        let mut s = Some(se);
        while let Some(x) = s {
            if !self.e(x).on_rq {
                return;
            }
            let cfs = self.cfs_rq_of(x);
            self.c_mut(cfs).next = Some(x);
            s = self.parent_entity(x);
        }
    }

    /// `clear_buddies` (`__clear_buddies_next`).
    pub(crate) fn clear_buddies(&mut self, cfs: CfsId, se: EntityId) {
        if self.c(cfs).next != Some(se) {
            return;
        }
        let mut s = Some(se);
        while let Some(x) = s {
            let q = self.cfs_rq_of(x);
            if self.c(q).next != Some(x) {
                break;
            }
            self.c_mut(q).next = None;
            s = self.parent_entity(x);
        }
    }

    /// `find_matching_se`: sobe as duas entidades até ficarem na mesma fila.
    fn find_matching_se(&self, mut se: EntityId, mut pse: EntityId) -> (EntityId, EntityId) {
        let mut se_depth = self.e(se).depth;
        let mut pse_depth = self.e(pse).depth;
        while se_depth > pse_depth {
            se_depth -= 1;
            se = self.parent_entity(se).expect("profundidade");
        }
        while pse_depth > se_depth {
            pse_depth -= 1;
            pse = self.parent_entity(pse).expect("profundidade");
        }
        while self.cfs_rq_of(se) != self.cfs_rq_of(pse) {
            se = self.parent_entity(se).expect("ancestral comum");
            pse = self.parent_entity(pse).expect("ancestral comum");
        }
        (se, pse)
    }

    /// `check_preempt_wakeup_fair`: quem acorda preempta se, no nível em que ele e o corrente são
    /// irmãos, virou a escolha do `pick_eevdf`. Antes, cobra o corrente e, com PREEMPT_SHORT, cancela a
    /// proteção dele se quem acorda tem fatia menor e é elegível.
    pub(crate) fn check_preempt_wakeup_fair(&mut self, cpu: usize, curr: TaskId, p: TaskId, wake_flags: u32) {
        if curr == p {
            return;
        }
        let pse0 = p.entity();
        let se0 = curr.entity();
        if self.throttled_hierarchy(self.cfs_rq_of(pse0)) {
            return;
        }
        if self.cfg.features.next_buddy && wake_flags & WF_FORK == 0 && !self.e(pse0).sched_delayed {
            self.set_next_buddy(pse0);
        }
        if self.cpus[cpu].need_resched {
            return;
        }
        if !self.cfg.features.wakeup_preemption {
            return;
        }
        let (se, pse) = self.find_matching_se(se0, pse0);
        let cfs = self.cfs_rq_of(se);
        self.update_curr(cfs);
        if self.do_preempt_short(cfs, pse, se) {
            self.cancel_protect_slice(se);
        }
        if self.pick_eevdf(cfs, cpu) == Some(pse) {
            self.cpus[cpu].stats.nr_wakeup_preemptions += 1;
            self.resched_cpu(cpu);
        }
    }

    // ---------------------------------------------------------------------------------------------
    // Tick
    // ---------------------------------------------------------------------------------------------

    /// `task_tick_fair`: `entity_tick` em cada nível, de baixo pra cima.
    pub(crate) fn task_tick_fair(&mut self, curr: TaskId) {
        let mut se = Some(curr.entity());
        while let Some(s) = se {
            let cfs = self.cfs_rq_of(s);
            // entity_tick
            self.update_curr(cfs);
            self.update_load_avg(cfs, s, UPDATE_TG);
            self.update_cfs_group(s);
            se = self.parent_entity(s);
        }
    }

    // ---------------------------------------------------------------------------------------------
    // Peso: reweight_entity, peso de grupo
    // ---------------------------------------------------------------------------------------------

    /// `reweight_entity`. Na fila, preserva o lag (`v' = V - (V - v) * w / w'`) e a deadline relativa
    /// (`d' = V + (d - V) * w / w'`); fora da fila, só escala o `vlag` guardado. Também reescala a carga
    /// PELT da entidade pro peso novo.
    pub(crate) fn reweight_entity(&mut self, cfs: CfsId, se: EntityId, weight: u64) {
        let curr = self.is_running(cfs, se);
        let on_rq = self.e(se).on_rq;
        let mut avruntime = 0;
        if on_rq {
            self.update_curr(cfs);
            avruntime = self.avg_vruntime(cfs);
            if !curr {
                self.dequeue_from_tree(cfs, se);
            }
            let w = self.e(se).load.weight;
            self.c_mut(cfs).load -= w;
        }
        self.dequeue_load_avg(cfs, se);

        if on_rq {
            self.reweight_eevdf(se, avruntime, weight);
        } else {
            let e = self.e_mut(se);
            e.vlag = div_s64(e.vlag.wrapping_mul(e.load.weight as i64), weight as i32);
        }

        self.e_mut(se).load = LoadWeight::with_weight(weight);
        // se->avg.load_avg = se_weight(se) * se->avg.load_sum / divider
        let divider = get_pelt_divider(&self.e(se).avg);
        let e = self.e_mut(se);
        e.avg.load_avg = scale_load_down(e.load.weight) * e.avg.load_sum / divider;

        self.enqueue_load_avg(cfs, se);
        if on_rq {
            self.c_mut(cfs).load += weight;
            if !curr {
                self.enqueue_in_tree(cfs, se);
            }
        }
    }

    /// `reweight_eevdf`.
    fn reweight_eevdf(&mut self, se: EntityId, avruntime: u64, weight: u64) {
        let old_weight = self.e(se).load.weight;
        if avruntime != self.e(se).vruntime {
            let vlag = self.entity_lag(avruntime, se);
            let vlag = div_s64(vlag.wrapping_mul(old_weight as i64), weight as i32);
            self.e_mut(se).vruntime = avruntime.wrapping_sub(vlag as u64);
        }
        let e = self.e_mut(se);
        let vslice = e.deadline.wrapping_sub(avruntime) as i64;
        let vslice = div_s64(vslice.wrapping_mul(old_weight as i64), weight as i32);
        e.deadline = avruntime.wrapping_add(vslice as u64);
    }

    /// `calc_group_shares`: `tg->shares * load / tg_weight`, com `load = max(load.weight, load_avg)`
    /// desta fila e `tg_weight = tg->load_avg - contribuição desta fila + load`, limitado a
    /// `[MIN_SHARES, tg->shares]`. Numa CPU só, dá exatamente os shares do grupo.
    pub(crate) fn calc_group_shares(&self, gcfs: CfsId) -> u64 {
        let c = self.c(gcfs);
        let tg = self.g(c.tg);
        let tg_shares = tg.shares as i64;
        let load = (scale_load_down(c.load) as i64).max(c.avg.load_avg as i64);
        let mut tg_weight = tg.load_avg;
        tg_weight -= c.tg_load_avg_contrib as i64;
        tg_weight += load;
        let mut shares = tg_shares * load;
        if tg_weight != 0 {
            shares /= tg_weight;
        }
        shares.clamp(MIN_SHARES as i64, tg_shares) as u64
    }

    /// `update_cfs_group`: recalcula o peso da entidade de grupo a partir da fila dela. Fila vazia
    /// preserva o peso (importa pro DELAY_DEQUEUE); hierarquia estrangulada também.
    pub(crate) fn update_cfs_group(&mut self, se: EntityId) {
        let Some(gcfs) = self.group_cfs_rq(se) else { return };
        if self.c(gcfs).load == 0 {
            return;
        }
        if self.throttled_hierarchy(gcfs) {
            return;
        }
        let shares = self.calc_group_shares(gcfs);
        if self.e(se).load.weight != shares {
            let cfs = self.cfs_rq_of(se);
            self.reweight_entity(cfs, se, shares);
        }
    }

    // ---------------------------------------------------------------------------------------------
    // PELT
    // ---------------------------------------------------------------------------------------------

    /// `cfs_rq_clock_pelt`: o relógio do PELT da fila, parado enquanto ela está estrangulada.
    pub(crate) fn cfs_rq_clock_pelt(&self, cfs: CfsId) -> u64 {
        let c = self.c(cfs);
        if c.throttle_count > 0 {
            c.throttled_clock_pelt - c.throttled_clock_pelt_time
        } else {
            self.sched_clock() - c.throttled_clock_pelt_time
        }
    }

    /// `__update_load_avg_se`: carga da entidade, com entrada `on_rq` e peso `se_weight`.
    fn update_load_avg_se(&mut self, now: u64, se: EntityId) {
        let e = self.e_mut(se);
        let load = u64::from(e.on_rq);
        if update_load_sum(now, &mut e.avg, load) {
            let w = scale_load_down(e.load.weight);
            pelt_update_avg(&mut e.avg, w);
        }
    }

    /// `update_cfs_rq_load_avg` (sem `removed`): carga da fila, com entrada `load.weight`.
    fn update_cfs_rq_load_avg(&mut self, now: u64, cfs: CfsId) -> bool {
        let c = self.c_mut(cfs);
        let load = scale_load_down(c.load);
        if update_load_sum(now, &mut c.avg, load) {
            pelt_update_avg(&mut c.avg, 1);
            true
        } else {
            false
        }
    }

    /// `enqueue_load_avg`.
    fn enqueue_load_avg(&mut self, cfs: CfsId, se: EntityId) {
        let e = self.e(se);
        let (la, ls, w) = (e.avg.load_avg, e.avg.load_sum, scale_load_down(e.load.weight));
        let c = self.c_mut(cfs);
        c.avg.load_avg += la;
        c.avg.load_sum += w * ls;
    }

    /// `dequeue_load_avg`.
    fn dequeue_load_avg(&mut self, cfs: CfsId, se: EntityId) {
        let e = self.e(se);
        let (la, ls, w) = (e.avg.load_avg, e.avg.load_sum, scale_load_down(e.load.weight));
        let c = self.c_mut(cfs);
        c.avg.load_avg = c.avg.load_avg.saturating_sub(la);
        c.avg.load_sum = c.avg.load_sum.saturating_sub(w * ls);
        c.avg.load_sum = c.avg.load_sum.max(c.avg.load_avg * PELT_MIN_DIVIDER);
    }

    /// `attach_entity_load_avg`: alinha a janela da entidade com a da fila e soma a carga dela.
    fn attach_entity_load_avg(&mut self, cfs: CfsId, se: EntityId) {
        let divider = get_pelt_divider(&self.c(cfs).avg);
        let (lut, pc) = (self.c(cfs).avg.last_update_time, self.c(cfs).avg.period_contrib);
        let e = self.e_mut(se);
        e.avg.last_update_time = lut;
        e.avg.period_contrib = pc;
        let w = scale_load_down(e.load.weight);
        e.avg.load_sum = e.avg.load_avg * divider;
        if w < e.avg.load_sum {
            e.avg.load_sum /= w;
        } else {
            e.avg.load_sum = 1;
        }
        self.enqueue_load_avg(cfs, se);
    }

    /// `detach_entity_load_avg`.
    fn detach_entity_load_avg(&mut self, cfs: CfsId, se: EntityId) {
        self.dequeue_load_avg(cfs, se);
    }

    /// `attach_entity_cfs_rq` (grupo novo): atualiza e anexa a entidade de grupo na fila do pai.
    pub(crate) fn attach_entity_cfs_rq(&mut self, se: EntityId) {
        let cfs = self.cfs_rq_of(se);
        self.update_load_avg(cfs, se, 0);
        self.attach_entity_load_avg(cfs, se);
        self.update_tg_load_avg(cfs);
    }

    /// `update_load_avg`: atualiza a carga da entidade e da fila; anexa na chegada de migração
    /// (`DO_ATTACH` com `last_update_time == 0`), destaca na saída (`DO_DETACH`) e repassa a mudança pro
    /// `tg->load_avg` quando pedido.
    pub(crate) fn update_load_avg(&mut self, cfs: CfsId, se: EntityId, flags: u32) {
        let now = self.cfs_rq_clock_pelt(cfs);
        if self.e(se).avg.last_update_time != 0 && flags & SKIP_AGE_LOAD == 0 {
            self.update_load_avg_se(now, se);
        }
        let decayed = self.update_cfs_rq_load_avg(now, cfs);
        if self.e(se).avg.last_update_time == 0 && flags & DO_ATTACH != 0 {
            self.attach_entity_load_avg(cfs, se);
            self.update_tg_load_avg(cfs);
        } else if flags & DO_DETACH != 0 {
            self.detach_entity_load_avg(cfs, se);
            self.update_tg_load_avg(cfs);
        } else if decayed && flags & UPDATE_TG != 0 {
            self.update_tg_load_avg(cfs);
        }
    }

    /// `update_tg_load_avg`: repassa a carga da fila pro `tg->load_avg` por diferença, no máximo uma
    /// vez por milissegundo e só se mudou mais de 1/64.
    pub(crate) fn update_tg_load_avg(&mut self, cfs: CfsId) {
        let tg = self.c(cfs).tg;
        if tg == GroupId::ROOT {
            return;
        }
        let now = self.sched_clock();
        if now - self.c(cfs).last_update_tg_load_avg < 1_000_000 {
            return;
        }
        let c = self.c(cfs);
        let delta = c.avg.load_avg as i64 - c.tg_load_avg_contrib as i64;
        if delta.unsigned_abs() > c.tg_load_avg_contrib / 64 {
            let la = c.avg.load_avg;
            self.g_mut(tg).load_avg += delta;
            let c = self.c_mut(cfs);
            c.tg_load_avg_contrib = la;
            c.last_update_tg_load_avg = now;
        }
    }

    /// Grupos de baixo pra cima (filhos antes dos pais), a ordem em que o kernel percorre a
    /// `leaf_cfs_rq_list`.
    fn groups_bottom_up(&self) -> Vec<GroupId> {
        let mut out = Vec::new();
        let mut stack = vec![(GroupId::ROOT, false)];
        while let Some((g, children_done)) = stack.pop() {
            if children_done {
                out.push(g);
            } else {
                stack.push((g, true));
                stack.extend(self.g(g).children.iter().map(|&c| (c, false)));
            }
        }
        out
    }

    /// `sched_balance_update_blocked_averages` (a parte do CFS, `__update_blocked_fair`): decai a
    /// carga das filas da CPU que ficaram sem entidade na fila (a "carga bloqueada"), de baixo pra cima,
    /// e repassa a mudança pro `tg->load_avg`. Sem isso, a carga de uma fila que esvaziou fica congelada
    /// e infla o `tg_weight` do `calc_group_shares` nas outras CPUs.
    ///
    /// Como na `leaf_cfs_rq_list`, ficam de fora as filas estranguladas (`tg_throttle_down` as tira da
    /// lista) e as já decaídas sem filha na lista (`cfs_rq_is_decayed`). Devolve se a CPU ainda tem
    /// carga bloqueada (`!done`, que mantém `rq->has_blocked_load`).
    pub(crate) fn update_blocked_averages(&mut self, cpu: usize) -> bool {
        let slot = self.group_slot(cpu);
        let mut on_list = vec![false; self.groups.len()];
        let mut has_blocked = false;
        for g in self.groups_bottom_up() {
            let cfs = self.g(g).cfs_rq[slot];
            let child_on_list = self.g(g).children.iter().any(|c| on_list[c.index() as usize]);
            let c = self.c(cfs);
            if c.throttle_count > 0 || (c.load == 0 && c.avg.load_sum == 0 && !child_on_list) {
                continue;
            }
            let now = self.cfs_rq_clock_pelt(cfs);
            if self.update_cfs_rq_load_avg(now, cfs) {
                self.update_tg_load_avg(cfs);
            }
            // skip_blocked_update: entidade de grupo com carga zero não tem o que decair.
            if let Some(se) = self.c(cfs).my_se
                && self.e(se).avg.load_avg != 0
            {
                let parent = self.cfs_rq_of(se);
                self.update_load_avg(parent, se, UPDATE_TG);
            }
            let c = self.c(cfs);
            on_list[g.index() as usize] = !(c.load == 0 && c.avg.load_sum == 0 && !child_on_list);
            has_blocked |= c.avg.load_avg != 0;
        }
        let jiffy = 1_000_000_000 / self.cfg.balance.hz.max(1);
        let b = &mut self.cpus[cpu].balance;
        b.last_blocked_update_jiffy = self.now / jiffy;
        if !has_blocked {
            b.has_blocked_load = false;
        }
        has_blocked
    }

    /// `task_h_load`: a parte da carga da raiz da CPU que cabe à tarefa, descendo pela hierarquia
    /// (`update_cfs_rq_h_load`).
    pub(crate) fn task_h_load(&self, p: TaskId) -> u64 {
        let se = p.entity();
        let cfs = self.cfs_rq_of(se);
        let h = self.cfs_rq_h_load(cfs);
        self.e(se).avg.load_avg * h / (self.c(cfs).avg.load_avg + 1)
    }

    /// `cfs_rq->h_load`, calculado de cima pra baixo a partir da raiz da CPU.
    fn cfs_rq_h_load(&self, cfs: CfsId) -> u64 {
        match self.c(cfs).my_se {
            None => self.c(cfs).avg.load_avg,
            Some(se) => {
                let parent_cfs = self.cfs_rq_of(se);
                let ph = self.cfs_rq_h_load(parent_cfs);
                ph * self.e(se).avg.load_avg / (self.c(parent_cfs).avg.load_avg + 1)
            }
        }
    }
}
