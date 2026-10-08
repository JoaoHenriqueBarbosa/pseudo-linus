// Mesclado das partes traduzidas de sqlite3_h (as partes quebram funções grandes no meio).
#![allow(unused_imports, ambiguous_glob_reexports)]
use crate::prelude::*;

// ---- part_000.rs ----

/*
** 2001-09-15
**
** The author disclaims copyright to this source code.  In place of
** a legal notice, here is a blessing:
**
**    May you do good and not evil.
**    May you find forgiveness for yourself and forgive others.
**    May you share freely, never taking more than you give.
**
*************************************************************************
** Arquivo de cabeçalho SQLite 3 que define a interface apresentada
** pela biblioteca SQLite a programas clientes. Se uma função C, estrutura,
** tipo de dado ou definição de constante não aparecer neste arquivo, então
** não é uma API publicada do SQLite, está sujeita a mudanças sem aviso prévio
** e não deve ser referenciada por programas que usam o SQLite.
**
** Algumas definições neste arquivo são marcadas como "experimental".
** Interfaces experimentais são normalmente novas funcionalidades adicionadas
** recentemente ao SQLite. Não antecipamos mudanças em interfaces experimentais,
** mas nos reservamos o direito de fazer pequenas mudanças se a experiência de uso
** no mundo real sugerir que tais mudanças são prudentes.
**
** A documentação oficial de API em linguagem C para SQLite é derivada de
** comentários neste arquivo. Este arquivo é a fonte autoritária sobre como
** as interfaces do SQLite devem operar.
**
** O nome deste arquivo sob gerenciamento de configuração é "sqlite.h.in".
** O makefile faz algumas pequenas mudanças neste arquivo (como inserir o
** número de versão) e muda seu nome para "sqlite3.h" como parte do processo
** de construção.
*/

/// Constante de versão compilada do SQLite.
pub const SQLITE_VERSION: &str = "3.46.1";

/// Número de versão compilada do SQLite, no formato: (X * 1000000 + Y * 1000 + Z)
/// onde X, Y, Z correspondem aos números maior, menor e de lançamento em SQLITE_VERSION.
pub const SQLITE_VERSION_NUMBER: i32 = 3046001;

/// Identificador de origem compilado do SQLite, contendo data, hora e hash SHA3-256.
pub const SQLITE_SOURCE_ID: &str =
    "2024-08-13 09:16:08 c9c2ab54ba1f5f46360f1b4f35d849cd3f080e6fc2b6c60e91b16c63f69a1e33";

/// Constante de string contendo a versão compilada do SQLite.
pub const SQLITE3_VERSION: &str = SQLITE_VERSION;

/// Marca de deprecação (sem-op em Rust).
pub const SQLITE_DEPRECATED: () = ();

/// Marca de experimentação (sem-op em Rust).
pub const SQLITE_EXPERIMENTAL: () = ();

/// Retorna um ponteiro para a string de versão compilada da biblioteca SQLite.
///
/// Esta função é fornecida para uso em DLLs, onde os usuários normalmente não têm
/// acesso direto a constantes de string dentro da DLL.
pub fn libversion() -> &'static str {
    SQLITE_VERSION
}

/// Retorna o número de versão compilada da biblioteca SQLite.
///
/// Aplicações cautelosas podem incluir afirmações para verificar que este valor
/// corresponde a SQLITE_VERSION_NUMBER, garantindo que a aplicação está compilada
/// com arquivos de biblioteca e cabeçalho correspondentes.
pub fn libversion_number() -> i32 {
    SQLITE_VERSION_NUMBER
}

/// Retorna um ponteiro para a string de identificador de origem compilada da biblioteca SQLite.
///
/// O valor é o mesmo da constante SQLITE_SOURCE_ID, exceto que, se o SQLite foi construído
/// usando uma cópia editada da concatenação de amalgamação, os últimos quatro caracteres
/// do hash podem ser diferentes.
pub fn sourceid() -> &'static str {
    SQLITE_SOURCE_ID
}

/// Retorna zero se e somente se o SQLite foi compilado com código de mutexing omitido
/// devido à opção de compilação SQLITE_THREADSAFE ser definida como 0.
///
/// O SQLite pode ser compilado com ou sem mutexes. Quando a macro C do pré-processador
/// SQLITE_THREADSAFE for 1 ou 2, mutexes são habilitados e SQLite é thread-safe. Quando
/// a macro SQLITE_THREADSAFE for 0, os mutexes são omitidos. Sem os mutexes, não é seguro
/// usar SQLite simultaneamente de mais de uma thread.
///
/// Habilitar mutexes causa uma perda de desempenho mensurável. Então, se a velocidade é
/// de suprema importância, faz sentido desabilitar os mutexes. Mas para máxima segurança,
/// mutexes devem ser habilitados. O comportamento padrão é para mutexes serem habilitados.
///
/// Esta função retorna 1, indicando que o SQLite foi compilado com threading habilitado.
pub fn threadsafe() -> i32 {
    1
}

// `sqlite3_compileoption_used` e `sqlite3_compileoption_get` são só protótipos aqui; o corpo
// (que no Debian reporta as opções reais, pois SQLITE_OMIT_COMPILEOPTION_DIAGS não está
// definido) vem de ctime.c e main.c, traduzido como `api::compileoption_used` e
// `api::compileoption_get`.

/// Estrutura opaca que representa uma conexão de banco de dados SQLite aberta.
///
/// Cada banco de dados SQLite aberto é representado por um ponteiro para uma instância
/// dessa estrutura opaca. É útil pensar em um sqlite3 como um objeto. As interfaces
/// sqlite3_open(), sqlite3_open16() e sqlite3_open_v2() são seus construtores, e
/// sqlite3_close() e sqlite3_close_v2() são seus destrutores. Existem muitas outras
/// interfaces que são métodos em um objeto sqlite3.
// (O struct `Sqlite3` é definido em sqliteInt_h.)

/// Tipo alias para inteiros de 64 bits com sinal.
///
/// Por não haver uma forma multiplataforma de especificar tipos inteiros de 64 bits,
/// o SQLite inclui typedefs para inteiros de 64 bits com e sem sinal.
///
/// O sqlite3_int64 pode armazenar valores inteiros entre -9223372036854775808 e
/// +9223372036854775807 inclusive.
pub type Sqlite3Int64 = i64;

/// Tipo alias para inteiros de 64 bits sem sinal.
///
/// Por não haver uma forma multiplataforma de especificar tipos inteiros de 64 bits,
/// o SQLite inclui typedefs para inteiros de 64 bits com e sem sinal.
///
/// O sqlite3_uint64 pode armazenar valores inteiros entre 0 e +18446744073709551615 inclusive.
pub type Sqlite3Uint64 = u64;


// ---- part_001.rs ----

/// Tipo de callback do `exec`. Legado e descontinuado, incluído apenas para
/// compatibilidade histórica. O `void*` do usuário vira o contexto `&mut dyn Any`,
/// `char**` dos valores vira fatia de `Option<Vec<u8>>` (None é NULL) e `char**` dos
/// nomes vira fatia de `Vec<u8>`; o número de colunas é o tamanho das fatias.
pub type Sqlite3Callback =
    fn(&mut dyn std::any::Any, &[Option<Vec<u8>>], &[Vec<u8>]) -> i32;

/// Handle de arquivo aberto na camada VFS do SQLite. `None` em `p_methods` é o
/// `pMethods` NULL do C (o `xClose` não é chamado).
pub struct Sqlite3File {
    pub p_methods: Option<Rc<Sqlite3IoMethods>>,
}

// Códigos de resultado do SQLite
pub const SQLITE_OK: i32 = 0;
pub const SQLITE_ERROR: i32 = 1;
pub const SQLITE_INTERNAL: i32 = 2;
pub const SQLITE_PERM: i32 = 3;
pub const SQLITE_ABORT: i32 = 4;
pub const SQLITE_BUSY: i32 = 5;
pub const SQLITE_LOCKED: i32 = 6;
pub const SQLITE_NOMEM: i32 = 7;
pub const SQLITE_READONLY: i32 = 8;
pub const SQLITE_INTERRUPT: i32 = 9;
pub const SQLITE_IOERR: i32 = 10;
pub const SQLITE_CORRUPT: i32 = 11;
pub const SQLITE_NOTFOUND: i32 = 12;
pub const SQLITE_FULL: i32 = 13;
pub const SQLITE_CANTOPEN: i32 = 14;
pub const SQLITE_PROTOCOL: i32 = 15;
pub const SQLITE_EMPTY: i32 = 16;
pub const SQLITE_SCHEMA: i32 = 17;
pub const SQLITE_TOOBIG: i32 = 18;
pub const SQLITE_CONSTRAINT: i32 = 19;
pub const SQLITE_MISMATCH: i32 = 20;
pub const SQLITE_MISUSE: i32 = 21;
pub const SQLITE_NOLFS: i32 = 22;
pub const SQLITE_AUTH: i32 = 23;
pub const SQLITE_FORMAT: i32 = 24;
pub const SQLITE_RANGE: i32 = 25;
pub const SQLITE_NOTADB: i32 = 26;
pub const SQLITE_NOTICE: i32 = 27;
pub const SQLITE_WARNING: i32 = 28;
pub const SQLITE_ROW: i32 = 100;
pub const SQLITE_DONE: i32 = 101;

// Códigos de resultado estendidos
pub const SQLITE_ERROR_MISSING_COLLSEQ: i32 = SQLITE_ERROR | (1 << 8);
pub const SQLITE_ERROR_RETRY: i32 = SQLITE_ERROR | (2 << 8);
pub const SQLITE_ERROR_SNAPSHOT: i32 = SQLITE_ERROR | (3 << 8);
pub const SQLITE_IOERR_READ: i32 = SQLITE_IOERR | (1 << 8);
pub const SQLITE_IOERR_SHORT_READ: i32 = SQLITE_IOERR | (2 << 8);
pub const SQLITE_IOERR_WRITE: i32 = SQLITE_IOERR | (3 << 8);
pub const SQLITE_IOERR_FSYNC: i32 = SQLITE_IOERR | (4 << 8);
pub const SQLITE_IOERR_DIR_FSYNC: i32 = SQLITE_IOERR | (5 << 8);
pub const SQLITE_IOERR_TRUNCATE: i32 = SQLITE_IOERR | (6 << 8);
pub const SQLITE_IOERR_FSTAT: i32 = SQLITE_IOERR | (7 << 8);
pub const SQLITE_IOERR_UNLOCK: i32 = SQLITE_IOERR | (8 << 8);
pub const SQLITE_IOERR_RDLOCK: i32 = SQLITE_IOERR | (9 << 8);
pub const SQLITE_IOERR_DELETE: i32 = SQLITE_IOERR | (10 << 8);
pub const SQLITE_IOERR_BLOCKED: i32 = SQLITE_IOERR | (11 << 8);
pub const SQLITE_IOERR_NOMEM: i32 = SQLITE_IOERR | (12 << 8);
pub const SQLITE_IOERR_ACCESS: i32 = SQLITE_IOERR | (13 << 8);
pub const SQLITE_IOERR_CHECKRESERVEDLOCK: i32 = SQLITE_IOERR | (14 << 8);
pub const SQLITE_IOERR_LOCK: i32 = SQLITE_IOERR | (15 << 8);
pub const SQLITE_IOERR_CLOSE: i32 = SQLITE_IOERR | (16 << 8);
pub const SQLITE_IOERR_DIR_CLOSE: i32 = SQLITE_IOERR | (17 << 8);
pub const SQLITE_IOERR_SHMOPEN: i32 = SQLITE_IOERR | (18 << 8);
pub const SQLITE_IOERR_SHMSIZE: i32 = SQLITE_IOERR | (19 << 8);
pub const SQLITE_IOERR_SHMLOCK: i32 = SQLITE_IOERR | (20 << 8);
pub const SQLITE_IOERR_SHMMAP: i32 = SQLITE_IOERR | (21 << 8);
pub const SQLITE_IOERR_SEEK: i32 = SQLITE_IOERR | (22 << 8);
pub const SQLITE_IOERR_DELETE_NOENT: i32 = SQLITE_IOERR | (23 << 8);
pub const SQLITE_IOERR_MMAP: i32 = SQLITE_IOERR | (24 << 8);
pub const SQLITE_IOERR_GETTEMPPATH: i32 = SQLITE_IOERR | (25 << 8);
pub const SQLITE_IOERR_CONVPATH: i32 = SQLITE_IOERR | (26 << 8);
pub const SQLITE_IOERR_VNODE: i32 = SQLITE_IOERR | (27 << 8);
pub const SQLITE_IOERR_AUTH: i32 = SQLITE_IOERR | (28 << 8);
pub const SQLITE_IOERR_BEGIN_ATOMIC: i32 = SQLITE_IOERR | (29 << 8);
pub const SQLITE_IOERR_COMMIT_ATOMIC: i32 = SQLITE_IOERR | (30 << 8);
pub const SQLITE_IOERR_ROLLBACK_ATOMIC: i32 = SQLITE_IOERR | (31 << 8);
pub const SQLITE_IOERR_DATA: i32 = SQLITE_IOERR | (32 << 8);
pub const SQLITE_IOERR_CORRUPTFS: i32 = SQLITE_IOERR | (33 << 8);
pub const SQLITE_IOERR_IN_PAGE: i32 = SQLITE_IOERR | (34 << 8);
pub const SQLITE_LOCKED_SHAREDCACHE: i32 = SQLITE_LOCKED | (1 << 8);
pub const SQLITE_LOCKED_VTAB: i32 = SQLITE_LOCKED | (2 << 8);
pub const SQLITE_BUSY_RECOVERY: i32 = SQLITE_BUSY | (1 << 8);
pub const SQLITE_BUSY_SNAPSHOT: i32 = SQLITE_BUSY | (2 << 8);
pub const SQLITE_BUSY_TIMEOUT: i32 = SQLITE_BUSY | (3 << 8);
pub const SQLITE_CANTOPEN_NOTEMPDIR: i32 = SQLITE_CANTOPEN | (1 << 8);
pub const SQLITE_CANTOPEN_ISDIR: i32 = SQLITE_CANTOPEN | (2 << 8);
pub const SQLITE_CANTOPEN_FULLPATH: i32 = SQLITE_CANTOPEN | (3 << 8);
pub const SQLITE_CANTOPEN_CONVPATH: i32 = SQLITE_CANTOPEN | (4 << 8);
pub const SQLITE_CANTOPEN_DIRTYWAL: i32 = SQLITE_CANTOPEN | (5 << 8);
pub const SQLITE_CANTOPEN_SYMLINK: i32 = SQLITE_CANTOPEN | (6 << 8);
pub const SQLITE_CORRUPT_VTAB: i32 = SQLITE_CORRUPT | (1 << 8);
pub const SQLITE_CORRUPT_SEQUENCE: i32 = SQLITE_CORRUPT | (2 << 8);
pub const SQLITE_CORRUPT_INDEX: i32 = SQLITE_CORRUPT | (3 << 8);
pub const SQLITE_READONLY_RECOVERY: i32 = SQLITE_READONLY | (1 << 8);
pub const SQLITE_READONLY_CANTLOCK: i32 = SQLITE_READONLY | (2 << 8);
pub const SQLITE_READONLY_ROLLBACK: i32 = SQLITE_READONLY | (3 << 8);
pub const SQLITE_READONLY_DBMOVED: i32 = SQLITE_READONLY | (4 << 8);
pub const SQLITE_READONLY_CANTINIT: i32 = SQLITE_READONLY | (5 << 8);
pub const SQLITE_READONLY_DIRECTORY: i32 = SQLITE_READONLY | (6 << 8);
pub const SQLITE_ABORT_ROLLBACK: i32 = SQLITE_ABORT | (2 << 8);
pub const SQLITE_CONSTRAINT_CHECK: i32 = SQLITE_CONSTRAINT | (1 << 8);
pub const SQLITE_CONSTRAINT_COMMITHOOK: i32 = SQLITE_CONSTRAINT | (2 << 8);
pub const SQLITE_CONSTRAINT_FOREIGNKEY: i32 = SQLITE_CONSTRAINT | (3 << 8);
pub const SQLITE_CONSTRAINT_FUNCTION: i32 = SQLITE_CONSTRAINT | (4 << 8);
pub const SQLITE_CONSTRAINT_NOTNULL: i32 = SQLITE_CONSTRAINT | (5 << 8);
pub const SQLITE_CONSTRAINT_PRIMARYKEY: i32 = SQLITE_CONSTRAINT | (6 << 8);
pub const SQLITE_CONSTRAINT_TRIGGER: i32 = SQLITE_CONSTRAINT | (7 << 8);
pub const SQLITE_CONSTRAINT_UNIQUE: i32 = SQLITE_CONSTRAINT | (8 << 8);
pub const SQLITE_CONSTRAINT_VTAB: i32 = SQLITE_CONSTRAINT | (9 << 8);
pub const SQLITE_CONSTRAINT_ROWID: i32 = SQLITE_CONSTRAINT | (10 << 8);
pub const SQLITE_CONSTRAINT_PINNED: i32 = SQLITE_CONSTRAINT | (11 << 8);
pub const SQLITE_CONSTRAINT_DATATYPE: i32 = SQLITE_CONSTRAINT | (12 << 8);
pub const SQLITE_NOTICE_RECOVER_WAL: i32 = SQLITE_NOTICE | (1 << 8);
pub const SQLITE_NOTICE_RECOVER_ROLLBACK: i32 = SQLITE_NOTICE | (2 << 8);
pub const SQLITE_NOTICE_RBU: i32 = SQLITE_NOTICE | (3 << 8);
pub const SQLITE_WARNING_AUTOINDEX: i32 = SQLITE_WARNING | (1 << 8);
pub const SQLITE_AUTH_USER: i32 = SQLITE_AUTH | (1 << 8);
pub const SQLITE_OK_LOAD_PERMANENTLY: i32 = SQLITE_OK | (1 << 8);
pub const SQLITE_OK_SYMLINK: i32 = SQLITE_OK | (2 << 8);

