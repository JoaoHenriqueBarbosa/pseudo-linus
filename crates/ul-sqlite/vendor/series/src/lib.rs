//! `generate_series` do SQLite 3.46.1 (`ext/misc/series.c`, domínio público) como tabela virtual do
//! rusqlite.
//!
//! O módulo `rusqlite::vtab::series` é um porte da versão antiga do series.c: com passo negativo ele
//! inverte a ordem de `start..stop` em vez de contar de `start` até `stop` (`generate_series(10,1,-4)`
//! devolve 10, 6, 2 no sqlite3 do Debian 13 e nada no módulo do rusqlite), não conhece LIMIT e
//! OFFSET empurrados para a tabela, não detecta o primeiro argumento ausente e converte os
//! argumentos como inteiros estritos. Este crate refaz o módulo com a lógica do 3.46.1 (a estrutura
//! da integração com o rusqlite vem do `vtab/series.rs` dele, licença MIT).
//!
//! A trait `VTab` do rusqlite é `unsafe`; por isso o módulo vive aqui, fora do `ul-sqlite`, que tem
//! `forbid(unsafe_code)`.

use std::borrow::Cow;
use std::ffi::{c_int, CStr};
use std::marker::PhantomData;

use rusqlite::types::ValueRef;
use rusqlite::vtab::{
    sqlite3_vtab, sqlite3_vtab_cursor, Context, Filters, IndexConstraintOp, IndexInfo, Module, VTab, VTabConfig,
    VTabConnection, VTabCursor,
};
use rusqlite::{Connection, Error, Result};

const MODULE_NAME: &CStr = c"generate_series";

/// Registra o módulo `generate_series` (eponímico) na conexão.
pub fn load_module(conn: &Connection) -> Result<()> {
    const MODULE: Module<SeriesTab> = Module::eponymous_only_module();
    let aux: Option<()> = None;
    conn.create_module(MODULE_NAME, &MODULE, aux)
}

// Números das colunas: value, start, stop e step (as três últimas ocultas).
const COLUMN_START: c_int = 1;

// Bits do plano de consulta (`idxNum`).
const PLAN_START: c_int = 0x01;
const PLAN_STOP: c_int = 0x02;
const PLAN_STEP: c_int = 0x04;
const PLAN_DESC: c_int = 0x08;
const PLAN_ASC: c_int = 0x10;
const PLAN_LIMIT: c_int = 0x20;
const PLAN_OFFSET: c_int = 0x40;

/// Membro de índice `ix` (base 0) da sequência que começa em `base` e anda `step` por índice. As
/// contas dão a volta como no C com aritmética de complemento de dois.
fn gen_seq_member(mut base: i64, step: i64, mut ix: u64) -> i64 {
    const MX_I64: u64 = i64::MAX as u64;
    if ix >= MX_I64 {
        // Leva `ix` para dentro da faixa de i64.
        ix -= MX_I64;
        base = base.wrapping_add(((MX_I64 / 2) as i64).wrapping_mul(step));
        base = base.wrapping_add(((MX_I64 - MX_I64 / 2) as i64).wrapping_mul(step));
    }
    if ix >= 2 {
        let ix2 = (ix as i64) / 2;
        base = base.wrapping_add(ix2.wrapping_mul(step));
        ix -= ix2 as u64;
    }
    base.wrapping_add((ix as i64).wrapping_mul(step))
}

/// Estado do gerador (`SequenceSpec` do C).
#[derive(Default)]
struct SequenceSpec {
    /// Valor inicial (`start`).
    base: i64,
    /// Valor terminal dado (`stop`).
    term: i64,
    /// Incremento (`step`).
    step: i64,
    /// Maior índice da sequência (o "n").
    seq_index_max: u64,
    /// Índice atual durante a geração.
    seq_index_now: u64,
    /// Valor atual durante a geração.
    value_now: i64,
    /// A sequência ainda não esgotou.
    is_not_eof: bool,
    /// A sequência está sendo gerada de trás para a frente.
    is_reversing: bool,
}

