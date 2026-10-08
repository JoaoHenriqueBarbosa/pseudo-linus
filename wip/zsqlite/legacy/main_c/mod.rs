// Mesclado das partes traduzidas de main_c (as partes quebram funções grandes no meio).
#![allow(unused_imports, ambiguous_glob_reexports)]
use crate::prelude::*;

// ---- part_000.rs ----

use std::sync::Mutex;

/// Inicializador de extensão que não faz nada e sempre tem sucesso, exceto que
/// falha se a simulação de falhas estiver ajustada para 500.
pub fn test_ext_init(db: &Sqlite3Ref) -> i32 {
    let _ = db;
    fault_sim(500)
}

/// Vetor de inicializadores das extensões embutidas, na ordem do C, já com as
/// opções de compilação do Debian 13 resolvidas (FTS3, FTS5, RTREE, DBPAGE_VTAB,
/// DBSTAT_VTAB, JSON e STMTVTAB ligados; ICU, BYTECODE_VTAB e EXTRA_AUTOEXT ausentes).
pub static BUILTIN_EXTENSIONS: &[fn(&Sqlite3Ref) -> i32] = &[
    fts3_init,
    fts5_init,
    rtree_init,
    dbpage_register,
    dbstat_register,
    test_ext_init,
    json_table_functions,
    stmt_vtab_init,
];

// A constante sqlite3_version[] não é definida aqui: na amalgamação o
// SQLITE_AMALGAMATION a define em outro lugar (aqui é `SQLITE3_VERSION`).

/// Devolve o texto de SQLITE_VERSION (sqlite3_libversion).
pub fn libversion() -> &'static str {
    SQLITE3_VERSION
}

/// Devolve o inteiro SQLITE_VERSION_NUMBER (sqlite3_libversion_number).
pub fn libversion_number() -> i32 {
    SQLITE_VERSION_NUMBER
}

/// Devolve zero se e somente se o SQLite foi compilado com o código de mutex
/// omitido, ou seja, SQLITE_THREADSAFE igual a 0 (sqlite3_threadsafe).
pub fn threadsafe() -> i32 {
    SQLITE_THREADSAFE
}

/// Se apontar para o nome de um diretório, ele é usado para guardar arquivos
/// temporários. Veja também "PRAGMA temp_store_directory".
pub static SQLITE_TEMP_DIRECTORY: Mutex<Option<Vec<u8>>> = Mutex::new(None);

/// Se apontar para o nome de um diretório, ele é usado para guardar todos os
/// arquivos de banco de dados indicados com caminho relativo. Veja também
/// "PRAGMA data_store_directory".
pub static SQLITE_DATA_DIRECTORY: Mutex<Option<Vec<u8>>> = Mutex::new(None);

/// Determina se a matemática de ponto flutuante de alta precisão (long double)
/// funciona corretamente na CPU atual.
///
/// No C, `long double` do x86-64 e do arm64 do Debian tem mantissa de 64 bits ou
/// mais (sizeof maior que 8) e o experimento roda de verdade. Aqui o experimento
/// é refeito exatamente em aritmética inteira: `a` e `b` são calculados em
/// `double` (as constantes do C são `double`), e a soma `c = a+b` é feita com
/// mantissa de 64 bits. Com `b` perto de 1e18 (entre 2^59 e 2^60) o passo da
/// mantissa de 64 bits é 2^-4, então a soma é arredondada em múltiplos de 1/16.
pub fn has_high_precision_double(rc: i32) -> i32 {
    let rc = rc.wrapping_add(1);
    let a: f64 = 1.0 + (rc as f64) * 0.1;
    let b: f64 = 1.0e+18 + (rc as f64) * 25.0;
    // Representação em ponto fixo com 4 bits fracionários (passo de 1/16).
    let a_q: i128 = (a * 16.0).round_ties_even() as i128;
    let b_q: i128 = (b as i128) * 16;
    let c_q: i128 = a_q + b_q;
    (b_q != c_q) as i32
}

/// Inicializa o SQLite.
///
/// Esta rotina precisa ser chamada para inicializar os subsistemas de alocação de
/// memória, VFS e mutex antes de qualquer trabalho sério com o SQLite. Como o
/// SQLITE_OMIT_AUTOINIT não é usado, ela é chamada automaticamente por rotinas
/// como sqlite3_open().
///
/// É um no-op, exceto na primeira chamada do processo ou na primeira chamada
/// depois de sqlite3_shutdown.
///
/// A primeira thread a chamar executa a inicialização até o fim. Threads
/// seguintes que chamem antes de ela terminar ficam bloqueadas até a primeira
/// concluir. Chamadas recursivas da própria primeira thread voltam sem bloquear.
///
/// Nota: SQLITE_CONFIG é tratado aqui como o singleton global mutável
/// sqlite3GlobalConfig, como no C; o acesso concorrente é serializado pelos
/// mutexes que esta rotina adquire.
pub fn initialize() -> i32 {
    let mut rc: i32; // Código de resultado

    // Se o SQLite já está completamente inicializado, esta chamada é um no-op.
    // Mas a inicialização precisa estar completa; por isso is_init só é
    // marcado no fim desta rotina.
    if config_mut().is_init != 0 {
        memory_barrier();
        return SQLITE_OK;
    }

    // Garante que o subsistema de mutex foi inicializado. Se não for possível,
    // retorna cedo com o erro. O subsistema de mutex serializa a própria
    // inicialização.
    rc = mutex_init();
    if rc != 0 {
        return rc;
    }

    // Inicializa o sistema malloc() e o mutex recursivo p_init_mutex. Esta
    // operação é protegida pelo mutex STATIC_MAIN. Note que mutex_alloc() é
    // chamado para um mutex estático antes de inicializar o subsistema malloc:
    // a alocação de um mutex estático não pode exigir suporte do malloc.
    // O mutex estático principal.
    let p_main_mtx = mutex_alloc(SQLITE_MUTEX_STATIC_MAIN);
    mutex_enter(p_main_mtx.as_deref());
    config_mut().is_mutex_init = 1;
    // No C, `rc` chega aqui com o valor de mutex_init(), que é zero (SQLITE_OK).
    rc = SQLITE_OK;
    if config_mut().is_malloc_init == 0 {
        rc = malloc_init();
    }
    if rc == SQLITE_OK {
        config_mut().is_malloc_init = 1;
        if config_mut().p_init_mutex.is_none() {
            let p_init_mutex = mutex_alloc(SQLITE_MUTEX_RECURSIVE);
            let init_mutex_missing = p_init_mutex.is_none();
            config_mut().p_init_mutex = p_init_mutex;
            if config_mut().b_core_mutex != 0 && init_mutex_missing {
                rc = SQLITE_NOMEM_BKPT;
            }
        }
    }
    if rc == SQLITE_OK {
        config_mut().n_ref_init_mutex += 1;
    }
    mutex_leave(p_main_mtx.as_deref());

    // Se rc não é SQLITE_OK neste ponto, o subsistema malloc não pôde ser
    // inicializado ou o sistema falhou ao alocar o mutex p_init_mutex.
    // Retorna o erro nos dois casos.
    if rc != SQLITE_OK {
        return rc;
    }

    // Faz o resto da inicialização sob o mutex recursivo, para tratar chamadas
    // recursivas a initialize(). As chamadas recursivas normalmente vêm de
    // os_init() quando ele chama vfs_register(), mas outras também são possíveis.
    //
    // O SQLite serializa automaticamente as chamadas ao método xInit, então ele
    // não precisa ser thread-safe. O mutex abaixo é o que serializa o acesso aos
    // métodos xInit do pcache da aplicação: a chamada a xInit está embutida em
    // pcache_initialize().
    // O guard do mutex recursivo é solto antes de qualquer chamada que acesse a configuração,
    // então o p_init_mutex é clonado (Rc) para fora da configuração.
    let p_init_mutex = config_mut().p_init_mutex.clone();
    mutex_enter(p_init_mutex.as_deref());
    if config_mut().is_init == 0 && config_mut().in_progress == 0 {
        config_mut().in_progress = 1;
        // memset(&sqlite3BuiltinFunctions, 0, sizeof(...)).
        *builtin_functions_mut() = builtin_functions_new();
        register_builtin_functions();
        if config_mut().is_p_cache_init == 0 {
            rc = pcache_initialize();
        }
        if rc == SQLITE_OK {
            config_mut().is_p_cache_init = 1;
            rc = os_init();
        }
        if rc == SQLITE_OK {
            rc = memdb_init();
        }
        if rc == SQLITE_OK {
            let (mut p_page, sz_page, n_page) = {
                let cfg = config_mut();
                (cfg.p_page.clone(), cfg.sz_page, cfg.n_page)
            };
            pcache_buffer_setup(p_page.as_deref_mut(), sz_page, n_page);
            memory_barrier();
            config_mut().is_init = 1;
        }
        config_mut().in_progress = 0;
    }
    mutex_leave(p_init_mutex.as_deref());
    drop(p_init_mutex);

    // Volta ao mutex estático e limpa o mutex recursivo para evitar vazamento
    // de recurso.
    mutex_enter(p_main_mtx.as_deref());
    config_mut().n_ref_init_mutex -= 1;
    if config_mut().n_ref_init_mutex <= 0 {
        debug_assert!(config_mut().n_ref_init_mutex == 0);
        let p_old = config_mut().p_init_mutex.take();
        mutex_free(p_old);
    }
    mutex_leave(p_main_mtx.as_deref());

    // Determina experimentalmente se o ponto flutuante de alta precisão está
    // disponível.
    config_mut().b_use_long_double = has_high_precision_double(rc) as u8;

    rc
}

/// Atribui `sqlite3_data_directory` (usado pelo shutdown, pelo PRAGMA data_store_directory e
/// pelo os_unix).
pub fn set_data_directory(v: Option<Vec<u8>>) {
    *SQLITE_DATA_DIRECTORY.lock().unwrap() = v;
}

/// Atribui `sqlite3_temp_directory` (usado pelo shutdown, pelo PRAGMA temp_store_directory e
/// pelo os_unix).
pub fn set_temp_directory(v: Option<Vec<u8>>) {
    *SQLITE_TEMP_DIRECTORY.lock().unwrap() = v;
}


// ---- part_001.rs ----

// Notas para o integrador (tech lead):
//  - A configuração global `sqlite3GlobalConfig` vive em `global_c` como `SQLITE_CONFIG`. Como
//    ela é mutada em tempo de execução, este trecho a acessa por `config_mut()`, que deve devolver
//    um acesso exclusivo de curta duração (`RefMut<'static, Sqlite3Config>` sobre um
//    `thread_local!`). Nenhum empréstimo fica vivo durante chamada a outra rotina do SQLite.
//  - `sqlite3_data_directory` e `sqlite3_temp_directory` (globais de main.c) são gravadas por
//    `set_data_directory(None)` e `set_temp_directory(None)`.
//  - O `sqlite3_config(int op, ...)` variádico do C recebe os argumentos como um `Vec<ConfigArg>`,
//    na ordem em que o C faria `va_arg`. O `pLogArg` do `SQLITE_CONFIG_LOG` fica capturado na
//    própria closure de log, então esse caso consome um único argumento.

