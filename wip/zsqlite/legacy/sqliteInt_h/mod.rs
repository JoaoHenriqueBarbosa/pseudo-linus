// Mesclado das partes traduzidas de sqliteInt_h (as partes quebram funções grandes no meio).
#![allow(unused_imports, ambiguous_glob_reexports)]
use crate::prelude::*;

// ---- part_000.rs ----

// Configuração de SQLITE_TCLAPI (define vazio, apenas marcador para integração Tcl).
// Em Rust, não é necessária uma declaração explícita.

// Define para flags de arquivo grande no POSIX.
// Habilitadas por padrão para suportar arquivos maiores que 2GB.
// Desabilitáveis com SQLITE_DISABLE_LFS (Debian 13 não desabilita).
pub const LARGE_FILE: i32 = 1;
pub const FILE_OFFSET_BITS: i32 = 64;
pub const LARGEFILE_SOURCE: i32 = 1;

// Versão do compilador GCC (construída a partir de __GNUC__ * 1000000 + __GNUC_MINOR__ * 1000 + __GNUC_PATCHLEVEL__).
// Retorna 0 se não compilando com GCC (e SQLITE_DISABLE_INTRINSIC não está definido).
// Valor do GCC 14.2 do Debian 13 (14*1000000 + 2*1000 + 0), que compilou o SQLite original.
pub const GCC_VERSION: i32 = 14002000;

// Versão do compilador MSVC (_MSC_VER): 0, pois o alvo é Linux.
pub const MSVC_VERSION: i32 = 0;

// Funções C99 de matemática disponíveis (SQLITE_HAVE_C99_MATH_FUNCS).
// Verdadeiro em compiladores não-MSVC ou MSVC 2013 (versão 1800) e posterior.
pub const SQLITE_HAVE_C99_MATH_FUNCS: i32 = 1; // Debian 13 tem suporte

// Hinting ao compilador para queda intencional em case (fallthrough).
// Em GCC 7+, usa __attribute__((fallthrough)); em compiladores anteriores, é vazio.
// Em Rust, match não permite fallthrough, então é apenas comentário semântico.
#[inline(always)]
pub fn deliberate_fall_through() {
    // Marcador: não há queda de case em Rust
}

// Macro para conversão de inteiro para ponteiro (SQLITE_INT_TO_PTR).
// Comportamento depende de suporte a intptr_t e HAVE_STDINT_H.
// Em Rust puro, não temos ponteiros reais; simulamos com usize.
#[inline(always)]
pub fn sqlite_int_to_ptr(x: i64) -> usize {
    x as usize
}

// Macro para conversão de ponteiro para inteiro (SQLITE_PTR_TO_INT).
// Inversa de sqlite_int_to_ptr.
#[inline(always)]
pub fn sqlite_ptr_to_int(x: usize) -> i32 {
    x as i32
}

// Hinting ao compilador para sempre inlinar uma função (SQLITE_INLINE).
// Define: __attribute__((always_inline)) inline em GCC, __forceinline em MSVC.
// Desabilitado se SQLITE_COVERAGE_TEST ou __STRICT_ANSI__ estiverem definidos.
// Debian 13: habilitado (nem coverage test, nem strict ANSI).
// Equivalente em Rust: #[inline(always)]

// Hinting ao compilador para não inlinar uma função (SQLITE_NOINLINE).
// Define: __attribute__((noinline)) em GCC, __declspec(noinline) em MSVC.
// Equivalente em Rust: #[inline(never)]

// Habilitação de intrinsics atômicos (SQLITE_ATOMIC_INTRINSICS).
// Define: 1 se __atomic_* ou __has_extension(c_atomic) disponível (GCC 4.7+, clang).
// Caso contrário, 0 (fallback para carga/armazenamento simples).
pub const SQLITE_ATOMIC_INTRINSICS: i32 = 1; // Debian 13 tem suporte

// Funções de carga/armazenamento atômico com semântica Relaxed (__ATOMIC_RELAXED).
// AtomicLoad: lê valor de forma relaxada de um ponteiro (thread-safe).
// AtomicStore: escreve valor de forma relaxada em um ponteiro (thread-safe).
// Quando SQLITE_ATOMIC_INTRINSICS = 0, fallback para leitura/escrita simples.
#[inline(always)]
pub fn atomic_load(ptr: &std::sync::atomic::AtomicI32) -> i32 {
    use std::sync::atomic::Ordering;
    ptr.load(Ordering::Relaxed)
}

#[inline(always)]
pub fn atomic_store(ptr: &std::sync::atomic::AtomicI32, val: i32) {
    use std::sync::atomic::Ordering;
    ptr.store(val, Ordering::Relaxed);
}

// Carregamento direto de overflow (SQLITE_DIRECT_OVERFLOW_READ).
// Define: 1 por padrão (habilitado). Desabilitável com SQLITE_DIRECT_OVERFLOW_READ=0.
// Permite ler além do tamanho físico da página para evitar acesso separado a overflow.
pub const SQLITE_DIRECT_OVERFLOW_READ: i32 = 1; // Debian 13 habilita

// Modo de thread-safety do SQLite (SQLITE_THREADSAFE).
// 0: mutexes desabilitados permanentemente, nunca thread-safe.
// 1: serializado (máximo nível de thread-safety).
// 2: multi-threaded (múltiplas threads compartilham SQLite, contanto que não acessem DB simultaneamente).
// Debian 13: SQLITE_THREADSAFE=1 (modo serializado).
pub const SQLITE_THREADSAFE: i32 = 1;


// ---- part_001.rs ----

// Versões antigas do SQLite usavam THREADSAFE como opcional.
// Suportamos isto por compatibilidade legada.
//
// Para garantir que o valor correto de "THREADSAFE" é reportado ao consultar
// opções de compilação em tempo de execução (ex: "PRAGMA compile_options"), a lógica
// é parcialmente replicada em ctime.c. Se atualizado aqui, deve ser atualizado lá também.
// SQLITE_THREADSAFE (=1) já está definida em part_000.rs, onde o C a define primeiro.

// Sobrescrita "powersafe" é ativada por padrão. Mas pode ser desativada usando
// a opção de linha de comando -DSQLITE_POWERSAFE_OVERWRITE=0.
pub const SQLITE_POWERSAFE_OVERWRITE: i32 = 1;

// Estatísticas de alocação de memória são ativadas por padrão, a menos que SQLite
// seja compilado com SQLITE_DEFAULT_MEMSTATUS=0, neste caso as estatísticas de
// alocação de memória são desativadas por padrão.
pub const SQLITE_DEFAULT_MEMSTATUS: i32 = 1;

// Exatamente um dos seguintes subsistemas de alocação de memória é usado:
// SQLITE_SYSTEM_MALLOC, SQLITE_WIN32_MALLOC, SQLITE_ZERO_MALLOC, SQLITE_MEMDEBUG.
// No Debian 13, SQLITE_SYSTEM_MALLOC é o padrão.

// Se SQLITE_MALLOC_SOFT_LIMIT não é zero, tenta manter os tamanhos de alocação
// de memória abaixo deste valor onde possível.
pub const SQLITE_MALLOC_SOFT_LIMIT: usize = 1024;

// NDEBUG e SQLITE_DEBUG são opostos. Deve sempre ser verdadeiro que
// defined(NDEBUG)==!defined(SQLITE_DEBUG). Se isto não é atualmente verdadeiro,
// torna-se verdadeiro definindo ou removendo NDEBUG.
//
// Configurar NDEBUG reduz o código e o torna mais rápido desabilitando as
// declarações assert() no código. Então desejamos que a ação padrão seja que
// NDEBUG seja configurado e NDEBUG seja removido apenas se SQLITE_DEBUG é
// configurado. Assim NDEBUG torna-se um recurso "opt-in" em vez de "opt-out".
// No Debian 13, SQLITE_DEBUG não é definido, portanto NDEBUG é ativo.

// SQLITE_ENABLE_EXPLAIN_COMMENTS é ativado se SQLITE_DEBUG estiver ligado.
// No Debian 13, SQLITE_DEBUG é desativado, portanto este não é ativado.

// As macros testcase(), TESTONLY(), VVA_ONLY() são usadas para testes de cobertura.
// Em compilação normal (não DEBUG, não COVERAGE_TEST), estas expandem para nada.

// ALWAYS() e NEVER() cercam expressões booleanas que são intencionadas para serem
// sempre verdadeiras ou falsas, respectivamente. Tais expressões poderiam ser omitidas
// do código completamente. Mas são incluídas em alguns casos para aumentar a resiliência
// do SQLite a comportamentos inesperados, tornando o código "auto-curável" ou "dúctil"
// em vez de "frágil" e quebrando no primeiro sinal de comportamento não planejado.
//
// Em outras palavras, ALWAYS e NEVER são adicionadas para código defensivo.
//
// Durante testes de cobertura, ALWAYS e NEVER são codificados para serem verdadeiro
// e falso para que o código inalcançável não seja contado como testado. Em compilação
// normal (Debian 13), são pass-throughs.

/// Dica de otimização do compilador: a expressão é geralmente verdadeira.
/// A implementação é um pass-through em compilação normal.
#[inline(always)]
pub fn likely<T>(x: T) -> T {
    x
}

/// Dica de otimização do compilador: a expressão é geralmente falsa.
/// A implementação é um pass-through em compilação normal.
#[inline(always)]
pub fn unlikely<T>(x: T) -> T {
    x
}

/// ALWAYS(X): asserta que X é verdadeiro. Em modo de teste, codificado para verdadeiro.
/// Em compilação normal, é um pass-through.
#[inline(always)]
pub fn always<T>(x: T) -> T {
    x
}

/// NEVER(X): asserta que X é falso. Em modo de teste, codificado para falso.
/// Em compilação normal, é um pass-through.
#[inline(always)]
pub fn never<T>(x: T) -> T {
    x
}

// OK_IF_ALWAYS_TRUE(X) e OK_IF_ALWAYS_FALSE(X) marcam condicionais que são apenas
// otimizações. Se as condicionais forem substituídas por uma constante 1 (verdadeira)
// ou 0 (falsa), a resposta correta ainda é obtida, embora talvez não tão rapidamente.
//
// Em modo de teste de mutação, estas são substituídas por constantes.
// Em compilação normal, são pass-throughs.

/// Marcador de otimização: resultado é geralmente sempre verdadeiro.
#[inline(always)]
pub fn ok_if_always_true<T>(x: T) -> T {
    x
}

/// Marcador de otimização: resultado é geralmente sempre falso.
#[inline(always)]
pub fn ok_if_always_false<T>(x: T) -> T {
    x
}

// ONLY_IF_REALLOC_STRESS(X): algumas falhas de malloc são possíveis apenas se
// SQLITE_TEST_REALLOC_STRESS está definido. Precisa-se defender contra essas falhas
// ao testar com SQLITE_TEST_REALLOC_STRESS, mas não desejamos ramos inalcançáveis
// durante uma compilação normal. Esta macro pode ser usada para desabilitar testes
// que são sempre falsos exceto quando SQLITE_TEST_REALLOC_STRESS está definido.
//
// Em compilação normal (Debian 13), sempre retorna falso.

/// Teste que só é ativo em compilação com SQLITE_TEST_REALLOC_STRESS.
/// Em compilação normal, sempre retorna falso.
#[inline(always)]
pub fn only_if_realloc_stress(_x: bool) -> bool {
    false
}

// Separador de dígitos em literais numéricos do SQLite.
pub const SQLITE_DIGIT_SEPARATOR: u8 = b'_';

/// Retorna verdadeiro (não zero) se a entrada é um inteiro que é grande demais
/// para caber em 32 bits. Esta macro é usada dentro de testcase() para verificar
/// se testamos SQLite para suporte a arquivo grande.
#[inline(always)]
pub fn is_big_int(x: i64) -> bool {
    (x & !0xffffffff_i64) != 0
}

// Número "grande" para representar valores em ponto flutuante muito altos.
pub const SQLITE_BIG_DBL: f64 = 1e99;

// OMIT_TEMPDB é configurado para 1 se SQLITE_OMIT_TEMPDB está definido, ou 0 caso contrário.
// Possuir esta constante nos permite fazer o compilador C omitir código usado por
// tabelas TEMP sem declarações #ifndef bagunçadas.
// No Debian 13, SQLITE_OMIT_TEMPDB não está definido, portanto OMIT_TEMPDB é 0.
pub const OMIT_TEMPDB: i32 = 0;

// O número do "formato de arquivo" é um inteiro que é incrementado sempre que o
// formato de arquivo a nível de VDBE muda. Os seguintes macros definem o formato
// padrão de arquivo para novos bancos de dados e o formato máximo de arquivo que
// a biblioteca pode ler.
pub const SQLITE_MAX_FILE_FORMAT: i32 = 4;

/// Formato de arquivo padrão para novos bancos de dados SQLite.
/// No Debian 13, o padrão é 4.
pub const SQLITE_DEFAULT_FILE_FORMAT: i32 = 4;

/// Determina se gatilhos são recursivos por padrão. Isto pode ser alterado em
/// tempo de execução usando um pragma.
pub const SQLITE_DEFAULT_RECURSIVE_TRIGGERS: i32 = 0;


// ---- part_002.rs ----

// Forneça um valor padrão para SQLITE_TEMP_STORE caso não seja especificado na linha de comando
pub const SQLITE_TEMP_STORE: i32 = 1;

// Se nenhum valor foi fornecido para SQLITE_MAX_WORKER_THREADS ou se
// SQLITE_TEMP_STORE é 3 (nunca usar arquivos temporários), defina-o como zero.
// Para a configuração do Debian 13, o padrão é 8.
pub const SQLITE_MAX_WORKER_THREADS: i32 = 8;
pub const SQLITE_DEFAULT_WORKER_THREADS: i32 = 0;

// Alocação inicial padrão para o pagecache ao usar caches de página separados
// para cada conexão de banco de dados. Um número positivo é a quantidade de páginas.
// Um número negativo N significa que um buffer de -1024*N bytes é alocado.
// O valor padrão "20" foi escolhido para minimizar o tempo de execução do
// teste speedtest1.
pub const SQLITE_DEFAULT_PCACHE_INITSZ: i32 = 20;

// Valor padrão para a opção SQLITE_CONFIG_SORTERREF_SIZE
pub const SQLITE_DEFAULT_SORTERREF_SIZE: i32 = 0x7fffffff;

// Nota: As opções SQLITE_MMAP_READWRITE e SQLITE_ENABLE_BATCH_ATOMIC_WRITE
// não são compatíveis uma com a outra. Você deve escolher uma ou a outra (ou nenhuma)
// mas não as duas. A configuração do Debian 13 não define ambas.

// O C define offsetof via stddef.h. Em Rust seguro o deslocamento de campo não é necessário
// (as estruturas não são acessadas por aritmética de ponteiro); onde o C usa offsetof, o
// tradutor usa `core::mem::offset_of!` direto no ponto de uso.

// Macros para calcular mínimo e máximo de dois números.
#[inline]
pub fn min<T: PartialOrd>(a: T, b: T) -> T {
    if a < b { a } else { b }
}

#[inline]
pub fn max<T: PartialOrd>(a: T, b: T) -> T {
    if a > b { a } else { b }
}

// Trocar dois objetos do mesmo tipo.
#[inline]
pub fn swap<T>(a: &mut T, b: &mut T) {
    core::mem::swap(a, b);
}

// Verificar se esta máquina usa EBCDIC (o Debian 13 usa ASCII)
pub const SQLITE_EBCDIC: bool = false;
pub const SQLITE_ASCII: bool = true;

// Inteiros de tamanhos conhecidos. Os typedefs i64, u64, u32, u16, i16, u8 e i8 do C coincidem
// com os tipos primitivos de mesmo nome do Rust, então não há apelido a declarar (um
// `pub type i64 = i64;` seria um ciclo de tipos).

// SQLITE_MAX_U32 é uma constante u64 que é o valor máximo de u64 que pode
// ser armazenado em u32 sem perda de dados. O valor é 0x00000000ffffffff.
pub const SQLITE_MAX_U32: u64 = ((1u64 << 32) - 1);

// O tipo de dados usado para armazenar estimativas do número de linhas
// em uma tabela ou índice.
pub type tRowcnt = u64;

// Quantidades estimadas usadas para planejamento de consultas são armazenadas como
// logaritmos de 16 bits. Para a quantidade X, o valor armazenado é 10*log2(X).
// Isso fornece um intervalo possível de valores de aproximadamente 1.0e986 a 1e-986.
// Porém, os valores permitidos são "granulares". Nem todo valor é representável.
// Por exemplo, as quantidades 16 e 17 são ambas representadas por LogEst de 40.
// Como quantidades LogEst devem ser estimativas, não valores exatos, essa imprecisão
// não é um problema.
//
// Exemplos:
//      1 -> 0              20 -> 43          10000 -> 132
//      2 -> 10             25 -> 46          25000 -> 146
//      3 -> 16            100 -> 66        1000000 -> 199
//      4 -> 20           1000 -> 99        1048576 -> 200
//     10 -> 33           1024 -> 100    4294967296 -> 320
//
// LogEst pode ser negativo para indicar valores fracionários.
// Exemplos:
//    0.5 -> -10           0.1 -> -33        0.0625 -> -40
pub type LogEst = i16;

// Tamanho de um ponteiro, usado para detecção de endianness em tempo de compilação.
// No x86-64 (arquitetura padrão do Debian 13), o tamanho é 8 bytes.
pub const SQLITE_PTRSIZE: usize = core::mem::size_of::<*const ()>();

// Tipo inteiro sem sinal grande o suficiente para armazenar um ponteiro.
pub type uptr = usize;

// Macro SQLITE_WITHIN(P,S,E) verifica se o ponteiro P aponta para algo
// entre S (inclusive) e E (exclusivo).
// Em outras palavras, S é um buffer e E é um ponteiro para o primeiro byte
// após o fim do buffer S. Essa macro retorna verdadeiro se P aponta para
// algo contido no buffer S.
#[inline]
pub fn sqlite_within(p: usize, s: usize, e: usize) -> bool {
    (p >= s) && (p < e)
}

// P é um byte após o fim de um buffer grande. Retorne verdadeiro se um intervalo de bytes
// entre S..E cruza o fim desse buffer. Em outras palavras, retorne verdadeiro
// se o sub-buffer S..E-1 transborda do buffer cujo último byte é P-1.
//
// S é o início do intervalo. E é um byte após o fim do intervalo.
//
//                        P
//     |-----------------|                FALSO
//               |-------|
//               S        E
//
//                        P
//     |-----------------|
//                    |-------|           VERDADEIRO
//                    S        E
//
//                        P
//     |-----------------|
//                        |-------|       FALSO
//                        S        E
#[inline]
pub fn sqlite_overflow(p: usize, s: usize, e: usize) -> bool {
    (s < p) && (e > p)
}

// Macros para determinar se a máquina é big-endian ou little-endian.
// A configuração do Debian 13 em x86 é little-endian.
pub const SQLITE_BYTEORDER: i32 = 1234;
pub const SQLITE_BIGENDIAN: bool = false;
pub const SQLITE_LITTLEENDIAN: bool = true;

// UTF-16 nativo é UTF-16LE em máquinas little-endian.
pub const SQLITE_UTF16NATIVE: i32 = SQLITE_UTF16LE;

// Constantes para os maiores e menores inteiros com sinal de 64 bits possíveis.
// Essas macros são projetadas para funcionar corretamente em compiladores de 32 e 64 bits.
pub const LARGEST_INT64: i64 = 0x7fffffffffffffff;
pub const LARGEST_UINT64: u64 = 0xffffffffffffffff;
pub const SMALLEST_INT64: i64 = (-9223372036854775807i64 - 1);

// Arredondar para o próximo múltiplo maior de 8. Isso é usado para forçar
// alinhamento de 8 bytes em arquiteturas de 64 bits.
//
// ROUND8() sempre faz o arredondamento, para qualquer argumento.
//
// ROUND8P() assume que o argumento já é um número inteiro de tamanhos de ponteiro,
// e assim é uma operação nula em sistemas onde o tamanho do ponteiro é 8.
#[inline]
pub fn round8(x: usize) -> usize {
    (x + 7) & !7
}

#[inline]
pub fn round8p(x: usize) -> usize {
    if SQLITE_PTRSIZE == 8 {
        x
    } else {
        (x + 7) & !7
    }
}

// Arredondar para baixo para o múltiplo mais próximo de 8
#[inline]
pub fn rounddown8(x: usize) -> usize {
    x & !7
}

// Verificar que o ponteiro X está alinhado a um limite de 8 bytes. Essa
// macro é usada apenas dentro de verificações para garantir que o código
// obtém todas as restrições de alinhamento corretas.
//
// Exceto, se SQLITE_4_BYTE_ALIGNED_MALLOC for definido, então a implementação
// malloc subjacente pode retornar ponteiros alinhados a 4 bytes.
// Nesse caso, verifique apenas o alinhamento de 4 bytes.
// A configuração do Debian 13 não define SQLITE_4_BYTE_ALIGNED_MALLOC.
#[inline]
pub fn eight_byte_alignment(x: usize) -> bool {
    (x & 7) == 0
}

// Tamanho máximo de memória mapeada em memória no VFS.
// A configuração do Debian 13 em Linux é 2147418112 (0x7fff0000).
pub const SQLITE_MAX_MMAP_SIZE: usize = 0x7fff0000;

// O tamanho MMAP padrão é zero em todos os plataformas, ou mesmo que um
// tamanho MMAP padrão maior seja especificado em tempo de compilação,
// certifique-se de que não ultrapasse o tamanho máximo de mmap.
pub const SQLITE_DEFAULT_MMAP_SIZE: usize = 0;

// TREETRACE_ENABLED será 1 ou 0 dependendo se a lógica de rastreamento da
// árvore de sintaxe abstrata está ligada. A configuração do Debian 13 não
// define SQLITE_DEBUG, SQLITE_TEST ou SQLITE_ENABLE_TREETRACE.
pub const TREETRACE_ENABLED: i32 = 0;

/// Máscara de rastreamento da árvore (sqlite3TreeTrace), sempre zero em compilação normal.
pub static TREE_TRACE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

// Marcadores de TREETRACE (comentário informativo; não usado em compilação normal):
//
//   0x00000001     Início e fim do processamento SELECT
//   0x00000002     Processamento da cláusula WHERE
//   0x00000004     Achatador de consultas
//   0x00000008     Expansão de caractere curinga de conjunto de resultados
//   0x00000010     Resolução de nome de consulta
//   0x00000020     Análise agregada
//   0x00000040     Funções de janela
//   0x00000080     Nomes de colunas geradas
//   0x00000100     Mover termos HAVING para WHERE
//   0x00000200     Otimização de contagem de visualização
//   0x00000400     Processamento SELECT composto
//   0x00000800     Soltar ORDER BY supérflua
//   0x00001000     LEFT JOIN simplifica para JOIN
//   0x00002000     Propagação de constante
//   0x00004000     Otimização de push-down
//   0x00008000     Após toda análise da cláusula FROM
//   0x00010000     Início do processamento DELETE/INSERT/UPDATE
//   0x00020000     Transformar DISTINCT em GROUP BY
//   0x00040000     Despejo da árvore SELECT após todo código ser gerado
//   0x00080000     Redução de força NOT NULL

// Macros para "wheretrace"
pub const WHERETRACE_ENABLED: i32 = 0;

/// Máscara de rastreamento do WHERE (sqlite3WhereTrace), sempre zero em compilação normal.
pub static WHERE_TRACE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

// Bits para a máscara sqlite3WhereTrace (comentário informativo):
//
// (---qualquer--)   Estrutura de bloco de nível superior
// 0x-------F   Mensagens de depuração de alto nível
// 0x----FFF-   Mais detalhes
// 0xFFFF----   Mensagens de depuração de nível baixo
//
// 0x00000001   Geração de código
// 0x00000002   Solucionador
// 0x00000004   Custos do solucionador
// 0x00000008   Inserções de WhereLoop
//
// 0x00000010   Exibir chamadas xBestIndex do sqlite3_index_info
// 0x00000020   Métricas de varredura de intervalo e igualdade
// 0x00000040   Decisões do operador IN
// 0x00000080   Ajustes de custo de WhereLoop
// 0x00000100
// 0x00000200   Decisões de índice de cobertura
// 0x00000400   Otimização OR
// 0x00000800   Scanner de índice
// 0x00001000   Mais detalhes associados à geração de código
// 0x00002000
// 0x00004000   Mostrar todos os termos WHERE em pontos-chave
// 0x00008000   Mostrar a instrução SELECT completa em lugares-chave
//
// 0x00010000   Mostrar mais detalhes ao imprimir termos WHERE
// 0x00020000   Mostrar termos WHERE retornados de whereScanNext()

// Uma instância da seguinte estrutura é usada para armazenar o callback do manipulador
// de ocupado para um determinado identificador sqlite.
//
// O membro busyHandler do sqlite contém o callback de ocupado para o identificador do banco de dados.
// Cada pager aberto através do identificador sqlite é passado um ponteiro para sqlite.busyHandler.
// O callback do manipulador de ocupado é atualmente invocado apenas dentro de pager.c.
#[derive(Clone, Default)]
pub struct BusyHandler {
    /// Callback de ocupado: recebe o argumento opaco e a contagem de tentativas, devolve
    /// não zero para tentar de novo.
    pub x_busy_handler: Option<std::rc::Rc<dyn Fn(Option<&std::rc::Rc<dyn std::any::Any>>, i32) -> i32>>,
    /// Primeiro argumento do callback de ocupado (pBusyArg).
    pub p_busy_arg: Option<std::rc::Rc<dyn std::any::Any>>,
    /// Incrementado a cada chamada de ocupado.
    pub n_busy: i32,
}


// ---- part_003.rs ----

// Nome da tabela que contém o esquema do banco de dados.
//
// Os nomes PREFERRED são usados onde possível. LEGACY também é usado
// para retrocompatibilidade.
//
//  1. Consultas podem usar PREFERRED ou LEGACY
//  2. O callback sqlite3_set_authorizer() usa LEGACY
//  3. O PRAGMA table_list usa PREFERRED
//
// Os nomes LEGACY são armazenados na tabela de símbolos interna
// de suporte a (2). Nomes são traduzidos por sqlite3PreferredTableName()
// para (3). A função sqlite3FindTable() cuida de traduzir nomes para (1).
//
// Note que "sqlite_temp_schema" também pode ser chamado "temp.sqlite_schema".
pub const LEGACY_SCHEMA_TABLE: &[u8] = b"sqlite_master";
pub const LEGACY_TEMP_SCHEMA_TABLE: &[u8] = b"sqlite_temp_master";
pub const PREFERRED_SCHEMA_TABLE: &[u8] = b"sqlite_schema";
pub const PREFERRED_TEMP_SCHEMA_TABLE: &[u8] = b"sqlite_temp_schema";

// Página raiz da tabela de esquema.
pub const SCHEMA_ROOT: u32 = 1;

// Nome da tabela de esquema. O nome é diferente para TEMP.
#[inline]
pub fn schema_table(x: i32) -> &'static [u8] {
    if OMIT_TEMPDB == 0 && x == 1 {
        LEGACY_TEMP_SCHEMA_TABLE
    } else {
        LEGACY_SCHEMA_TABLE
    }
}

// Macro de conveniência que retorna o número de elementos de um array.
#[inline]
pub fn array_size<T>(arr: &[T]) -> i32 {
    arr.len() as i32
}

// Determina se o argumento é uma potência de dois.
#[inline]
pub fn is_power_of_two(x: usize) -> bool {
    (x & (x.wrapping_sub(1))) == 0
}

