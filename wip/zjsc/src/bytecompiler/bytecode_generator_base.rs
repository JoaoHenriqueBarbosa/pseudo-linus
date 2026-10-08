//! Porte de `bytecompiler/BytecodeGeneratorBase.h` e `BytecodeGeneratorBaseInlines.h`.
//!
//! O `template<typename Traits>` vira o trait `BytecodeGeneratorTraits` com tipos associados
//! (`OpcodeID`, `CodeBlock`, `Writer`, `OpcodeTraits`) e a constante `opcodeForDisablingOptimizations`.
//!
//! `GenericLabel<Traits>::setLocation(BytecodeGenerator&, unsigned)` NÃO está no
//! `BytecodeGeneratorBaseInlines.h` desta versão do upstream: é a especialização
//! `GenericLabel<JSGeneratorTraits>::setLocation` em `BytecodeGenerator.cpp`, que patcheia os saltos
//! pendentes por opcode. No Rust ela é `BytecodeGeneratorTraits::set_label_location`, implementada
//! junto do `.cpp` pelo trait do JS; o `emit_label` daqui a chama como o C++ chama `label.setLocation`.
//!
//! `SuperSamplerBytecodeScope.h` foi omitido de propósito: a classe só tem construtor/destrutor
//! que chamam `emitSuperSamplerBegin/End`, e nenhum arquivo do upstream a instancia (só aparece
//! no `.xcodeproj`), então não há efeito observável a portar. Se um dia for usada, vira um guard
//! com `Drop` junto de `emit_super_sampler_begin/end`.

use crate::bytecode::virtual_register::virtual_register_for_local;
use crate::bytecompiler::label::{GenericLabel, GenericLabelRef, LabelGenerator};
use crate::bytecompiler::register_id::{RegisterID, RegisterIDRef};
use crate::wtf::math_extras::round_up_to_multiple_of;
use std::cell::RefCell;
use std::rc::Rc;

/// `stackAlignmentRegisters()` de `StackAlignment.h`: `stackAlignmentBytes() / sizeof(Register)`
/// (16 / 8).
const STACK_ALIGNMENT_REGISTERS: usize = 2;

/// `OpcodeSize`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpcodeSize {
    Narrow,
    Wide16,
    Wide32,
}

/// Resultado de `Fits<T, size>::convert`: o valor já na largura do operando (padrão de bits).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fitted {
    Narrow(u8),
    Wide16(u16),
    Wide32(u32),
}

/// `Fits<T, size>`: `convert` de um valor (opcode ou operando) para a largura pedida.
pub trait Fits {
    fn convert(&self, size: OpcodeSize) -> Fitted;
}

/// `Traits::OpcodeTraits`: os prefixos wide e a largura do opcode por tamanho
/// (`OpcodeIDWidthBySize<OpcodeTraits, size>::opcodeIDSize`).
pub trait OpcodeTraits {
    type OpcodeID: Fits + Copy;

    fn wide16() -> Self::OpcodeID;
    fn wide32() -> Self::OpcodeID;
    fn opcode_id_size(size: OpcodeSize) -> OpcodeSize;
}

/// `Traits::CodeBlock` (um ponteiro no C++): só o que o gerador-base usa.
pub trait GeneratorCodeBlock {
    fn num_callee_locals(&self) -> u32;
    fn set_num_callee_locals(&mut self, count: u32);
    fn num_vars(&self) -> i32;
    fn set_num_vars(&mut self, count: i32);
}

/// `InstructionStream<...>::MutableRef`: a referência para a última instrução escrita.
pub trait InstructionMutableRef<OpcodeID> {
    fn opcode_id(&self) -> OpcodeID;
    fn offset(&self) -> usize;
    fn size(&self) -> usize;
}

/// `InstructionStreamWriter<Traits::InstructionType>`.
pub trait InstructionStreamWriter: Default {
    type MutableRef: InstructionMutableRef<<<Self as InstructionStreamWriter>::Traits as BytecodeGeneratorTraits>::OpcodeID>;
    type Traits: BytecodeGeneratorTraits<Writer = Self>;