/// Argumento variádico de `sqlite3_config()`. Cada variante corresponde a um tipo que o C lê com
/// `va_arg`. As variantes `*Out` representam os ponteiros de saída (`sqlite3_mem_methods*` etc.).
pub enum ConfigArg<'a> {
    Int(i32),
    UInt(u32),
    Int64(i64),
    Page(Option<Box<[u8]>>),
    MemMethods(sqlite3_mem_methods),
    MemMethodsOut(&'a mut sqlite3_mem_methods),
    MutexMethods(sqlite3_mutex_methods),
    MutexMethodsOut(&'a mut sqlite3_mutex_methods),
    PcacheMethods2(sqlite3_pcache_methods2),
    PcacheMethods2Out(&'a mut sqlite3_pcache_methods2),
    IntOut(&'a mut i32),
    Log(Option<std::rc::Rc<dyn Fn(i32, &[u8])>>),
}

/// `va_arg(ap, int)`.
fn arg_int<'a, I: Iterator<Item = ConfigArg<'a>>>(ap: &mut I) -> i32 {
    match ap.next() {
        Some(ConfigArg::Int(v)) => v,
        Some(ConfigArg::UInt(v)) => v as i32,
        _ => 0,
    }
}

/// `va_arg(ap, unsigned int)`.
fn arg_uint<'a, I: Iterator<Item = ConfigArg<'a>>>(ap: &mut I) -> u32 {
    match ap.next() {
        Some(ConfigArg::UInt(v)) => v,
        Some(ConfigArg::Int(v)) => v as u32,
        _ => 0,
    }
}

/// `va_arg(ap, sqlite3_int64)`.
fn arg_int64<'a, I: Iterator<Item = ConfigArg<'a>>>(ap: &mut I) -> i64 {
    match ap.next() {
        Some(ConfigArg::Int64(v)) => v,
        Some(ConfigArg::Int(v)) => v as i64,
        _ => 0,
    }
}

/// Desfaz os efeitos de `sqlite3_initialize()`. Não pode ser chamada enquanto houver conexões de
/// banco ou alocações de memória pendentes, nem enquanto qualquer parte do SQLite estiver em uso
/// em alguma thread. Não é thread-safe, mas é seguro chamá-la quando o SQLite já está desligado
/// (nesse caso é um no-op inofensivo).
pub fn shutdown() -> i32 {
    if config_mut().is_init != 0 {
        os_end();
        reset_auto_extension();
        config_mut().is_init = 0;
    }
    if config_mut().is_p_cache_init != 0 {
        pcache_shutdown();
        config_mut().is_p_cache_init = 0;
    }
    if config_mut().is_malloc_init != 0 {
        malloc_end();
        config_mut().is_malloc_init = 0;

        // O subsistema de heap acabou de ser desligado e estes valores deveriam ser NULL ou
        // apontar para memória obtida de sqlite3_malloc(), que depende desse subsistema;
        // portanto, garante que eles não se refiram a memória de heap recém-invalidada. Isso só
        // é feito se a chamada atual foi a que de fato desligou o subsistema de heap.
        set_data_directory(None);
        set_temp_directory(None);
    }
    if config_mut().is_mutex_init != 0 {
        mutex_end();
        config_mut().is_mutex_init = 0;
    }

    SQLITE_OK
}

/// Permite que a aplicação modifique a configuração global da biblioteca SQLite em tempo de
/// execução. Só deve ser chamada quando não há conexões de banco nem alocações pendentes. Não é
/// thread-safe. Ignorar esses avisos leva a comportamento imprevisível.
pub fn config(op: i32, args: Vec<ConfigArg>) -> i32 {
    let mut rc = SQLITE_OK;

    // sqlite3_config() normalmente devolve SQLITE_MISUSE se for chamada enquanto a biblioteca
    // está em uso. Exceto por alguns opcodes selecionados, que são permitidos.
    if config_mut().is_init != 0 {
        let m_anytime_config_option: u64 =
            maskbit64(SQLITE_CONFIG_LOG) | maskbit64(SQLITE_CONFIG_PCACHE_HDRSZ);
        if op < 0 || op > 63 || (maskbit64(op) & m_anytime_config_option) == 0 {
            return sqlite_misuse_bkpt(line!() as i32);
        }
    }

    let mut ap = args.into_iter();
    match op {
        // As opções de configuração de mutex só existem em uma compilação threadsafe.
        SQLITE_CONFIG_SINGLETHREAD => {
            // Esta opção define o modo de threading como Single-thread.
            let mut cfg = config_mut();
            cfg.b_core_mutex = 0; // Desabilita o mutex do núcleo
            cfg.b_full_mutex = 0; // Desabilita o mutex nas conexões
        }
        SQLITE_CONFIG_MULTITHREAD => {
            // Esta opção define o modo de threading como Multi-thread.
            let mut cfg = config_mut();
            cfg.b_core_mutex = 1; // Habilita o mutex do núcleo
            cfg.b_full_mutex = 0; // Desabilita o mutex nas conexões
        }
        SQLITE_CONFIG_SERIALIZED => {
            // Esta opção define o modo de threading como Serialized.
            let mut cfg = config_mut();
            cfg.b_core_mutex = 1; // Habilita o mutex do núcleo
            cfg.b_full_mutex = 1; // Habilita o mutex nas conexões
        }
        SQLITE_CONFIG_MUTEX => {
            // Especifica uma implementação alternativa de mutex
            if let Some(ConfigArg::MutexMethods(v)) = ap.next() {
                config_mut().mutex = v;
            }
        }
        SQLITE_CONFIG_GETMUTEX => {
            // Obtém a implementação de mutex atual
            if let Some(ConfigArg::MutexMethodsOut(out)) = ap.next() {
                *out = config_mut().mutex.clone();
            }
        }

        SQLITE_CONFIG_MALLOC => {
            // A opção recebe um único argumento, um sqlite3_mem_methods. O argumento especifica
            // rotinas alternativas de alocação de memória no lugar das embutidas no SQLite.
            if let Some(ConfigArg::MemMethods(v)) = ap.next() {
                config_mut().m = v;
            }
        }
        SQLITE_CONFIG_GETMALLOC => {
            // A opção recebe um único argumento, um sqlite3_mem_methods, que é preenchido com as
            // rotinas de alocação de memória atualmente definidas.
            if config_mut().m.x_malloc.is_none() {
                mem_set_default();
            }
            if let Some(ConfigArg::MemMethodsOut(out)) = ap.next() {
                *out = config_mut().m.clone();
            }
        }
        SQLITE_CONFIG_MEMSTATUS => {
            // Recebe um único argumento int, interpretado como booleano, que liga ou desliga a
            // coleta de estatísticas de alocação de memória. Não pode mudar em tempo de execução.
            config_mut().b_memstat = arg_int(&mut ap);
        }
        SQLITE_CONFIG_SMALL_MALLOC => {
            config_mut().b_small_malloc = arg_int(&mut ap) as u8;
        }
        SQLITE_CONFIG_PAGECACHE => {
            // Três argumentos: ponteiro para memória alinhada em 8 bytes (pMem), tamanho de cada
            // linha do cache de páginas (sz) e número de linhas (N).
            let p_page = match ap.next() {
                Some(ConfigArg::Page(v)) => v,
                _ => None,
            };
            let sz_page = arg_int(&mut ap);
            let n_page = arg_int(&mut ap);
            let mut cfg = config_mut();
            cfg.p_page = p_page;
            cfg.sz_page = sz_page;
            cfg.n_page = n_page;
        }
        SQLITE_CONFIG_PCACHE_HDRSZ => {
            // Recebe um único parâmetro, um ponteiro para inteiro, e grava nele o número de bytes
            // extras por página necessários para cada página em SQLITE_CONFIG_PAGECACHE.
            let v = header_size_btree() + header_size_pcache() + header_size_pcache1();
            if let Some(ConfigArg::IntOut(out)) = ap.next() {
                *out = v;
            }
        }

        SQLITE_CONFIG_PCACHE => {
            // no-op
        }
        SQLITE_CONFIG_GETPCACHE => {
            // agora é um erro
            rc = SQLITE_ERROR;
        }

        SQLITE_CONFIG_PCACHE2 => {
            // Recebe um único argumento, um sqlite3_pcache_methods2, que especifica a interface
            // de uma implementação personalizada de cache de páginas.
            if let Some(ConfigArg::PcacheMethods2(v)) = ap.next() {
                config_mut().pcache2 = v;
            }
        }
        SQLITE_CONFIG_GETPCACHE2 => {
            // Recebe um único argumento, um sqlite3_pcache_methods2; o SQLite copia para ele a
            // implementação atual do cache de páginas.
            if config_mut().pcache2.x_init.is_none() {
                pcache_set_default();
            }
            if let Some(ConfigArg::PcacheMethods2Out(out)) = ap.next() {
                *out = config_mut().pcache2.clone();
            }
        }

        SQLITE_CONFIG_LOOKASIDE => {
            let sz_lookaside = arg_int(&mut ap);
            let n_lookaside = arg_int(&mut ap);
            let mut cfg = config_mut();
            cfg.sz_lookaside = sz_lookaside;
            cfg.n_lookaside = n_lookaside;
        }

        // Registra a função de log e seu primeiro argumento. O padrão é NULL. O log fica
        // desabilitado se o ponteiro da função for NULL.
        SQLITE_CONFIG_LOG => {
            let x_log = match ap.next() {
                Some(ConfigArg::Log(v)) => v,
                _ => None,
            };
            config_mut().x_log = x_log;
        }

        // O ajuste de compilação para nomes de arquivo URI pode ser mudado na partida com
        // sqlite3_config(SQLITE_CONFIG_URI,1) ou sqlite3_config(SQLITE_CONFIG_URI,0).
        SQLITE_CONFIG_URI => {
            // Recebe um único argumento int. Se não zero, o tratamento de URI fica habilitado
            // globalmente. Se zero, fica desabilitado globalmente.
            let b_open_uri = arg_int(&mut ap);
            config_mut().b_open_uri = b_open_uri as u8;
        }

        SQLITE_CONFIG_COVERING_INDEX_SCAN => {
            // Recebe um único argumento inteiro, interpretado como booleano, que liga ou desliga
            // o uso de índices de cobertura em varreduras completas de tabela no otimizador.
            config_mut().b_use_cis = arg_int(&mut ap) as u8;
        }

        SQLITE_CONFIG_MMAP_SIZE => {
            // Recebe dois inteiros de 64 bits (sqlite3_int64): o limite padrão de tamanho de mmap
            // (o padrão de PRAGMA mmap_size) e o limite máximo permitido.
            let mut sz_mmap = arg_int64(&mut ap);
            let mut mx_mmap = arg_int64(&mut ap);
            // Se qualquer argumento for negativo, ele muda para o padrão de compilação. O máximo
            // permitido é truncado em silêncio para não exceder SQLITE_MAX_MMAP_SIZE.
            if mx_mmap < 0 || mx_mmap > SQLITE_MAX_MMAP_SIZE as i64 {
                mx_mmap = SQLITE_MAX_MMAP_SIZE as i64;
            }
            if sz_mmap < 0 {
                sz_mmap = SQLITE_DEFAULT_MMAP_SIZE as i64;
            }
            if sz_mmap > mx_mmap {
                sz_mmap = mx_mmap;
            }
            let mut cfg = config_mut();
            cfg.mx_mmap = mx_mmap;
            cfg.sz_mmap = sz_mmap;
        }

        SQLITE_CONFIG_PMASZ => {
            config_mut().sz_pma = arg_uint(&mut ap);
        }

        SQLITE_CONFIG_STMTJRNL_SPILL => {
            config_mut().n_stmt_spill = arg_int(&mut ap);
        }

        SQLITE_CONFIG_MEMDB_MAXSIZE => {
            config_mut().mx_memdb_size = arg_int64(&mut ap);
        }

        SQLITE_CONFIG_ROWID_IN_VIEW => {
            // SQLITE_ALLOW_ROWID_IN_VIEW não está definido: o resultado é sempre 0.
            if let Some(ConfigArg::IntOut(p_val)) = ap.next() {
                *p_val = 0;
            }
        }

        _ => {
            rc = SQLITE_ERROR;
        }
    }
    rc
}


// ---- part_002.rs ----

/// Tipo da função de comparação de uma `CollSeq` (o `xCmp` do C), compartilhável por `Rc`.
pub type CollCmp = std::rc::Rc<dyn Fn(&CallbackArg, &[u8], &[u8]) -> i32>;

// O `xCmp==binCollFunc` do C compara endereços de função. No modelo Rust a identidade é a do `Rc`:
// as sequências embutidas BINARY, RTRIM e NOCASE devem ser registradas (em `open_database`) com os
// `Rc` devolvidos por `bin_coll_func_rc()`, `rtrim_coll_func_rc()` e `nocase_collating_func_rc()`,
// e `is_binary` compara com `Rc::ptr_eq`.
thread_local! {
    static BIN_COLL_FUNC_RC: CollCmp = std::rc::Rc::new(bin_coll_func);
    static RTRIM_COLL_FUNC_RC: CollCmp = std::rc::Rc::new(rtrim_coll_func);
    static NOCASE_COLLATING_FUNC_RC: CollCmp = std::rc::Rc::new(nocase_collating_func);
}

/// Devolve o `Rc` canônico da função de comparação BINARY.
pub fn bin_coll_func_rc() -> CollCmp {
    BIN_COLL_FUNC_RC.with(|f| f.clone())
}

/// Devolve o `Rc` canônico da função de comparação RTRIM.
pub fn rtrim_coll_func_rc() -> CollCmp {
    RTRIM_COLL_FUNC_RC.with(|f| f.clone())
}

/// Devolve o `Rc` canônico da função de comparação NOCASE.
pub fn nocase_collating_func_rc() -> CollCmp {
    NOCASE_COLLATING_FUNC_RC.with(|f| f.clone())
}

/// Configura os buffers de lookaside de uma conexão de banco de dados.
/// Devolve SQLITE_OK em caso de sucesso.
/// Se o lookaside já está ativo, devolve SQLITE_BUSY.
///
/// O parâmetro `sz` é o número de bytes de cada slot de lookaside. O parâmetro `cnt` é o número de
/// slots. Se `p_buf` é `None` o espaço da memória de lookaside é obtido de `sqlite3_malloc()`.
/// Se `p_buf` é `Some`, são `sz*cnt` bytes de memória a usar para o lookaside.
///
/// No modelo Rust o lookaside é descrito por deslocamentos (índices) no buffer, e os slots livres
/// são os cabeçalhos `LookasideSlot`: o `Box` guarda em `p_next` o índice do slot do topo da lista,
/// e o encadeamento entre os slots é implícito (cada slot aponta para o anterior na ordem de
/// construção, `sz` bytes antes nos slots grandes e `LOOKASIDE_SMALL` bytes antes nos pequenos).
pub fn setup_lookaside(db: &mut Sqlite3, p_buf: Option<Vec<u8>>, sz: i32, cnt: i32) -> i32 {
    // SQLITE_OMIT_LOOKASIDE não está definido no Debian.
    let mut sz = sz;
    let mut cnt = cnt;
    // Posição de pStart: Some(0) quando há buffer, None quando é nulo.
    let p_start: Option<usize>;
    let mut sz_alloc: i64 = (sz as i64) * (cnt as i64);
    let n_big: i64; // Número de slots de tamanho completo
    let n_sm: i64; // Número de slots menores de LOOKASIDE_SMALL bytes

    if lookaside_used(db, None) > 0 {
        return SQLITE_BUSY;
    }
    // Libera qualquer buffer de lookaside existente deste handle antes de alocar um novo, para
    // não precisar de espaço para os dois ao mesmo tempo.
    if db.lookaside.b_malloced != 0 {
        // sqlite3_free(db->lookaside.pStart): o buffer é um índice no modelo Rust, nada a liberar.
    }
    // O tamanho de um slot de lookaside após ROUNDDOWN8 precisa ser maior que um ponteiro para
    // ser útil.
    sz &= !7; // IMP: R-33038-09382
    if sz <= 8 {
        sz = 0;
    }
    if cnt < 0 {
        cnt = 0;
    }
    if sz == 0 || cnt == 0 {
        sz = 0;
        p_start = None;
    } else if p_buf.is_none() {
        begin_benign_malloc();
        p_start = Some(0); // IMP: R-61949-35727
        end_benign_malloc();
        // szAlloc = sqlite3MallocSize(pStart): o tamanho obtido é o solicitado.
    } else {
        p_start = Some(0);
    }
    // SQLITE_OMIT_TWOSIZE_LOOKASIDE não está definido no Debian.
    if (sz as usize) >= LOOKASIDE_SMALL * 3 {
        n_big = sz_alloc / ((3 * LOOKASIDE_SMALL) as i64 + sz as i64);
        n_sm = (sz_alloc - (sz as i64) * n_big) / (LOOKASIDE_SMALL as i64);
    } else if (sz as usize) >= LOOKASIDE_SMALL * 2 {
        n_big = sz_alloc / (LOOKASIDE_SMALL as i64 + sz as i64);
        n_sm = (sz_alloc - (sz as i64) * n_big) / (LOOKASIDE_SMALL as i64);
    } else if sz > 0 {
        n_big = sz_alloc / (sz as i64);
        n_sm = 0;
    } else {
        n_big = 0;
        n_sm = 0;
    }
    let n_big = n_big as i32;
    let n_sm = n_sm as i32;
    db.lookaside.p_start = 0;
    db.lookaside.p_init = None;
    db.lookaside.p_free = None;
    db.lookaside.sz = sz as u16;
    db.lookaside.sz_true = sz as u16;
    if p_start.is_some() {
        // `p` é o deslocamento do próximo slot no buffer.
        let mut p: usize = 0;
        let mut init_top: Option<usize> = None;
        debug_assert!(sz > 8);
        for _ in 0..n_big {
            init_top = Some(p);
            p += sz as usize;
        }
        db.lookaside.p_init = Some(Box::new(LookasideSlot { p_next: init_top }));
        db.lookaside.p_small_init = None;
        db.lookaside.p_small_free = None;
        db.lookaside.p_middle = p;
        let mut small_top: Option<usize> = None;
        for _ in 0..n_sm {
            small_top = Some(p);
            p += LOOKASIDE_SMALL;
        }
        db.lookaside.p_small_init = Some(Box::new(LookasideSlot { p_next: small_top }));
        debug_assert!((p as i64) <= sz_alloc);
        db.lookaside.p_end = p;
        db.lookaside.b_disable = 0;
        db.lookaside.b_malloced = if p_buf.is_none() { 1 } else { 0 };
        db.lookaside.n_slot = (n_big + n_sm) as u32;
    } else {
        db.lookaside.p_start = 0;
        db.lookaside.p_small_init = None;
        db.lookaside.p_small_free = None;
        db.lookaside.p_middle = 0;
        db.lookaside.p_end = 0;
        db.lookaside.b_disable = 1;
        db.lookaside.sz = 0;
        db.lookaside.b_malloced = 0;
        db.lookaside.n_slot = 0;
    }
    db.lookaside.p_true_end = db.lookaside.p_end;
    debug_assert!(lookaside_used(db, None) == 0);
    sz_alloc = 0;
    let _ = sz_alloc;
    SQLITE_OK
}

/// Devolve o mutex associado a uma conexão de banco de dados.
pub fn db_mutex(db: &Sqlite3Ref) -> Option<std::rc::Rc<Sqlite3Mutex>> {
    db.borrow().mutex.clone()
}

/// Libera o máximo de memória possível da conexão de banco de dados dada.
pub fn db_release_memory(db: &Sqlite3Ref) -> i32 {
    mutex_enter(db.borrow().mutex.as_deref());
    btree_enter_all(&mut db.borrow_mut());
    let n_db = db.borrow().n_db;
    for i in 0..n_db as usize {
        let p_bt = db.borrow().a_db[i].p_bt.clone();
        if let Some(p_bt) = p_bt {
            let p_pager = btree_pager(&p_bt.borrow());
            pager_shrink(&mut p_pager.borrow_mut());
        }
    }
    btree_leave_all(&mut db.borrow_mut());
    mutex_leave(db.borrow().mutex.as_deref());
    SQLITE_OK
}

/// Descarrega para o disco as páginas sujas do cache do pager de qualquer banco anexado.
pub fn db_cacheflush(db: &Sqlite3Ref) -> i32 {
    let mut rc = SQLITE_OK;
    let mut b_seen_busy = 0;

    mutex_enter(db.borrow().mutex.as_deref());
    btree_enter_all(&mut db.borrow_mut());
    let n_db = db.borrow().n_db;
    let mut i = 0usize;
    while rc == SQLITE_OK && (i as i32) < n_db {
        let p_bt = db.borrow().a_db[i].p_bt.clone();
        if let Some(p_bt) = p_bt {
            if btree_txn_state(Some(&p_bt)) == SQLITE_TXN_WRITE {
                let p_pager = btree_pager(&p_bt.borrow());
                rc = pager_flush(&mut p_pager.borrow_mut());
                if rc == SQLITE_BUSY {
                    b_seen_busy = 1;
                    rc = SQLITE_OK;
                }
            }
        }
        i += 1;
    }
    btree_leave_all(&mut db.borrow_mut());
    mutex_leave(db.borrow().mutex.as_deref());
    if rc == SQLITE_OK && b_seen_busy != 0 {
        SQLITE_BUSY
    } else {
        rc
    }
}

/// Argumentos variádicos de `sqlite3_db_config()`, um por forma de chamada do C.
pub enum DbConfigArg<'a> {
    /// `SQLITE_DBCONFIG_MAINDBNAME`: o `char*` com o nome do banco principal.
    MainDbName(Vec<u8>),
    /// `SQLITE_DBCONFIG_LOOKASIDE`: buffer (`void*`), tamanho do slot e número de slots.
    Lookaside { p_buf: Option<Vec<u8>>, sz: i32, cnt: i32 },
    /// Opções de sinalizador: `onoff` (int) e o `int*` opcional que recebe o estado final.
    Flag { onoff: i32, p_res: Option<&'a mut i32> },
}

/// Configurações de uma conexão de banco de dados individual.
pub fn db_config(db: &Sqlite3Ref, op: i32, arg: DbConfigArg) -> i32 {
    /// Tabela de (opcode, máscara do bit em `sqlite3.flags` a ligar ou desligar).
    const A_FLAG_OP: [(i32, u64); 18] = [
        (SQLITE_DBCONFIG_ENABLE_FKEY, SQLITE_FOREIGN_KEYS as u64),
        (SQLITE_DBCONFIG_ENABLE_TRIGGER, SQLITE_ENABLE_TRIGGER as u64),
        (SQLITE_DBCONFIG_ENABLE_VIEW, SQLITE_ENABLE_VIEW as u64),
        (SQLITE_DBCONFIG_ENABLE_FTS3_TOKENIZER, SQLITE_FTS3_TOKENIZER as u64),
        (SQLITE_DBCONFIG_ENABLE_LOAD_EXTENSION, SQLITE_LOAD_EXTENSION as u64),
        (SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE, SQLITE_NO_CKPT_ON_CLOSE as u64),
        (SQLITE_DBCONFIG_ENABLE_QPSG, SQLITE_ENABLE_QPSG as u64),
        (SQLITE_DBCONFIG_TRIGGER_EQP, SQLITE_TRIGGER_EQP as u64),
        (SQLITE_DBCONFIG_RESET_DATABASE, SQLITE_RESET_DATABASE as u64),
        (SQLITE_DBCONFIG_DEFENSIVE, SQLITE_DEFENSIVE as u64),
        (
            SQLITE_DBCONFIG_WRITABLE_SCHEMA,
            (SQLITE_WRITE_SCHEMA as u64) | (SQLITE_NO_SCHEMA_ERROR as u64),
        ),
        (SQLITE_DBCONFIG_LEGACY_ALTER_TABLE, SQLITE_LEGACY_ALTER as u64),
        (SQLITE_DBCONFIG_DQS_DDL, SQLITE_DQS_DDL as u64),
        (SQLITE_DBCONFIG_DQS_DML, SQLITE_DQS_DML as u64),
        (SQLITE_DBCONFIG_LEGACY_FILE_FORMAT, SQLITE_LEGACY_FILE_FMT as u64),
        (SQLITE_DBCONFIG_TRUSTED_SCHEMA, SQLITE_TRUSTED_SCHEMA as u64),
        (SQLITE_DBCONFIG_STMT_SCANSTATUS, SQLITE_STMT_SCAN_STATUS as u64),
        (SQLITE_DBCONFIG_REVERSE_SCANORDER, SQLITE_REVERSE_ORDER as u64),
    ];
    let rc: i32;

    mutex_enter(db.borrow().mutex.as_deref());
    match (op, arg) {
        (SQLITE_DBCONFIG_MAINDBNAME, DbConfigArg::MainDbName(z_name)) => {
            // IMP: R-06824-28531
            // IMP: R-36257-52125
            db.borrow_mut().a_db[0].z_db_sname = Some(z_name);
            rc = SQLITE_OK;
        }
        (SQLITE_DBCONFIG_LOOKASIDE, DbConfigArg::Lookaside { p_buf, sz, cnt }) => {
            // IMP: R-26835-10964, R-47871-25994, R-04460-53386
            rc = setup_lookaside(&mut db.borrow_mut(), p_buf, sz, cnt);
        }
        (_, arg) => {
            let mut arg = arg;
            let mut r = SQLITE_ERROR; // IMP: R-42790-23372
            for &(flag_op, mask) in A_FLAG_OP.iter() {
                if flag_op == op {
                    if let DbConfigArg::Flag { onoff, p_res } = &mut arg {
                        let old_flags: u64 = db.borrow().flags;
                        if *onoff > 0 {
                            db.borrow_mut().flags |= mask;
                        } else if *onoff == 0 {
                            db.borrow_mut().flags &= !mask;
                        }
                        let new_flags: u64 = db.borrow().flags;
                        if old_flags != new_flags {
                            expire_prepared_statements(db, 0);
                        }
                        if let Some(p_res) = p_res.as_mut() {
                            **p_res = if (new_flags & mask) != 0 { 1 } else { 0 };
                        }
                        r = SQLITE_OK;
                    }
                    break;
                }
            }
            rc = r;
        }
    }
    mutex_leave(db.borrow().mutex.as_deref());
    rc
}

/// Esta é a função de comparação padrão chamada "BINARY", que está sempre disponível.
pub fn bin_coll_func(_not_used: &CallbackArg, p_key1: &[u8], p_key2: &[u8]) -> i32 {
    let n_key1 = p_key1.len() as i32;
    let n_key2 = p_key2.len() as i32;
    let n = if n_key1 < n_key2 { n_key1 } else { n_key2 } as usize;
    // EVIDENCE-OF: R-65033-28449 A collation BINARY embutida compara strings byte a byte usando
    // o memcmp() da biblioteca C padrão.
    let mut rc: i32 = 0;
    for k in 0..n {
        if p_key1[k] != p_key2[k] {
            rc = (p_key1[k] as i32) - (p_key2[k] as i32);
            break;
        }
    }
    if rc == 0 {
        rc = n_key1.wrapping_sub(n_key2);
    }
    rc
}

/// Esta é a função de comparação chamada "RTRIM", que está sempre disponível. Ignora os espaços
/// no final.
pub fn rtrim_coll_func(p_user: &CallbackArg, p_key1: &[u8], p_key2: &[u8]) -> i32 {
    let mut n_key1 = p_key1.len();
    let mut n_key2 = p_key2.len();
    while n_key1 != 0 && p_key1[n_key1 - 1] == b' ' {
        n_key1 -= 1;
    }
    while n_key2 != 0 && p_key2[n_key2 - 1] == b' ' {
        n_key2 -= 1;
    }
    bin_coll_func(p_user, &p_key1[..n_key1], &p_key2[..n_key2])
}

/// Devolve verdadeiro se a `CollSeq` é a BINARY embutida padrão.
pub fn is_binary(p: Option<&CollSeq>) -> i32 {
    match p {
        None => 1,
        Some(p) => {
            debug_assert!(match &p.x_cmp {
                Some(f) => !std::rc::Rc::ptr_eq(f, &bin_coll_func_rc()) || p.z_name == b"BINARY",
                None => true,
            });
            match &p.x_cmp {
                Some(f) => {
                    if std::rc::Rc::ptr_eq(f, &bin_coll_func_rc()) {
                        1
                    } else {
                        0
                    }
                }
                None => 0,
            }
        }
    }
}

/// Outra sequência de comparação embutida: NOCASE.
///
/// Esta sequência se destina à "comparação sem distinção de maiúsculas". O conhecimento do SQLite
/// sobre equivalentes de maiúsculas e minúsculas se estende apenas aos 26 caracteres usados no
/// idioma inglês.
///
/// No momento há apenas uma implementação UTF-8.
pub fn nocase_collating_func(_not_used: &CallbackArg, p_key1: &[u8], p_key2: &[u8]) -> i32 {
    let n_key1 = p_key1.len() as i32;
    let n_key2 = p_key2.len() as i32;
    let n = if n_key1 < n_key2 { n_key1 } else { n_key2 } as usize;
    let mut r = strnicmp(Some(p_key1), Some(p_key2), n as i32);
    if 0 == r {
        r = n_key1.wrapping_sub(n_key2);
    }
    r
}

/// Devolve o ROWID da inserção mais recente.
pub fn last_insert_rowid(db: &Sqlite3Ref) -> i64 {
    db.borrow().last_rowid
}

/// Define o valor devolvido pela função de API `sqlite3_last_insert_rowid()`.
pub fn set_last_insert_rowid(db: &Sqlite3Ref, i_rowid: i64) {
    mutex_enter(db.borrow().mutex.as_deref());
    db.borrow_mut().last_rowid = i_rowid;
    mutex_leave(db.borrow().mutex.as_deref());
}


// ---- part_003.rs ----

/// Retorna o número de mudanças da chamada mais recente a `sqlite3_exec()`.
pub fn api_changes64(db: &Sqlite3Ref) -> i64 {
    db.borrow().n_change
}

/// Versão `int` de `api_changes64()`.
pub fn api_changes(db: &Sqlite3Ref) -> i32 {
    api_changes64(db) as i32
}

/// Retorna o número de mudanças desde que o handle do banco foi aberto.
pub fn api_total_changes64(db: &Sqlite3Ref) -> i64 {
    db.borrow().n_total_change
}

/// Versão `int` de `api_total_changes64()`.
pub fn api_total_changes(db: &Sqlite3Ref) -> i32 {
    api_total_changes64(db) as i32
}

/// Fecha todos os savepoints abertos. Esta função só mexe nos campos do objeto do handle
/// do banco; não fecha savepoints que estejam abertos no nível da b-tree ou do pager.
pub fn close_savepoints(db: &mut Sqlite3) {
    while let Some(p_tmp) = db.p_savepoint.take() {
        db.p_savepoint = p_tmp.borrow_mut().p_next.take();
        // A liberação do savepoint (sqlite3DbFree) acontece no Drop de `p_tmp`.
    }
    db.n_savepoint = 0;
    db.n_statement = 0;
    db.is_transaction_savepoint = 0;
}

/// Invoca o destrutor associado ao FuncDef `p`, se houver. Exceto se esta não for a última
/// cópia da função: neste caso não o invoca. Várias cópias de uma função são criadas quando
/// `create_function()` é chamada com SQLITE_ANY como codificação.
fn function_destroy(_db: &Sqlite3Ref, p: &FuncDefRef) {
    debug_assert!((p.borrow().func_flags & SQLITE_FUNC_BUILTIN) == 0);
    let p_destructor: Option<FuncDestructorRef> = match &p.borrow().u {
        FuncDefU::PDestructor(d) => d.clone(),
        FuncDefU::PHash(_) => None,
    };
    if let Some(p_destructor) = p_destructor {
        p_destructor.borrow_mut().n_ref -= 1;
        if p_destructor.borrow().n_ref == 0 {
            let (x_destroy, p_user_data) = {
                let d = p_destructor.borrow();
                (d.x_destroy.clone(), d.p_user_data.clone())
            };
            if let Some(x_destroy) = x_destroy {
                x_destroy(&p_user_data);
            }
            // A liberação do destrutor (sqlite3DbFree) acontece no Drop.
        }
    }
}

/// Desconecta todos os objetos sqlite3_vtab que pertencem à conexão `db`. Chamada quando
/// `db` está sendo fechada.
fn disconnect_all_vtab(db: &Sqlite3Ref) {
    btree_enter_all(&mut db.borrow_mut());
    let n_db = db.borrow().n_db;
    for i in 0..n_db {
        let p_schema = db.borrow().a_db[i as usize].p_schema.clone();
        if let Some(p_schema) = p_schema {
            let mut p = hash_first(&p_schema.borrow().tbl_hash);
            while let Some(elem) = p {
                let p_tab: Option<TableRef> = elem.borrow().data.downcast_ref::<TableRef>().cloned();
                if let Some(p_tab) = p_tab {
                    let virtual_tab = is_virtual(&p_tab.borrow());
                    if virtual_tab {
                        vtab_disconnect(db, &mut p_tab.borrow_mut());
                    }
                }
                p = hash_next(&elem.borrow());
            }
        }
    }
    let mut p = hash_first(&db.borrow().a_module);
    while let Some(elem) = p {
        let p_mod: Option<ModuleRef> = elem.borrow().data.downcast_ref::<ModuleRef>().cloned();
        if let Some(p_mod) = p_mod {
            let p_epo_tab = p_mod.borrow().p_epo_tab.clone();
            if let Some(p_epo_tab) = p_epo_tab {
                vtab_disconnect(db, &mut p_epo_tab.borrow_mut());
            }
        }
        p = hash_next(&elem.borrow());
    }
    vtab_unlock_list(db);
    btree_leave_all(&mut db.borrow_mut());
}

/// Retorna verdadeiro se a conexão `db` tem instruções preparadas não finalizadas ou
/// objetos sqlite3_backup não terminados.
fn connection_is_busy(db: &Sqlite3Ref) -> i32 {
    if db.borrow().p_vdbe.is_some() {
        return 1;
    }
    let n_db = db.borrow().n_db;
    for j in 0..n_db {
        let p_bt = db.borrow().a_db[j as usize].p_bt.clone();
        if let Some(p_bt) = p_bt {
            if btree_is_in_backup(p_bt) != 0 {
                return 1;
            }
        }
    }
    0
}

/// Fecha um banco de dados SQLite existente.
fn close(db: Option<&Sqlite3Ref>, force_zombie: i32) -> i32 {
    let db = match db {
        None => {
            // EVIDENCE-OF: R-63257-11740 Chamar sqlite3_close() ou sqlite3_close_v2() com
            // ponteiro NULL é um no-op inofensivo.
            return SQLITE_OK;
        }
        Some(db) => db,
    };
    if safety_check_sick_or_ok(&db.borrow()) == 0 {
        return sqlite_misuse_bkpt(line!() as i32);
    }
    mutex_enter(db.borrow().mutex.as_deref());
    if (db.borrow().m_trace as u32 & SQLITE_TRACE_CLOSE) != 0 {
        let (trace, p_trace_arg) = {
            let d = db.borrow();
            (d.trace.clone(), d.p_trace_arg.clone())
        };
        if let Sqlite3Trace::V2(x_v2) = trace {
            x_v2(SQLITE_TRACE_CLOSE, &p_trace_arg, db, &());
        }
    }

    // Força chamadas xDisconnect em todas as tabelas virtuais.
    disconnect_all_vtab(db);

    // Se uma transação está aberta, a chamada a disconnect_all_vtab() acima não terá chamado
    // o método xDisconnect() de nenhuma tabela virtual do array aVTrans[]. A chamada a
    // vtab_rollback() abaixo o faz. Precisa vir antes da verificação de instruções SQL
    // ativas, pois a implementação da v-table pode guardar instruções preparadas internamente.
    vtab_rollback(db);

    // O comportamento legado (sqlite3_close()) é devolver SQLITE_BUSY se a conexão não pode
    // ser fechada imediatamente.
    if force_zombie == 0 && connection_is_busy(db) != 0 {
        error_with_msg(
            &mut db.borrow_mut(),
            SQLITE_BUSY,
            Some(b"unable to close due to unfinalized statements or unfinished backups"),
            &[],
        );
        mutex_leave(db.borrow().mutex.as_deref());
        return SQLITE_BUSY;
    }

    loop {
        let p_data = db.borrow_mut().p_db_data.take();
        let mut p = match p_data {
            None => break,
            Some(p) => p,
        };
        let p_next = p.p_next.take();
        db.borrow_mut().p_db_data = p_next;
        // O destrutor do dado de cliente roda no Drop, depois de soltar o empréstimo de `db`.
        drop(p);
    }

    // Converte a conexão em zumbi e a fecha.
    db.borrow_mut().e_open_state = SQLITE_STATE_ZOMBIE;
    leave_mutex_and_close_zombie(db);
    SQLITE_OK
}

/// Retorna o estado de transação de um único banco ou o estado máximo de transação de
/// todos os bancos anexados se `z_schema` é nulo.
pub fn api_txn_state(db: &Sqlite3Ref, z_schema: Option<&[u8]>) -> i32 {
    let mut i_db: i32;
    let n_db: i32;
    let mut i_txn: i32 = -1;
    mutex_enter(db.borrow().mutex.as_deref());
    if let Some(z_schema) = z_schema {
        i_db = find_db_name(&db.borrow(), Some(z_schema));
        let mut n = i_db;
        if i_db < 0 {
            n -= 1;
        }
        n_db = n;
    } else {
        i_db = 0;
        n_db = db.borrow().n_db - 1;
    }
    while i_db <= n_db {
        let p_bt = db.borrow().a_db[i_db as usize].p_bt.clone();
        let x = if p_bt.is_some() {
            btree_txn_state(p_bt.as_ref())
        } else {
            SQLITE_TXN_NONE
        };
        if x > i_txn {
            i_txn = x;
        }
        i_db += 1;
    }
    mutex_leave(db.borrow().mutex.as_deref());
    i_txn
}

/// Duas variações da interface pública para fechar uma conexão. A versão `sqlite3_close()`
/// devolve SQLITE_BUSY e deixa a conexão aberta se há instruções preparadas não finalizadas
/// ou sqlite3_backups não terminados. A versão `sqlite3_close_v2()` força a conexão a virar
/// zumbi se há recursos não fechados, e providencia a liberação quando a última instrução
/// preparada ou backup for fechado.
pub fn api_close(db: Option<&Sqlite3Ref>) -> i32 {
    close(db, 0)
}

/// Variante `v2` de `api_close()`: força o estado zumbi se houver recursos abertos.
pub fn api_close_v2(db: Option<&Sqlite3Ref>) -> i32 {
    close(db, 1)
}

/// Fecha o mutex da conexão `db`.
///
/// Além disso, se `db` é um zumbi (houve uma chamada anterior a sqlite3_close(db) ou
/// sqlite3_close_v2(db)) e toda sqlite3_stmt foi finalizada e todo sqlite3_backup terminou,
/// libera todos os recursos.
pub fn leave_mutex_and_close_zombie(db: &Sqlite3Ref) {
    // Se há objetos sqlite3_stmt ou sqlite3_backup pendentes, ou se a conexão ainda não foi
    // fechada por sqlite3_close_v2(), apenas solta o mutex e retorna.
    if db.borrow().e_open_state != SQLITE_STATE_ZOMBIE || connection_is_busy(db) != 0 {
        mutex_leave(db.borrow().mutex.as_deref());
        return;
    }

    // Neste ponto a conexão fechou todos os objetos sqlite3_stmt e sqlite3_backup e foi
    // passada a sqlite3_close (é um zumbi). Então libera todos os recursos.

    // Se uma transação está aberta, desfaz. Isto também garante que, se algum schema foi
    // modificado por uma transação não confirmada, ele seja reiniciado. E que o mutex
    // necessário da b-tree seja mantido para tornar atômicos o rollback do pager e o reset
    // do schema.
    rollback_all(db, SQLITE_OK);

    // Libera as estruturas Savepoint pendentes.
    close_savepoints(&mut db.borrow_mut());

    // Fecha todas as conexões de banco de dados.
    let n_db = db.borrow().n_db;
    for j in 0..n_db {
        let p_bt = db.borrow_mut().a_db[j as usize].p_bt.take();
        if let Some(p_bt) = p_bt {
            btree_close(&mut p_bt.borrow_mut());
            if j != 1 {
                db.borrow_mut().a_db[j as usize].p_schema = None;
            }
        }
    }
    // Limpa o schema TEMP separadamente e por último.
    let p_temp_schema = db.borrow().a_db[1].p_schema.clone();
    if let Some(p_temp_schema) = p_temp_schema {
        schema_clear(&mut p_temp_schema.borrow_mut());
    }
    vtab_unlock_list(db);

    // Libera o array de bancos auxiliares.
    collapse_database_array(&mut db.borrow_mut());
    debug_assert!(db.borrow().n_db <= 2);

    // Avisa o código de notify.c que a conexão não mantém mais nenhum lock e não precisa de
    // mais callbacks de unlock-notify.
    connection_closed(db);

    let mut i = hash_first(&db.borrow().a_func);
    while let Some(elem) = i {
        let mut p: Option<FuncDefRef> = elem.borrow().data.downcast_ref::<FuncDefRef>().cloned();
        while let Some(cur) = p {
            function_destroy(db, &cur);
            let p_next = cur.borrow().p_next.clone();
            // sqlite3DbFree(db, p): a liberação acontece no Drop.
            p = p_next;
        }
        i = hash_next(&elem.borrow());
    }
    hash_clear(&mut db.borrow_mut().a_func);
    let mut i = hash_first(&db.borrow().a_coll_seq);
    while let Some(elem) = i {
        let p_coll: Option<Vec<CollSeqRef>> =
            elem.borrow().data.downcast_ref::<Vec<CollSeqRef>>().cloned();
        if let Some(p_coll) = p_coll {
            // Invoca os destrutores registrados para os dados de usuário da collation.
            for j in 0..3 {
                let (x_del, p_user) = {
                    let c = p_coll[j].borrow();
                    (c.x_del.clone(), c.p_user.clone())
                };
                if let Some(x_del) = x_del {
                    x_del(&p_user);
                }
            }
            // sqlite3DbFree(db, pColl): a liberação acontece no Drop.
        }
        i = hash_next(&elem.borrow());
    }
    hash_clear(&mut db.borrow_mut().a_coll_seq);
    let mut i = hash_first(&db.borrow().a_module);
    while let Some(elem) = i {
        let p_mod: Option<ModuleRef> = elem.borrow().data.downcast_ref::<ModuleRef>().cloned();
        if let Some(p_mod) = p_mod {
            vtab_eponymous_table_clear(&mut db.borrow_mut(), &p_mod);
            vtab_module_unref(db, &p_mod);
        }
        i = hash_next(&elem.borrow());
    }
    hash_clear(&mut db.borrow_mut().a_module);

    // Desaloca quaisquer strings de erro em cache.
    error(&mut db.borrow_mut(), SQLITE_OK);
    value_free(db.borrow_mut().p_err.take());
    close_extensions(&db.borrow());

    db.borrow_mut().e_open_state = SQLITE_STATE_ERROR;

    // O schema do banco temporário é alocado de forma diferente dos demais objetos de schema
    // (com sqliteMalloc() direto, em vez de sqlite3BtreeSchema()). Por isso precisa ser
    // liberado aqui.
    db.borrow_mut().a_db[1].p_schema = None;
    let (x_autovac_destr, p_autovac_pages_arg) = {
        let d = db.borrow();
        (d.x_autovac_destr.clone(), d.p_autovac_pages_arg.clone())
    };
    if let Some(x_autovac_destr) = x_autovac_destr {
        x_autovac_destr(&p_autovac_pages_arg);
    }
    mutex_leave(db.borrow().mutex.as_deref());
    db.borrow_mut().e_open_state = SQLITE_STATE_CLOSED;
    let p_mutex = db.borrow_mut().mutex.take();
    mutex_free(p_mutex);
    debug_assert!(lookaside_used(&db.borrow(), None) == 0);
    // Lookaside e a própria conexão (sqlite3_free) são liberados no Drop.
}

/// Desfaz todos os arquivos de banco. Se `trip_code` não é SQLITE_OK, qualquer cursor de
/// escrita é invalidado ("tripped", como disparar um disjuntor) e passa a devolver
/// `trip_code` em qualquer tentativa de uso. Cursores de leitura continuam abertos e
/// válidos, mas são "salvos" caso as páginas da tabela se movam.
pub fn rollback_all(db: &Sqlite3Ref, trip_code: i32) {
    let mut in_trans: i32 = 0;
    begin_benign_malloc();

    // Obtém todos os mutexes de b-tree antes de qualquer chamada a btree_rollback(). Isto é
    // importante caso a transação desfeita tenha modificado o schema do banco. Se os mutexes
    // não fossem tomados aqui, outra conexão de cache compartilhado poderia entrar entre o
    // rollback do banco e o reset do schema, o que pode causar falsos relatos de corrupção.
    btree_enter_all(&mut db.borrow_mut());
    let schema_change = {
        let d = db.borrow();
        (d.m_db_flags & DBFLAG_SCHEMA_CHANGE) != 0 && d.init.busy == 0
    };

    let n_db = db.borrow().n_db;
    for i in 0..n_db {
        let p = db.borrow().a_db[i as usize].p_bt.clone();
        if let Some(p) = p {
            if btree_txn_state(Some(&p)) == SQLITE_TXN_WRITE {
                in_trans = 1;
            }
            btree_rollback(&mut p.borrow_mut(), trip_code, (!schema_change) as i32);
        }
    }
    vtab_rollback(db);
    end_benign_malloc();

    if schema_change {
        expire_prepared_statements(db, 0);
        reset_all_schemas_of_connection(&mut db.borrow_mut());
    }
    btree_leave_all(&mut db.borrow_mut());

    // Quaisquer violações de restrição adiadas foram resolvidas.
    {
        let mut d = db.borrow_mut();
        d.n_deferred_cons = 0;
        d.n_deferred_imm_cons = 0;
        d.flags &= !(SQLITE_DEFER_FKS | SQLITE_CORRUPT_RD_ONLY);
    }

    // Se um foi configurado, invoca o callback do rollback-hook.
    let (x_rollback_callback, p_rollback_arg, auto_commit) = {
        let d = db.borrow();
        (d.x_rollback_callback.clone(), d.p_rollback_arg.clone(), d.auto_commit)
    };
    if let Some(x_rollback_callback) = x_rollback_callback {
        if in_trans != 0 || auto_commit == 0 {
            x_rollback_callback(&p_rollback_arg);
        }
    }
}


// ---- part_004.rs ----

/// Devolve a mensagem estática que descreve o tipo de erro do código `rc`.
/// (`sqlite3ErrName` só existe com `SQLITE_NEED_ERR_NAME`, que o Debian não define.)
pub fn err_str(rc: i32) -> &'static [u8] {
    static A_MSG: [Option<&'static [u8]>; 29] = [
        /* SQLITE_OK          */ Some(b"not an error"),
        /* SQLITE_ERROR       */ Some(b"SQL logic error"),
        /* SQLITE_INTERNAL    */ None,
        /* SQLITE_PERM        */ Some(b"access permission denied"),
        /* SQLITE_ABORT       */ Some(b"query aborted"),
        /* SQLITE_BUSY        */ Some(b"database is locked"),
        /* SQLITE_LOCKED      */ Some(b"database table is locked"),
        /* SQLITE_NOMEM       */ Some(b"out of memory"),
        /* SQLITE_READONLY    */ Some(b"attempt to write a readonly database"),
        /* SQLITE_INTERRUPT   */ Some(b"interrupted"),
        /* SQLITE_IOERR       */ Some(b"disk I/O error"),
        /* SQLITE_CORRUPT     */ Some(b"database disk image is malformed"),
        /* SQLITE_NOTFOUND    */ Some(b"unknown operation"),
        /* SQLITE_FULL        */ Some(b"database or disk is full"),
        /* SQLITE_CANTOPEN    */ Some(b"unable to open database file"),
        /* SQLITE_PROTOCOL    */ Some(b"locking protocol"),
        /* SQLITE_EMPTY       */ None,
        /* SQLITE_SCHEMA      */ Some(b"database schema has changed"),
        /* SQLITE_TOOBIG      */ Some(b"string or blob too big"),
        /* SQLITE_CONSTRAINT  */ Some(b"constraint failed"),
        /* SQLITE_MISMATCH    */ Some(b"datatype mismatch"),
        /* SQLITE_MISUSE      */ Some(b"bad parameter or other API misuse"),
        /* SQLITE_NOLFS       */ None,
        /* SQLITE_AUTH        */ Some(b"authorization denied"),
        /* SQLITE_FORMAT      */ None,
        /* SQLITE_RANGE       */ Some(b"column index out of range"),
        /* SQLITE_NOTADB      */ Some(b"file is not a database"),
        /* SQLITE_NOTICE      */ Some(b"notification message"),
        /* SQLITE_WARNING     */ Some(b"warning message"),
    ];
    let mut z_err: &'static [u8] = b"unknown error";
    match rc {
        SQLITE_ABORT_ROLLBACK => {
            z_err = b"abort due to ROLLBACK";
        }
        SQLITE_ROW => {
            z_err = b"another row available";
        }
        SQLITE_DONE => {
            z_err = b"no more rows available";
        }
        _ => {
            let rc = rc & 0xff;
            if rc >= 0 && rc < array_size(&A_MSG) {
                if let Some(msg) = A_MSG[rc as usize] {
                    z_err = msg;
                }
            }
        }
    }
    z_err
}

/// Busy callback que dorme e tenta de novo até estourar o timeout. O argumento
/// opaco é um `Weak<RefCell<Sqlite3>>` para a conexão (evita ciclo de `Rc`).
/// Devolve não zero para tentar o lock de novo, zero para desistir e o SQLite
/// devolver `SQLITE_BUSY`.
fn sqlite_default_busy_callback(ptr: Option<&std::rc::Rc<dyn std::any::Any>>, count: i32) -> i32 {
    // Caso para sistemas que dormem frações de segundo (unix com nanosleep()).
    static DELAYS: [u8; 12] = [1, 2, 5, 10, 15, 20, 25, 25, 25, 50, 50, 100];
    static TOTALS: [u8; 12] = [0, 1, 3, 8, 18, 33, 53, 78, 103, 128, 178, 228];
    const NDELAY: i32 = 12;

    let db: Sqlite3Ref = match ptr
        .and_then(|p| p.downcast_ref::<std::rc::Weak<std::cell::RefCell<Sqlite3>>>())
        .and_then(|w| w.upgrade())
    {
        Some(db) => db,
        None => return 0,
    };
    let (tmout, p_vfs) = {
        let db_b = db.borrow();
        (db_b.busy_timeout, db_b.p_vfs.clone())
    };
    let mut delay: i32;
    let prior: i32;

    assert!(count >= 0);
    if count < NDELAY {
        delay = DELAYS[count as usize] as i32;
        prior = TOTALS[count as usize] as i32;
    } else {
        delay = DELAYS[(NDELAY - 1) as usize] as i32;
        prior = (TOTALS[(NDELAY - 1) as usize] as i32)
            .wrapping_add(delay.wrapping_mul(count.wrapping_sub(NDELAY - 1)));
    }
    if prior.wrapping_add(delay) > tmout {
        delay = tmout.wrapping_sub(prior);
        if delay <= 0 {
            return 0;
        }
    }
    if let Some(vfs) = p_vfs {
        os_sleep(vfs.as_ref(), delay.wrapping_mul(1000));
    }
    1
}

/// Invoca o busy handler dado. Chamada quando uma operação falhou ao obter um
/// lock num arquivo do VFS. Se devolver não zero o lock é tentado de novo; se
/// devolver 0 a operação aborta com `SQLITE_BUSY`.
pub fn invoke_busy_handler(p: &mut BusyHandler) -> i32 {
    if p.x_busy_handler.is_none() || p.n_busy < 0 {
        return 0;
    }
    let x_busy_handler = p.x_busy_handler.clone();
    let rc = match x_busy_handler {
        Some(f) => f(p.p_busy_arg.as_ref(), p.n_busy),
        None => 0,
    };
    if rc == 0 {
        p.n_busy = -1;
    } else {
        p.n_busy += 1;
    }
    rc
}

/// Instala o callback de busy da conexão com a função e o argumento dados.
pub fn busy_handler(
    db: &Sqlite3Ref,
    x_busy: Option<std::rc::Rc<dyn Fn(Option<&std::rc::Rc<dyn std::any::Any>>, i32) -> i32>>,
    p_arg: Option<std::rc::Rc<dyn std::any::Any>>,
) -> i32 {
    let mutex = db.borrow().mutex.clone();
    mutex_enter(mutex.as_ref());
    {
        let mut db_b = db.borrow_mut();
        db_b.busy_handler.x_busy_handler = x_busy;
        db_b.busy_handler.p_busy_arg = p_arg;
        db_b.busy_handler.n_busy = 0;
        db_b.busy_timeout = 0;
    }
    mutex_leave(mutex.as_ref());
    SQLITE_OK
}

/// Instala o callback de progresso da conexão; é invocado a cada `n_ops` opcodes.
pub fn progress_handler(
    db: &Sqlite3Ref,
    n_ops: i32,
    x_progress: Option<std::rc::Rc<dyn Fn(&CallbackArg) -> i32>>,
    p_arg: CallbackArg,
) {
    let mutex = db.borrow().mutex.clone();
    mutex_enter(mutex.as_ref());
    {
        let mut db_b = db.borrow_mut();
        if n_ops > 0 {
            db_b.x_progress = x_progress;
            db_b.n_progress_ops = n_ops as u32;
            db_b.p_progress_arg = p_arg;
        } else {
            db_b.x_progress = None;
            db_b.n_progress_ops = 0;
            db_b.p_progress_arg = None;
        }
    }
    mutex_leave(mutex.as_ref());
}

/// Instala o busy handler padrão, que espera `ms` milissegundos antes de devolver 0.
pub fn busy_timeout(db: &Sqlite3Ref, ms: i32) -> i32 {
    if ms > 0 {
        let arg: std::rc::Rc<dyn std::any::Any> = std::rc::Rc::new(std::rc::Rc::downgrade(db));
        busy_handler(
            db,
            Some(std::rc::Rc::new(sqlite_default_busy_callback)),
            Some(arg),
        );
        db.borrow_mut().busy_timeout = ms;
    } else {
        busy_handler(db, None, None);
    }
    SQLITE_OK
}

/// Faz qualquer operação pendente parar o quanto antes.
pub fn interrupt(db: &Sqlite3Ref) {
    db.borrow_mut().is_interrupted = 1;
}

/// Diz se há uma interrupção pendente na conexão.
pub fn is_interrupted(db: &Sqlite3Ref) -> i32 {
    (db.borrow().is_interrupted != 0) as i32
}


// ---- part_005.rs ----

/// Esta função é exatamente igual a `sqlite3_create_function()`, exceto que é projetada para ser
/// chamada por código interno. A diferença é que, se um malloc() falha em
/// `sqlite3_create_function()`, um código de erro é retornado e o sinalizador `mallocFailed` é
/// limpo.
pub fn create_func(
    db: &Sqlite3Ref,
    z_function_name: Option<&[u8]>,
    n_arg: i32,
    enc: i32,
    p_user_data: CallbackArg,
    x_s_func: Option<XSFunc>,
    x_step: Option<XSFunc>,
    x_final: Option<XFinalFunc>,
    x_value: Option<XFinalFunc>,
    x_inverse: Option<XSFunc>,
    p_destructor: Option<FuncDestructorRef>,
) -> i32 {
    let mut enc = enc;

    debug_assert!(x_value.is_none() || x_s_func.is_none());
    let z_function_name = match z_function_name {
        // O nome precisa ser válido.
        None => return sqlite_misuse_bkpt(line!() as i32),
        Some(z) => z,
    };
    if
        // Não pode ter xSFunc e xFinal ao mesmo tempo.
        (x_s_func.is_some() && x_final.is_some())
        // Ambos ou nenhum de xFinal e xStep.
        || (x_final.is_none() != x_step.is_none())
        // Ambos ou nenhum de xValue e xInverse.
        || (x_value.is_none() != x_inverse.is_none())
        || (n_arg < -1 || n_arg > SQLITE_MAX_FUNCTION_ARG)
        || (255 < strlen30(z_function_name))
    {
        return sqlite_misuse_bkpt(line!() as i32);
    }

    debug_assert!(SQLITE_FUNC_CONSTANT as i32 == SQLITE_DETERMINISTIC);
    debug_assert!(SQLITE_FUNC_DIRECT as i32 == SQLITE_DIRECTONLY);
    let mut extra_flags: i32 = enc
        & (SQLITE_DETERMINISTIC
            | SQLITE_DIRECTONLY
            | SQLITE_SUBTYPE
            | SQLITE_INNOCUOUS
            | SQLITE_RESULT_SUBTYPE);
    enc &= SQLITE_FUNC_ENCMASK as i32 | SQLITE_ANY;

    // O sinalizador SQLITE_INNOCUOUS é o mesmo bit de SQLITE_FUNC_UNSAFE, mas com o significado
    // invertido. Então inverte-se o bit.
    debug_assert!(SQLITE_FUNC_UNSAFE as i32 == SQLITE_INNOCUOUS);
    extra_flags ^= SQLITE_FUNC_UNSAFE as i32; // tag-20230109-1

    // Se SQLITE_UTF16 é especificado como tipo de codificação, transforma-o em SQLITE_UTF16LE ou
    // SQLITE_UTF16BE usando SQLITE_UTF16NATIVE. SQLITE_UTF16 não é usado internamente.
    //
    // Se SQLITE_ANY é especificado, adiciona três versões da função à tabela hash.
    match enc {
        SQLITE_UTF16 => {
            enc = SQLITE_UTF16NATIVE;
        }
        SQLITE_ANY => {
            let mut rc: i32;
            rc = create_func(
                db,
                Some(z_function_name),
                n_arg,
                (SQLITE_UTF8 | extra_flags) ^ SQLITE_FUNC_UNSAFE as i32, // tag-20230109-1
                p_user_data.clone(),
                x_s_func.clone(),
                x_step.clone(),
                x_final.clone(),
                x_value.clone(),
                x_inverse.clone(),
                p_destructor.clone(),
            );
            if rc == SQLITE_OK {
                rc = create_func(
                    db,
                    Some(z_function_name),
                    n_arg,
                    (SQLITE_UTF16LE | extra_flags) ^ SQLITE_FUNC_UNSAFE as i32, // tag-20230109-1
                    p_user_data.clone(),
                    x_s_func.clone(),
                    x_step.clone(),
                    x_final.clone(),
                    x_value.clone(),
                    x_inverse.clone(),
                    p_destructor.clone(),
                );
            }
            if rc != SQLITE_OK {
                return rc;
            }
            enc = SQLITE_UTF16BE;
        }
        SQLITE_UTF8 | SQLITE_UTF16LE | SQLITE_UTF16BE => {}
        _ => {
            enc = SQLITE_UTF8;
        }
    }

    // Verifica se uma função existente está sendo sobrescrita ou apagada. Se estiver, e houver
    // VMs ativas, retorna SQLITE_BUSY. Se estiver sendo sobrescrita/apagada mas não houver VMs
    // ativas, permite que a operação continue mas invalida todas as instruções pré-compiladas.
    let mut p = find_function(db, z_function_name, n_arg, enc as u8, false);
    let existing = match &p {
        Some(f) => {
            let f = f.borrow();
            (f.func_flags & SQLITE_FUNC_ENCMASK) == enc as u32 && f.n_arg as i32 == n_arg
        }
        None => false,
    };
    if existing {
        if db.borrow().n_vdbe_active != 0 {
            error_with_msg(
                db,
                SQLITE_BUSY,
                Some(b"unable to delete/modify user-function due to active statements"),
            );
            debug_assert!(db.borrow().malloc_failed == 0);
            return SQLITE_BUSY;
        } else {
            expire_prepared_statements(db, 0);
        }
    } else if x_s_func.is_none() && x_final.is_none() {
        // Tentando apagar uma função que não existe. É um no-op.
        // https://sqlite.org/forum/forumpost/726219164b
        return SQLITE_OK;
    }

    p = find_function(db, z_function_name, n_arg, enc as u8, true);
    debug_assert!(p.is_some() || db.borrow().malloc_failed != 0);
    let p = match p {
        None => return sqlite_nomem_bkpt(line!() as i32),
        Some(p) => p,
    };

    // Se uma versão mais antiga da função com destrutor configurado está sendo substituída,
    // invoca a função destrutora aqui.
    function_destroy(db, &p);

    if let Some(d) = &p_destructor {
        d.borrow_mut().n_ref += 1;
    }
    let mut f = p.borrow_mut();
    f.u = FuncDefU::PDestructor(p_destructor);
    f.func_flags = (f.func_flags & SQLITE_FUNC_ENCMASK) | extra_flags as u32;
    f.x_s_func = match x_s_func {
        Some(x) => Some(x),
        None => x_step,
    };
    f.x_finalize = x_final;
    f.x_value = x_value;
    f.x_inverse = x_inverse;
    f.p_user_data = p_user_data;
    f.n_arg = n_arg as i16;
    SQLITE_OK
}

/// Função auxiliar usada pelas APIs UTF-8 que criam novas funções:
///
///    sqlite3_create_function()
///    sqlite3_create_function_v2()
///    sqlite3_create_window_function()
fn create_function_api(
    db: &Sqlite3Ref,
    z_func: Option<&[u8]>,
    n_arg: i32,
    enc: i32,
    p: CallbackArg,
    x_s_func: Option<XSFunc>,
    x_step: Option<XSFunc>,
    x_final: Option<XFinalFunc>,
    x_value: Option<XFinalFunc>,
    x_inverse: Option<XSFunc>,
    x_destroy: Option<Rc<dyn Fn(&CallbackArg)>>,
) -> i32 {
    let mut rc: i32;
    let mut p_arg: Option<FuncDestructorRef> = None;

    let mutex = db.borrow().mutex.clone();
    mutex_enter(mutex.as_ref());
    if x_destroy.is_some() {
        // No C, a falha de sqlite3Malloc() aqui chama oom_fault, xDestroy(p) e salta para `out`.
        // A alocação de `Rc` em Rust não falha, então só o ramo de sucesso existe.
        p_arg = Some(Rc::new(RefCell::new(FuncDestructor {
            n_ref: 0,
            x_destroy: x_destroy.clone(),
            p_user_data: p.clone(),
        })));
    }
    rc = create_func(
        db,
        z_func,
        n_arg,
        enc,
        p.clone(),
        x_s_func,
        x_step.clone(),
        x_final.clone(),
        x_value,
        x_inverse,
        p_arg.clone(),
    );
    if let Some(arg) = &p_arg {
        if arg.borrow().n_ref == 0 {
            debug_assert!(rc != SQLITE_OK || (x_step.is_none() && x_final.is_none()));
            if let Some(x_destroy) = &x_destroy {
                x_destroy(&p);
            }
            drop(p_arg);
        }
    }

    rc = api_exit(db, rc);
    mutex_leave(mutex.as_ref());
    rc
}

/// Cria novas funções de usuário.
pub fn create_function(
    db: &Sqlite3Ref,
    z_func: Option<&[u8]>,
    n_arg: i32,
    enc: i32,
    p: CallbackArg,
    x_s_func: Option<XSFunc>,
    x_step: Option<XSFunc>,
    x_final: Option<XFinalFunc>,
) -> i32 {
    create_function_api(
        db, z_func, n_arg, enc, p, x_s_func, x_step, x_final, None, None, None,
    )
}

pub fn create_function_v2(
    db: &Sqlite3Ref,
    z_func: Option<&[u8]>,
    n_arg: i32,
    enc: i32,
    p: CallbackArg,
    x_s_func: Option<XSFunc>,
    x_step: Option<XSFunc>,
    x_final: Option<XFinalFunc>,
    x_destroy: Option<Rc<dyn Fn(&CallbackArg)>>,
) -> i32 {
    create_function_api(
        db, z_func, n_arg, enc, p, x_s_func, x_step, x_final, None, None, x_destroy,
    )
}

pub fn create_window_function(
    db: &Sqlite3Ref,
    z_func: Option<&[u8]>,
    n_arg: i32,
    enc: i32,
    p: CallbackArg,
    x_step: Option<XSFunc>,
    x_final: Option<XFinalFunc>,
    x_value: Option<XFinalFunc>,
    x_inverse: Option<XSFunc>,
    x_destroy: Option<Rc<dyn Fn(&CallbackArg)>>,
) -> i32 {
    create_function_api(
        db, z_func, n_arg, enc, p, None, x_step, x_final, x_value, x_inverse, x_destroy,
    )
}

pub fn create_function16(
    db: &Sqlite3Ref,
    z_function_name: &[u8],
    n_arg: i32,
    e_text_rep: i32,
    p: CallbackArg,
    x_s_func: Option<XSFunc>,
    x_step: Option<XSFunc>,
    x_final: Option<XFinalFunc>,
) -> i32 {
    let mut rc: i32;

    let mutex = db.borrow().mutex.clone();
    mutex_enter(mutex.as_ref());
    debug_assert!(db.borrow().malloc_failed == 0);
    let z_func8 = utf16to8(db, z_function_name, -1, SQLITE_UTF16NATIVE);
    rc = create_func(
        db,
        z_func8.as_deref(),
        n_arg,
        e_text_rep,
        p,
        x_s_func,
        x_step,
        x_final,
        None,
        None,
        None,
    );
    drop(z_func8);
    rc = api_exit(db, rc);
    mutex_leave(mutex.as_ref());
    rc
}

/// A seguir está a implementação de uma função SQL que sempre falha com uma mensagem de erro
/// dizendo que a função é usada no contexto errado. A API `sqlite3_overload_function()` pode
/// construir funções SQL que usam esta rotina, de modo que elas existam para a resolução de nomes
/// mas sejam de fato sobrecarregadas pelo método xFindFunction de tabelas virtuais.
fn invalid_function(
    context: &mut Sqlite3Context,
    _not_used: &[Sqlite3ValueRef],
) {
    let z_name: Vec<u8> = match user_data(context) {
        Some(any) => match any.downcast_ref::<Vec<u8>>() {
            Some(v) => v.clone(),
            None => Vec::new(),
        },
        None => Vec::new(),
    };
    let mut z_err: Vec<u8> = Vec::new();
    z_err.extend_from_slice(b"unable to use function ");
    z_err.extend_from_slice(&z_name);
    z_err.extend_from_slice(b" in the requested context");
    result_error(context, &z_err, -1);
}

/// Declara que uma função foi sobrecarregada por uma tabela virtual.
///
/// Se a função já existe como função global regular, esta rotina é um no-op. Se a função não
/// existe, cria uma nova que sempre lança um erro em tempo de execução.
///
/// Quando tabelas virtuais pretendem fornecer uma função sobrecarregada, devem chamar esta rotina
/// para garantir que a função global exista. Uma função global precisa existir para que a
/// resolução de nomes funcione corretamente.
pub fn overload_function(db: &Sqlite3Ref, z_name: &[u8], n_arg: i32) -> i32 {
    let rc: bool;

    let mutex = db.borrow().mutex.clone();
    mutex_enter(mutex.as_ref());
    rc = find_function(db, z_name, n_arg, SQLITE_UTF8 as u8, false).is_some();
    mutex_leave(mutex.as_ref());
    if rc {
        return SQLITE_OK;
    }
    let z_copy: Vec<u8> = z_name.to_vec();
    let x_invalid: XSFunc = Rc::new(invalid_function);
    // O destrutor do C é sqlite3_free(zCopy); aqui a cópia é liberada quando o último `Rc` cai.
    let x_destroy: Rc<dyn Fn(&CallbackArg)> = Rc::new(|_: &CallbackArg| {});
    create_function_v2(
        db,
        Some(z_name),
        n_arg,
        SQLITE_UTF8,
        Some(Rc::new(z_copy)),
        Some(x_invalid),
        None,
        None,
        Some(x_destroy),
    )
}

/// Registra uma função de rastreamento. O pArg do rastreamento registrado anteriormente é
/// retornado.
///
/// Uma função de rastreamento NULL significa que nenhum rastreamento é executado. Uma função
/// não NULL é invocada no início de cada instrução SQL.
pub fn trace(
    db: &Sqlite3Ref,
    x_trace: Option<Rc<dyn Fn(&CallbackArg, &[u8])>>,
    p_arg: CallbackArg,
) -> CallbackArg {
    let mutex = db.borrow().mutex.clone();
    mutex_enter(mutex.as_ref());
    let mut d = db.borrow_mut();
    let p_old = d.p_trace_arg.clone();
    d.m_trace = if x_trace.is_some() { SQLITE_TRACE_LEGACY } else { 0 };
    d.trace = match x_trace {
        Some(x) => Sqlite3Trace::Legacy(x),
        None => Sqlite3Trace::None,
    };
    d.p_trace_arg = p_arg;
    drop(d);
    mutex_leave(mutex.as_ref());
    p_old
}

/// Registra um callback de rastreamento usando a interface versão 2.
pub fn trace_v2(
    db: &Sqlite3Ref,
    m_trace: u32,                                                       // Máscara de eventos a rastrear
    x_trace: Option<Rc<dyn Fn(u32, &CallbackArg, &dyn Any, &dyn Any) -> i32>>, // Callback a invocar
    p_arg: CallbackArg,                                                 // Contexto
) -> i32 {
    let mut m_trace = m_trace;
    let mut x_trace = x_trace;
    let mutex = db.borrow().mutex.clone();
    mutex_enter(mutex.as_ref());
    if m_trace == 0 {
        x_trace = None;
    }
    if x_trace.is_none() {
        m_trace = 0;
    }
    let mut d = db.borrow_mut();
    d.m_trace = m_trace as u8;
    d.trace = match x_trace {
        Some(x) => Sqlite3Trace::V2(x),
        None => Sqlite3Trace::None,
    };
    d.p_trace_arg = p_arg;
    drop(d);
    mutex_leave(mutex.as_ref());
    SQLITE_OK
}


// ---- part_006.rs ----

/// Registra uma função de profiling. Devolve o `pArg` registrado antes.
///
/// Uma função nula significa que nenhum profiling é executado; uma função não nula é invocada
/// ao fim de cada instrução SQL executada.
pub fn profile(
    db: &Sqlite3Ref,
    x_profile: Option<Rc<dyn Fn(&CallbackArg, &[u8], u64)>>,
    p_arg: CallbackArg,
) -> CallbackArg {
    let mutex = db.borrow().mutex.clone();
    mutex_enter(mutex.as_ref());
    let p_old;
    {
        let mut d = db.borrow_mut();
        p_old = d.p_profile_arg.take();
        d.x_profile = x_profile;
        d.p_profile_arg = p_arg;
        d.m_trace &= SQLITE_TRACE_NONLEGACY_MASK;
        if d.x_profile.is_some() {
            d.m_trace |= SQLITE_TRACE_XPROFILE;
        }
    }
    mutex_leave(mutex.as_ref());
    p_old
}

/// Registra uma função a ser invocada quando uma transação faz commit. Se a função devolver
/// não zero, o commit vira rollback.
pub fn commit_hook(
    db: &Sqlite3Ref,
    x_callback: Option<Rc<dyn Fn(&CallbackArg) -> i32>>,
    p_arg: CallbackArg,
) -> CallbackArg {
    let mutex = db.borrow().mutex.clone();
    mutex_enter(mutex.as_ref());
    let p_old;
    {
        let mut d = db.borrow_mut();
        p_old = d.p_commit_arg.take();
        d.x_commit_callback = x_callback;
        d.p_commit_arg = p_arg;
    }
    mutex_leave(mutex.as_ref());
    p_old
}

/// Registra um callback invocado a cada linha atualizada, inserida ou apagada por esta conexão.
pub fn update_hook(
    db: &Sqlite3Ref,
    x_callback: Option<Rc<dyn Fn(&CallbackArg, i32, &[u8], &[u8], i64)>>,
    p_arg: CallbackArg,
) -> CallbackArg {
    let mutex = db.borrow().mutex.clone();
    mutex_enter(mutex.as_ref());
    let p_ret;
    {
        let mut d = db.borrow_mut();
        p_ret = d.p_update_arg.take();
        d.x_update_callback = x_callback;
        d.p_update_arg = p_arg;
    }
    mutex_leave(mutex.as_ref());
    p_ret
}

/// Registra um callback invocado a cada rollback de transação desta conexão.
pub fn rollback_hook(
    db: &Sqlite3Ref,
    x_callback: Option<Rc<dyn Fn(&CallbackArg)>>,
    p_arg: CallbackArg,
) -> CallbackArg {
    let mutex = db.borrow().mutex.clone();
    mutex_enter(mutex.as_ref());
    let p_ret;
    {
        let mut d = db.borrow_mut();
        p_ret = d.p_rollback_arg.take();
        d.x_rollback_callback = x_callback;
        d.p_rollback_arg = p_arg;
    }
    mutex_leave(mutex.as_ref());
    p_ret
}

/// Registra um callback invocado antes de cada linha atualizada, inserida ou apagada por esta
/// conexão (`SQLITE_ENABLE_PREUPDATE_HOOK`).
pub fn preupdate_hook(
    db: &Sqlite3Ref,
    x_callback: Option<Rc<dyn Fn(&CallbackArg, &Sqlite3Ref, i32, &[u8], &[u8], i64, i64)>>,
    p_arg: CallbackArg,
) -> CallbackArg {
    let mutex = db.borrow().mutex.clone();
    mutex_enter(mutex.as_ref());
    let p_ret;
    {
        let mut d = db.borrow_mut();
        p_ret = d.p_pre_update_arg.take();
        d.x_pre_update_callback = x_callback;
        d.p_pre_update_arg = p_arg;
    }
    mutex_leave(mutex.as_ref());
    p_ret
}

/// Registra uma função invocada antes de cada autovacuum, que determina o número de páginas
/// a compactar.
pub fn autovacuum_pages(
    db: &Sqlite3Ref,
    x_callback: Option<Rc<dyn Fn(&CallbackArg, &[u8], u32, u32, u32) -> u32>>,
    p_arg: CallbackArg,
    x_destructor: Option<Rc<dyn Fn(&CallbackArg)>>,
) -> i32 {
    let mutex = db.borrow().mutex.clone();
    mutex_enter(mutex.as_ref());
    let old_destr = db.borrow().x_autovac_destr.clone();
    if let Some(destr) = old_destr {
        let old_arg = db.borrow().p_autovac_pages_arg.clone();
        destr(&old_arg);
    }
    {
        let mut d = db.borrow_mut();
        d.x_autovac_pages = x_callback;
        d.p_autovac_pages_arg = p_arg;
        d.x_autovac_destr = x_destructor;
    }
    mutex_leave(mutex.as_ref());
    SQLITE_OK
}

/// O callback de `wal_hook()` registrado por `wal_autocheckpoint()`. Chama o checkpoint se o
/// número de frames no log for maior ou igual ao inteiro guardado em `p_client_data` (o valor
/// configurado por `wal_autocheckpoint()`).
pub fn wal_default_hook(
    p_client_data: &CallbackArg,
    db: &Sqlite3Ref,
    z_db: &[u8],
    n_frame: i32,
) -> i32 {
    let limit = p_client_data
        .as_ref()
        .and_then(|a| a.downcast_ref::<i32>())
        .copied()
        .unwrap_or(0);
    if n_frame >= limit {
        begin_benign_malloc();
        wal_checkpoint(db, Some(z_db));
        end_benign_malloc();
    }
    SQLITE_OK
}

/// Configura um callback de `wal_hook()` para fazer checkpoint automático depois de um commit
/// se houver `n_frame` ou mais frames no log. Zero ou negativo desabilita o checkpoint
/// automático por completo.
///
/// O callback registrado aqui substitui qualquer callback existente registrado com
/// `wal_hook()`; do mesmo modo, registrar um callback com `wal_hook()` desabilita o mecanismo
/// configurado por esta função.
pub fn wal_autocheckpoint(db: &Sqlite3Ref, n_frame: i32) -> i32 {
    if n_frame > 0 {
        let hook: Rc<dyn Fn(&CallbackArg, &Sqlite3Ref, &[u8], i32) -> i32> =
            Rc::new(wal_default_hook);
        let arg: Rc<dyn Any> = Rc::new(n_frame);
        wal_hook(db, Some(hook), Some(arg));
    } else {
        wal_hook(db, None, None);
    }
    SQLITE_OK
}

/// Registra um callback invocado a cada vez que uma transação é escrita no write-ahead-log por
/// esta conexão.
pub fn wal_hook(
    db: &Sqlite3Ref,
    x_callback: Option<Rc<dyn Fn(&CallbackArg, &Sqlite3Ref, &[u8], i32) -> i32>>,
    p_arg: CallbackArg,
) -> CallbackArg {
    let mutex = db.borrow().mutex.clone();
    mutex_enter(mutex.as_ref());
    let p_ret;
    {
        let mut d = db.borrow_mut();
        p_ret = d.p_wal_arg.take();
        d.x_wal_callback = x_callback;
        d.p_wal_arg = p_arg;
    }
    mutex_leave(mutex.as_ref());
    p_ret
}

/// Faz checkpoint do banco `z_db`.
pub fn wal_checkpoint_v2(
    db: &Sqlite3Ref,
    z_db: Option<&[u8]>,
    e_mode: i32,
    pn_log: Option<&mut i32>,
    pn_ckpt: Option<&mut i32>,
) -> i32 {
    let mut rc: i32;
    let i_db: i32;

    // Inicializa as saídas com -1 caso ocorra um erro.
    let mut pn_log = pn_log;
    let mut pn_ckpt = pn_ckpt;
    if let Some(n) = pn_log.as_deref_mut() {
        *n = -1;
    }
    if let Some(n) = pn_ckpt.as_deref_mut() {
        *n = -1;
    }

    debug_assert!(SQLITE_CHECKPOINT_PASSIVE == 0);
    debug_assert!(SQLITE_CHECKPOINT_FULL == 1);
    debug_assert!(SQLITE_CHECKPOINT_RESTART == 2);
    debug_assert!(SQLITE_CHECKPOINT_TRUNCATE == 3);
    if e_mode < SQLITE_CHECKPOINT_PASSIVE || e_mode > SQLITE_CHECKPOINT_TRUNCATE {
        // EVIDENCE-OF: R-03996-12088 O parâmetro M precisa ser um modo de checkpoint válido.
        return SQLITE_MISUSE_BKPT;
    }

    let mutex = db.borrow().mutex.clone();
    mutex_enter(mutex.as_ref());
    match z_db {
        Some(z) if !z.is_empty() => {
            i_db = find_db_name(db, z);
        }
        _ => {
            i_db = SQLITE_MAX_DB; // Significa processar todos os schemas.
        }
    }
    if i_db < 0 {
        rc = SQLITE_ERROR;
        let mut msg: Vec<u8> = b"unknown database: ".to_vec();
        if let Some(z) = z_db {
            msg.extend_from_slice(z);
        }
        error_with_msg(db, SQLITE_ERROR, Some(msg.as_slice()));
    } else {
        db.borrow_mut().busy_handler.n_busy = 0;
        rc = checkpoint(db, i_db, e_mode, pn_log, pn_ckpt);
        error(db, rc);
    }
    rc = api_exit(db, rc);

    // Se não há instruções ativas, limpa a flag de interrupção neste ponto.
    if db.borrow().n_vdbe_active == 0 {
        db.borrow_mut().is_interrupted = 0;
    }

    mutex_leave(mutex.as_ref());
    rc
}

/// Faz checkpoint do banco `z_db`. Se `z_db` for nulo, ou apontar para uma string vazia, todos
/// os bancos anexados sofrem checkpoint.
pub fn wal_checkpoint(db: &Sqlite3Ref, z_db: Option<&[u8]>) -> i32 {
    // EVIDENCE-OF: R-41613-20553 wal_checkpoint(D,X) equivale a
    // wal_checkpoint_v2(D,X,SQLITE_CHECKPOINT_PASSIVE,0,0).
    wal_checkpoint_v2(db, z_db, SQLITE_CHECKPOINT_PASSIVE, None, None)
}

/// Executa um checkpoint no banco `i_db`. Não faz nada se `i_db` não estiver aberto em modo WAL.
///
/// Se há uma transação aberta no banco sob checkpoint, devolve SQLITE_LOCKED e o checkpoint
/// não é tentado. Se ocorrer erro durante o checkpoint, devolve o código de erro do SQLite
/// (por exemplo SQLITE_IOERR). Caso contrário, SQLITE_OK.
///
/// O mutex do handle `db` deve estar em posse do chamador. O mutex associado à b-tree
/// específica é tomado por esta função enquanto o checkpoint roda.
///
/// Se `i_db` for SQLITE_MAX_DB, todos os bancos anexados sofrem checkpoint; um erro é devolvido
/// imediatamente, sem tentar os bancos restantes.
///
/// O parâmetro `e_mode` é SQLITE_CHECKPOINT_PASSIVE, FULL, RESTART ou TRUNCATE.
pub fn checkpoint(
    db: &Sqlite3Ref,
    i_db: i32,
    e_mode: i32,
    pn_log: Option<&mut i32>,
    pn_ckpt: Option<&mut i32>,
) -> i32 {
    let mut rc = SQLITE_OK; // Código de retorno.
    let mut i: i32 = 0; // Itera pelos bancos anexados.
    let mut b_busy = false; // Verdadeiro se SQLITE_BUSY foi encontrado.
    let mut pn_log = pn_log;
    let mut pn_ckpt = pn_ckpt;

    debug_assert!(pn_log.as_deref().map_or(true, |n| *n == -1));
    debug_assert!(pn_ckpt.as_deref().map_or(true, |n| *n == -1));

    while i < db.borrow().n_db && rc == SQLITE_OK {
        if i == i_db || i_db == SQLITE_MAX_DB {
            let p_bt = db.borrow().a_db[i as usize].p_bt.clone();
            rc = btree_checkpoint(p_bt, e_mode, pn_log.as_deref_mut(), pn_ckpt.as_deref_mut());
            pn_log = None;
            pn_ckpt = None;
            if rc == SQLITE_BUSY {
                b_busy = true;
                rc = SQLITE_OK;
            }
        }
        i += 1;
    }

    if rc == SQLITE_OK && b_busy {
        SQLITE_BUSY
    } else {
        rc
    }
}


// ---- part_007.rs ----

/// Diz se a memória principal deve ser usada no lugar de um arquivo temporário para os
/// arquivos transitórios do pager e para os journals de instrução. O valor devolvido depende
/// de `db.temp_store` (parâmetro de execução) e do valor de compilação SQLITE_TEMP_STORE
/// (1 no Debian 13). Com SQLITE_TEMP_STORE igual a 1, só `temp_store == 2` pede memória.
pub fn temp_in_memory(db: &Sqlite3) -> i32 {
    (db.temp_store == 2) as i32
}

/// Devolve a explicação em inglês, em UTF-8, do erro mais recente (`sqlite3_errmsg`).
/// `db` None é o ponteiro NULL do C.
pub fn errmsg(db: Option<&Sqlite3Ref>) -> Vec<u8> {
    let db = match db {
        Some(db) => db,
        None => return err_str(SQLITE_NOMEM_BKPT).to_vec(),
    };
    if safety_check_sick_or_ok(&db.borrow()) == 0 {
        return err_str(sqlite_misuse_bkpt(line!() as i32)).to_vec();
    }
    let mutex = db.borrow().mutex.clone();
    mutex_enter(mutex.as_ref());
    let z: Vec<u8>;
    if db.borrow().malloc_failed != 0 {
        z = err_str(SQLITE_NOMEM_BKPT).to_vec();
    } else {
        let err_code = db.borrow().err_code;
        let p_err = db.borrow().p_err.clone();
        let z_val: Option<Vec<u8>> = if err_code != 0 {
            value_text(p_err.as_ref())
        } else {
            None
        };
        z = match z_val {
            Some(z_val) => z_val,
            None => err_str(err_code).to_vec(),
        };
    }
    mutex_leave(mutex.as_ref());
    z
}

/// Devolve o deslocamento em bytes do erro mais recente (`sqlite3_error_offset`).
pub fn error_offset(db: Option<&Sqlite3Ref>) -> i32 {
    let mut i_offset: i32 = -1;
    if let Some(db) = db {
        if safety_check_sick_or_ok(&db.borrow()) != 0 && db.borrow().err_code != 0 {
            let mutex = db.borrow().mutex.clone();
            mutex_enter(mutex.as_ref());
            i_offset = db.borrow().err_byte_offset;
            mutex_leave(mutex.as_ref());
        }
    }
    i_offset
}

/// Converte uma sequência de unidades UTF-16 nos bytes da ordem nativa, como o C as guarda.
fn utf16_native_bytes(z: &[u16]) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::with_capacity(z.len() * 2);
    for c in z {
        out.extend_from_slice(&c.to_ne_bytes());
    }
    out
}

/// Devolve a explicação em inglês, em UTF-16 (bytes na ordem nativa, com o terminador),
/// do erro mais recente (`sqlite3_errmsg16`). None é o ponteiro NULL do C.
pub fn errmsg16(db: Option<&Sqlite3Ref>) -> Option<Vec<u8>> {
    // "out of memory\0"
    const OUT_OF_MEM: [u16; 14] = [
        b'o' as u16, b'u' as u16, b't' as u16, b' ' as u16, b'o' as u16, b'f' as u16,
        b' ' as u16, b'm' as u16, b'e' as u16, b'm' as u16, b'o' as u16, b'r' as u16,
        b'y' as u16, 0,
    ];
    // "bad parameter or other API misuse\0"
    const MISUSE: [u16; 34] = [
        b'b' as u16, b'a' as u16, b'd' as u16, b' ' as u16, b'p' as u16, b'a' as u16,
        b'r' as u16, b'a' as u16, b'm' as u16, b'e' as u16, b't' as u16, b'e' as u16,
        b'r' as u16, b' ' as u16, b'o' as u16, b'r' as u16, b' ' as u16, b'o' as u16,
        b't' as u16, b'h' as u16, b'e' as u16, b'r' as u16, b' ' as u16, b'A' as u16,
        b'P' as u16, b'I' as u16, b' ' as u16, b'm' as u16, b'i' as u16, b's' as u16,
        b'u' as u16, b's' as u16, b'e' as u16, 0,
    ];

    let db = match db {
        Some(db) => db,
        None => return Some(utf16_native_bytes(&OUT_OF_MEM)),
    };
    if safety_check_sick_or_ok(&db.borrow()) == 0 {
        return Some(utf16_native_bytes(&MISUSE));
    }
    let mutex = db.borrow().mutex.clone();
    mutex_enter(mutex.as_ref());
    let mut z: Option<Vec<u8>>;
    if db.borrow().malloc_failed != 0 {
        z = Some(utf16_native_bytes(&OUT_OF_MEM));
    } else {
        let p_err = db.borrow().p_err.clone();
        z = value_text16(p_err.as_ref());
        if z.is_none() {
            let err_code = db.borrow().err_code;
            error_with_msg(&mut db.borrow_mut(), err_code, Some(err_str(err_code)));
            let p_err = db.borrow().p_err.clone();
            z = value_text16(p_err.as_ref());
        }
        // Um malloc() pode ter falhado dentro da chamada a value_text16() acima. Nesse caso o
        // sinalizador malloc_failed precisa ser limpo antes de retornar. Faz-se direto, e não
        // por api_exit(), para não gravar mensagem de erro no handle da conexão.
        oom_clear(&mut db.borrow_mut());
    }
    mutex_leave(mutex.as_ref());
    z
}

/// Devolve o código de erro mais recente gerado por uma rotina do SQLite. Se `db` for None,
/// supõe-se que um malloc() falhou dentro de sqlite3_open() (`sqlite3_errcode`).
pub fn errcode(db: Option<&Sqlite3Ref>) -> i32 {
    if let Some(db) = db {
        if safety_check_sick_or_ok(&db.borrow()) == 0 {
            return sqlite_misuse_bkpt(line!() as i32);
        }
    }
    match db {
        Some(db) if db.borrow().malloc_failed == 0 => {
            let db_b = db.borrow();
            db_b.err_code & db_b.err_mask
        }
        _ => SQLITE_NOMEM_BKPT,
    }
}

/// Devolve o código de erro estendido mais recente (`sqlite3_extended_errcode`).
pub fn extended_errcode(db: Option<&Sqlite3Ref>) -> i32 {
    if let Some(db) = db {
        if safety_check_sick_or_ok(&db.borrow()) == 0 {
            return sqlite_misuse_bkpt(line!() as i32);
        }
    }
    match db {
        Some(db) if db.borrow().malloc_failed == 0 => db.borrow().err_code,
        _ => SQLITE_NOMEM_BKPT,
    }
}

/// Devolve o errno do último erro do sistema (`sqlite3_system_errno`).
pub fn system_errno(db: Option<&Sqlite3Ref>) -> i32 {
    match db {
        Some(db) => db.borrow().i_sys_errno,
        None => 0,
    }
}

/// Devolve a string que descreve o tipo de erro do argumento. Por ora só chama a rotina
/// interna `err_str()` (`sqlite3_errstr`).
pub fn errstr(rc: i32) -> &'static [u8] {
    err_str(rc)
}