// Flags para operações de abertura de arquivo
pub const SQLITE_OPEN_READONLY: i32 = 0x00000001;
pub const SQLITE_OPEN_READWRITE: i32 = 0x00000002;
pub const SQLITE_OPEN_CREATE: i32 = 0x00000004;
pub const SQLITE_OPEN_DELETEONCLOSE: i32 = 0x00000008;
pub const SQLITE_OPEN_EXCLUSIVE: i32 = 0x00000010;
pub const SQLITE_OPEN_AUTOPROXY: i32 = 0x00000020;
pub const SQLITE_OPEN_URI: i32 = 0x00000040;
pub const SQLITE_OPEN_MEMORY: i32 = 0x00000080;
pub const SQLITE_OPEN_MAIN_DB: i32 = 0x00000100;
pub const SQLITE_OPEN_TEMP_DB: i32 = 0x00000200;
pub const SQLITE_OPEN_TRANSIENT_DB: i32 = 0x00000400;
pub const SQLITE_OPEN_MAIN_JOURNAL: i32 = 0x00000800;
pub const SQLITE_OPEN_TEMP_JOURNAL: i32 = 0x00001000;
pub const SQLITE_OPEN_SUBJOURNAL: i32 = 0x00002000;
pub const SQLITE_OPEN_SUPER_JOURNAL: i32 = 0x00004000;
pub const SQLITE_OPEN_NOMUTEX: i32 = 0x00008000;
pub const SQLITE_OPEN_FULLMUTEX: i32 = 0x00010000;
pub const SQLITE_OPEN_SHAREDCACHE: i32 = 0x00020000;
pub const SQLITE_OPEN_PRIVATECACHE: i32 = 0x00040000;
pub const SQLITE_OPEN_WAL: i32 = 0x00080000;
pub const SQLITE_OPEN_NOFOLLOW: i32 = 0x01000000;
pub const SQLITE_OPEN_EXRESCODE: i32 = 0x02000000;
pub const SQLITE_OPEN_MASTER_JOURNAL: i32 = 0x00004000;

// Características de dispositivo I/O
pub const SQLITE_IOCAP_ATOMIC: i32 = 0x00000001;
pub const SQLITE_IOCAP_ATOMIC512: i32 = 0x00000002;
pub const SQLITE_IOCAP_ATOMIC1K: i32 = 0x00000004;
pub const SQLITE_IOCAP_ATOMIC2K: i32 = 0x00000008;
pub const SQLITE_IOCAP_ATOMIC4K: i32 = 0x00000010;
pub const SQLITE_IOCAP_ATOMIC8K: i32 = 0x00000020;
pub const SQLITE_IOCAP_ATOMIC16K: i32 = 0x00000040;
pub const SQLITE_IOCAP_ATOMIC32K: i32 = 0x00000080;
pub const SQLITE_IOCAP_ATOMIC64K: i32 = 0x00000100;
pub const SQLITE_IOCAP_SAFE_APPEND: i32 = 0x00000200;
pub const SQLITE_IOCAP_SEQUENTIAL: i32 = 0x00000400;
pub const SQLITE_IOCAP_UNDELETABLE_WHEN_OPEN: i32 = 0x00000800;
pub const SQLITE_IOCAP_POWERSAFE_OVERWRITE: i32 = 0x00001000;
pub const SQLITE_IOCAP_IMMUTABLE: i32 = 0x00002000;
pub const SQLITE_IOCAP_BATCH_ATOMIC: i32 = 0x00004000;

// Níveis de travamento de arquivo
pub const SQLITE_LOCK_NONE: i32 = 0;
pub const SQLITE_LOCK_SHARED: i32 = 1;
pub const SQLITE_LOCK_RESERVED: i32 = 2;
pub const SQLITE_LOCK_PENDING: i32 = 3;
pub const SQLITE_LOCK_EXCLUSIVE: i32 = 4;

// Flags de sincronização
pub const SQLITE_SYNC_NORMAL: i32 = 0x00002;
pub const SQLITE_SYNC_FULL: i32 = 0x00003;
pub const SQLITE_SYNC_DATAONLY: i32 = 0x00010;


// ---- part_002.rs ----

/// Região de memória compartilhada (ou mapeada) devolvida por `xShmMap` e `xFetch`.
/// Substitui o `void volatile**` e o `void**` do C.
pub type ShmRegion = Rc<RefCell<Vec<u8>>>;

/// Métodos de arquivo do VFS do SQLite. Cada arquivo aberto por `xOpen` popula um
/// `Sqlite3File` com uma referência a uma instância deste tipo. Define os métodos usados
/// nas operações sobre o arquivo aberto.
///
/// Se `xRead` devolve SQLITE_IOERR_SHORT_READ, precisa preencher com zeros a parte não
/// lida do buffer; VFS que não faz isso parece funcionar, mas acaba corrompendo o banco.
pub struct Sqlite3IoMethods {
    /// Versão da interface.
    pub i_version: i32,
    /// Fecha o arquivo.
    pub x_close: Option<FileCloseFn>,
    /// Lê dados do arquivo (o tamanho do buffer é `iAmt`).
    pub x_read: Option<FileReadFn>,
    /// Escreve dados no arquivo (o tamanho da fatia é `iAmt`).
    pub x_write: Option<FileWriteFn>,
    /// Trunca o arquivo.
    pub x_truncate: Option<FileTruncateFn>,
    /// Sincroniza o arquivo (fsync ou fullsync).
    pub x_sync: Option<FileSyncFn>,
    /// Obtém o tamanho do arquivo.
    pub x_file_size: Option<FileFileSizeFn>,
    /// Adquire lock no arquivo.
    pub x_lock: Option<FileLockFn>,
    /// Libera lock no arquivo.
    pub x_unlock: Option<FileUnlockFn>,
    /// Verifica se há lock reservado.
    pub x_check_reserved_lock: Option<FileCheckReservedLockFn>,
    /// Controle genérico de arquivo.
    pub x_file_control: Option<FileFileControlFn>,
    /// Obtém tamanho do setor.
    pub x_sector_size: Option<FileSectorSizeFn>,
    /// Obtém características do dispositivo.
    pub x_device_characteristics: Option<FileDeviceCharacteristicsFn>,
    // Métodos acima são válidos para a versão 1
    /// Mapeia região de memória compartilhada.
    pub x_shm_map: Option<FileShmMapFn>,
    /// Faz lock em região de memória compartilhada.
    pub x_shm_lock: Option<FileShmLockFn>,
    /// Barreira de memória compartilhada.
    pub x_shm_barrier: Option<FileShmBarrierFn>,
    /// Desfaz o mapeamento da memória compartilhada.
    pub x_shm_unmap: Option<FileShmUnmapFn>,
    // Métodos acima são válidos para a versão 2
    /// Busca região mapeada do arquivo.
    pub x_fetch: Option<FileFetchFn>,
    /// Libera região mapeada do arquivo.
    pub x_unfetch: Option<FileUnfetchFn>,
    // Métodos acima são válidos para a versão 3
    // Métodos adicionais podem ser acrescentados em versões futuras
}

/// `int (*xClose)(sqlite3_file*)`.
pub type FileCloseFn = fn(&mut Sqlite3File) -> i32;

/// `int (*xRead)(sqlite3_file*, void*, int iAmt, sqlite3_int64 iOfst)`; `iAmt` é o tamanho
/// da fatia.
pub type FileReadFn = fn(&mut Sqlite3File, &mut [u8], i64) -> i32;

/// `int (*xWrite)(sqlite3_file*, const void*, int iAmt, sqlite3_int64 iOfst)`.
pub type FileWriteFn = fn(&mut Sqlite3File, &[u8], i64) -> i32;

/// `int (*xTruncate)(sqlite3_file*, sqlite3_int64 size)`.
pub type FileTruncateFn = fn(&mut Sqlite3File, i64) -> i32;

/// `int (*xSync)(sqlite3_file*, int flags)`.
pub type FileSyncFn = fn(&mut Sqlite3File, i32) -> i32;

/// `int (*xFileSize)(sqlite3_file*, sqlite3_int64 *pSize)`.
pub type FileFileSizeFn = fn(&mut Sqlite3File, &mut i64) -> i32;

/// `int (*xLock)(sqlite3_file*, int)`.
pub type FileLockFn = fn(&mut Sqlite3File, i32) -> i32;

/// `int (*xUnlock)(sqlite3_file*, int)`.
pub type FileUnlockFn = fn(&mut Sqlite3File, i32) -> i32;

/// `int (*xCheckReservedLock)(sqlite3_file*, int *pResOut)`.
pub type FileCheckReservedLockFn = fn(&mut Sqlite3File, &mut i32) -> i32;

/// `int (*xFileControl)(sqlite3_file*, int op, void *pArg)`; o `void*` vira o argumento
/// dinâmico que cada opcode `SQLITE_FCNTL_*` interpreta (`i32`, `i64`, `u32`, ...).
pub type FileFileControlFn = fn(&mut Sqlite3File, i32, Option<&mut dyn std::any::Any>) -> i32;

/// `int (*xSectorSize)(sqlite3_file*)`.
pub type FileSectorSizeFn = fn(&mut Sqlite3File) -> i32;

/// `int (*xDeviceCharacteristics)(sqlite3_file*)`.
pub type FileDeviceCharacteristicsFn = fn(&mut Sqlite3File) -> i32;

/// `int (*xShmMap)(sqlite3_file*, int iPg, int pgsz, int, void volatile**)`; o quarto
/// argumento é o `bExtend` e a região mapeada sai pelo último parâmetro.
pub type FileShmMapFn = fn(&mut Sqlite3File, i32, i32, i32, &mut Option<ShmRegion>) -> i32;

/// `int (*xShmLock)(sqlite3_file*, int offset, int n, int flags)`.
pub type FileShmLockFn = fn(&mut Sqlite3File, i32, i32, i32) -> i32;

/// `void (*xShmBarrier)(sqlite3_file*)`.
pub type FileShmBarrierFn = fn(&mut Sqlite3File);

/// `int (*xShmUnmap)(sqlite3_file*, int deleteFlag)`.
pub type FileShmUnmapFn = fn(&mut Sqlite3File, i32) -> i32;

/// `int (*xFetch)(sqlite3_file*, sqlite3_int64 iOfst, int iAmt, void **pp)`.
pub type FileFetchFn = fn(&mut Sqlite3File, i64, i32, &mut Option<ShmRegion>) -> i32;

/// `int (*xUnfetch)(sqlite3_file*, sqlite3_int64 iOfst, void *p)`.
pub type FileUnfetchFn = fn(&mut Sqlite3File, i64, Option<ShmRegion>) -> i32;

// As constantes SQLITE_IOCAP_*, SQLITE_LOCK_*, SQLITE_SYNC_* ficam em part_001 e as
// SQLITE_FCNTL_* em part_003 (mesma ordem do cabeçalho C); não se repetem aqui.


// ---- part_003.rs ----

/// Referência do código de controle de arquivo (FCNTL)
///
/// As constantes abaixo são os códigos de operação passados ao método xFileControl
/// do VFS. Cada código tem uma semântica específica.
///
/// SQLITE_FCNTL_LOCKSTATE: usado para depuração; grava o estado atual do lock (só com
/// SQLITE_DEBUG, portanto inerte neste porte).
///
/// SQLITE_FCNTL_GET_LOCKPROXYFILE: obtém o caminho do arquivo de proxy de travas
/// (macOS e sistemas com suporte a proxy de travaçamento de arquivo).
///
/// SQLITE_FCNTL_SET_LOCKPROXYFILE: define o caminho do arquivo de proxy de travas.
///
/// SQLITE_FCNTL_LAST_ERRNO: obtém o número do último erro do sistema operacional
/// ao tentar executar uma operação de arquivo.
///
/// SQLITE_FCNTL_SIZE_HINT: fornece uma dica ao VFS sobre o tamanho final esperado
/// do arquivo. O VFS pode usar isso para pré-alocar espaço ou otimizar a alocação.
///
/// SQLITE_FCNTL_CHUNK_SIZE: especifica o tamanho do bloco de alocação para o arquivo.
///
/// SQLITE_FCNTL_FILE_POINTER: obtém o descritor de arquivo nativo subjacente
/// associado ao manipulador de arquivo. Interpreta o argumento como um ponteiro
/// para um descritor de arquivo nativo e escreve o valor resultante lá.
///
/// SQLITE_FCNTL_WIN32_SET_HANDLE: usado para depuração. Troca o manipulador de arquivo
/// com o apontado pelo argumento pArg. Só precisa ser suportado quando SQLITE_TEST
/// está definido.
///
/// SQLITE_FCNTL_WAL_BLOCK: sinal à camada VFS de que pode ser vantajoso bloquear
/// no próximo lock de WAL se ele não estiver imediatamente disponível. Emitido
/// raramente pela subsistema WAL para corrigir problemas de inversão de prioridade.
///
/// SQLITE_FCNTL_ZIPVFS: implementado apenas por zipvfs. Todos os outros VFS retornam
/// SQLITE_NOTFOUND.
///
/// SQLITE_FCNTL_RBU: implementado apenas pelo VFS especial da extensão RBU.
/// Todos os outros VFS retornam SQLITE_NOTFOUND.
///
/// SQLITE_FCNTL_BEGIN_ATOMIC_WRITE: coloca o descritor de arquivo em modo de escrita
/// em lote atômico. Todas as operações de escrita subsequentes são adiadas e executadas
/// atomicamente no próximo SQLITE_FCNTL_COMMIT_ATOMIC_WRITE. Sistemas que não suportam
/// escritas atômicas em lote retornam SQLITE_NOTFOUND.
///
/// SQLITE_FCNTL_COMMIT_ATOMIC_WRITE: executa atomicamente todas as operações de escrita
/// desde o anterior SQLITE_FCNTL_BEGIN_ATOMIC_WRITE bem sucedido. Retorna SQLITE_OK se
/// e somente se as escritas foram todas executadas com sucesso e confirmadas no
/// armazenamento persistente. Tira o descritor de arquivo do modo de escrita em lote.
///
/// SQLITE_FCNTL_ROLLBACK_ATOMIC_WRITE: desfaz todas as operações de escrita desde o
/// anterior SQLITE_FCNTL_BEGIN_ATOMIC_WRITE bem sucedido. Tira o descritor de arquivo
/// do modo de escrita em lote, fazendo com que todas as operações de escrita
/// subsequentes sejam independentes.
///
/// SQLITE_FCNTL_LOCK_TIMEOUT: configura o tempo máximo de bloqueio ao tentar obter
/// um lock de arquivo. O argumento é um ponteiro para um inteiro assinado de 32 bits
/// contendo o valor em milissegundos. O inteiro é sobrescrito com o valor anterior.
///
/// SQLITE_FCNTL_DATA_VERSION: detecta mudanças no arquivo de banco de dados. O argumento
/// é um ponteiro para um inteiro sem sinal de 32 bits. A versão de dados para o pager
/// é escrita no ponteiro. A versão de dados muda sempre que ocorre qualquer mudança
/// no arquivo de banco de dados correspondente.
///
/// SQLITE_FCNTL_SIZE_LIMIT: define um limite de tamanho para o arquivo.
///
/// SQLITE_FCNTL_CKPT_START: invocado antes de o cliente começar a copiar páginas
/// do arquivo WAL para o arquivo de banco de dados durante um ponto de verificação.
///
/// SQLITE_FCNTL_CKPT_DONE: invocado após o cliente ter terminado de copiar páginas
/// do arquivo WAL para o arquivo de banco de dados, mas antes de o arquivo *-shm
/// ser atualizado para registrar que as páginas foram verificadas.
///
/// SQLITE_FCNTL_EXTERNAL_READER: detecta se há um cliente de banco de dados em outro
/// processo com uma transação em modo WAL aberta no banco de dados. Disponível apenas
/// em Unix. O argumento (void*) é um ponteiro para um inteiro que é definido como 1
/// se o banco está em modo WAL e existe pelo menos um cliente em outro processo com
/// uma transação SQL aberta. Definido como 0 caso contrário.
///
/// SQLITE_FCNTL_CKSM_FILE: para uso interno apenas pela camada de VFS de soma de
/// verificação.
///
/// SQLITE_FCNTL_RESET_CACHE: se não houver transação aberta e o banco não for temp,
/// limpa o cache de páginas em memória. Se houver transação aberta ou for temp,
/// é uma operação nula, não erro.
pub const SQLITE_FCNTL_LOCKSTATE: i32 = 1;
pub const SQLITE_FCNTL_GET_LOCKPROXYFILE: i32 = 2;
pub const SQLITE_FCNTL_SET_LOCKPROXYFILE: i32 = 3;
pub const SQLITE_FCNTL_LAST_ERRNO: i32 = 4;
pub const SQLITE_FCNTL_SIZE_HINT: i32 = 5;
pub const SQLITE_FCNTL_CHUNK_SIZE: i32 = 6;
pub const SQLITE_FCNTL_FILE_POINTER: i32 = 7;
pub const SQLITE_FCNTL_SYNC_OMITTED: i32 = 8;
pub const SQLITE_FCNTL_WIN32_AV_RETRY: i32 = 9;
pub const SQLITE_FCNTL_PERSIST_WAL: i32 = 10;
pub const SQLITE_FCNTL_OVERWRITE: i32 = 11;
pub const SQLITE_FCNTL_VFSNAME: i32 = 12;
pub const SQLITE_FCNTL_POWERSAFE_OVERWRITE: i32 = 13;
pub const SQLITE_FCNTL_PRAGMA: i32 = 14;
pub const SQLITE_FCNTL_BUSYHANDLER: i32 = 15;
pub const SQLITE_FCNTL_TEMPFILENAME: i32 = 16;
pub const SQLITE_FCNTL_MMAP_SIZE: i32 = 18;
pub const SQLITE_FCNTL_TRACE: i32 = 19;
pub const SQLITE_FCNTL_HAS_MOVED: i32 = 20;
pub const SQLITE_FCNTL_SYNC: i32 = 21;
pub const SQLITE_FCNTL_COMMIT_PHASETWO: i32 = 22;
pub const SQLITE_FCNTL_WIN32_SET_HANDLE: i32 = 23;
pub const SQLITE_FCNTL_WAL_BLOCK: i32 = 24;
pub const SQLITE_FCNTL_ZIPVFS: i32 = 25;
pub const SQLITE_FCNTL_RBU: i32 = 26;
pub const SQLITE_FCNTL_VFS_POINTER: i32 = 27;
pub const SQLITE_FCNTL_JOURNAL_POINTER: i32 = 28;
pub const SQLITE_FCNTL_WIN32_GET_HANDLE: i32 = 29;
pub const SQLITE_FCNTL_PDB: i32 = 30;
pub const SQLITE_FCNTL_BEGIN_ATOMIC_WRITE: i32 = 31;
pub const SQLITE_FCNTL_COMMIT_ATOMIC_WRITE: i32 = 32;
pub const SQLITE_FCNTL_ROLLBACK_ATOMIC_WRITE: i32 = 33;
pub const SQLITE_FCNTL_LOCK_TIMEOUT: i32 = 34;
pub const SQLITE_FCNTL_DATA_VERSION: i32 = 35;
pub const SQLITE_FCNTL_SIZE_LIMIT: i32 = 36;
pub const SQLITE_FCNTL_CKPT_DONE: i32 = 37;
pub const SQLITE_FCNTL_RESERVE_BYTES: i32 = 38;
pub const SQLITE_FCNTL_CKPT_START: i32 = 39;
pub const SQLITE_FCNTL_EXTERNAL_READER: i32 = 40;
pub const SQLITE_FCNTL_CKSM_FILE: i32 = 41;
pub const SQLITE_FCNTL_RESET_CACHE: i32 = 42;