    fn position(&self) -> usize;
    /// `ref()` sem argumento: a referência para a posição atual.
    fn ref_(&self) -> Self::MutableRef;
    /// `write(args...)`, um valor por vez.
    fn write(&mut self, value: Fitted);
}

/// `typename Traits` do `BytecodeGeneratorBase`.
pub trait BytecodeGeneratorTraits: Sized {
    type OpcodeID: Fits + Copy + PartialEq;
    type OpcodeTraits: OpcodeTraits<OpcodeID = Self::OpcodeID>;
    type CodeBlock: GeneratorCodeBlock;
    type Writer: InstructionStreamWriter<Traits = Self>;

    /// `Traits::opcodeForDisablingOptimizations`.
    const OPCODE_FOR_DISABLING_OPTIMIZATIONS: Self::OpcodeID;

    /// `GenericLabel<Traits>::setLocation(BytecodeGenerator&, unsigned)`: escreve `m_location` e
    /// resolve os saltos pendentes (especialização por Traits no `.cpp`).
    fn set_label_location(
        generator: &mut BytecodeGeneratorBase<Self>,
        label: &GenericLabelRef<Self>,
        location: u32,
    );
}

/// O que `shrinkToFit` precisa do último elemento: `refCount()`.
trait RefCounted {
    fn ref_count(&self) -> i32;
}

impl<Traits> RefCounted for Rc<RefCell<GenericLabel<Traits>>> {
    fn ref_count(&self) -> i32 {
        self.borrow().ref_count()
    }
}

impl RefCounted for RegisterIDRef {
    fn ref_count(&self) -> i32 {
        self.borrow().ref_count()
    }
}

/// `shrinkToFit(SegmentedVector&)`.
fn shrink_to_fit<T: RefCounted>(segmented_vector: &mut Vec<T>) {
    while segmented_vector.last().is_some_and(|last| last.ref_count() == 0) {
        segmented_vector.pop();
    }
}

/// `BytecodeGeneratorBase<Traits>`. Os membros `protected` são `pub(crate)`.
pub struct BytecodeGeneratorBase<Traits: BytecodeGeneratorTraits> {
    pub(crate) writer: Traits::Writer,
    pub(crate) code_block: Traits::CodeBlock,

    pub(crate) out_of_memory_during_construction: bool,
    pub(crate) last_opcode_id: Traits::OpcodeID,
    pub(crate) last_instruction: <Traits::Writer as InstructionStreamWriter>::MutableRef,

    /// `SegmentedVector<GenericLabel<Traits>, 32>`: a contagem de referência vive no rótulo.
    pub(crate) labels: Vec<Rc<RefCell<GenericLabel<Traits>>>>,
    /// `SegmentedVector<RegisterID, 32>`.
    pub(crate) callee_locals: Vec<RegisterIDRef>,
}

impl<Traits: BytecodeGeneratorTraits> LabelGenerator for BytecodeGeneratorBase<Traits> {
    fn writer_position(&self) -> i32 {
        self.writer.position() as i32
    }
}

impl<Traits: BytecodeGeneratorTraits> BytecodeGeneratorBase<Traits> {
    /// `BytecodeGeneratorBase(typename Traits::CodeBlock, uint32_t virtualRegisterCountForCalleeSaves)`.
    pub fn new(code_block: Traits::CodeBlock, virtual_register_count_for_callee_saves: u32) -> Self {
        let writer = Traits::Writer::default();
        let last_instruction = writer.ref_();
        let mut this = BytecodeGeneratorBase {
            writer,
            code_block,
            out_of_memory_during_construction: false,
            last_opcode_id: Traits::OPCODE_FOR_DISABLING_OPTIMIZATIONS,
            last_instruction,
            labels: Vec::new(),
            callee_locals: Vec::new(),
        };
        this.allocate_callee_save_space(virtual_register_count_for_callee_saves);
        this
    }

    pub fn new_label(&mut self) -> GenericLabelRef<Traits> {
        shrink_to_fit(&mut self.labels);

        // Allocate new label ID.
        let label = Rc::new(RefCell::new(GenericLabel::new()));
        let result = GenericLabelRef::new(&label);
        self.labels.push(label);
        result
    }

    pub fn new_emitted_label(&mut self) -> GenericLabelRef<Traits> {
        let label = self.new_label();
        self.emit_label(&label);
        label
    }