// O valor a seguir como destrutor significa usar sqlite3DbFree().
// A rotina sqlite3DbFree() requer dois parâmetros em vez do único
// que destrutores normalmente querem. Então precisamos introduzir
// este valor mágico que o código sabe tratar diferentemente. Qualquer
// ponteiro funciona aqui desde que seja distinto de SQLITE_STATIC
// e SQLITE_TRANSIENT.
pub const SQLITE_DYNAMIC: i32 = -1; // Marcador especial para sqlite3OomClear

// Quando SQLITE_OMIT_WSD é definido, significa que a plataforma alvo não
// suporta Writable Static Data (WSD) como variáveis globais e estáticas.
// Todas as variáveis devem estar na pilha ou alocadas dinamicamente no heap.
// Quando WSD não é suportado, as declarações de variáveis espalhadas
// pelo código SQLite devem se tornar constantes. A macro SQLITE_WSD
// é usada para este propósito. E em vez de referenciar a variável
// diretamente, usamos sua constante como chave para buscar o buffer
// alocado em tempo de execução que contém a variável real. A constante
// também é o inicializador para o buffer alocado em tempo de execução.
//
// No caso usual onde WSD é suportado, as macros SQLITE_WSD e GLOBAL
// se tornam no-ops e têm zero impacto de desempenho.
//
// Em zsqlite, WSD é sempre suportado (target padrão).

// Macro para suprimir avisos do compilador e deixar claro para leitores
// humanos quando um parâmetro de função é deliberadamente deixado não
// utilizado no corpo da função. Isto geralmente acontece quando uma
// função é chamada via ponteiro de função. Por exemplo, a implementação
// de um callback de passo de agregação SQL pode não usar o parâmetro
// indicando o número de argumentos passados para a agregação, se souber
// que isto é cumprido em outro lugar.
//
// Quando um parâmetro de função não é usado no corpo da função, geralmente
// é nomeado "not_used" ou "not_used2" para deixar as coisas ainda mais
// claras. Porém, estas macros também podem ser usadas para suprimir avisos
// relacionados a parâmetros que podem ou não ser usados dependendo de
// opções de compilação. Por exemplo, parâmetros usados apenas em assert().
#[inline]
pub fn unused_parameter<T>(_x: T) {}

#[inline]
pub fn unused_parameter2<T, U>(_x: T, _y: U) {}

// Referências para frente de estruturas. Em Rust não há declaração antecipada: AggInfo,
// AuthContext, AutoincInfo, Bitvec, CollSeq, Column, Cte, CteUse, Db, DbClientData, DbFixer,
// Schema, Expr, ExprList, FKey, FpDecode, FuncDestructor, FuncDef, FuncDefHash, IdList, Index,
// IndexedExpr, IndexSample, KeyClass, KeyInfo, Lookaside, LookasideSlot, Module, NameContext,
// OnOrUsing, Parse, ParseCleanup, PreUpdate, PrintfArguments, RCStr, RenameToken, Returning,
// RowSet, Savepoint, Select, SQLiteThread, SelectDest, SrcItem, SrcList, Table, TableLock,
// Token, TreeView, Trigger, TriggerPrg, TriggerStep, UnpackedRecord, Upsert, VTable, VtabCtx,
// Walker, WhereInfo, Window e With são definidas com o corpo completo no módulo que as define
// e reexportadas pelo prelude. Só o apelido interno abaixo precisa existir aqui.
/// Apelido interno de `sqlite3_str` (typedef struct sqlite3_str StrAccum).
pub type StrAccum = Sqlite3Str;

// O tipo de máscara de bits definido a seguir é usado para várias otimizações.
//
// Mudar isto de um tipo 64-bit para 32-bit limita o número de tabelas
// em um join a 32 em vez de 64. Mas também reduz o tamanho da biblioteca
// em 738 bytes no ix86.
pub type Bitmask = u64;

// O número de bits em uma Bitmask. "BMS" significa "BitMask Size".
pub const BMS: i32 = (std::mem::size_of::<Bitmask>() * 8) as i32;

// Um bit em uma Bitmask.
#[inline]
pub fn maskbit(n: u32) -> Bitmask {
    1u64.wrapping_shl(n)
}

#[inline]
pub fn maskbit64(n: u32) -> u64 {
    1u64.wrapping_shl(n)
}

#[inline]
pub fn maskbit32(n: u32) -> u32 {
    1u32.wrapping_shl(n)
}

#[inline]
pub fn smaskbit32(n: u32) -> u32 {
    if n <= 31 {
        1u32.wrapping_shl(n)
    } else {
        0
    }
}

pub const ALLBITS: Bitmask = !0u64;

pub const TOPBIT: Bitmask = 1u64 << ((BMS as u32) - 1);

// Um objeto VList registra um mapeamento entre parâmetros/variáveis/wildcards
// na sentença SQL (como $abc, @pqr, ou :xyz) e o número de variável inteira
// associado àquele parâmetro. Veja a descrição de formato na rotina
// sqlite3VListAdd() para mais informações. Uma VList é na verdade
// apenas um array de inteiros.
pub type VList = i32;

// Cada arquivo de banco de dados a ser acessado pelo sistema é uma instância
// da seguinte estrutura. Normalmente há duas destas estruturas no array
// sqlite3.aDb[]. aDb[0] é o arquivo do banco de dados principal e
// aDb[1] é o arquivo de banco de dados usado para manter tabelas temporárias.
// Bancos de dados adicionais podem ser anexados.
pub struct Db {
    pub z_db_sname: Option<Vec<u8>>,  // Nome deste banco de dados (nome de esquema, não nome de arquivo)
    pub p_bt: Option<BtreeRef>,       // A estrutura B*Tree para este arquivo de banco de dados
    pub safety_level: u8,              // Quão agressivo ao sincronizar dados para disco
    pub b_sync_set: u8,                // Verdadeiro se "PRAGMA synchronous=N" foi executado
    pub p_schema: Option<SchemaRef>,  // Ponteiro para esquema de banco de dados (possivelmente compartilhado)
}

// Uma instância da seguinte estrutura armazena um esquema de banco de dados.
//
// A maioria dos objetos Schema estão associados a uma Btree. A exceção é
// o Schema para o banco de dados TEMP (sqlite3.aDb[1]) que é independente.
// No modo de cache compartilhado, um único objeto Schema pode ser compartilhado
// por múltiplas Btrees que referem o mesmo objeto BtShared subjacente.
//
// Objetos Schema são desalocados automaticamente quando a última Btree que
// os referencia é destruída. O Schema TEMP é liberado manualmente por
// sqlite3_close().
//
// Uma thread deve estar segurando um mutex no Btree correspondente a fim de
// acessar conteúdo de Schema. Isto implica que a thread também deve estar
// segurando um mutex no ponteiro de conexão sqlite3 que possui a Btree.
// Para um Schema TEMP, apenas o mutex de conexão é necessário.
pub struct Schema {
    pub schema_cookie: i32,    // Número da versão do esquema do banco de dados para este arquivo
    pub i_generation: i32,     // Contador de geração. Incrementado a cada mudança
    pub tbl_hash: Hash,        // Todas as tabelas indexadas por nome
    pub idx_hash: Hash,        // Todos os índices (nomeados) indexados por nome
    pub trig_hash: Hash,       // Todos os gatilhos indexados por nome
    pub fkey_hash: Hash,       // Todas as chaves estrangeiras por nome da tabela referenciada
    pub p_seq_tab: Option<TableRef>,  // A tabela sqlite_sequence usada por AUTOINCREMENT
    pub file_format: u8,       // Versão de formato de esquema para este arquivo
    pub enc: u8,               // Codificação de texto usada por este banco de dados
    pub schema_flags: u16,     // Flags associadas a este esquema
    pub cache_size: i32,       // Número de páginas a usar no cache
}

// Estas macros podem ser usadas para testar, definir ou limpar bits
// no campo Db.pSchema->flags.
#[inline]
pub fn db_has_property(db: &Sqlite3, i: usize, p: u16) -> bool {
    (db.a_db[i].p_schema
        .as_ref()
        .map(|s| s.borrow().schema_flags)
        .unwrap_or(0) & p) == p
}

#[inline]
pub fn db_has_any_property(db: &Sqlite3, i: usize, p: u16) -> bool {
    (db.a_db[i].p_schema
        .as_ref()
        .map(|s| s.borrow().schema_flags)
        .unwrap_or(0) & p) != 0
}

#[inline]
pub fn db_set_property(db: &mut Sqlite3, i: usize, p: u16) {
    if let Some(schema) = &db.a_db[i].p_schema {
        schema.borrow_mut().schema_flags |= p;
    }
}

#[inline]
pub fn db_clear_property(db: &mut Sqlite3, i: usize, p: u16) {
    if let Some(schema) = &db.a_db[i].p_schema {
        schema.borrow_mut().schema_flags &= !p;
    }
}

// Valores permitidos para o campo DB.pSchema->flags.
//
// O flag DB_SchemaLoaded é definido após o esquema do banco de dados
// ter sido lido em tabelas de hash internas.
//
// DB_UnresetViews significa que uma ou mais views têm nomes de coluna
// que foram preenchidos. Se o esquema muda, estes nomes de coluna
// podem mudar e assim a view precisará ser resetada.
pub const DB_SCHEMALOADED: u16 = 0x0001;  // O esquema foi carregado
pub const DB_UNRESETVIEWS: u16 = 0x0002;  // Algumas views têm nomes de coluna definidos
pub const DB_RESETWANTED: u16 = 0x0008;   // Resetar o esquema quando nSchemaLock==0

// O número de diferentes tipos de coisas que podem ser limitadas
// usando a interface sqlite3_limit().
pub const SQLITE_N_LIMIT: i32 = (SQLITE_LIMIT_WORKER_THREADS + 1);

// Lookaside malloc é um conjunto de buffers de tamanho fixo que podem ser
// usados para satisfazer pequenas requisições de alocação de memória
// transitória para objetos associados a uma conexão de banco de dados
// particular. O uso de lookaside malloc oferece um melhoramento
// significativo de desempenho (aprox 10%) evitando numerosas requisições
// malloc/free durante o parsing de sentências SQL.
//
// A estrutura Lookaside contém informações de configuração sobre o
// subsistema de lookaside malloc. Cada alocação de memória disponível
// no subsistema lookaside é armazenada em uma lista encadeada de
// objetos LookasideSlot.
//
// Alocações de lookaside só são permitidas para objetos que estão
// associados a uma conexão de banco de dados particular. Portanto,
// informações de esquema não podem ser armazenadas em lookaside porque
// em modo de cache compartilhado as informações de esquema são
// compartilhadas por múltiplas conexões de banco de dados. Portanto,
// enquanto fazendo parsing de informações de esquema, o flag
// Lookaside.bEnabled é apagado de modo que alocações de lookaside
// não sejam usadas para construir os objetos de esquema.
//
// Novas alocações de lookaside só são permitidas se bDisable==0. Quando
// bDisable é maior que zero, sz é definido para zero que efetivamente
// desabilita lookaside sem adicionar um novo teste para o flag bDisable
// em um caminho crítico de desempenho. sz deve ser definido para szTrue
// sempre que bDisable muda de volta para zero.
//
// Buffers de lookaside são inicialmente mantidos na lista pInit. Conforme
// são usados e liberados, são adicionados de volta à lista pFree. Novas
// alocações vêm de pFree primeiro, depois pInit como fallback. Esta
// lista dual permite calcular uma marca de água alta, o número máximo
// de alocações pendentes em qualquer ponto do passado, subtraindo o
// número de alocações na lista pInit do número total de alocações.
//
// Melhoria em 2019-12-12: Two-size-lookaside
// A configuração padrão de lookaside é 100 slots de 1200 bytes cada.
// Os tamanhos maiores de slot são importantes para desempenho, mas desperdiçam
// muito espaço, já que a maioria das alocações de lookaside são menores
// que 128 bytes. A melhoria two-size-lookaside quebra a alocação de
// lookaside em dois pools: Um de slots de 128 bytes e o outro do tamanho
// padrão (1200-byte) de slots. Alocações são preenchidas do pequeno pool
// primeiro, falhando para o pool de tamanho completo se aquele não funcionar.
// Assim mais slots de lookaside estão disponíveis enquanto também usam menos memória.
// Esta melhoria pode ser omitida compilando com SQLITE_OMIT_TWOSIZE_LOOKASIDE.
pub struct Lookaside {
    pub b_disable: u32,              // Operar o lookaside apenas quando zero
    pub sz: u16,                     // Tamanho de cada buffer em bytes
    pub sz_true: u16,                // Valor verdadeiro de sz, mesmo se desabilitado
    pub b_malloced: u8,              // Verdadeiro se pStart foi obtido de sqlite3_malloc()
    pub n_slot: u32,                 // Número de slots de lookaside alocados
    pub an_stat: [u32; 3],           // 0: hits, 1: misses de tamanho, 2: misses completos
    pub p_init: Option<Box<LookasideSlot>>,     // Lista de buffers não previamente usados
    pub p_free: Option<Box<LookasideSlot>>,     // Lista de buffers disponíveis
    pub p_small_init: Option<Box<LookasideSlot>>,  // Lista de pequenos buffers não previamente usados
    pub p_small_free: Option<Box<LookasideSlot>>,  // Lista de pequenos buffers disponíveis
    pub p_middle: usize,             // Primeiro byte após fim de buffers de tamanho completo e
                                      // primeiro byte de buffers LOOKASIDE_SMALL (índice no buffer)
    pub p_start: usize,              // Primeiro byte de espaço de memória disponível (índice)
    pub p_end: usize,                // Primeiro byte após fim de espaço disponível (índice)
    pub p_true_end: usize,           // Valor verdadeiro de pEnd, quando db->pnBytesFreed!=0 (índice)
}


// ---- part_004.rs ----

/// Referência compartilhada para a conexão de banco de dados.
pub type Sqlite3Ref = Rc<RefCell<Sqlite3>>;
/// Referência compartilhada para uma definição de função.
pub type FuncDefRef = Rc<RefCell<FuncDef>>;
/// Referência compartilhada para um destrutor de função de usuário.
pub type FuncDestructorRef = Rc<RefCell<FuncDestructor>>;
/// Argumento opaco de callback (o `void*` do C).
pub type CallbackArg = Option<Rc<dyn Any>>;

/// Slot na lista de buffers livres do lookaside. O lookaside em si é modelado por índices no
/// `Vec<u8>` do `Lookaside`; a lista de livres guarda o índice do próximo slot.
pub struct LookasideSlot {
    /// Próximo buffer na lista de buffers livres (índice no arranjo de slots).
    pub p_next: Option<usize>,
}

/// Desabilita alocação lookaside: incrementa o contador de desabilitação e zera o tamanho.
#[inline]
pub fn disable_lookaside(db: &mut Sqlite3) {
    db.lookaside.b_disable += 1;
    db.lookaside.sz = 0;
}

/// Habilita alocação lookaside: decrementa o contador e restaura o tamanho se nenhum outro
/// desabilitador estiver ativo.
#[inline]
pub fn enable_lookaside(db: &mut Sqlite3) {
    db.lookaside.b_disable -= 1;
    db.lookaside.sz = if db.lookaside.b_disable != 0 { 0 } else { db.lookaside.sz_true };
}

/// Tamanho das alocações menores no lookaside de dois tamanhos.
pub const LOOKASIDE_SMALL: usize = 128;

/// Número de slots da tabela hash de funções embutidas.
pub const SQLITE_FUNC_HASH_SZ: usize = 23;

/// Tabela hash para definições de função embutidas. Cada `FuncDef` cai em um dos slots `a[]`;
/// colisões seguem a cadeia `FuncDef.u.pHash`.
pub struct FuncDefHash {
    /// Tabela hash para funções.
    pub a: [Option<FuncDefRef>; SQLITE_FUNC_HASH_SZ],
}

/// Calcula o slot hash de um nome de função: `c` é o primeiro byte e `l` o comprimento.
#[inline]
pub fn sqlite_func_hash(c: i32, l: i32) -> i32 {
    (c + l) % (SQLITE_FUNC_HASH_SZ as i32)
}

/// Callback de autorização (`sqlite3_xauth`, sem `SQLITE_USER_AUTHENTICATION`).
pub type Sqlite3XAuth =
    Rc<dyn Fn(&CallbackArg, i32, Option<&[u8]>, Option<&[u8]>, Option<&[u8]>, Option<&[u8]>) -> i32>;

/// Indica rastreamento "legado" no estilo de `sqlite3_trace()`.
pub const SQLITE_TRACE_LEGACY: u8 = 0x40;
/// Indica o uso do `xProfile` legado.
pub const SQLITE_TRACE_XPROFILE: u8 = 0x80;
/// Sinalizadores normais de rastreamento.
pub const SQLITE_TRACE_NONLEGACY_MASK: u8 = 0x0f;

/// Número máximo de entradas em `sqlite3.aDb[]`: bancos anexados mais 2 para "main" e "temp".
pub const SQLITE_MAX_DB: i32 = SQLITE_MAX_ATTACHED + 2;

/// Informação usada durante a inicialização da conexão (`struct sqlite3InitInfo`).
#[derive(Default)]
pub struct Sqlite3InitInfo {
    /// Página raiz da tabela sendo inicializada.
    pub new_tnum: Pgno,
    /// Qual arquivo de banco está sendo inicializado.
    pub i_db: u8,
    /// Verdadeiro se a inicialização está em curso.
    pub busy: u8,
    /// Última instrução é um trigger TEMP órfão (campo de 1 bit).
    pub orphan_trigger: u8,
    /// Construindo uma tabela impostora (campo de 1 bit).
    pub imposter_table: u8,
    /// ATTACH é na verdade uma reabertura usando MemDB (campo de 1 bit).
    pub reopen_memdb: u8,
    /// Colunas "type", "name" e "tbl_name".
    pub az_init: Vec<Vec<u8>>,
}

/// Função de rastreamento: o `union trace` do C vira enum, discriminado por `mTrace`.
#[derive(Clone, Default)]
pub enum Sqlite3Trace {
    /// Nenhuma função registrada.
    #[default]
    None,
    /// `xLegacy`, usada quando `mTrace == SQLITE_TRACE_LEGACY`.
    Legacy(Rc<dyn Fn(&CallbackArg, &[u8])>),
    /// `xV2`, usada nos demais valores de `mTrace`.
    V2(Rc<dyn Fn(u32, &CallbackArg, &dyn Any, &dyn Any) -> i32>),
}

/// Cada conexão de banco de dados é uma instância desta estrutura (`struct sqlite3`).
pub struct Sqlite3 {
    /// Interface do SO.
    pub p_vfs: Option<Rc<dyn Vfs>>,
    /// Lista de máquinas virtuais ativas.
    pub p_vdbe: Option<VdbeRef>,
    /// Sequência de comparação BINARY para a codificação do banco.
    pub p_dflt_coll: Option<CollSeqRef>,
    /// Mutex da conexão.
    pub mutex: Option<Rc<Sqlite3Mutex>>,
    /// Todos os backends.
    pub a_db: Vec<Db>,
    /// Número de backends em uso.
    pub n_db: i32,
    /// Sinalizadores que registram estado interno.
    pub m_db_flags: u32,
    /// Sinalizadores definíveis por pragmas.
    pub flags: u64,
    /// ROWID da inserção mais recente.
    pub last_rowid: i64,
    /// Configuração padrão de `mmap_size`.
    pub sz_mmap: i64,
    /// Não reinicia o schema quando diferente de zero.
    pub n_schema_lock: u32,
    /// Sinalizadores passados para `sqlite3_vfs.xOpen()`.
    pub open_flags: u32,
    /// Código de erro mais recente (SQLITE_*).
    pub err_code: i32,
    /// Deslocamento em bytes do erro na instrução SQL.
    pub err_byte_offset: i32,
    /// Máscara aplicada aos códigos de resultado antes de retornar.
    pub err_mask: i32,
    /// Valor de errno do último erro do sistema.
    pub i_sys_errno: i32,
    /// Sinalizadores para habilitar/desabilitar otimizações.
    pub db_opt_flags: u32,
    /// Codificação de texto.
    pub enc: u8,
    /// Sinalizador de auto-commit.
    pub auto_commit: u8,
    /// 1: arquivo, 2: memória, 0: padrão.
    pub temp_store: u8,
    /// Verdadeiro se houve falha de malloc.
    pub malloc_failed: u8,
    /// Não exigir OOMs se verdadeiro.
    pub b_benign_malloc: u8,
    /// Modo de bloqueio padrão para bancos anexados.
    pub dflt_lock_mode: u8,
    /// Configuração de autovacuum após VACUUM se >= 0.
    pub next_autovac: i8,
    /// Não emitir mensagens de erro se verdadeiro.
    pub suppress_err: u8,
    /// Valor a retornar para `s3_vtab_on_conflict()`.
    pub vtab_on_conflict: u8,
    /// Verdadeiro se o savepoint mais externo é um savepoint de transação.
    pub is_transaction_savepoint: u8,
    /// Zero ou mais sinalizadores SQLITE_TRACE.
    pub m_trace: u8,
    /// Verdadeiro se nenhum backend usa cache compartilhado.
    pub no_shared_cache: u8,
    /// Número de opcodes OP_SqlExec pendentes.
    pub n_sql_exec: u8,
    /// Condição atual da conexão.
    pub e_open_state: u8,
    /// Tamanho de página após VACUUM se > 0.
    pub next_pagesize: i32,
    /// Valor retornado por `sqlite3_changes()`.
    pub n_change: i64,
    /// Valor retornado por `sqlite3_total_changes()`.
    pub n_total_change: i64,
    /// Limites.
    pub a_limit: [i32; SQLITE_N_LIMIT as usize],
    /// Tamanho máximo das regiões mapeadas pelo sorter.
    pub n_max_sorter_mmap: i32,
    /// Informação usada durante a inicialização.
    pub init: Sqlite3InitInfo,
    /// Número de VDBEs em execução.
    pub n_vdbe_active: i32,
    /// Número de VDBEs ativos que leem ou escrevem.
    pub n_vdbe_read: i32,
    /// Número de VDBEs ativos que leem e escrevem.
    pub n_vdbe_write: i32,
    /// Número de chamadas aninhadas a `VdbeExec()`.
    pub n_vdbe_exec: i32,
    /// Número de operações `OP_VDestroy` ativas.
    pub n_v_destroy: i32,
    /// Número de extensões carregadas.
    pub n_extension: i32,
    /// Handles de bibliotecas compartilhadas.
    pub a_extension: Vec<Rc<dyn Any>>,
    /// Função de rastreamento (legada ou v2).
    pub trace: Sqlite3Trace,
    /// Argumento para a função de rastreamento.
    pub p_trace_arg: CallbackArg,
    /// Função de profiling.
    pub x_profile: Option<Rc<dyn Fn(&CallbackArg, &[u8], u64)>>,
    /// Argumento para a função de profiling.
    pub p_profile_arg: CallbackArg,
    /// Argumento para `xCommitCallback()`.
    pub p_commit_arg: CallbackArg,
    /// Invocado a cada commit.
    pub x_commit_callback: Option<Rc<dyn Fn(&CallbackArg) -> i32>>,
    /// Argumento para `xRollbackCallback()`.
    pub p_rollback_arg: CallbackArg,
    /// Invocado a cada rollback (o comentário do C diz "commit").
    pub x_rollback_callback: Option<Rc<dyn Fn(&CallbackArg)>>,
    /// Argumento do callback de update.
    pub p_update_arg: CallbackArg,
    /// Callback de update.
    pub x_update_callback: Option<Rc<dyn Fn(&CallbackArg, i32, &[u8], &[u8], i64)>>,
    /// Argumento do cliente para autovac_pages.
    pub p_autovac_pages_arg: CallbackArg,
    /// Destrutor de `pAutovacPagesArg`.
    pub x_autovac_destr: Option<Rc<dyn Fn(&CallbackArg)>>,
    /// Callback `xAutovacPages`.
    pub x_autovac_pages: Option<Rc<dyn Fn(&CallbackArg, &[u8], u32, u32, u32) -> u32>>,
    /// Parse atual.
    pub p_parse: Option<ParseRef>,
    /// Primeiro argumento de `xPreUpdateCallback`.
    pub p_pre_update_arg: CallbackArg,
    /// Callback registrado com `sqlite3_preupdate_hook()`.
    pub x_pre_update_callback:
        Option<Rc<dyn Fn(&CallbackArg, &Sqlite3Ref, i32, &[u8], &[u8], i64, i64)>>,
    /// Contexto do callback de pré-update ativo.
    pub p_pre_update: Option<Rc<RefCell<PreUpdate>>>,
    /// Callback de WAL.
    pub x_wal_callback: Option<Rc<dyn Fn(&CallbackArg, &Sqlite3Ref, &[u8], i32) -> i32>>,
    /// Argumento do callback de WAL.
    pub p_wal_arg: CallbackArg,
    /// Callback de collation necessária (UTF-8).
    pub x_coll_needed: Option<Rc<dyn Fn(&CallbackArg, &Sqlite3Ref, i32, &[u8])>>,
    /// Callback de collation necessária (UTF-16).
    pub x_coll_needed16: Option<Rc<dyn Fn(&CallbackArg, &Sqlite3Ref, i32, &[u8])>>,
    /// Argumento dos callbacks de collation necessária.
    pub p_coll_needed_arg: CallbackArg,
    /// Mensagem de erro mais recente.
    pub p_err: Option<Rc<RefCell<Sqlite3Value>>>,
    /// Verdadeiro se `sqlite3_interrupt` foi chamada (membro `u1.isInterrupted`).
    pub is_interrupted: i32,
    /// Configuração do malloc lookaside.
    pub lookaside: Lookaside,
    /// Função de autorização de acesso.
    pub x_auth: Option<Sqlite3XAuth>,
    /// Primeiro argumento da função de autorização.
    pub p_auth_arg: CallbackArg,
    /// Callback de progresso.
    pub x_progress: Option<Rc<dyn Fn(&CallbackArg) -> i32>>,
    /// Argumento do callback de progresso.
    pub p_progress_arg: CallbackArg,
    /// Número de opcodes entre chamadas do callback de progresso.
    pub n_progress_ops: u32,
    /// Tamanho alocado de `aVTrans`.
    pub n_v_trans: i32,
    /// Preenchido por `sqlite3_create_module()`.
    pub a_module: Hash,
    /// Contexto da conexão/criação de vtab em curso.
    pub p_vtab_ctx: Option<Rc<RefCell<VtabCtx>>>,
    /// Tabelas virtuais com transações abertas.
    pub a_v_trans: Vec<VTableRef>,
    /// Desconectar estas na próxima chamada a `sqlite3_prepare()`.
    pub p_disconnect: Option<VTableRef>,
    /// Tabela hash das funções da conexão.
    pub a_func: Hash,
    /// Todas as sequências de comparação.
    pub a_coll_seq: Hash,
    /// Callback de busy.
    pub busy_handler: BusyHandler,
    /// Espaço estático para os 2 backends padrão.
    pub a_db_static: [Db; 2],
    /// Lista de savepoints ativos.
    pub p_savepoint: Option<SavepointRef>,
    /// Número de linhas de índice a analisar.
    pub n_analysis_limit: i32,
    /// Timeout do busy handler, em ms.
    pub busy_timeout: i32,
    /// Número de savepoints que não são de transação.
    pub n_savepoint: i32,
    /// Número de transações de instrução aninhadas.
    pub n_statement: i32,
    /// Restrições adiadas líquidas nesta transação.
    pub n_deferred_cons: i64,
    /// Restrições imediatas adiadas líquidas.
    pub n_deferred_imm_cons: i64,
    /// Se presente, incrementar isto em `DbFree()`.
    pub pn_bytes_freed: Option<Rc<Cell<i32>>>,
    /// Conteúdo de `sqlite3_set_clientdata()`.
    pub p_db_data: Option<Box<DbClientData>>,
    /// Conexão que causou SQLITE_LOCKED (protegida pelo mutex STATIC_MAIN).
    pub p_blocking_connection: Option<Weak<RefCell<Sqlite3>>>,
    /// Conexão a observar até desbloquear.
    pub p_unlock_connection: Option<Weak<RefCell<Sqlite3>>>,
    /// Argumento de `xUnlockNotify`.
    pub p_unlock_arg: CallbackArg,
    /// Callback de notificação de desbloqueio.
    pub x_unlock_notify: Option<Rc<dyn Fn(&[CallbackArg])>>,
    /// Próxima na lista de todas as conexões bloqueadas.
    pub p_next_blocked: Option<Weak<RefCell<Sqlite3>>>,
}

