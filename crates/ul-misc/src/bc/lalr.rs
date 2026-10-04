//! Gerador de tabelas LALR(1) e analisador com o comportamento observável de um parser do bison
//! (esqueleto `yacc.c`, 3.x). Código nosso, MIT, escrito a partir da descrição do algoritmo (livro do
//! dragão, seção 4.7) e do comportamento documentado do bison.
//!
//! O bc do GNU é gerado pelo bison, e várias coisas que aparecem na saída dependem das tabelas e não
//! só da linguagem: em que linha sai uma mensagem emitida numa ação (depende de o estado precisar ou
//! não ler o próximo token), quantos erros de sintaxe são reportados (o bison cala os erros até três
//! tokens serem deslocados depois de um erro) e onde a recuperação retoma (o token `error`). Por isso
//! a gramática roda sobre um autômato construído com as mesmas regras do bison:
//!
//! - conflito deslocar/reduzir resolvido por precedência quando o token e a regra têm precedência (a
//!   da regra é a do último terminal dela, ou a do `%prec`); senão desloca. `%nonassoc` vira erro
//!   explícito. Reduzir/reduzir fica com a regra de menor número;
//! - redução padrão de cada estado: a regra que aparece em mais entradas (empate: menor número),
//!   exceto em estado que desloca `error`; as entradas iguais à padrão saem da tabela, e um estado sem
//!   nenhuma entrada explícita reduz sem ler o próximo token;
//! - recuperação: mensagem só com `errstatus == 0`; com `errstatus == 3` o token da vez é descartado
//!   (no EOF a análise aborta); desempilha até um estado que desloque `error`, desloca e põe
//!   `errstatus = 3`; cada token deslocado depois decrementa.

use std::collections::{BTreeMap, BTreeSet, HashMap};

/// Associatividade de um nível de precedência.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Assoc {
    Left,
    Right,
    NonAssoc,
}

/// Uma regra: `lhs -> rhs`, com `%prec` opcional (um terminal).
#[derive(Clone, Debug)]
pub struct Rule {
    pub lhs: u16,
    pub rhs: Vec<u16>,
    pub prec: Option<u16>,
}

/// Uma gramática. Os símbolos `0..n_terms` são terminais; os demais, não terminais. A regra 0 tem de
/// ser `$accept -> start $end`.
#[derive(Clone, Debug)]
pub struct Grammar {
    pub n_terms: usize,
    pub n_symbols: usize,
    pub eof: u16,
    pub error: u16,
    pub rules: Vec<Rule>,
    /// Precedência de cada terminal: (nível, associatividade); nível 0 = sem precedência.
    pub term_prec: Vec<(u8, Assoc)>,
}

/// Ação de uma entrada da tabela.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Shift(u32),
    Reduce(u32),
    /// Erro explícito (`%nonassoc`).
    Error,
    Accept,
}

/// As tabelas prontas.
#[derive(Debug)]
pub struct Tables {
    /// Ações explícitas por estado (terminal -> ação), sem as iguais à redução padrão.
    pub actions: Vec<HashMap<u16, Action>>,
    /// Redução padrão por estado.
    pub default: Vec<Option<u32>>,
    /// Desvio por estado e não terminal.
    pub gotos: Vec<HashMap<u16, u32>>,
    /// Estado sem entrada explícita: reduz sem ler o próximo token.
    pub default_only: Vec<bool>,
    pub rule_lhs: Vec<u16>,
    pub rule_len: Vec<usize>,
    /// Conflitos deslocar/reduzir resolvidos por "desloca" (sem precedência), pra diagnóstico.
    pub sr_conflicts: usize,
    pub rr_conflicts: usize,
}

type Item = (u32, u32); // (regra, posição do ponto)

impl Grammar {
    fn is_term(&self, s: u16) -> bool {
        (s as usize) < self.n_terms
    }

    fn rule_prec(&self, r: usize) -> (u8, Assoc) {
        let rule = &self.rules[r];
        if let Some(p) = rule.prec {
            return self.term_prec[p as usize];
        }
        // A precedência padrão é a do último terminal do corpo (mesmo que ele não tenha nenhuma).
        match rule.rhs.iter().rev().find(|&&s| self.is_term(s)) {
            Some(&t) => self.term_prec[t as usize],
            None => (0, Assoc::Left),
        }
    }

