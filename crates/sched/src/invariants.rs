//! Inspeção e verificação de coerência do [`Sched`]: o que testes, simulador e benchmarks usam pra
//! olhar dentro das filas sem mexer no estado.

use crate::clock::Clock;
use crate::sched::{CfsId, Kind, Sched};
use crate::timeline::{CurrView, Timeline};
use crate::weight::scale_load_down;
use crate::{EntityId, GroupId, TaskId};

impl<C: Clock> Sched<C> {
    /// A linha do tempo da fila raiz da CPU.
    pub fn root_timeline(&self, cpu: usize) -> &Timeline {
        &self.c(self.cpus[cpu].root).timeline
    }

    /// `rq->cfs.nr_running` da CPU.
    pub fn root_nr_running(&self, cpu: usize) -> u32 {
        self.c(self.cpus[cpu].root).nr_running
    }

    /// `rq->cfs.h_nr_delayed` da CPU.
    pub fn root_nr_delayed(&self, cpu: usize) -> u32 {
        self.c(self.cpus[cpu].root).h_nr_delayed
    }

    /// `rq->cfs.load.weight` da CPU.
    pub fn root_load_weight(&self, cpu: usize) -> u64 {
        self.c(self.cpus[cpu].root).load
    }

    /// `rq->cfs.avg.load_avg` da CPU (a `cpu_load` do balanceamento).
    pub fn root_load_avg(&self, cpu: usize) -> u64 {
        self.c(self.cpus[cpu].root).avg.load_avg
    }

    /// Visão do corrente da CPU na fila raiz.
    pub fn curr_view(&self, cpu: usize) -> Option<CurrView> {
        let root = self.cpus[cpu].root;
        let se = self.curr_of(root, cpu)?;
        let e = self.e(se);
        if !e.on_rq {
            return None;
        }
        Some(CurrView {
            entity: se,
            vruntime: e.vruntime,
            deadline: e.deadline,
            weight: e.load.weight,
            slice: e.slice,
            protected: e.vlag as u64 == e.deadline,
        })
    }

    /// V da fila raiz da CPU, sem mexer no estado.
    pub fn avg_vruntime_peek(&self, cpu: usize) -> u64 {
        self.avg_vruntime_peek_cfs(self.cpus[cpu].root)
    }

    /// O que o `pick_eevdf` escolheria agora na fila raiz da CPU.
    pub fn peek_pick(&self, cpu: usize) -> Option<EntityId> {
        self.pick_eevdf(self.cpus[cpu].root, cpu)
    }

    /// A mesma escolha por força bruta.
    pub fn peek_pick_linear(&self, cpu: usize) -> Option<EntityId> {
        self.pick_linear(self.cpus[cpu].root, cpu)
    }

    /// Entidades na fila raiz da CPU (árvore e quem roda), com `(entidade, vruntime, peso escalado)`.
    pub fn queued_entities(&self, cpu: usize) -> Vec<(EntityId, u64, u64)> {
        self.queued_in(self.cpus[cpu].root)
    }

    fn queued_in(&self, cfs: CfsId) -> Vec<(EntityId, u64, u64)> {
        let c = self.c(cfs);
        let mut out: Vec<(EntityId, u64, u64)> =
            c.timeline.tree().iter().map(|(_, _, e)| (e.entity, e.vruntime, e.weight)).collect();
        for v in self.running(cfs).as_slice() {
            out.push((v.entity, v.vruntime, v.weight));
        }
        out
    }

    /// Σ w_i (V - v_i) e Σ w_i de cada fila com entidades, com `w_i = scale_load_down(peso)`. Como
    /// `V = v0 + piso(Σ w (v - v0) / W)`, cada soma fica em (-W, 0].
    pub fn weighted_lag_sums(&self) -> Vec<(i128, i128)> {
        let mut out = Vec::new();
        for i in 0..self.cfs.len() {
            let cfs = CfsId(i as u32);
            let ents = self.queued_in(cfs);
            if ents.is_empty() {
                continue;
            }
            let v = self.avg_vruntime_peek_cfs(cfs);
            let mut sum = 0i128;
            let mut total = 0i128;
            for (_, vr, w) in ents {
                let w = i128::from(scale_load_down(w));
                sum += w * i128::from(v.wrapping_sub(vr) as i64);
                total += w;
            }
            out.push((sum, total));
        }
        out
    }

