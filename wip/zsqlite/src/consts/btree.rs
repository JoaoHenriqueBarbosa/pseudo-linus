//! Constantes extraídas de btree_h (codegen do material de consulta em legacy/).
#![allow(unused_imports)]
use super::*;
pub const SQLITE_N_BTREE_META: u32 = 16;
pub const SQLITE_DEFAULT_AUTOVACUUM: u32 = 0;
pub const BTREE_AUTOVACUUM_NONE: u32 = 0;      // Não fazer auto-vacuum
pub const BTREE_AUTOVACUUM_FULL: u32 = 1;      // Fazer auto-vacuum completo
pub const BTREE_AUTOVACUUM_INCR: u32 = 2;      // Vacuum incremental
pub const BTREE_INTKEY: u32 = 1;                // Tabela só tem chaves inteiras de 64 bits com sinal
pub const BTREE_BLOBKEY: u32 = 2;               // Tabela só tem chaves, sem dados
pub const BTREE_OMIT_JOURNAL: u32 = 1;          // Não criar ou usar journal de rollback
pub const BTREE_MEMORY: u32 = 2;                // Este é um BD em memória
pub const BTREE_SINGLE: u32 = 4;                // O arquivo contém no máximo 1 árvore-b
pub const BTREE_UNORDERED: u32 = 8;             // O uso de implementação hash é OK
pub const BTREE_SAVEPOSITION: u32 = 0x02;       // Deixar cursor apontando para NEXT ou PREV
pub const BTREE_AUXDELETE: u32 = 0x04;          // Não é a operação de delete primária
pub const BTREE_APPEND: u32 = 0x08;             // Insert é provável um append
pub const BTREE_PREFORMAT: u32 = 0x80;          // Dados inseridos são uma célula pré-formatada
pub const BTREE_WRCSR: u32 = 0x00000004;        // Cursor de leitura-escrita
pub const BTREE_FORDELETE: u32 = 0x00000008;    // Cursor é apenas para seek e delete
pub const BTREE_HINT_RANGE: u32 = 0;            // Restrições de intervalo em consultas
pub const BTREE_BULKLOAD: u32 = 0x00000001;     // Usado para preencher índice em ordem classificada
pub const BTREE_SEEK_EQ: u32 = 0x00000002;      // Apenas buscas de igualdade, sem buscas de intervalo
pub const BTREE_FREE_PAGE_COUNT: u32 = 0;
pub const BTREE_SCHEMA_VERSION: u32 = 1;
pub const BTREE_FILE_FORMAT: u32 = 2;
pub const BTREE_DEFAULT_CACHE_SIZE: u32 = 3;
pub const BTREE_LARGEST_ROOT_PAGE: u32 = 4;
pub const BTREE_TEXT_ENCODING: u32 = 5;
pub const BTREE_USER_VERSION: u32 = 6;
pub const BTREE_INCR_VACUUM: u32 = 7;
pub const BTREE_APPLICATION_ID: u32 = 8;
pub const BTREE_DATA_VERSION: u32 = 15;         // Um valor metavirtual