/// Codificação do schema principal do banco: `db->aDb[0].pSchema->enc`.
#[inline]
pub fn schema_enc(db: &Sqlite3) -> u8 {
    db.a_db[0].p_schema.as_ref().expect("schema principal").borrow().enc
}

/// Codificação do banco de dados da conexão.
#[inline]
pub fn enc(db: &Sqlite3) -> u8 {
    db.enc
}

/// Constante `u64` cujos 32 bits inferiores são zero; só os 32 superiores vão no argumento.
#[inline]
pub const fn hi(x: u64) -> u64 {
    x << 32
}

// Valores possíveis para `sqlite3.flags`.
/// OK para atualizar SQLITE_SCHEMA.
pub const SQLITE_WRITE_SCHEMA: u64 = 0x00000001;
/// Criar novos bancos no formato 1.
pub const SQLITE_LEGACY_FILE_FMT: u64 = 0x00000002;
/// Mostrar nomes completos de coluna no SELECT.
pub const SQLITE_FULL_COL_NAMES: u64 = 0x00000004;
/// Usar fsync completo no backend.
pub const SQLITE_FULL_FSYNC: u64 = 0x00000008;
/// Usar fsync completo no checkpoint.
pub const SQLITE_CKPT_FULL_FSYNC: u64 = 0x00000010;
/// OK para despejar o cache do pager.
pub const SQLITE_CACHE_SPILL: u64 = 0x00000020;
/// Mostrar nomes curtos de coluna.
pub const SQLITE_SHORT_COL_NAMES: u64 = 0x00000040;
/// Permitir funções inseguras e vtabs na definição do schema.
pub const SQLITE_TRUSTED_SCHEMA: u64 = 0x00000080;
/// Invocar o callback uma vez se o resultado for vazio.
pub const SQLITE_NULL_CALLBACK: u64 = 0x00000100;
/// Não impor restrições CHECK.
pub const SQLITE_IGNORE_CHECKS: u64 = 0x00000200;
/// Habilitar contadores de `stmt_scanstats()`.
pub const SQLITE_STMT_SCAN_STATUS: u64 = 0x00000400;
/// Sem checkpoint em close()/DETACH.
pub const SQLITE_NO_CKPT_ON_CLOSE: u64 = 0x00000800;
/// Inverter SELECTs sem ordem.
pub const SQLITE_REVERSE_ORDER: u64 = 0x00001000;
/// Habilitar triggers recursivos.
pub const SQLITE_REC_TRIGGERS: u64 = 0x00002000;
/// Impor chaves estrangeiras.
pub const SQLITE_FOREIGN_KEYS: u64 = 0x00004000;
/// Habilitar índices automáticos.
pub const SQLITE_AUTO_INDEX: u64 = 0x00008000;
/// Habilitar load_extension.
pub const SQLITE_LOAD_EXTENSION: u64 = 0x00010000;
/// Habilitar a função SQL load_extension().
pub const SQLITE_LOAD_EXT_FUNC: u64 = 0x00020000;
/// Verdadeiro para habilitar triggers.
pub const SQLITE_ENABLE_TRIGGER: u64 = 0x00040000;
/// Adiar todas as restrições de chave estrangeira.
pub const SQLITE_DEFER_FKS: u64 = 0x00080000;
/// Desabilitar mudanças no banco.
pub const SQLITE_QUERY_ONLY: u64 = 0x00100000;
/// Verificar tamanhos de célula da btree ao carregar.
pub const SQLITE_CELL_SIZE_CK: u64 = 0x00200000;
/// Habilitar fts3_tokenizer(2).
pub const SQLITE_FTS3_TOKENIZER: u64 = 0x00400000;
/// Query Planner Stability Guarantee.
pub const SQLITE_ENABLE_QPSG: u64 = 0x00800000;
/// Mostrar EXPLAIN QUERY PLAN de trigger.
pub const SQLITE_TRIGGER_EQP: u64 = 0x01000000;
/// Reiniciar o banco.
pub const SQLITE_RESET_DATABASE: u64 = 0x02000000;
/// Comportamento legado de ALTER TABLE.
pub const SQLITE_LEGACY_ALTER: u64 = 0x04000000;
/// Não relatar erros de parse do schema.
pub const SQLITE_NO_SCHEMA_ERROR: u64 = 0x08000000;
/// O SQL de entrada é provavelmente hostil.
pub const SQLITE_DEFENSIVE: u64 = 0x10000000;
/// Strings entre aspas duplas permitidas em DDL.
pub const SQLITE_DQS_DDL: u64 = 0x20000000;
/// Strings entre aspas duplas permitidas em DML.
pub const SQLITE_DQS_DML: u64 = 0x40000000;
/// Habilitar o uso de views.
pub const SQLITE_ENABLE_VIEW: u64 = 0x80000000;
/// Contar linhas alteradas por INSERT, DELETE ou UPDATE e devolver a contagem por callback.
pub const SQLITE_COUNT_ROWS: u64 = hi(0x00001);
/// Proibir escritas devido a erro.
pub const SQLITE_CORRUPT_RD_ONLY: u64 = hi(0x00002);
/// READ UNCOMMITTED em cache compartilhado.
pub const SQLITE_READ_UNCOMMIT: u64 = hi(0x00004);
/// Tratar todas as FK como NO ACTION.
pub const SQLITE_FK_NO_ACTION: u64 = hi(0x00008);

// Valores permitidos para `sqlite3.mDbFlags`.
/// Alterações não confirmadas nas tabelas hash.
pub const DBFLAG_SCHEMA_CHANGE: u32 = 0x0001;
/// Preferência por funções embutidas.
pub const DBFLAG_PREFER_BUILTIN: u32 = 0x0002;
/// Dentro de um VACUUM.
pub const DBFLAG_VACUUM: u32 = 0x0004;
/// Executando VACUUM INTO.
pub const DBFLAG_VACUUM_INTO: u32 = 0x0008;
/// Schema sabidamente válido.
pub const DBFLAG_SCHEMA_KNOWN_OK: u32 = 0x0010;
/// Permitir o uso de funções internas.
pub const DBFLAG_INTERNAL_FUNC: u32 = 0x0020;
/// Não é mais possível mudar a codificação.
pub const DBFLAG_ENCODING_FIXED: u32 = 0x0040;

// Bits de `sqlite3.dbOptFlags` usados por `sqlite3_test_control(SQLITE_TESTCTRL_OPTIMIZATIONS)`.
/// Achatamento de consultas.
pub const SQLITE_QUERY_FLATTENER: u32 = 0x00000001;
/// Usar xInverse para funções de janela.
pub const SQLITE_WINDOW_FUNC: u32 = 0x00000002;
/// Cobertura de ORDER BY por GROUP BY.
pub const SQLITE_GROUP_BY_ORDER: u32 = 0x00000004;
/// Fatoração de constantes.
pub const SQLITE_FACTOR_OUT_CONST: u32 = 0x00000008;
/// DISTINCT usando índices.
pub const SQLITE_DISTINCT_OPT: u32 = 0x00000010;
/// Varreduras de índice cobridor.
pub const SQLITE_COVER_IDX_SCAN: u32 = 0x00000020;
/// ORDER BY de joins via índice.
pub const SQLITE_ORDER_BY_IDX_JOIN: u32 = 0x00000040;
/// Restrições transitivas.
pub const SQLITE_TRANSITIVE: u32 = 0x00000080;
/// Omitir tabelas não usadas em joins.
pub const SQLITE_OMIT_NOOP_JOIN: u32 = 0x00000100;
/// Otimização count-of-view.
pub const SQLITE_COUNT_OF_VIEW: u32 = 0x00000200;
/// Adicionar opcodes OP_CursorHint.
pub const SQLITE_CURSOR_HINTS: u32 = 0x00000400;
/// Usar dados STAT4 (o TH3 espera 0x0000800, não mudar).
pub const SQLITE_STAT4: u32 = 0x00000800;
/// Otimização de push-down da cláusula WHERE.
pub const SQLITE_PUSH_DOWN: u32 = 0x00001000;
/// Converter LEFT JOIN em JOIN.
pub const SQLITE_SIMPLIFY_JOIN: u32 = 0x00002000;
/// Skip-scans.
pub const SQLITE_SKIP_SCAN: u32 = 0x00004000;
/// Otimização de propagação de constantes.
pub const SQLITE_PROPAGATE_CONST: u32 = 0x00008000;
/// Otimização de min/max.
pub const SQLITE_MIN_MAX_OPT: u32 = 0x00010000;
/// Otimização OP_SeekScan.
pub const SQLITE_SEEK_SCAN: u32 = 0x00020000;
/// Omitir ORDER BY inútil (o TH3 espera 0x40000).
pub const SQLITE_OMIT_ORDER_BY: u32 = 0x00040000;
/// Usar filtro Bloom nas buscas.
pub const SQLITE_BLOOM_FILTER: u32 = 0x00080000;
/// Executar filtros Bloom cedo.
pub const SQLITE_BLOOM_PULLDOWN: u32 = 0x00100000;
/// Balancear merges de várias vias.
pub const SQLITE_BALANCED_MERGE: u32 = 0x00200000;
/// Usar OP_ReleaseReg para testes.
pub const SQLITE_RELEASE_REG: u32 = 0x00400000;
/// Desabilitar o achatador de UNION ALL (ver flatten04.test).
pub const SQLITE_FLTTN_UNION_ALL: u32 = 0x00800000;
/// Extrair expressões do índice quando possível.
pub const SQLITE_INDEXED_EXPR: u32 = 0x01000000;
/// Co-rotinas para subconsultas.
pub const SQLITE_COROUTINES: u32 = 0x02000000;
/// NULL nas colunas não usadas de subconsultas.
pub const SQLITE_NULL_UNUSED_COLS: u32 = 0x04000000;
/// DELETE e UPDATE de passada única.
pub const SQLITE_ONE_PASS: u32 = 0x08000000;
/// Todas as otimizações.
pub const SQLITE_ALL_OPTS: u32 = 0xffffffff;

/// Verdadeiro se alguma otimização da máscara está desabilitada.
#[inline]
pub fn optimization_disabled(db: &Sqlite3, mask: u32) -> bool {
    (db.db_opt_flags & mask) != 0
}

/// Verdadeiro se todas as otimizações da máscara estão habilitadas.
#[inline]
pub fn optimization_enabled(db: &Sqlite3, mask: u32) -> bool {
    (db.db_opt_flags & mask) == 0
}

/// Verdadeiro se é permitido fatorar expressões constantes no código de inicialização.
#[inline]
pub fn const_factor_ok(p: &Parse) -> bool {
    p.ok_const_factor != 0
}

// Valores possíveis de `sqlite3.eOpenState`.
/// Banco aberto.
pub const SQLITE_STATE_OPEN: u8 = 0x76;
/// Banco fechado.
pub const SQLITE_STATE_CLOSED: u8 = 0xce;
/// Erro, aguardando fechamento.
pub const SQLITE_STATE_SICK: u8 = 0xba;
/// Banco em uso.
pub const SQLITE_STATE_BUSY: u8 = 0x6d;
/// Ocorreu um erro SQLITE_MISUSE.
pub const SQLITE_STATE_ERROR: u8 = 0xd5;
/// Fechar junto com a última instrução.
pub const SQLITE_STATE_ZOMBIE: u8 = 0xa7;

/// Union `u` de `FuncDef`: `pHash` se SQLITE_FUNC_BUILTIN, `pDestructor` caso contrário.
#[derive(Clone)]
pub enum FuncDefU {
    /// Próxima com nome diferente mas mesmo hash.
    PHash(Option<FuncDefRef>),
    /// Destrutor com contagem de referências.
    PDestructor(Option<FuncDestructorRef>),
}

/// Cada função SQL é definida por uma instância desta estrutura.
pub struct FuncDef {
    /// Número de argumentos; -1 significa ilimitado.
    pub n_arg: i8,
    /// Combinação de SQLITE_FUNC_*.
    pub func_flags: u32,
    /// Parâmetro de dados do usuário.
    pub p_user_data: CallbackArg,
    /// Próxima função com o mesmo nome.
    pub p_next: Option<FuncDefRef>,
    /// Função escalar ou passo de agregação.
    pub x_s_func: Option<Rc<dyn Fn(&mut Sqlite3Context, &[Sqlite3ValueRef])>>,
    /// Finalizador de agregação.
    pub x_finalize: Option<Rc<dyn Fn(&mut Sqlite3Context)>>,
    /// Valor corrente da agregação.
    pub x_value: Option<Rc<dyn Fn(&mut Sqlite3Context)>>,
    /// Passo inverso da agregação.
    pub x_inverse: Option<Rc<dyn Fn(&mut Sqlite3Context, &[Sqlite3ValueRef])>>,
    /// Nome SQL da função.
    pub z_name: Vec<u8>,
    /// Union `u`.
    pub u: FuncDefU,
}


// ---- part_005.rs ----

/// Referência compartilhada para um savepoint.
pub type SavepointRef = Rc<RefCell<Savepoint>>;
/// Referência compartilhada para um módulo de tabela virtual.
pub type ModuleRef = Rc<RefCell<Module>>;
/// Referência compartilhada para uma sequência de comparação.
pub type CollSeqRef = Rc<RefCell<CollSeq>>;
/// Referência compartilhada para uma instância de tabela virtual de uma conexão.
pub type VTableRef = Rc<RefCell<VTable>>;
/// Referência compartilhada para um valor SQL (`sqlite3_value*`).
pub type Sqlite3ValueRef = Rc<RefCell<Sqlite3Value>>;

/// Implementação de função escalar ou passo de agregação (`xSFunc`, `xStep`, `xInverse`).
pub type XSFunc = Rc<dyn Fn(&mut Sqlite3Context, &[Sqlite3ValueRef])>;
/// Implementação de finalizador ou de valor corrente de agregação (`xFinalize`, `xValue`).
pub type XFinalFunc = Rc<dyn Fn(&mut Sqlite3Context)>;

/// Destrutor de função de usuário (configurado por `create_function_v2()`) com contador de
/// referências. Quando `create_function_v2()` cria uma função com destrutor, um único objeto
/// deste tipo é alocado e `n_ref` recebe o número de `FuncDef` criados (1 ou 3, conforme a
/// codificação seja ou não SQLITE_ANY). O membro `pDestructor` de cada `FuncDef` aponta para ele.
/// Quando um `FuncDef` é apagado o contador é decrementado; ao chegar a 0 o destrutor é invocado
/// e a estrutura liberada.
pub struct FuncDestructor {
    /// Contador de referências.
    pub n_ref: i32,
    /// Função destrutora.
    pub x_destroy: Option<Rc<dyn Fn(&CallbackArg)>>,
    /// Dados do usuário passados ao destrutor.
    pub p_user_data: CallbackArg,
}

// Valores possíveis de `FuncDef.flags`. Os valores _LENGTH e _TYPEOF devem corresponder a
// OPFLAG_LENGTHARG e OPFLAG_TYPEOFARG, e SQLITE_FUNC_CONSTANT deve ser igual a
// SQLITE_DETERMINISTIC. SQLITE_FUNC_UNSAFE e SQLITE_INNOCUOUS têm o mesmo valor com significados
// invertidos (ver as ocorrências de tag-20230109-1).
//
// Restrições de valor (verificadas por assert() no C):
//     SQLITE_FUNC_MINMAX    == NC_MinMaxAgg == SF_MinMaxAgg
//     SQLITE_FUNC_ANYORDER  == NC_OrderAgg  == SF_OrderByReqd
//     SQLITE_FUNC_LENGTH    == OPFLAG_LENGTHARG
//     SQLITE_FUNC_TYPEOF    == OPFLAG_TYPEOFARG
//     SQLITE_FUNC_BYTELEN   == OPFLAG_BYTELENARG
//     SQLITE_FUNC_CONSTANT  == SQLITE_DETERMINISTIC da API
//     SQLITE_FUNC_DIRECT    == SQLITE_DIRECTONLY da API
//     SQLITE_FUNC_UNSAFE    == SQLITE_INNOCUOUS (significados opostos)
//     SQLITE_FUNC_ENCMASK   depende das macros SQLITE_UTF* da API
/// SQLITE_UTF8, SQLITE_UTF16BE ou UTF16LE.
pub const SQLITE_FUNC_ENCMASK: u32 = 0x0003;
/// Candidata à otimização do LIKE.
pub const SQLITE_FUNC_LIKE: u32 = 0x0004;
/// Função tipo LIKE sensível a maiúsculas.
pub const SQLITE_FUNC_CASE: u32 = 0x0008;
/// Efêmera: apagar junto com o VDBE.
pub const SQLITE_FUNC_EPHEM: u32 = 0x0010;
/// `sqlite3GetFuncCollSeq()` pode ser chamada.
pub const SQLITE_FUNC_NEEDCOLL: u32 = 0x0020;
/// Função embutida length().
pub const SQLITE_FUNC_LENGTH: u32 = 0x0040;
/// Função embutida typeof().
pub const SQLITE_FUNC_TYPEOF: u32 = 0x0080;
/// Função embutida octet_length().
pub const SQLITE_FUNC_BYTELEN: u32 = 0x00c0;
/// Agregação embutida count(*).
pub const SQLITE_FUNC_COUNT: u32 = 0x0100;
// 0x0200 está disponível para reuso.
/// Função embutida unlikely().
pub const SQLITE_FUNC_UNLIKELY: u32 = 0x0400;
/// Entradas constantes dão saída constante.
pub const SQLITE_FUNC_CONSTANT: u32 = 0x0800;
/// Verdadeiro para as agregações min() e max().
pub const SQLITE_FUNC_MINMAX: u32 = 0x1000;
/// "Slow Change": constante durante uma consulta, pode mudar com o tempo.
pub const SQLITE_FUNC_SLOCHNG: u32 = 0x2000;
/// Funções embutidas de teste.
pub const SQLITE_FUNC_TEST: u32 = 0x4000;
/// Não pode ser usada por valueFromFunction.
pub const SQLITE_FUNC_RUNONLY: u32 = 0x8000;
/// Função embutida apenas de janela.
pub const SQLITE_FUNC_WINDOW: u32 = 0x00010000;
/// Para uso exclusivo de NestedParse().
pub const SQLITE_FUNC_INTERNAL: u32 = 0x00040000;
/// Proibida em TRIGGERs e VIEWs.
pub const SQLITE_FUNC_DIRECT: u32 = 0x00080000;
// SQLITE_SUBTYPE 0x00100000: consumidor de subtipos.
/// A função tem efeitos colaterais.
pub const SQLITE_FUNC_UNSAFE: u32 = 0x00200000;
/// Funções implementadas em linha.
pub const SQLITE_FUNC_INLINE: u32 = 0x00400000;
/// Função embutida.
pub const SQLITE_FUNC_BUILTIN: u32 = 0x00800000;
// SQLITE_RESULT_SUBTYPE 0x01000000: gerador de subtipos.
/// Agregação count/min/max.
pub const SQLITE_FUNC_ANYORDER: u32 = 0x08000000;

// Números de identificação de cada função em linha.
pub const INLINEFUNC_COALESCE: i32 = 0;
pub const INLINEFUNC_IMPLIES_NONNULL_ROW: i32 = 1;
pub const INLINEFUNC_EXPR_IMPLIES_EXPR: i32 = 2;
pub const INLINEFUNC_EXPR_COMPARE: i32 = 3;
pub const INLINEFUNC_AFFINITY: i32 = 4;
pub const INLINEFUNC_IIF: i32 = 5;
pub const INLINEFUNC_SQLITE_OFFSET: i32 = 6;
/// Caso padrão.
pub const INLINEFUNC_UNLIKELY: i32 = 99;

// As macros FUNCTION(), LIKEFUNC(), AGGREGATE() e afins criam os inicializadores das estruturas
// `FuncDef`. O `#zName` vira o argumento `z_name`; `SQLITE_INT_TO_PTR(iArg)` vira um
// `Rc<isize>` em `p_user_data`.

/// Monta um `FuncDef` embutido (ponto comum das macros de inicialização).
#[allow(clippy::too_many_arguments)]
fn func_def_init(
    z_name: &str,
    n_arg: i8,
    func_flags: u32,
    p_user_data: CallbackArg,
    x_s_func: Option<XSFunc>,
    x_finalize: Option<XFinalFunc>,
    x_value: Option<XFinalFunc>,
    x_inverse: Option<XSFunc>,
) -> FuncDef {
    FuncDef {
        n_arg,
        func_flags,
        p_user_data,
        p_next: None,
        x_s_func,
        x_finalize,
        x_value,
        x_inverse,
        z_name: z_name.as_bytes().to_vec(),
        u: FuncDefU::PHash(None),
    }
}

/// `SQLITE_INT_TO_PTR(i)`: inteiro guardado como dado opaco.
#[inline]
fn int_to_ptr(i: isize) -> CallbackArg {
    Some(Rc::new(i))
}

/// Função escalar `zName` implementada por `x_func` com `n_arg` argumentos. `i_arg` vira o
/// user-data. Se `b_nc` for verdadeiro, liga SQLITE_FUNC_NEEDCOLL.
pub fn function(z_name: &str, n_arg: i8, i_arg: isize, b_nc: u32, x_func: XSFunc) -> FuncDef {
    func_def_init(
        z_name,
        n_arg,
        SQLITE_FUNC_BUILTIN | SQLITE_FUNC_CONSTANT | SQLITE_UTF8 | (b_nc * SQLITE_FUNC_NEEDCOLL),
        int_to_ptr(i_arg),
        Some(x_func),
        None,
        None,
        None,
    )
}

/// Como `function`, mas sem o sinalizador SQLITE_FUNC_CONSTANT.
pub fn vfunction(z_name: &str, n_arg: i8, i_arg: isize, b_nc: u32, x_func: XSFunc) -> FuncDef {
    func_def_init(
        z_name,
        n_arg,
        SQLITE_FUNC_BUILTIN | SQLITE_UTF8 | (b_nc * SQLITE_FUNC_NEEDCOLL),
        int_to_ptr(i_arg),
        Some(x_func),
        None,
        None,
        None,
    )
}

/// Como `function`, sem SQLITE_FUNC_CONSTANT e com SQLITE_DIRECTONLY e SQLITE_FUNC_UNSAFE.
pub fn sfunction(z_name: &str, n_arg: i8, i_arg: isize, _b_nc: u32, x_func: XSFunc) -> FuncDef {
    func_def_init(
        z_name,
        n_arg,
        SQLITE_FUNC_BUILTIN | SQLITE_UTF8 | SQLITE_DIRECTONLY | SQLITE_FUNC_UNSAFE,
        int_to_ptr(i_arg),
        Some(x_func),
        None,
        None,
        None,
    )
}

/// Funções da biblioteca matemática; `x_ptr` é um dado opaco arbitrário.
pub fn mfunction(z_name: &str, n_arg: i8, x_ptr: CallbackArg, x_func: XSFunc) -> FuncDef {
    func_def_init(
        z_name,
        n_arg,
        SQLITE_FUNC_BUILTIN | SQLITE_FUNC_CONSTANT | SQLITE_UTF8,
        x_ptr,
        Some(x_func),
        None,
        None,
        None,
    )
}

/// Funções JSON: `b_use_cache`, `b_ws`, `b_rs` e `b_json_b` são 0 ou 1.
#[allow(clippy::too_many_arguments)]
pub fn jfunction(
    z_name: &str,
    n_arg: i8,
    b_use_cache: u32,
    b_ws: u32,
    b_rs: u32,
    b_json_b: u32,
    i_arg: isize,
    x_func: XSFunc,
) -> FuncDef {
    func_def_init(
        z_name,
        n_arg,
        SQLITE_FUNC_BUILTIN
            | SQLITE_DETERMINISTIC
            | SQLITE_FUNC_CONSTANT
            | SQLITE_UTF8
            | (b_use_cache * SQLITE_FUNC_RUNONLY)
            | (b_rs * SQLITE_SUBTYPE)
            | (b_ws * SQLITE_RESULT_SUBTYPE),
        int_to_ptr(i_arg | ((b_json_b * JSON_BLOB) as isize)),
        Some(x_func),
        None,
        None,
        None,
    )
}

/// Função implementada por bytecode em linha, com `i_arg` como id da função.
pub fn inline_func(z_name: &str, n_arg: i8, i_arg: isize, m_flags: u32) -> FuncDef {
    func_def_init(
        z_name,
        n_arg,
        SQLITE_FUNC_BUILTIN | SQLITE_UTF8 | SQLITE_FUNC_INLINE | SQLITE_FUNC_CONSTANT | m_flags,
        int_to_ptr(i_arg),
        Some(Rc::new(noop_func)),
        None,
        None,
        None,
    )
}

/// Função de teste implementada por bytecode em linha.
pub fn test_func(z_name: &str, n_arg: i8, i_arg: isize, m_flags: u32) -> FuncDef {
    func_def_init(
        z_name,
        n_arg,
        SQLITE_FUNC_BUILTIN
            | SQLITE_UTF8
            | SQLITE_FUNC_INTERNAL
            | SQLITE_FUNC_TEST
            | SQLITE_FUNC_INLINE
            | SQLITE_FUNC_CONSTANT
            | m_flags,
        int_to_ptr(i_arg),
        Some(Rc::new(noop_func)),
        None,
        None,
        None,
    )
}

/// Funções de data e hora que podem mudar, mas não durante uma consulta. `i_arg` e `b_nc` são
/// ignorados e o user-data é sempre nulo.
pub fn dfunction(z_name: &str, n_arg: i8, _i_arg: isize, _b_nc: u32, x_func: XSFunc) -> FuncDef {
    func_def_init(
        z_name,
        n_arg,
        SQLITE_FUNC_BUILTIN | SQLITE_FUNC_SLOCHNG | SQLITE_UTF8,
        None,
        Some(x_func),
        None,
        None,
        None,
    )
}

/// Funções de data e hora "puras": como `dfunction`, mas liga SQLITE_FUNC_CONSTANT. O user-data
/// é um dado opaco não nulo qualquer (no C, o endereço de `sqlite3Config`).
pub fn pure_date(z_name: &str, n_arg: i8, _i_arg: isize, _b_nc: u32, x_func: XSFunc) -> FuncDef {
    func_def_init(
        z_name,
        n_arg,
        SQLITE_FUNC_BUILTIN | SQLITE_FUNC_SLOCHNG | SQLITE_UTF8 | SQLITE_FUNC_CONSTANT,
        Some(Rc::new(())),
        Some(x_func),
        None,
        None,
        None,
    )
}

/// Como `function`, com `extra_flags` somados.
pub fn function2(
    z_name: &str,
    n_arg: i8,
    i_arg: isize,
    b_nc: u32,
    x_func: XSFunc,
    extra_flags: u32,
) -> FuncDef {
    func_def_init(
        z_name,
        n_arg,
        SQLITE_FUNC_BUILTIN
            | SQLITE_FUNC_CONSTANT
            | SQLITE_UTF8
            | (b_nc * SQLITE_FUNC_NEEDCOLL)
            | extra_flags,
        int_to_ptr(i_arg),
        Some(x_func),
        None,
        None,
        None,
    )
}