    /// Pra cada fila com entidades, o maior lag em tempo real `w_i * (V - v_i) / 1024`, em módulo.
    pub fn max_abs_real_lag(&self) -> i128 {
        let mut worst = 0i128;
        for i in 0..self.cfs.len() {
            let cfs = CfsId(i as u32);
            let ents = self.queued_in(cfs);
            if ents.is_empty() {
                continue;
            }
            let v = self.avg_vruntime_peek_cfs(cfs);
            for (_, vr, w) in ents {
                let lag = i128::from(scale_load_down(w)) * i128::from(v.wrapping_sub(vr) as i64) / 1024;
                worst = worst.max(lag.abs());
            }
        }
        worst
    }

    /// Confere a coerência interna: árvores e somas de cada fila; cópias da árvore iguais às entidades;
    /// `nr_running`, `load`, `h_nr_queued`, `h_nr_runnable`, `h_nr_delayed` e `rq->nr_running` iguais aos
    /// recontados (respeitando filas estranguladas); caminho de quem roda em cada CPU; `tg->load_avg`
    /// igual à soma das contribuições; contagem de estrangulamento igual à dos ancestrais; e o estado
    /// de cada tarefa.
    pub fn check_invariants(&self) -> Result<(), String> {
        for (i, c) in self.cfs.iter().enumerate() {
            let cfs = CfsId(i as u32);
            c.timeline.check_invariants().map_err(|e| format!("fila {i}: {e}"))?;
            let mut nr = 0u32;
            let mut load = 0u64;
            for (j, slot) in self.ents.iter().enumerate() {
                let Some(e) = slot else { continue };
                if e.cfs_rq != cfs {
                    continue;
                }
                let id = EntityId::from_index(j as u32);
                if e.on_rq {
                    nr += 1;
                    load += e.load.weight;
                }
                let running = self.is_running(cfs, id);
                let in_tree = e.on_rq && !running;
                match (e.node, in_tree) {
                    (Some(n), true) => {
                        let q = c.timeline.entity(n);
                        if q.entity != id || q.vruntime != e.vruntime || q.slice != e.slice || q.weight != e.load.weight {
                            return Err(format!("{id:?}: cópia na árvore diverge da entidade"));
                        }
                        if c.timeline.deadline(n) != e.deadline {
                            return Err(format!("{id:?}: deadline na árvore diverge"));
                        }
                    }
                    (None, false) => {}
                    (Some(_), false) => return Err(format!("{id:?} está na árvore sem dever")),
                    (None, true) => return Err(format!("{id:?} deveria estar na árvore")),
                }
            }
            if nr != c.nr_running {
                return Err(format!("fila {i}: nr_running {} mas recontei {nr}", c.nr_running));
            }
            if load != c.load {
                return Err(format!("fila {i}: load {} mas recontei {load}", c.load));
            }
            let (q, d) = self.count_hierarchical(cfs);
            if q != c.h_nr_queued || d != c.h_nr_delayed {
                return Err(format!(
                    "fila {i}: h_nr_queued/h_nr_delayed {}/{} mas recontei {q}/{d}",
                    c.h_nr_queued, c.h_nr_delayed
                ));
            }
            if c.h_nr_runnable + c.h_nr_delayed != c.h_nr_queued {
                return Err(format!("fila {i}: h_nr_runnable + h_nr_delayed != h_nr_queued"));
            }
            for s in &c.curr {
                if !self.e(s.ent).on_rq {
                    return Err(format!("fila {i}: {:?} roda sem estar na fila", s.ent));
                }
            }
            // Contagem de estrangulamento: quantas filas estranguladas há nela e nos ancestrais.
            let mut count = 0;
            let mut q = Some(cfs);
            while let Some(x) = q {
                if self.c(x).throttled {
                    count += 1;
                }
                q = self.c(x).my_se.map(|se| self.cfs_rq_of(se));
            }
            if count != c.throttle_count {
                return Err(format!("fila {i}: throttle_count {} mas há {count} estranguladas no caminho", c.throttle_count));
            }
            if c.throttled {
                if let Some(se) = c.my_se
                    && self.e(se).on_rq
                {
                    return Err(format!("fila {i} estrangulada com a entidade do grupo na fila"));
                }
                if !self.g(c.tg).bw.throttled.contains(&cfs) {
                    return Err(format!("fila {i} estrangulada fora da lista do grupo"));
                }
            }
        }

        // Caminho de quem roda.
        for cpu in 0..self.cfg.cpus {
            let mut expected: Vec<(CfsId, EntityId)> = Vec::new();
            if let Some(t) = self.cpus[cpu].curr {
                let mut se = Some(t.entity());
                while let Some(s) = se {
                    expected.push((self.cfs_rq_of(s), s));
                    se = self.parent_entity(s);
                }
            }
            let mut found: Vec<(CfsId, EntityId)> = Vec::new();
            for (i, c) in self.cfs.iter().enumerate() {
                for s in &c.curr {
                    if s.cpu == cpu {
                        found.push((CfsId(i as u32), s.ent));
                    }
                }
            }
            expected.sort();
            found.sort();
            if expected != found {
                return Err(format!("CPU {cpu}: caminho de quem roda {expected:?} diferente dos correntes {found:?}"));
            }
        }

        // rq->nr_running.
        if self.shared() {
            let root = self.cpus[0].root;
            if self.cpus[0].nr_running != self.c(root).h_nr_queued {
                return Err(format!("rq->nr_running {} != h_nr_queued da raiz {}", self.cpus[0].nr_running, self.c(root).h_nr_queued));
            }
        } else {
            for cpu in 0..self.cfg.cpus {
                let root = self.cpus[cpu].root;
                if self.cpus[cpu].nr_running != self.c(root).h_nr_queued {
                    return Err(format!(
                        "CPU {cpu}: rq->nr_running {} != h_nr_queued da raiz {}",
                        self.cpus[cpu].nr_running,
                        self.c(root).h_nr_queued
                    ));
                }
            }
        }

        // tg->load_avg.
        for g in self.group_ids().collect::<Vec<_>>() {
            if g == GroupId::ROOT {
                continue;
            }
            let tg = self.g(g);
            let sum: i64 = tg.cfs_rq.iter().map(|&c| self.c(c).tg_load_avg_contrib as i64).sum();
            if sum != tg.load_avg {
                return Err(format!("{g:?}: load_avg {} mas a soma das contribuições é {sum}", tg.load_avg));
            }
        }

        // Estado de cada tarefa.
        for t in self.task_ids().collect::<Vec<TaskId>>() {
            let e = self.e(t.entity());
            let Kind::Task(d) = &e.kind else { continue };
            if e.sched_delayed && !(e.on_rq && d.sleeping) {
                return Err(format!("{t:?} atrasada mas fora da fila ou acordada"));
            }
            if e.on_rq != d.queued {
                return Err(format!("{t:?}: se->on_rq {} e p->on_rq {} divergem", e.on_rq, d.queued));
            }
            if d.sleeping && d.queued && !e.sched_delayed {
                return Err(format!("{t:?} dorme mas está na fila sem atraso"));
            }
            if !d.sleeping && !d.new && !d.queued {
                return Err(format!("{t:?} está pronta mas fora da fila"));
            }
            if !self.shared() && e.cfs_rq != self.g(d.group).cfs_rq[d.cpu] {
                return Err(format!("{t:?} na fila errada pra CPU {}", d.cpu));
            }
        }
        Ok(())
    }

    /// Recontagem de `h_nr_queued` e `h_nr_delayed`: tarefas na fila diretamente mais o que vem de cada
    /// entidade de grupo na fila (fila estrangulada não sobe).
    fn count_hierarchical(&self, cfs: CfsId) -> (u32, u32) {
        let mut q = 0;
        let mut d = 0;
        for slot in self.ents.iter().flatten() {
            if slot.cfs_rq != cfs || !slot.on_rq {
                continue;
            }
            match &slot.kind {
                Kind::Task(_) => {
                    q += 1;
                    if slot.sched_delayed {
                        d += 1;
                    }
                }
                Kind::Group { my_q, .. } => {
                    if !self.c(*my_q).throttled {
                        let (cq, cd) = (self.c(*my_q).h_nr_queued, self.c(*my_q).h_nr_delayed);
                        q += cq;
                        d += cd;
                    }
                }
            }
        }
        (q, d)
    }
}