impl SequenceSpec {
    /// Prepara o gerador a partir de `base`, `term`, `step` e `is_reversing` já preenchidos.
    fn setup(&mut self) {
        self.seq_index_max = 0;
        self.is_not_eof = false;
        let same_signs = (self.base < 0) == (self.term < 0);
        if self.term < self.base {
            let nuspan: u64 = if same_signs {
                self.base.wrapping_sub(self.term) as u64
            } else {
                // Aqui base >= 0 e term < 0.
                1u64.wrapping_add(self.base as u64).wrapping_add((-(self.term + 1)) as u64)
            };
            if self.step < 0 {
                self.is_not_eof = true;
                if nuspan == u64::MAX {
                    self.seq_index_max = if self.step > i64::MIN { nuspan / ((-self.step) as u64) } else { 1 };
                } else if self.step > i64::MIN {
                    self.seq_index_max = nuspan / ((-self.step) as u64);
                }
            }
        } else if self.term > self.base {
            let puspan: u64 = if same_signs {
                self.term.wrapping_sub(self.base) as u64
            } else {
                // Aqui term >= 0 e base < 0.
                1u64.wrapping_add(self.term as u64).wrapping_add((-(self.base + 1)) as u64)
            };
            if self.step > 0 {
                self.is_not_eof = true;
                self.seq_index_max = puspan / (self.step as u64);
            }
        } else {
            self.is_not_eof = true;
            self.seq_index_max = 0;
        }
        self.seq_index_now = if self.is_reversing { self.seq_index_max } else { 0 };
        self.value_now =
            if self.is_reversing { gen_seq_member(self.base, self.step, self.seq_index_max) } else { self.base };
    }

    /// Avança para o próximo valor; deixa o gerador num valor válido ou no fim.
    fn progress(&mut self) {
        if !self.is_not_eof {
            return;
        }
        if self.is_reversing {
            if self.seq_index_now > 0 {
                self.seq_index_now -= 1;
                self.value_now = self.value_now.wrapping_sub(self.step);
            } else {
                self.is_not_eof = false;
            }
        } else if self.seq_index_now < self.seq_index_max {
            self.seq_index_now += 1;
            self.value_now = self.value_now.wrapping_add(self.step);
        } else {
            self.is_not_eof = false;
        }
    }
}

/// `sqlite3_value_int64` sobre um `ValueRef`: inteiro direto, real truncado com saturação, texto e
/// blob pelo prefixo numérico, NULL como 0.
fn value_int64(v: Option<&ValueRef<'_>>) -> i64 {
    match v {
        None | Some(ValueRef::Null) => 0,
        Some(ValueRef::Integer(i)) => *i,
        Some(ValueRef::Real(r)) => {
            if *r <= i64::MIN as f64 {
                i64::MIN
            } else if *r >= i64::MAX as f64 {
                i64::MAX
            } else {
                *r as i64
            }
        }
        Some(ValueRef::Text(t)) | Some(ValueRef::Blob(t)) => atoi64(t),
    }
}

/// Prefixo inteiro de um texto como o `sqlite3Atoi64`: espaços, sinal, dígitos; satura no overflow.
fn atoi64(text: &[u8]) -> i64 {
    let mut i = 0;
    while i < text.len() && matches!(text[i], b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r') {
        i += 1;
    }
    let mut neg = false;
    if i < text.len() && (text[i] == b'-' || text[i] == b'+') {
        neg = text[i] == b'-';
        i += 1;
    }
    let mut acc: u64 = 0;
    while i < text.len() && text[i].is_ascii_digit() {
        acc = match acc.checked_mul(10).and_then(|v| v.checked_add(u64::from(text[i] - b'0'))) {
            Some(v) => v,
            None => return if neg { i64::MIN } else { i64::MAX },
        };
        i += 1;
    }
    let v = if neg { -(acc as i128) } else { acc as i128 };
    v.clamp(i64::MIN as i128, i64::MAX as i128) as i64
}

/// Instância da tabela virtual `generate_series`.
#[repr(C)]
struct SeriesTab {
    /// Classe base: precisa ser o primeiro campo.
    base: sqlite3_vtab,
}