// Nomes deprecados, mantidos por compatibilidade
pub const SQLITE_GET_LOCKPROXYFILE: i32 = SQLITE_FCNTL_GET_LOCKPROXYFILE;
pub const SQLITE_SET_LOCKPROXYFILE: i32 = SQLITE_FCNTL_SET_LOCKPROXYFILE;
pub const SQLITE_LAST_ERRNO: i32 = SQLITE_FCNTL_LAST_ERRNO;

/// Referência da estrutura de mutex do SQLite
///
/// O módulo de mutex dentro do SQLite define sqlite3_mutex como um tipo abstrato
/// para um objeto de mutex. O núcleo do SQLite nunca examina a representação interna
/// de um sqlite3_mutex, apenas trabalha com ponteiros para o objeto.
///
/// Mutexes são criados usando sqlite3_mutex_alloc().
pub struct Sqlite3Mutex;

/// Referência de Thunk de Extensão Carregável
///
/// Um ponteiro para a estrutura opaca sqlite3_api_routines é passado como terceiro
/// parâmetro para os pontos de entrada de extensões carregáveis. Essa estrutura precisa
/// ser typedef'd para contornar avisos do compilador em algumas plataformas.
pub struct Sqlite3ApiRoutines;

/// Referência de Nome de Arquivo
///
/// O tipo sqlite3_filename é usado pelo SQLite para passar nomes de arquivo ao método
/// xOpen de um VFS. Pode ser convertido para (const char*) e tratado como um buffer
/// UTF-8 normal terminado com nulo contendo o nome do arquivo, mas também pode ser
/// passado para APIs especiais como:
///
/// - sqlite3_filename_database()
/// - sqlite3_filename_journal()
/// - sqlite3_filename_wal()
/// - sqlite3_uri_parameter()
/// - sqlite3_uri_boolean()
/// - sqlite3_uri_int64()
/// - sqlite3_uri_key()
pub type Sqlite3Filename = Vec<u8>;

/// Referência da Interface do Sistema Operacional do VFS
///
/// Uma instância do objeto sqlite3_vfs define a interface entre o núcleo do SQLite
/// e o sistema operacional subjacente. O "vfs" no nome do objeto significa "virtual
/// file system" (sistema de arquivos virtual).
///
/// A interface VFS é às vezes estendida adicionando novos métodos ao final. Cada vez
/// que tal extensão ocorre, o campo iVersion é incrementado. O valor iVersion começou
/// como 1 no SQLite versão 3.5.0, aumentou para 2 com a versão 3.7.0, e depois
/// aumentou para 3 com a versão 3.7.6. Campos adicionais podem ser acrescentados ao
/// objeto sqlite3_vfs e o valor iVersion pode aumentar novamente em versões futuras
/// do SQLite.
///
/// O campo szOsFile é o tamanho da estrutura sqlite3_file subclassificada usada por
/// este VFS. mxPathname é o comprimento máximo de um caminho neste VFS.
///
/// Os objetos sqlite3_vfs registrados são mantidos em uma lista encadeada formada pelo
/// ponteiro pNext. As interfaces sqlite3_vfs_register() e sqlite3_vfs_unregister()
/// gerenciam essa lista de forma segura para thread. A interface sqlite3_vfs_find()
/// procura na lista. Nem o código da aplicação nem a implementação VFS devem usar
/// o ponteiro pNext.
///
/// O pNext é o único campo da estrutura sqlite3_vfs que o SQLite modificará. O SQLite
/// apenas acessa ou modifica esse campo enquanto mantém um mutex estático específico.
/// A aplicação nunca deve modificar nada dentro do objeto sqlite3_vfs uma vez que o
/// objeto tenha sido registrado.
///
/// O zName contém o nome do módulo VFS. O nome deve ser único entre todos os módulos VFS.
///
/// SQLite garante que o parâmetro zFilename para xOpen é um ponteiro NULL ou uma string
/// obtida de xFullPathname() com um sufixo opcional adicionado. Se um sufixo for
/// adicionado, consistirá em um caractere hífen seguido de no máximo 11 caracteres
/// alfanuméricos e/ou hífen. SQLite além disso garante que a string será válida e
/// inalterada até que xClose() seja chamado. Por causa da sentença anterior, o
/// sqlite3_file pode seguramente armazenar um ponteiro para o nome do arquivo se
/// precisar lembrar do nome do arquivo por algum motivo. Se o parâmetro zFilename
/// para xOpen for um ponteiro NULL, então xOpen deve inventar seu próprio nome
/// temporário para o arquivo.
///
/// O argumento flags para xOpen() inclui todos os bits definidos no argumento flags
/// para sqlite3_open_v2(). Ou se sqlite3_open() ou sqlite3_open16() for usado, então
/// flags inclui pelo menos SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE. Se xOpen()
/// abre um arquivo somente leitura, define *pOutFlags para incluir SQLITE_OPEN_READONLY.
/// Outros bits em *pOutFlags podem estar definidos.
///
/// SQLite também adicionará um dos seguintes flags à chamada xOpen(), dependendo
/// do objeto sendo aberto:
/// - SQLITE_OPEN_MAIN_DB
/// - SQLITE_OPEN_MAIN_JOURNAL
/// - SQLITE_OPEN_TEMP_DB
/// - SQLITE_OPEN_TEMP_JOURNAL
/// - SQLITE_OPEN_TRANSIENT_DB
/// - SQLITE_OPEN_SUBJOURNAL
/// - SQLITE_OPEN_SUPER_JOURNAL
/// - SQLITE_OPEN_WAL
///
/// A implementação de E/S de arquivo pode usar as flags de tipo de objeto para alterar
/// a forma como lida com os arquivos. Por exemplo, uma aplicação que não se importa
/// com recuperação de falhas ou reversão pode fazer a abertura de um arquivo de journal
/// uma operação sem efeito.
///
/// SQLite também pode adicionar um dos seguintes flags ao método xOpen:
/// - SQLITE_OPEN_DELETEONCLOSE
/// - SQLITE_OPEN_EXCLUSIVE
///
/// O flag SQLITE_OPEN_DELETEONCLOSE significa que o arquivo deve ser deletado quando
/// for fechado. Será definido para bancos de dados TEMP e seus journals, bancos de dados
/// transitórios e subjournals.
///
/// O flag SQLITE_OPEN_EXCLUSIVE é sempre usado em conjunto com o flag SQLITE_OPEN_CREATE,
/// que são ambos diretamente análogos aos flags O_EXCL e O_CREAT da API POSIX open().
/// O flag SQLITE_OPEN_EXCLUSIVE, quando emparelhado com SQLITE_OPEN_CREATE, é usado para
/// indicar que o arquivo deve sempre ser criado e é um erro se já existir. Não é usado
/// para indicar que o arquivo deve ser aberto para acesso exclusivo.
///
/// Pelo menos szOsFile bytes de memória são alocados pelo SQLite para manter a estrutura
/// sqlite3_file passada como terceiro argumento para xOpen. O método xOpen não precisa
/// alocar a estrutura, apenas preenchê-la. Note que o método xOpen deve definir o
/// sqlite3_file.pMethods para um objeto sqlite3_io_methods válido ou para NULL. xOpen
/// deve fazer isso mesmo que a abertura falhe. SQLite espera que o elemento
/// sqlite3_file.pMethods seja válido após xOpen retornar independentemente do sucesso
/// ou falha da chamada xOpen.
///
/// O argumento flags para xAccess() pode ser SQLITE_ACCESS_EXISTS para testar a
/// existência de um arquivo, ou SQLITE_ACCESS_READWRITE para testar se um arquivo
/// é legível e gravável, ou SQLITE_ACCESS_READ para testar se um arquivo é pelo menos
/// legível. O flag SQLITE_ACCESS_READ nunca é realmente usado e não é implementado nos
/// VFSes built-in do SQLite. O arquivo é nomeado pelo segundo argumento e pode ser um
/// diretório. O método xAccess retorna SQLITE_OK em caso de sucesso ou algum código
/// de erro diferente de zero se houver um erro de E/S ou se o nome do arquivo fornecido
/// no segundo argumento for inválido. Se SQLITE_OK for retornado, zero ou um valor
/// diferente de zero é escrito em *pResOut para indicar se o arquivo é acessível.
///
/// SQLite sempre aloca pelo menos mxPathname+1 bytes para o buffer de saída xFullPathname.
/// O tamanho exato do buffer de saída também é passado como parâmetro para ambos os
/// métodos. Se o buffer de saída não for grande o suficiente, SQLITE_CANTOPEN deve ser
/// retornado. Como isso é tratado como erro fatal pelo SQLite, as implementações VFS
/// devem se esforçar para prevenir isso definindo mxPathname para um valor suficientemente
/// grande.
///
/// As interfaces xRandomness(), xSleep(), xCurrentTime() e xCurrentTimeInt64() não fazem
/// parte estritamente do sistema de arquivos, mas são incluídas na estrutura VFS para
/// completude. A função xRandomness() tenta retornar nBytes bytes de boa qualidade de
/// aleatoriedade em zOut. O valor retornado é o número real de bytes de aleatoriedade
/// obtidos. O método xSleep() faz com que a thread chamadora durma por pelo menos o
/// número de microssegundos fornecido. O método xCurrentTime() retorna um Número de Dia
/// Juliano para a data e hora atuais como valor de ponto flutuante. O método
/// xCurrentTimeInt64() retorna, como inteiro, o Número de Dia Juliano multiplicado por
/// 86400000 (o número de milissegundos em um dia de 24 horas). SQLite usará o método
/// xCurrentTimeInt64() para obter a data e hora atuais se esse método estiver disponível
/// (se iVersion for 2 ou maior e o ponteiro de função não for NULL) e voltará para
/// xCurrentTime() se xCurrentTimeInt64() não estiver disponível.
///
/// As interfaces xSetSystemCall(), xGetSystemCall() e xNestSystemCall() não são usadas
/// pelo núcleo do SQLite. Essas interfaces opcionais são fornecidas por alguns VFSes para
/// facilitar testes do código VFS. Ao substituir chamadas do sistema com funções sob seu
/// controle, um programa de teste pode simular falhas e condições de erro que seriam
/// difíceis ou impossíveis de induzir de outra forma. O conjunto de chamadas do sistema
/// que podem ser substituídas varia de um VFS para outro e de uma versão para a próxima
/// do mesmo VFS. Aplicações que usam essas interfaces devem estar preparadas para
/// qualquer ou todas essas interfaces serem NULL ou para seu comportamento mudar de
/// uma versão para a próxima. Aplicações não devem tentar acessar nenhum desses métodos
/// se o iVersion do VFS for menor que 3.
/// Tipo opaco de ponteiro para chamada de sistema do VFS (`void (*)(void)`).
pub type Sqlite3SyscallPtr = fn();

/// Objeto de interface do sistema operacional. A lista encadeada de VFS registrados
/// (`pNext`) é do módulo `os`; o campo fica aqui por fidelidade à estrutura. Os métodos
/// recebem o próprio VFS (`&Sqlite3Vfs`) no lugar do `sqlite3_vfs*`, nomes são `&[u8]`
/// e os ponteiros de saída viram `&mut`.
pub struct Sqlite3Vfs {
    /// Número da versão da estrutura (atualmente 3)
    pub i_version: i32,
    /// Tamanho da estrutura sqlite3_file subclassificada
    pub sz_os_file: i32,
    /// Comprimento máximo de caminho de arquivo neste VFS
    pub mx_pathname: i32,
    /// Próximo VFS registrado
    pub p_next: Option<std::rc::Rc<std::cell::RefCell<Sqlite3Vfs>>>,
    /// Nome deste sistema de arquivos virtual
    pub z_name: Vec<u8>,
    /// Dados específicos da aplicação
    pub p_app_data: Option<std::rc::Rc<dyn std::any::Any>>,
    /// Abre um arquivo (`zName` None é NULL; o último parâmetro é `pOutFlags`)
    pub x_open: Option<fn(&Sqlite3Vfs, Option<&[u8]>, &mut Sqlite3File, i32, &mut i32) -> i32>,
    /// Deleta um arquivo
    pub x_delete: Option<fn(&Sqlite3Vfs, &[u8], i32) -> i32>,
    /// Verifica a existência ou acessibilidade de um arquivo (último parâmetro é `pResOut`)
    pub x_access: Option<fn(&Sqlite3Vfs, &[u8], i32, &mut i32) -> i32>,
    /// Retorna o caminho completo para um arquivo (`nOut` é o tamanho de `zOut`)
    pub x_full_pathname: Option<fn(&Sqlite3Vfs, &[u8], &mut [u8]) -> i32>,
    /// Abre uma biblioteca dinâmica (devolve um handle opaco, None é NULL)
    pub x_dl_open: Option<fn(&Sqlite3Vfs, &[u8]) -> Option<usize>>,
    /// Obtém mensagem de erro da biblioteca dinâmica (`nByte` é o tamanho de `zErrMsg`)
    pub x_dl_error: Option<fn(&Sqlite3Vfs, &mut [u8])>,
    /// Obtém o endereço de um símbolo da biblioteca dinâmica
    pub x_dl_sym: Option<fn(&Sqlite3Vfs, usize, &[u8]) -> Option<Sqlite3SyscallPtr>>,
    /// Fecha uma biblioteca dinâmica
    pub x_dl_close: Option<fn(&Sqlite3Vfs, usize)>,
    /// Preenche `zOut` com bytes aleatórios de boa qualidade (`nByte` é o tamanho)
    pub x_randomness: Option<fn(&Sqlite3Vfs, &mut [u8]) -> i32>,
    /// Coloca a thread em sleep, em microssegundos
    pub x_sleep: Option<fn(&Sqlite3Vfs, i32) -> i32>,
    /// Retorna a data e hora atual como Dia Juliano
    pub x_current_time: Option<fn(&Sqlite3Vfs, &mut f64) -> i32>,
    /// Obtém a mensagem de erro mais recente (`nByte` é o tamanho de `zBuf`)
    pub x_get_last_error: Option<fn(&Sqlite3Vfs, &mut [u8]) -> i32>,
    // Os métodos acima são da versão 1 da definição do objeto sqlite_vfs; os seguintes
    // foram acrescentados na versão 2 ou posterior.
    /// Retorna o Dia Juliano multiplicado por 86400000, como inteiro (versão 2+)
    pub x_current_time_int64: Option<fn(&Sqlite3Vfs, &mut i64) -> i32>,
    // Os métodos acima são das versões 1 e 2; os abaixo são da versão 3 em diante.
    /// Define uma chamada do sistema (versão 3+)
    pub x_set_system_call: Option<fn(&Sqlite3Vfs, Option<&[u8]>, Option<Sqlite3SyscallPtr>) -> i32>,
    /// Obtém uma chamada do sistema (versão 3+)
    pub x_get_system_call: Option<fn(&Sqlite3Vfs, &[u8]) -> Option<Sqlite3SyscallPtr>>,
    /// Obtém o nome da próxima chamada do sistema (versão 3+)
    pub x_next_system_call: Option<fn(&Sqlite3Vfs, Option<&[u8]>) -> Option<Vec<u8>>>,
    // Os métodos acima são das versões 1 a 3. Novos campos podem ser acrescentados em
    // versões futuras; o iVersion aumenta quando isso acontece.
}


// ---- part_004.rs ----

/// Bandeiras para o método xAccess do VFS.
/// Determinam que tipo de permissões o método xAccess está procurando.
/// SQLITE_ACCESS_EXISTS verifica se o arquivo existe.
/// SQLITE_ACCESS_READWRITE verifica se o diretório é legível e gravável.
/// SQLITE_ACCESS_READ verifica se o arquivo é legível (atualmente não usado).
pub const SQLITE_ACCESS_EXISTS: i32 = 0;
/// Usado por PRAGMA temp_store_directory.
pub const SQLITE_ACCESS_READWRITE: i32 = 1;
/// Não usado.
pub const SQLITE_ACCESS_READ: i32 = 2;

/// Bandeiras para o método xShmLock do VFS.
/// Definem as operações de bloqueio permitidas pelo método xShmLock.
pub const SQLITE_SHM_UNLOCK: i32 = 1;
pub const SQLITE_SHM_LOCK: i32 = 2;
pub const SQLITE_SHM_SHARED: i32 = 4;
pub const SQLITE_SHM_EXCLUSIVE: i32 = 8;

/// Índice máximo para xShmLock.
/// O método xShmLock pode usar valores entre 0 e este limite superior
/// como argumento de deslocamento.
pub const SQLITE_SHM_NLOCK: i32 = 8;

// sqlite3_initialize, sqlite3_shutdown, sqlite3_os_init, sqlite3_os_end,
// sqlite3_config e sqlite3_db_config são declarações de API pública: as
// implementações vivem no módulo `api` (main.c), pela regra de nomes.

/// Interface entre o SQLite e as rotinas de alocação de memória de baixo nível.
/// Usada com sqlite3_config(SQLITE_CONFIG_MALLOC ou SQLITE_CONFIG_GETMALLOC).
/// Sem ponteiros: um "ponteiro de alocação" é um identificador `usize` opaco
/// e os ponteiros de função viram `Rc<dyn Fn>`.
#[derive(Clone)]
pub struct Sqlite3MemMethods {
    /// Função de alocação de memória.
    pub x_malloc: Rc<dyn Fn(i32) -> usize>,
    /// Libera uma alocação anterior.
    pub x_free: Rc<dyn Fn(usize)>,
    /// Redimensiona uma alocação.
    pub x_realloc: Rc<dyn Fn(usize, i32) -> usize>,
    /// Retorna o tamanho de uma alocação.
    pub x_size: Rc<dyn Fn(usize) -> i32>,
    /// Arredonda o tamanho da requisição para o tamanho da alocação.
    pub x_roundup: Rc<dyn Fn(i32) -> i32>,
    /// Inicializa o alocador de memória.
    pub x_init: Rc<dyn Fn(usize) -> i32>,
    /// Desinicializa o alocador de memória.
    pub x_shutdown: Rc<dyn Fn(usize)>,
    /// Argumento para xInit() e xShutdown().
    pub p_app_data: usize,
}

// As constantes SQLITE_CONFIG_* cujo #define está no trecho seguinte do C
// (sqlite3_h.005) ficam em part_005.rs, para não duplicar definição.


// ---- part_005.rs ----