    pub(crate) fn reclaim_free_registers(&mut self) {
        shrink_to_fit(&mut self.callee_locals);
    }

    pub fn emit_label(&mut self, label: &GenericLabelRef<Traits>) {
        let location = self.writer.position() as u32;
        Traits::set_label_location(self, label, location);
        self.last_opcode_id = Traits::OPCODE_FOR_DISABLING_OPTIMIZATIONS;
    }

    pub fn record_opcode(&mut self, opcode_id: Traits::OpcodeID) {
        debug_assert!(
            self.last_opcode_id == Traits::OPCODE_FOR_DISABLING_OPTIMIZATIONS
                || (self.last_opcode_id == self.last_instruction.opcode_id()
                    && self.writer.position() == self.last_instruction.offset() + self.last_instruction.size())
        );
        self.last_instruction = self.writer.ref_();
        self.last_opcode_id = opcode_id;
    }

    /// `write(Args... args)`.
    pub fn write(&mut self, args: &[Fitted]) {
        debug_assert!(!args.is_empty());
        for arg in args {
            self.writer.write(*arg);
        }
    }

    /// `writeOpcode<size>(opcodeID, ops...)`.
    pub fn write_opcode(&mut self, size: OpcodeSize, opcode_id: Traits::OpcodeID, ops: &[&dyn Fits]) {
        let opcode_id_size = <Traits::OpcodeTraits as OpcodeTraits>::opcode_id_size(size);
        match size {
            OpcodeSize::Wide16 => {
                let prefix = <Traits::OpcodeTraits as OpcodeTraits>::wide16();
                self.writer.write(prefix.convert(OpcodeSize::Narrow));
            }
            OpcodeSize::Wide32 => {
                let prefix = <Traits::OpcodeTraits as OpcodeTraits>::wide32();
                self.writer.write(prefix.convert(OpcodeSize::Narrow));
            }
            OpcodeSize::Narrow => {}
        }
        self.writer.write(opcode_id.convert(opcode_id_size));
        for op in ops {
            self.writer.write(op.convert(size));
        }
    }

    pub fn new_register(&mut self) -> RegisterIDRef {
        let local = virtual_register_for_local(self.callee_locals.len() as i32);
        let register = Rc::new(RefCell::new(RegisterID::from_virtual_register(local)));
        self.callee_locals.push(Rc::clone(&register));
        let mut num_callee_locals = (self.code_block.num_callee_locals() as usize).max(self.callee_locals.len());
        num_callee_locals = round_up_to_multiple_of(STACK_ALIGNMENT_REGISTERS, num_callee_locals);
        self.code_block.set_num_callee_locals(num_callee_locals as u32);
        assert_eq!(num_callee_locals, self.code_block.num_callee_locals() as usize);
        register
    }

    /// Returns the next available temporary register. Registers returned by `new_temporary`
    /// require a modified form of reference counting: any register with a refcount of 0 is
    /// considered "available", meaning that the next instruction may overwrite it.
    pub fn new_temporary(&mut self) -> RegisterIDRef {
        self.reclaim_free_registers();

        let result = self.new_register();
        result.borrow_mut().set_temporary();
        result
    }

    pub fn new_temporaries(&mut self, count: usize, mut func: impl FnMut(&RegisterIDRef)) {
        self.reclaim_free_registers();
        for _ in 0..count {
            let result = self.new_register();
            result.borrow_mut().set_temporary();
            func(&result);
        }
    }

    /// Adds an anonymous local var slot. To give this slot a name, add it to `symbolTable()`.
    pub fn add_var(&mut self) -> RegisterIDRef {
        let num_vars = self.code_block.num_vars();
        self.code_block.set_num_vars(num_vars + 1);
        let result = self.new_register();
        debug_assert_eq!(
            crate::bytecode::virtual_register::VirtualRegister::new(result.borrow().index()).to_local(),
            num_vars
        );
        result.borrow_mut().ref_(); // We should never free this slot.
        result
    }

    pub fn allocate_callee_save_space(&mut self, virtual_register_count_for_callee_saves: u32) {
        for _ in 0..virtual_register_count_for_callee_saves {
            self.add_var();
        }
    }
}