    fn nullable_and_first(&self) -> (Vec<bool>, Vec<BTreeSet<u16>>) {
        let n = self.n_symbols;
        let mut nullable = vec![false; n];
        let mut first: Vec<BTreeSet<u16>> = vec![BTreeSet::new(); n];
        for (t, f) in first.iter_mut().enumerate().take(self.n_terms) {
            f.insert(t as u16);
        }
        let mut changed = true;
        while changed {
            changed = false;
            for r in &self.rules {
                let lhs = r.lhs as usize;
                let mut all_nullable = true;
                for &s in &r.rhs {
                    let add: Vec<u16> = first[s as usize].iter().copied().collect();
                    for a in add {
                        if first[lhs].insert(a) {
                            changed = true;
                        }
                    }
                    if !nullable[s as usize] {
                        all_nullable = false;
                        break;
                    }
                }
                if all_nullable && !nullable[lhs] {
                    nullable[lhs] = true;
                    changed = true;
                }
            }
        }
        (nullable, first)
    }

    /// Fecho LR(0) de um conjunto de itens.
    fn closure0(&self, kernel: &[Item], by_lhs: &HashMap<u16, Vec<u32>>) -> Vec<Item> {
        let mut set: BTreeSet<Item> = kernel.iter().copied().collect();
        let mut work: Vec<Item> = kernel.to_vec();
        while let Some((r, d)) = work.pop() {
            let rule = &self.rules[r as usize];
            if let Some(&s) = rule.rhs.get(d as usize)
                && !self.is_term(s)
            {
                for &r2 in by_lhs.get(&s).map(Vec::as_slice).unwrap_or(&[]) {
                    if set.insert((r2, 0)) {
                        work.push((r2, 0));
                    }
                }
            }
        }
        set.into_iter().collect()
    }