/// Cria uma nova função de collation para a conexão `db`. O nome é `z_name` e a codificação
/// é `enc`.
pub fn create_collation(
    db: &Sqlite3Ref,
    z_name: &[u8],
    enc: u8,
    p_ctx: CallbackArg,
    x_compare: Option<Rc<dyn Fn(&CallbackArg, &[u8], &[u8]) -> i32>>,
    x_del: Option<Rc<dyn Fn(&CallbackArg)>>,
) -> i32 {
    // Se a codificação for SQLITE_UTF16, transforma-a em SQLITE_UTF16LE ou SQLITE_UTF16BE
    // com SQLITE_UTF16NATIVE. SQLITE_UTF16 não é usada internamente.
    let mut enc2: i32 = enc as i32;
    if enc2 == SQLITE_UTF16 || enc2 == SQLITE_UTF16_ALIGNED {
        enc2 = SQLITE_UTF16NATIVE;
    }
    if enc2 < SQLITE_UTF8 || enc2 > SQLITE_UTF16BE {
        return sqlite_misuse_bkpt(line!() as i32);
    }

    // Verifica se esta chamada remove ou substitui uma sequência de collation existente. Se
    // sim, e houver VMs ativas, devolve busy. Se não houver VMs ativas, invalida os
    // statements pré-compilados.
    let p_coll = find_coll_seq(db, enc2 as u8, z_name, 0);
    if let Some(p_coll) = p_coll {
        let has_cmp = p_coll.borrow().x_cmp.is_some();
        if has_cmp {
            if db.borrow().n_vdbe_active != 0 {
                error_with_msg(
                    &mut db.borrow_mut(),
                    SQLITE_BUSY,
                    Some(b"unable to delete/modify collation sequence due to active statements"),
                );
                return SQLITE_BUSY;
            }
            expire_prepared_statements(db, 0);

            // Se a sequência p_coll foi criada direto por uma chamada a
            // sqlite3_create_collation, e não gerada por synth_coll_seq(), as cópias feitas
            // por synth_coll_seq() precisam ser invalidadas. Também pode ser preciso chamar
            // o destrutor da collation, CollSeq.x_del().
            let p_coll_enc: u8 = p_coll.borrow().enc;
            if ((p_coll_enc as i32) & !SQLITE_UTF16_ALIGNED) == enc2 {
                // O valor da tabela hash é o vetor de 3 CollSeq (UTF-8, UTF-16LE, UTF-16BE).
                let a_coll: Vec<CollSeqRef> =
                    hash_find(&db.borrow().a_coll_seq, z_name).expect("a_coll da hash");
                for j in 0..3 {
                    let p = &a_coll[j];
                    if p.borrow().enc == p_coll_enc {
                        let p_x_del = p.borrow().x_del.clone();
                        let p_user = p.borrow().p_user.clone();
                        if let Some(p_x_del) = p_x_del {
                            p_x_del(&p_user);
                        }
                        p.borrow_mut().x_cmp = None;
                    }
                }
            }
        }
    }

    let p_coll = match find_coll_seq(db, enc2 as u8, z_name, 1) {
        Some(p_coll) => p_coll,
        None => return SQLITE_NOMEM_BKPT,
    };
    {
        let mut p_coll_b = p_coll.borrow_mut();
        p_coll_b.x_cmp = x_compare;
        p_coll_b.p_user = p_ctx;
        p_coll_b.x_del = x_del;
        p_coll_b.enc = (enc2 | ((enc as i32) & SQLITE_UTF16_ALIGNED)) as u8;
    }
    error(&mut db.borrow_mut(), SQLITE_OK);
    SQLITE_OK
}