/// Função de string: `p_arg` é o user-data (o C não inicializa o último membro `u`).
pub fn str_function(z_name: &str, n_arg: i8, p_arg: CallbackArg, b_nc: u32, x_func: XSFunc) -> FuncDef {
    func_def_init(
        z_name,
        n_arg,
        SQLITE_FUNC_BUILTIN | SQLITE_FUNC_SLOCHNG | SQLITE_UTF8 | (b_nc * SQLITE_FUNC_NEEDCOLL),
        p_arg,
        Some(x_func),
        None,
        None,
        None,
    )
}

/// Função escalar `zName` implementada por `like_func`; `arg` é o user-data e `flags` os
/// sinalizadores de `FuncDef.flags`.
pub fn likefunc(z_name: &str, n_arg: i8, arg: CallbackArg, flags: u32) -> FuncDef {
    func_def_init(
        z_name,
        n_arg,
        SQLITE_FUNC_BUILTIN | SQLITE_FUNC_CONSTANT | SQLITE_UTF8 | flags,
        arg,
        Some(Rc::new(like_func)),
        None,
        None,
        None,
    )
}

/// Agregação (de janela) implementada por `x_step`, `x_final`, `x_value` e `x_inverse`.
#[allow(clippy::too_many_arguments)]
pub fn waggregate(
    z_name: &str,
    n_arg: i8,
    arg: isize,
    nc: u32,
    x_step: XSFunc,
    x_final: XFinalFunc,
    x_value: Option<XFinalFunc>,
    x_inverse: Option<XSFunc>,
    f: u32,
) -> FuncDef {
    func_def_init(
        z_name,
        n_arg,
        SQLITE_FUNC_BUILTIN | SQLITE_UTF8 | (nc * SQLITE_FUNC_NEEDCOLL) | f,
        int_to_ptr(arg),
        Some(x_step),
        Some(x_final),
        x_value,
        x_inverse,
    )
}

/// Função interna, para uso do próprio SQLite.
pub fn internal_function(z_name: &str, n_arg: i8, x_func: XSFunc) -> FuncDef {
    func_def_init(
        z_name,
        n_arg,
        SQLITE_FUNC_BUILTIN | SQLITE_FUNC_INTERNAL | SQLITE_UTF8 | SQLITE_FUNC_CONSTANT,
        None,
        Some(x_func),
        None,
        None,
        None,
    )
}

/// Todos os savepoints atuais ficam numa lista encadeada que começa em `sqlite3.pSavepoint`. O
/// primeiro elemento é o savepoint aberto mais recentemente; a instrução VDBE OP_Savepoint os
/// adiciona à lista.
pub struct Savepoint {
    /// Nome do savepoint.
    pub z_name: Vec<u8>,
    /// Número de violações de FK adiadas.
    pub n_deferred_cons: i64,
    /// Número de violações de FK imediatas adiadas.
    pub n_deferred_imm_cons: i64,
    /// Savepoint pai, se houver.
    pub p_next: Option<SavepointRef>,
}

// Segundo parâmetro de `sqlite3Savepoint()` e argumento P1 de OP_Savepoint.
pub const SAVEPOINT_BEGIN: i32 = 0;
pub const SAVEPOINT_RELEASE: i32 = 1;
pub const SAVEPOINT_ROLLBACK: i32 = 2;

/// Cada módulo SQLite (definição de tabela virtual) é uma instância desta estrutura, guardada na
/// tabela hash `sqlite3.aModule`.
pub struct Module {
    /// Ponteiros de callback.
    pub p_module: Rc<Sqlite3Module>,
    /// Nome passado a `create_module()`.
    pub z_name: Vec<u8>,
    /// Número de ponteiros para este objeto.
    pub n_ref_module: i32,
    /// `pAux` passado a `create_module()`.
    pub p_aux: CallbackArg,
    /// Destrutor do módulo.
    pub x_destroy: Option<Rc<dyn Fn(&CallbackArg)>>,
    /// Tabela epônima deste módulo.
    pub p_epo_tab: Option<TableRef>,
}

/// Informação sobre cada coluna de uma tabela SQL, em `Table.aCol[]`.
///
/// "Table column index" é o índice em `Table.aCol[]` e no CREATE TABLE original. "Storage column
/// index" é o índice no registro gerado por OP_MakeRecord; é menor ou igual ao índice de tabela e
/// igual se e somente se não há colunas VIRTUAL à esquerda.
///
/// `z_cn_name` guarda, numa única alocação e nesta ordem, o nome da coluna, o tipo de dado
/// (só com COLFLAG_HASTYPE) e o nome da collation (só com COLFLAG_HASCOLL), cada um terminado
/// em 0x00.
#[derive(Clone, Default)]
pub struct Column {
    /// Nome da coluna.
    pub z_cn_name: Vec<u8>,
    /// Código OE_ para NOT NULL (campo de 4 bits).
    pub not_null: u8,
    /// Um dos tipos padrão (campo de 4 bits).
    pub e_c_type: u8,
    /// Um dos valores SQLITE_AFF_...
    pub affinity: u8,
    /// Tamanho estimado do valor; sizeof(INT)==1.
    pub sz_est: u8,
    /// Hash do nome da coluna para busca rápida.
    pub h_name: u8,
    /// Índice (base 1) do DEFAULT; 0 significa nenhum.
    pub i_dflt: u16,
    /// Propriedades booleanas (COLFLAG_*).
    pub col_flags: u16,
}

// Valores permitidos de `Column.eCType`. Devem casar com as entradas dos arranjos constantes
// `sqlite3StdTypeLen[]` e `sqlite3StdType[]`; cada valor é um a mais que o deslocamento nesses
// arranjos. Ajustar SQLITE_N_STDTYPE ao adicionar ou remover entradas.
/// Tipo anexado ao nome.
pub const COLTYPE_CUSTOM: u8 = 0;
pub const COLTYPE_ANY: u8 = 1;
pub const COLTYPE_BLOB: u8 = 2;
pub const COLTYPE_INT: u8 = 3;
pub const COLTYPE_INTEGER: u8 = 4;
pub const COLTYPE_REAL: u8 = 5;
pub const COLTYPE_TEXT: u8 = 6;
/// Número de tipos padrão.
pub const SQLITE_N_STDTYPE: usize = 6;

// Valores permitidos de `Column.colFlags`.
// Restrições: TF_HasVirtual == COLFLAG_VIRTUAL, TF_HasStored == COLFLAG_STORED,
// TF_HasHidden == COLFLAG_HIDDEN.
/// Coluna faz parte da chave primária.
pub const COLFLAG_PRIMKEY: u16 = 0x0001;
/// Coluna oculta numa tabela virtual.
pub const COLFLAG_HIDDEN: u16 = 0x0002;
/// O nome do tipo segue o nome da coluna.
pub const COLFLAG_HASTYPE: u16 = 0x0004;
/// A definição da coluna contém "UNIQUE" ou "PK".
pub const COLFLAG_UNIQUE: u16 = 0x0008;
/// Usar sorter-refs com esta coluna.
pub const COLFLAG_SORTERREF: u16 = 0x0010;
/// GENERATED ALWAYS AS ... VIRTUAL.
pub const COLFLAG_VIRTUAL: u16 = 0x0020;
/// GENERATED ALWAYS AS ... STORED.
pub const COLFLAG_STORED: u16 = 0x0040;
/// Coluna STORED ainda não calculada.
pub const COLFLAG_NOTAVAIL: u16 = 0x0080;
/// Bloqueia recursão em colunas GENERATED.
pub const COLFLAG_BUSY: u16 = 0x0100;
/// O nome da collation está em zCnName.
pub const COLFLAG_HASCOLL: u16 = 0x0200;
/// Omitir esta coluna ao expandir "*".
pub const COLFLAG_NOEXPAND: u16 = 0x0400;
/// Combinação: _STORED, _VIRTUAL.
pub const COLFLAG_GENERATED: u16 = 0x0060;
/// Combinação: _HIDDEN, _STORED, _VIRTUAL.
pub const COLFLAG_NOINSERT: u16 = 0x0062;

/// Uma "Collating Sequence": um nome e uma rotina de comparação que define a ordem. Se `x_cmp` é
/// `None` a sequência é indefinida e índices construídos sobre ela não podem ser lidos nem
/// escritos.
pub struct CollSeq {
    /// Nome da sequência, codificado em UTF-8.
    pub z_name: Vec<u8>,
    /// Codificação de texto tratada por `x_cmp()`.
    pub enc: u8,
    /// Primeiro argumento de `x_cmp()`.
    pub p_user: CallbackArg,
    /// Função de comparação.
    pub x_cmp: Option<Rc<dyn Fn(&CallbackArg, &[u8], &[u8]) -> i32>>,
    /// Destrutor de `p_user`.
    pub x_del: Option<Rc<dyn Fn(&CallbackArg)>>,
}

// Ordem de classificação.
/// Ordem ascendente.
pub const SQLITE_SO_ASC: i32 = 0;
/// Ordem descendente (o comentário do C repete "ascending").
pub const SQLITE_SO_DESC: i32 = 1;
/// Nenhuma ordem especificada.
pub const SQLITE_SO_UNDEFINED: i32 = -1;

// Tipos de afinidade de coluna. Numerados consecutivamente a partir de 'A' para que, concatenados
// num operando P4, fiquem legíveis; os tipos numéricos ficam juntos (teste numa só comparação) e
// BLOB vem primeiro.
/// '@'
pub const SQLITE_AFF_NONE: u8 = 0x40;
/// 'A'
pub const SQLITE_AFF_BLOB: u8 = 0x41;
/// 'B'
pub const SQLITE_AFF_TEXT: u8 = 0x42;
/// 'C'
pub const SQLITE_AFF_NUMERIC: u8 = 0x43;
/// 'D'
pub const SQLITE_AFF_INTEGER: u8 = 0x44;
/// 'E'
pub const SQLITE_AFF_REAL: u8 = 0x45;
/// 'F'
pub const SQLITE_AFF_FLEXNUM: u8 = 0x46;

/// Verdadeiro se a afinidade é numérica.
#[inline]
pub fn is_numeric_affinity(x: u8) -> bool {
    x >= SQLITE_AFF_NUMERIC
}

/// Máscara dos bits significativos de um valor de afinidade.
pub const SQLITE_AFF_MASK: u8 = 0x47;

// Bits adicionais que podem ser combinados com a afinidade sem mudá-la. SQLITE_NOTNULL combina
// NULLEQ e JUMPIFNULL: um assert() dispara se algum operando da comparação for NULL.
/// Salta se algum operando for NULL.
pub const SQLITE_JUMPIFNULL: u8 = 0x10;
/// NULL=NULL.
pub const SQLITE_NULLEQ: u8 = 0x80;
/// Garante que os operandos nunca são NULL.
pub const SQLITE_NOTNULL: u8 = 0x90;

/// Objeto criado para cada tabela virtual presente no schema do banco.
///
/// Com schema compartilhado há uma instância por conexão que usa o schema, porque cada conexão
/// precisa do próprio handle `sqlite3_vtab*` (que guarda a conexão recebida em xConnect() ou
/// xCreate()). Os VTable de uma mesma tabela ficam numa lista encadeada em `Table.pVTable`. Quando
/// um `Table` em memória é apagado, os VTable não são apagados nem desconectados na hora: vão para
/// a lista de `sqlite3.pDisconnect` da conexão e são desconectados no próximo prepare, para evitar
/// deadlock entre mutexes `sqlite3.mutex` (ver `sqlite3VtabUnlockList()`).
pub struct VTable {
    /// Conexão de banco associada a esta tabela.
    pub db: Weak<RefCell<Sqlite3>>,
    /// Implementação do módulo.
    pub p_mod: Option<ModuleRef>,
    /// Instância de vtab.
    pub p_vtab: Option<Rc<RefCell<Sqlite3Vtab>>>,
    /// Número de ponteiros para esta estrutura.
    pub n_ref: i32,
    /// Verdadeiro se há suporte a restrições.
    pub b_constraint: u8,
    /// Verdadeiro se pode usar qualquer schema anexado.
    pub b_all_schemas: u8,
    /// Risco de permitir acesso de um atacante.
    pub e_vtab_risk: u8,
    /// Profundidade da pilha de SAVEPOINT.
    pub i_savepoint: i32,
    /// Próximo da lista encadeada.
    pub p_next: Option<VTableRef>,
}


// ---- part_006.rs ----

/// Referência compartilhada para uma tabela, visão ou tabela virtual.
pub type TableRef = Rc<RefCell<Table>>;
/// Referência compartilhada para um índice.
pub type IndexRef = Rc<RefCell<Index>>;
/// Referência compartilhada para uma chave estrangeira.
pub type FKeyRef = Rc<RefCell<FKey>>;
/// Referência compartilhada para um `KeyInfo`.
pub type KeyInfoRef = Rc<RefCell<KeyInfo>>;
/// Referência compartilhada para um schema.
pub type SchemaRef = Rc<RefCell<Schema>>;
/// Referência compartilhada para um trigger.
pub type TriggerRef = Rc<RefCell<Trigger>>;

// Valores permitidos de VTable.eVtabRisk.
pub const SQLITE_VTABRISK_LOW: u8 = 0;
pub const SQLITE_VTABRISK_NORMAL: u8 = 1;
pub const SQLITE_VTABRISK_HIGH: u8 = 2;

/// Union `u` de `Table`: o `struct tab` das tabelas comuns.
#[derive(Default)]
pub struct TableTab {
    /// Deslocamento no CREATE TABLE em que se adiciona uma nova coluna.
    pub add_col_offset: i32,
    /// Lista encadeada de todas as chaves estrangeiras desta tabela.
    pub p_fkey: Option<FKeyRef>,
    /// Cláusulas DEFAULT das colunas, ou a cláusula AS das colunas geradas.
    pub p_dflt_list: Option<Box<ExprList>>,
}

/// Union `u` de `Table`: o `struct view` das views.
#[derive(Default)]
pub struct TableView {
    /// Definição da view.
    pub p_select: Option<Box<Select>>,
}

/// Union `u` de `Table`: o `struct vtab`, usado só por tabelas virtuais.
#[derive(Default)]
pub struct TableVtab {
    /// Número de argumentos do módulo.
    pub n_arg: i32,
    /// 0: módulo, 1: schema, 2: nome da vtab, 3...: argumentos.
    pub az_arg: Vec<Vec<u8>>,
    /// Lista de objetos VTable.
    pub p: Option<VTableRef>,
}

/// Union `u` de `Table`, discriminada por `eTabType` (TABTYP_NORM, TABTYP_VIEW, TABTYP_VTAB).
pub enum TableU {
    /// Tabelas comuns.
    Tab(TableTab),
    /// Views.
    View(TableView),
    /// Tabelas virtuais.
    VTab(TableVtab),
}

impl Default for TableU {
    fn default() -> Self {
        TableU::Tab(TableTab::default())
    }
}

/// O schema de cada tabela SQL, tabela virtual e view é representado em memória por uma
/// instância desta estrutura.
pub struct Table {
    /// Nome da tabela ou view.
    pub z_name: Vec<u8>,
    /// Informação sobre cada coluna.
    pub a_col: Vec<Column>,
    /// Lista de índices SQL desta tabela.
    pub p_index: Option<IndexRef>,
    /// String que define a afinidade de cada coluna.
    pub z_col_aff: Vec<u8>,
    /// Todas as restrições CHECK; numa VIEW também serve de lista de nomes de coluna.
    pub p_check: Option<Box<ExprList>>,
    /// Página raiz da BTree desta tabela.
    pub tnum: Pgno,
    /// Número de ponteiros para esta Table.
    pub n_tab_ref: u32,
    /// Máscara de valores TF_*.
    pub tab_flags: u32,
    /// Se não negativo, usa `aCol[iPKey]` como rowid.
    pub i_p_key: i16,
    /// Número de colunas da tabela.
    pub n_col: i16,
    /// Número de colunas que não são VIRTUAL.
    pub n_nv_col: i16,
    /// Linhas estimadas na tabela, vindas de sqlite_stat1.
    pub n_row_log_est: LogEst,
    /// Tamanho estimado de cada linha da tabela, em bytes.
    pub sz_tab_row: LogEst,
    /// O que fazer em caso de conflito de unicidade em iPKey.
    pub key_conf: u8,
    /// 0: normal, 1: virtual, 2: view.
    pub e_tab_type: u8,
    /// Union `u`.
    pub u: TableU,
    /// Lista de triggers deste objeto.
    pub p_trigger: Option<TriggerRef>,
    /// Schema que contém esta tabela.
    pub p_schema: Option<Weak<RefCell<Schema>>>,
}

// Valores permitidos de Table.tabFlags.
//
// TF_OOOHidden vale para tabelas ou views com colunas ocultas seguidas de colunas não ocultas.
// Exemplo: "CREATE VIRTUAL TABLE x USING vtab1(a HIDDEN, b);". Tais tabelas exigem tratamento
// especial no INSERT. "OOO" quer dizer "Out Of Order".
//
// Restrições: TF_HasVirtual == COLFLAG_VIRTUAL, TF_HasStored == COLFLAG_STORED e
// TF_HasHidden == COLFLAG_HIDDEN.
/// Tabela de sistema somente leitura.
pub const TF_READONLY: u32 = 0x00000001;
/// Tem uma ou mais colunas ocultas.
pub const TF_HAS_HIDDEN: u32 = 0x00000002;
/// Tabela com chave primária.
pub const TF_HAS_PRIMARY_KEY: u32 = 0x00000004;
/// A chave primária inteira é autoincremento.
pub const TF_AUTOINCREMENT: u32 = 0x00000008;
/// `n_row_log_est` veio de sqlite_stat1.
pub const TF_HAS_STAT1: u32 = 0x00000010;
/// Tem uma ou mais colunas VIRTUAL.
pub const TF_HAS_VIRTUAL: u32 = 0x00000020;
/// Tem uma ou mais colunas STORED.
pub const TF_HAS_STORED: u32 = 0x00000040;
/// Combinação: HasVirtual + HasStored.
pub const TF_HAS_GENERATED: u32 = 0x00000060;
/// Sem rowid; a PRIMARY KEY é a chave.
pub const TF_WITHOUT_ROWID: u32 = 0x00000080;
/// Talvez rodar ANALYZE nesta tabela.
pub const TF_MAYBE_REANALYZE: u32 = 0x00000100;
/// Sem coluna "rowid" visível ao usuário.
pub const TF_NO_VISIBLE_ROWID: u32 = 0x00000200;
/// Colunas ocultas fora de ordem.
pub const TF_OOO_HIDDEN: u32 = 0x00000400;
/// Contém restrições NOT NULL.
pub const TF_HAS_NOT_NULL: u32 = 0x00000800;
/// Verdadeiro para uma tabela sombra.
pub const TF_SHADOW: u32 = 0x00001000;
/// Há informação STAT4 para esta tabela.
pub const TF_HAS_STAT4: u32 = 0x00002000;
/// Tabela efêmera.
pub const TF_EPHEMERAL: u32 = 0x00004000;
/// Tabela virtual epônima.
pub const TF_EPONYMOUS: u32 = 0x00008000;
/// Modo STRICT.
pub const TF_STRICT: u32 = 0x00010000;

// Valores permitidos de Table.eTabType.
/// Tabela comum.
pub const TABTYP_NORM: u8 = 0;
/// Tabela virtual.
pub const TABTYP_VTAB: u8 = 1;
/// View.
pub const TABTYP_VIEW: u8 = 2;

/// Verdadeiro se a tabela é uma view.
#[inline]
pub fn is_view(x: &Table) -> bool {
    x.e_tab_type == TABTYP_VIEW
}

/// Verdadeiro se a tabela é uma tabela comum.
#[inline]
pub fn is_ordinary_table(x: &Table) -> bool {
    x.e_tab_type == TABTYP_NORM
}

/// Verdadeiro se a tabela é virtual.
#[inline]
pub fn is_virtual(x: &Table) -> bool {
    x.e_tab_type == TABTYP_VTAB
}

/// Verdadeiro se a expressão é uma coluna de tabela virtual.
#[inline]
pub fn expr_is_vtab(x: &Expr) -> bool {
    x.op == TK_COLUMN
        && x.y.p_tab.as_ref().map_or(false, |t| t.borrow().e_tab_type == TABTYP_VTAB)
}

/// Verdadeiro se a coluna é oculta (de propósito geral).
#[inline]
pub fn is_hidden_column(x: &Column) -> bool {
    (x.col_flags & COLFLAG_HIDDEN) != 0
}

/// Verdadeiro se a coluna é oculta numa tabela não virtual (tabela comum ou view); sem
/// SQLITE_ENABLE_HIDDEN_COLUMNS é sempre falso.
#[inline]
pub fn is_ordinary_hidden_column(_x: &Column) -> bool {
    false
}

/// A tabela tem rowid?
#[inline]
pub fn has_rowid(x: &Table) -> bool {
    (x.tab_flags & TF_WITHOUT_ROWID) == 0
}

/// O rowid é visível ao usuário?
#[inline]
pub fn visible_rowid(x: &Table) -> bool {
    (x.tab_flags & TF_NO_VISIBLE_ROWID) == 0
}

/// Verdadeira se a (falha de) funcionalidade SQLITE_ALLOW_ROWID_IN_VIEW está disponível; por
/// padrão é falsa.
pub const VIEW_CAN_HAVE_ROWID: bool = false;

/// Cada restrição de chave estrangeira é uma instância desta estrutura.
///
/// Uma chave estrangeira se associa a duas tabelas. A tabela "from" (filha) contém a cláusula
/// REFERENCES que cria a chave; a tabela "to" (pai) é a nomeada na cláusula. No exemplo
/// `CREATE TABLE ex1(a INTEGER PRIMARY KEY, b INTEGER CONSTRAINT fk1 REFERENCES ex2(x));`, para
/// a chave "fk1" a tabela from é "ex1" e a to é "ex2".
///
/// Cada cláusula REFERENCES gera uma instância ligada à tabela from; a tabela to não precisa
/// existir na criação e sua existência não é verificada. A lista de todos os pais de uma Table X
/// fica em `X.pFKey`; a lista de todos os filhos de uma tabela Z (que pode nem existir) fica em
/// `Schema.fkeyHash` com chave hash Z.
pub struct FKey {
    /// Tabela que contém a cláusula REFERENCES (filha).
    pub p_from: Weak<RefCell<Table>>,
    /// Próxima FKey com o mesmo pFrom; próximo pai de pFrom.
    pub p_next_from: Option<FKeyRef>,
    /// Nome da tabela para a qual a chave aponta (pai).
    pub z_to: Vec<u8>,
    /// Próxima com o mesmo zTo; próximo filho de zTo.
    pub p_next_to: Option<FKeyRef>,
    /// Anterior com o mesmo zTo.
    pub p_prev_to: Option<Weak<RefCell<FKey>>>,
    /// Número de colunas da chave.
    pub n_col: i32,
    /// Verdadeiro se a verificação é adiada até o COMMIT.
    pub is_deferred: u8,
    /// Ações de ON DELETE e ON UPDATE, respectivamente.
    pub a_action: [u8; 2],
    /// Triggers das ações de `a_action`.
    pub ap_trigger: [Option<TriggerRef>; 2],
    /// Mapeamento das colunas de pFrom para colunas de zTo, uma entrada por coluna.
    pub a_col: Vec<SColMap>,
}

/// Mapeamento de coluna de uma FKey (`struct sColMap`).
pub struct SColMap {
    /// Índice da coluna em pFrom.
    pub i_from: i32,
    /// Nome da coluna em zTo; `None` usa a PRIMARY KEY.
    pub z_col: Option<Vec<u8>>,
}

// O SQLite resolve erros de restrição de várias formas. ROLLBACK faz a operação falhar e
// desfaz a transação. ABORT faz a operação falhar e desfaz as mudanças dela, sem desfazer a
// transação. FAIL para a operação e devolve erro, sem desfazer mudanças anteriores da mesma
// operação nem a transação. IGNORE descarta a linha que causou o erro e segue sem erro. REPLACE
// remove as linhas preexistentes que violam UNIQUE para o insert ou update prosseguir. UPDATE
// vale só para insert: omite o insert e roda a cláusula DO UPDATE do upsert.
//
// RESTRICT, SETNULL, SETDFLT e CASCADE valem só para chaves estrangeiras. RESTRICT equivale a
// ABORT para chaves IMMEDIATE e a ROLLBACK para DEFERRED. SETNULL põe NULL na chave, SETDFLT põe
// o valor padrão e CASCADE propaga o DELETE ou UPDATE da linha referenciada. OE_Default é um
// marcador que significa "usar o algoritmo que o contexto exigir".
/// Não há restrição a verificar.
pub const OE_NONE: u8 = 0;
/// Falha a operação e desfaz a transação.
pub const OE_ROLLBACK: u8 = 1;
/// Desfaz as mudanças, mas não a transação.
pub const OE_ABORT: u8 = 2;
/// Para a operação, mas deixa as mudanças anteriores.
pub const OE_FAIL: u8 = 3;
/// Ignora o erro; não faz o INSERT ou UPDATE.
pub const OE_IGNORE: u8 = 4;
/// Apaga o registro existente e faz o INSERT ou UPDATE.
pub const OE_REPLACE: u8 = 5;
/// Processa como um DO UPDATE de upsert.
pub const OE_UPDATE: u8 = 6;
/// OE_ABORT para IMMEDIATE, OE_ROLLBACK para DEFERRED.
pub const OE_RESTRICT: u8 = 7;
/// Põe NULL no valor da chave estrangeira.
pub const OE_SET_NULL: u8 = 8;
/// Põe o valor padrão na chave estrangeira.
pub const OE_SET_DFLT: u8 = 9;
/// Propaga as mudanças em cascata.
pub const OE_CASCADE: u8 = 10;
/// Faz o que a ação padrão mandar.
pub const OE_DEFAULT: u8 = 11;

/// Instância passada como primeiro argumento de `sqlite3VdbeKeyCompare` para controlar a
/// comparação de duas chaves de índice. `a_sort_flags` e `a_coll` têm `n_field + 1` posições:
/// uma por coluna do índice mais uma extra para o rowid, no fim.
pub struct KeyInfo {
    /// Número de referências a este objeto.
    pub n_ref: u32,
    /// Codificação de texto, um dos valores SQLITE_UTF*.
    pub enc: u8,
    /// Número de colunas-chave do índice.
    pub n_key_field: u16,
    /// Total de colunas, incluindo chave e outras.
    pub n_all_field: u16,
    /// A conexão de banco de dados.
    pub db: Weak<RefCell<Sqlite3>>,
    /// Ordem de classificação de cada coluna.
    pub a_sort_flags: Vec<u8>,
    /// Sequência de comparação de cada termo da chave.
    pub a_coll: Vec<Option<CollSeqRef>>,
}

// Bits permitidos nas entradas de KeyInfo.aSortFlags[].
/// Ordem DESC.
pub const KEYINFO_ORDER_DESC: u8 = 0x01;
/// NULL é maior que qualquer outro valor.
pub const KEYINFO_ORDER_BIGNULL: u8 = 0x02;

/// Cache da union `u` de `UnpackedRecord`.
pub enum UnpackedRecordU {
    /// Cache de `aMem[0].z` para `vdbeRecordCompareString()`.
    Z(Option<Vec<u8>>),
    /// Cache de `aMem[0].u.i` para `vdbeRecordCompareInt()`.
    I(i64),
}