/// Opções alternativas de alocação de memória de baixo nível
/// (podem ser usadas no lugar das rotinas de alocação
/// embutidas no SQLite). O SQLite faz uma cópia privada
/// do conteúdo da estrutura [sqlite3_mem_methods]
/// antes que [sqlite3_config()] retorne.
///
/// [[SQLITE_CONFIG_GETMALLOC]] Opção para obter os métodos
/// de alocação de memória definidos atualmente.
/// A estrutura [sqlite3_mem_methods] é preenchida
/// com as rotinas de alocação de memória definidas.
/// Pode ser usada para sobrepor as rotinas padrão
/// com um wrapper que simula falha de alocação
/// ou rastreia uso de memória, por exemplo.
///
/// [[SQLITE_CONFIG_SMALL_MALLOC]] Dica a respeito de
/// alocação de memória grande. Se verdadeira, o SQLite
/// evita alocações grandes se possível. O SQLite
/// roda mais rápido se livre para fazer alocações grandes,
/// mas algumas aplicações podem preferir rodar mais lentamente
/// em troca de garantias sobre fragmentação de memória
/// que são possíveis se alocações grandes forem evitadas.
/// Desativada por padrão.
///
/// [[SQLITE_CONFIG_MEMSTATUS]] Habilita ou desabilita
/// coleta de estatísticas de alocação de memória.
/// Quando desabilitadas, as interfaces [sqlite3_hard_heap_limit64()],
/// [sqlite3_memory_used()], [sqlite3_memory_highwater()],
/// [sqlite3_soft_heap_limit64()] e [sqlite3_status64()] ficam inoperacionais.
/// Ativadas por padrão a menos que compilado com
/// [SQLITE_DEFAULT_MEMSTATUS]=0.
///
/// [[SQLITE_CONFIG_SCRATCH]] Opção descontinuada.
///
/// [[SQLITE_CONFIG_PAGECACHE]] Especifica um pool de memória
/// que o SQLite pode usar para o cache de página do banco de dados
/// com a implementação padrão de cache de página.
/// Sem efeito se uma implementação de cache de página
/// definida pela aplicação estiver carregada via [SQLITE_CONFIG_PCACHE2].
/// Três argumentos: ponteiro para memória alinhada a 8 bytes (pMem),
/// tamanho de cada linha de cache (sz), e número de linhas (N).
/// sz deve ser o tamanho da maior página do banco de dados
/// (potência de 2 entre 512 e 65536) mais alguns bytes extras
/// para cabeçalho de página. Bytes extras podem ser determinados
/// usando [SQLITE_CONFIG_PCACHE_HDRSZ].
/// Inofensivo (exceto memória desperdiçada) se sz for maior que necessário.
/// pMem deve ser NULL ou apontador para bloco alinhado a 8 bytes
/// com pelo menos sz*N bytes, senão comportamento indefinido.
/// Quando pMem não é NULL, SQLite tenta usar memória fornecida
/// para necessidades de cache, voltando a [sqlite3_malloc()] se
/// linha de cache for maior que sz ou se buffer pMem esgotado.
/// Se pMem é NULL e N não zero, cada conexão do banco faz
/// alocação em massa inicial de memória de cache
/// de [sqlite3_malloc()] suficiente para N linhas se N positivo
/// ou -1024*N bytes se N negativo. Se memória adicional
/// for necessária além do fornecido, SQLite vai a [sqlite3_malloc()]
/// separadamente para cada linha de cache adicional.
///
/// [[SQLITE_CONFIG_HEAP]] Especifica buffer estático de memória
/// que SQLite usa para todas as necessidades de alocação
/// dinâmica além daquelas fornecidas por [SQLITE_CONFIG_PAGECACHE].
/// Disponível apenas se SQLite compilado com [SQLITE_ENABLE_MEMSYS3]
/// ou [SQLITE_ENABLE_MEMSYS5], senão retorna [SQLITE_ERROR].
/// Três argumentos: ponteiro alinhado a 8 bytes para memória,
/// número de bytes no buffer, tamanho mínimo de alocação.
/// Se primeiro ponteiro (ponteiro de memória) for NULL,
/// SQLite reverte para alocador de memória padrão (system malloc),
/// desfazendo invocações prévias de [SQLITE_CONFIG_MALLOC].
/// Se ponteiro não NULL, alocador alternativo trata
/// todas necessidades de alocação de memória do SQLite.
/// Deve estar alinhado a limite de 8 bytes ou comportamento indefinido.
/// Tamanho mínimo de alocação é limitado a 2^12. Valores razoáveis
/// para tamanho mínimo são 2^5 a 2^8.
///
/// [[SQLITE_CONFIG_MUTEX]] Especifica rotinas mutex alternativas
/// de baixo nível a serem usadas no lugar das rotinas mutex
/// embutidas no SQLite. SQLite faz cópia do conteúdo
/// da estrutura [sqlite3_mutex_methods] antes que
/// [sqlite3_config()] retorne. Se compilado com
/// [SQLITE_THREADSAFE]=0, subsistema mutex inteiro é omitido
/// e chamadas a [sqlite3_config()] com SQLITE_CONFIG_MUTEX
/// retornam [SQLITE_ERROR].
///
/// [[SQLITE_CONFIG_GETMUTEX]] Preenche uma estrutura
/// [sqlite3_mutex_methods] com as rotinas mutex definidas.
/// Pode sobrepor rotinas padrão com wrapper usado para
/// rastreamento de desempenho ou testes, por exemplo.
/// Se compilado com [SQLITE_THREADSAFE]=0, retorna [SQLITE_ERROR].
///
/// [[SQLITE_CONFIG_LOOKASIDE]] Dois argumentos que determinam
/// tamanho padrão de memória lookaside em cada conexão.
/// Primeiro argumento é tamanho de cada slot de lookaside,
/// segundo é número de slots alocados por conexão.
/// [SQLITE_DBCONFIG_LOOKASIDE] para [sqlite3_db_config()]
/// pode mudar configuração lookaside em conexões individuais.
///
/// [[SQLITE_CONFIG_PCACHE2]] Especifica interface para
/// implementação customizada de cache de página via
/// [sqlite3_pcache_methods2]. SQLite faz cópia do objeto.
///
/// [[SQLITE_CONFIG_GETPCACHE2]] Copia implementação atual
/// de cache de página em objeto [sqlite3_pcache_methods2].
///
/// [[SQLITE_CONFIG_LOG]] Configura log de erro global do SQLite.
/// Dois argumentos: ponteiro para função com assinatura
/// void(*)(void*,int,const char*), ponteiro para void.
/// Se ponteiro função não NULL, é invocado por [sqlite3_log()]
/// para processar cada evento de log. Se NULL, torna sem efeito.
/// Ponteiro void passa como primeiro parâmetro da função logger
/// sempre que invocada. Segundo parâmetro é cópia do primeiro
/// parâmetro da chamada [sqlite3_log()] correspondente,
/// código de resultado ou código resultado estendido.
/// Terceiro parâmetro é mensagem de log após formatação
/// via [sqlite3_snprintf()]. Interface logging não reentrante,
/// função logger não deve invocar interface SQLite.
/// Em aplicação multi-thread, função logger deve ser thread-safe.
///
/// [[SQLITE_CONFIG_URI]] Habilita ou desabilita globalmente
/// tratamento de URI. Se não zero, tratamento URI ativado.
/// Se zero, desativado. Se ativado globalmente, todos nomes
/// de arquivo passados para [sqlite3_open()], [sqlite3_open_v2()],
/// [sqlite3_open16()] ou especificados em [ATTACH] são interpretados
/// como URIs, independentemente de flag [SQLITE_OPEN_URI].
/// Padrão é desativado globalmente, pode mudar compilando
/// com [SQLITE_USE_URI] definido.
///
/// [[SQLITE_CONFIG_COVERING_INDEX_SCAN]] Habilita ou desabilita
/// uso de índices cobrindo para varreduras completas de tabelas
/// em otimizador de queries. Padrão é determinado por
/// [SQLITE_ALLOW_COVERING_INDEX_SCAN], ou ligado se omitido.
/// Desabilitar útil para aplicações legadas codificadas incorretamente.
///
/// [[SQLITE_CONFIG_PCACHE]] e [[SQLITE_CONFIG_GETPCACHE]]
/// Opções obsoletas, retidas para compatibilidade,
/// agora sem efeito.
///
/// [[SQLITE_CONFIG_SQLLOG]] Disponível se compilado com
/// [SQLITE_ENABLE_SQLLOG]. Primeiro argumento é ponteiro
/// para função tipo void(*)(void*,sqlite3*,const char*, int).
/// Segundo é tipo (void*). Callback invocado em três situações
/// identificadas por quarto parâmetro. Se zero, conexão acabou
/// de abrir, terceiro argumento aponta nome arquivo banco principal.
/// Se um, instrução SQL apontada por terceiro parâmetro acabou executar.
/// Se dois, conexão sendo fechada, terceiro NULL.
/// Exemplo em "test_sqllog.c" no repositório canônico.
///
/// [[SQLITE_CONFIG_MMAP_SIZE]] Dois valores inteiros 64-bit
/// (sqlite3_int64) que são tamanho mmap padrão (padrão [PRAGMA mmap_size])
/// e tamanho mmap máximo permitido. Padrão pode ser sobrescrito
/// por cada conexão usando [PRAGMA mmap_size] ou
/// [SQLITE_FCNTL_MMAP_SIZE]. Tamanho máximo silenciosamente
/// truncado se necessário para não exceder máximo tempo compilação
/// [SQLITE_MAX_MMAP_SIZE]. Se argumento negativo, muda para padrão
/// tempo compilação.
///
/// [[SQLITE_CONFIG_WIN32_HEAPSIZE]] Disponível apenas se compilado
/// para Windows com [SQLITE_WIN32_MALLOC]. Valor inteiro sem sinal
/// 32-bit que especifica tamanho máximo do heap criado.
///
/// [[SQLITE_CONFIG_PCACHE_HDRSZ]] Escreve em parâmetro inteiro
/// número de bytes extras por página requeridos em [SQLITE_CONFIG_PAGECACHE].
/// Quantidade pode variar com compilador, plataforma alvo, versão SQLite.
///
/// [[SQLITE_CONFIG_PMASZ]] Parâmetro inteiro sem sinal
/// que define tamanho PMA mínimo para classificador multi-thread.
/// Padrão definido por [SQLITE_SORTER_PMASZ]. Novas threads lançadas
/// quando classificação multi-thread ativada ([PRAGMA threads])
/// e conteúdo excede tamanho página vezes mínimo de
/// [PRAGMA cache_size] e este valor.
///
/// [[SQLITE_CONFIG_STMTJRNL_SPILL]] Parâmetro que se torna
/// limiar de derrame em disco de journal de instrução.
/// Journals mantidos em memória até tamanho (bytes) exceder limiar,
/// momento em que gravados em disco. Ou se limiar -1, journals
/// sempre mantidos exclusivamente em memória. Muitos nunca crescem grande,
/// então limiar 64KiB reduz muito I/O de suporte a rollback.
/// Padrão controlado por [SQLITE_STMTJRNL_SPILL].
///
/// [[SQLITE_CONFIG_SORTERREF_SIZE]] Parâmetro tipo int, novo
/// valor de limiar de tamanho de referência do classificador.
/// Normalmente quando SQLite usa ordenação externa para registros,
/// todos campos necessários do chamador presentes nos registros ordenados.
/// Mas se SQLite determina baseado em tipo declarado que valores
/// coluna provavelmente muito grandes, referência armazenada
/// e valores coluna necessários carregados conforme registros retornados.
/// Padrão nunca usar otimização. Especificar negativo restaura padrão.
/// Disponível apenas se compilado com [SQLITE_ENABLE_SORTER_REFERENCES].
///
/// [[SQLITE_CONFIG_MEMDB_MAXSIZE]] Parâmetro [sqlite3_int64]
/// que é tamanho máximo padrão para banco de dados em memória
/// criado usando [sqlite3_deserialize()]. Padrão ajustável
/// para bancos individuais usando [SQLITE_FCNTL_SIZE_LIMIT].
/// Se nunca usado, padrão determinado por [SQLITE_MEMDB_DEFAULT_MAXSIZE].
/// Se não definido, padrão é 1073741824.
///
/// [[SQLITE_CONFIG_ROWID_IN_VIEW]] Habilita ou desabilita
/// capacidade de VIEWs terem ROWID. Pode ser habilitada
/// apenas se compilado com -DSQLITE_ALLOW_ROWID_IN_VIEW,
/// padrão ativado. Argumento é ponteiro para inteiro.
/// Se inteiro inicialmente valor 1, capacidade ativada.
/// Se zero, desativada. Outro valor deixa configuração inalterada.
/// Após mudanças, inteiro escrito com 1 ou 0. Se compilado
/// sem -DSQLITE_ALLOW_ROWID_IN_VIEW (usual e recomendado),
/// inteiro sempre preenchido com zero.

/// Opção de thread single
pub const SQLITE_CONFIG_SINGLETHREAD: i32 = 1;

/// Opção de thread multi
pub const SQLITE_CONFIG_MULTITHREAD: i32 = 2;

/// Opção de thread serializado
pub const SQLITE_CONFIG_SERIALIZED: i32 = 3;

/// Opção de métodos de alocação
pub const SQLITE_CONFIG_MALLOC: i32 = 4;

/// Opção de obtenção de métodos de alocação
pub const SQLITE_CONFIG_GETMALLOC: i32 = 5;

/// Opção de scratch descontinuada
pub const SQLITE_CONFIG_SCRATCH: i32 = 6;

/// Opção de cache de página
pub const SQLITE_CONFIG_PAGECACHE: i32 = 7;

/// Opção de heap
pub const SQLITE_CONFIG_HEAP: i32 = 8;

/// Opção de status de memória
pub const SQLITE_CONFIG_MEMSTATUS: i32 = 9;

/// Opção de métodos de mutex
pub const SQLITE_CONFIG_MUTEX: i32 = 10;

/// Opção de obtenção de métodos de mutex
pub const SQLITE_CONFIG_GETMUTEX: i32 = 11;

/// Opção de lookaside (antigo chunk_alloc em desuso)
pub const SQLITE_CONFIG_LOOKASIDE: i32 = 13;

/// Opção de cache de página descontinuada
pub const SQLITE_CONFIG_PCACHE: i32 = 14;

/// Opção de obtenção de cache de página descontinuada
pub const SQLITE_CONFIG_GETPCACHE: i32 = 15;

/// Opção de log
pub const SQLITE_CONFIG_LOG: i32 = 16;

/// Opção de URI
pub const SQLITE_CONFIG_URI: i32 = 17;

/// Opção de cache de página versão 2
pub const SQLITE_CONFIG_PCACHE2: i32 = 18;

/// Opção de obtenção de cache de página versão 2
pub const SQLITE_CONFIG_GETPCACHE2: i32 = 19;

/// Opção de varredura de índice cobridor
pub const SQLITE_CONFIG_COVERING_INDEX_SCAN: i32 = 20;

/// Opção de SQL log
pub const SQLITE_CONFIG_SQLLOG: i32 = 21;

/// Opção de tamanho mmap
pub const SQLITE_CONFIG_MMAP_SIZE: i32 = 22;

/// Opção de tamanho de heap Windows 32
pub const SQLITE_CONFIG_WIN32_HEAPSIZE: i32 = 23;

/// Opção de tamanho de cabeçalho de cache de página
pub const SQLITE_CONFIG_PCACHE_HDRSZ: i32 = 24;

/// Opção de tamanho PMA
pub const SQLITE_CONFIG_PMASZ: i32 = 25;

/// Opção de derrame de journal de instrução
pub const SQLITE_CONFIG_STMTJRNL_SPILL: i32 = 26;

/// Opção de alocação pequena
pub const SQLITE_CONFIG_SMALL_MALLOC: i32 = 27;

/// Opção de tamanho de referência do classificador
pub const SQLITE_CONFIG_SORTERREF_SIZE: i32 = 28;

/// Opção de tamanho máximo de banco de dados em memória
pub const SQLITE_CONFIG_MEMDB_MAXSIZE: i32 = 29;

/// Opção de ROWID em VIEW
pub const SQLITE_CONFIG_ROWID_IN_VIEW: i32 = 30;


// ---- part_006.rs ----

// Verbos de configuração de conexão para sqlite3_db_config(). Os valores
// são os do sqlite3.h do 3.46.1. Cada verbo recebe argumentos próprios (um
// inteiro de habilitação e um local de saída do estado atual, salvo indicação).

/// Muda o nome do esquema "main" da conexão (argumento: texto constante).
pub const SQLITE_DBCONFIG_MAINDBNAME: i32 = 1000;
/// Configura a memória lookaside da conexão (buffer, tamanho do slot, número de slots).
pub const SQLITE_DBCONFIG_LOOKASIDE: i32 = 1001;
/// Habilita ou desabilita a aplicação de chaves estrangeiras.
pub const SQLITE_DBCONFIG_ENABLE_FKEY: i32 = 1002;
/// Habilita ou desabilita gatilhos.
pub const SQLITE_DBCONFIG_ENABLE_TRIGGER: i32 = 1003;
/// Habilita ou desabilita a função fts3_tokenizer().
pub const SQLITE_DBCONFIG_ENABLE_FTS3_TOKENIZER: i32 = 1004;
/// Habilita ou desabilita sqlite3_load_extension() independentemente de load_extension().
pub const SQLITE_DBCONFIG_ENABLE_LOAD_EXTENSION: i32 = 1005;
/// Controla se o fechamento em modo WAL faz checkpoint.
pub const SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE: i32 = 1006;
/// Habilita ou desabilita a garantia de estabilidade do planejador (QPSG).
pub const SQLITE_DBCONFIG_ENABLE_QPSG: i32 = 1007;
/// Controla se EXPLAIN QUERY PLAN mostra as operações dos gatilhos.
pub const SQLITE_DBCONFIG_TRIGGER_EQP: i32 = 1008;
/// Marca o banco para ser zerado no próximo VACUUM.
pub const SQLITE_DBCONFIG_RESET_DATABASE: i32 = 1009;
/// Liga ou desliga o modo defensivo.
pub const SQLITE_DBCONFIG_DEFENSIVE: i32 = 1010;
/// Liga ou desliga o esquema gravável (PRAGMA writable_schema).
pub const SQLITE_DBCONFIG_WRITABLE_SCHEMA: i32 = 1011;
/// Liga ou desliga o comportamento legado de ALTER TABLE RENAME anterior à 3.24.0.
pub const SQLITE_DBCONFIG_LEGACY_ALTER_TABLE: i32 = 1012;
/// Aceita literais de string entre aspas duplas em DML.
pub const SQLITE_DBCONFIG_DQS_DML: i32 = 1013;
/// Aceita literais de string entre aspas duplas em DDL.
pub const SQLITE_DBCONFIG_DQS_DDL: i32 = 1014;
/// Habilita ou desabilita views.
pub const SQLITE_DBCONFIG_ENABLE_VIEW: i32 = 1015;
/// Formato de arquivo legado (versão 1 no cabeçalho de bancos novos).
pub const SQLITE_DBCONFIG_LEGACY_FILE_FORMAT: i32 = 1016;
/// Assume que o esquema é confiável.
pub const SQLITE_DBCONFIG_TRUSTED_SCHEMA: i32 = 1017;
/// Habilita a coleta de sqlite3_stmt_scanstatus_v2().
pub const SQLITE_DBCONFIG_STMT_SCANSTATUS: i32 = 1018;
/// Inverte a ordem padrão de varredura de tabelas e índices.
pub const SQLITE_DBCONFIG_REVERSE_SCANORDER: i32 = 1019;
/// Maior verbo DBCONFIG.
pub const SQLITE_DBCONFIG_MAX: i32 = 1019;

