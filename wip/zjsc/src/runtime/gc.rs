//! Porte de `heap/Heap.cpp` (`collectNow`, `collectIfNecessaryOrDefer`) na forma que o registro de células
//! permite: marcação e varredura sobre o `cell_registry`, parando o mundo, sem geração e sem `unsafe`.
//!
//! As raízes:
//! - a célula cujo `Rc` alguém fora do registro segura (contagem forte maior que 1): um local de função
//!   nativa, um campo `Rc` de outra estrutura ou um `thread_local` a está usando, então ela não cai. É o papel
//!   da varredura conservadora da pilha nativa do C++, só que exata;
//! - a pilha de registradores do interpretador, conservadora (todo padrão de bits de célula conta);
//! - as constantes de todos os `CodeBlock`s ligados e dos `UnlinkedCodeBlock`s vivos;
//! - as raízes explícitas (`Root`), de quem guarda um `JSValue` solto num local Rust entre execuções;
//! - o estado do VM e dos `thread_local`s que guardam `JSValue` (`visit_global_roots`).
//!
//! A marcação segue os `JSValue` de cada célula marcada (`visit_children`): a base `JSObject` (armazenamento
//! inline, butterfly e protótipo da estrutura), os campos internos e o que cada tipo guarda a mais.
//!
//! Só se coleta num ponto seguro: o `op_loop_hint` com uma única execução do `llint` na pilha nativa (nenhuma
//! função nativa no meio, que poderia segurar um `JSValue` solto num local) e fora de `DeferGC`, ou a chamada
//! explícita (`VM::collect_garbage`) fora de qualquer execução.
//!
//! Em build de depuração a célula varrida deixa uma lápide: ler o id dela depois é pânico com o tipo, o que
//! transforma uma aresta esquecida em erro barulhento no lugar de comportamento estranho.
//!
//! DIVERGÊNCIAS:
//! - O índice de uma célula varrida não é reaproveitado (o slot fica vazio): os caches por id (o da
//!   `Structure` por protótipo, as tabelas laterais por célula) seguram ids mortos sem enfraquecê-los, como os
//!   `Weak` do C++ fariam. O slot vazio custa o tamanho de uma `Option<CellEntry>` e o dono.
//! - `WeakRef`, `WeakMap`, `WeakSet` e `FinalizationRegistry` ainda seguram o alvo forte.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Weak;

use crate::bytecode::unlinked_code_block::UnlinkedCodeBlock;
use crate::runtime::cell_registry::{self, CellEntry};
use crate::runtime::js_value::{JSValue, NOT_CELL_MASK};

/// `Heap::m_minBytesPerCycle` em células: o mínimo de alocações entre duas coletas automáticas.
const MIN_ALLOCATIONS_PER_CYCLE: usize = 100_000;

/// O conjunto marcado e a pilha de trabalho de uma coleta (`SlotVisitor`).
pub struct Marker {
    marked: Vec<bool>,
    worklist: Vec<usize>,
    /// Algum `RefCell` estava emprestado para escrita: a célula não pôde ser lida e a coleta desiste (não
    /// varre nada), porque não dá para provar o que ela segura.
    aborted: bool,
}

impl Marker {
    fn new(slot_count: usize) -> Marker {
        Marker { marked: vec![false; slot_count], worklist: Vec::new(), aborted: false }
    }

    /// `SlotVisitor::append(JSValue)`; o `Option` vazio é o `WriteBarrier` nulo.
    pub fn value(&mut self, value: impl Into<Option<JSValue>>) {
        if let Some(JSValue::Cell(cell_id)) = value.into() {
            self.cell(cell_id);
        }
    }

    /// `appendValues`.
    pub fn values(&mut self, values: &[JSValue]) {
        for value in values {
            self.value(*value);
        }
    }

    /// Marca a célula do id (ids fora do registro, de lápide ou de padrão conservador, não contam).
    pub fn cell(&mut self, cell_id: usize) {
        let Some(index) = cell_registry::index_of(cell_id) else { return };
        if let Some(marked) = self.marked.get_mut(index) {
            if !*marked {
                *marked = true;
                self.worklist.push(cell_id);
            }
        }
    }

    /// Um registrador da pilha ou outro valor codificado lido sem saber se é `JSValue`: conta se tem o padrão
    /// de bits de uma célula (a varredura conservadora do C++).
    pub fn encoded(&mut self, encoded: i64) {
        if encoded != 0 && encoded & NOT_CELL_MASK == 0 {
            self.cell(encoded as usize);
        }
    }

    /// O `RefCell` estava emprestado para escrita: desiste da coleta.
    pub fn abort(&mut self) {
        self.aborted = true;
    }
}