unsafe impl<'vtab> VTab<'vtab> for SeriesTab {
    type Aux = ();
    type Cursor = SeriesTabCursor<'vtab>;

    fn connect(
        db: &mut VTabConnection,
        aux: Option<&()>,
        module_name: &[u8],
        _database_name: &[u8],
        table_name: &[u8],
        _args: &[&[u8]],
    ) -> Result<(Cow<'static, CStr>, Self)> {
        debug_assert_eq!(aux, None);
        debug_assert_eq!(module_name, MODULE_NAME.to_bytes());
        debug_assert_eq!(table_name, MODULE_NAME.to_bytes());
        let vtab = Self { base: sqlite3_vtab::default() };
        db.config(VTabConfig::Innocuous)?;
        Ok((Cow::Borrowed(c"CREATE TABLE x(value,start hidden,stop hidden,step hidden)"), vtab))
    }

    /// `seriesBestIndex` do 3.46.1. O plano vai em `idxNum` com os bits `PLAN_*`.
    fn best_index(&self, info: &mut IndexInfo) -> Result<bool> {
        let mut idx_num: c_int = 0;
        // Houve restrição de igualdade na coluna `start` (usável ou não).
        let mut start_seen = false;
        // Máscara das restrições inutilizáveis.
        let mut unusable_mask: c_int = 0;
        // Restrições sobre start, stop, step, LIMIT e OFFSET.
        let mut a_idx: [Option<usize>; 5] = [None; 5];
        for (i, constraint) in info.constraints().enumerate() {
            let op = constraint.operator();
            if op == IndexConstraintOp::SQLITE_INDEX_CONSTRAINT_LIMIT
                || op == IndexConstraintOp::SQLITE_INDEX_CONSTRAINT_OFFSET
            {
                if !constraint.is_usable() {
                    // Nada a fazer.
                } else if op == IndexConstraintOp::SQLITE_INDEX_CONSTRAINT_LIMIT {
                    a_idx[3] = Some(i);
                    idx_num |= PLAN_LIMIT;
                } else {
                    a_idx[4] = Some(i);
                    idx_num |= PLAN_OFFSET;
                }
                continue;
            }
            if constraint.column() < COLUMN_START {
                continue;
            }
            let i_col = (constraint.column() - COLUMN_START) as usize;
            if i_col > 2 {
                continue;
            }
            let mask: c_int = 1 << i_col;
            if i_col == 0 && op == IndexConstraintOp::SQLITE_INDEX_CONSTRAINT_EQ {
                start_seen = true;
            }
            if !constraint.is_usable() {
                unusable_mask |= mask;
                continue;
            } else if op == IndexConstraintOp::SQLITE_INDEX_CONSTRAINT_EQ {
                idx_num |= mask;
                a_idx[i_col] = Some(i);
            }
        }
        // O C compara `aIdx[3]==0` (e não -1) para "ignorar o OFFSET sem LIMIT"; o porte repete.
        if a_idx[3] == Some(0) {
            idx_num &= !(PLAN_LIMIT | PLAN_OFFSET);
            a_idx[4] = Some(0);
        }
        // Número de argumentos que `filter` espera.
        let mut n_arg: c_int = 0;
        for j in a_idx.iter().flatten() {
            n_arg += 1;
            let mut usage = info.constraint_usage(*j);
            usage.set_argv_index(n_arg);
            usage.set_omit(true);
        }
        // O gerador exige pelo menos o START (as versões antigas assumiam 0).
        if !start_seen {
            return Err(Error::ModuleError("first argument to \"generate_series()\" missing or unusable".to_string()));
        }
        if unusable_mask & !idx_num != 0 {
            // start, stop e step são entradas: restrição inutilizável nelas inviabiliza o plano.
            return Ok(false);
        }
        if idx_num & (PLAN_START | PLAN_STOP) == (PLAN_START | PLAN_STOP) {
            // start= e stop= disponíveis: o caso preferido.
            info.set_estimated_cost(f64::from(2 - c_int::from(idx_num & PLAN_STEP != 0)));
            info.set_estimated_rows(1000);
            let order_by_consumed = {
                let mut order_bys = info.order_bys();
                match order_bys.next() {
                    Some(order_by) if order_by.column() == 0 => {
                        if order_by.is_order_by_desc() {
                            idx_num |= PLAN_DESC;
                        } else {
                            idx_num |= PLAN_ASC;
                        }
                        true
                    }
                    _ => false,
                }
            };
            if order_by_consumed {
                info.set_order_by_consumed(true);
            }
        } else if idx_num & (PLAN_START | PLAN_LIMIT) == (PLAN_START | PLAN_LIMIT) {
            // start= e LIMIT.
            info.set_estimated_rows(2500);
        } else {
            // Falta um limite: o intervalo é enorme. Custa caro para o planejador evitar o caso.
            info.set_estimated_rows(2_147_483_647);
        }
        info.set_idx_num(idx_num);
        Ok(true)
    }