/// Registro já separado em campos individuais, para fazer comparações. Serve de "chave" na busca
/// numa b+tree de índice, cujo objetivo é achar a entrada mais próxima da chave descrita aqui;
/// pode guardar só um prefixo da chave, e o número de campos vem de `pKeyInfo->nField`.
///
/// `r1` e `r2` são os valores a retornar se esta chave for menor ou maior que a da btree
/// (normalmente -1 e +1, invertidos numa b-tree DESC). As funções de comparação retornam
/// `default_rc` (-1, 0 ou +1) quando acham igualdade: -1 faz a busca achar a última ocorrência e
/// +1 a primeira. Elas ligam `eq_seen` se virem alguma igualdade; com `default_rc != 0` a busca
/// pode parar logo antes da primeira ocorrência ou logo depois da última, e `eq_seen` diz se há
/// correspondência exata.
pub struct UnpackedRecord {
    /// Informação de collation e ordem de classificação.
    pub p_key_info: Option<KeyInfoRef>,
    /// Valores.
    pub a_mem: Vec<Mem>,
    /// Union `u`.
    pub u: UnpackedRecordU,
    /// Cache de `aMem[0].n` usado por `vdbeRecordCompareString()`.
    pub n: i32,
    /// Número de entradas em `a_mem`.
    pub n_field: u16,
    /// Resultado da comparação se as chaves forem iguais.
    pub default_rc: i8,
    /// Erro detectado por xRecordCompare (CORRUPT ou NOMEM).
    pub err_code: u8,
    /// Valor a retornar se (lhs < rhs).
    pub r1: i8,
    /// Valor a retornar se (lhs > rhs).
    pub r2: i8,
    /// Verdadeiro se uma comparação de igualdade foi vista.
    pub eq_seen: u8,
}

/// Cada índice SQL é representado em memória por uma instância desta estrutura.
///
/// As colunas indexadas são descritas por `ai_column`. Com `CREATE TABLE Ex1(c1 int, c2 int, c3
/// text); CREATE INDEX Ex2 ON Ex1(c3,c1);`, a Table de Ex1 tem nCol==3 e o Index de Ex2 tem
/// nColumn==2 e aiColumn {2, 0}: a primeira coluna indexada (c3) tem índice 2 em Ex1.aCol[], e a
/// segunda (c1) tem índice 0.
///
/// `on_error` diz se as colunas devem ser únicas e o que fazer se não forem: `OE_NONE` significa
/// índice não único; caso contrário é único e o valor indica o algoritmo de resolução de conflito.
///
/// `col_not_idxed` é usada com `SrcItem.colUsed` para testar rápido se o índice é cobridor: tem um
/// bit 1 para cada coluna da tabela que NÃO está no índice, de modo que "colUsed & colNotIdxed"
/// é diferente de zero se o índice não é cobridor. O bit mais significativo é sempre 1
/// (note-20221022-a); se uma coluna além da 63ª é usada o teste é sempre diferente de zero e é
/// preciso assumir que o índice não é cobridor ou usar um algoritmo alternativo, mais lento.
///
/// Ao analisar um CREATE TABLE ou CREATE INDEX para gerar código VDBE (e não ao ler o
/// sqlite_schema), podem ser criadas instâncias transitórias em que `tnum` guarda o endereço de
/// uma instrução VDBE, não um número de página (ver `convertToWithoutRowidTable()`).
pub struct Index {
    /// Nome deste índice.
    pub z_name: Vec<u8>,
    /// Quais colunas o índice usa; a primeira é 0.
    pub ai_column: Vec<i16>,
    /// Do ANALYZE: estimativa de linhas selecionadas por cada coluna.
    pub ai_row_log_est: Vec<LogEst>,
    /// A tabela SQL indexada.
    pub p_table: Weak<RefCell<Table>>,
    /// String que define a afinidade de cada coluna.
    pub z_col_aff: Vec<u8>,
    /// Próximo índice associado à mesma tabela.
    pub p_next: Option<IndexRef>,
    /// Schema que contém este índice.
    pub p_schema: Option<Weak<RefCell<Schema>>>,
    /// Por coluna: verdadeiro == DESC, falso == ASC.
    pub a_sort_order: Vec<u8>,
    /// Nomes das sequências de comparação do índice.
    pub az_coll: Vec<Vec<u8>>,
    /// Cláusula WHERE dos índices parciais.
    pub p_part_idx_where: Option<Box<Expr>>,
    /// Expressões de coluna.
    pub a_col_expr: Option<Box<ExprList>>,
    /// Página do banco que contém a raiz deste índice.
    pub tnum: Pgno,
    /// Tamanho médio estimado da linha, em bytes.
    pub sz_idx_row: LogEst,
    /// Número de colunas que formam a chave.
    pub n_key_col: u16,
    /// Número de colunas guardadas no índice.
    pub n_column: u16,
    /// OE_ABORT, OE_IGNORE, OE_REPLACE ou OE_NONE.
    pub on_error: u8,
    /// 0: normal, 1: UNIQUE, 2: PRIMARY KEY, 3: IPK (campo de 2 bits).
    pub idx_type: u8,
    /// Usar este índice só em consultas com == ou IN.
    pub b_unordered: bool,
    /// Verdadeiro se UNIQUE e NOT NULL em todas as colunas.
    pub uniq_not_null: bool,
    /// Verdadeiro se `resizeIndexObject()` foi chamada.
    pub is_resized: bool,
    /// Verdadeiro se é um índice cobridor.
    pub is_covering: bool,
    /// Não tentar skip-scan se verdadeiro.
    pub no_skip_scan: bool,
    /// Valores de `ai_row_log_est` vêm de sqlite_stat1.
    pub has_stat1: bool,
    /// sqlite_stat1 diz que o índice é de baixa qualidade.
    pub b_low_qual: bool,
    /// Não usar este índice para otimizar consultas.
    pub b_no_query: bool,
    /// Verdadeiro se o bug bba7b69f9849b5bf se aplica.
    pub b_asc_key_bug: bool,
    /// O índice referencia uma ou mais colunas VIRTUAL.
    pub b_has_vcol: bool,
    /// O índice contém uma expressão literal ou referência a coluna VIRTUAL.
    pub b_has_expr: bool,
    /// Número de elementos em `a_sample`.
    pub n_sample: i32,
    /// Número de posições alocadas em `a_sample`.
    pub mx_sample: i32,
    /// Tamanho de `IndexSample.anEq[]` e afins.
    pub n_sample_col: i32,
    /// Valores nEq médios para chaves que não estão em `a_sample`.
    pub a_avg_eq: Vec<tRowcnt>,
    /// Amostras da chave mais à esquerda.
    pub a_sample: Vec<IndexSample>,
    /// Dados stat1 não logarítmicos deste índice.
    pub ai_row_est: Vec<tRowcnt>,
    /// Número não logarítmico de linhas do índice.
    pub n_row_est0: tRowcnt,
    /// Colunas não indexadas em pTab.
    pub col_not_idxed: Bitmask,
}


// ---- part_007.rs ----

/// Referência compartilhada para um `AggInfo`.
pub type AggInfoRef = Rc<RefCell<AggInfo>>;
/// Referência compartilhada para uma definição de janela.
pub type WindowRef = Rc<RefCell<Window>>;

// Valores permitidos de Index.idxType.
/// Criado com CREATE INDEX.
pub const SQLITE_IDXTYPE_APPDEF: u8 = 0;
/// Implementa uma restrição UNIQUE.
pub const SQLITE_IDXTYPE_UNIQUE: u8 = 1;
/// É a PRIMARY KEY da tabela.
pub const SQLITE_IDXTYPE_PRIMARYKEY: u8 = 2;
/// Índice de INTEGER PRIMARY KEY.
pub const SQLITE_IDXTYPE_IPK: u8 = 3;

/// Verdadeiro se o índice é um índice de PRIMARY KEY.
#[inline]
pub fn is_primary_key_index(x: &Index) -> bool {
    x.idx_type == SQLITE_IDXTYPE_PRIMARYKEY
}

/// Verdadeiro se o índice é um índice UNIQUE.
#[inline]
pub fn is_unique_index(x: &Index) -> bool {
    x.on_error != OE_NONE
}

// Os valores de Index.aiColumn[] são normalmente inteiros positivos, mas alguns valores negativos
// têm significado especial.
/// A coluna indexada é o rowid.
pub const XN_ROWID: i16 = -1;
/// A coluna indexada é uma expressão.
pub const XN_EXPR: i16 = -2;

/// Cada amostra guardada na tabela sqlite_stat4 é representada em memória por esta estrutura (ver
/// a documentação no início de analyze.c). O `n` do C (tamanho do registro) é `p.len()`.
pub struct IndexSample {
    /// Registro amostrado.
    pub p: Vec<u8>,
    /// Estimativa de linhas em que a chave é igual a esta amostra.
    pub an_eq: Vec<tRowcnt>,
    /// Estimativa de linhas em que a chave é menor que esta amostra.
    pub an_lt: Vec<tRowcnt>,
    /// Estimativa de chaves distintas menores que esta amostra.
    pub an_d_lt: Vec<tRowcnt>,
}

// Valores possíveis do argumento flags de sqlite3GetToken().
/// O token é um identificador entre aspas.
pub const SQLITE_TOKEN_QUOTED: u32 = 0x1;
/// O token é uma palavra-chave.
pub const SQLITE_TOKEN_KEYWORD: u32 = 0x2;

/// Cada token que sai do lexer é uma instância desta estrutura; tokens também são usados em
/// expressões. No C, `z` aponta para texto de outros objetos (muitas vezes o meio de
/// `Parse.zSql`); aqui o token carrega a própria cópia do texto, e `n` é `z.len()`.
#[derive(Clone, Default)]
pub struct Token {
    /// Texto do token. Não termina em nulo.
    pub z: Vec<u8>,
    /// Número de caracteres do token.
    pub n: u32,
}

/// Informação necessária para gerar código de um SELECT com funções de agregação.
///
/// Se `Expr.op` é TK_AGG_COLUMN ou TK_AGG_FUNCTION, `Expr.pAggInfo` aponta para esta estrutura e
/// `Expr.iAgg` é o índice em `a_col` ou `a_func` das informações para gerar o código daquele nó.
/// `p_group_by` e `a_func.p_f_expr` apontam para campos do `Select` original e não são liberados
/// com o `AggInfo`.
pub struct AggInfo {
    /// Modo de renderização direta: pega dados direto das tabelas de origem e não dos acumuladores.
    pub direct_mode: u8,
    /// No modo direto, referencia o índice de ordenação em vez da tabela de origem.
    pub use_sorting_idx: u8,
    /// Número de colunas do índice de ordenação.
    pub n_sorting_column: u16,
    /// Número do cursor do índice de ordenação.
    pub sorting_idx: i32,
    /// Número do cursor da pseudo-tabela.
    pub sorting_idx_p_tab: i32,
    /// Primeiro registro do intervalo de `a_col` e `a_func`.
    pub i_first_reg: i32,
    /// A cláusula GROUP BY.
    pub p_group_by: Option<Rc<RefCell<ExprList>>>,
    /// Uma entrada por coluna usada nas tabelas de origem.
    pub a_col: Vec<AggInfoCol>,
    /// Número de entradas usadas em `a_col`.
    pub n_column: i32,
    /// Número de colunas que aparecem na saída; as demais só servem de parâmetro de agregações.
    pub n_accumulator: i32,
    /// Uma entrada por função de agregação.
    pub a_func: Vec<AggInfoFunc>,
    /// Número de entradas em `a_func`.
    pub n_func: i32,
    /// Select ao qual este AggInfo pertence.
    pub sel_id: u32,
}

/// `struct AggInfo_col`: coluna usada nas tabelas de origem.
pub struct AggInfoCol {
    /// Tabela de origem.
    pub p_tab: Option<TableRef>,
    /// A expressão original.
    pub p_c_expr: Option<Rc<RefCell<Expr>>>,
    /// Número do cursor da tabela de origem.
    pub i_table: i32,
    /// Número da coluna na tabela de origem.
    pub i_column: i16,
    /// Número da coluna no índice de ordenação.
    pub i_sorter_column: i16,
}

/// `struct AggInfo_func`: função de agregação.
pub struct AggInfoFunc {
    /// Expressão que codifica a função.
    pub p_f_expr: Option<Rc<RefCell<Expr>>>,
    /// Implementação da função de agregação.
    pub p_func: Option<FuncDefRef>,
    /// Tabela efêmera usada para impor DISTINCT.
    pub i_distinct: i32,
    /// Endereço de OP_OpenEphemeral.
    pub i_dist_addr: i32,
    /// Tabela efêmera que implementa ORDER BY.
    pub i_ob_tab: i32,
    /// `i_ob_tab` tem colunas de carga separadas da chave.
    pub b_ob_payload: u8,
    /// Impõe unicidade nas chaves de `i_ob_tab`.
    pub b_ob_unique: u8,
    /// Transfere informação de subtipo pelo sorter.
    pub b_use_subtype: u8,
}

/// Número do registro de `a_col[i]`. Não usar antes de `assignAggregateRegisters()`, que calcula
/// `i_first_reg` (o assert do C verifica essa restrição).
#[inline]
pub fn agg_info_column_reg(a: &AggInfo, i: i32) -> i32 {
    debug_assert!(a.i_first_reg != 0);
    a.i_first_reg + i
}

/// Número do registro de `a_func[i]`; mesma restrição de `agg_info_column_reg`.
#[inline]
pub fn agg_info_func_reg(a: &AggInfo, i: i32) -> i32 {
    debug_assert!(a.i_first_reg != 0);
    a.i_first_reg + a.n_column + i
}

/// O tipo `ynVar` é um inteiro com sinal de 16 ou 32 bits. Normalmente 16, mas com
/// SQLITE_MAX_VARIABLE_NUMBER maior que 32767 precisa ser de 32 bits; com
/// SQLITE_MAX_VARIABLE_NUMBER=250000 (Debian 13) é `i32`.
pub type YnVar = i32;

/// Union `u` de `Expr`: `zToken` (texto terminado em zero e sem aspas) ou `iValue` (inteiro não
/// negativo se EP_IntValue). Os dois campos coexistem e o `flags` diz qual vale.
#[derive(Clone, Default)]
pub struct ExprU {
    /// Valor do token: literal SQL, nome de variável ou nome de função.
    pub z_token: Option<Vec<u8>>,
    /// Valor inteiro não negativo se EP_IntValue.
    pub i_value: i32,
}

/// Union `x` de `Expr`: `pList` ou `pSelect` (vale `pSelect` se EP_xIsSelect está ligado).
#[derive(Default)]
pub struct ExprX {
    /// op = IN, EXISTS, SELECT, CASE, FUNCTION, BETWEEN.
    pub p_list: Option<Box<ExprList>>,
    /// EP_xIsSelect e op = IN, EXISTS, SELECT.
    pub p_select: Option<Box<Select>>,
}

/// Union `w` de `Expr`: `iJoin` (tabela direita, se EP_OuterON ou EP_InnerON) ou `iOfst`
/// (início do token desde o início da instrução).
#[derive(Clone, Copy, Default)]
pub struct ExprW {
    /// Tabela direita do join.
    pub i_join: i32,
    /// Deslocamento do início do token.
    pub i_ofst: i32,
}

/// Union `y` de `Expr`, campo `sub`: TK_IN, TK_SELECT e TK_EXISTS.
#[derive(Clone, Copy, Default)]
pub struct ExprYSub {
    /// Endereço de entrada da sub-rotina.
    pub i_addr: i32,
    /// Registro que guarda o endereço de retorno.
    pub reg_return: i32,
}

/// Union `y` de `Expr`.
#[derive(Default)]
pub struct ExprY {
    /// TK_COLUMN: tabela que contém a coluna; pode ser `None` para coluna de índice sobre expressão.
    pub p_tab: Option<TableRef>,
    /// EP_WinFunc: definição de janela/filtro da função.
    pub p_win: Option<WindowRef>,
    /// TK_IN, TK_SELECT e TK_EXISTS.
    pub sub: ExprYSub,
}

/// Cada nó de uma expressão na árvore de análise.
///
/// `op` é o opcode; os códigos inteiros de token do parser são reutilizados como opcodes (TK_GE
/// representa ">=" também na árvore). Num literal SQL (TK_INTEGER, TK_FLOAT, TK_BLOB, TK_STRING)
/// `u.z_token` tem o texto; numa variável (TK_VARIABLE) o nome; numa função (TK_FUNCTION) o nome.
/// `p_left` e `p_right` são as subexpressões de um operador binário (qualquer uma pode ser
/// ausente). `x.p_list` é a lista de argumentos de função, CASE ou IN com lista; `x.p_select`
/// vale com EP_xIsSelect (subselect ou "<lhs> IN (SELECT ...)").
///
/// Uma expressão ID ou ID.ID refere-se a uma coluna: `op` é TK_COLUMN, `i_table` o cursor VDBE e
/// `i_column` o número da coluna; num SELECT agregado o valor também vai para `i_agg`. Uma
/// variável sem valor ('?') guarda o número da variável em `i_table`. Numa subconsulta,
/// `i_column` é o registro com o resultado e `i_table` é -1 se o resultado é constante, ou o
/// endereço da sub-rotina que o calcula se varia durante o processamento. Para OP_Column numa
/// tabela de disco ou na pseudo-tabela "old.*", `y.p_tab` aponta para a definição da tabela.
///
/// Notas de alocação: no C os objetos Expr podem ser truncados (EP_Reduced, EP_TokenOnly) e
/// guardados numa só alocação junto com `z_token`. O modelo Rust não trunca: o `Expr` é sempre de
/// tamanho completo e os campos abaixo existem sempre; `EP_REDUCED` e `EP_TOKENONLY` seguem
/// existindo como bits de `flags`, por fidelidade às cópias feitas por `sqlite3ExprDup`.
pub struct Expr {
    /// Operação executada por este nó.
    pub op: u8,
    /// Afinidade, ou tipo de RAISE.
    pub aff_expr: u8,
    /// TK_REGISTER/TK_TRUTH: valor original de `op`; TK_COLUMN: valor de p5 de OP_Column;
    /// TK_AGG_FUNCTION: profundidade de aninhamento; TK_FUNCTION: NC_SelfRef se precisa OP_PureFunc.
    pub op2: u8,
    /// Vários sinalizadores EP_*.
    pub flags: u32,
    /// Union `u`.
    pub u: ExprU,
    /// Subnó esquerdo.
    pub p_left: Option<Box<Expr>>,
    /// Subnó direito.
    pub p_right: Option<Box<Expr>>,
    /// Union `x`.
    pub x: ExprX,
    /// Altura da árvore que começa neste nó (SQLITE_MAX_EXPR_DEPTH>0).
    pub n_height: i32,
    /// TK_COLUMN: cursor da tabela que tem a coluna; TK_REGISTER: número do registro;
    /// TK_TRIGGER: 1 -> new, 0 -> old; EP_Unlikely: 134217728 vezes a probabilidade; TK_IN: tabela
    /// efêmera com o lado direito; TK_SELECT_COLUMN: número de colunas do lado esquerdo;
    /// TK_SELECT: primeiro registro do vetor de resultado.
    pub i_table: i32,
    /// TK_COLUMN: índice da coluna, -1 para rowid; TK_VARIABLE: número da variável (sempre >= 1);
    /// TK_SELECT_COLUMN: coluna do vetor de resultado.
    pub i_column: YnVar,
    /// Qual entrada de `pAggInfo->aCol[]` ou `->aFunc[]`.
    pub i_agg: i16,
    /// Union `w`.
    pub w: ExprW,
    /// Usado por TK_AGG_COLUMN e TK_AGG_FUNCTION.
    pub p_agg_info: Option<AggInfoRef>,
    /// Union `y`.
    pub y: ExprY,
}

// Significado dos bits de Expr.flags. Restrições de valor: EP_Agg == NC_HasAgg == SF_HasAgg e
// EP_Win == NC_HasWin.
/// Origina-se numa cláusula ON/USING de outer join.
pub const EP_OUTER_ON: u32 = 0x000001;
/// Origina-se num ON/USING de inner join.
pub const EP_INNER_ON: u32 = 0x000002;
/// Função de agregação com a palavra-chave DISTINCT.
pub const EP_DISTINCT: u32 = 0x000004;
/// Contém uma ou mais funções de qualquer tipo.
pub const EP_HAS_FUNC: u32 = 0x000008;
/// Contém uma ou mais funções de agregação.
pub const EP_AGG: u32 = 0x000010;
/// TK_Column com valor fixo conhecido.
pub const EP_FIXED_COL: u32 = 0x000020;
/// `p_select` é correlacionado, não constante.
pub const EP_VAR_SELECT: u32 = 0x000040;
/// `token.z` estava originalmente entre "...".
pub const EP_DBL_QUOTED: u32 = 0x000080;
/// Verdadeiro para função infixa: LIKE, GLOB etc.
pub const EP_INFIX_FUNC: u32 = 0x000100;
/// A árvore contém um operador TK_COLLATE.
pub const EP_COLLATE: u32 = 0x000200;
/// O operador de comparação foi comutado.
pub const EP_COMMUTED: u32 = 0x000400;
/// Valor inteiro em `u.i_value`.
pub const EP_INT_VALUE: u32 = 0x000800;
/// `x.p_select` é válido (senão `x.p_list` é).
pub const EP_X_IS_SELECT: u32 = 0x001000;
/// O operador não contribui para a afinidade.
pub const EP_SKIP: u32 = 0x002000;
/// Struct Expr de apenas EXPR_REDUCEDSIZE bytes.
pub const EP_REDUCED: u32 = 0x004000;
/// Contém funções de janela.
pub const EP_WIN: u32 = 0x008000;
/// Struct Expr de apenas EXPR_TOKENONLYSIZE bytes.
pub const EP_TOKEN_ONLY: u32 = 0x010000;
/// A estrutura Expr deve continuar com tamanho completo.
pub const EP_FULL_SIZE: u32 = 0x020000;
/// O opcode TK_IF_NULL_ROW.
pub const EP_IF_NULL_ROW: u32 = 0x040000;
/// Função unlikely() ou likelihood().
pub const EP_UNLIKELY: u32 = 0x080000;
/// Função SQLITE_FUNC_CONSTANT ou _SLOCHNG.
pub const EP_CONST_FUNC: u32 = 0x100000;
/// Pode ser nulo apesar da restrição NOT NULL.
pub const EP_CAN_BE_NULL: u32 = 0x200000;
/// A árvore contém um operador TK_SELECT.
pub const EP_SUBQUERY: u32 = 0x400000;
/// `p_left`, `p_right` e `u.p_select` são todos nulos.
pub const EP_LEAF: u32 = 0x800000;
/// TK_FUNCTION com `y.p_win` definido.
pub const EP_WIN_FUNC: u32 = 0x1000000;
/// Usa `y.sub`: TK_IN, _SELECT ou _EXISTS.
pub const EP_SUBRTN: u32 = 0x2000000;
/// TK_ID estava originalmente entre aspas.
pub const EP_QUOTED: u32 = 0x4000000;
/// Mantido em memória não obtida de malloc().
pub const EP_STATIC: u32 = 0x8000000;
/// Sempre tem valor booleano TRUE.
pub const EP_IS_TRUE: u32 = 0x10000000;
/// Sempre tem valor booleano FALSE.
pub const EP_IS_FALSE: u32 = 0x20000000;
/// Origina-se de sqlite_schema.
pub const EP_FROM_DDL: u32 = 0x40000000;
// 0x80000000 está disponível.

/// Propriedades que se propagam automaticamente para os nós pais.
pub const EP_PROPAGATE: u32 = EP_COLLATE | EP_SUBQUERY | EP_HAS_FUNC;

/// Testa se algum bit de `p` está ligado em `Expr.flags`.
#[inline]
pub fn expr_has_property(e: &Expr, p: u32) -> bool {
    (e.flags & p) != 0
}

/// Testa se todos os bits de `p` estão ligados em `Expr.flags`.
#[inline]
pub fn expr_has_all_property(e: &Expr, p: u32) -> bool {
    (e.flags & p) == p
}

/// Liga bits em `Expr.flags`.
#[inline]
pub fn expr_set_property(e: &mut Expr, p: u32) {
    e.flags |= p;
}

/// Desliga bits em `Expr.flags`.
#[inline]
pub fn expr_clear_property(e: &mut Expr, p: u32) {
    e.flags &= !p;
}

/// Sempre verdadeira (e não vem de outer join).
#[inline]
pub fn expr_always_true(e: &Expr) -> bool {
    (e.flags & (EP_OUTER_ON | EP_IS_TRUE)) == EP_IS_TRUE
}

/// Sempre falsa (e não vem de outer join).
#[inline]
pub fn expr_always_false(e: &Expr) -> bool {
    (e.flags & (EP_OUTER_ON | EP_IS_FALSE)) == EP_IS_FALSE
}

/// O Expr é de tamanho completo?
#[inline]
pub fn expr_is_full_size(e: &Expr) -> bool {
    (e.flags & (EP_REDUCED | EP_TOKEN_ONLY)) == 0
}

// Garantem que os membros corretos das unions de Expr são acessados.
/// Vale `u.z_token`.
#[inline]
pub fn expr_use_u_token(e: &Expr) -> bool {
    (e.flags & EP_INT_VALUE) == 0
}

/// Vale `u.i_value`.
#[inline]
pub fn expr_use_u_value(e: &Expr) -> bool {
    (e.flags & EP_INT_VALUE) != 0
}

/// Vale `w.i_ofst`.
#[inline]
pub fn expr_use_w_ofst(e: &Expr) -> bool {
    (e.flags & (EP_INNER_ON | EP_OUTER_ON)) == 0
}

/// Vale `w.i_join`.
#[inline]
pub fn expr_use_w_join(e: &Expr) -> bool {
    (e.flags & (EP_INNER_ON | EP_OUTER_ON)) != 0
}

/// Vale `x.p_list`.
#[inline]
pub fn expr_use_x_list(e: &Expr) -> bool {
    (e.flags & EP_X_IS_SELECT) == 0
}

/// Vale `x.p_select`.
#[inline]
pub fn expr_use_x_select(e: &Expr) -> bool {
    (e.flags & EP_X_IS_SELECT) != 0
}

/// Vale `y.p_tab`.
#[inline]
pub fn expr_use_y_tab(e: &Expr) -> bool {
    (e.flags & (EP_WIN_FUNC | EP_SUBRTN)) == 0
}

/// Vale `y.p_win`.
#[inline]
pub fn expr_use_y_win(e: &Expr) -> bool {
    (e.flags & EP_WIN_FUNC) != 0
}

/// Vale `y.sub`.
#[inline]
pub fn expr_use_y_sub(e: &Expr) -> bool {
    (e.flags & EP_SUBRTN) != 0
}

// Sinalizadores de Expr.vvaFlags. O campo e as macros ExprSetVVAProperty e afins só existem sob
// SQLITE_DEBUG e somem; as constantes ficam por completude.
/// Não pode aplicar EXPRDUP_REDUCE neste Expr.
pub const EP_NO_REDUCE: u8 = 0x01;
/// Não alterar este nó Expr.
pub const EP_IMMUTABLE: u8 = 0x02;

// Tamanho em bytes de um Expr normal, de um com EP_Reduced e de um com EP_TokenOnly. No C vêm de
// sizeof e offsetof; aqui o Expr não é truncado (ver nota em `Expr`) e os três valores só servem
// de referência de contabilidade.
/// Tamanho completo.
pub const EXPR_FULLSIZE: usize = std::mem::size_of::<Expr>();
/// Características comuns.
pub const EXPR_REDUCEDSIZE: usize = std::mem::offset_of!(Expr, i_table);
/// Menos características.
pub const EXPR_TOKENONLYSIZE: usize = std::mem::offset_of!(Expr, p_left);

/// Sinalizador de `sqlite3ExprDup()`: usar nós Expr de tamanho reduzido.
pub const EXPRDUP_REDUCE: u32 = 0x0001;

/// Verdadeiro se a expressão é uma função com cláusula OVER() (função de janela).
#[inline]
pub fn is_window_func(p: &Expr) -> bool {
    expr_has_property(p, EP_WIN_FUNC)
        && p.y.p_win.as_ref().map_or(false, |w| w.borrow().e_frm_type != TK_FILTER as u8)
}