#[derive(Default)]
struct State {
    /// As raízes explícitas (`Root`), por id, com a contagem de quem as segura.
    roots: HashMap<usize, u32>,
    /// O tamanho do registro logo depois da última coleta: o que cresceu desde então foi alocado.
    slots_after_collection: usize,
    /// Quantas alocações disparam a próxima coleta automática.
    threshold: usize,
    /// Os `UnlinkedCodeBlock`s criados: as constantes dos vivos são raízes (o código pode ser ligado de novo).
    unlinked_code_blocks: Vec<Weak<RefCell<UnlinkedCodeBlock>>>,
    /// Os protótipos de toda `Structure` criada: a estrutura guarda o protótipo por id e vive em caches (o perfil
    /// de alocação de um construtor, o cache por protótipo, as transições) que podem criar objeto novo com ela.
    /// Como as estruturas não morrem antes do programa (a árvore de transições é cíclica), o protótipo também
    /// não.
    prototypes: std::collections::HashSet<usize>,
    collecting: bool,
    /// As lápides do build de depuração: id varrido e o tipo que ele tinha.
    #[cfg(debug_assertions)]
    swept: HashMap<usize, crate::runtime::js_type::JSType>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State { threshold: initial_threshold(), ..State::default() });
}

/// `ZJSC_GC_STRESS=N` coleta a cada N alocações (teste de estresse das raízes e das arestas).
fn stress_threshold() -> Option<usize> {
    std::env::var("ZJSC_GC_STRESS").ok().and_then(|value| value.parse().ok()).filter(|&value| value > 0)
}

fn initial_threshold() -> usize {
    stress_threshold().unwrap_or(MIN_ALLOCATIONS_PER_CYCLE)
}

/// Fim do programa (`cell_registry::reset_program_state`): o estado do coletor é do VM que cai.
pub(crate) fn reset_for_program() {
    let taken = STATE.try_with(|state| std::mem::take(&mut *state.borrow_mut()));
    drop(taken);
    let _ = STATE.try_with(|state| state.borrow_mut().threshold = initial_threshold());
}

/// Guarda um `UnlinkedCodeBlock` recém-criado entre as raízes (fraco: some com ele).
pub(crate) fn register_unlinked_code_block(code_block: &std::rc::Rc<RefCell<UnlinkedCodeBlock>>) {
    STATE.with(|state| state.borrow_mut().unlinked_code_blocks.push(std::rc::Rc::downgrade(code_block)));
}

/// Guarda o protótipo de uma `Structure` nova entre as raízes (ver `State::prototypes`).
pub(crate) fn retain_prototype(prototype: JSValue) {
    if let JSValue::Cell(cell_id) = prototype {
        let _ = STATE.try_with(|state| {
            if let Ok(mut state) = state.try_borrow_mut() {
                state.prototypes.insert(cell_id);
            }
        });
    }
}

/// `Strong<Unknown>`: mantém o valor vivo enquanto o `Root` existir, para quem o guarda num local Rust
/// enquanto JS roda (o resultado de um programa que o laço de eventos ainda não leu, por exemplo).
pub struct Root(usize);

impl Root {
    pub fn new(value: JSValue) -> Root {
        let cell_id = if let JSValue::Cell(cell_id) = value { cell_id } else { 0 };
        if cell_id != 0 {
            STATE.with(|state| *state.borrow_mut().roots.entry(cell_id).or_insert(0) += 1);
        }
        Root(cell_id)
    }
}

impl Drop for Root {
    fn drop(&mut self) {
        if self.0 == 0 {
            return;
        }
        let _ = STATE.try_with(|state| {
            let mut state = state.borrow_mut();
            if let Some(count) = state.roots.get_mut(&self.0) {
                *count -= 1;
                if *count == 0 {
                    state.roots.remove(&self.0);
                }
            }
        });
    }
}

/// O id foi varrido? Em depuração, ler uma lápide é pânico (aresta ou raiz esquecida).
#[cfg(debug_assertions)]
pub(crate) fn check_not_swept(cell_id: usize) {
    let swept = STATE.try_with(|state| state.borrow().swept.get(&cell_id).copied()).ok().flatten();
    if let Some(js_type) = swept {
        panic!("célula {cell_id:#x} ({js_type:?}) varrida pelo coletor ainda referenciada: falta uma raiz ou uma aresta");
    }
}

/// O slot do id voltou a ser usado (o registro cortou o rabo e realocou): a lápide sai.
#[cfg(debug_assertions)]
pub(crate) fn forget_tombstone(cell_id: usize) {
    let _ = STATE.try_with(|state| {
        if let Ok(mut state) = state.try_borrow_mut() {
            state.swept.remove(&cell_id);
        }
    });
}