// sqlite3_extended_result_codes é API pública: implementada em `api::extended_result_codes`.


// ---- part_007.rs ----

// Este trecho do sqlite3.h só tem declarações de API pública (sem #define):
//   sqlite3_last_insert_rowid, sqlite3_set_last_insert_rowid, sqlite3_changes,
//   sqlite3_changes64, sqlite3_total_changes, sqlite3_total_changes64,
//   sqlite3_interrupt, sqlite3_is_interrupted, sqlite3_complete,
//   sqlite3_complete16, sqlite3_busy_handler, sqlite3_busy_timeout.
// Pela regra de nomes, as implementações vivem no módulo `api` (main.c e
// complete.c) como `api::last_insert_rowid`, `api::changes` e assim por diante.
// O contrato de assinatura que o integrador deve respeitar:
//   last_insert_rowid(db: &SqliteRef) -> i64
//   set_last_insert_rowid(db: &SqliteRef, rowid: i64)
//   changes(db) -> i32, changes64(db) -> i64, total_changes(db) -> i32, total_changes64(db) -> i64
//   interrupt(db), is_interrupted(db) -> i32
//   complete(sql: &[u8]) -> i32, complete16(sql: &[u8]) -> i32
//   busy_handler(db, Option<Rc<dyn Fn(i32) -> i32>>) -> i32 (o void* vira captura da closure)
//   busy_timeout(db, ms: i32) -> i32


// ---- part_008.rs ----

// Os protótipos sqlite3_get_table, sqlite3_free_table, sqlite3_mprintf,
// sqlite3_malloc, sqlite3_memory_used, sqlite3_randomness e sqlite3_set_authorizer
// são só declarações no cabeçalho: as implementações ficam nos módulos de origem
// (table.c, printf.c, malloc.c, random.c, main.c/auth.c).

/// Códigos de retorno do callback autorizador. O callback deve retornar
/// SQLITE_OK para permitir a ação, SQLITE_IGNORE para desautorizar mas permitir
/// a compilação, ou SQLITE_DENY para rejeitar a declaração SQL com erro.
/// Aborta a declaração SQL com erro.
pub const SQLITE_DENY: i32 = 1;
/// Não permite o acesso, mas não gera erro.
pub const SQLITE_IGNORE: i32 = 2;


// ---- part_009.rs ----

// Códigos de ação do autorizador (sqlite3_set_authorizer). O 3º e o 4º parâmetro do
// callback dependem do código; o 5º é o nome do banco ("main", "temp") e o 6º o
// gatilho ou visão mais interna responsável pelo acesso, ou NULL no SQL de topo.
pub const SQLITE_CREATE_INDEX: i32 = 1;
pub const SQLITE_CREATE_TABLE: i32 = 2;
pub const SQLITE_CREATE_TEMP_INDEX: i32 = 3;
pub const SQLITE_CREATE_TEMP_TABLE: i32 = 4;
pub const SQLITE_CREATE_TEMP_TRIGGER: i32 = 5;
pub const SQLITE_CREATE_TEMP_VIEW: i32 = 6;
pub const SQLITE_CREATE_TRIGGER: i32 = 7;
pub const SQLITE_CREATE_VIEW: i32 = 8;
pub const SQLITE_DELETE: i32 = 9;
pub const SQLITE_DROP_INDEX: i32 = 10;
pub const SQLITE_DROP_TABLE: i32 = 11;
pub const SQLITE_DROP_TEMP_INDEX: i32 = 12;
pub const SQLITE_DROP_TEMP_TABLE: i32 = 13;
pub const SQLITE_DROP_TEMP_TRIGGER: i32 = 14;
pub const SQLITE_DROP_TEMP_VIEW: i32 = 15;
pub const SQLITE_DROP_TRIGGER: i32 = 16;
pub const SQLITE_DROP_VIEW: i32 = 17;
pub const SQLITE_INSERT: i32 = 18;
pub const SQLITE_PRAGMA: i32 = 19;
pub const SQLITE_READ: i32 = 20;
pub const SQLITE_SELECT: i32 = 21;
pub const SQLITE_TRANSACTION: i32 = 22;
pub const SQLITE_UPDATE: i32 = 23;
pub const SQLITE_ATTACH: i32 = 24;
pub const SQLITE_DETACH: i32 = 25;
pub const SQLITE_ALTER_TABLE: i32 = 26;
pub const SQLITE_REINDEX: i32 = 27;
pub const SQLITE_ANALYZE: i32 = 28;
pub const SQLITE_CREATE_VTABLE: i32 = 29;
pub const SQLITE_DROP_VTABLE: i32 = 30;
pub const SQLITE_FUNCTION: i32 = 31;
pub const SQLITE_SAVEPOINT: i32 = 32;
/// Não é mais usado.
pub const SQLITE_COPY: i32 = 0;
pub const SQLITE_RECURSIVE: i32 = 33;

// Códigos de evento de sqlite3_trace_v2(); a máscara M é um OR deles.
pub const SQLITE_TRACE_STMT: u32 = 0x01;
pub const SQLITE_TRACE_PROFILE: u32 = 0x02;
pub const SQLITE_TRACE_ROW: u32 = 0x04;
pub const SQLITE_TRACE_CLOSE: u32 = 0x08;

// Os protótipos sqlite3_trace, sqlite3_profile, sqlite3_trace_v2,
// sqlite3_progress_handler e sqlite3_open* são só declarações no cabeçalho: as
// implementações (api::trace, api::profile, api::trace_v2, api::progress_handler,
// api::open) ficam em main.c e os callbacks viram `Rc<dyn Fn>` lá.


// ---- part_010.rs ----

// Este trecho do cabeçalho só traz protótipos (sqlite3_open, open16, open_v2,
// uri_parameter, uri_boolean, uri_int64, uri_key, filename_database,
// filename_journal, filename_wal, database_file_object, create_filename,
// free_filename) e documentação. Não há constante nem tipo a traduzir: as
// implementações ficam em main.c (api::open, api::open_v2, api::uri_parameter,
// ...), com `sqlite3_filename` modelado como `Vec<u8>` e os parâmetros de URI
// como `&[(Vec<u8>, Vec<u8>)]`, sem ponteiros.


// ---- part_011.rs ----

// sqlite3_stmt é opaco no C e, na implementação, é o próprio `Vdbe` (vdbeInt.h):
// `type Sqlite3Stmt = VdbeRef` fica a cargo do módulo vdbe. Os protótipos
// sqlite3_errcode, sqlite3_extended_errcode, sqlite3_errmsg, sqlite3_errmsg16,
// sqlite3_errstr, sqlite3_error_offset, sqlite3_limit, sqlite3_prepare e
// sqlite3_prepare_v2 são só declarações: as implementações ficam em main.c e
// prepare.c.

// Categorias de limite de sqlite3_limit().
pub const SQLITE_LIMIT_LENGTH: i32 = 0;
pub const SQLITE_LIMIT_SQL_LENGTH: i32 = 1;
pub const SQLITE_LIMIT_COLUMN: i32 = 2;
pub const SQLITE_LIMIT_EXPR_DEPTH: i32 = 3;
pub const SQLITE_LIMIT_COMPOUND_SELECT: i32 = 4;
pub const SQLITE_LIMIT_VDBE_OP: i32 = 5;
pub const SQLITE_LIMIT_FUNCTION_ARG: i32 = 6;
pub const SQLITE_LIMIT_ATTACHED: i32 = 7;
pub const SQLITE_LIMIT_LIKE_PATTERN_LENGTH: i32 = 8;
pub const SQLITE_LIMIT_VARIABLE_NUMBER: i32 = 9;
pub const SQLITE_LIMIT_TRIGGER_DEPTH: i32 = 10;
pub const SQLITE_LIMIT_WORKER_THREADS: i32 = 11;

// Flags de prepare para sqlite3_prepare_v3().
pub const SQLITE_PREPARE_PERSISTENT: u32 = 0x01;
pub const SQLITE_PREPARE_NORMALIZE: u32 = 0x02;
pub const SQLITE_PREPARE_NO_VTAB: u32 = 0x04;


// ---- part_012.rs ----

// Trecho de sqlite3.h: só declarações de protótipos (prepare, prepare_v2, prepare_v3,
// prepare16*, sql, expanded_sql, stmt_readonly, stmt_isexplain, stmt_explain, stmt_busy)
// e os typedefs opacos `sqlite3_value` e `sqlite3_context`. As funções são traduzidas
// onde estão definidas (prepare.c, vdbeaux.c, vdbeapi.c, no módulo `api`); protótipo sem
// corpo não compila em Rust, então nada é emitido aqui. `normalized_sql` fica de fora
// porque SQLITE_ENABLE_NORMALIZE não está ligado no Debian 13.
//
// `sqlite3_value` é o `Mem` (vdbeInt.h) e `sqlite3_context` é o `Sqlite3Context`
// (vdbeInt.h); `sqlite3_stmt` é o `Vdbe`. Os três são tipos de cabeçalhos próprios, e o
// integrador não deve redefini-los aqui.


// ---- part_013.rs ----

// Trecho de sqlite3.h: só protótipos (bind_blob, bind_blob64, bind_double, bind_int,
// bind_int64, bind_null, bind_text, bind_text16, bind_text64, bind_value, bind_pointer,
// bind_zeroblob, bind_zeroblob64, bind_parameter_count, bind_parameter_name,
// bind_parameter_index, clear_bindings, column_count, column_name*, column_database_name*,
// column_table_name*, column_origin_name*, column_decltype* e step). Não há constante nem
// tipo neste trecho. As implementações são traduzidas no módulo `api` (vdbeapi.c, vdbe.c),
// e protótipo sem corpo não compila em Rust, então nada é emitido aqui.


// ---- part_014.rs ----

// Códigos de tipo fundamental do SQLite (sqlite3.h).
pub const SQLITE_INTEGER: i32 = 1;
pub const SQLITE_FLOAT: i32 = 2;
pub const SQLITE_BLOB: i32 = 4;
pub const SQLITE_NULL: i32 = 5;
// No C: `#ifdef SQLITE_TEXT / # undef SQLITE_TEXT / #else / # define SQLITE_TEXT 3`.
// Sem SQLITE_TEXT predefinido, vale 3.
pub const SQLITE_TEXT: i32 = 3;
pub const SQLITE3_TEXT: i32 = 3;

// Os protótipos de data_count, column_blob, column_double, column_int, column_int64,
// column_text, column_text16, column_value, column_bytes, column_bytes16, column_type,
// finalize e reset ficam no módulo `api` (vdbeapi.c). Protótipo sem corpo não compila
// em Rust, então nada é emitido aqui.


// ---- part_015.rs ----

// Codificações de texto (sqlite3.h).
pub const SQLITE_UTF8: i32 = 1; /* IMP: R-37514-35566 */
pub const SQLITE_UTF16LE: i32 = 2; /* IMP: R-03371-37637 */
pub const SQLITE_UTF16BE: i32 = 3; /* IMP: R-51971-34154 */
pub const SQLITE_UTF16: i32 = 4; /* Usa a ordem de bytes nativa */
pub const SQLITE_ANY: i32 = 5; /* Descontinuada */
pub const SQLITE_UTF16_ALIGNED: i32 = 8; /* Só para create_collation */

// Flags de função, combináveis com a codificação preferida no quarto argumento de
// create_function, create_function16 e create_function_v2.
pub const SQLITE_DETERMINISTIC: i32 = 0x000000800;
pub const SQLITE_DIRECTONLY: i32 = 0x000080000;
pub const SQLITE_SUBTYPE: i32 = 0x000100000;
pub const SQLITE_INNOCUOUS: i32 = 0x000200000;
pub const SQLITE_RESULT_SUBTYPE: i32 = 0x001000000;

// Os protótipos de create_function, create_function16, create_function_v2,
// create_window_function e os descontinuados (aggregate_count, expired, transfer_bindings,
// global_recover, thread_cleanup, memory_alarm; SQLITE_OMIT_DEPRECATED não está ligado)
// ficam no módulo `api` (main.c, vdbeapi.c, legacy.c). Protótipo sem corpo não compila em
// Rust, então nada é emitido aqui. Os ponteiros de função xFunc, xStep, xFinal, xValue,
// xInverse e xDestroy viram `Rc<dyn Fn(...)>` no ponto da definição, não aqui.


// ---- part_016.rs ----

// Trecho 016 de sqlite3.h: só comentário de documentação e protótipos da API pública
// (sqlite3_value_*, sqlite3_aggregate_context, sqlite3_user_data, sqlite3_context_db_handle,
// sqlite3_get_auxdata, sqlite3_set_auxdata, sqlite3_get_clientdata, sqlite3_set_clientdata).
// Protótipo não tem corpo nem tipo a declarar: as funções nascem com o corpo em `api::`
// (vdbeapi.c e main.c), com os nomes `api::value_blob`, `api::value_text`, `api::aggregate_context`
// e assim por diante, pela regra de nomes das convenções. Este trecho não define item nenhum.


// ---- part_017.rs ----

/// Destrutor passado a `sqlite3_result_blob`, `sqlite3_result_text` e afins.
/// No C é `void(*)(void*)` com os valores mágicos SQLITE_STATIC (0) e SQLITE_TRANSIENT (-1).
/// Aqui o conteúdo é sempre um `Vec<u8>`/`&[u8]` do dono, então um destrutor próprio não tem o que
/// liberar: os dois valores mágicos bastam e mudam só se o SQLite copia o conteúdo.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Destructor {
    /// Conteúdo constante que nunca muda; não precisa ser copiado nem destruído.
    Static,
    /// O SQLite faz a própria cópia do conteúdo antes de retornar.
    Transient,
}

/// Conteúdo estático, não copiado nem liberado.
pub const SQLITE_STATIC: Destructor = Destructor::Static;

/// O SQLite copia o conteúdo antes de retornar.
pub const SQLITE_TRANSIENT: Destructor = Destructor::Transient;

// Os protótipos do restante do trecho (sqlite3_result_*, sqlite3_create_collation*,
// sqlite3_collation_needed*) não têm corpo: ficam em `api::` (vdbeapi.c e main.c).


// ---- part_018.rs ----

// Protótipos deste trecho (sqlite3_sleep, sqlite3_get_autocommit, sqlite3_db_handle,
// sqlite3_db_name, sqlite3_db_filename, sqlite3_db_readonly, sqlite3_txn_state,
// sqlite3_next_stmt) não têm corpo: ficam em `api::` (main.c e vdbeapi.c).
//
// Somem por não valerem no Debian: sqlite3_activate_cerod (SQLITE_ENABLE_CEROD desligado) e
// sqlite3_win32_set_directory*, SQLITE_WIN32_DATA_DIRECTORY_TYPE e SQLITE_WIN32_TEMP_DIRECTORY_TYPE
// (só Windows).
//
// As variáveis globais sqlite3_temp_directory e sqlite3_data_directory são apenas declaradas aqui;
// a definição é de main.c, onde viram `static` do módulo `main`.

/// Estado de transação: nenhuma transação em andamento.
pub const SQLITE_TXN_NONE: i32 = 0;

/// Estado de transação: transação de leitura em andamento.
pub const SQLITE_TXN_READ: i32 = 1;

/// Estado de transação: transação de escrita em andamento.
pub const SQLITE_TXN_WRITE: i32 = 2;


// ---- part_019.rs ----

// Protótipos deste trecho (sqlite3_commit_hook, sqlite3_rollback_hook, sqlite3_autovacuum_pages,
// sqlite3_update_hook, sqlite3_enable_shared_cache, sqlite3_release_memory,
// sqlite3_db_release_memory, sqlite3_soft_heap_limit64, sqlite3_hard_heap_limit64,
// sqlite3_soft_heap_limit, sqlite3_table_column_metadata, sqlite3_load_extension,
// sqlite3_enable_load_extension, sqlite3_auto_extension, sqlite3_cancel_auto_extension,
// sqlite3_reset_auto_extension) não têm corpo: ficam em `api::` (main.c, loadext.c, ...).
//
// Os typedefs sqlite3_vtab, sqlite3_index_info e sqlite3_vtab_cursor são declarações adiantadas;
// os structs (Sqlite3Vtab, Sqlite3IndexInfo, Sqlite3VtabCursor) são definidos no trecho seguinte.

/// Valor de retorno de um método de módulo: código `SQLITE_*` como no C.
/// `pAux` (`void*`) vira `Option<Rc<dyn Any>>`; `char**` de saída vira `&mut Option<Vec<u8>>`.
pub type VtabConnectFn = fn(
    db: &mut Sqlite3,
    p_aux: &Option<Rc<dyn Any>>,
    argv: &[Vec<u8>],
    pp_vtab: &mut Option<Box<Sqlite3Vtab>>,
    pz_err: &mut Option<Vec<u8>>,
) -> i32;

/// Função SQL de saída de `xFindFunction` (`void (*)(sqlite3_context*,int,sqlite3_value**)`).
pub type VtabSqlFn = fn(ctx: &mut Sqlite3Context, argv: &mut [Sqlite3Value]);