/// Esta tabela define os limites rígidos superiores dos valores de limite. O inicializador
/// precisa ficar em sincronia com os #defines SQLITE_LIMIT_* de sqlite3.h.
pub const A_HARD_LIMIT: [i32; 12] = [
    SQLITE_MAX_LENGTH,
    SQLITE_MAX_SQL_LENGTH,
    SQLITE_MAX_COLUMN,
    SQLITE_MAX_EXPR_DEPTH,
    SQLITE_MAX_COMPOUND_SELECT,
    SQLITE_MAX_VDBE_OP,
    SQLITE_MAX_FUNCTION_ARG,
    SQLITE_MAX_ATTACHED,
    SQLITE_MAX_LIKE_PATTERN_LENGTH,
    SQLITE_MAX_VARIABLE_NUMBER, /* IMP: R-38091-32352 */
    SQLITE_MAX_TRIGGER_DEPTH,
    SQLITE_MAX_WORKER_THREADS,
];

// Garante que os limites rígidos têm valores razoáveis (os #error do C) e que a tabela está
// em sincronia com os índices SQLITE_LIMIT_* (os assert do C).
const _: () = {
    assert!(SQLITE_MAX_LENGTH >= 100, "SQLITE_MAX_LENGTH must be at least 100");
    assert!(SQLITE_MAX_SQL_LENGTH >= 100, "SQLITE_MAX_SQL_LENGTH must be at least 100");
    assert!(
        SQLITE_MAX_SQL_LENGTH <= SQLITE_MAX_LENGTH,
        "SQLITE_MAX_SQL_LENGTH must not be greater than SQLITE_MAX_LENGTH"
    );
    assert!(SQLITE_MAX_COMPOUND_SELECT >= 2, "SQLITE_MAX_COMPOUND_SELECT must be at least 2");
    assert!(SQLITE_MAX_VDBE_OP >= 40, "SQLITE_MAX_VDBE_OP must be at least 40");
    assert!(
        SQLITE_MAX_FUNCTION_ARG >= 0 && SQLITE_MAX_FUNCTION_ARG <= 127,
        "SQLITE_MAX_FUNCTION_ARG must be between 0 and 127"
    );
    assert!(
        SQLITE_MAX_ATTACHED >= 0 && SQLITE_MAX_ATTACHED <= 125,
        "SQLITE_MAX_ATTACHED must be between 0 and 125"
    );
    assert!(
        SQLITE_MAX_LIKE_PATTERN_LENGTH >= 1,
        "SQLITE_MAX_LIKE_PATTERN_LENGTH must be at least 1"
    );
    assert!(SQLITE_MAX_COLUMN <= 32767, "SQLITE_MAX_COLUMN must not exceed 32767");
    assert!(SQLITE_MAX_TRIGGER_DEPTH >= 1, "SQLITE_MAX_TRIGGER_DEPTH must be at least 1");
    assert!(
        SQLITE_MAX_WORKER_THREADS >= 0 && SQLITE_MAX_WORKER_THREADS <= 50,
        "SQLITE_MAX_WORKER_THREADS must be between 0 and 50"
    );

    assert!(A_HARD_LIMIT[SQLITE_LIMIT_LENGTH as usize] == SQLITE_MAX_LENGTH);
    assert!(A_HARD_LIMIT[SQLITE_LIMIT_SQL_LENGTH as usize] == SQLITE_MAX_SQL_LENGTH);
    assert!(A_HARD_LIMIT[SQLITE_LIMIT_COLUMN as usize] == SQLITE_MAX_COLUMN);
    assert!(A_HARD_LIMIT[SQLITE_LIMIT_EXPR_DEPTH as usize] == SQLITE_MAX_EXPR_DEPTH);
    assert!(A_HARD_LIMIT[SQLITE_LIMIT_COMPOUND_SELECT as usize] == SQLITE_MAX_COMPOUND_SELECT);
    assert!(A_HARD_LIMIT[SQLITE_LIMIT_VDBE_OP as usize] == SQLITE_MAX_VDBE_OP);
    assert!(A_HARD_LIMIT[SQLITE_LIMIT_FUNCTION_ARG as usize] == SQLITE_MAX_FUNCTION_ARG);
    assert!(A_HARD_LIMIT[SQLITE_LIMIT_ATTACHED as usize] == SQLITE_MAX_ATTACHED);
    assert!(
        A_HARD_LIMIT[SQLITE_LIMIT_LIKE_PATTERN_LENGTH as usize] == SQLITE_MAX_LIKE_PATTERN_LENGTH
    );
    assert!(A_HARD_LIMIT[SQLITE_LIMIT_VARIABLE_NUMBER as usize] == SQLITE_MAX_VARIABLE_NUMBER);
    assert!(A_HARD_LIMIT[SQLITE_LIMIT_TRIGGER_DEPTH as usize] == SQLITE_MAX_TRIGGER_DEPTH);
    assert!(A_HARD_LIMIT[SQLITE_LIMIT_WORKER_THREADS as usize] == SQLITE_MAX_WORKER_THREADS);
    assert!(SQLITE_LIMIT_WORKER_THREADS == (SQLITE_N_LIMIT - 1));
};

