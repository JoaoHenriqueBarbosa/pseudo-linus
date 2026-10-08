//! Constantes de `runtime/IndexingType.h` (só os valores; as funções `has*` e
//! `leastUpperBoundOfIndexingTypes` não entram aqui).

/// `typedef uint8_t IndexingType`.
pub type IndexingType = u8;

pub const IS_ARRAY: IndexingType = 0x01;

pub const NO_INDEXING_SHAPE: IndexingType = 0x00;
pub const UNDECIDED_SHAPE: IndexingType = 0x02;
pub const INT32_SHAPE: IndexingType = 0x04;
pub const DOUBLE_SHAPE: IndexingType = 0x06;
pub const CONTIGUOUS_SHAPE: IndexingType = 0x08;
pub const ARRAY_STORAGE_SHAPE: IndexingType = 0x0A;
pub const SLOW_PUT_ARRAY_STORAGE_SHAPE: IndexingType = 0x0C;

pub const INDEXING_SHAPE_MASK: IndexingType = 0x0E;
pub const INDEXING_SHAPE_SHIFT: IndexingType = 1;
pub const NUMBER_OF_INDEXING_SHAPES: IndexingType = 7;
pub const INDEXING_TYPE_MASK: IndexingType = INDEXING_SHAPE_MASK | IS_ARRAY;

pub const COPY_ON_WRITE: IndexingType = 0x10;
pub const INDEXING_SHAPE_AND_WRITABILITY_MASK: IndexingType = COPY_ON_WRITE | INDEXING_SHAPE_MASK;
pub const INDEXING_MODE_MASK: IndexingType = COPY_ON_WRITE | INDEXING_TYPE_MASK;
pub const NUMBER_OF_COPY_ON_WRITE_INDEXING_MODES: IndexingType = 3;
pub const NUMBER_OF_ARRAY_INDEXING_MODES: IndexingType =
    NUMBER_OF_INDEXING_SHAPES + NUMBER_OF_COPY_ON_WRITE_INDEXING_MODES;

pub const MAY_HAVE_INDEXED_ACCESSORS: IndexingType = 0x20;

pub const INDEXING_TYPE_LOCK_IS_HELD: IndexingType = 0x40;
pub const INDEXING_TYPE_LOCK_HAS_PARKED: IndexingType = 0x80;

pub const NON_ARRAY: IndexingType = 0x0;
pub const NON_ARRAY_WITH_INT32: IndexingType = INT32_SHAPE;
pub const NON_ARRAY_WITH_DOUBLE: IndexingType = DOUBLE_SHAPE;
pub const NON_ARRAY_WITH_CONTIGUOUS: IndexingType = CONTIGUOUS_SHAPE;
pub const NON_ARRAY_WITH_ARRAY_STORAGE: IndexingType = ARRAY_STORAGE_SHAPE;
pub const NON_ARRAY_WITH_SLOW_PUT_ARRAY_STORAGE: IndexingType = SLOW_PUT_ARRAY_STORAGE_SHAPE;
/// `ArrayClass`.
pub const ARRAY_CLASS: IndexingType = IS_ARRAY;
pub const ARRAY_WITH_UNDECIDED: IndexingType = IS_ARRAY | UNDECIDED_SHAPE;
pub const ARRAY_WITH_INT32: IndexingType = IS_ARRAY | INT32_SHAPE;
pub const ARRAY_WITH_DOUBLE: IndexingType = IS_ARRAY | DOUBLE_SHAPE;
pub const ARRAY_WITH_CONTIGUOUS: IndexingType = IS_ARRAY | CONTIGUOUS_SHAPE;
pub const ARRAY_WITH_ARRAY_STORAGE: IndexingType = IS_ARRAY | ARRAY_STORAGE_SHAPE;
pub const ARRAY_WITH_SLOW_PUT_ARRAY_STORAGE: IndexingType = IS_ARRAY | SLOW_PUT_ARRAY_STORAGE_SHAPE;
pub const COPY_ON_WRITE_ARRAY_WITH_INT32: IndexingType = IS_ARRAY | INT32_SHAPE | COPY_ON_WRITE;
pub const COPY_ON_WRITE_ARRAY_WITH_DOUBLE: IndexingType = IS_ARRAY | DOUBLE_SHAPE | COPY_ON_WRITE;
pub const COPY_ON_WRITE_ARRAY_WITH_CONTIGUOUS: IndexingType = IS_ARRAY | CONTIGUOUS_SHAPE | COPY_ON_WRITE;