/// Lista de expressões. Cada expressão pode ter um nome opcional; a combinação expr/nome serve,
/// por exemplo, para "expr AS ID" após um SELECT ou "ID = expr" num UPDATE. Como argumento de
/// função o nome não é usado.
///
/// Para poupar memória, `z_e_name` tem vários usos conforme `e_e_name`: ENAME_NAME (1: o AS da
/// coluna do resultado, 2: COLUMN= de um UPDATE), ENAME_TAB (DB.TABLE.NAME usado para resolver
/// nomes de subconsultas) e ENAME_SPAN (texto da expressão original do resultado).
pub struct ExprList {
    /// Número de expressões da lista.
    pub n_expr: i32,
    /// Número de posições alocadas em `a`.
    pub n_alloc: i32,
    /// Uma entrada por expressão.
    pub a: Vec<ExprListItem>,
}

/// Campos de bit `fg` de `ExprList_item`.
#[derive(Clone, Copy, Default)]
pub struct ExprListItemFg {
    /// Máscara de sinalizadores KEYINFO_ORDER_*.
    pub sort_flags: u8,
    /// Significado de `z_e_name` (2 bits).
    pub e_e_name: u8,
    /// Indica quando o processamento terminou (1 bit).
    pub done: u8,
    /// A expressão constante é reutilizável (1 bit).
    pub reusable: u8,
    /// Adia a avaliação para depois da ordenação (1 bit).
    pub b_sorter_ref: u8,
    /// Verdadeiro se há "NULLS FIRST/LAST" explícito (1 bit).
    pub b_nulls: u8,
    /// Coluna usada numa subconsulta SF_NestedFrom (1 bit).
    pub b_used: u8,
    /// Termo da cláusula USING de um NestedFrom (1 bit).
    pub b_using_term: u8,
    /// Termo auxiliar num NestedFrom que "*" não deve expandir nas consultas pai (1 bit).
    pub b_no_expand: u8,
}

/// Union `u` de `ExprList_item`: `x` (usado por qualquer ExprList que não seja `Parse.pConsExpr`)
/// ou `iConstExprReg` (registro em que o valor da Expr fica em cache; só `Parse.pConstExpr`).
#[derive(Clone, Copy)]
pub enum ExprListItemU {
    /// `struct x`: `iOrderByCol` (coluna do resultado, para ORDER BY) e `iAlias` (índice em
    /// `Parse.aAlias[]` para zName).
    X { i_order_by_col: u16, i_alias: u16 },
    /// Registro em que o valor da Expr fica em cache.
    IConstExprReg(i32),
}

/// `struct ExprList_item`: uma entrada da lista.
pub struct ExprListItem {
    /// A árvore de análise desta expressão.
    pub p_expr: Option<Box<Expr>>,
    /// Token associado a esta expressão.
    pub z_e_name: Option<Vec<u8>>,
    /// Campos de bit `fg`.
    pub fg: ExprListItemFg,
    /// Union `u`.
    pub u: ExprListItemU,
}


// ---- part_008.rs ----

// Allowed values for Expr.a.eEName
pub const ENAME_NAME: u8 = 0;      // The AS clause of a result set
pub const ENAME_SPAN: u8 = 1;      // Complete text of the result set expression
pub const ENAME_TAB: u8 = 2;       // "DB.TABLE.NAME" for the result set
pub const ENAME_ROWID: u8 = 3;     // "DB.TABLE._rowid_" for * expansion of rowid

// An instance of this structure can hold a simple list of identifiers,
// such as the list "a,b,c" in the following statements:
//
//      INSERT INTO t(a,b,c) VALUES ...;
//      CREATE INDEX idx ON t(a,b,c);
//      CREATE TRIGGER trig BEFORE UPDATE ON t(a,b,c) ...;
//
// The IdList.a.idx field is used when the IdList represents the list of
// column names after a table name in an INSERT statement.  In the statement
//
//     INSERT INTO t(a,b,c) ...
//
// If "a" is the k-th column of table "t", then IdList.a[0].idx==k.
pub struct IdListItem {
    pub z_name: Vec<u8>,           // Name of the identifier
    pub u4_idx: i32,                // Index in some Table.aCol[] of a column named zName
}

pub struct IdList {
    pub n_id: i32,                  // Number of identifiers on the list
    pub e_u4: u8,                   // Which element of a.u4 is valid
    pub a: Vec<IdListItem>,         // One entry for each identifier
}

// Allowed values for IdList.eType, which determines which value of the a.u4
// is valid.
pub const EU4_NONE: u8 = 0;   // Does not use IdList.a.u4
pub const EU4_IDX: u8 = 1;    // Uses IdList.a.u4.idx
pub const EU4_EXPR: u8 = 2;   // Uses IdList.a.u4.pExpr, NOT CURRENTLY USED

// The SrcItem object represents a single term in the FROM clause of a query.
// The SrcList object is mostly an array of SrcItems.
//
// The jointype starts out showing the join type between the current table
// and the next table on the list.  The parser builds the list this way.
// But sqlite3SrcListShiftJoinType() later shifts the jointypes so that each
// jointype expresses the join between the table and the previous table.
//
// In the colUsed field, the high-order bit (bit 63) is set if the table
// contains more than 63 columns and the 64-th or later column is used.
//
// Union member validity:
//
//    u1.zIndexedBy      fg.isIndexedBy && !fg.isTabFunc
//    u1.pFuncArg        fg.isTabFunc   && !fg.isIndexedBy
//    u1.nRow            !fg.isTabFunc  && !fg.isIndexedBy
//
//    u2.pIBIndex        fg.isIndexedBy && !fg.isCte
//    u2.pCteUse         fg.isCte       && !fg.isIndexedBy

pub struct SrcItemFg {
    pub jointype: u8,               // Type of join between this table and the previous
    pub not_indexed: u8,            // True if there is a NOT INDEXED clause
    pub is_indexed_by: u8,          // True if there is an INDEXED BY clause
    pub is_tab_func: u8,            // True if table-valued-function syntax
    pub is_correlated: u8,          // True if sub-query is correlated
    pub is_materialized: u8,        // This is a materialized view
    pub via_coroutine: u8,          // Implemented as a co-routine
    pub is_recursive: u8,           // True for recursive reference in WITH
    pub from_ddl: u8,               // Comes from sqlite_schema
    pub is_cte: u8,                 // This is a CTE
    pub not_cte: u8,                // This item may not match a CTE
    pub is_using: u8,               // u3.pUsing is valid
    pub is_on: u8,                  // u3.pOn was once valid and non-NULL
    pub is_synth_using: u8,         // u3.pUsing is synthesized from NATURAL
    pub is_nested_from: u8,         // pSelect is a SF_NestedFrom subquery
    pub rowid_used: u8,             // The ROWID of this table is referenced
}

pub enum SrcItemU3 {
    On(Box<Expr>),                  // fg.isUsing==0: The ON clause of a join
    Using(Box<IdList>),             // fg.isUsing==1: The USING clause of a join
}

pub enum SrcItemU1 {
    IndexedBy(Vec<u8>),             // Identifier from "INDEXED BY <zIndex>" clause
    FuncArg(Box<ExprList>),         // Arguments to table-valued-function
    NRow(u32),                      // Number of rows in a VALUES clause
}

pub enum SrcItemU2 {
    IBIndex(IndexRef),          // Index structure corresponding to u1.zIndexedBy
    CteUse(CteUseRef),// CTE Usage info when fg.isCte is true
}

pub struct SrcItem {
    pub p_schema: Option<SchemaRef>,    // Schema to which this item is fixed
    pub z_database: Vec<u8>,            // Name of database holding this table
    pub z_name: Vec<u8>,                // Name of the table
    pub z_alias: Vec<u8>,               // The "B" part of a "A AS B" phrase.  zName is the "A"
    pub p_tab: Option<TableRef>,        // An SQL table corresponding to zName
    pub p_select: Option<Box<Select>>,  // A SELECT statement used in place of a table name
    pub addr_fill_sub: i32,             // Address of subroutine to manifest a subquery
    pub reg_return: i32,                // Register holding return address of addrFillSub
    pub reg_result: i32,                // Registers holding results of a co-routine
    pub fg: SrcItemFg,
    pub i_cursor: i32,                  // The VDBE cursor number used to access this table
    pub u3: SrcItemU3,
    pub col_used: u64,                  // Bit N set if column N used. Details above for N>62
    pub u1: SrcItemU1,
    pub u2: SrcItemU2,
}

// The OnOrUsing object represents either an ON clause or a USING clause.
// It can never be both at the same time, but it can be neither.
pub struct OnOrUsing {
    pub p_on: Option<Box<Expr>>,        // The ON clause of a join
    pub p_using: Option<Box<IdList>>,   // The USING clause of a join
}

// This object represents one or more tables that are the source of
// content for an SQL statement.  For example, a single SrcList object
// is used to hold the FROM clause of a SELECT statement.  SrcList also
// represents the target tables for DELETE, INSERT, and UPDATE statements.
pub struct SrcList {
    pub n_src: i32,                 // Number of tables or subqueries in the FROM clause
    pub n_alloc: u32,               // Number of entries allocated in a[] below
    pub a: Vec<SrcItem>,            // One entry for each identifier on the list
}

// Permitted values of the SrcList.a.jointype field
pub const JT_INNER: u8 = 0x01;    // Any kind of inner or cross join
pub const JT_CROSS: u8 = 0x02;    // Explicit use of the CROSS keyword
pub const JT_NATURAL: u8 = 0x04;  // True for a "natural" join
pub const JT_LEFT: u8 = 0x08;     // Left outer join
pub const JT_RIGHT: u8 = 0x10;    // Right outer join
pub const JT_OUTER: u8 = 0x20;    // The "OUTER" keyword is present
pub const JT_LTORJ: u8 = 0x40;    // One of the LEFT operands of a RIGHT JOIN. Mnemonic: Left Table Of Right Join
pub const JT_ERROR: u8 = 0x80;    // unknown or unsupported join type

// Flags appropriate for the wctrlFlags parameter of sqlite3WhereBegin()
// and the WhereInfo.wctrlFlags member.
//
// Value constraints (enforced via assert()):
//     WHERE_USE_LIMIT  == SF_FixedLimit
pub const WHERE_ORDERBY_NORMAL: u32 = 0x0000;   // No-op
pub const WHERE_ORDERBY_MIN: u32 = 0x0001;     // ORDER BY processing for min() func
pub const WHERE_ORDERBY_MAX: u32 = 0x0002;     // ORDER BY processing for max() func
pub const WHERE_ONEPASS_DESIRED: u32 = 0x0004; // Want to do one-pass UPDATE/DELETE
pub const WHERE_ONEPASS_MULTIROW: u32 = 0x0008; // ONEPASS is ok with multiple rows
pub const WHERE_DUPLICATES_OK: u32 = 0x0010;   // Ok to return a row more than once
pub const WHERE_OR_SUBCLAUSE: u32 = 0x0020;    // Processing a sub-WHERE as part of the OR optimization
pub const WHERE_GROUPBY: u32 = 0x0040;         // pOrderBy is really a GROUP BY
pub const WHERE_DISTINCTBY: u32 = 0x0080;      // pOrderby is really a DISTINCT clause
pub const WHERE_WANT_DISTINCT: u32 = 0x0100;   // All output needs to be distinct
pub const WHERE_SORTBYGROUP: u32 = 0x0200;     // Support sqlite3WhereIsSorted()
pub const WHERE_AGG_DISTINCT: u32 = 0x0400;    // Query is "SELECT agg(DISTINCT ...)"
pub const WHERE_ORDERBY_LIMIT: u32 = 0x0800;   // ORDERBY+LIMIT on the inner loop
pub const WHERE_RIGHT_JOIN: u32 = 0x1000;      // Processing a RIGHT JOIN
pub const WHERE_KEEP_ALL_JOINS: u32 = 0x2000;  // Do not do the omit-noop-join opt
pub const WHERE_USE_LIMIT: u32 = 0x4000;       // Use the LIMIT in cost estimates
// 0x8000 not currently used

// Allowed return values from sqlite3WhereIsDistinct()
pub const WHERE_DISTINCT_NOOP: u32 = 0;       // DISTINCT keyword not used
pub const WHERE_DISTINCT_UNIQUE: u32 = 1;     // No duplicates
pub const WHERE_DISTINCT_ORDERED: u32 = 2;    // All duplicates are adjacent
pub const WHERE_DISTINCT_UNORDERED: u32 = 3;  // Duplicates are scattered

// A NameContext defines a context in which to resolve table and column
// names.  The context consists of a list of tables (the pSrcList) field and
// a list of named expression (pEList).  The named expression list may
// be NULL.  The pSrc corresponds to the FROM clause of a SELECT or
// to the table being operated on by INSERT, UPDATE, or DELETE.  The
// pEList corresponds to the result set of a SELECT and is NULL for
// other statements.
//
// NameContexts can be nested.  When resolving names, the inner-most
// context is searched first.  If no match is found, the next outer
// context is checked.  If there is still no match, the next context
// is checked.  This process continues until either a match is found
// or all contexts are check.  When a match is found, the nRef member of
// the context containing the match is incremented.
//
// Each subquery gets a new NameContext.  The pNext field points to the
// NameContext in the parent query.  Thus the process of scanning the
// NameContext list corresponds to searching through successively outer
// subqueries looking for a match.

pub enum NameContextUNC {
    EList(Box<ExprList>),               // Optional list of result-set columns
    AggInfo(Box<AggInfo>),              // Information about aggregates at this level
    Upsert(Box<Upsert>),                // ON CONFLICT clause information from an upsert
    IBaseReg(i32),                      // For TK_REGISTER when parsing RETURNING
}

pub struct NameContext {
    pub p_parse: Option<ParseRef>,      // The parser (ponteiro de volta, não dono)
    pub p_src_list: Option<Box<SrcList>>, // One or more tables used to resolve names
    pub u_nc: NameContextUNC,
    pub p_next: Option<Box<NameContext>>, // Next outer name context.  NULL for outermost
    pub n_ref: i32,                     // Number of names resolved by this context
    pub n_nc_err: i32,                  // Number of errors encountered while resolving names
    pub nc_flags: i32,                  // Zero or more NC_* flags defined below
    pub n_nested_select: u32,           // Number of nested selects using this NC
    pub p_win_select: Option<Box<Select>>, // SELECT statement for any window functions
}

// Allowed values for the NameContext, ncFlags field.
//
// Value constraints (all checked via assert()):
//    NC_HasAgg    == SF_HasAgg       == EP_Agg
//    NC_MinMaxAgg == SF_MinMaxAgg    == SQLITE_FUNC_MINMAX
//    NC_OrderAgg  == SF_OrderByReqd  == SQLITE_FUNC_ANYORDER
//    NC_HasWin    == EP_Win

pub const NC_ALLOWAGG: i32 = 0x000001;  // Aggregate functions are allowed here
pub const NC_PARTIDX: i32 = 0x000002;   // True if resolving a partial index WHERE
pub const NC_ISCHECK: i32 = 0x000004;   // True if resolving a CHECK constraint
pub const NC_GENCOL: i32 = 0x000008;    // True for a GENERATED ALWAYS AS clause
pub const NC_HASAGG: i32 = 0x000010;    // One or more aggregate functions seen
pub const NC_IDXEXPR: i32 = 0x000020;   // True if resolving columns of CREATE INDEX
pub const NC_SELFREF: i32 = 0x00002e;   // Combo: PartIdx, isCheck, GenCol, and IdxExpr
pub const NC_SUBQUERY: i32 = 0x000040;   // A subquery has been seen
pub const NC_UELIST: i32 = 0x000080;    // True if uNC.pEList is used
pub const NC_UAGGINFO: i32 = 0x000100; // True if uNC.pAggInfo is used
pub const NC_UUPSERT: i32 = 0x000200;   // True if uNC.pUpsert is used
pub const NC_UBASEREG: i32 = 0x000400; // True if uNC.iBaseReg is used
pub const NC_MINMAXAGG: i32 = 0x001000; // min/max aggregates seen.  See note above
pub const NC_COMPLEX: i32 = 0x002000;    // True if a function or subquery seen
pub const NC_ALLOWWIN: i32 = 0x004000;  // Window functions are allowed here
pub const NC_HASWIN: i32 = 0x008000;    // One or more window functions seen
pub const NC_ISDDL: i32 = 0x010000;     // Resolving names in a CREATE statement
pub const NC_INAGGFUNC: i32 = 0x020000; // True if analyzing arguments to an agg func
pub const NC_FROMDDL: i32 = 0x040000;   // SQL text comes from sqlite_schema
pub const NC_NOSELECT: i32 = 0x080000;  // Do not descend into sub-selects
pub const NC_WHERE: i32 = 0x100000;      // Processing WHERE clause of a SELECT
pub const NC_ORDERAGG: i32 = 0x8000000; // Has an aggregate other than count/min/max

// An instance of the following object describes a single ON CONFLICT
// clause in an upsert.
//
// The pUpsertTarget field is only set if the ON CONFLICT clause includes
// conflict-target clause.  (In "ON CONFLICT(a,b)" the "(a,b)" is the
// conflict-target clause.)  The pUpsertTargetWhere is the optional
// WHERE clause used to identify partial unique indexes.
//
// pUpsertSet is the list of column,expr terms of the UPDATE statement.
// The pUpsertSet field is NULL for a ON CONFLICT DO NOTHING.  The
// pUpsertWhere is the WHERE clause for the UPDATE and is NULL if the
// WHERE clause is omitted.
pub struct Upsert {
    pub p_upsert_target: Option<Box<ExprList>>, // Optional description of conflict target
    pub p_upsert_target_where: Option<Box<Expr>>, // WHERE clause for partial index targets
    pub p_upsert_set: Option<Box<ExprList>>, // The SET clause from an ON CONFLICT UPDATE
    pub p_upsert_where: Option<Box<Expr>>, // WHERE clause for the ON CONFLICT UPDATE
    pub p_next_upsert: Option<Box<Upsert>>, // Next ON CONFLICT clause in the list
    pub is_do_update: u8,           // True for DO UPDATE.  False for DO NOTHING
    pub is_dup: u8,                 // True if 2nd or later with same pUpsertIdx
    // Above this point is the parse tree for the ON CONFLICT clauses.
    // The next group of fields stores intermediate data.
    pub p_to_free: Option<Vec<u8>>, // Free memory when deleting the Upsert object
    // All fields above are owned by the Upsert object and must be freed
    // when the Upsert is destroyed.  The fields below are used to transfer
    // information from the INSERT processing down into the UPDATE processing
    // while generating code.  The fields below are owned by the INSERT
    // statement and will be freed by INSERT processing.
    pub p_upsert_idx: Option<IndexRef>, // UNIQUE constraint specified by pUpsertTarget
    pub p_upsert_src: Option<Box<SrcList>>, // Table to be updated
    pub reg_data: i32,              // First register holding array of VALUES
    pub i_data_cur: i32,            // Index of the data cursor
    pub i_idx_cur: i32,             // Index of the first index cursor
}

// An instance of the following structure contains all information
// needed to generate code for a single SELECT statement.
//
// See the header comment on the computeLimitRegisters() routine for a
// detailed description of the meaning of the iLimit and iOffset fields.
//
// addrOpenEphm[] entries contain the address of OP_OpenEphemeral opcodes.
// These addresses must be stored so that we can go back and fill in
// the P4_KEYINFO and P2 parameters later.  Neither the KeyInfo nor
// the number of columns in P2 can be computed at the same time
// as the OP_OpenEphm instruction is coded because not
// enough information about the compound query is known at that point.
// The KeyInfo for addrOpenTran[0] and [1] contains collating sequences
// for the result set.  The KeyInfo for addrOpenEphm[2] contains collating
// sequences for the ORDER BY clause.
pub struct Select {
    pub op: u8,                     // One of: TK_UNION TK_ALL TK_INTERSECT TK_EXCEPT
    pub n_select_row: i16,          // Estimated number of result rows (LogEst)
    pub sel_flags: u32,             // Various SF_* values
    pub i_limit: i32,               // Memory registers holding LIMIT counter
    pub i_offset: i32,              // Memory registers holding OFFSET counter
    pub sel_id: u32,                // Unique identifier number for this SELECT
    pub addr_open_ephm: [i32; 2],   // OP_OpenEphem opcodes related to this select
    pub p_elist: Option<Box<ExprList>>, // The fields of the result
    pub p_src: Option<Box<SrcList>>, // The FROM clause
    pub p_where: Option<Box<Expr>>, // The WHERE clause
    pub p_group_by: Option<Box<ExprList>>, // The GROUP BY clause
    pub p_having: Option<Box<Expr>>, // The HAVING clause
    pub p_order_by: Option<Box<ExprList>>, // The ORDER BY clause
    pub p_prior: Option<Box<Select>>, // Prior select in a compound select statement
    pub p_next: Option<Box<Select>>, // Next select to the left in a compound
    pub p_limit: Option<Box<Expr>>, // LIMIT expression. NULL means not used.
    pub p_with: Option<WithRef>,    // WITH clause attached to this select. Or NULL.
    pub p_win: Option<Box<Window>>, // List of window functions
    pub p_win_defn: Option<Box<Window>>, // List of named window definitions
}

// Allowed values for Select.selFlags.  The "SF" prefix stands for
// "Select Flag".
//
// Value constraints (all checked via assert())
//     SF_HasAgg      == NC_HasAgg
//     SF_MinMaxAgg   == NC_MinMaxAgg     == SQLITE_FUNC_MINMAX
//     SF_OrderByReqd == NC_OrderAgg      == SQLITE_FUNC_ANYORDER
//     SF_FixedLimit  == WHERE_USE_LIMIT

pub const SF_DISTINCT: u32 = 0x0000001;   // Output should be DISTINCT
pub const SF_ALL: u32 = 0x0000002;        // Includes the ALL keyword
pub const SF_RESOLVED: u32 = 0x0000004;   // Identifiers have been resolved
pub const SF_AGGREGATE: u32 = 0x0000008;  // Contains agg functions or a GROUP BY
pub const SF_HASAGG: u32 = 0x0000010;    // Contains aggregate functions
pub const SF_USESEPHEMERAL: u32 = 0x0000020; // Uses the OpenEphemeral opcode
pub const SF_EXPANDED: u32 = 0x0000040;   // sqlite3SelectExpand() called on this
pub const SF_HASTYPEINFO: u32 = 0x0000080; // FROM subqueries have Table metadata
pub const SF_COMPOUND: u32 = 0x0000100;   // Part of a compound query
pub const SF_VALUES: u32 = 0x0000200;     // Synthesized from VALUES clause
pub const SF_MULTIVALUE: u32 = 0x0000400; // Single VALUES term with multiple rows
pub const SF_NESTEDFROM: u32 = 0x0000800; // Part of a parenthesized FROM clause
pub const SF_MINMAXAGG: u32 = 0x0001000; // Aggregate containing min() or max()
pub const SF_RECURSIVE: u32 = 0x0002000;  // The recursive part of a recursive CTE
pub const SF_FIXEDLIMIT: u32 = 0x0004000; // nSelectRow set by a constant LIMIT
pub const SF_MAYBECONVERT: u32 = 0x0008000; // Need convertCompoundSelectToSubquery()
pub const SF_CONVERTED: u32 = 0x0010000;  // By convertCompoundSelectToSubquery()
pub const SF_INCLUDEHIDDEN: u32 = 0x0020000; // Include hidden columns in output
pub const SF_COMPLEXRESULT: u32 = 0x0040000; // Result contains subquery or function
pub const SF_WHEREBEGIN: u32 = 0x0080000; // Really a WhereBegin() call.  Debug Only
pub const SF_WINREWRITE: u32 = 0x0100000; // Window function rewrite accomplished
pub const SF_VIEW: u32 = 0x0200000;       // SELECT statement is a view
pub const SF_NOOPORDERBY: u32 = 0x0400000; // ORDER BY is ignored for this query
pub const SF_UFSRCCHECK: u32 = 0x0800000; // Check pSrc as required by UPDATE...FROM
pub const SF_PUSHDOWN: u32 = 0x1000000;  // Modified by WHERE-clause push-down opt
pub const SF_MULTIPART: u32 = 0x2000000; // Has multiple incompatible PARTITIONs
pub const SF_COPYCTE: u32 = 0x4000000;   // SELECT statement is a copy of a CTE
pub const SF_ORDERBYREQD: u32 = 0x8000000; // The ORDER BY clause may not be omitted
pub const SF_UPDATEFROM: u32 = 0x10000000; // Query originates with UPDATE FROM
pub const SF_CORRELATED: u32 = 0x20000000; // True if references the outer context

// True if S exists and has SF_NestedFrom
#[inline]
pub fn is_nested_from(s: Option<&Select>) -> bool {
    match s {
        Some(select) => (select.sel_flags & SF_NESTEDFROM) != 0,
        None => false,
    }
}

// The results of a SELECT can be distributed in several ways, as defined
// by one of the following macros.  The "SRT" prefix means "SELECT Result
// Type".
//
// SRT_Union       Store results as a key in a temporary index
//                 identified by pDest,iSDParm.
//
// SRT_Except      Remove results from the temporary index pDest,iSDParm.
//
// SRT_Exists      Store a 1 in memory cell pDest,iSDParm if the result
//                 set is not empty.
//
// SRT_Discard     Throw the results away.  This is used by SELECT
//                 statements within triggers whose only purpose is
//                 the side-effects of functions.
//
// SRT_Output      Generate a row of output (using the OP_ResultRow
//                 opcode) for each row in the result set.
//
// SRT_Mem         Only valid if the result is a single column.
//                 Store the first column of the first result row
//                 in register pDest,iSDParm then abandon the rest
//                 of the query.  This destination implies "LIMIT 1".
//
// SRT_Set         The result must be a single column.  Store each
//                 row of result as the key in table pDest,iSDParm.
//                 Apply the affinity pDest,affSdst before storing
//                 results.  Used to implement "IN (SELECT ...)".
//
// SRT_EphemTab    Create an temporary table pDest,iSDParm and store
//                 the result there. The cursor is left open after
//                 returning.  This is like SRT_Table except that
//                 this destination uses OP_OpenEphemeral to create
//                 the table first.
//
// SRT_Coroutine   Generate a co-routine that returns a new row of
//                 results each time it is invoked.  The entry point
//                 of the co-routine is stored in register pDest,iSDParm
//                 and the result row is stored in pDest,nDest registers
//                 starting with pDest,iSdst.
//
// SRT_Table       Store results in temporary table pDest,iSDParm.
// SRT_Fifo        This is like SRT_EphemTab except that the table
//                 is assumed to already be open.  SRT_Fifo has
//                 the additional property of being able to ignore
//                 the ORDER BY clause.
//
// SRT_DistFifo    Store results in a temporary table pDest,iSDParm.
//                 But also use temporary table pDest,iSDParm+1 as
//                 a record of all prior results and ignore any duplicate
//                 rows.  Name means: "Distinct Fifo".
//
// SRT_Queue       Store results in priority queue pDest,iSDParm (really
//                 an index).  Append a sequence number so that all entries
//                 are distinct.
//
// SRT_DistQueue   Store results in priority queue pDest,iSDParm only if
//                 the same record has never been stored before.  The
//                 index at pDest,iSDParm+1 hold all prior stores.
//
// SRT_Upfrom      Store results in the temporary table already opened by
//                 pDest,iSDParm. If (pDest,iSDParm<0), then the temp
//                 table is an intkey table in this case the first
//                 column returned by the SELECT is used as the integer
//                 key. If (pDest,iSDParm>0), then the table is an index
//                 table. (pDest,iSDParm) is the number of key columns in
//                 each index record in this case.