    /// Constrói as tabelas.
    pub fn build(&self) -> Tables {
        let mut by_lhs: HashMap<u16, Vec<u32>> = HashMap::new();
        for (i, r) in self.rules.iter().enumerate() {
            by_lhs.entry(r.lhs).or_default().push(i as u32);
        }
        let (nullable, first) = self.nullable_and_first();

        // Estados LR(0) pelos kernels.
        let mut kernels: Vec<Vec<Item>> = vec![vec![(0, 0)]];
        let mut index: HashMap<Vec<Item>, u32> = HashMap::new();
        index.insert(vec![(0, 0)], 0);
        let mut trans: Vec<BTreeMap<u16, u32>> = Vec::new();
        let mut i = 0;
        while i < kernels.len() {
            let items = self.closure0(&kernels[i], &by_lhs);
            let mut next: BTreeMap<u16, Vec<Item>> = BTreeMap::new();
            for &(r, d) in &items {
                let rule = &self.rules[r as usize];
                if let Some(&s) = rule.rhs.get(d as usize) {
                    next.entry(s).or_default().push((r, d + 1));
                }
            }
            let mut t = BTreeMap::new();
            for (s, mut k) in next {
                k.sort();
                k.dedup();
                let id = match index.get(&k) {
                    Some(&id) => id,
                    None => {
                        let id = kernels.len() as u32;
                        index.insert(k.clone(), id);
                        kernels.push(k);
                        id
                    }
                };
                t.insert(s, id);
            }
            trans.push(t);
            i += 1;
        }
        let n_states = kernels.len();

        // Lookaheads LALR(1) por propagação (livro do dragão, algoritmo 4.62/4.63). `DUMMY` é o
        // símbolo '#' do livro.
        let dummy = self.n_symbols as u16;
        let mut la: Vec<HashMap<Item, BTreeSet<u16>>> = kernels
            .iter()
            .map(|k| k.iter().map(|&it| (it, BTreeSet::new())).collect())
            .collect();
        la[0].get_mut(&(0, 0)).expect("item inicial").insert(self.eof);
        let mut prop: Vec<((usize, Item), (usize, Item))> = Vec::new();
        for (st, kernel) in kernels.iter().enumerate() {
            for &kit in kernel {
                // Fecho LR(1) de [kit, #].
                let cl = self.closure1(&[(kit, dummy)], &by_lhs, &nullable, &first);
                for ((r, d), a) in cl {
                    let rule = &self.rules[r as usize];
                    let Some(&s) = rule.rhs.get(d as usize) else { continue };
                    let target = trans[st][&s] as usize;
                    let titem = (r, d + 1);
                    if a == dummy {
                        prop.push(((st, kit), (target, titem)));
                    } else {
                        la[target].get_mut(&titem).expect("item do kernel").insert(a);
                    }
                }
            }
        }
        let mut changed = true;
        while changed {
            changed = false;
            for ((s1, i1), (s2, i2)) in &prop {
                let from: Vec<u16> = la[*s1][i1].iter().copied().collect();
                let to = la[*s2].get_mut(i2).expect("item");
                for a in from {
                    if to.insert(a) {
                        changed = true;
                    }
                }
            }
        }

        let rule_lhs: Vec<u16> = self.rules.iter().map(|r| r.lhs).collect();
        let rule_len: Vec<usize> = self.rules.iter().map(|r| r.rhs.len()).collect();
        let mut actions = Vec::with_capacity(n_states);
        let mut default = Vec::with_capacity(n_states);
        let mut gotos = Vec::with_capacity(n_states);
        let mut default_only = Vec::with_capacity(n_states);
        let mut sr_conflicts = 0;
        let mut rr_conflicts = 0;

        for st in 0..n_states {
            // Itens completos com os lookaheads (os do fecho vêm de propagação interna).
            let full = self.closure1(
                &kernels[st].iter().flat_map(|it| la[st][it].iter().map(move |&a| (*it, a))).collect::<Vec<_>>(),
                &by_lhs,
                &nullable,
                &first,
            );
            let mut reductions: BTreeMap<u32, BTreeSet<u16>> = BTreeMap::new();
            for ((r, d), a) in &full {
                if *d as usize == self.rules[*r as usize].rhs.len() && *r != 0 {
                    reductions.entry(*r).or_default().insert(*a);
                }
            }
            // Também os completos sem lookahead (estado consistente usa a redução pra tudo).
            let lr0 = self.closure0(&kernels[st], &by_lhs);
            let mut complete_rules: Vec<u32> = lr0
                .iter()
                .filter(|(r, d)| *d as usize == self.rules[*r as usize].rhs.len() && *r != 0)
                .map(|(r, _)| *r)
                .collect();
            complete_rules.sort();
            complete_rules.dedup();
            let has_term_shift = trans[st].keys().any(|&s| self.is_term(s));
            let consistent = complete_rules.len() <= 1 && !(complete_rules.len() == 1 && has_term_shift);

            let mut row: BTreeMap<u16, Action> = BTreeMap::new();
            let mut go = HashMap::new();
            for (&s, &t) in &trans[st] {
                if self.is_term(s) {
                    if s == self.eof && lr0.iter().any(|&(r, d)| r == 0 && d == 1) {
                        row.insert(s, Action::Accept);
                    } else {
                        row.insert(s, Action::Shift(t));
                    }
                } else {
                    go.insert(s, t);
                }
            }
            let mut def: Option<u32> = None;
            if consistent {
                if let Some(&r) = complete_rules.first() {
                    def = Some(r);
                }
            } else {
                for (&r, las) in &reductions {
                    for &a in las {
                        match row.get(&a).copied() {
                            None => {
                                row.insert(a, Action::Reduce(r));
                            }
                            Some(Action::Reduce(r0)) => {
                                rr_conflicts += 1;
                                if r < r0 {
                                    row.insert(a, Action::Reduce(r));
                                }
                            }
                            Some(Action::Shift(_)) | Some(Action::Accept) => {
                                let (tp, _) = self.term_prec[a as usize];
                                let (rp, ra) = self.rule_prec(r as usize);
                                if tp == 0 || rp == 0 {
                                    sr_conflicts += 1;
                                } else if tp < rp {
                                    row.insert(a, Action::Reduce(r));
                                } else if tp == rp {
                                    match ra {
                                        Assoc::Left => {
                                            row.insert(a, Action::Reduce(r));
                                        }
                                        Assoc::Right => {}
                                        Assoc::NonAssoc => {
                                            row.insert(a, Action::Error);
                                        }
                                    }
                                }
                            }
                            Some(Action::Error) => {}
                        }
                    }
                }
                let shifts_error = matches!(row.get(&self.error), Some(Action::Shift(_)));
                if !shifts_error {
                    let mut best: Option<(u32, usize)> = None;
                    for &r in reductions.keys() {
                        let count = row.values().filter(|a| **a == Action::Reduce(r)).count();
                        if count > 0 && best.is_none_or(|(_, c)| count > c) {
                            best = Some((r, count));
                        }
                    }
                    if let Some((r, _)) = best {
                        def = Some(r);
                        row.retain(|_, a| *a != Action::Reduce(r));
                    }
                }
                if def.is_none() {
                    // Sem redução padrão o padrão é erro: erro explícito vira "use o padrão".
                    row.retain(|_, a| *a != Action::Error);
                }
            }
            default_only.push(row.is_empty() && def.is_some());
            actions.push(row.into_iter().collect());
            default.push(def);
            gotos.push(go);
        }
        Tables { actions, default, gotos, default_only, rule_lhs, rule_len, sr_conflicts, rr_conflicts }
    }