    fn open(&mut self) -> Result<SeriesTabCursor<'_>> {
        Ok(SeriesTabCursor::default())
    }
}

/// Cursor da tabela virtual.
#[derive(Default)]
#[repr(C)]
struct SeriesTabCursor<'vtab> {
    /// Classe base: precisa ser o primeiro campo.
    base: sqlite3_vtab_cursor,
    ss: SequenceSpec,
    phantom: PhantomData<&'vtab SeriesTab>,
}

unsafe impl VTabCursor for SeriesTabCursor<'_> {
    /// `seriesFilter` do 3.46.1.
    fn filter(&mut self, idx_num: c_int, _idx_str: Option<&str>, args: &Filters<'_>) -> Result<()> {
        let mut idx_num = idx_num;
        let argv: Vec<ValueRef<'_>> = args.iter().collect();
        let mut i = 0;
        let mut next_arg = || {
            let v = value_int64(argv.get(i));
            i += 1;
            v
        };
        let ss = &mut self.ss;
        ss.base = if idx_num & PLAN_START != 0 { next_arg() } else { 0 };
        ss.term = if idx_num & PLAN_STOP != 0 { next_arg() } else { 0xffff_ffff };
        if idx_num & PLAN_STEP != 0 {
            ss.step = next_arg();
            if ss.step == 0 {
                ss.step = 1;
            } else if ss.step < 0 && idx_num & PLAN_ASC == 0 {
                idx_num |= PLAN_DESC;
            }
        } else {
            ss.step = 1;
        }
        if idx_num & PLAN_LIMIT != 0 {
            let limit = next_arg();
            if idx_num & PLAN_OFFSET != 0 {
                let offset = next_arg();
                if offset > 0 {
                    ss.base = ss.base.wrapping_add(ss.step.wrapping_mul(offset));
                }
            }
            if limit >= 0 {
                let term = ss.base.wrapping_add((limit - 1).wrapping_mul(ss.step));
                if ss.step < 0 {
                    if term > ss.term {
                        ss.term = term;
                    }
                } else if term < ss.term {
                    ss.term = term;
                }
            }
        }
        if argv.iter().any(|v| matches!(v, ValueRef::Null)) {
            // Qualquer restrição NULL não devolve linha nenhuma (ticket fac496b61722daf2).
            ss.base = 1;
            ss.term = 0;
            ss.step = 1;
        }
        ss.is_reversing = if idx_num & PLAN_DESC != 0 { ss.step > 0 } else { ss.step < 0 };
        ss.setup();
        Ok(())
    }

    fn next(&mut self) -> Result<()> {
        self.ss.progress();
        Ok(())
    }

    fn eof(&self) -> bool {
        !self.ss.is_not_eof
    }

    fn column(&self, ctx: &mut Context, i: c_int) -> Result<()> {
        let x = match i {
            1 => self.ss.base,
            2 => self.ss.term,
            3 => self.ss.step,
            _ => self.ss.value_now,
        };
        ctx.set_result(&x)
    }

    fn rowid(&self) -> Result<i64> {
        let n = self.ss.seq_index_now;
        Ok(if n < u64::MAX { (n + 1) as i64 } else { 0 })
    }
}
