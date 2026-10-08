//! Porte mínimo de `runtime/JSString.h` e `JSStringInlines.h`.
//!
//! DIVERGÊNCIA (depende do heap, camada 3): no C++ `JSString` é um `JSCell` alocado no heap do GC e
//! o `JSValue` guarda o ponteiro cru. Enquanto `crate::heap` não existe, a string é um valor
//! imutável compartilhado (`JSStringRef = Rc<JSString>`) sobre `WtfString`, e o `JSValue::Cell(usize)`
//! guarda o "endereço" que este módulo atribui: o índice de um registro por thread, deslocado em três
//! bits e somado de um para nunca coincidir com `Empty` (0) nem `Deleted` (4) e manter o bit 1 limpo
//! (padrão de célula do JSVALUE64). O registro mantém a string viva (sem coleta). Quando o heap
//! chegar, `JSString` vira célula, `cell_id()` vira o `CellId` e o registro some.
//!
//! Também fora desta fatia: ropes (`JSRopeString`, `is_rope()` é sempre falso, então toda string já
//! está resolvida), `StructureID`, `jsSingleCharacterString`, `SmallStrings` e a conversão por
//! `ExecState`. `try_get_value` é o `tryGetValue` (sem rope não há resolução que falhe) e
//! `value` é o `value(globalObject)`.

use std::cell::RefCell;
use std::rc::Rc;

use crate::runtime::vm::VM;
use crate::wtf::text::wtf_string::String as WtfString;

/// `class JSString`: valor imutável com o texto já resolvido.
#[derive(Debug)]
pub struct JSString {
    value: WtfString,
    cell_id: usize,
}

/// Referência compartilhada, o `JSString*` do C++.
pub type JSStringRef = Rc<JSString>;

thread_local! {
    /// Registro que faz o papel do heap: o índice (mais um, vezes oito) é o `cell_id`.
    static REGISTRY: RefCell<Vec<JSStringRef>> = const { RefCell::new(Vec::new()) };
    /// `SmallStrings::emptyString()`.
    static EMPTY_STRING: RefCell<Option<JSStringRef>> = const { RefCell::new(None) };
}

const CELL_ID_SHIFT: usize = 3;

impl JSString {
    /// `JSString::create(vm, String)`.
    fn create(value: WtfString) -> JSStringRef {
        REGISTRY.with(|registry| {
            let mut registry = registry.borrow_mut();
            let cell_id = (registry.len() + 1) << CELL_ID_SHIFT;
            let string = Rc::new(JSString { value, cell_id });
            registry.push(Rc::clone(&string));
            string
        })
    }

    /// Procura a string pelo `cell_id` guardado em `JSValue::Cell`.
    pub fn from_cell_id(cell_id: usize) -> Option<JSStringRef> {
        let index = (cell_id >> CELL_ID_SHIFT).checked_sub(1)?;
        if cell_id & ((1 << CELL_ID_SHIFT) - 1) != 0 {
            return None;
        }
        REGISTRY.with(|registry| registry.borrow().get(index).cloned())
    }

    /// O "endereço" da célula, o que o `JSValue` codifica.
    pub fn cell_id(&self) -> usize {
        self.cell_id
    }

    /// `length()`.
    pub fn length(&self) -> u32 {
        self.value.length()
    }

    /// `is8Bit()`.
    pub fn is_8bit(&self) -> bool {
        self.value.is_8bit()
    }

    /// `isRope()`: não há ropes neste porte.
    pub fn is_rope(&self) -> bool {
        false
    }

    /// `value(globalObject)`.
    pub fn value(&self) -> WtfString {
        self.value.clone()
    }

    /// `tryGetValue()`.
    pub fn try_get_value(&self) -> WtfString {
        self.value.clone()
    }
}

/// `jsString(vm, const String&)`.
pub fn js_string(_vm: &VM, value: &WtfString) -> JSStringRef {
    JSString::create(value.clone())
}

/// `jsOwnedString(vm, const String&)`: o texto não é internado, o mesmo que `js_string` aqui.
pub fn js_owned_string(_vm: &VM, value: &WtfString) -> JSStringRef {
    JSString::create(value.clone())
}

/// `jsEmptyString(vm)`: uma só instância por thread, como `SmallStrings::emptyString()`.
pub fn js_empty_string(_vm: &VM) -> JSStringRef {
    EMPTY_STRING.with(|slot| {
        let mut slot = slot.borrow_mut();
        Rc::clone(slot.get_or_insert_with(|| JSString::create(WtfString::from_latin1(b""))))
    })
}
