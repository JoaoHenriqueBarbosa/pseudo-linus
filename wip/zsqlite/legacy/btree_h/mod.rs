// Mesclado das partes traduzidas de btree_h (as partes quebram funções grandes no meio).
#![allow(unused_imports, ambiguous_glob_reexports)]
use crate::prelude::*;

// ---- part_000.rs ----

// Número de entradas de metainformação na árvore B.
pub const SQLITE_N_BTREE_META: u32 = 16;

// Se definido como não-zero, auto-vacuum está habilitado por padrão. Caso contrário,
// deve ser ativado para cada banco de dados usando "PRAGMA auto_vacuum = 1".
pub const SQLITE_DEFAULT_AUTOVACUUM: u32 = 0;

// Tipos de auto-vacuum.
pub const BTREE_AUTOVACUUM_NONE: u32 = 0;      // Não fazer auto-vacuum
pub const BTREE_AUTOVACUUM_FULL: u32 = 1;      // Fazer auto-vacuum completo
pub const BTREE_AUTOVACUUM_INCR: u32 = 2;      // Vacuum incremental

// As declarações adiante de Btree, BtCursor e BtShared não geram item em Rust:
// as estruturas são definidas em btreeInt.h. BtreePayload é definida abaixo.

// Sinalizadores de sqlite3BtreeCreateTable (podem ser combinados com OR).
// Toda tabela precisa de BTREE_INTKEY ou BTREE_BLOBKEY.
pub const BTREE_INTKEY: u32 = 1;                // Tabela só tem chaves inteiras de 64 bits com sinal
pub const BTREE_BLOBKEY: u32 = 2;               // Tabela só tem chaves, sem dados

// Sinalizadores para sqlite3BtreeOpen (devem corresponder aos sinalizadores PAGER_ em pager.h)
pub const BTREE_OMIT_JOURNAL: u32 = 1;          // Não criar ou usar journal de rollback
pub const BTREE_MEMORY: u32 = 2;                // Este é um BD em memória
pub const BTREE_SINGLE: u32 = 4;                // O arquivo contém no máximo 1 árvore-b
pub const BTREE_UNORDERED: u32 = 8;             // O uso de implementação hash é OK

// Sinalizadores permitidos para sqlite3BtreeDelete() e sqlite3BtreeInsert()
pub const BTREE_SAVEPOSITION: u32 = 0x02;       // Deixar cursor apontando para NEXT ou PREV
pub const BTREE_AUXDELETE: u32 = 0x04;          // Não é a operação de delete primária
pub const BTREE_APPEND: u32 = 0x08;             // Insert é provável um append
pub const BTREE_PREFORMAT: u32 = 0x80;          // Dados inseridos são uma célula pré-formatada

// Sinalizadores passados como terceiro argumento para sqlite3BtreeCursor().
pub const BTREE_WRCSR: u32 = 0x00000004;        // Cursor de leitura-escrita
pub const BTREE_FORDELETE: u32 = 0x00000008;    // Cursor é apenas para seek e delete

// Tipos de hints que podem ser passados para sqlite3BtreeCursorHint().
pub const BTREE_HINT_RANGE: u32 = 0;            // Restrições de intervalo em consultas

// Valores que podem ser OR'd juntos para formar o argumento ao hint BTREE_HINT_FLAGS.
pub const BTREE_BULKLOAD: u32 = 0x00000001;     // Usado para preencher índice em ordem classificada
pub const BTREE_SEEK_EQ: u32 = 0x00000002;      // Apenas buscas de igualdade, sem buscas de intervalo

// Índices das metainformações da árvore B no cabeçalho do banco de dados.
// O deslocamento do campo correspondente no cabeçalho é calculado como:
// deslocamento = 36 + (idx * 4)
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

/// Descrição do conteúdo de uma única entrada em uma árvore-b de índice ou tabela.
///
/// As árvores-b de índice (usadas para índices e também tabelas WITHOUT ROWID)
/// contêm uma chave arbitrária e nenhum dado. Estas árvores têm p_key e n_key
/// definidos para a chave, e os campos p_data, n_data, n_zero não inicializados.
/// Os campos a_mem e n_mem fornecem uma matriz de objetos Mem que são uma
/// decomposição da chave. O campo n_mem pode ser zero, indicando que nenhuma
/// decomposição está disponível.
///
/// As árvores-b de tabela (usadas para tabelas com rowid) contêm um rowid inteiro
/// usado como chave e passado no campo n_key. O campo p_key é zero.
/// p_data e n_data mantêm o conteúdo da nova entrada. n_zero bytes zero extras
/// são anexados ao final do conteúdo ao construir a entrada.
/// Os campos a_mem e n_mem não são inicializados para árvores-b de tabela.
///
/// Este objeto é usado para passar informações para sqlite3BtreeInsert().
/// As mesmas informações costumavam ser passadas como cinco parâmetros separados.
/// Mas colocar as informações neste objeto ajuda a manter a interface mais
/// organizada e compreensível, e também ajuda o código resultante a executar
/// um pouco mais rápido usando menos registradores para passagem de parâmetros.
///
/// Resumo de uso de campos:
///
///            Árvores-b de Tabela      Árvores-b de Índice
///
/// p_key       sempre NULL              chave codificada
/// n_key       o ROWID                  comprimento de p_key
/// p_data      dados                    não usado
/// a_mem       não usado                valor de chave decomposto
/// n_mem       não usado                entradas em a_mem
/// n_data      comprimento de p_data    não usado
/// n_zero      zeros extras após p_data não usado
pub struct BtreePayload {
    /// Conteúdo da chave para índices. NULL para tabelas.
    pub p_key: Option<Vec<u8>>,
    /// Tamanho de p_key para índices. PRIMARY KEY para tabelas.
    pub n_key: i64,
    /// Dados para tabelas.
    pub p_data: Option<Vec<u8>>,
    /// Primeiro dos n_mem valores na p_key desempacotada.
    pub a_mem: Option<Vec<Mem>>,
    /// Número de valores em a_mem[]. Pode ser zero.
    pub n_mem: u16,
    /// Tamanho de p_data. 0 se nenhum.
    pub n_data: i32,
    /// Dados zero extras anexados após p_data e n_data.
    pub n_zero: i32,
}


// ---- part_001.rs ----

//
// Cabeçalho btree.h do sqlite3.c 3.46.1, parte 1
// Configuração do Debian 13: cache compartilhada habilitada, SQLITE_THREADSAFE=1.
// Blocos SQLITE_DEBUG, SQLITE_TEST e NDEBUG (só usados em assert) excluídos.
// As demais linhas do trecho são só protótipos; as funções reais (btree_enter,
// btree_leave, btree_sharable, btree_connection_count e afins, de btmutex.c)
// são traduzidas nos seus próprios módulos.
//

/// Contagem de buscas: sem SQLITE_DEBUG o macro vale sempre zero.
#[inline]
pub fn btree_seek_count(_x: &Btree) -> u64 {
    0
}