/// `Heap::collectIfNecessaryOrDefer` no ponto seguro do interpretador: coleta se o limite de alocações foi
/// passado, só uma execução do `llint` está na pilha nativa e não há `DeferGC`.
pub(crate) fn safe_point(interpreter: &crate::interpreter::interpreter::Interpreter, vm: &crate::runtime::vm::VM) {
    if interpreter.native_depth() != 1 || vm.heap().is_deferred() {
        return;
    }
    let due = STATE.with(|state| {
        let state = state.borrow();
        !state.collecting && cell_registry::slot_count().saturating_sub(state.slots_after_collection) >= state.threshold
    });
    if due {
        collect(Some(interpreter), vm);
    }
}

/// `Heap::collectNow(Sync, CollectionScope::Full)`: marca a partir das raízes e varre o resto. Devolve
/// quantas células foram soltas.
pub fn collect(interpreter: Option<&crate::interpreter::interpreter::Interpreter>, vm: &crate::runtime::vm::VM) -> usize {
    let already = STATE.with(|state| std::mem::replace(&mut state.borrow_mut().collecting, true));
    if already {
        return 0;
    }
    let freed = mark_and_sweep(interpreter, vm);
    let live = cell_registry::live_cell_count();
    STATISTICS.with(|statistics| {
        let mut current = statistics.get();
        current.collections += 1;
        current.freed_cells += freed;
        current.peak_live_cells = current.peak_live_cells.max(live);
        statistics.set(current);
    });
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        state.collecting = false;
        state.slots_after_collection = cell_registry::slot_count();
        // `Heap::updateAllocationLimits`: o próximo ciclo espera alocar tanto quanto ficou vivo.
        state.threshold = stress_threshold().unwrap_or_else(|| live.max(MIN_ALLOCATIONS_PER_CYCLE));
    });
    freed
}

/// O que as coletas da thread fizeram (`Heap::numberOfCollections` e afins), para medir o coletor de fora.
#[derive(Clone, Copy, Debug, Default)]
pub struct Statistics {
    pub collections: usize,
    pub freed_cells: usize,
    /// O maior número de células vivas logo depois de uma coleta.
    pub peak_live_cells: usize,
}

thread_local! {
    /// Fora do `State` porque sobrevive ao fim do programa, que é quando o teste lê.
    static STATISTICS: std::cell::Cell<Statistics> = const { std::cell::Cell::new(Statistics { collections: 0, freed_cells: 0, peak_live_cells: 0 }) };
}

/// As estatísticas acumuladas da thread.
pub fn statistics() -> Statistics {
    STATISTICS.with(std::cell::Cell::get)
}

fn mark_and_sweep(interpreter: Option<&crate::interpreter::interpreter::Interpreter>, vm: &crate::runtime::vm::VM) -> usize {
    let (slot_count, pinned) = cell_registry::pinned_cells();
    let mut marker = Marker::new(slot_count);
    for cell_id in pinned {
        marker.cell(cell_id);
    }
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        for &cell_id in state.roots.keys().chain(state.prototypes.iter()) {
            marker.cell(cell_id);
        }
        state.unlinked_code_blocks.retain(|code_block| code_block.strong_count() > 0);
        for code_block in &state.unlinked_code_blocks {
            if let Some(code_block) = code_block.upgrade() {
                match code_block.try_borrow() {
                    Ok(code_block) => marker.values(&code_block.constant_registers),
                    Err(_) => marker.abort(),
                }
            }
        }
    });
    let owned;
    let interpreter = match interpreter {
        Some(interpreter) => interpreter,
        None => {
            owned = vm.interpreter();
            &owned
        }
    };
    interpreter.visit_gc_roots(&mut marker);
    vm.visit_gc_roots(&mut marker);
    while let Some(cell_id) = marker.worklist.pop() {
        if let Some(entry) = cell_registry::get_for_marking(cell_id) {
            visit_children(&entry, &mut marker);
        }
    }
    if marker.aborted || !interpreter.update_profiles_before_sweep() {
        return 0;
    }
    let swept = cell_registry::sweep(&marker.marked);
    let count = swept.len();
    #[cfg(debug_assertions)]
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        for (cell_id, js_type) in &swept {
            state.swept.insert(*cell_id, *js_type);
        }
    });
    count
}

/// `JSCell::visitChildren` por tipo: os `JSValue` que a célula segura.
fn visit_children(entry: &CellEntry, marker: &mut Marker) {
    if let Some(object) = entry.as_js_object() {
        object.visit_children(marker);
    }
    if let Some(fields) = entry.internal_fields() {
        for index in 0..fields.field_count() {
            marker.value(fields.field(index));
        }
    }
    entry.visit_extra_children(marker);
}