pub const SRT_UNION: u8 = 1;        // Store result as keys in an index
pub const SRT_EXCEPT: u8 = 2;       // Remove result from a UNION index
pub const SRT_EXISTS: u8 = 3;       // Store 1 if the result is not empty
pub const SRT_DISCARD: u8 = 4;      // Do not save the results anywhere
pub const SRT_DISTFIFO: u8 = 5;    // Like SRT_Fifo, but unique results only
pub const SRT_DISTQUEUE: u8 = 6;   // Like SRT_Queue, but unique results only

// The DISTINCT clause is ignored for all of the above. Note that
// IgnorableDistinct() implies IgnorableOrderby()

#[inline]
pub fn ignorable_distinct(x: &SelectDest) -> bool {
    x.e_dest <= SRT_DISTQUEUE
}

pub const SRT_QUEUE: u8 = 7;        // Store result in an queue
pub const SRT_FIFO: u8 = 8;         // Store result as data with an automatic rowid

// The ORDER BY clause is ignored for all of the above

#[inline]
pub fn ignorable_orderby(x: &SelectDest) -> bool {
    x.e_dest <= SRT_FIFO
}

pub const SRT_OUTPUT: u8 = 9;       // Output each row of result
pub const SRT_MEM: u8 = 10;         // Store result in a memory cell
pub const SRT_SET: u8 = 11;         // Store results as keys in an index
pub const SRT_EPHEMTAB: u8 = 12;   // Create transient tab and store like SRT_Table
pub const SRT_COROUTINE: u8 = 13;   // Generate a single row of result
pub const SRT_TABLE: u8 = 14;       // Store result as data with an automatic rowid
pub const SRT_UPFROM: u8 = 15;      // Store result as data with rowid

// An instance of this object describes where to put of the results of
// a SELECT statement.
pub struct SelectDest {
    pub e_dest: u8,                 // How to dispose of the results.  One of SRT_* above.
    pub i_sdparm: i32,              // A parameter used by the eDest disposal method
    pub i_sdparm2: i32,             // A second parameter for the eDest disposal method
    pub i_sdst: i32,                // Base register where results are written
    pub n_sdst: i32,                // Number of registers allocated
    pub z_aff_sdst: Vec<u8>,        // Affinity used for SRT_Set
    pub p_order_by: Option<Box<ExprList>>, // Key columns for SRT_Queue and SRT_DistQueue
}


// ---- part_009.rs ----

/// Durante a geração de código de instruções que inserem em tabelas AUTOINCREMENT,
/// esta informação é anexada ao ponteiro `Table.u.autoInc.p` de cada tabela com
/// autoincrement para registrar dados auxiliares de que o gerador de código precisa.
/// A informação é mantida por tabela porque inserções podem ocorrer dentro de triggers.
/// Triggers normalmente não coordenam suas atividades, mas é preciso coordenar o
/// carregamento e o salvamento da informação de autoincrement.
pub struct AutoincInfo {
    pub p_next: Option<Box<AutoincInfo>>,  // Próximo bloco de informação na lista de todos
    pub p_tab: TableRef,                   // Tabela à qual este bloco se refere
    pub i_db: i32,                         // Índice em sqlite3.aDb[] do banco que contém p_tab
    pub reg_ctr: i32,                      // Registro de memória com o contador de rowid
}

/// Pelo menos uma instância desta estrutura é criada para cada trigger que pode
/// disparar durante a análise de um INSERT, UPDATE ou DELETE. Todos esses objetos
/// ficam na lista ligada que começa em `Parse.pTriggerPrg` e são apagados quando a
/// compilação da instrução termina.
///
/// Um sub-programa Vdbe que implementa o corpo e a cláusula WHEN de
/// `TriggerPrg.pTrigger`, assumindo a cláusula ON CONFLICT padrão `TriggerPrg.orconf`,
/// fica em `TriggerPrg.pProgram`. A lista `Parse.pTriggerPrg` nunca contém duas
/// entradas com os mesmos valores de `pTrigger` e `orconf`.
///
/// `TriggerPrg.aColmask[0]` é a máscara das colunas old.* acessadas (ou 0 para
/// triggers disparados por INSERT). `TriggerPrg.aColmask[1]` é a máscara das colunas
/// new.* usadas pelo programa.
pub struct TriggerPrg {
    pub p_trigger: TriggerRef,             // Trigger a partir do qual este programa foi codificado
    pub p_next: Option<Box<TriggerPrg>>,   // Próxima entrada da lista Parse.pTriggerPrg
    pub p_program: SubProgramRef,          // Programa que implementa p_trigger/orconf
    pub orconf: i32,                       // Política padrão de ON CONFLICT
    pub a_colmask: [u32; 2],               // Máscaras das colunas old.*, new.* acessadas
}

/// O tipo yDbMask: máscara de bits de todos os bancos anexados. Com
/// SQLITE_MAX_ATTACHED=10 (padrão, não maior que 30) é um `unsigned int`.
#[allow(non_camel_case_types)]
pub type yDbMask = u32;

#[inline]
pub fn db_mask_test(m: yDbMask, i: u32) -> bool {
    (m & (1u32 << i)) != 0
}

#[inline]
pub fn db_mask_zero(m: &mut yDbMask) {
    *m = 0;
}

#[inline]
pub fn db_mask_set(m: &mut yDbMask, i: u32) {
    *m |= 1u32 << i;
}

#[inline]
pub fn db_mask_all_zero(m: yDbMask) -> bool {
    m == 0
}

#[inline]
pub fn db_mask_non_zero(m: yDbMask) -> bool {
    m != 0
}

/// Para cada índice X que tem como argumento uma expressão ou o nome de uma coluna
/// gerada virtual, e que está no escopo de modo que o valor da expressão pode ser
/// lido direto do índice, existe uma instância deste objeto na lista `Parse.pIdxExpr`.
///
/// Durante a geração de código, ao gerar código para avaliar expressões, essa lista
/// é consultada e, se uma expressão correspondente é achada, o valor é lido do
/// índice em vez de ser recalculado.
pub struct IndexedExpr {
    pub p_expr: Box<Expr>,                    // A expressão contida no índice
    pub i_data_cur: i32,                      // Cursor de dados associado ao índice
    pub i_idx_cur: i32,                       // O cursor do índice
    pub i_idx_col: i32,                       // Coluna do índice que contém o valor de p_expr
    pub b_maybe_null_row: u8,                 // Verdadeiro se precisa de um OP_IfNullRow
    pub aff: u8,                              // Afinidade da expressão p_expr
    pub p_ie_next: Option<Box<IndexedExpr>>,  // Próxima da lista de todas as expressões indexadas
}

/// Uma instância de ParseCleanup especifica uma operação que deve ser feita depois
/// da análise para liberar recursos obtidos durante a análise e que não são mais
/// necessários. O par (`pPtr`, `xCleanup`) do C vira uma closure que já captura o objeto.
pub struct ParseCleanup {
    pub p_next: Option<Box<ParseCleanup>>,                  // Próxima tarefa de limpeza
    pub x_cleanup: Option<Box<dyn FnOnce(&Sqlite3Ref)>>,    // Rotina de liberação (com o objeto capturado)
}

/// Contexto do analisador SQL. Uma cópia desta estrutura é passada pelo analisador e
/// por todas as rotinas de ação para carregar a informação global de toda a análise.
///
/// A estrutura se divide em duas partes. Quando o analisador e o gerador de código
/// se chamam recursivamente, a primeira parte é constante, mas a segunda é zerada no
/// início e no fim de cada recursão.
///
/// `nTableLock` e `aTableLock` só são usados se o recurso de cache compartilhado
/// está habilitado (se `sqlite3Tsd()->useSharedData` é verdadeiro). Guardam o
/// conjunto de travas de tabela exigidas pela instrução em compilação.
pub struct Parse {
    pub db: Weak<RefCell<Sqlite3>>,        // A estrutura principal do banco
    pub z_err_msg: Option<Vec<u8>>,        // Uma mensagem de erro
    pub p_vdbe: Option<VdbeRef>,           // Máquina que executa o bytecode do banco
    pub rc: i32,                           // Código de retorno da execução
    pub col_names_set: u8,                 // Verdadeiro depois que OP_ColumnName foi emitido para p_vdbe
    pub check_schema: u8,                  // Causa verificação do cookie do schema após um erro
    pub nested: u8,                        // Número de chamadas aninhadas ao analisador/gerador
    pub n_temp_reg: u8,                    // Número de registros temporários em a_temp_reg[]
    pub is_multi_write: u8,                // Verdadeiro se a instrução pode modificar/inserir várias linhas
    pub may_abort: u8,                     // Verdadeiro se a instrução pode lançar uma exceção ABORT
    pub has_compound: u8,                  // Precisa chamar convertCompoundSelectToSubquery()
    pub ok_const_factor: u8,               // Pode fatorar constantes
    pub disable_lookaside: u8,             // Número de vezes que o lookaside foi desabilitado
    pub prep_flags: u8,                    // Flags SQLITE_PREPARE_*
    pub within_rj_subrtn: u8,              // Nível de aninhamento das sub-rotinas do corpo de RIGHT JOIN
    pub b_has_with: u8,                    // Verdadeiro se a instrução contém WITH
    pub n_range_reg: i32,                  // Tamanho do bloco de registros temporários
    pub i_range_reg: i32,                  // Primeiro registro do bloco de registros temporários
    pub n_err: i32,                        // Número de erros vistos
    pub n_tab: i32,                        // Número de cursores VDBE já alocados
    pub n_mem: i32,                        // Número de células de memória usadas até agora
    pub sz_op_alloc: i32,                  // Bytes de memória alocados para Vdbe.aOp[]
    pub i_self_tab: i32,                   // Tabela associada a um índice em expressão, ou o negativo
                                           // do registro base durante a avaliação de check-constraint
    pub n_label: i32,                      // O *negativo* do número de rótulos usados
    pub n_label_alloc: i32,                // Número de posições em a_label
    pub a_label: Vec<i32>,                 // Espaço para guardar os rótulos
    pub p_const_expr: Option<Box<ExprList>>, // Expressões constantes
    pub p_idx_epr: Option<Box<IndexedExpr>>, // Lista de expressões usadas por índices ativos
    pub p_idx_part_expr: Option<Box<IndexedExpr>>, // Expressões restritas por cláusulas WHERE de índices
    pub constraint_name: Token,            // Nome da restrição em análise no momento
    pub write_mask: yDbMask,               // Inicia transação de escrita nestes bancos
    pub cookie_mask: yDbMask,              // Máscara de bits dos bancos com schema verificado
    pub reg_rowid: i32,                    // Registro com o rowid da entrada de CREATE TABLE
    pub reg_root: i32,                     // Registro com o número da página raiz de objetos novos
    pub n_max_arg: i32,                    // Máximo de args passados a função de usuário por sub-programa
    pub n_select: i32,                     // Número de SELECTs. Contador para Select.selId
    pub n_progress_steps: u32,             // Passos de xProgress dados durante sqlite3_prepare()
    pub n_table_lock: i32,                 // Número de travas em a_table_lock
    pub a_table_lock: Vec<TableLock>,      // Travas de tabela exigidas no modo de cache compartilhado
    pub p_ainc: Option<Box<AutoincInfo>>,  // Informação sobre contadores AUTOINCREMENT
    pub p_toplevel: Option<Weak<RefCell<Parse>>>, // Parse do programa principal (ou None)
    pub p_trigger_tab: Option<TableRef>,   // Tabela para a qual os triggers estão sendo codificados
    pub p_trigger_prg: Option<Box<TriggerPrg>>, // Lista ligada de triggers codificados
    pub p_cleanup: Option<Box<ParseCleanup>>,   // Operações de limpeza a rodar após a análise
    // union u1 do C: os dois membros nunca são usados ao mesmo tempo
    pub u1_addr_cr_tab: i32,               // Endereço do OP_CreateBtree em CREATE TABLE
    pub u1_p_returning: Option<Box<Returning>>, // A cláusula RETURNING
    pub oldmask: u32,                      // Máscara das colunas old.* referenciadas
    pub newmask: u32,                      // Máscara das colunas new.* referenciadas
    pub n_query_loop: LogEst,              // Estimativa de iterações de uma consulta (10*log2(N))
    pub e_trigger_op: u8,                  // TK_UPDATE, TK_INSERT ou TK_DELETE
    pub b_returning: u8,                   // Codificando um trigger RETURNING
    pub e_orconf: u8,                      // Política ON CONFLICT padrão para passos de trigger
    pub disable_triggers: u8,              // Verdadeiro para desabilitar triggers

    // Os campos acima devem ser inicializados com zero. Os que seguem, até o começo
    // da seção recursiva, não precisam de inicialização pois são definidos antes de
    // serem usados. A fronteira é dada por offsetof(Parse,aTempReg).

    pub a_temp_reg: [i32; 8],              // Área de retenção de registros temporários
    pub p_outer_parse: Option<Weak<RefCell<Parse>>>, // Parse externo quando aninhado
    pub s_name_token: Token,               // Token com o nome não qualificado do objeto de schema

    // Acima é constante entre recursões. Abaixo é zerado antes e depois de cada
    // recursão. A fronteira é dada por offsetof(Parse,sLastToken), então o campo
    // s_last_token deve ser o primeiro da região recursiva.

    pub s_last_token: Token,               // O último token analisado
    pub n_var: ynVar,                      // Número de variáveis '?' vistas no SQL até agora
    pub i_pk_sort_order: u8,               // ASC ou DESC para INTEGER PRIMARY KEY
    pub explain: u8,                       // Verdadeiro se o flag EXPLAIN foi achado na consulta
    pub e_parse_mode: u8,                  // Constante PARSE_MODE_XXX
    pub n_vtab_lock: i32,                  // Número de tabelas virtuais a travar
    pub n_height: i32,                     // Altura da árvore de expressão do sub-select atual
    pub addr_explain: i32,                 // Endereço do OP_Explain atual
    pub p_vlist: Vec<i32>,                 // Mapeamento entre nomes de variáveis e números (VList)
    pub p_reprepare: Option<VdbeRef>,      // VM sendo reparada (sqlite3Reprepare())
    pub z_tail: usize,                     // Posição no SQL de todo o texto após o último ponto e vírgula analisado
    pub p_new_table: Option<TableRef>,     // Tabela em construção por CREATE TABLE
    pub p_new_index: Option<IndexRef>,     // Índice em construção por CREATE INDEX.
                                           // Também guarda restrições UNIQUE redundantes
                                           // durante um RENAME COLUMN
    pub p_new_trigger: Option<TriggerRef>, // Trigger em construção por CREATE TRIGGER
    pub z_auth_context: Option<Vec<u8>>,   // O 6o parâmetro dos callbacks db->xAuth
    pub s_arg: Token,                      // Texto completo de um argumento de módulo
    pub ap_vtab_lock: Vec<TableRef>,       // Tabelas virtuais que precisam de trava
    pub p_with: Option<WithRef>,           // Cláusula WITH atual, ou None
    pub p_rename: Option<Box<RenameToken>>, // Tokens sujeitos a renomeação por ALTER TABLE
}

// Valores permitidos para Parse.eParseMode
pub const PARSE_MODE_NORMAL: u8 = 0;
pub const PARSE_MODE_DECLARE_VTAB: u8 = 1;
pub const PARSE_MODE_RENAME: u8 = 2;
pub const PARSE_MODE_UNMAP: u8 = 3;

// Os macros PARSE_HDR, PARSE_HDR_SZ, PARSE_RECURSE_SZ, PARSE_TAIL_SZ e PARSE_TAIL
// dependem de offsetof sobre a estrutura em C. Em Rust a parte recursiva é zerada
// campo a campo pelo código que os usa (reset de s_last_token em diante).

/// Verdadeiro se estamos dentro de uma chamada a sqlite3_declare_vtab().
#[inline]
pub fn in_declare_vtab(p_parse: &Parse) -> bool {
    p_parse.e_parse_mode == PARSE_MODE_DECLARE_VTAB
}

/// Verdadeiro se estamos renomeando um objeto.
#[inline]
pub fn in_rename_object(p_parse: &Parse) -> bool {
    p_parse.e_parse_mode >= PARSE_MODE_RENAME
}

/// Verdadeiro se estamos em modo especial de análise.
#[inline]
pub fn in_special_parse(p_parse: &Parse) -> bool {
    p_parse.e_parse_mode != PARSE_MODE_NORMAL
}

/// Pode ser declarada na pilha e usada para salvar o valor de `Parse.zAuthContext`
/// para restaurá-lo depois.
pub struct AuthContext {
    pub z_auth_context: Option<Vec<u8>>,   // Guarde aqui o Parse.zAuthContext salvo
    pub p_parse: Weak<RefCell<Parse>>,     // A estrutura Parse
}

// Flags de bits para o valor P5 de vários opcodes.
//
// Restrições de valor (verificadas por assert() no C):
//    OPFLAG_LENGTHARG    == SQLITE_FUNC_LENGTH
//    OPFLAG_TYPEOFARG    == SQLITE_FUNC_TYPEOF
//    OPFLAG_BULKCSR      == BTREE_BULKLOAD
//    OPFLAG_SEEKEQ       == BTREE_SEEK_EQ
//    OPFLAG_FORDELETE    == BTREE_FORDELETE
//    OPFLAG_SAVEPOSITION == BTREE_SAVEPOSITION
//    OPFLAG_AUXDELETE    == BTREE_AUXDELETE
pub const OPFLAG_NCHANGE: u8 = 0x01;       // OP_Insert: atualiza db->nChange. Também usado em P2 (não P5) de OP_Delete
pub const OPFLAG_NOCHNG: u8 = 0x01;        // OP_VColumn nochange para UPDATE
pub const OPFLAG_EPHEM: u8 = 0x01;         // OP_Column: saída efêmera é aceita
pub const OPFLAG_LASTROWID: u8 = 0x20;     // Atualiza db->lastRowid
pub const OPFLAG_ISUPDATE: u8 = 0x04;      // Este OP_Insert é um UPDATE do SQL
pub const OPFLAG_APPEND: u8 = 0x08;        // Provavelmente é um append
pub const OPFLAG_USESEEKRESULT: u8 = 0x10; // Tenta evitar um seek em BtreeInsert()
pub const OPFLAG_ISNOOP: u8 = 0x40;        // OP_Delete só roda o pre-update-hook
pub const OPFLAG_LENGTHARG: u8 = 0x40;     // OP_Column usado só para length()
pub const OPFLAG_TYPEOFARG: u8 = 0x80;     // OP_Column usado só para typeof()
pub const OPFLAG_BYTELENARG: u8 = 0xc0;    // OP_Column só para octet_length()
pub const OPFLAG_BULKCSR: u8 = 0x01;       // OP_Open** abre cursor de carga em bloco
pub const OPFLAG_SEEKEQ: u8 = 0x02;        // Cursor de OP_Open** só usa seek de igualdade
pub const OPFLAG_FORDELETE: u8 = 0x08;     // OP_Open deve usar BTREE_FORDELETE
pub const OPFLAG_P2ISREG: u8 = 0x10;       // P2 de OP_Open** é um número de registro
pub const OPFLAG_PERMUTE: u8 = 0x01;       // OP_Compare: usa a permutação
pub const OPFLAG_SAVEPOSITION: u8 = 0x02;  // OP_Delete/Insert: salva a posição do cursor
pub const OPFLAG_AUXDELETE: u8 = 0x04;     // OP_Delete: índice em uma operação DELETE
pub const OPFLAG_NOCHNG_MAGIC: u8 = 0x6d;  // OP_MakeRecord: serialtype 10 é aceito
pub const OPFLAG_PREFORMAT: u8 = 0x80;     // OP_Insert usa célula pré-formatada

/// Cada trigger presente no schema do banco é guardado como uma instância de Trigger.
///
/// Ponteiros para instâncias de Trigger são guardados de duas formas.
/// 1. Na tabela hash "trigHash" (parte do sqlite3 que representa o banco), o que
///    permite achar Triggers pelo nome.
/// 2. Todos os triggers associados a uma única tabela formam uma lista ligada, pelo
///    membro `pNext`. O primeiro da lista fica no membro `pTrigger` da Table.
///
/// O membro `step_list` aponta para o primeiro elemento de uma lista ligada com as
/// instruções SQL que formam o programa do trigger.
pub struct Trigger {
    pub z_name: Vec<u8>,                   // O nome do trigger
    pub table: Vec<u8>,                    // A tabela ou visão à qual o trigger se aplica
    pub op: u8,                            // Um de TK_DELETE, TK_UPDATE, TK_INSERT
    pub tr_tm: u8,                         // Um de TRIGGER_BEFORE, TRIGGER_AFTER
    pub b_returning: u8,                   // Este trigger implementa uma cláusula RETURNING
    pub p_when: Option<Box<Expr>>,         // A cláusula WHEN da expressão (pode ser None)
    pub p_columns: Option<Box<IdList>>,    // Em um trigger UPDATE OF <column-list>, a lista fica aqui
    pub p_schema: Weak<RefCell<Schema>>,   // Schema que contém o trigger
    pub p_tab_schema: Weak<RefCell<Schema>>, // Schema que contém a tabela
    pub step_list: Option<Box<TriggerStep>>, // Lista ligada dos passos do programa do trigger
    pub p_next: Option<TriggerRef>,        // Próximo trigger associado à tabela
}

// Um trigger é BEFORE ou AFTER. As constantes abaixo determinam qual.
//
// Se há vários triggers, pode haver uns BEFORE e outros AFTER. Nesse caso as
// constantes abaixo podem ser combinadas com OR.
pub const TRIGGER_BEFORE: u8 = 1;
pub const TRIGGER_AFTER: u8 = 2;

/// Uma instância de TriggerStep guarda uma única instrução SQL que faz parte do
/// programa de um trigger.
///
/// As instâncias formam uma lista simplesmente ligada (pelo membro `pNext`)
/// referenciada pelo `step_list` do Trigger associado. O primeiro elemento é o
/// primeiro passo do programa.
///
/// O membro `op` indica se é DELETE, INSERT, UPDATE ou SELECT. O significado dos
/// outros membros depende de `op`:
///
/// (op == TK_INSERT)
/// orconf    -> guarda o algoritmo ON CONFLICT
/// pSelect   -> o conteúdo a inserir: um SELECT ou uma cláusula VALUES
/// zTarget   -> nome sem aspas da tabela onde inserir
/// pIdList   -> em INSERT INTO ... (<column-names>) VALUES ..., guarda os nomes das colunas
/// pUpsert   -> as cláusulas ON CONFLICT de um Upsert
///
/// (op == TK_DELETE)
/// zTarget   -> nome sem aspas da tabela de onde apagar
/// pWhere    -> a cláusula WHERE do DELETE, se houver; senão None
///
/// (op == TK_UPDATE)
/// zTarget   -> nome sem aspas da tabela a atualizar
/// pWhere    -> a cláusula WHERE do UPDATE, se houver; senão None
/// pExprList -> lista das colunas a atualizar e das expressões que as atualizam
///              (ver o argumento "pChanges" de sqlite3Update())
///
/// (op == TK_SELECT)
/// pSelect   -> a instrução SELECT
///
/// (op == TK_RETURNING)
/// pExprList -> a lista de expressões após a palavra RETURNING
pub struct TriggerStep {
    pub op: u8,                            // Um de TK_DELETE, TK_UPDATE, TK_INSERT, TK_SELECT
                                           // ou TK_RETURNING
    pub orconf: u8,                        // OE_Rollback etc.
    pub p_trig: Weak<RefCell<Trigger>>,    // O trigger de que este passo faz parte
    pub p_select: Option<Box<Select>>,     // SELECT ou o lado direito de INSERT INTO SELECT ...
    pub z_target: Vec<u8>,                 // Tabela alvo de DELETE, UPDATE, INSERT
    pub p_from: Option<Box<SrcList>>,      // Cláusula FROM do UPDATE (se houver)
    pub p_where: Option<Box<Expr>>,        // Cláusula WHERE dos passos DELETE ou UPDATE
    pub p_expr_list: Option<Box<ExprList>>, // Cláusula SET do UPDATE, ou cláusula RETURNING
    pub p_id_list: Option<Box<IdList>>,    // Nomes de colunas do INSERT
    pub p_upsert: Option<Box<Upsert>>,     // Cláusulas Upsert de um INSERT
    pub z_span: Vec<u8>,                   // Texto SQL original deste comando
    pub p_next: Option<Box<TriggerStep>>,  // Próximo da lista ligada
    // O `pLast` do C (último elemento, válido só no primeiro) é um ponteiro para dentro
    // da própria lista; em Rust o último é achado percorrendo a lista a partir de step_list.
}


// ---- part_010.rs ----

/// Informação sobre uma cláusula RETURNING
pub struct Returning {
    pub p_parse: Option<Weak<RefCell<Parse>>>, // O parse que inclui a cláusula RETURNING
    pub p_return_el: Option<Box<ExprList>>,    // Lista de expressões a retornar
    pub ret_trig: Trigger,                     // O trigger transitório que implementa RETURNING
    pub ret_t_step: TriggerStep,               // O passo do trigger
    pub i_ret_cur: i32,                        // Tabela transitória com os resultados de RETURNING
    pub n_ret_col: i32,                        // Número de colunas em p_return_el após a expansão
    pub i_ret_reg: i32,                        // Array de registros que guarda uma linha de RETURNING
    pub z_name: [u8; 40],                      // Nome do trigger: "sqlite_returning_%p"
}

/// Objeto usado para acumular o texto de uma string quando não se sabe
/// necessariamente o tamanho final dela.
pub struct Sqlite3Str {
    pub db: Option<Sqlite3Ref>,   // Banco opcional para o lookaside. Pode ser None
    pub z_text: Vec<u8>,          // A string coletada até agora
    pub n_alloc: u32,             // Espaço alocado em z_text
    pub mx_alloc: u32,            // Alocação máxima permitida. 0 para nenhum uso de malloc
    pub n_char: u32,              // Tamanho da string até agora
    pub acc_error: u8,            // SQLITE_NOMEM ou SQLITE_TOOBIG
    pub printf_flags: u8,         // Flags SQLITE_PRINTF abaixo
}
pub const SQLITE_PRINTF_INTERNAL: u8 = 0x01; // Conversores de uso interno permitidos
pub const SQLITE_PRINTF_SQLFUNC: u8 = 0x02;  // Argumentos de função SQL para VXPrintf
pub const SQLITE_PRINTF_MALLOCED: u8 = 0x04; // Verdadeiro se xText é espaço alocado

#[inline]
pub fn is_malloced(x: &Sqlite3Str) -> bool {
    (x.printf_flags & SQLITE_PRINTF_MALLOCED) != 0
}

/// Cabeçalho de uma "RCStr", ou string com contagem de referências. Uma RCStr
/// circula e é usada como qualquer outro char* alocado dinamicamente. Diferenças
/// importantes da interface:
///
///   1. Strings RCStr têm contagem de referências e são liberadas quando a
///      contagem chega a zero.
///   2. Use sqlite3RCStrUnref() para liberar uma RCStr, e não sqlite3_free().
///   3. Faça uma cópia (somente leitura) de uma RCStr somente leitura com
///      sqlite3RCStrRef().
///
/// "String" está no nome, mas uma RCStr também pode guardar dados binários.
pub struct RCStr {
    pub n_rc_ref: u64,   // Número de referências
}

/// Ponteiro para esta estrutura comunica informação de sqlite3Init e de
/// OP_ParseSchema para o sqlite3InitCallback.
pub struct InitData {
    pub db: Option<Sqlite3Ref>,       // O banco em inicialização
    pub pz_err_msg: Option<Vec<u8>>,  // Mensagem de erro guardada aqui
    pub i_db: i32,                    // 0 para o principal, 1 para TEMP, 2.. para ANEXADOS
    pub rc: i32,                      // Código de resultado guardado aqui
    pub m_init_flags: u32,            // Flags que controlam as mensagens de erro
    pub n_init_row: u32,              // Número de linhas processadas
    pub mx_page: u32,                 // Número máximo de página (Pgno). 0 para sem limite
}