/// Objeto de módulo de tabela virtual (`struct sqlite3_module`).
/// Os métodos ausentes (NULL no C) são `None`.
pub struct Sqlite3Module {
    pub i_version: i32,
    pub x_create: Option<VtabConnectFn>,
    pub x_connect: Option<VtabConnectFn>,
    pub x_best_index: Option<fn(p_vtab: &mut Sqlite3Vtab, info: &mut Sqlite3IndexInfo) -> i32>,
    pub x_disconnect: Option<fn(p_vtab: &mut Sqlite3Vtab) -> i32>,
    pub x_destroy: Option<fn(p_vtab: &mut Sqlite3Vtab) -> i32>,
    pub x_open: Option<
        fn(p_vtab: &mut Sqlite3Vtab, pp_cursor: &mut Option<Box<Sqlite3VtabCursor>>) -> i32,
    >,
    pub x_close: Option<fn(cursor: &mut Sqlite3VtabCursor) -> i32>,
    pub x_filter: Option<
        fn(
            cursor: &mut Sqlite3VtabCursor,
            idx_num: i32,
            idx_str: Option<&[u8]>,
            argv: &mut [Sqlite3Value],
        ) -> i32,
    >,
    pub x_next: Option<fn(cursor: &mut Sqlite3VtabCursor) -> i32>,
    pub x_eof: Option<fn(cursor: &mut Sqlite3VtabCursor) -> i32>,
    pub x_column:
        Option<fn(cursor: &mut Sqlite3VtabCursor, ctx: &mut Sqlite3Context, n: i32) -> i32>,
    pub x_rowid: Option<fn(cursor: &mut Sqlite3VtabCursor, p_rowid: &mut i64) -> i32>,
    pub x_update: Option<
        fn(p_vtab: &mut Sqlite3Vtab, argv: &mut [Sqlite3Value], p_rowid: &mut i64) -> i32,
    >,
    pub x_begin: Option<fn(p_vtab: &mut Sqlite3Vtab) -> i32>,
    pub x_sync: Option<fn(p_vtab: &mut Sqlite3Vtab) -> i32>,
    pub x_commit: Option<fn(p_vtab: &mut Sqlite3Vtab) -> i32>,
    pub x_rollback: Option<fn(p_vtab: &mut Sqlite3Vtab) -> i32>,
    pub x_find_function: Option<
        fn(
            p_vtab: &mut Sqlite3Vtab,
            n_arg: i32,
            z_name: &[u8],
            px_func: &mut Option<VtabSqlFn>,
            pp_arg: &mut Option<Rc<dyn Any>>,
        ) -> i32,
    >,
    pub x_rename: Option<fn(p_vtab: &mut Sqlite3Vtab, z_new: &[u8]) -> i32>,
    // Os métodos acima são da versão 1 do objeto; os de baixo, da versão 2 em diante.
    pub x_savepoint: Option<fn(p_vtab: &mut Sqlite3Vtab, n: i32) -> i32>,
    pub x_release: Option<fn(p_vtab: &mut Sqlite3Vtab, n: i32) -> i32>,
    pub x_rollback_to: Option<fn(p_vtab: &mut Sqlite3Vtab, n: i32) -> i32>,
    // Os de baixo são da versão 3 em diante.
    pub x_shadow_name: Option<fn(z_name: &[u8]) -> i32>,
    // Os de baixo são da versão 4 em diante.
    pub x_integrity: Option<
        fn(
            p_vtab: &mut Sqlite3Vtab,
            z_schema: &[u8],
            z_tab_name: &[u8],
            m_flags: i32,
            pz_err: &mut Option<Vec<u8>>,
        ) -> i32,
    >,
}


// ---- part_020.rs ----

/// Informações de indexação de tabela virtual para xBestIndex.
/// A estrutura e suas subestruturas são usadas como parte da interface de tabela virtual
/// para passar informações para e receber a resposta do método xBestIndex de um módulo de tabela virtual.
pub struct Sqlite3IndexInfo {
    /// Número de entradas em a_constraint (entrada)
    pub n_constraint: i32,
    /// Tabela de restrições da cláusula WHERE (entrada)
    pub a_constraint: Option<Box<[Sqlite3IndexConstraint]>>,
    /// Número de termos na cláusula ORDER BY (entrada)
    pub n_order_by: i32,
    /// A cláusula ORDER BY (entrada)
    pub a_order_by: Option<Box<[Sqlite3IndexOrderBy]>>,
    /// Array de uso de restrição (saída)
    pub a_constraint_usage: Option<Box<[Sqlite3IndexConstraintUsage]>>,
    /// Número usado para identificar o índice (saída)
    pub idx_num: i32,
    /// String possivelmente obtida de sqlite3_malloc (saída)
    pub idx_str: Option<Vec<u8>>,
    /// Liberar idx_str usando sqlite3_free se verdadeiro (saída)
    pub need_to_free_idx_str: i32,
    /// Verdadeiro se a saída já está ordenada (saída)
    pub order_by_consumed: i32,
    /// Custo estimado de usar este índice (saída)
    pub estimated_cost: f64,
    /// Número estimado de linhas retornadas (saída, disponível apenas em SQLite 3.8.2 e posteriores)
    pub estimated_rows: i64,
    /// Máscara de flags SQLITE_INDEX_SCAN_* (saída, disponível apenas em SQLite 3.9.0 e posteriores)
    pub idx_flags: i32,
    /// Máscara de colunas usadas pela instrução (entrada, disponível apenas em SQLite 3.10.0 e posteriores)
    pub col_used: u64,
}

/// Restrição da cláusula WHERE para indexação de tabela virtual.
pub struct Sqlite3IndexConstraint {
    /// Coluna restringida. -1 para ROWID
    pub i_column: i32,
    /// Operador de restrição (SQLITE_INDEX_CONSTRAINT_*)
    pub op: u8,
    /// Verdadeiro se essa restrição é usável
    pub usable: u8,
    /// Usado internamente - xBestIndex deve ignorar
    pub i_term_offset: i32,
}

/// Informações de um termo da cláusula ORDER BY para indexação de tabela virtual.
pub struct Sqlite3IndexOrderBy {
    /// Número da coluna
    pub i_column: i32,
    /// Verdadeiro para DESC. Falso para ASC
    pub desc: u8,
}

/// Uso de restrição para indexação de tabela virtual.
pub struct Sqlite3IndexConstraintUsage {
    /// Se > 0, a restrição é parte de argv para xFilter
    pub argv_index: i32,
    /// Não codificar um teste para essa restrição
    pub omit: u8,
}

/// Máscara de flags para varredura de índice de tabela virtual.
pub const SQLITE_INDEX_SCAN_UNIQUE: i32 = 1; /* Varredura visita no máximo 1 linha */

/// Códigos de operador de restrição de tabela virtual.
pub const SQLITE_INDEX_CONSTRAINT_EQ: i32 = 2;
pub const SQLITE_INDEX_CONSTRAINT_GT: i32 = 4;
pub const SQLITE_INDEX_CONSTRAINT_LE: i32 = 8;
pub const SQLITE_INDEX_CONSTRAINT_LT: i32 = 16;
pub const SQLITE_INDEX_CONSTRAINT_GE: i32 = 32;
pub const SQLITE_INDEX_CONSTRAINT_MATCH: i32 = 64;
pub const SQLITE_INDEX_CONSTRAINT_LIKE: i32 = 65;
pub const SQLITE_INDEX_CONSTRAINT_GLOB: i32 = 66;
pub const SQLITE_INDEX_CONSTRAINT_REGEXP: i32 = 67;
pub const SQLITE_INDEX_CONSTRAINT_NE: i32 = 68;
pub const SQLITE_INDEX_CONSTRAINT_ISNOT: i32 = 69;
pub const SQLITE_INDEX_CONSTRAINT_ISNOTNULL: i32 = 70;
pub const SQLITE_INDEX_CONSTRAINT_ISNULL: i32 = 71;
pub const SQLITE_INDEX_CONSTRAINT_IS: i32 = 72;
pub const SQLITE_INDEX_CONSTRAINT_LIMIT: i32 = 73;
pub const SQLITE_INDEX_CONSTRAINT_OFFSET: i32 = 74;
pub const SQLITE_INDEX_CONSTRAINT_FUNCTION: i32 = 150;

/// Objeto de instância de tabela virtual.
/// Cada implementação de módulo de tabela virtual usa uma subclasse desse objeto
/// para descrever uma instância particular da tabela virtual.
pub struct Sqlite3Vtab {
    /// O módulo para essa tabela virtual
    pub p_module: Option<std::rc::Rc<Sqlite3Module>>,
    /// Número de cursores abertos
    pub n_ref: i32,
    /// Mensagem de erro de sqlite3_mprintf()
    pub z_err_msg: Option<Vec<u8>>,
}

/// Objeto de cursor de tabela virtual.
/// Cada implementação de módulo de tabela virtual usa uma subclasse
/// da seguinte estrutura para descrever cursores que apontam para a tabela virtual
/// e são usados para percorrer a tabela virtual.
pub struct Sqlite3VtabCursor {
    /// Tabela virtual desse cursor
    pub p_vtab: Option<std::rc::Rc<std::cell::RefCell<Sqlite3Vtab>>>,
}

/// Registra uma nova implementação de módulo de tabela virtual.
/// Implementado em vdbeapi.c como parte da API pública sqlite3_create_module.
/// Parâmetros:
///  - db: conexão SQLite para registrar o módulo
///  - z_name: nome do módulo
///  - p: implementação do módulo de tabela virtual
///  - p_client_data: ponteiro de dados do cliente
/// Retorna: código de resultado SQLITE_OK ou erro
// pub fn create_module(
//     db: *mut sqlite3,
//     z_name: *const u8,
//     p: *const Sqlite3Module,
//     p_client_data: *mut ::std::ffi::c_void,
// ) -> i32

/// Registra uma nova implementação de módulo de tabela virtual com destrutor.
/// Implementado em vdbeapi.c como parte da API pública sqlite3_create_module_v2.
/// Parâmetros:
///  - db: conexão SQLite para registrar o módulo
///  - z_name: nome do módulo
///  - p: implementação do módulo de tabela virtual
///  - p_client_data: ponteiro de dados do cliente
///  - x_destroy: função destruidora para p_client_data
/// Retorna: código de resultado SQLITE_OK ou erro
// pub fn create_module_v2(
//     db: *mut sqlite3,
//     z_name: *const u8,
//     p: *const Sqlite3Module,
//     p_client_data: *mut ::std::ffi::c_void,
//     x_destroy: Option<extern "C" fn(*mut ::std::ffi::c_void)>,
// ) -> i32

/// Remove módulos de tabela virtual desnecessários de uma conexão.
/// Implementado em main.c como parte da API pública sqlite3_drop_modules.
/// Parâmetros:
///  - db: conexão para remover módulos
///  - az_keep: array de ponteiros para strings de nomes a manter, terminado por NULL
/// Retorna: código de resultado SQLITE_OK ou erro
// pub fn drop_modules(
//     db: *mut sqlite3,
//     az_keep: *const *const u8,
// ) -> i32

/// Declara o esquema de uma tabela virtual.
/// Implementado em main.c como parte da API pública sqlite3_declare_vtab.
/// Parâmetros:
///  - db: conexão SQLite
///  - z_sql: instrução CREATE TABLE para a tabela virtual
/// Retorna: código de resultado SQLITE_OK ou erro
// pub fn declare_vtab(
//     db: *mut sqlite3,
//     z_sql: *const u8,
// ) -> i32

/// Sobrecarrega uma função para uma tabela virtual.
/// Implementado em main.c como parte da API pública sqlite3_overload_function.
/// Parâmetros:
///  - db: conexão SQLite
///  - z_func_name: nome da função a sobrecarregar
///  - n_arg: número de argumentos
/// Retorna: código de resultado SQLITE_OK ou erro
// pub fn overload_function(
//     db: *mut sqlite3,
//     z_func_name: *const u8,
//     n_arg: i32,
// ) -> i32

// O tipo Sqlite3Blob (typedef sqlite3_blob) é declarado em part_021.


// ---- part_021.rs ----

/// Alça para um BLOB aberto em que pode ser realizada I/O de BLOB incremental.
/// Os campos são definidos em blob.c (struct Incrblob); aqui fica só o nome público.
pub struct Sqlite3Blob;

// Sqlite3Vfs e Sqlite3Mutex são definidos em part_003 (sqlite3_vfs e sqlite3_mutex).

// Assinaturas públicas desta faixa, implementadas nos módulos indicados
// (nomes pela convenção: api::xxx e mutex_xxx):
//   blob_open(db, z_db, z_table, z_column, i_row, flags) -> (i32, Option<BlobRef>)  (blob.c)
//   blob_reopen(blob, i_row) -> i32                                                  (blob.c)
//   blob_close(blob) -> i32                                                          (blob.c)
//   blob_bytes(blob) -> i32                                                          (blob.c)
//   blob_read(blob, z: &mut [u8], n, i_offset) -> i32                                (blob.c)
//   blob_write(blob, z: &[u8], n, i_offset) -> i32                                   (blob.c)
//   vfs_find(z_vfs_name: Option<&[u8]>) -> Option<Rc<RefCell<Sqlite3Vfs>>>           (os.c)
//   vfs_register(vfs, make_dflt) -> i32                                              (os.c)
//   vfs_unregister(vfs) -> i32                                                       (os.c)
//   mutex_alloc(i32) -> Option<Rc<Sqlite3Mutex>>, mutex_free, mutex_enter,
//   mutex_try, mutex_leave                                                           (mutex.c)

/// Métodos de implementação de mutex (sqlite3_mutex_methods).
pub struct Sqlite3MutexMethods {
    /// Inicializa o subsistema de mutexes.
    pub x_mutex_init: Option<fn() -> i32>,
    /// Finaliza o subsistema de mutexes.
    pub x_mutex_end: Option<fn() -> i32>,
    /// Aloca um novo mutex.
    pub x_mutex_alloc: Option<fn(i32) -> Option<std::rc::Rc<Sqlite3Mutex>>>,
    /// Libera um mutex alocado.
    pub x_mutex_free: Option<fn(&std::rc::Rc<Sqlite3Mutex>)>,
    /// Entra em um mutex.
    pub x_mutex_enter: Option<fn(&std::rc::Rc<Sqlite3Mutex>)>,
    /// Tenta entrar em um mutex.
    pub x_mutex_try: Option<fn(&std::rc::Rc<Sqlite3Mutex>) -> i32>,
    /// Sai de um mutex.
    pub x_mutex_leave: Option<fn(&std::rc::Rc<Sqlite3Mutex>)>,
    /// Verifica se um mutex está retido pela thread atual.
    pub x_mutex_held: Option<fn(&std::rc::Rc<Sqlite3Mutex>) -> i32>,
    /// Verifica se um mutex não está retido pela thread atual.
    pub x_mutex_notheld: Option<fn(&std::rc::Rc<Sqlite3Mutex>) -> i32>,
}


// ---- part_022.rs ----

// Assinaturas públicas desta faixa, implementadas nos módulos indicados:
//   mutex_held(Option<&Rc<Sqlite3Mutex>>) -> i32, mutex_notheld(..) -> i32   (mutex.c, sem NDEBUG)
//   db_mutex(db) -> Option<Rc<Sqlite3Mutex>>                                  (main.c)
//   file_control(db, z_db_name: Option<&[u8]>, op, arg) -> i32               (main.c)
//   test_control(op, args) -> i32                                             (main.c)
//   keyword_count() -> i32, keyword_name(n) -> Option<&'static [u8]>,
//   keyword_check(z: &[u8]) -> i32                                            (tokenize.c)
//   str_new(db), str_finish(Sqlite3Str) -> Option<Vec<u8>>, str_appendf,
//   str_vappendf, str_append, str_appendall, str_appendchar, str_reset        (printf.c)

// ===== Tipos de mutex (sqlite3_mutex_alloc) =====

pub const SQLITE_MUTEX_FAST: i32 = 0;
pub const SQLITE_MUTEX_RECURSIVE: i32 = 1;
pub const SQLITE_MUTEX_STATIC_MAIN: i32 = 2;
/// sqlite3_malloc()
pub const SQLITE_MUTEX_STATIC_MEM: i32 = 3;
/// NÃO USADO
pub const SQLITE_MUTEX_STATIC_MEM2: i32 = 4;
/// sqlite3BtreeOpen()
pub const SQLITE_MUTEX_STATIC_OPEN: i32 = 4;
/// sqlite3_randomness()
pub const SQLITE_MUTEX_STATIC_PRNG: i32 = 5;
/// lista LRU de páginas
pub const SQLITE_MUTEX_STATIC_LRU: i32 = 6;
/// NÃO USADO
pub const SQLITE_MUTEX_STATIC_LRU2: i32 = 7;
/// sqlite3PageMalloc()
pub const SQLITE_MUTEX_STATIC_PMEM: i32 = 7;
/// Para uso da aplicação
pub const SQLITE_MUTEX_STATIC_APP1: i32 = 8;
/// Para uso da aplicação
pub const SQLITE_MUTEX_STATIC_APP2: i32 = 9;
/// Para uso da aplicação
pub const SQLITE_MUTEX_STATIC_APP3: i32 = 10;
/// Para uso do VFS embutido
pub const SQLITE_MUTEX_STATIC_VFS1: i32 = 11;
/// Para uso do VFS de extensão
pub const SQLITE_MUTEX_STATIC_VFS2: i32 = 12;
/// Para uso do VFS da aplicação
pub const SQLITE_MUTEX_STATIC_VFS3: i32 = 13;

/// Compatibilidade legada.
pub const SQLITE_MUTEX_STATIC_MASTER: i32 = 2;

// ===== Códigos de operação de sqlite3_test_control =====

pub const SQLITE_TESTCTRL_FIRST: i32 = 5;
pub const SQLITE_TESTCTRL_PRNG_SAVE: i32 = 5;
pub const SQLITE_TESTCTRL_PRNG_RESTORE: i32 = 6;
/// NÃO USADO
pub const SQLITE_TESTCTRL_PRNG_RESET: i32 = 7;
pub const SQLITE_TESTCTRL_FK_NO_ACTION: i32 = 7;
pub const SQLITE_TESTCTRL_BITVEC_TEST: i32 = 8;
pub const SQLITE_TESTCTRL_FAULT_INSTALL: i32 = 9;
pub const SQLITE_TESTCTRL_BENIGN_MALLOC_HOOKS: i32 = 10;
pub const SQLITE_TESTCTRL_PENDING_BYTE: i32 = 11;
pub const SQLITE_TESTCTRL_ASSERT: i32 = 12;
pub const SQLITE_TESTCTRL_ALWAYS: i32 = 13;
/// NÃO USADO
pub const SQLITE_TESTCTRL_RESERVE: i32 = 14;
pub const SQLITE_TESTCTRL_JSON_SELFCHECK: i32 = 14;
pub const SQLITE_TESTCTRL_OPTIMIZATIONS: i32 = 15;
/// NÃO USADO
pub const SQLITE_TESTCTRL_ISKEYWORD: i32 = 16;
/// NÃO USADO
pub const SQLITE_TESTCTRL_SCRATCHMALLOC: i32 = 17;
pub const SQLITE_TESTCTRL_INTERNAL_FUNCTIONS: i32 = 17;
pub const SQLITE_TESTCTRL_LOCALTIME_FAULT: i32 = 18;
/// NÃO USADO
pub const SQLITE_TESTCTRL_EXPLAIN_STMT: i32 = 19;
pub const SQLITE_TESTCTRL_ONCE_RESET_THRESHOLD: i32 = 19;
pub const SQLITE_TESTCTRL_NEVER_CORRUPT: i32 = 20;
pub const SQLITE_TESTCTRL_VDBE_COVERAGE: i32 = 21;
pub const SQLITE_TESTCTRL_BYTEORDER: i32 = 22;
pub const SQLITE_TESTCTRL_ISINIT: i32 = 23;
pub const SQLITE_TESTCTRL_SORTER_MMAP: i32 = 24;
pub const SQLITE_TESTCTRL_IMPOSTER: i32 = 25;
pub const SQLITE_TESTCTRL_PARSER_COVERAGE: i32 = 26;
pub const SQLITE_TESTCTRL_RESULT_INTREAL: i32 = 27;
pub const SQLITE_TESTCTRL_PRNG_SEED: i32 = 28;
pub const SQLITE_TESTCTRL_EXTRA_SCHEMA_CHECKS: i32 = 29;
pub const SQLITE_TESTCTRL_SEEK_COUNT: i32 = 30;
pub const SQLITE_TESTCTRL_TRACEFLAGS: i32 = 31;
pub const SQLITE_TESTCTRL_TUNE: i32 = 32;
pub const SQLITE_TESTCTRL_LOGEST: i32 = 33;
pub const SQLITE_TESTCTRL_USELONGDOUBLE: i32 = 34;
/// Maior TESTCTRL
pub const SQLITE_TESTCTRL_LAST: i32 = 34;