/// Muda o valor de um limite e devolve o valor antigo. Se o índice for inválido, devolve -1.
/// Não muda nada, mas ainda devolve o valor antigo, se o novo valor for negativo.
///
/// Um novo limite menor não encolhe construções existentes: só impede que se formem novas
/// construções que o excedam (`sqlite3_limit`).
pub fn limit(db: &Sqlite3Ref, limit_id: i32, new_limit: i32) -> i32 {
    // EVIDENCE-OF: R-30189-54097 Para cada categoria de limite SQLITE_LIMIT_NAME há um
    // limite rígido superior, fixado na compilação por uma macro SQLITE_MAX_NAME (o
    // "_LIMIT_" do nome vira "_MAX_").
    let mut new_limit = new_limit;

    if limit_id < 0 || limit_id >= SQLITE_N_LIMIT {
        return -1;
    }
    let mut db_b = db.borrow_mut();
    let old_limit = db_b.a_limit[limit_id as usize];
    if new_limit >= 0 {
        /* IMP: R-52476-28732 */
        if new_limit > A_HARD_LIMIT[limit_id as usize] {
            new_limit = A_HARD_LIMIT[limit_id as usize]; /* IMP: R-51463-25634 */
        } else if new_limit < 1 && limit_id == SQLITE_LIMIT_LENGTH {
            new_limit = 1;
        }
        db_b.a_limit[limit_id as usize] = new_limit;
    }
    old_limit /* IMP: R-53341-35419 */
}

/// Esta função interpreta tanto URIs quanto nomes de arquivo comuns passados pelo usuário a
/// `sqlite3_open()` ou `sqlite3_open_v2()`, e URIs de banco dadas em ATTACH.
///
/// O primeiro argumento é o nome do VFS a usar (ou None para o VFS padrão) se a URI não
/// trouxer o parâmetro "vfs=xxx". O segundo é a URI (ou nome de arquivo comum). `p_flags`
/// entra com os sinalizadores padrão de abertura e pode ser alterado se a URI tiver
/// "cache=xxx" ou "mode=xxx".
///
/// Em caso de sucesso devolve SQLITE_OK, `pp_vfs` recebe o VFS a usar e `pz_file` recebe o
/// buffer com o nome do arquivo. O buffer mantém os 4 bytes zero iniciais do C (o marcador de
/// início do nome do banco): o nome começa no índice 4, e cada opção da URI vem em seguida,
/// como pares nome/valor terminados em zero, com um zero extra no fim. Liberar o buffer
/// (`sqlite3_free_filename`) é soltar o `Vec`.
///
/// Em caso de erro devolve um código de erro do SQLite e `pz_err_msg` pode receber uma
/// mensagem em inglês.
pub fn parse_uri(
    z_default_vfs: Option<&[u8]>,
    z_uri: &[u8],
    p_flags: &mut u32,
    pp_vfs: &mut Option<Rc<dyn Vfs>>,
    pz_file: &mut Option<Vec<u8>>,
    pz_err_msg: &mut Option<Vec<u8>>,
) -> i32 {
    let mut rc: i32 = SQLITE_OK;
    let mut flags: u32 = *p_flags;
    let mut z_vfs: Option<Vec<u8>> = z_default_vfs.map(|z| z.to_vec());
    let mut z_file: Vec<u8> = Vec::new();
    let n_uri: usize = strlen30(z_uri) as usize;
    // Os 4 bytes zero iniciais do buffer (`zFile += 4` do C).
    let base: usize = 4;
    // Leitura da URI como string terminada em zero: além do fim vem o byte zero.
    let at = |i: usize| -> u8 {
        if i < z_uri.len() {
            z_uri[i]
        } else {
            0
        }
    };

    'parse_uri_out: {
        if ((flags & (SQLITE_OPEN_URI as u32)) != 0 /* IMP: R-48725-32206 */
            || SQLITE_CONFIG.b_open_uri != 0)       /* IMP: R-51689-46548 */
            && n_uri >= 5
            && &z_uri[..5] == b"file:"              /* IMP: R-57884-37496 */
        {
            let mut e_state: i32; // Estado do parser ao ler a URI
            let mut i_in: usize; // Índice do caractere de entrada
            let mut i_out: usize = 0; // Índice do caractere de saída
            let mut n_byte: u64 = n_uri as u64 + 8; // Bytes de espaço a alocar

            // Garante que SQLITE_OPEN_URI está ligado, para indicar ao método x_open do VFS
            // que pode haver parâmetros extras depois do nome do arquivo.
            flags |= SQLITE_OPEN_URI as u32;

            for i_in in 0..n_uri {
                n_byte += (z_uri[i_in] == b'&') as u64;
            }
            z_file = vec![0u8; n_byte as usize];
            // Os 4 bytes zero iniciais são o marcador de início do nome do banco: já estão
            // zerados no vetor (memset(zFile, 0, 4)).

            i_in = 5;
            // Descarta os segmentos de esquema e de autoridade da URI.
            if at(5) == b'/' && at(6) == b'/' {
                i_in = 7;
                while at(i_in) != 0 && at(i_in) != b'/' {
                    i_in += 1;
                }
                if i_in != 7 && (i_in != 16 || &z_uri[7..16] != b"localhost") {
                    let mut msg: Vec<u8> = b"invalid uri authority: ".to_vec();
                    msg.extend_from_slice(&z_uri[7..i_in]);
                    *pz_err_msg = Some(msg);
                    rc = SQLITE_ERROR;
                    break 'parse_uri_out;
                }
            }

            // Copia o nome do arquivo e os parâmetros de consulta para o buffer z_file,
            // decodificando os códigos de escape %HH no caminho.
            //
            // Dentro deste laço, e_state vale 0, 1 ou 2 conforme o contexto:
            //
            //   0: lendo o nome do arquivo.
            //   1: lendo a parte do nome de um parâmetro nome=valor.
            //   2: lendo a parte do valor de um parâmetro nome=valor.
            e_state = 0;
            loop {
                let mut c: u8 = at(i_in);
                if c == 0 || c == b'#' {
                    break;
                }
                i_in += 1;
                if c == b'%' && isxdigit(at(i_in)) && isxdigit(at(i_in + 1)) {
                    let mut octet: i32 = (hex_to_int(at(i_in)) as i32) << 4;
                    i_in += 1;
                    octet += hex_to_int(at(i_in)) as i32;
                    i_in += 1;

                    if octet == 0 {
                        // Este ramo vale quando "%00" aparece na URI. Ignora-se todo o texto
                        // restante do caminho, nome ou valor em leitura: descarta o
                        // caractere atual e salta até o próximo "?", "=" ou "&", conforme o
                        // caso.
                        loop {
                            c = at(i_in);
                            if c != 0
                                && c != b'#'
                                && (e_state != 0 || c != b'?')
                                && (e_state != 1 || (c != b'=' && c != b'&'))
                                && (e_state != 2 || c != b'&')
                            {
                                i_in += 1;
                            } else {
                                break;
                            }
                        }
                        continue;
                    }
                    c = octet as u8;
                } else if e_state == 1 && (c == b'&' || c == b'=') {
                    if z_file[base + i_out - 1] == 0 {
                        // Nome de opção vazio. Ignora a opção por inteiro.
                        while at(i_in) != 0 && at(i_in) != b'#' && at(i_in - 1) != b'&' {
                            i_in += 1;
                        }
                        continue;
                    }
                    if c == b'&' {
                        z_file[base + i_out] = 0;
                        i_out += 1;
                    } else {
                        e_state = 2;
                    }
                    c = 0;
                } else if (e_state == 0 && c == b'?') || (e_state == 2 && c == b'&') {
                    c = 0;
                    e_state = 1;
                }
                z_file[base + i_out] = c;
                i_out += 1;
            }
            if e_state == 1 {
                z_file[base + i_out] = 0;
                i_out += 1;
            }
            // Fim das opções + nomes de journal vazios.
            z_file[base + i_out..base + i_out + 4].fill(0);

            // Verifica se foi indicada alguma opção que deva ser interpretada aqui. As
            // interpretadas aqui são "vfs" e as que correspondem a sinalizadores que podem ser
            // passados ao método sqlite3_open_v2().
            let mut z_opt: usize = base + strlen30(&z_file[base..]) as usize + 1;
            while z_file[z_opt] != 0 {
                let n_opt: usize = strlen30(&z_file[z_opt..]) as usize;
                let z_val: usize = z_opt + n_opt + 1;
                let n_val: usize = strlen30(&z_file[z_val..]) as usize;

                if n_opt == 3 && &z_file[z_opt..z_opt + 3] == b"vfs" {
                    z_vfs = Some(z_file[z_val..z_val + n_val].to_vec());
                } else {
                    struct OpenMode {
                        z: &'static [u8],
                        mode: i32,
                    }
                    const A_CACHE_MODE: [OpenMode; 2] = [
                        OpenMode { z: b"shared", mode: SQLITE_OPEN_SHAREDCACHE },
                        OpenMode { z: b"private", mode: SQLITE_OPEN_PRIVATECACHE },
                    ];
                    const A_OPEN_MODE: [OpenMode; 4] = [
                        OpenMode { z: b"ro", mode: SQLITE_OPEN_READONLY },
                        OpenMode { z: b"rw", mode: SQLITE_OPEN_READWRITE },
                        OpenMode {
                            z: b"rwc",
                            mode: SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE,
                        },
                        OpenMode { z: b"memory", mode: SQLITE_OPEN_MEMORY },
                    ];

                    let mut a_mode: Option<&[OpenMode]> = None;
                    let mut z_mode_type: &[u8] = b"";
                    let mut mask: i32 = 0;
                    let mut limit: i32 = 0;

                    if n_opt == 5 && &z_file[z_opt..z_opt + 5] == b"cache" {
                        mask = SQLITE_OPEN_SHAREDCACHE | SQLITE_OPEN_PRIVATECACHE;
                        a_mode = Some(&A_CACHE_MODE);
                        limit = mask;
                        z_mode_type = b"cache";
                    }
                    if n_opt == 4 && &z_file[z_opt..z_opt + 4] == b"mode" {
                        mask = SQLITE_OPEN_READONLY
                            | SQLITE_OPEN_READWRITE
                            | SQLITE_OPEN_CREATE
                            | SQLITE_OPEN_MEMORY;
                        a_mode = Some(&A_OPEN_MODE);
                        limit = ((mask as u32) & flags) as i32;
                        z_mode_type = b"access";
                    }

                    if let Some(a_mode) = a_mode {
                        let mut mode: i32 = 0;
                        for m in a_mode.iter() {
                            if n_val == m.z.len() && &z_file[z_val..z_val + n_val] == m.z {
                                mode = m.mode;
                                break;
                            }
                        }
                        if mode == 0 {
                            let mut msg: Vec<u8> = b"no such ".to_vec();
                            msg.extend_from_slice(z_mode_type);
                            msg.extend_from_slice(b" mode: ");
                            msg.extend_from_slice(&z_file[z_val..z_val + n_val]);
                            *pz_err_msg = Some(msg);
                            rc = SQLITE_ERROR;
                            break 'parse_uri_out;
                        }
                        if (mode & !SQLITE_OPEN_MEMORY) > limit {
                            let mut msg: Vec<u8> = z_mode_type.to_vec();
                            msg.extend_from_slice(b" mode not allowed: ");
                            msg.extend_from_slice(&z_file[z_val..z_val + n_val]);
                            *pz_err_msg = Some(msg);
                            rc = SQLITE_PERM;
                            break 'parse_uri_out;
                        }
                        flags = (flags & !(mask as u32)) | (mode as u32);
                    }
                }

                z_opt = z_val + n_val + 1;
            }
        } else {
            z_file = vec![0u8; n_uri + 8];
            if n_uri != 0 {
                z_file[base..base + n_uri].copy_from_slice(&z_uri[..n_uri]);
            }
            z_file[base + n_uri..base + n_uri + 4].fill(0);
            flags &= !(SQLITE_OPEN_URI as u32);
        }

        *pp_vfs = vfs_find(z_vfs.as_deref());
        if pp_vfs.is_none() {
            let mut msg: Vec<u8> = b"no such vfs: ".to_vec();
            if let Some(z_vfs) = z_vfs.as_ref() {
                // O %s do printf para no primeiro byte zero.
                let n = z_vfs.iter().position(|&b| b == 0).unwrap_or(z_vfs.len());
                msg.extend_from_slice(&z_vfs[..n]);
            }
            *pz_err_msg = Some(msg);
            rc = SQLITE_ERROR;
        }
    }
    // parse_uri_out:
    let z_file_out: Option<Vec<u8>> = if rc != SQLITE_OK {
        // sqlite3_free_filename(zFile): soltar o buffer.
        None
    } else {
        Some(z_file)
    };
    *p_flags = flags;
    *pz_file = z_file_out;
    rc
}