    /// Fecho LR(1).
    fn closure1(
        &self,
        start: &[(Item, u16)],
        by_lhs: &HashMap<u16, Vec<u32>>,
        nullable: &[bool],
        first: &[BTreeSet<u16>],
    ) -> BTreeSet<(Item, u16)> {
        let mut set: BTreeSet<(Item, u16)> = start.iter().copied().collect();
        let mut work: Vec<(Item, u16)> = start.to_vec();
        while let Some(((r, d), a)) = work.pop() {
            let rule = &self.rules[r as usize];
            let Some(&b) = rule.rhs.get(d as usize) else { continue };
            if self.is_term(b) {
                continue;
            }
            // FIRST(beta a)
            let mut las: BTreeSet<u16> = BTreeSet::new();
            let mut all_null = true;
            for &s in &rule.rhs[d as usize + 1..] {
                las.extend(first[s as usize].iter().copied());
                if !nullable[s as usize] {
                    all_null = false;
                    break;
                }
            }
            if all_null {
                las.insert(a);
            }
            for &r2 in by_lhs.get(&b).map(Vec::as_slice).unwrap_or(&[]) {
                for &l in &las {
                    if set.insert(((r2, 0), l)) {
                        work.push(((r2, 0), l));
                    }
                }
            }
        }
        set
    }
}

/// O que o analisador pede a quem o usa.
pub trait Handler {
    type Value: Clone + Default;
    /// Próximo token (símbolo terminal e valor).
    fn lex(&mut self) -> (u16, Self::Value);
    /// Ação da regra `rule`. `stack` é a pilha de valores inteira; os `len` do topo são o corpo da
    /// regra (ações no meio da regra leem valores mais abaixo).
    fn reduce(&mut self, rule: u32, stack: &[Self::Value], len: usize) -> Self::Value;
    /// O `yyerror("syntax error")`.
    fn syntax_error(&mut self);
    /// `yyerrok` pedido pela última ação.
    fn take_errok(&mut self) -> bool {
        false
    }
    /// A análise tem de parar já (o `quit`, o `halt` e os erros fatais saem do processo no meio de
    /// uma ação ou da leitura).
    fn aborted(&self) -> bool {
        false
    }
}

/// Como a análise terminou.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Accept,
    Abort,
}