// O tipo Sqlite3Str (typedef sqlite3_str) é o de sqliteInt_h (apelidado StrAccum).


// ---- part_023.rs ----

// Sqlite3Str (sqlite3_str), Sqlite3 e Sqlite3Stmt são definidos em sqliteInt_h e vdbeInt_h.
// Assinaturas públicas desta faixa, implementadas nos módulos indicados:
//   str_errcode(&Sqlite3Str) -> i32, str_length(&Sqlite3Str) -> i32,
//   str_value(&Sqlite3Str) -> &[u8]                                 (printf.c)
//   status(op, &mut cur, &mut hiwtr, reset_flag) -> i32            (status.c)
//   status64(op, &mut cur, &mut hiwtr, reset_flag) -> i32          (status.c)
//   db_status(db, op, &mut cur, &mut hiwtr, reset_flg) -> i32      (main.c)
//   stmt_status(stmt, op, reset_flg) -> i32                        (vdbeapi.c)

// ===== Parâmetros de status global (SQLITE_STATUS_*) =====

pub const SQLITE_STATUS_MEMORY_USED: i32 = 0;
pub const SQLITE_STATUS_PAGECACHE_USED: i32 = 1;
pub const SQLITE_STATUS_PAGECACHE_OVERFLOW: i32 = 2;
pub const SQLITE_STATUS_SCRATCH_USED: i32 = 3;
pub const SQLITE_STATUS_SCRATCH_OVERFLOW: i32 = 4;
pub const SQLITE_STATUS_MALLOC_SIZE: i32 = 5;
pub const SQLITE_STATUS_PARSER_STACK: i32 = 6;
pub const SQLITE_STATUS_PAGECACHE_SIZE: i32 = 7;
pub const SQLITE_STATUS_SCRATCH_SIZE: i32 = 8;
pub const SQLITE_STATUS_MALLOC_COUNT: i32 = 9;

// ===== Parâmetros de status para conexão de banco (SQLITE_DBSTATUS_*) =====

pub const SQLITE_DBSTATUS_LOOKASIDE_USED: i32 = 0;
pub const SQLITE_DBSTATUS_CACHE_USED: i32 = 1;
pub const SQLITE_DBSTATUS_SCHEMA_USED: i32 = 2;
pub const SQLITE_DBSTATUS_STMT_USED: i32 = 3;
pub const SQLITE_DBSTATUS_LOOKASIDE_HIT: i32 = 4;
pub const SQLITE_DBSTATUS_LOOKASIDE_MISS_SIZE: i32 = 5;
pub const SQLITE_DBSTATUS_LOOKASIDE_MISS_FULL: i32 = 6;
pub const SQLITE_DBSTATUS_CACHE_HIT: i32 = 7;
pub const SQLITE_DBSTATUS_CACHE_MISS: i32 = 8;
pub const SQLITE_DBSTATUS_CACHE_WRITE: i32 = 9;
pub const SQLITE_DBSTATUS_DEFERRED_FKS: i32 = 10;
pub const SQLITE_DBSTATUS_CACHE_USED_SHARED: i32 = 11;
pub const SQLITE_DBSTATUS_CACHE_SPILL: i32 = 12;
/// Maior DBSTATUS definido.
pub const SQLITE_DBSTATUS_MAX: i32 = 12;

// ===== Parâmetros de status para prepared statement (SQLITE_STMTSTATUS_*) =====

pub const SQLITE_STMTSTATUS_FULLSCAN_STEP: i32 = 1;
pub const SQLITE_STMTSTATUS_SORT: i32 = 2;
pub const SQLITE_STMTSTATUS_AUTOINDEX: i32 = 3;
pub const SQLITE_STMTSTATUS_VM_STEP: i32 = 4;
pub const SQLITE_STMTSTATUS_REPREPARE: i32 = 5;
pub const SQLITE_STMTSTATUS_RUN: i32 = 6;
pub const SQLITE_STMTSTATUS_FILTER_MISS: i32 = 7;
pub const SQLITE_STMTSTATUS_FILTER_HIT: i32 = 8;
pub const SQLITE_STMTSTATUS_MEMUSED: i32 = 99;

// ===== Cache de página customizado =====

/// Tipo opaco sqlite3_pcache, implementado pelo módulo plugável. O núcleo só o
/// mantém e o repassa; aqui ele é o contrato de um cache de página.
pub trait Sqlite3Pcache {}

/// Uma única página no cache de página (sqlite3_pcache_page).
pub struct Sqlite3PcachePage {
    /// Conteúdo da página (pBuf)
    pub p_buf: Vec<u8>,
    /// Informação extra associada à página (pExtra)
    pub p_extra: Vec<u8>,
}


// ---- part_024.rs ----

/// Estrutura que define um cache de página customizável.
///
/// A interface [sqlite3_config]([SQLITE_CONFIG_PCACHE2], ...) pode registrar
/// uma implementação alternativa de cache de página passando uma instância
/// dessa estrutura. Em muitas aplicações, a maior parte da memória alocada
/// pelo SQLite é usada para o cache de página.
///
/// Implementando um cache de página customizado usando essa API, uma aplicação
/// pode controlar melhor a quantidade de memória consumida pelo SQLite, a forma
/// como essa memória é alocada e liberada, e as políticas usadas para determinar
/// exatamente quais partes do arquivo de banco de dados são cacheadas e por quanto tempo.
///
/// O mecanismo alternativo de cache de página é uma medida extrema, necessária
/// apenas para as aplicações mais exigentes. O cache de página embutido é recomendado
/// para a maioria dos usos.
///
/// O conteúdo da estrutura sqlite3_pcache_methods2 é copiado para um buffer
/// interno pelo SQLite na chamada para [sqlite3_config]. Por isso a aplicação
/// pode descartar o parâmetro após a chamada a [sqlite3_config()] retornar.
///
/// **Assinatura xInit():**
/// O método xInit() é chamado uma vez a cada chamada efetiva de [sqlite3_initialize()]
/// (normalmente apenas uma vez durante a vida do processo). O método xInit()
/// recebe uma cópia do valor de sqlite3_pcache_methods2.pArg.
/// A intenção do método xInit() é configurar estruturas de dados globais exigidas
/// pela implementação de cache de página customizado.
/// Se xInit() for NULL, então o cache de página padrão embutido é usado
/// em lugar do cache de página definido pela aplicação.
///
/// **Assinatura xShutdown():**
/// O método xShutdown() é chamado por [sqlite3_shutdown()]. Pode ser usado para
/// limpar quaisquer recursos pendentes antes do encerramento do processo, se necessário.
/// O método xShutdown() pode ser NULL.
///
/// SQLite serializa automaticamente chamadas ao método xInit(), então xInit não
/// precisa ser thread-safe. O método xShutdown() é chamado apenas de [sqlite3_shutdown()]
/// então também não precisa ser thread-safe. Todos os outros métodos devem ser
/// thread-safe em aplicações multithreaded.
///
/// SQLite nunca invocará xInit() mais de uma vez sem uma chamada intermediária a xShutdown().
///
/// **Assinatura xCreate():**
/// SQLite invoca o método xCreate() para construir uma nova instância de cache.
/// SQLite normalmente cria uma instância de cache para cada arquivo de banco de dados aberto,
/// embora isso não seja garantido. O primeiro parâmetro, szPage, é o tamanho em bytes
/// das páginas que devem ser alocadas pelo cache. szPage sempre será uma potência de dois.
/// O segundo parâmetro szExtra é um número de bytes de armazenamento extra associado
/// a cada entrada do cache de página. O parâmetro szExtra será um número menor que 250.
/// SQLite usará os szExtra bytes extras em cada página para armazenar metadados sobre a
/// página de banco de dados subjacente no disco. O valor passado para szExtra depende da
/// versão do SQLite, da plataforma de destino e de como o SQLite foi compilado.
/// O terceiro argumento para xCreate(), bPurgeable, é verdadeiro se o cache sendo criado
/// será usado para cachear páginas de banco de dados de um arquivo armazenado em disco,
/// ou falso se for usado para um banco de dados em memória. A implementação de cache não
/// precisa fazer nada especial baseado no valor de bPurgeable; é puramente consultivo.
/// Em um cache onde bPurgeable é falso, SQLite nunca invocará xUnpin() exceto para
/// deletar propositalmente uma página. Em outras palavras, chamadas a xUnpin() em um
/// cache com bPurgeable definido como falso sempre terão a bandeira "discard" definida
/// como verdadeira. Por isso, um cache criado com bPurgeable falso nunca conterá quaisquer
/// páginas desafixadas.
///
/// **Assinatura xCachesize():**
/// O método xCachesize() pode ser chamado a qualquer momento pelo SQLite para definir
/// o tamanho de cache máximo sugerido (número de páginas armazenadas por) da instância
/// de cache passada como primeiro argumento. Este é o valor configurado usando o comando
/// SQLite "[PRAGMA cache_size]". Como com o parâmetro bPurgeable, a implementação não
/// é obrigada a fazer nada com esse valor; é consultivo apenas.
///
/// **Assinatura xPagecount():**
/// O método xPagecount() deve retornar o número de páginas atualmente armazenadas
/// no cache, tanto fixadas quanto desafixadas.
///
/// **Assinatura xFetch():**
/// O método xFetch() localiza uma página no cache e retorna um ponteiro para um objeto
/// sqlite3_pcache_page associado a essa página, ou um ponteiro NULL.
/// O elemento pBuf do objeto sqlite3_pcache_page retornado será um ponteiro para um
/// buffer de szPage bytes usado para armazenar o conteúdo de uma única página de banco
/// de dados. O elemento pExtra de sqlite3_pcache_page será um ponteiro para os szExtra
/// bytes de armazenamento extra que SQLite solicitou para cada entrada no cache de página.
///
/// A página a ser buscada é determinada pela chave. O valor mínimo de chave é 1.
/// Após ser recuperada usando xFetch, a página é considerada "fixada".
///
/// Se a página solicitada já estiver no cache de página, então a implementação de
/// cache de página deve retornar um ponteiro para o buffer de página com seu conteúdo
/// intacto. Se a página solicitada não estiver no cache, então a implementação de cache
/// deve usar o valor do parâmetro createFlag para ajudá-la a determinar que ação tomar.
///
/// createFlag=0: não aloca uma página nova. Retorna NULL.
/// createFlag=1: aloca uma página nova se for fácil e conveniente. Senão retorna NULL.
/// createFlag=2: faz todo esforço para alocar uma página nova. Retorna NULL apenas se
///              alocar uma página nova for efetivamente impossível.
///
/// SQLite normalmente invocará xFetch() com um createFlag de 0 ou 1. SQLite usará apenas
/// um createFlag de 2 após uma chamada anterior com createFlag de 1 ter falhado.
/// Entre as chamadas xFetch(), SQLite pode tentar desafixar uma ou mais páginas do cache
/// derramando o conteúdo de páginas fixadas para disco e sincronizando o cache de disco
/// do sistema operacional.
///
/// **Assinatura xUnpin():**
/// xUnpin() é chamado pelo SQLite com um ponteiro para uma página atualmente fixada
/// como seu segundo argumento. Se o terceiro parâmetro, discard, for diferente de zero,
/// então a página deve ser removida do cache. Se o parâmetro discard for zero, então a
/// página pode ser descartada ou retida à discrição da implementação de cache de página.
/// A implementação de cache de página pode escolher remover páginas desafixadas a qualquer
/// momento.
///
/// O cache não deve realizar qualquer contagem de referência. Uma única chamada a xUnpin()
/// desafixará a página independentemente do número de chamadas anteriores a xFetch().
///
/// **Assinatura xRekey():**
/// O método xRekey() é usado para alterar o valor de chave associado à página passada
/// como segundo argumento. Se o cache anteriormente continha uma entrada associada a
/// newKey, deve ser descartada. Qualquer entrada de cache anterior associada a newKey
/// é garantida não estar fixada.
///
/// Quando SQLite chama o método xTruncate(), o cache deve descartar todas as entradas
/// de cache existentes com números de página (chaves) maiores ou iguais ao valor do
/// parâmetro iLimit passado a xTruncate(). Se qualquer dessas páginas estiver fixada,
/// elas estão implicitamente desafixadas, significando que podem ser seguramente descartadas.
///
/// **Assinatura xDestroy():**
/// O método xDestroy() é usado para deletar um cache alocado por xCreate().
/// Todos os recursos associados ao cache especificado devem ser liberados. Após chamar
/// o método xDestroy(), SQLite considera o handle [sqlite3_pcache*] inválido e não o
/// usará com nenhuma outra função sqlite3_pcache_methods2.
///
/// **Assinatura xShrink():**
/// SQLite invoca o método xShrink() quando quer que o cache de página libere o máximo
/// possível de memória heap. A implementação de cache de página não é obrigada a liberar
/// qualquer memória, mas implementações bem-comportadas devem fazer o seu melhor.

pub type PcacheInit = fn(usize) -> i32;
pub type PcacheShutdown = fn(usize);
pub type PcacheCreate = fn(i32, i32, i32) -> usize;
pub type PcacheCachesize = fn(usize, i32);
pub type PcachePagecount = fn(usize) -> i32;
pub type PcacheFetch = fn(usize, u32, i32) -> usize;
pub type PcacheUnpin = fn(usize, usize, i32);
pub type PcacheRekey = fn(usize, usize, u32, u32);
pub type PcacheTruncate = fn(usize, u32);
pub type PcacheDestroy = fn(usize);
pub type PcacheShrink = fn(usize);

pub struct Sqlite3PcacheMethods2 {
    pub i_version: i32,
    pub p_arg: usize,
    pub x_init: Option<PcacheInit>,
    pub x_shutdown: Option<PcacheShutdown>,
    pub x_create: Option<PcacheCreate>,
    pub x_cachesize: Option<PcacheCachesize>,
    pub x_pagecount: Option<PcachePagecount>,
    pub x_fetch: Option<PcacheFetch>,
    pub x_unpin: Option<PcacheUnpin>,
    pub x_rekey: Option<PcacheRekey>,
    pub x_truncate: Option<PcacheTruncate>,
    pub x_destroy: Option<PcacheDestroy>,
    pub x_shrink: Option<PcacheShrink>,
}

/// Estrutura obsoleta de métodos de cache de página que foi substituída por
/// sqlite3_pcache_methods2. Esse objeto não é usado pelo SQLite. É mantido no
/// arquivo de cabeçalho apenas para compatibilidade com versões anteriores.

pub type PcacheInitObsolete = fn(usize) -> i32;
pub type PcacheShutdownObsolete = fn(usize);
pub type PcacheCreateObsolete = fn(i32, i32) -> usize;
pub type PcacheCachesizeObsolete = fn(usize, i32);
pub type PcachePagecountObsolete = fn(usize) -> i32;
pub type PcacheFetchObsolete = fn(usize, u32, i32) -> usize;
pub type PcacheUnpinObsolete = fn(usize, usize, i32);
pub type PcacheRekeyObsolete = fn(usize, usize, u32, u32);
pub type PcacheTruncateObsolete = fn(usize, u32);
pub type PcacheDestroyObsolete = fn(usize);

pub struct Sqlite3PcacheMethods {
    pub p_arg: usize,
    pub x_init: Option<PcacheInitObsolete>,
    pub x_shutdown: Option<PcacheShutdownObsolete>,
    pub x_create: Option<PcacheCreateObsolete>,
    pub x_cachesize: Option<PcacheCachesizeObsolete>,
    pub x_pagecount: Option<PcachePagecountObsolete>,
    pub x_fetch: Option<PcacheFetchObsolete>,
    pub x_unpin: Option<PcacheUnpinObsolete>,
    pub x_rekey: Option<PcacheRekeyObsolete>,
    pub x_truncate: Option<PcacheTruncateObsolete>,
    pub x_destroy: Option<PcacheDestroyObsolete>,
}

/// Estrutura que registra informações de estado sobre uma operação contínua de backup.
/// O objeto sqlite3_backup é criado por uma chamada a [sqlite3_backup_init()] e é
/// destruído por uma chamada a [sqlite3_backup_finish()].
///
/// Ver também: [Usando a API de Backup Online do SQLite]

pub struct Sqlite3Backup {
    _private: (),
}