// ---- part_008.rs ----

/// Faz o trabalho central de extrair parâmetros de URI de um nome de arquivo de banco de dados,
/// para a interface `sqlite3_uri_parameter()`.
///
/// `z_filename` é o buffer completo do nome de arquivo no formato do C: o nome, um NUL, e depois
/// pares chave/valor terminados em NUL, fechados por um NUL extra. Devolve o valor (sem o NUL)
/// do primeiro parâmetro cuja chave é igual a `z_param`.
pub fn uri_parameter<'a>(z_filename: &'a [u8], z_param: &[u8]) -> Option<&'a [u8]> {
    // Comprimento da string terminada em NUL que começa em `pos` (sem contar o NUL).
    let len_at = |pos: usize| -> usize {
        z_filename[pos.min(z_filename.len())..]
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(z_filename.len().saturating_sub(pos))
    };
    let mut pos = len_at(0) + 1;
    while pos < z_filename.len() && z_filename[pos] != 0 {
        let key_len = len_at(pos);
        let x = &z_filename[pos..pos + key_len] == z_param;
        pos += key_len + 1;
        if x {
            let val_len = len_at(pos);
            return Some(&z_filename[pos..pos + val_len]);
        }
        pos += len_at(pos) + 1;
    }
    None
}

/// Faz o trabalho de abrir um banco de dados em nome de `sqlite3_open()` e `sqlite3_open16()`.
/// O nome de arquivo `z_filename` está codificado em UTF-8.
pub fn open_database(
    z_filename: Option<&[u8]>, // Nome do banco de dados em UTF-8
    pp_db: &mut Option<Sqlite3Ref>, // SAÍDA: o handle devolvido
    flags: u32,                // Sinalizadores operacionais
    z_vfs: Option<&[u8]>,      // Nome do VFS a usar
) -> i32 {
    let mut flags: u32 = flags;
    let mut db: Option<Sqlite3Ref>; // O handle alocado fica aqui
    let mut rc: i32; // Código de retorno
    let is_threadsafe: i32; // Verdadeiro para conexões threadsafe
    let mut z_open: Option<Vec<u8>> = None; // Nome de arquivo a passar para BtreeOpen()
    let mut z_err_msg: Option<Vec<u8>> = None; // Mensagem de erro de parse_uri()

    // SQLITE_ENABLE_API_ARMOR não é definido no Debian: `pp_db` nunca é nulo.
    *pp_db = None;
    // SQLITE_OMIT_AUTOINIT não é definido: a inicialização automática vale.
    rc = initialize();
    if rc != 0 {
        return rc;
    }

    if SQLITE_CONFIG.b_core_mutex == 0 {
        is_threadsafe = 0;
    } else if flags & (SQLITE_OPEN_NOMUTEX as u32) != 0 {
        is_threadsafe = 0;
    } else if flags & (SQLITE_OPEN_FULLMUTEX as u32) != 0 {
        is_threadsafe = 1;
    } else {
        is_threadsafe = SQLITE_CONFIG.b_full_mutex as i32;
    }

    if flags & (SQLITE_OPEN_PRIVATECACHE as u32) != 0 {
        flags &= !(SQLITE_OPEN_SHAREDCACHE as u32);
    } else if SQLITE_CONFIG.shared_cache_enabled != 0 {
        flags |= SQLITE_OPEN_SHAREDCACHE as u32;
    }

    // Remove os bits nocivos do parâmetro flags.
    //
    // Os sinalizadores SQLITE_OPEN_NOMUTEX e SQLITE_OPEN_FULLMUTEX foram tratados no bloco de
    // código anterior. Além desses, os únicos sinalizadores de entrada válidos para
    // sqlite3_open_v2() são SQLITE_OPEN_READONLY, SQLITE_OPEN_READWRITE, SQLITE_OPEN_CREATE,
    // SQLITE_OPEN_SHAREDCACHE, SQLITE_OPEN_PRIVATECACHE, SQLITE_OPEN_EXRESCODE e alguns bits
    // reservados. Todos os outros sinalizadores são mascarados em silêncio.
    flags &= !((SQLITE_OPEN_DELETEONCLOSE
        | SQLITE_OPEN_EXCLUSIVE
        | SQLITE_OPEN_MAIN_DB
        | SQLITE_OPEN_TEMP_DB
        | SQLITE_OPEN_TRANSIENT_DB
        | SQLITE_OPEN_MAIN_JOURNAL
        | SQLITE_OPEN_TEMP_JOURNAL
        | SQLITE_OPEN_SUBJOURNAL
        | SQLITE_OPEN_SUPER_JOURNAL
        | SQLITE_OPEN_NOMUTEX
        | SQLITE_OPEN_FULLMUTEX
        | SQLITE_OPEN_WAL) as u32);

    // Aloca a estrutura de dados do sqlite.
    db = Some(Rc::new(RefCell::new(Sqlite3::default())));
    'opendb_out: {
        let dbr: Sqlite3Ref = db.clone().expect("handle recém alocado");
        // SQLITE_ENABLE_MULTITHREADED_CHECKS não é definido: só is_threadsafe decide. Por isso o
        // ramo `isThreadsafe==0` com sqlite3MutexWarnOnContention() é inalcançável aqui.
        if is_threadsafe != 0 {
            let m = mutex_alloc(SQLITE_MUTEX_RECURSIVE);
            if m.is_none() {
                db = None;
                break 'opendb_out;
            }
            dbr.borrow_mut().mutex = m;
        }
        let db_mutex = dbr.borrow().mutex.clone();
        mutex_enter(db_mutex.as_ref());
        {
            let mut d = dbr.borrow_mut();
            d.err_mask = if (flags & (SQLITE_OPEN_EXRESCODE as u32)) != 0 {
                0xffffffffu32 as i32
            } else {
                0xff
            };
            d.n_db = 2;
            d.e_open_state = SQLITE_STATE_BUSY;
            // aDb aponta para aDbStatic no C: dois backends padrão, o principal e o temporário.
            d.a_db = vec![
                Db {
                    z_db_sname: None,
                    p_bt: None,
                    safety_level: 0,
                    b_sync_set: 0,
                    p_schema: None,
                },
                Db {
                    z_db_sname: None,
                    p_bt: None,
                    safety_level: 0,
                    b_sync_set: 0,
                    p_schema: None,
                },
            ];
            d.lookaside.b_disable = 1;
            d.lookaside.sz = 0;

            d.a_limit = A_HARD_LIMIT;
            d.a_limit[SQLITE_LIMIT_WORKER_THREADS as usize] = SQLITE_DEFAULT_WORKER_THREADS;
            d.auto_commit = 1;
            d.next_autovac = -1;
            d.sz_mmap = SQLITE_CONFIG.sz_mmap;
            d.next_pagesize = 0;
            // Qualquer array de ponteiros para string serve.
            d.init.az_init = STD_TYPE.iter().map(|s| s.as_bytes().to_vec()).collect();
            // SQLITE_ENABLE_SORTER_MMAP não é definido.
            //
            // SQLITE_DQS vale 3 (padrão): SQLITE_DBCONFIG_DQS_DDL e DQS_DML ligados. Com os
            // padrões do Debian: auto-índice ligado, formato de arquivo 4 (sem LegacyFileFmt),
            // LOAD_EXTENSION ligado, sem triggers recursivos, sem chaves estrangeiras por padrão
            // e sem ReverseOrder, CellSizeCk, Fts3Tokenizer, EnableQPSG, Defensive,
            // LegacyAlter ou StmtScanStatus.
            d.flags |= SQLITE_SHORT_COL_NAMES
                | SQLITE_ENABLE_TRIGGER
                | SQLITE_ENABLE_VIEW
                | SQLITE_CACHE_SPILL
                | SQLITE_TRUSTED_SCHEMA
                | SQLITE_DQS_DML
                | SQLITE_DQS_DDL
                | SQLITE_AUTO_INDEX
                | SQLITE_LOAD_EXTENSION;
            hash_init(&mut d.a_coll_seq);
            hash_init(&mut d.a_module);
        }

        // Adiciona a sequência de comparação padrão BINARY. BINARY serve tanto para UTF-8 quanto
        // para UTF-16, então se acrescenta uma versão para cada, evitando conversões
        // desnecessárias. O único erro possível aqui é falha de malloc().
        //
        // EVIDENCE-OF: R-52786-44878 SQLite defines three built-in collating
        // functions:
        create_collation(&dbr, STR_BINARY, SQLITE_UTF8 as u8, None, Some(bin_coll_func_rc()), None);
        create_collation(&dbr, STR_BINARY, SQLITE_UTF16BE as u8, None, Some(bin_coll_func_rc()), None);
        create_collation(&dbr, STR_BINARY, SQLITE_UTF16LE as u8, None, Some(bin_coll_func_rc()), None);
        create_collation(&dbr, b"NOCASE", SQLITE_UTF8 as u8, None, Some(nocase_collating_func_rc()), None);
        create_collation(&dbr, b"RTRIM", SQLITE_UTF8 as u8, None, Some(rtrim_coll_func_rc()), None);
        if dbr.borrow().malloc_failed != 0 {
            break 'opendb_out;
        }

        // SQLITE_OS_KV_OPTIONAL não é definido: os nomes mágicos ":localStorage:" e
        // ":sessionStorage:" não existem.

        // Analisa o argumento nome de arquivo/URI.
        //
        // Só permite combinações sensatas de bits no argumento flags. Gera erro para qualquer
        // combinação sem sentido. Se as combinações ilegais não fossem barradas aqui, poderiam
        // disparar assert() em camadas mais profundas. As combinações sensatas são:
        //
        //  1:  SQLITE_OPEN_READONLY
        //  2:  SQLITE_OPEN_READWRITE
        //  6:  SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE
        dbr.borrow_mut().open_flags = flags;
        if ((1u32 << (flags & 7)) & 0x46) == 0 {
            rc = SQLITE_MISUSE_BKPT; // IMP: R-18321-05872
        } else {
            let mut p_vfs: Option<Rc<dyn Vfs>> = None;
            rc = parse_uri(z_vfs, z_filename, &mut flags, &mut p_vfs, &mut z_open, &mut z_err_msg);
            dbr.borrow_mut().p_vfs = p_vfs;
        }
        if rc != SQLITE_OK {
            {
                let mut d = dbr.borrow_mut();
                if rc == SQLITE_NOMEM {
                    oom_fault(&mut d);
                }
                // sqlite3ErrorWithMsg(db, rc, zErrMsg ? "%s" : 0, zErrMsg)
                error_with_msg(&mut d, rc, z_err_msg.as_deref());
            }
            // sqlite3_free(zErrMsg): o Vec é liberado ao sair do escopo.
            break 'opendb_out;
        }
        // SQLITE_OS_KV não é definido: o VFS kvvfs não força temp_store.

        // Abre o driver de backend do banco de dados.
        let p_vfs = dbr.borrow().p_vfs.clone().expect("VFS definido por parse_uri");
        let mut p_bt: Option<BtreeRef> = None;
        rc = btree_open(
            &p_vfs,
            z_open.as_deref(),
            &dbr,
            &mut p_bt,
            0,
            flags | (SQLITE_OPEN_MAIN_DB as u32),
        );
        dbr.borrow_mut().a_db[0].p_bt = p_bt;
        if rc != SQLITE_OK {
            if rc == SQLITE_IOERR_NOMEM {
                rc = SQLITE_NOMEM_BKPT;
            }
            error(&mut dbr.borrow_mut(), rc);
            break 'opendb_out;
        }
        let bt0 = dbr.borrow().a_db[0].p_bt.clone().expect("btree principal");
        btree_enter(&mut bt0.borrow_mut());
        {
            let mut d = dbr.borrow_mut();
            let schema = schema_get(&mut d, Some(bt0.clone()));
            d.a_db[0].p_schema = schema;
            if d.malloc_failed == 0 {
                let enc = schema_enc(&d);
                set_text_encoding(&mut d, enc);
            }
        }
        btree_leave(&mut bt0.borrow_mut());
        {
            let mut d = dbr.borrow_mut();
            let schema = schema_get(&mut d, None);
            d.a_db[1].p_schema = schema;

            // O safety_level padrão do banco principal é FULL; o do temporário é OFF. Isso casa
            // com os padrões da camada do pager.
            d.a_db[0].z_db_sname = Some(b"main".to_vec());
            d.a_db[0].safety_level = (SQLITE_DEFAULT_SYNCHRONOUS + 1) as u8;
            d.a_db[1].z_db_sname = Some(b"temp".to_vec());
            d.a_db[1].safety_level = PAGER_SYNCHRONOUS_OFF as u8;

            d.e_open_state = SQLITE_STATE_OPEN;
            if d.malloc_failed != 0 {
                break 'opendb_out;
            }
        }

        // Registra todas as funções embutidas, mas não tenta ler o schema do banco ainda. Isso é
        // adiado até o primeiro acesso ao banco.
        error(&mut dbr.borrow_mut(), SQLITE_OK);
        register_per_connection_builtin_functions(&dbr);
        rc = errcode(Some(&dbr));

        // Carrega as extensões compiladas no binário.
        let mut i: usize = 0;
        while rc == SQLITE_OK && i < BUILTIN_EXTENSIONS.len() {
            rc = BUILTIN_EXTENSIONS[i](&dbr);
            i += 1;
        }

        // Carrega as extensões automáticas, registradas com a API sqlite3_automatic_extension().
        if rc == SQLITE_OK {
            auto_load_extensions(&dbr);
            rc = errcode(Some(&dbr));
            if rc != SQLITE_OK {
                break 'opendb_out;
            }
        }

        // SQLITE_ENABLE_INTERNAL_FUNCTIONS e SQLITE_DEFAULT_LOCKING_MODE não são definidos.

        if rc != 0 {
            error(&mut dbr.borrow_mut(), rc);
        }

        // Habilita o subsistema de malloc lookaside.
        {
            let mut d = dbr.borrow_mut();
            setup_lookaside(&mut d, None, SQLITE_CONFIG.sz_lookaside, SQLITE_CONFIG.n_lookaside);
        }

        wal_autocheckpoint(&dbr, SQLITE_DEFAULT_WAL_AUTOCHECKPOINT);
    }

    // opendb_out:
    if let Some(dbr) = &db {
        let db_mutex = dbr.borrow().mutex.clone();
        mutex_leave(db_mutex.as_ref());
    }
    rc = errcode(db.as_ref());
    if (rc & 0xff) == SQLITE_NOMEM {
        close(db.take());
        db = None;
    } else if rc != SQLITE_OK {
        if let Some(dbr) = &db {
            dbr.borrow_mut().e_open_state = SQLITE_STATE_SICK;
        }
    }
    *pp_db = db;
    // SQLITE_ENABLE_SQLLOG não é definido.
    free_filename(z_open);
    rc
}


// ---- part_009.rs ----

// Notas para o integrador (tech lead):
//  - `open_database` (static de main.c, parte 8) é usada como
//    `open_database(z_filename: Option<&[u8]>, pp_db: &mut Option<Sqlite3Ref>, flags: u32,
//    z_vfs: Option<&[u8]>) -> i32`.
//  - `create_collation` (static de main.c, parte 7) é usada como
//    `create_collation(db: &Sqlite3Ref, z_name: &[u8], enc: u8, p_ctx: CallbackArg,
//    x_compare: Option<Rc<dyn Fn(&CallbackArg, &[u8], &[u8]) -> i32>>,
//    x_del: Option<Rc<dyn Fn(&CallbackArg)>>) -> i32`.
//  - A `sqlite3_create_collation` pública colide em nome com a `createCollation` estática; aqui ela
//    se chama `create_collation_public` (a estática mantém `create_collation`, que é o nome que as
//    partes 7 e 8 usam).
//  - O destrutor de `sqlite3_set_clientdata` é o `Drop` do `Box<dyn Any>` guardado em
//    `DbClientData.p_data`; quem registra o dado embrulha o destrutor num tipo com `Drop`.

/// Abre um novo handle de banco de dados (sqlite3_open).
pub fn open(z_filename: Option<&[u8]>, pp_db: &mut Option<Sqlite3Ref>) -> i32 {
    open_database(
        z_filename,
        pp_db,
        (SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE) as u32,
        None,
    )
}

/// Abre um novo handle de banco de dados com flags e VFS explícitos (sqlite3_open_v2).
pub fn open_v2(
    filename: Option<&[u8]>,    // Nome do arquivo do banco (UTF-8)
    pp_db: &mut Option<Sqlite3Ref>, // SAÍDA: handle do banco
    flags: i32,                 // Flags
    z_vfs: Option<&[u8]>,       // Nome do módulo VFS a usar
) -> i32 {
    open_database(filename, pp_db, flags as u32, z_vfs)
}

/// Abre um novo handle de banco de dados com o nome do arquivo em UTF-16 (sqlite3_open16).
pub fn open16(z_filename: Option<&[u8]>, pp_db: &mut Option<Sqlite3Ref>) -> i32 {
    let mut rc: i32;

    *pp_db = None;
    rc = initialize();
    if rc != 0 {
        return rc;
    }
    let z_filename: &[u8] = z_filename.unwrap_or(&[0u8, 0u8][..]);
    let mut p_val = value_new(None);
    if let Some(v) = p_val.as_mut() {
        value_set_str(v, -1, z_filename, SQLITE_UTF16NATIVE as u8, SQLITE_STATIC);
    }
    {
        // zFilename8: o nome do arquivo codificado em UTF-8 em vez de UTF-16
        let z_filename8 = value_text(p_val.as_deref_mut(), SQLITE_UTF8);
        if let Some(z8) = z_filename8 {
            rc = open_database(
                Some(z8),
                pp_db,
                (SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE) as u32,
                None,
            );
            debug_assert!(pp_db.is_some() || rc == SQLITE_NOMEM);
            if rc == SQLITE_OK {
                if let Some(db) = pp_db.as_ref() {
                    let schema_loaded = db_has_property(&db.borrow(), 0, DB_SCHEMA_LOADED);
                    if !schema_loaded {
                        let mut d = db.borrow_mut();
                        d.enc = SQLITE_UTF16NATIVE as u8;
                        if let Some(s) = d.a_db[0].p_schema.as_ref() {
                            s.borrow_mut().enc = SQLITE_UTF16NATIVE as u8;
                        }
                    }
                }
            }
        } else {
            rc = SQLITE_NOMEM_BKPT;
        }
    }
    value_free(p_val);

    rc & 0xff
}

/// Registra uma nova sequência de ordenação no handle `db` (sqlite3_create_collation).
pub fn create_collation_public(
    db: &Sqlite3Ref,
    z_name: &[u8],
    enc: i32,
    p_ctx: CallbackArg,
    x_compare: Option<Rc<dyn Fn(&CallbackArg, &[u8], &[u8]) -> i32>>,
) -> i32 {
    create_collation_v2(db, z_name, enc, p_ctx, x_compare, None)
}

/// Registra uma nova sequência de ordenação no handle `db`, com destrutor (sqlite3_create_collation_v2).
pub fn create_collation_v2(
    db: &Sqlite3Ref,
    z_name: &[u8],
    enc: i32,
    p_ctx: CallbackArg,
    x_compare: Option<Rc<dyn Fn(&CallbackArg, &[u8], &[u8]) -> i32>>,
    x_del: Option<Rc<dyn Fn(&CallbackArg)>>,
) -> i32 {
    let mutex = db.borrow().mutex.clone();
    mutex_enter(mutex.as_ref());
    debug_assert!(db.borrow().malloc_failed == 0);
    let mut rc = create_collation(db, z_name, enc as u8, p_ctx, x_compare, x_del);
    rc = api_exit(&mut db.borrow_mut(), rc);
    mutex_leave(mutex.as_ref());
    rc
}

/// Registra uma nova sequência de ordenação com o nome em UTF-16 (sqlite3_create_collation16).
pub fn create_collation16(
    db: &Sqlite3Ref,
    z_name: &[u8],
    enc: i32,
    p_ctx: CallbackArg,
    x_compare: Option<Rc<dyn Fn(&CallbackArg, &[u8], &[u8]) -> i32>>,
) -> i32 {
    let mut rc = SQLITE_OK;

    let mutex = db.borrow().mutex.clone();
    mutex_enter(mutex.as_ref());
    debug_assert!(db.borrow().malloc_failed == 0);
    let z_name8 = {
        let d = db.borrow();
        utf16_to_8(&d, z_name, -1, SQLITE_UTF16NATIVE as u8)
    };
    if let Some(z_name8) = z_name8 {
        rc = create_collation(db, &z_name8, enc as u8, p_ctx, x_compare, None);
    }
    rc = api_exit(&mut db.borrow_mut(), rc);
    mutex_leave(mutex.as_ref());
    rc
}

/// Registra um callback de fábrica de sequências de ordenação no handle `db`, substituindo
/// qualquer fábrica instalada antes (sqlite3_collation_needed).
pub fn collation_needed(
    db: &Sqlite3Ref,
    p_coll_needed_arg: CallbackArg,
    x_coll_needed: Option<Rc<dyn Fn(&CallbackArg, &Sqlite3Ref, i32, &[u8])>>,
) -> i32 {
    let mutex = db.borrow().mutex.clone();
    mutex_enter(mutex.as_ref());
    {
        let mut d = db.borrow_mut();
        d.x_coll_needed = x_coll_needed;
        d.x_coll_needed16 = None;
        d.p_coll_needed_arg = p_coll_needed_arg;
    }
    mutex_leave(mutex.as_ref());
    SQLITE_OK
}

/// Igual a `collation_needed`, mas com o nome da sequência em UTF-16 (sqlite3_collation_needed16).
pub fn collation_needed16(
    db: &Sqlite3Ref,
    p_coll_needed_arg: CallbackArg,
    x_coll_needed16: Option<Rc<dyn Fn(&CallbackArg, &Sqlite3Ref, i32, &[u8])>>,
) -> i32 {
    let mutex = db.borrow().mutex.clone();
    mutex_enter(mutex.as_ref());
    {
        let mut d = db.borrow_mut();
        d.x_coll_needed = None;
        d.x_coll_needed16 = x_coll_needed16;
        d.p_coll_needed_arg = p_coll_needed_arg;
    }
    mutex_leave(mutex.as_ref());
    SQLITE_OK
}

/// Procura um dado de cliente já existente (sqlite3_get_clientdata). O dado fica dentro da
/// conexão, então o acesso é feito por uma closure que recebe a referência ao dado; devolve
/// `None` se o nome não existe.
pub fn get_clientdata<R>(
    db: &Sqlite3Ref,
    z_name: &[u8],
    f: impl FnOnce(&dyn std::any::Any) -> R,
) -> Option<R> {
    let mutex = db.borrow().mutex.clone();
    mutex_enter(mutex.as_ref());
    let mut result: Option<R> = None;
    {
        let d = db.borrow();
        let mut p = d.p_db_data.as_ref();
        while let Some(node) = p {
            if node.z_name.as_slice() == z_name {
                result = Some(f(node.p_data.as_ref()));
                break;
            }
            p = node.p_next.as_ref();
        }
    }
    mutex_leave(mutex.as_ref());
    result
}