// Valores permitidos para mInitFlags
pub const INITFLAG_ALTERMASK: u32 = 0x0003;    // Tipos de ALTER
pub const INITFLAG_ALTERRENAME: u32 = 0x0001;  // Reanalisar após um RENAME
pub const INITFLAG_ALTERDROP: u32 = 0x0002;    // Reanalisar após um DROP COLUMN
pub const INITFLAG_ALTERADD: u32 = 0x0003;     // Reanalisar após um ADD COLUMN

// Parâmetros de ajuste (tuning) só existem em builds de depuração do CLI. Na
// build do Debian (sem SQLITE_DEBUG) `Tuning(X)` vale sempre 0.
pub const SQLITE_NTUNE: usize = 6;

#[inline]
pub fn tuning(_x: usize) -> i64 {
    0
}

/// Estrutura com os dados globais de configuração da biblioteca SQLite.
/// Também contém algumas informações de estado.
pub struct Sqlite3Config {
    pub b_memstat: i32,                   // Verdadeiro para habilitar estatísticas de memória
    pub b_core_mutex: u8,                 // Verdadeiro para habilitar o mutex do núcleo
    pub b_full_mutex: u8,                 // Verdadeiro para habilitar mutex completo
    pub b_open_uri: u8,                   // Verdadeiro para interpretar nomes de arquivo como URIs
    pub b_use_cis: u8,                    // Usa índices de cobertura em varreduras completas
    pub b_small_malloc: u8,               // Evita alocações grandes se verdadeiro
    pub b_extra_schema_checks: u8,        // Verifica type,name,tbl_name no schema
    pub b_use_long_double: u8,            // Usa long double
    pub mx_strlen: i32,                   // Tamanho máximo de string
    pub never_corrupt: i32,               // O banco está sempre bem formado
    pub sz_lookaside: i32,                // Tamanho padrão do buffer de lookaside
    pub n_lookaside: i32,                 // Quantidade padrão de buffers de lookaside
    pub n_stmt_spill: i32,                // Limite de despejo em disco do journal de instrução
    pub m: sqlite3_mem_methods,           // Interface de baixo nível de alocação de memória
    pub mutex: sqlite3_mutex_methods,     // Interface de baixo nível de mutex
    pub pcache2: sqlite3_pcache_methods2, // Interface de baixo nível do cache de páginas
    pub p_heap: Option<Box<[u8]>>,        // Espaço de armazenamento do heap
    pub n_heap: i32,                      // Tamanho de p_heap[]
    pub mn_req: i32,                      // Tamanho mínimo das requisições ao heap
    pub mx_req: i32,                      // Tamanho máximo das requisições ao heap
    pub sz_mmap: i64,                     // Espaço de mmap() por arquivo aberto
    pub mx_mmap: i64,                     // Valor máximo de sz_mmap
    pub p_page: Option<Box<[u8]>>,        // Memória do cache de páginas
    pub sz_page: i32,                     // Tamanho de cada página em p_page[]
    pub n_page: i32,                      // Número de páginas em p_page[]
    pub mx_parser_stack: i32,             // Profundidade máxima da pilha do analisador
    pub shared_cache_enabled: i32,        // Verdadeiro se o modo de cache compartilhado está ligado
    pub sz_pma: u32,                      // Tamanho máximo de PMA do ordenador
    // Os campos acima podem ser inicializados com valor diferente de zero. Os
    // seguintes precisam sempre começar em zero.
    pub is_init: i32,                     // Verdadeiro após a inicialização terminar
    pub in_progress: i32,                 // Verdadeiro enquanto a inicialização está em andamento
    pub is_mutex_init: i32,               // Verdadeiro após os mutexes serem inicializados
    pub is_malloc_init: i32,              // Verdadeiro após o malloc ser inicializado
    pub is_p_cache_init: i32,             // Verdadeiro após o cache de páginas ser inicializado
    pub n_ref_init_mutex: i32,            // Número de usuários de p_init_mutex
    pub p_init_mutex: Option<Sqlite3MutexRef>, // Mutex usado por sqlite3_initialize()
    pub x_log: Option<Rc<dyn Fn(i32, &[u8])>>, // Função de log (já captura o p_log_arg do C)
    pub mx_memdb_size: i64,               // Tamanho máximo padrão do memdb
    pub x_test_callback: Option<fn(i32) -> i32>, // Chamado por sqlite3FaultSim()
    pub b_localtime_fault: i32,           // Verdadeiro para fazer localtime() falhar
    pub x_alt_localtime: Option<fn(&[u8], &mut [u8]) -> i32>, // Rotina alternativa de localtime()
    pub i_once_reset_threshold: i32,      // Quando zerar os contadores de OP_Once
    pub sz_sorter_ref: u32,               // Tamanho mínimo em bytes para usar sorter-refs
    pub i_prng_seed: u32,                 // Semente fixa alternativa para o PRNG
}

/// Usada dentro de assert() para indicar que a asserção só vale em banco bem
/// formado: `CORRUPT_DB` é `sqlite3Config.neverCorrupt==0`.
#[inline]
pub fn corrupt_db(config: &Sqlite3Config) -> bool {
    config.never_corrupt == 0
}

/// Dados extras do callback do Walker (a `union u` do C).
pub enum WalkerU {
    None,
    Nc(Box<NameContext>),                 // Contexto de nomes
    N(i32),                               // Um contador
    ICur(i32),                            // Um número de cursor
    SrcList(Box<SrcList>),                // Cláusula FROM
    CCurHint(Box<CCurHint>),              // Usado por codeCursorHint()
    RefSrcList(Box<RefSrcList>),          // sqlite3ReferencesSrcList()
    AiCol(Vec<i32>),                      // Array de índices de coluna
    IdxCover(Box<IdxCover>),              // Verificação de cobertura de índice
    GroupBy(Box<ExprList>),               // Cláusula GROUP BY
    Select(Box<Select>),                  // Contexto de HAVING para WHERE
    Rewrite(Box<WindowRewrite>),          // Contexto de reescrita de janela
    Const(Box<WhereConst>),               // Constantes da cláusula WHERE
    Rename(Box<RenameCtx>),               // Contexto de RENAME COLUMN
    Tab(TableRef),                        // Tabela da coluna gerada
    CovIdxCk(Box<CoveringIndexCheck>),    // Verificação de índice de cobertura
    SrcItem(Box<SrcItem>),                // Um único item da cláusula FROM
    Fix(Box<DbFixer>),                    // Ver sqlite3FixSelect()
    AMem(Vec<Mem>),                       // Ver sqlite3BtreeCursorHint()
}

/// Ponteiro de contexto passado pela caminhada na árvore.
pub struct Walker {
    pub p_parse: Option<ParseRef>,                                     // Contexto do analisador
    pub x_expr_callback: Option<fn(&mut Walker, &mut Expr) -> i32>,    // Callback para expressões
    pub x_select_callback: Option<fn(&mut Walker, &mut Select) -> i32>, // Callback para SELECTs
    pub x_select_callback2: Option<fn(&mut Walker, &mut Select)>,      // Segundo callback para SELECTs
    pub walker_depth: i32,                                             // Número de subconsultas
    pub e_code: u16,                                                   // Um pequeno código de processamento
    pub m_w_flags: u16,                                                // Flags dependentes do uso
    pub u: WalkerU,                                                    // Dados extras do callback
}

/// Informação usada pelas rotinas sqliteFix... ao percorrer a árvore de análise
/// para tornar explícitas as referências a bancos.
pub struct DbFixer {
    pub p_parse: Option<ParseRef>,      // Contexto de análise. Mensagens de erro vão aqui
    pub w: Walker,                      // Objeto Walker
    pub p_schema: Option<SchemaRef>,    // Fixa os itens a este schema
    pub b_temp: u8,                     // Verdadeiro para entradas do schema TEMP
    pub z_db: Vec<u8>,                  // Garante que todos os objetos estão neste banco
    pub z_type: Vec<u8>,                // Tipo do contêiner, usado em mensagens de erro
    pub p_name: Option<Token>,          // Nome do contêiner, usado em mensagens de erro
}

// Códigos de retorno das primitivas de caminhada na árvore e de seus callbacks.
pub const WRC_CONTINUE: i32 = 0;   // Continua descendo aos filhos
pub const WRC_PRUNE: i32 = 1;      // Omite os filhos mas continua nos irmãos
pub const WRC_ABORT: i32 = 2;      // Abandona a caminhada

/// Uma única expressão de tabela comum (CTE)
pub struct Cte {
    pub z_name: Vec<u8>,                 // Nome desta CTE
    pub p_cols: Option<Box<ExprList>>,   // Lista de nomes de coluna explícitos, ou None
    pub p_select: Option<Box<Select>>,   // A definição desta CTE
    pub z_cte_err: Option<Vec<u8>>,      // Mensagem de erro para referências circulares
    pub p_use: Option<CteUseRef>,        // Informação de uso desta CTE
    pub e_m10d: u8,                      // O flag MATERIALIZED
}

// Valores permitidos para o flag de materialização (eM10d)
pub const M10D_YES: u8 = 0;   // AS MATERIALIZED
pub const M10D_ANY: u8 = 1;   // Não especificado. Escolha do planejador
pub const M10D_NO: u8 = 2;    // AS NOT MATERIALIZED

/// Uma instância de With representa uma cláusula WITH com uma ou mais CTEs.
pub struct With {
    pub n_cte: i32,                  // Número de CTEs na cláusula WITH
    pub b_view: i32,                 // Pertence ao Select mais externo de uma visão
    pub p_outer: Option<WithRef>,    // Cláusula WITH que contém esta, ou None
    pub a: Vec<Cte>,                 // Uma entrada para cada CTE da cláusula WITH
}

/// O objeto Cte não tem garantia de durar toda a geração de código (o achatador
/// de consultas ou outras edições da árvore podem apagá-lo). Este objeto guarda a
/// informação de cada CTE que precisa ser preservada durante toda a análise.
///
/// Os CteUse são liberados com sqlite3ParserAddCleanup() e não com
/// sqlite3SelectDelete(), o que lhes permite durar até o fim da geração de código.
pub struct CteUse {
    pub n_use: i32,          // Número de usuários desta CTE
    pub addr_m9e: i32,       // Início da sub-rotina que calcula a materialização
    pub reg_rtn: i32,        // Registro do endereço de retorno da sub-rotina addr_m9e
    pub i_cur: i32,          // Tabela efêmera com a materialização
    pub n_row_est: LogEst,   // Número estimado de linhas da tabela
    pub e_m10d: u8,          // O flag MATERIALIZED
}

/// Dados de cliente associados a sqlite3_set_clientdata() e sqlite3_get_clientdata().
/// O destrutor do C vira o `Drop` do próprio dado.
pub struct DbClientData {
    pub p_next: Option<Box<DbClientData>>,  // Próximo da lista ligada
    pub p_data: Box<dyn std::any::Any>,     // O dado (o destrutor roda no Drop)
    pub z_name: Vec<u8>,                    // Nome deste dado de cliente
}

/// Usado de várias formas, a maioria (mas não todas) ligada a funções de janela.
///
///   (1) Uma única instância é anexada ao campo Expr.y.pWin de cada função de
///       janela de uma árvore de expressão. Guarda a informação da cláusula OVER
///       mais campos usados durante a geração de código.
///   (2) Todas as funções de janela de um SELECT formam uma lista ligada em
///       Select.pWin. Window.pFunc e Window.pExpr apontam de volta à expressão.
///   (3) Os termos da cláusula WINDOW de um SELECT são instâncias deste objeto
///       numa lista ligada em Select.pWinDefn.
///   (4) Numa função de agregação com FILTER, uma instância fica em Expr.y.pWin
///       com eFrmType igual a TK_FILTER; o único campo usado é Window.pFilter.
///
/// Os usos (1) e (2) são o mesmo objeto Window acessível de dois jeitos; o uso (3)
/// são objetos separados.
pub struct Window {
    pub z_name: Option<Vec<u8>>,           // Nome da janela (pode ser None)
    pub z_base: Option<Vec<u8>>,           // Nome da janela base para encadeamento (pode ser None)
    pub p_partition: Option<Box<ExprList>>, // Cláusula PARTITION BY
    pub p_order_by: Option<Box<ExprList>>, // Cláusula ORDER BY
    pub e_frm_type: u8,                    // TK_RANGE, TK_GROUPS, TK_ROWS ou 0
    pub e_start: u8,                       // UNBOUNDED, CURRENT, PRECEDING ou FOLLOWING
    pub e_end: u8,                         // UNBOUNDED, CURRENT, PRECEDING ou FOLLOWING
    pub b_implicit_frame: u8,              // Verdadeiro se o frame foi especificado implicitamente
    pub e_exclude: u8,                     // TK_NO, TK_CURRENT, TK_TIES, TK_GROUP ou 0
    pub p_start: Option<Box<Expr>>,        // Expressão de "<expr> PRECEDING"
    pub p_end: Option<Box<Expr>>,          // Expressão de "<expr> FOLLOWING"
    // `ppThis` do C (ponteiro para este objeto na lista Select.pWin) não tem
    // equivalente sem ponteiros: o desligamento da lista é feito por busca.
    pub p_next_win: Option<Box<Window>>,   // Próxima função de janela deste SELECT
    pub p_filter: Option<Box<Expr>>,       // A expressão FILTER
    pub p_w_func: Option<Rc<FuncDef>>,     // A função
    pub i_eph_csr: i32,                    // Buffer de partição ou de pares
    pub reg_accum: i32,                    // Acumulador
    pub reg_result: i32,                   // Resultado intermediário
    pub csr_app: i32,                      // Cursor da função (usado por min/max)
    pub reg_app: i32,                      // Registro da função (também usado por min/max)
    pub reg_part: i32,                     // Array de registros com os valores de PARTITION BY
    pub p_owner: Option<Weak<RefCell<Expr>>>, // Expressão à qual esta janela está anexada
    pub n_buffer_col: i32,                 // Número de colunas na tabela de buffer
    pub i_arg_col: i32,                    // Deslocamento do primeiro argumento desta função
    pub reg_one: i32,                      // Registro com o valor constante 1
    pub reg_start_rowid: i32,
    pub reg_end_rowid: i32,
    pub b_expr_args: u8,                   // Adia a avaliação dos argumentos da função de janela
                                           // por causa do flag SQLITE_SUBTYPE
}


// ---- part_011.rs ----

// Este trecho do sqliteInt.h é quase só protótipo de função. Protótipo não tem
// tradução: cada função vive no módulo do arquivo C que a define (window.c,
// malloc.c, mutex.c, status.c, util.c, printf.c, treeview.c, parse.y, expr.c ...) e
// recebe o nome pela regra de nomes. Aqui ficam só os `#define` com corpo, as
// estruturas e as constantes.
//
// Notas para o integrador:
//  - As rotinas de memória (`sqlite3DbMallocRaw`, `sqlite3DbFree`, `sqlite3StackAllocRaw`
//    e afins) somem: em Rust a alocação é `Vec`/`Box` e a liberação é o `Drop`.
//    `sqlite3StackAllocRaw(D,N)` vira `vec![0u8; n]`; `sqlite3StackFree` é soltar o Vec.
//  - `sqlite3MutexWarnOnContention` é vazio (sem SQLITE_ENABLE_MULTITHREADED_CHECKS).
//  - As funções de TreeView/Show só existem com SQLITE_DEBUG e foram omitidas.
//  - Com SQLITE_ENABLE_MEMSYS3/5 ausentes, não há sqlite3MemGetMemsys3/5.

/// Avança `i` (índice em `z`) da primeira byte de um caractere UTF-8 para a
/// primeira byte do caractere seguinte. Macro SQLITE_SKIP_UTF8.
#[inline]
pub fn skip_utf8(z: &[u8], i: &mut usize) {
    let c = z[*i];
    *i += 1;
    if c >= 0xc0 {
        while *i < z.len() && (z[*i] & 0xc0) == 0x80 {
            *i += 1;
        }
    }
}

// Os macros SQLITE_*_BKPT substituem os códigos de erro de mesmo nome sem o sufixo
// _BKPT. Chamam rotinas que relatam, via sqlite3_log(), a linha em que o erro nasceu
// (o chamador passa `line!()`), e dão um ponto cômodo para breakpoint de depurador.
// As rotinas sqlite3CorruptError, sqlite3MisuseError e sqlite3CantopenError vivem em
// main.c (módulo `api`/main).
#[inline]
pub fn sqlite_corrupt_bkpt(lineno: i32) -> i32 {
    corrupt_error(lineno)
}

#[inline]
pub fn sqlite_misuse_bkpt(lineno: i32) -> i32 {
    misuse_error(lineno)
}

#[inline]
pub fn sqlite_cantopen_bkpt(lineno: i32) -> i32 {
    cantopen_error(lineno)
}

// Sem SQLITE_DEBUG, as variantes de memória esgotada são as próprias constantes.
pub const SQLITE_NOMEM_BKPT: i32 = SQLITE_NOMEM;
pub const SQLITE_IOERR_NOMEM_BKPT: i32 = SQLITE_IOERR_NOMEM;

/// Sem SQLITE_DEBUG nem SQLITE_ENABLE_CORRUPT_PGNO o número da página é ignorado.
#[inline]
pub fn sqlite_corrupt_pgno(lineno: i32, _p: Pgno) -> i32 {
    corrupt_error(lineno)
}

// Os macros abaixo imitam toupper(), isspace(), isalnum(), isdigit() e isxdigit() da
// biblioteca padrão. As versões do SQLite só funcionam com caracteres ASCII,
// qualquer que seja o locale. `CTYPE_MAP` e `UPPER_TO_LOWER` são as tabelas
// sqlite3CtypeMap e sqlite3UpperToLower de global.c.
#[inline]
pub fn toupper(x: u8) -> u8 {
    x & !(CTYPE_MAP[x as usize] & 0x20)
}

#[inline]
pub fn isspace(x: u8) -> bool {
    (CTYPE_MAP[x as usize] & 0x01) != 0
}

#[inline]
pub fn isalnum(x: u8) -> bool {
    (CTYPE_MAP[x as usize] & 0x06) != 0
}

#[inline]
pub fn isalpha(x: u8) -> bool {
    (CTYPE_MAP[x as usize] & 0x02) != 0
}

#[inline]
pub fn isdigit(x: u8) -> bool {
    (CTYPE_MAP[x as usize] & 0x04) != 0
}

#[inline]
pub fn isxdigit(x: u8) -> bool {
    (CTYPE_MAP[x as usize] & 0x08) != 0
}

#[inline]
pub fn tolower(x: u8) -> u8 {
    UPPER_TO_LOWER[x as usize]
}

#[inline]
pub fn isquote(x: u8) -> bool {
    (CTYPE_MAP[x as usize] & 0x80) != 0
}

#[inline]
pub fn json_id1(x: u8) -> bool {
    (CTYPE_MAP[x as usize] & 0x42) != 0
}

#[inline]
pub fn json_id2(x: u8) -> bool {
    (CTYPE_MAP[x as usize] & 0x46) != 0
}

/// `strlen(C)&0x3fffffff` sobre texto terminado em NUL (ou até o fim do slice).
#[inline]
pub fn strlen30_nn(z: &[u8]) -> i32 {
    let n = z.iter().position(|&b| b == 0).unwrap_or(z.len());
    (n & 0x3fffffff) as i32
}

// Aritmética de bits de um double IEEE 754 visto como u64.
pub const EXP754: u64 = 0x7ffu64 << 52;
pub const MAN754: u64 = (1u64 << 52) - 1;

/// Macro IsNaN(X): X são os bits do double.
#[inline]
pub fn is_nan(x: u64) -> bool {
    (x & EXP754) == EXP754 && (x & MAN754) != 0
}

/// Macro IsOvfl(X): X são os bits do double.
#[inline]
pub fn is_ovfl(x: u64) -> bool {
    (x & EXP754) == EXP754
}

/// Guarda informação sobre os argumentos de função SQL que são os parâmetros da
/// função printf().
pub struct PrintfArguments {
    pub n_arg: i32,                       // Número total de argumentos
    pub n_used: i32,                      // Número de argumentos usados até agora
    pub ap_arg: Vec<Rc<RefCell<Mem>>>,    // Os valores dos argumentos (sqlite3_value*)
}

/// Recebe a decodificação de um valor de ponto flutuante numa representação
/// decimal aproximada.
pub struct FpDecode {
    pub sign: u8,           // '+' ou '-'
    pub is_special: u8,     // 1: Infinito  2: NaN
    pub n: i32,             // Dígitos significativos na decodificação
    pub i_dp: i32,          // Posição do ponto decimal
    pub z: usize,           // Início dos dígitos significativos (índice em z_buf)
    pub z_buf: [u8; 24],    // Armazenamento dos dígitos significativos
}


// ---- part_012.rs ----

// Este trecho do sqliteInt.h só tem protótipos (as funções são definidas e
// traduzidas no módulo de origem de cada uma, pela regra de nomes) e alguns
// `#define`. Só os `#define` ativos nas opções do Debian 13 viram item aqui.
// `SQLITE_ENABLE_HIDDEN_COLUMNS`, `SQLITE_ENABLE_NULL_TRIM` e
// `SQLITE_ENABLE_UPDATE_DELETE_LIMIT` não estão ligados: as macros
// `sqlite3ColumnPropertiesFromName` e `sqlite3SetMakeRecordP5` são no-op e
// somem, e `sqlite3LimitWhere` não existe.

/// `sqlite3CodecQueryParameters(A,B,C)`: sem codec, sempre 0.
#[inline]
pub fn codec_query_parameters(_db: &Sqlite3, _z_path: &[u8], _z_uri: &[u8]) -> i32 {
    0
}

/// Valores de `sqlite3WhereOkOnePass()`.
pub const ONEPASS_OFF: i32 = 0; // Uso de ONEPASS não permitido
pub const ONEPASS_SINGLE: i32 = 1; // ONEPASS válido para atualização de uma linha
pub const ONEPASS_MULTI: i32 = 2; // ONEPASS válido para várias linhas

/// Flags de `sqlite3ExprCodeExprList()`.
pub const SQLITE_ECEL_DUP: u8 = 0x01; // Cópias profundas, não rasas
pub const SQLITE_ECEL_FACTOR: u8 = 0x02; // Fatorar termos constantes
pub const SQLITE_ECEL_REF: u8 = 0x04; // Usar ExprList.u.x.iOrderByCol
pub const SQLITE_ECEL_OMITREF: u8 = 0x08; // Omitir se ExprList.u.x.iOrderByCol

/// Flags de `sqlite3LocateTable()`.
pub const LOCATE_VIEW: u32 = 0x01;
pub const LOCATE_NOERR: u32 = 0x02;

/// `sqlite3ParseToplevel(p)`: o `Parse` de nível mais alto (triggers ligados).
#[inline]
pub fn parse_toplevel(p: &ParseRef) -> ParseRef {
    match &p.borrow().p_toplevel {
        Some(top) => top.clone(),
        None => p.clone(),
    }
}

/// `sqlite3IsToplevel(p)`.
#[inline]
pub fn is_toplevel(p: &Parse) -> bool {
    p.p_toplevel.is_none()
}


// ---- part_013.rs ----

// Este trecho do sqliteInt.h só tem protótipos, declarações de globais
// (`sqlite3OpcodeProperty`, `sqlite3UpperToLower`, `sqlite3Config`,
// `sqlite3PendingByte` e afins, que pertencem ao main.c/global.c) e macros.
// Funções e globais são traduzidas no módulo de origem, pela regra de nomes.
// Aqui ficam só as macros ativas nas opções do Debian 13:
// - `sqlite3FileSuffix3`, `sqlite3IsMemdb`, `sqlite3ExprCheckIN`,
//   `sqlite3CloseExtensions`, `sqlite3TableLock`: variantes `#else` inativas,
//   as funções reais existem.
// - `getVarint` e `putVarint` são apelidos de `sqlite3GetVarint` e
//   `sqlite3PutVarint`: quem chama usa `get_varint` e `put_varint` do util.

/// Macro `getVarint32(A,B)`: caso comum de um byte sem chamada de função.
#[inline]
pub fn get_varint32_fast(a: &[u8], b: &mut u32) -> u8 {
    if a[0] < 0x80 {
        *b = a[0] as u32;
        1
    } else {
        get_varint32(a, b)
    }
}

/// Macro `getVarint32NR(A,B)`: igual à anterior, sem valor de retorno.
#[inline]
pub fn get_varint32_nr(a: &[u8], b: &mut u32) {
    *b = a[0] as u32;
    if *b >= 0x80 {
        get_varint32(a, b);
    }
}

/// Macro `putVarint32(A,B)`: caso comum de um byte sem chamada de função.
#[inline]
pub fn put_varint32(a: &mut [u8], b: u32) -> u8 {
    if b < 0x80 {
        a[0] = b as u8;
        1
    } else {
        put_varint(a, b as u64) as u8
    }
}

/// Macro `sqlite3VtabInSync(db)` (virtual tables ligadas).
#[inline]
pub fn vtab_in_sync(db: &Sqlite3) -> bool {
    db.n_v_trans > 0 && db.a_v_trans.is_empty()
}


// ---- part_014.rs ----

// Este trecho do sqliteInt.h só tem protótipos (CTE, UPSERT, chaves
// estrangeiras, journal, vetores, ...), traduzidos no módulo de origem de
// cada função, e as macros abaixo. As variantes `#else` de `OMIT_CTE`,
// `OMIT_UPSERT`, `OMIT_FOREIGN_KEY`, `UNTESTABLE`, `ENABLE_UNLOCK_NOTIFY` com
// no-op e `SQLITE_MEMDEBUG` (que só alimenta assert) não valem no Debian 13.
// `SQLITE_ENABLE_STMT_SCANSTATUS` não está ligado, então `IS_STMT_SCANSTATUS`
// é sempre falso.

/// Injetores de falha disponíveis, numerados a partir de 0.
pub const SQLITE_FAULTINJECTOR_MALLOC: i32 = 0;
pub const SQLITE_FAULTINJECTOR_COUNT: i32 = 1;

/// Valores de retorno de `sqlite3FindInIndex()`.
pub const IN_INDEX_ROWID: i32 = 1; // Procura o rowid da tabela
pub const IN_INDEX_EPH: i32 = 2; // Procura numa b-tree efêmera
pub const IN_INDEX_INDEX_ASC: i32 = 3; // Índice existente ASCENDENTE
pub const IN_INDEX_INDEX_DESC: i32 = 4; // Índice existente DESCENDENTE
pub const IN_INDEX_NOOP: i32 = 5; // Sem tabela disponível, usa comparações

/// Flags do 3o parâmetro de `sqlite3FindInIndex()`.
pub const IN_INDEX_NOOP_OK: u32 = 0x0001; // Pode retornar IN_INDEX_NOOP
pub const IN_INDEX_MEMBERSHIP: u32 = 0x0002; // IN usado como teste de pertinência
pub const IN_INDEX_LOOP: u32 = 0x0004; // IN usado como laço

/// Tipos de alocação do depurador de memória.
pub const MEMTYPE_HEAP: u8 = 0x01; // Alocações gerais do heap
pub const MEMTYPE_LOOKASIDE: u8 = 0x02; // Heap que poderia ter vindo do lookaside
pub const MEMTYPE_PCACHE: u8 = 0x04; // Alocações do cache de páginas

/// Macro `IS_STMT_SCANSTATUS(db)`: sem `SQLITE_ENABLE_STMT_SCANSTATUS`, sempre 0.
#[inline]
pub fn is_stmt_scanstatus(_db: &Sqlite3) -> bool {
    false
}