/// Roda o analisador do jeito do esqueleto `yacc.c`.
pub fn parse<H: Handler>(t: &Tables, h: &mut H, eof: u16, error: u16) -> Outcome {
    let mut states: Vec<u32> = vec![0];
    let mut values: Vec<H::Value> = vec![H::Value::default()];
    let mut lookahead: Option<(u16, H::Value)> = None;
    let mut errstatus: u8 = 0;
    loop {
        if h.aborted() {
            return Outcome::Abort;
        }
        let st = *states.last().expect("pilha") as usize;
        // yybackup
        let act = if t.default_only[st] {
            None
        } else {
            if lookahead.is_none() {
                lookahead = Some(h.lex());
            }
            let tok = lookahead.as_ref().map(|l| l.0).expect("lookahead");
            t.actions[st].get(&tok).copied()
        };
        let act = match act {
            Some(a) => a,
            None => match t.default[st] {
                Some(r) => Action::Reduce(r),
                None => Action::Error,
            },
        };
        match act {
            Action::Accept => return Outcome::Accept,
            Action::Shift(ns) => {
                errstatus = errstatus.saturating_sub(1);
                let (_, v) = lookahead.take().expect("lookahead");
                states.push(ns);
                values.push(v);
            }
            Action::Reduce(r) => {
                let len = t.rule_len[r as usize];
                let v = h.reduce(r, &values, len);
                if h.take_errok() {
                    errstatus = 0;
                }
                for _ in 0..len {
                    states.pop();
                    values.pop();
                }
                let top = *states.last().expect("pilha") as usize;
                let lhs = t.rule_lhs[r as usize];
                let ns = t.gotos[top][&lhs];
                states.push(ns);
                values.push(v);
            }
            Action::Error => {
                // yyerrlab
                if errstatus == 0 {
                    h.syntax_error();
                }
                if errstatus == 3 {
                    match lookahead.as_ref().map(|l| l.0) {
                        Some(tok) if tok == eof => return Outcome::Abort,
                        Some(_) => lookahead = None,
                        None => {}
                    }
                }
                // yyerrlab1
                errstatus = 3;
                loop {
                    let s = *states.last().expect("pilha") as usize;
                    if !t.default_only[s]
                        && let Some(Action::Shift(ns)) = t.actions[s].get(&error).copied()
                    {
                        states.push(ns);
                        values.push(H::Value::default());
                        break;
                    }
                    if states.len() == 1 {
                        return Outcome::Abort;
                    }
                    states.pop();
                    values.pop();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Gramática de expressões: E -> E + E | E * E | ( E ) | n, com + e * à esquerda.
    // Terminais: 0 $end, 1 error, 2 +, 3 *, 4 (, 5 ), 6 n. Não terminais: 7 $accept, 8 E.
    fn expr_grammar() -> Grammar {
        let r = |lhs: u16, rhs: &[u16]| Rule { lhs, rhs: rhs.to_vec(), prec: None };
        let mut term_prec = vec![(0u8, Assoc::Left); 7];
        term_prec[2] = (1, Assoc::Left);
        term_prec[3] = (2, Assoc::Left);
        Grammar {
            n_terms: 7,
            n_symbols: 9,
            eof: 0,
            error: 1,
            rules: vec![r(7, &[8, 0]), r(8, &[8, 2, 8]), r(8, &[8, 3, 8]), r(8, &[4, 8, 5]), r(8, &[6])],
            term_prec,
        }
    }

    struct Eval {
        toks: Vec<(u16, i64)>,
        pos: usize,
        errors: usize,
    }

    impl Handler for Eval {
        type Value = i64;
        fn lex(&mut self) -> (u16, i64) {
            let t = self.toks.get(self.pos).copied().unwrap_or((0, 0));
            self.pos += 1;
            t
        }
        fn reduce(&mut self, rule: u32, s: &[i64], len: usize) -> i64 {
            let v = &s[s.len() - len..];
            match rule {
                1 => v[0] + v[2],
                2 => v[0] * v[2],
                3 => v[1],
                4 => v[0],
                _ => 0,
            }
        }
        fn syntax_error(&mut self) {
            self.errors += 1;
        }
    }

    #[test]
    fn precedence_and_associativity() {
        let g = expr_grammar();
        let t = g.build();
        assert_eq!(t.sr_conflicts, 0);
        // 2 + 3 * 4
        let mut h = Eval { toks: vec![(6, 2), (2, 0), (6, 3), (3, 0), (6, 4)], pos: 0, errors: 0 };
        assert_eq!(parse(&t, &mut h, 0, 1), Outcome::Accept);
        assert_eq!(h.errors, 0);
    }

    #[test]
    fn syntax_error_aborts_without_error_rules() {
        let t = expr_grammar().build();
        let mut h = Eval { toks: vec![(6, 2), (2, 0), (2, 0)], pos: 0, errors: 0 };
        assert_eq!(parse(&t, &mut h, 0, 1), Outcome::Abort);
        assert_eq!(h.errors, 1);
    }
}