/// Adiciona um novo dado de cliente à conexão (sqlite3_set_clientdata). O destrutor do C é o
/// `Drop` do dado: ele roda quando o dado é substituído, removido ou quando a conexão é fechada.
pub fn set_clientdata(
    db: &Sqlite3Ref,
    z_name: &[u8],
    p_data: Option<Box<dyn std::any::Any>>,
) -> i32 {
    let mutex = db.borrow().mutex.clone();
    mutex_enter(mutex.as_ref());
    // O dado antigo (e o nó removido) só são destruídos depois de soltar o empréstimo da
    // conexão, mas ainda com o mutex preso, como no C.
    let old: Option<Box<dyn std::any::Any>>;
    let removed: Option<Box<DbClientData>>;
    {
        let mut d = db.borrow_mut();
        let mut cur = &mut d.p_db_data;
        while cur.as_ref().map_or(false, |n| n.z_name.as_slice() != z_name) {
            cur = &mut cur.as_mut().unwrap().p_next;
        }
        if cur.is_some() {
            // Achou o nome: o destrutor do dado antigo roda agora.
            if p_data.is_none() {
                // Remove o nó da lista.
                let mut node = cur.take().unwrap();
                *cur = node.p_next.take();
                old = None;
                removed = Some(node);
            } else {
                let node = cur.as_mut().unwrap();
                old = Some(std::mem::replace(&mut node.p_data, p_data.unwrap()));
                removed = None;
            }
        } else if p_data.is_none() {
            old = None;
            removed = None;
            drop(d);
            mutex_leave(mutex.as_ref());
            return SQLITE_OK;
        } else {
            // Nome novo: insere na cabeça da lista.
            let mut d = d;
            let node = Box::new(DbClientData {
                p_next: d.p_db_data.take(),
                p_data: p_data.unwrap(),
                z_name: z_name.to_vec(),
            });
            d.p_db_data = Some(node);
            old = None;
            removed = None;
        }
    }
    drop(old);
    drop(removed);
    mutex_leave(mutex.as_ref());
    SQLITE_OK
}

/// Esta função é hoje um anacronismo. Servia para se recuperar de uma falha de malloc(), mas o
/// SQLite agora faz isso automaticamente (sqlite3_global_recover).
pub fn global_recover() -> i32 {
    SQLITE_OK
}

/// Testa se a conexão está em modo autocommit. Devolve verdadeiro se estiver. O autocommit vem
/// ligado por padrão, é desligado por BEGIN e religado pelo próximo COMMIT ou ROLLBACK
/// (sqlite3_get_autocommit).
pub fn get_autocommit(db: &Sqlite3Ref) -> i32 {
    db.borrow().auto_commit as i32
}

/// As rotinas a seguir substituem as constantes SQLITE_CORRUPT, SQLITE_MISUSE, SQLITE_CANTOPEN,
/// SQLITE_NOMEM e possivelmente outras. Servem de ponto para breakpoint e invocam sqlite3_log()
/// para informar o ponto do código-fonte onde o erro de baixo nível foi detectado primeiro.
pub fn report_error(i_err: i32, lineno: i32, z_type: &str) -> i32 {
    // "%s at line %d of [%.10s]" com 20+sqlite3_sourceid()
    let source_id = source_id_tail();
    let mut msg: Vec<u8> = Vec::new();
    msg.extend_from_slice(z_type.as_bytes());
    msg.extend_from_slice(b" at line ");
    msg.extend_from_slice(lineno.to_string().as_bytes());
    msg.extend_from_slice(b" of [");
    msg.extend_from_slice(source_id);
    msg.push(b']');
    // A mensagem já está pronta e não tem '%': serve de formato sem argumentos.
    api_log(i_err, &msg, &[]);
    i_err
}

/// Os primeiros 10 bytes de `20+sqlite3_sourceid()` (o `%.10s` do formato).
fn source_id_tail() -> &'static [u8] {
    let id = sourceid().as_bytes();
    let start = 20.min(id.len());
    let rest = &id[start..];
    let end = rest.iter().position(|&b| b == 0).unwrap_or(rest.len()).min(10);
    &rest[..end]
}

/// Erro de corrupção do banco de dados.
pub fn corrupt_error(lineno: i32) -> i32 {
    report_error(SQLITE_CORRUPT, lineno, "database corruption")
}

/// Erro de mau uso da API.
pub fn misuse_error(lineno: i32) -> i32 {
    report_error(SQLITE_MISUSE, lineno, "misuse")
}

/// Erro de arquivo que não pôde ser aberto.
pub fn cantopen_error(lineno: i32) -> i32 {
    report_error(SQLITE_CANTOPEN, lineno, "cannot open file")
}

/// Rotina de conveniência que garante que todo dado específico da thread foi liberado. O SQLite
/// não usa mais dado específico de thread, então ela não faz nada; fica por compatibilidade
/// histórica (sqlite3_thread_cleanup).
pub fn thread_cleanup() {}

/// Devolve metadados sobre uma coluna de uma tabela (sqlite3_table_column_metadata). Os
/// parâmetros de saída opcionais são `Option<&mut ...>`; os textos saem copiados.
pub fn table_column_metadata(
    db: &Sqlite3Ref,
    z_db_name: Option<&[u8]>,       // Nome do banco ou None
    z_table_name: &[u8],            // Nome da tabela
    z_column_name: Option<&[u8]>,   // Nome da coluna
    pz_data_type: Option<&mut Option<Vec<u8>>>, // SAÍDA: tipo declarado
    pz_coll_seq: Option<&mut Option<Vec<u8>>>,  // SAÍDA: nome da sequência de ordenação
    p_not_null: Option<&mut i32>,   // SAÍDA: verdadeiro se há restrição NOT NULL
    p_primary_key: Option<&mut i32>, // SAÍDA: verdadeiro se a coluna é parte da PK
    p_autoinc: Option<&mut i32>,    // SAÍDA: verdadeiro se é autoincremento
) -> i32 {
    let mut z_err_msg: Option<Vec<u8>> = None;
    let mut p_tab: Option<TableRef> = None;
    let mut p_col: Option<usize> = None;
    let mut i_col: i32 = 0;
    let mut z_data_type: Option<Vec<u8>> = None;
    let mut z_coll_seq: Option<Vec<u8>> = None;
    let mut notnull: i32 = 0;
    let mut primarykey: i32 = 0;
    let mut autoinc: i32 = 0;
    let mut rc: i32;

    // Garante que o schema do banco foi carregado
    let mutex = db.borrow().mutex.clone();
    mutex_enter(mutex.as_ref());
    let mut db_b = db.borrow_mut();
    btree_enter_all(&mut db_b);
    rc = init(&mut db_b, &mut z_err_msg);
    'error_out: {
        if SQLITE_OK != rc {
            break 'error_out;
        }

        // Localiza a tabela em questão
        p_tab = find_table(&db_b, z_table_name, z_db_name);
        if p_tab.as_ref().map_or(true, |t| is_view(&t.borrow())) {
            p_tab = None;
            break 'error_out;
        }
        let tab_rc = p_tab.clone().unwrap();
        let tab = tab_rc.borrow();

        // Acha a coluna da qual se pede a informação
        match z_column_name {
            None => {
                // Só consulta a existência da tabela
            }
            Some(z_col_name) => {
                i_col = 0;
                while i_col < tab.n_col as i32 {
                    p_col = Some(i_col as usize);
                    let cn = &tab.a_col[i_col as usize].z_cn_name;
                    let end = cn.iter().position(|&b| b == 0).unwrap_or(cn.len());
                    if 0 == str_i_cmp(&cn[..end], z_col_name) {
                        break;
                    }
                    i_col += 1;
                }
                if i_col == tab.n_col as i32 {
                    if has_rowid(&tab) && is_rowid(z_col_name) != 0 {
                        i_col = tab.i_p_key as i32;
                        p_col = if i_col >= 0 { Some(i_col as usize) } else { None };
                    } else {
                        p_tab = None;
                        break 'error_out;
                    }
                }
            }
        }

        // O bloco a seguir guarda a meta informação devolvida ao chamador nas variáveis locais
        // z_data_type, z_coll_seq, notnull, primarykey e autoinc. Neste ponto há duas
        // possibilidades:
        //
        //     1. O nome da coluna era "rowid", "oid" ou "_rowid_" e não há coluna IPK
        //        declarada explicitamente.
        //
        //     2. A tabela não é view e o nome identificou uma coluna declarada. Copia a meta
        //        informação de *pCol.
        if let Some(idx) = p_col {
            let col = &tab.a_col[idx];
            z_data_type = column_type(col, None).map(|s| s.to_vec());
            z_coll_seq = column_coll(col).map(|s| s.to_vec());
            notnull = (col.not_null != 0) as i32;
            primarykey = ((col.col_flags & COLFLAG_PRIMKEY) != 0) as i32;
            autoinc = (tab.i_p_key as i32 == i_col && (tab.tab_flags & TF_AUTOINCREMENT) != 0) as i32;
        } else {
            z_data_type = Some(b"INTEGER".to_vec());
            primarykey = 1;
        }
        if z_coll_seq.is_none() {
            z_coll_seq = Some(SQLITE_STR_BINARY.to_vec());
        }
    }
    // error_out:
    btree_leave_all(&mut db_b);

    // Tendo a chamada sucesso ou falha, os parâmetros de saída recebem o que as variáveis locais
    // contêm. Se houve erro, isso zera todos os parâmetros de saída.
    if let Some(p) = pz_data_type {
        *p = z_data_type;
    }
    if let Some(p) = pz_coll_seq {
        *p = z_coll_seq;
    }
    if let Some(p) = p_not_null {
        *p = notnull;
    }
    if let Some(p) = p_primary_key {
        *p = primarykey;
    }
    if let Some(p) = p_autoinc {
        *p = autoinc;
    }

    if SQLITE_OK == rc && p_tab.is_none() {
        // "no such table column: %s.%s" (um %s nulo sai como "(null)")
        let mut msg: Vec<u8> = Vec::new();
        msg.extend_from_slice(b"no such table column: ");
        msg.extend_from_slice(z_table_name);
        msg.push(b'.');
        msg.extend_from_slice(z_column_name.unwrap_or(b"(null)"));
        z_err_msg = Some(msg);
        rc = SQLITE_ERROR;
    }
    error_with_msg(&mut db_b, rc, z_err_msg.as_deref());
    rc = api_exit(&mut db_b, rc);
    drop(db_b);
    mutex_leave(mutex.as_ref());
    rc
}


// ---- part_010.rs ----

// Notas para o integrador (tech lead):
//  - `sqlite3_file_control(db, zDbName, op, void *pArg)` recebe `pArg` como
//    `Option<&mut dyn Any>`. O tipo concreto esperado por operação é o mesmo que o C
//    converteria de `void*`: `u32` para SQLITE_FCNTL_DATA_VERSION, `i32` para
//    SQLITE_FCNTL_RESERVE_BYTES, e o que o método xFileControl do VFS esperar nas demais.
//  - As operações SQLITE_FCNTL_FILE_POINTER, VFS_POINTER e JOURNAL_POINTER devolvem em C um
//    ponteiro cru para o arquivo ou para o VFS do pager. Sem ponteiros, o destino é um
//    `FcntlPointerOut`, que registra se o ponteiro existiria (não nulo).
//  - O `sqlite3_test_control(int op, ...)` variádico recebe os argumentos como um
//    `Vec<TestControlArg>`, na ordem em que o C faria `va_arg`.
//  - `sqlite3GlobalConfig` é acessado por `config_mut()` (acesso exclusivo de curta duração).
//  - `sqlite3_vfs_find` é `vfs_find(Option<&[u8]>) -> Option<Rc<RefCell<Sqlite3Vfs>>>`.
//  - Funções assumidas pela regra de nomes: `db_name_to_btree(&Sqlite3, Option<&[u8]>) ->
//    Option<BtreeRef>`, `pager_data_version(&Pager) -> u32`, `result_int_real(&mut
//    Sqlite3Context)`.

/// Destino de SQLITE_FCNTL_FILE_POINTER, VFS_POINTER e JOURNAL_POINTER: `present` fica
/// verdadeiro quando o ponteiro que o C devolveria em `*pArg` não seria nulo.
pub struct FcntlPointerOut {
    pub present: bool,
}

/// Argumento variádico de `sqlite3_test_control()`. Cada variante corresponde a um tipo que o C
/// lê com `va_arg`. As variantes `*Out` representam os ponteiros de saída.
pub enum TestControlArg<'a> {
    Int(i32),
    UInt(u32),
    Double(f64),
    Db(Option<Sqlite3Ref>),
    Str(Option<Vec<u8>>),
    IntSlice(&'a mut [i32]),
    IntOut(&'a mut i32),
    U32Out(&'a mut u32),
    U64Out(&'a mut u64),
    FaultCallback(Option<fn(i32) -> i32>),
    BenignHook(Option<std::rc::Rc<dyn Fn()>>),
    AltLocaltime(Option<fn(&[u8], &mut [u8]) -> i32>),
    Ctx(&'a mut Sqlite3Context),
}

/// `va_arg(ap, int)`.
fn va_int<'a>(ap: &mut std::vec::IntoIter<TestControlArg<'a>>) -> i32 {
    match ap.next() {
        Some(TestControlArg::Int(v)) => v,
        Some(TestControlArg::UInt(v)) => v as i32,
        _ => 0,
    }
}

/// `va_arg(ap, unsigned int)`.
fn va_uint<'a>(ap: &mut std::vec::IntoIter<TestControlArg<'a>>) -> u32 {
    match ap.next() {
        Some(TestControlArg::UInt(v)) => v,
        Some(TestControlArg::Int(v)) => v as u32,
        _ => 0,
    }
}

/// `va_arg(ap, double)`.
fn va_double<'a>(ap: &mut std::vec::IntoIter<TestControlArg<'a>>) -> f64 {
    match ap.next() {
        Some(TestControlArg::Double(v)) => v,
        _ => 0.0,
    }
}

/// `va_arg(ap, sqlite3*)`.
fn va_db<'a>(ap: &mut std::vec::IntoIter<TestControlArg<'a>>) -> Option<Sqlite3Ref> {
    match ap.next() {
        Some(TestControlArg::Db(v)) => v,
        _ => None,
    }
}

/// `va_arg(ap, const char*)`.
fn va_str<'a>(ap: &mut std::vec::IntoIter<TestControlArg<'a>>) -> Vec<u8> {
    match ap.next() {
        Some(TestControlArg::Str(Some(v))) => v,
        _ => Vec::new(),
    }
}