// API de Backup Online.
//
// A API de backup copia o conteúdo de um banco de dados para outro. É útil para
// criar backups de bancos de dados ou para copiar bancos de dados em memória para
// ou a partir de arquivos persistentes.
//
// Ver também: [Usando a API de Backup Online do SQLite]
//
// SQLite mantém uma transação de escrita aberta no arquivo de banco de dados de
// destino durante a duração da operação de backup. O banco de dados de origem é
// bloqueado apenas enquanto está sendo lido; não é bloqueado continuamente durante
// toda a operação de backup. Portanto, o backup pode ser realizado em um banco de
// dados de origem ao vivo sem impedir que outras conexões de banco de dados leiam
// ou escrevam no banco de dados de origem enquanto o backup está em andamento.
//
// Para realizar uma operação de backup:
// 1. [sqlite3_backup_init()] é chamado uma vez para inicializar o backup.
// 2. [sqlite3_backup_step()] é chamado uma ou mais vezes para transferir os dados entre os dois bancos de dados.
// 3. [sqlite3_backup_finish()] é chamado para liberar todos os recursos associados à operação de backup.
//
// Deve haver exatamente uma chamada a sqlite3_backup_finish() para cada chamada bem-sucedida
// a sqlite3_backup_init().
//
// **sqlite3_backup_init():**
//
// Os argumentos D e N para sqlite3_backup_init(D,N,S,M) são a [conexão de banco de dados]
// associada ao banco de dados de destino e o nome do banco de dados, respectivamente.
// O nome do banco de dados é "main" para o banco de dados principal, "temp" para o
// banco de dados temporário, ou o nome especificado após a palavra-chave AS em uma
// declaração [ATTACH] para um banco de dados anexado.
// Os argumentos S e M passados a sqlite3_backup_init(D,N,S,M) identificam a
// [conexão de banco de dados] e o nome do banco de dados de origem, respectivamente.
// As [conexões de banco de dados] de origem e destino (parâmetros S e D) devem ser
// diferentes ou sqlite3_backup_init(D,N,S,M) falhará com um erro.
//
// Uma chamada a sqlite3_backup_init() falhará, retornando NULL, se já houver uma
// transação de leitura ou leitura e escrita aberta no banco de dados de destino.
//
// Se um erro ocorrer dentro de sqlite3_backup_init(D,N,S,M), então NULL é retornado
// e um código de erro e mensagem de erro são armazenados na [conexão de banco de dados]
// de destino D. O código de erro e a mensagem para a chamada falhada a sqlite3_backup_init()
// podem ser recuperados usando as funções [sqlite3_errcode()], [sqlite3_errmsg()],
// e/ou [sqlite3_errmsg16()]. Uma chamada bem-sucedida a sqlite3_backup_init() retorna
// um ponteiro para um objeto [sqlite3_backup]. O objeto [sqlite3_backup] pode ser usado
// com as funções sqlite3_backup_step() e sqlite3_backup_finish() para realizar a operação
// de backup especificada.
//
// **sqlite3_backup_step():**
//
// A função sqlite3_backup_step(B,N) copiará até N páginas entre os bancos de dados de
// origem e destino especificados pelo objeto [sqlite3_backup] B. Se N for negativo,
// todas as páginas de origem restantes são copiadas. Se sqlite3_backup_step(B,N)
// copiar com sucesso N páginas e ainda houver mais páginas a serem copiadas, então a
// função retorna [SQLITE_OK]. Se sqlite3_backup_step(B,N) terminar com sucesso de copiar
// todas as páginas de origem para destino, então retorna [SQLITE_DONE]. Se um erro
// ocorrer ao executar sqlite3_backup_step(B,N), então um [código de erro] é retornado.
// Além de [SQLITE_OK] e [SQLITE_DONE], uma chamada a sqlite3_backup_step() pode retornar
// [SQLITE_READONLY], [SQLITE_NOMEM], [SQLITE_BUSY], [SQLITE_LOCKED], ou um código de
// erro estendido [SQLITE_IOERR_ACCESS | SQLITE_IOERR_XXX].
//
// O sqlite3_backup_step() pode retornar [SQLITE_READONLY] se:
// 1. O banco de dados de destino foi aberto como somente leitura.
// 2. O banco de dados de destino está usando journaling em write-ahead-log (WAL) e os tamanhos
//    de página de origem e destino diferem.
// 3. O banco de dados de destino é um banco de dados em memória e os tamanhos de página
//    de origem e destino diferem.
//
// Se sqlite3_backup_step() não conseguir obter um bloqueio de sistema de arquivos necessário,
// então a função [sqlite3_busy_handler | manipulador ocupado] é invocada (se uma for
// especificada). Se o manipulador ocupado retornar diferente de zero antes do bloqueio
// estar disponível, então [SQLITE_BUSY] é retornado ao chamador. Neste caso a chamada
// a sqlite3_backup_step() pode ser tentada novamente mais tarde. Se a [conexão de banco
// de dados] de origem estiver sendo usada para escrever no banco de dados de origem quando
// sqlite3_backup_step() é chamado, então [SQLITE_LOCKED] é retornado imediatamente.
// Novamente, neste caso a chamada a sqlite3_backup_step() pode ser tentada novamente mais
// tarde. Se [SQLITE_IOERR_ACCESS | SQLITE_IOERR_XXX], [SQLITE_NOMEM], ou [SQLITE_READONLY]
// for retornado, então não há sentido em tentar novamente a chamada a sqlite3_backup_step().
// Esses erros são considerados fatais. A aplicação deve aceitar que a operação de backup
// falhou e passar o handle da operação de backup para sqlite3_backup_finish() para liberar
// recursos associados.
//
// A primeira chamada a sqlite3_backup_step() obtém um bloqueio exclusivo no arquivo de
// destino. O bloqueio exclusivo não é liberado até que sqlite3_backup_finish() seja chamado
// ou a operação de backup seja concluída e sqlite3_backup_step() retorne [SQLITE_DONE].
// Cada chamada a sqlite3_backup_step() obtém um [bloqueio compartilhado] no banco de dados
// de origem que dura durante a chamada de sqlite3_backup_step(). Como o banco de dados de
// origem não é bloqueado entre chamadas a sqlite3_backup_step(), o banco de dados de origem
// pode ser modificado durante o processo de backup. Se o banco de dados de origem for
// modificado por um processo externo ou através de uma conexão de banco de dados diferente
// daquela sendo usada pela operação de backup, então o backup será automaticamente reiniciado
// pela próxima chamada a sqlite3_backup_step(). Se o banco de dados de origem for modificado
// usando a mesma conexão de banco de dados que é usada pela operação de backup, então o banco
// de dados de backup é automaticamente atualizado ao mesmo tempo.
//
// **sqlite3_backup_finish():**
//
// Quando sqlite3_backup_step() retornou [SQLITE_DONE], ou quando a aplicação deseja
// abandonar a operação de backup, a aplicação deve destruir o [sqlite3_backup] passando-o
// para sqlite3_backup_finish(). A interface sqlite3_backup_finish() libera todos os recursos
// associados ao objeto [sqlite3_backup]. Se sqlite3_backup_step() ainda não retornou
// [SQLITE_DONE], então qualquer transação de escrita ativa no banco de dados de destino
// é revertida. O objeto [sqlite3_backup] é inválido e pode não ser usado após uma chamada
// a sqlite3_backup_finish().
//
// O valor retornado por sqlite3_backup_finish é [SQLITE_OK] se nenhum erro
// sqlite3_backup_step() ocorreu, independentemente de sqlite3_backup_step() ter sido
// concluído ou não. Se uma condição de falta de memória ou erro de E/S ocorreu durante
// qualquer chamada anterior a sqlite3_backup_step() no mesmo objeto [sqlite3_backup],
// então sqlite3_backup_finish() retorna o código de erro correspondente.
//
// Um retorno de [SQLITE_BUSY] ou [SQLITE_LOCKED] de sqlite3_backup_step() não é um
// erro permanente e não afeta o valor de retorno de sqlite3_backup_finish().


// ---- part_025.rs ----

// As declarações `SQLITE_API` de sqlite3.h não geram código: as implementações
// vivem em backup.c, main.c, util.c, printf.c e global.c, e saem de lá com os
// nomes da convenção (`api::backup_init`, `api::backup_step`,
// `api::backup_finish`, `api::backup_remaining`, `api::backup_pagecount`,
// `api::unlock_notify`, `api::stricmp`, `api::strnicmp`, `api::strglob`,
// `api::strlike`, `api::log`, `api::wal_hook`, `api::wal_autocheckpoint`,
// `api::wal_checkpoint`).

/// Callback de `sqlite3_unlock_notify` (`xNotify`): recebe o vetor de contextos
/// das conexões desbloqueadas (o `void **apArg` do C, com `nArg` implícito no
/// tamanho da fatia).
pub type UnlockNotifyCallback = Rc<dyn Fn(&[usize])>;

/// Callback de `sqlite3_wal_hook`: contexto registrado, conexão, nome do banco
/// escrito ("main" ou o nome de um ATTACH) e número de páginas no log WAL.
/// Retorna um código de resultado (normalmente SQLITE_OK).
pub type WalHookCallback = Rc<dyn Fn(&DbRef, &[u8], i32) -> i32>;


// ---- part_026.rs ----

/// Executa uma operação de checkpoint em um banco de dados WAL.
/// Realiza checkpoint em modo especificado, retornando status em contadores de frames.
/// Modos: PASSIVE (sem bloqueio), FULL (espera escritores), RESTART (espera leitores),
/// TRUNCATE (trunca o arquivo WAL a zero bytes).
/// Retorna SQLITE_OK, SQLITE_BUSY se bloqueado por outro checkpoint, ou SQLITE_ERROR.
// pub fn wal_checkpoint_v2(
//     db: &mut Sqlite3,
//     z_db: Option<&[u8]>,
//     e_mode: i32,
//     pn_log: &mut i32,
//     pn_ckpt: &mut i32,
// ) -> i32

/// Modo de checkpoint passivo: realiza quantos frames forem possíveis sem bloqueio.
/// O callback de busy nunca é invocado. Pode deixar checkpoint incompleto com leitores/escritores.
pub const SQLITE_CHECKPOINT_PASSIVE: i32 = 0;

/// Modo de checkpoint completo: bloqueia até não haver escritores e leitores lerem snapshot mais recente.
/// Realiza checkpoint de todos os frames e sincroniza o arquivo. Bloqueia novos escritores.
pub const SQLITE_CHECKPOINT_FULL: i32 = 1;

/// Modo de checkpoint restart: igual a FULL, mas aguarda leitores lerem apenas do arquivo de banco.
/// Garante que o próximo escritor reinicie o arquivo de log.
pub const SQLITE_CHECKPOINT_RESTART: i32 = 2;

/// Modo de checkpoint truncate: igual a RESTART, mas trunca o arquivo de log a zero bytes.
/// Após sucesso, pnLog e pnCkpt são ambos definidos como zero.
pub const SQLITE_CHECKPOINT_TRUNCATE: i32 = 3;

/// Configura aspectos de uma implementação de tabela virtual.
/// Chamada no contexto de xConnect ou xCreate. Fora desse contexto, comportamento indefinido.
/// Op determina qual opção de configuração é usada.
// pub fn vtab_config(db: &mut Sqlite3, op: i32, ...) -> i32

/// Determina a política de conflito (ON CONFLICT) para uma tabela virtual.
/// Chamada apenas de dentro de xUpdate para INSERT ou UPDATE.
/// Retorna SQLITE_ROLLBACK, SQLITE_IGNORE, SQLITE_FAIL, SQLITE_ABORT ou SQLITE_REPLACE.
// pub fn vtab_on_conflict(db: &Sqlite3) -> i32

/// Determina se acesso a coluna de tabela virtual é para UPDATE sem alteração.
/// Chamada dentro de xColumn de uma tabela virtual.
/// Retorna true se a coluna não será alterada, permitindo valor otimizado.
// pub fn vtab_nochange(ctx: &Sqlite3Context) -> i32

/// Determina a sequência de colação para uma restrição de tabela virtual.
/// Chamada apenas de dentro de xBestIndex de uma tabela virtual.
/// Retorna nome de colação por COLLATE do operador, coluna ou "BINARY" padrão.
// pub fn vtab_collation(index_info: &Sqlite3IndexInfo, idx: i32) -> &'static [u8]

/// Determina se uma consulta de tabela virtual é DISTINCT.
/// Chamada apenas de dentro de xBestIndex. Retorna inteiro de 0 a 3 indicando requisitos de ordenação.
/// 0: ordenação por aOrderBy obrigatória; 1: linhas adjacentes suficientes; 2: qualquer ordem, duplicatas opcionais;
/// 3: ordenação por aOrderBy obrigatória como em 0, mas duplicatas sobre colUsed podem ser omitidas.
// pub fn vtab_distinct(index_info: &Sqlite3IndexInfo) -> i32

/// Opção de configuração virtual: tabela suporta restrições sem rollback automático.
/// xUpdate não deve modificar dados até confirmar sucesso; SQLITE_CONSTRAINT antes de modificação.
pub const SQLITE_VTAB_CONSTRAINT_SUPPORT: i32 = 1;

/// Opção de configuração virtual: tabela é segura para uso em disparadores e visões.
/// Implementador garante que tabela não causa dano, mesmo com controle malicioso.
/// Use apenas quando absolutamente necessário.
pub const SQLITE_VTAB_INNOCUOUS: i32 = 2;

/// Opção de configuração virtual: tabela proibida de uso dentro de disparadores e visões.
/// Revoga permissão padrão de uso em contextos de disparador/visão.
pub const SQLITE_VTAB_DIRECTONLY: i32 = 3;

/// Opção de configuração virtual: tabela usa todos os esquemas.
/// Força transação de leitura em todos os esquemas ("main", "temp", ATTACH-ed) quando tabela é usada.
pub const SQLITE_VTAB_USES_ALL_SCHEMAS: i32 = 4;


// ---- part_027.rs ----

// As declarações `SQLITE_API` de sqlite3.h não geram código: as implementações
// vivem nos módulos de origem (vtab.c, vdbeapi.c, main.c, vdbeaux.c) e saem de
// lá com os nomes da convenção (`api::vtab_distinct`, `api::vtab_in`,
// `api::vtab_in_first`, `api::vtab_in_next`, `api::vtab_rhs_value`,
// `api::stmt_scanstatus`, `api::stmt_scanstatus_v2`,
// `api::stmt_scanstatus_reset`, `api::db_cacheflush`, `api::preupdate_hook`,
// `api::preupdate_old`, `api::preupdate_count`, `api::preupdate_depth`,
// `api::preupdate_new`, `api::preupdate_blobwrite`, `api::system_errno`).

// Modos de resolução de conflito. SQLITE_IGNORE (2) e SQLITE_ABORT (4) são
// definidos junto dos códigos de resultado.
pub const SQLITE_ROLLBACK: i32 = 1;
pub const SQLITE_FAIL: i32 = 3;
pub const SQLITE_REPLACE: i32 = 5;

// Opcodes de status de varredura de instrução preparada.
pub const SQLITE_SCANSTAT_NLOOP: i32 = 0;
pub const SQLITE_SCANSTAT_NVISIT: i32 = 1;
pub const SQLITE_SCANSTAT_EST: i32 = 2;
pub const SQLITE_SCANSTAT_NAME: i32 = 3;
pub const SQLITE_SCANSTAT_EXPLAIN: i32 = 4;
pub const SQLITE_SCANSTAT_SELECTID: i32 = 5;
pub const SQLITE_SCANSTAT_PARENTID: i32 = 6;
pub const SQLITE_SCANSTAT_NCYCLE: i32 = 7;

// Flags de status de varredura.
pub const SQLITE_SCANSTAT_COMPLEX: i32 = 0x0001;

/// Callback do preupdate hook (`xPreUpdate`): contexto, conexão, operação
/// (SQLITE_UPDATE, DELETE ou INSERT), nome do banco, nome da tabela, rowid
/// original e rowid novo.
pub type PreUpdateCallback = Rc<dyn Fn(&DbRef, i32, &[u8], &[u8], i64, i64)>;

/// Registro do estado de um banco em modo WAL em um ponto específico da
/// história (`sqlite3_snapshot`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sqlite3Snapshot {
    pub hidden: [u8; 48],
}


// ---- part_028.rs ----

// Este trecho do sqlite3.h só tem protótipos (sqlite3_snapshot_get, sqlite3_snapshot_open,
// sqlite3_snapshot_free, sqlite3_snapshot_cmp, sqlite3_snapshot_recover, sqlite3_serialize,
// sqlite3_deserialize) e as constantes de flags. Os corpos vivem em outros arquivos de origem
// (snapshot no pager/wal/main, serialize e deserialize no memdb) e seguem a regra de nomes:
// `api::snapshot_get`, `api::snapshot_open`, `api::snapshot_free`, `api::snapshot_cmp`,
// `api::snapshot_recover`, `api::serialize`, `api::deserialize`. Nada a traduzir além das constantes.

// === Snapshot da base de dados ===

// sqlite3_snapshot_get(db, zSchema, ppSnapshot): captura um snapshot do estado atual do esquema S
// na conexão D. Exige conexão fora do modo autocommit, esquema em modo WAL, nenhuma transação de
// escrita aberta e ao menos uma transação já gravada no arquivo WAL. Pode devolver SQLITE_ERROR
// ou SQLITE_NOMEM. Só existe com SQLITE_ENABLE_SNAPSHOT.

// sqlite3_snapshot_open(db, zSchema, pSnapshot): inicia (ou promove) a transação de leitura do
// esquema S para o snapshot histórico P. Falha com SQLITE_ERROR_SNAPSHOT se o snapshot foi
// sobrescrito por um checkpoint.

// sqlite3_snapshot_free(pSnapshot): destrói o snapshot (em Rust, o Box é solto pelo dono).

// sqlite3_snapshot_cmp(p1, p2): negativo se p1 é mais antigo que p2, zero se iguais, positivo se
// p1 é mais novo.

// sqlite3_snapshot_recover(db, zDb): varre o WAL e torna todos os snapshots válidos disponíveis
// para snapshot_open.

// === Serialização ===

// sqlite3_serialize(db, zSchema, piSize, mFlags): devolve a serialização do banco S (cópia do
// arquivo em disco; para memória ou TEMP, os bytes que seriam gravados num backup). Com
// SQLITE_SERIALIZE_NOCOPY não aloca e devolve a imagem contígua em uso, ou nada.

/// Flag de `serialize`: não faz alocações, devolve a imagem contígua em uso.
pub const SQLITE_SERIALIZE_NOCOPY: u32 = 0x001;

// sqlite3_deserialize(db, zSchema, pData, szDb, szBuf, mFlags): faz a conexão D largar o banco S
// e reabri-lo em memória a partir da serialização em pData (szDb bytes, buffer de szBuf bytes).
// Falha com SQLITE_BUSY se o banco está em transação de leitura ou em backup, e com SQLITE_ERROR
// para o banco TEMP.

/// Flag de `deserialize`: o buffer pertence ao SQLite e é liberado ao fechar.
pub const SQLITE_DESERIALIZE_FREEONCLOSE: u32 = 1;

/// Flag de `deserialize`: o buffer pode crescer (realloc64).
pub const SQLITE_DESERIALIZE_RESIZEABLE: u32 = 2;

/// Flag de `deserialize`: o banco é somente leitura.
pub const SQLITE_DESERIALIZE_READONLY: u32 = 4;

// O restante do trecho (#ifdef SQLITE_OMIT_FLOATING_POINT, bloco __wasi__, fechamento do extern "C"
// e do guarda SQLITE3_H) não gera código: nenhuma das opções vale no Debian 13 amd64.