/// Acesso ao destino `T` de `pArg` (o `*(T*)pArg` do C).
fn fcntl_slot<'b, T: 'static>(arg: &'b mut Option<&mut dyn std::any::Any>) -> Option<&'b mut T> {
    arg.as_deref_mut().and_then(|a| a.downcast_mut::<T>())
}

/// Dorme por um tempo. Devolve a quantidade de tempo dormida.
pub fn sleep(ms: i32) -> i32 {
    let p_vfs = match vfs_find(None) {
        Some(v) => v,
        None => return 0,
    };

    // Esta função trabalha em milissegundos, mas a API OsSleep() por baixo usa
    // microssegundos. Daí os 1000.
    os_sleep(&p_vfs.borrow(), if ms < 0 { 0 } else { 1000i32.wrapping_mul(ms) }) / 1000
}

/// Habilita ou desabilita os códigos de resultado estendidos.
pub fn extended_result_codes(db: &Sqlite3Ref, onoff: i32) -> i32 {
    let mutex = db.borrow().mutex.clone();
    mutex_enter(mutex.as_ref());
    db.borrow_mut().err_mask = if onoff != 0 { 0xffffffffu32 as i32 } else { 0xff };
    mutex_leave(mutex.as_ref());
    SQLITE_OK
}

/// Invoca o método xFileControl em um banco de dados específico.
pub fn file_control(
    db: &Sqlite3Ref,
    z_db_name: Option<&[u8]>,
    op: i32,
    mut arg: Option<&mut dyn std::any::Any>,
) -> i32 {
    let mut rc = SQLITE_ERROR;

    let mutex = db.borrow().mutex.clone();
    mutex_enter(mutex.as_ref());
    let p_btree = db_name_to_btree(&db.borrow(), z_db_name);
    if let Some(p_btree) = p_btree {
        btree_enter(&mut p_btree.borrow_mut());
        let p_pager = btree_pager(&p_btree.borrow());
        debug_assert!(p_pager.is_some());
        let p_pager = p_pager.unwrap();
        debug_assert!(p_pager.borrow().fd.is_some());
        if op == SQLITE_FCNTL_FILE_POINTER {
            if let Some(slot) = fcntl_slot::<FcntlPointerOut>(&mut arg) {
                slot.present = pager_file(&p_pager.borrow()).is_some();
            }
            rc = SQLITE_OK;
        } else if op == SQLITE_FCNTL_VFS_POINTER {
            if let Some(slot) = fcntl_slot::<FcntlPointerOut>(&mut arg) {
                slot.present = pager_vfs(&p_pager.borrow()).is_some();
            }
            rc = SQLITE_OK;
        } else if op == SQLITE_FCNTL_JOURNAL_POINTER {
            if let Some(slot) = fcntl_slot::<FcntlPointerOut>(&mut arg) {
                slot.present = pager_jrnl_file(&p_pager.borrow()).is_some();
            }
            rc = SQLITE_OK;
        } else if op == SQLITE_FCNTL_DATA_VERSION {
            if let Some(slot) = fcntl_slot::<u32>(&mut arg) {
                *slot = pager_data_version(&p_pager.borrow());
            }
            rc = SQLITE_OK;
        } else if op == SQLITE_FCNTL_RESERVE_BYTES {
            let mut i_new: i32 = 0;
            if let Some(slot) = fcntl_slot::<i32>(&mut arg) {
                i_new = *slot;
                *slot = btree_get_requested_reserve(&p_btree);
            }
            if i_new >= 0 && i_new <= 255 {
                btree_set_page_size(&p_btree, 0, i_new, 0);
            }
            rc = SQLITE_OK;
        } else if op == SQLITE_FCNTL_RESET_CACHE {
            btree_clear_cache(&p_btree);
            rc = SQLITE_OK;
        } else {
            let n_save = db.borrow().busy_handler.n_busy;
            rc = {
                let mut pager = p_pager.borrow_mut();
                match pager.fd.as_mut() {
                    Some(fd) => os_file_control(&mut **fd, op, arg.as_deref_mut()),
                    None => SQLITE_NOTFOUND,
                }
            };
            db.borrow_mut().busy_handler.n_busy = n_save;
        }
        btree_leave(&mut p_btree.borrow_mut());
    }
    mutex_leave(mutex.as_ref());
    rc
}

/// Interface para a lógica de teste.
pub fn test_control(op: i32, args: Vec<TestControlArg>) -> i32 {
    let mut rc: i32 = 0;
    let mut ap = args.into_iter();
    match op {
        // Salva o estado atual do PRNG.
        SQLITE_TESTCTRL_PRNG_SAVE => {
            prng_save_state();
        }

        // Restaura o estado do PRNG para o último estado salvo com PRNG_SAVE. Se PRNG_SAVE
        // nunca foi chamado antes, este verbo age como PRNG_RESET.
        SQLITE_TESTCTRL_PRNG_RESTORE => {
            prng_restore_state();
        }

        // sqlite3_test_control(SQLITE_TESTCTRL_PRNG_SEED, int x, sqlite3 *db);
        //
        // Controla a semente do gerador de números pseudoaleatórios (PRNG) embutido no
        // SQLite. Casos:
        //
        //    x!=0 && db!=0       Semeia o PRNG com o valor atual do cookie de esquema do
        //                        banco principal de db, ou com x se o cookie for zero.
        //                        Este caso é conveniente com fuzzers de banco de dados,
        //                        pois dá ao fuzzer algum controle sobre a semente.
        //
        //    x!=0 && db==0       Semeia o PRNG com o valor de x.
        //
        //    x==0 && db==0       Volta ao comportamento padrão de usar o método
        //                        xRandomness do VFS primário.
        //
        // Este controle de teste também zera o PRNG para que a nova semente seja usada na
        // próxima chamada a sqlite3_randomness().
        SQLITE_TESTCTRL_PRNG_SEED => {
            let mut x = va_int(&mut ap);
            let db = va_db(&mut ap);
            if let Some(db) = &db {
                let y = {
                    let db_b = db.borrow();
                    debug_assert!(db_b.a_db[0].p_schema.is_some());
                    let schema = db_b.a_db[0].p_schema.as_ref().unwrap();
                    let cookie = schema.borrow().schema_cookie;
                    cookie
                };
                if y != 0 {
                    x = y;
                }
            }
            config_mut().i_prng_seed = x as u32;
            randomness(0, &mut []);
        }

        // sqlite3_test_control(SQLITE_TESTCTRL_FK_NO_ACTION, sqlite3 *db, int b);
        //
        // Se b for verdadeiro, ativa a configuração SQLITE_FkNoAction. Se b for falso,
        // desativa. Com SQLITE_FkNoAction ativa, todas as ações ON DELETE e ON UPDATE de
        // chave estrangeira se comportam como NO ACTION, independentemente de como foram
        // definidas.
        //
        // NB: Normalmente é preciso rodar "PRAGMA writable_schema=RESET" depois de usar
        // este controle de teste, antes que ele tenha efeito completo. Não zerar o esquema
        // pode causar comportamento inesperado.
        SQLITE_TESTCTRL_FK_NO_ACTION => {
            let db = va_db(&mut ap);
            let b = va_int(&mut ap);
            if let Some(db) = &db {
                if b != 0 {
                    db.borrow_mut().flags |= SQLITE_FK_NO_ACTION;
                } else {
                    db.borrow_mut().flags &= !SQLITE_FK_NO_ACTION;
                }
            }
        }

        // sqlite3_test_control(BITVEC_TEST, size, program)
        //
        // Roda um teste contra um objeto Bitvec de tamanho size. O argumento program é um
        // vetor de inteiros que define o teste. Devolve -1 em erro de alocação de memória,
        // 0 em sucesso, ou diferente de zero em caso de erro. Veja bitvec_builtin_test()
        // para informações adicionais.
        SQLITE_TESTCTRL_BITVEC_TEST => {
            let sz = va_int(&mut ap);
            rc = match ap.next() {
                Some(TestControlArg::IntSlice(a_prog)) => bitvec_builtin_test(sz, a_prog),
                _ => bitvec_builtin_test(sz, &mut []),
            };
        }

        // sqlite3_test_control(FAULT_INSTALL, xCallback)
        //
        // Providencia a invocação de xCallback sempre que sqlite3FaultSim() for chamada,
        // se xCallback não for NULL.
        //
        // Como teste do próprio mecanismo do simulador de falhas, sqlite3FaultSim(0) é
        // chamada logo depois de instalar o novo callback, e o valor devolvido por
        // sqlite3FaultSim(0) vira o retorno de sqlite3_test_control().
        SQLITE_TESTCTRL_FAULT_INSTALL => {
            let x_callback = match ap.next() {
                Some(TestControlArg::FaultCallback(f)) => f,
                _ => None,
            };
            config_mut().x_test_callback = x_callback;
            rc = fault_sim(0);
        }

        // sqlite3_test_control(BENIGN_MALLOC_HOOKS, xBegin, xEnd)
        //
        // Registra os ganchos chamados para indicar quais falhas de malloc() são benignas.
        SQLITE_TESTCTRL_BENIGN_MALLOC_HOOKS => {
            let x_benign_begin = match ap.next() {
                Some(TestControlArg::BenignHook(f)) => f,
                _ => None,
            };
            let x_benign_end = match ap.next() {
                Some(TestControlArg::BenignHook(f)) => f,
                _ => None,
            };
            benign_malloc_hooks(x_benign_begin, x_benign_end);
        }

        // sqlite3_test_control(SQLITE_TESTCTRL_PENDING_BYTE, unsigned int X)
        //
        // Ajusta o byte PENDING para o valor do argumento, se X>0. Não muda nada se X==0.
        // Devolve o valor do byte pending como existia antes desta rotina ser chamada.
        //
        // IMPORTANTE: Mudar o byte PENDING de 0x40000000 resulta em um formato de arquivo
        // de banco incompatível. Mudar o byte PENDING enquanto qualquer conexão de banco
        // estiver aberta resulta em comportamento indefinido e deletério.
        SQLITE_TESTCTRL_PENDING_BYTE => {
            rc = pending_byte() as i32;
            let new_val = va_uint(&mut ap);
            if new_val != 0 {
                SQLITE_PENDING_BYTE.store(new_val as i32, std::sync::atomic::Ordering::SeqCst);
            }
        }

        // sqlite3_test_control(SQLITE_TESTCTRL_ASSERT, int X)
        //
        // No C, esta ação testa em tempo de execução se assert() estava habilitado na
        // compilação. O Debian compila com NDEBUG: o assert() some, o argumento nem chega
        // a ser lido do va_list, `x` continua zero e o retorno é zero.
        SQLITE_TESTCTRL_ASSERT => {
            let x: i32 = 0;
            rc = x;
        }

        // sqlite3_test_control(SQLITE_TESTCTRL_ALWAYS, int X)
        //
        // Esta ação testa em tempo de execução como as macros ALWAYS e NEVER foram
        // definidas na compilação.
        //
        // O retorno é ALWAYS(X) se X for verdadeiro, ou 0 se X for falso.
        //
        // O teste recomendado é X==2. Se o retorno for 2, ALWAYS() e NEVER() são ambas
        // macros de repasse sem efeito, que é a configuração padrão (e a do Debian).
        SQLITE_TESTCTRL_ALWAYS => {
            let x = va_int(&mut ap);
            rc = if x != 0 { x } else { 0 };
        }

        // sqlite3_test_control(SQLITE_TESTCTRL_BYTEORDER);
        //
        // O inteiro devolvido revela a ordem de bytes do computador em que o SQLite roda:
        //
        //       1     big-endian,    determinado em tempo de execução
        //      10     little-endian, determinado em tempo de execução
        //  432101     big-endian,    determinado em tempo de compilação
        //  123410     little-endian, determinado em tempo de compilação
        SQLITE_TESTCTRL_BYTEORDER => {
            // x86-64 do Debian: SQLITE_BYTEORDER 1234, little-endian em tempo de compilação,
            // logo 1234*100 + 1*10 + 0.
            rc = 123410;
        }

        // sqlite3_test_control(SQLITE_TESTCTRL_OPTIMIZATIONS, sqlite3 *db, int N)
        //
        // Habilita ou desabilita várias otimizações para fins de teste. O argumento N é
        // uma máscara de bits das otimizações a desabilitar. Em operação normal N deve
        // ser 0. A ideia é que um programa de teste (como o SQL Logic Test ou o módulo
        // SLT) rode o mesmo SQL várias vezes com otimizações desabilitadas para verificar
        // que a mesma resposta sai em todos os casos.
        SQLITE_TESTCTRL_OPTIMIZATIONS => {
            let db = va_db(&mut ap);
            let flags = va_uint(&mut ap);
            if let Some(db) = &db {
                db.borrow_mut().db_opt_flags = flags;
            }
        }

        // sqlite3_test_control(SQLITE_TESTCTRL_LOCALTIME_FAULT, onoff, xAlt);
        //
        // Se o parâmetro onoff for 1, as chamadas seguintes a localtime() falham. Se for
        // 2, invoca xAlt() no lugar de localtime(). Se for 0, processamento normal.
        //
        // xAlt deve escrever o resultado no objeto struct tm do segundo argumento e
        // devolver zero em sucesso, ou diferente de zero em falha.
        SQLITE_TESTCTRL_LOCALTIME_FAULT => {
            let b_fault = va_int(&mut ap);
            let x_alt = if b_fault == 2 {
                match ap.next() {
                    Some(TestControlArg::AltLocaltime(f)) => f,
                    _ => None,
                }
            } else {
                None
            };
            let mut cfg = config_mut();
            cfg.b_localtime_fault = b_fault;
            cfg.x_alt_localtime = x_alt;
        }

        // sqlite3_test_control(SQLITE_TESTCTRL_INTERNAL_FUNCTIONS, sqlite3*);
        //
        // Liga ou desliga a capacidade de usar funções internas na conexão de banco dada
        // no argumento.
        SQLITE_TESTCTRL_INTERNAL_FUNCTIONS => {
            let db = va_db(&mut ap);
            if let Some(db) = &db {
                db.borrow_mut().m_db_flags ^= DBFLAG_INTERNAL_FUNC;
            }
        }

        // sqlite3_test_control(SQLITE_TESTCTRL_NEVER_CORRUPT, int);
        //
        // Liga ou desliga um flag que indica que o arquivo de banco está sempre bem
        // formado e nunca corrompido. O flag fica desligado por padrão, indicando que os
        // arquivos de banco podem ter corrupção arbitrária. Ligar o flag durante testes
        // ativa certos assert() que demonstram invariantes de arquivos bem formados.
        SQLITE_TESTCTRL_NEVER_CORRUPT => {
            config_mut().never_corrupt = va_int(&mut ap);
        }

        // sqlite3_test_control(SQLITE_TESTCTRL_EXTRA_SCHEMA_CHECKS, int);
        //
        // Liga ou desliga um flag que faz o SQLite verificar os campos type, name e
        // tbl_name da tabela sqlite_schema. Normalmente fica ligado, mas às vezes é útil
        // desligá-lo para testes.
        //
        // 2020-07-22: Desligar EXTRA_SCHEMA_CHECKS também desliga a verificação dos
        // números de página raiz ao interpretar o esquema. Isso facilita alcançar estados
        // de erro interno estranhos durante os testes. A configuração EXTRA_SCHEMA_CHECKS
        // fica sempre ligada em produção.
        SQLITE_TESTCTRL_EXTRA_SCHEMA_CHECKS => {
            config_mut().b_extra_schema_checks = va_int(&mut ap) as u8;
        }

        // Ajusta o limiar em que os contadores de OP_Once voltam a zero. Por padrão é
        // 0x7ffffffe (mais de 2 bilhões), mas esse valor é grande demais para testar em
        // tempo razoável, então este controle existe para definir um valor de reinício
        // pequeno e fácil de alcançar.
        SQLITE_TESTCTRL_ONCE_RESET_THRESHOLD => {
            config_mut().i_once_reset_threshold = va_int(&mut ap);
        }

        // sqlite3_test_control(SQLITE_TESTCTRL_VDBE_COVERAGE, xCallback, ptr);
        //
        // Ajusta a função de callback de cobertura do VDBE. O Debian não liga
        // SQLITE_VDBE_COVERAGE: o corpo some e nenhum argumento é lido.
        SQLITE_TESTCTRL_VDBE_COVERAGE => {}

        // sqlite3_test_control(SQLITE_TESTCTRL_SORTER_MMAP, db, nMax);
        SQLITE_TESTCTRL_SORTER_MMAP => {
            let db = va_db(&mut ap);
            let n_max = va_int(&mut ap);
            if let Some(db) = &db {
                db.borrow_mut().n_max_sorter_mmap = n_max;
            }
        }

        // sqlite3_test_control(SQLITE_TESTCTRL_ISINIT);
        //
        // Devolve SQLITE_OK se o SQLite foi inicializado e SQLITE_ERROR se não.
        SQLITE_TESTCTRL_ISINIT => {
            if config_mut().is_init == 0 {
                rc = SQLITE_ERROR;
            }
        }

        // sqlite3_test_control(SQLITE_TESTCTRL_IMPOSTER, db, dbName, onOff, tnum);
        //
        // Este controle de teste é usado para criar tabelas impostoras. "db" é a conexão
        // de banco. dbName é o nome do banco (ex: "main" ou "temp") que receberá a
        // impostora. "onOff" liga ou desliga o modo impostor. "tnum" é a página raiz da
        // b-tree à qual a tabela impostora deve se conectar.
        //
        // Habilite o modo impostor só quando o esquema já tiver sido interpretado. Depois
        // rode um único CREATE TABLE para construir a tabela impostora no esquema
        // interpretado. Depois desligue o modo impostor de novo.
        //
        // Se onOff==0 e tnum>0, zera o esquema de todos os bancos, fazendo o esquema ser
        // reinterpretado na próxima vez em que for necessário. Isso tem o efeito de
        // apagar todas as tabelas impostoras.
        SQLITE_TESTCTRL_IMPOSTER => {
            let db = va_db(&mut ap);
            let z_name = va_str(&mut ap);
            let on_off = va_int(&mut ap);
            let tnum = va_int(&mut ap);
            if let Some(db) = &db {
                let mutex = db.borrow().mutex.clone();
                mutex_enter(mutex.as_ref());
                let i_db = find_db_name(&db.borrow(), &z_name);
                if i_db >= 0 {
                    let reset = {
                        let mut db_m = db.borrow_mut();
                        db_m.init.i_db = i_db as u8;
                        // imposterTable é um campo de 1 bit: a atribuição encadeada do C
                        // copia o valor já truncado também para busy.
                        let v = (on_off & 1) as u8;
                        db_m.init.imposter_table = v;
                        db_m.init.busy = v;
                        db_m.init.new_tnum = tnum as Pgno;
                        db_m.init.busy == 0 && db_m.init.new_tnum > 0
                    };
                    if reset {
                        reset_all_schemas_of_connection(&mut db.borrow_mut());
                    }
                }
                mutex_leave(mutex.as_ref());
            }
        }

        // sqlite3_test_control(SQLITE_TESTCTRL_RESULT_INTREAL, sqlite3_context*);
        //
        // Este controle de teste faz o valor do sqlite3_result_int64() mais recente ser
        // interpretado como MEM_IntReal em vez de MEM_Int. Normalmente, valores MEM_IntReal
        // só surgem durante um INSERT de valores inteiros em uma coluna REAL, então são
        // difíceis de testar. Este controle permite escrever uma função SQL intreal() que
        // injeta um valor intreal() em lugares arbitrários de uma instrução SQL, para
        // fins de teste.
        SQLITE_TESTCTRL_RESULT_INTREAL => {
            if let Some(TestControlArg::Ctx(p_ctx)) = ap.next() {
                result_int_real(p_ctx);
            }
        }

        // sqlite3_test_control(SQLITE_TESTCTRL_SEEK_COUNT,
        //   sqlite3 *db,    // Conexão de banco
        //   u64 *pnSeek     // Escreve a contagem de seeks aqui
        // );
        //
        // Este controle de teste consulta o contador de seeks do arquivo de banco "main".
        // O contador é escrito em *pnSeek e depois zerado. A contagem de seeks só está
        // disponível se compilado com SQLITE_DEBUG.
        SQLITE_TESTCTRL_SEEK_COUNT => {
            let db = va_db(&mut ap);
            if let (Some(_db), Some(TestControlArg::U64Out(pn))) = (db, ap.next()) {
                // sqlite3BtreeSeekCount(X) é a macro `0` sem SQLITE_DEBUG (btree.h).
                *pn = 0;
            }
        }

        // sqlite3_test_control(SQLITE_TESTCTRL_TRACEFLAGS, op, ptr)
        //
        //  "ptr" é um ponteiro para um u32.
        //
        //   op==0       Guarda o sqlite3TreeTrace atual em *ptr
        //   op==1       Ajusta sqlite3TreeTrace para o valor de *ptr
        //   op==2       Guarda o sqlite3WhereTrace atual em *ptr
        //   op==3       Ajusta sqlite3WhereTrace para o valor de *ptr
        SQLITE_TESTCTRL_TRACEFLAGS => {
            let op_trace = va_int(&mut ap);
            if let Some(TestControlArg::U32Out(ptr)) = ap.next() {
                use std::sync::atomic::Ordering::SeqCst;
                match op_trace {
                    0 => *ptr = SQLITE_TREE_TRACE.load(SeqCst),
                    1 => SQLITE_TREE_TRACE.store(*ptr, SeqCst),
                    2 => *ptr = SQLITE_WHERE_TRACE.load(SeqCst),
                    3 => SQLITE_WHERE_TRACE.store(*ptr, SeqCst),
                    _ => {}
                }
            }
        }

        // sqlite3_test_control(SQLITE_TESTCTRL_LOGEST,
        //      double fIn,     // Valor de entrada
        //      int *pLogEst,   // sqlite3LogEstFromDouble(fIn)
        //      u64 *pInt,      // sqlite3LogEstToInt(*pLogEst)
        //      int *pLogEst2   // sqlite3LogEst(*pInt)
        // );
        //
        // Acesso de teste às rotinas de conversão de LogEst.
        SQLITE_TESTCTRL_LOGEST => {
            let r_in = va_double(&mut ap);
            let r_log_est = log_est_from_double(r_in);
            let p_i1 = ap.next();
            let p_u64 = ap.next();
            let p_i2 = ap.next();
            if let (
                Some(TestControlArg::IntOut(p_i1)),
                Some(TestControlArg::U64Out(p_u64)),
                Some(TestControlArg::IntOut(p_i2)),
            ) = (p_i1, p_u64, p_i2)
            {
                *p_i1 = r_log_est as i32;
                *p_u64 = log_est_to_int(r_log_est);
                *p_i2 = log_est(*p_u64) as i32;
            }
        }

        // sqlite3_test_control(SQLITE_TESTCTRL_USELONGDOUBLE, int X);
        //
        //   X<0     Não muda bUseLongDouble. Só informa o valor.
        //   X==0    Desabilita bUseLongDouble
        //   X==1    Habilita bUseLongDouble
        //   X>=2    Ajusta bUseLongDouble para seu valor padrão nesta plataforma
        SQLITE_TESTCTRL_USELONGDOUBLE => {
            let mut b = va_int(&mut ap);
            if b >= 2 {
                b = has_high_precision_double(b);
            }
            let mut cfg = config_mut();
            if b >= 0 {
                cfg.b_use_long_double = (b > 0) as u8;
            }
            rc = (cfg.b_use_long_double != 0) as i32;
        }

        // sqlite3_test_control(SQLITE_TESTCTRL_JSON_SELFCHECK, &onOff);
        //
        // A validação do JSONB só existe se compilado com SQLITE_DEBUG: no Debian o corpo
        // some e o argumento não é lido.
        SQLITE_TESTCTRL_JSON_SELFCHECK => {}

        _ => {}
    }
    rc
}


// ---- part_011.rs ----

// Modelo de memória dos nomes de arquivo: o `const char *` do C que aponta para o nome do banco de
// dados dentro do bloco do pager (nome do banco, parâmetros de URI, nome do journal e nome do WAL,
// um após o outro, cada texto terminado em NUL) vira `&[u8]` que começa no primeiro byte do nome do
// banco de dados e vai até o fim do bloco. Os quatro bytes zero que o C coloca antes do nome ficam
// fora da fatia; por isso `database_name` já recebe a fatia no começo do nome.

/// Comprimento do texto terminado em NUL que começa em `z`. Se a fatia não tem NUL (valor de
/// parâmetro já recortado por `uri_parameter`), vale a fatia inteira.
fn filename_len(z: &[u8]) -> usize {
    z.iter().position(|&c| c == 0).unwrap_or(z.len())
}

/// Pula o texto terminado em NUL que começa em `z` e devolve a fatia logo depois do NUL.
fn filename_next(z: &[u8]) -> &[u8] {
    &z[(filename_len(z) + 1).min(z.len())..]
}

/// O texto terminado em NUL que começa em `z`, sem o NUL.
fn filename_str(z: &[u8]) -> &[u8] {
    &z[..filename_len(z)]
}

/// O Pager guarda o nome do banco de dados, o nome do journal e o nome do WAL consecutivamente na
/// memória, nessa ordem. O nome do banco de dados é precedido por quatro bytes zero. No C, acha-se
/// o começo do nome do banco procurando para trás o primeiro byte depois de quatro zeros seguidos.
///
/// Isso só funciona se o nome passado foi obtido do Pager. Aqui a fatia já começa no nome do banco
/// (os quatro zeros ficam fora dela), então a busca para trás termina de imediato.
fn database_name(z_name: &[u8]) -> &[u8] {
    z_name
}

/// Acrescenta o texto `z` ao fim de `p`, junto com o terminador NUL.
fn append_text(p: &mut Vec<u8>, z: &[u8]) {
    p.extend_from_slice(filename_str(z));
    p.push(0);
}

/// Aloca memória para guardar os nomes de um banco de dados, do arquivo de journal, do arquivo WAL
/// e os parâmetros de consulta. O valor devolvido é válido para uso por `sqlite3_filename_database()`
/// e `sqlite3_uri_parameter()` e funções relacionadas.
///
/// O layout da memória precisa ser compatível com o gerado pelo pager e esperado por
/// `sqlite3_uri_parameter()` e `database_name()`. O resultado começa no nome do banco de dados (o
/// `pResult + 4` do C); os quatro zeros iniciais não fazem parte da fatia.
pub fn create_filename(
    z_database: &[u8],
    z_journal: &[u8],
    z_wal: &[u8],
    az_param: &[(Vec<u8>, Vec<u8>)],
) -> Vec<u8> {
    let mut n_byte: i64 = (filename_str(z_database).len()
        + filename_str(z_journal).len()
        + filename_str(z_wal).len()
        + 10) as i64;
    for (z_key, z_value) in az_param.iter() {
        n_byte += filename_str(z_key).len() as i64 + 1;
        n_byte += filename_str(z_value).len() as i64 + 1;
    }
    let mut p_result: Vec<u8> = Vec::with_capacity(n_byte as usize);
    p_result.extend_from_slice(&[0, 0, 0, 0]);
    append_text(&mut p_result, z_database);
    for (z_key, z_value) in az_param.iter() {
        append_text(&mut p_result, z_key);
        append_text(&mut p_result, z_value);
    }
    p_result.push(0);
    append_text(&mut p_result, z_journal);
    append_text(&mut p_result, z_wal);
    p_result.push(0);
    p_result.push(0);
    assert!(p_result.len() as i64 == n_byte);
    p_result.drain(..4);
    p_result
}

/// Libera a memória obtida de `create_filename()`. É um erro grave chamar esta rotina com qualquer
/// parâmetro que não seja um valor obtido antes de `create_filename()` ou `None`.
pub fn free_filename(p: Option<Vec<u8>>) {
    drop(p);
}

/// Rotina utilitária, útil para implementações de VFS, que verifica se um arquivo de banco de
/// dados era uma URI que continha um parâmetro de consulta específico e, se sim, obtém o valor do
/// parâmetro.
///
/// O argumento `z_filename` é o nome passado ao método xOpen() de uma implementação de VFS. O
/// argumento `z_param` é o nome do parâmetro de consulta procurado. Devolve o valor do parâmetro se
/// ele existir; se não existir, devolve `None`. Colide em nome com a `uriParameter` estática
/// (parte 8, `uri_parameter`), por isso a pública se chama `uri_parameter_public`.
pub fn uri_parameter_public<'a>(
    z_filename: Option<&'a [u8]>,
    z_param: Option<&[u8]>,
) -> Option<&'a [u8]> {
    let (z_filename, z_param) = match (z_filename, z_param) {
        (Some(f), Some(p)) => (f, p),
        _ => return None,
    };
    let z_filename = database_name(z_filename);
    uri_parameter(z_filename, z_param)
}

/// Devolve o nome do N-ésimo parâmetro de consulta do nome de arquivo.
pub fn uri_key(z_filename: Option<&[u8]>, mut n: i32) -> Option<&[u8]> {
    let z_filename = match z_filename {
        Some(z) if n >= 0 => z,
        _ => return None,
    };
    let mut z = filename_next(database_name(z_filename));
    while !z.is_empty() && z[0] != 0 && {
        let more = n > 0;
        n -= 1;
        more
    } {
        z = filename_next(z);
        z = filename_next(z);
    }
    if !z.is_empty() && z[0] != 0 {
        Some(filename_str(z))
    } else {
        None
    }
}

/// Devolve um valor booleano para um parâmetro de consulta.
pub fn uri_boolean(z_filename: Option<&[u8]>, z_param: Option<&[u8]>, b_dflt: i32) -> i32 {
    let z = uri_parameter_public(z_filename, z_param);
    let b_dflt: u8 = (b_dflt != 0) as u8;
    match z {
        Some(z) => get_boolean(filename_str(z), b_dflt) as i32,
        None => b_dflt as i32,
    }
}

/// Devolve um valor inteiro de 64 bits para um parâmetro de consulta.
pub fn uri_int64(z_filename: Option<&[u8]>, z_param: Option<&[u8]>, mut b_dflt: i64) -> i64 {
    let z = uri_parameter_public(z_filename, z_param);
    let mut v: i64 = 0;
    if let Some(z) = z {
        if dec_or_hex_to_i_64(filename_str(z), &mut v) == 0 {
            b_dflt = v;
        }
    }
    b_dflt
}

/// Traduz um nome de arquivo entregue a uma rotina de VFS para o arquivo correspondente de banco de
/// dados, de journal ou de WAL.
///
/// É um erro passar a esta rotina um nome que não foi entregue ao VFS pelo núcleo do SQLite. Fazer
/// isso é parecido com passar a `free()` um ponteiro que não veio de `malloc()`: um erro que não se
/// detecta com facilidade.
pub fn filename_database(z_filename: Option<&[u8]>) -> Option<&[u8]> {
    z_filename.map(database_name)
}

pub fn filename_journal(z_filename: Option<&[u8]>) -> Option<&[u8]> {
    let z_filename = z_filename?;
    let mut z = filename_next(database_name(z_filename));
    while !z.is_empty() && z[0] != 0 {
        z = filename_next(z);
        z = filename_next(z);
    }
    Some(&z[1..])
}

pub fn filename_wal(z_filename: Option<&[u8]>) -> Option<&[u8]> {
    // SQLITE_OMIT_WAL não está definido: o WAL existe.
    filename_journal(z_filename).map(filename_next)
}

/// Devolve o Btree identificado por `z_db_name`. Devolve `None` se não for encontrado.
pub fn db_name_to_btree(db: &Sqlite3, z_db_name: Option<&[u8]>) -> Option<BtreeRef> {
    let i_db = match z_db_name {
        Some(z) => find_db_name(db, z),
        None => 0,
    };
    if i_db < 0 {
        None
    } else {
        db.a_db[i_db as usize].p_bt.clone()
    }
}

/// Devolve o nome do N-ésimo esquema de banco de dados. Devolve `None` se N estiver fora da faixa.
pub fn db_name(db: &Sqlite3, n: i32) -> Option<&[u8]> {
    if n < 0 || n >= db.n_db {
        None
    } else {
        db.a_db[n as usize].z_db_sname.as_deref()
    }
}

/// Devolve o nome de arquivo do banco de dados associado a uma conexão.
pub fn db_filename(db: &Sqlite3, z_db_name: Option<&[u8]>) -> Option<Vec<u8>> {
    db_name_to_btree(db, z_db_name).map(btree_get_filename)
}

/// Devolve 1 se o banco de dados é somente leitura ou 0 se é de leitura e escrita. Devolve -1 se
/// o banco de dados não existe.
pub fn db_readonly(db: &Sqlite3, z_db_name: Option<&[u8]>) -> i32 {
    match db_name_to_btree(db, z_db_name) {
        Some(p_bt) => btree_is_readonly(&p_bt),
        None => -1,
    }
}

// As rotinas `sqlite3_snapshot_get`, `sqlite3_snapshot_open`, `sqlite3_snapshot_recover` e
// `sqlite3_snapshot_free` só existem sob SQLITE_ENABLE_SNAPSHOT, que o Debian 13 não define; por
// isso não são traduzidas.


// ---- part_012.rs ----

/// Dado o nome de uma opção de tempo de compilação, devolve verdadeiro se ela foi usada e falso
/// se não. O nome pode começar com "SQLITE_", mas o prefixo não é obrigatório para casar.
pub fn compileoption_used(z_opt_name: &[u8]) -> i32 {
    let az_compile_opt = compile_options();
    let n_opt = az_compile_opt.len();

    let mut z_opt_name = z_opt_name;
    if str_n_i_cmp(z_opt_name, b"SQLITE_", 7) == 0 {
        z_opt_name = &z_opt_name[7..];
    }
    let n = strlen30(z_opt_name);

    // Como n_opt costuma ter um só algarismo, a busca linear basta. Não precisa de busca binária.
    for i in 0..n_opt {
        let opt = az_compile_opt[i].as_bytes();
        // O C lê o byte seguinte ao prefixo, que pode ser o terminador NUL da string.
        let next = if (n as usize) < opt.len() { opt[n as usize] } else { 0 };
        if str_n_i_cmp(z_opt_name, opt, n) == 0 && is_id_char(next) == 0 {
            return 1;
        }
    }
    0
}

/// Devolve a N-ésima string de opção de tempo de compilação. Se N estiver fora da faixa,
/// devolve `None` (o ponteiro nulo do C).
pub fn compileoption_get(n: i32) -> Option<&'static [u8]> {
    let az_compile_opt = compile_options();
    let n_opt = az_compile_opt.len() as i32;
    if n >= 0 && n < n_opt {
        return Some(az_compile_opt[n as usize].as_bytes());
    }
    None
}

