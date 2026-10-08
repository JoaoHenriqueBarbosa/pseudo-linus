// Mesclado das partes traduzidas de pager_c (as partes quebram funções grandes no meio).
#![allow(unused_imports, ambiguous_glob_reexports)]
use crate::prelude::*;

// ---- part_000.rs ----

/// Implementação do subsistema de cache de páginas ou "pager".
///
/// O pager é usado para acessar um arquivo de banco de dados em disco. Implementa
/// confirmação atômica e reversão através do uso de um arquivo de journal separado
/// do arquivo do banco de dados. O pager também implementa bloqueio de arquivo
/// para impedir que dois processos escrevam no mesmo arquivo de banco de dados
/// simultaneamente, ou um processo leia o banco de dados enquanto outro está escrevendo.

/// Estados do pager. A variável Pager.e_state armazena o estado atual de um pager.
/// Um pager pode estar em qualquer um dos sete estados mostrados no diagrama de estados
/// a seguir.
///
/// ```text
///                             OPEN <------+------+
///                               |         |      |
///                               V         |      |
///                +---------> READER-------+      |
///                |              |                |
///                |              V                |
///                |<-------WRITER_LOCKED------> ERROR
///                |              |                ^
///                |              V                |
///                |<------WRITER_CACHEMOD-------->|
///                |              |                |
///                |              V                |
///                |<-------WRITER_DBMOD---------->|
///                |              |                |
///                |              V                |
///                +<------WRITER_FINISHED-------->+
/// ```
///
/// Lista de transições de estado e a função C que realiza cada uma:
///
/// - OPEN -> READER: [sqlite3_pager_shared_lock]
/// - READER -> OPEN: [pager_unlock]
/// - READER -> WRITER_LOCKED: [sqlite3_pager_begin]
/// - WRITER_LOCKED -> WRITER_CACHEMOD: [pager_open_journal]
/// - WRITER_CACHEMOD -> WRITER_DBMOD: [sync_journal]
/// - WRITER_DBMOD -> WRITER_FINISHED: [sqlite3_pager_commit_phase_one]
/// - WRITER_*** -> READER: [pager_end_transaction]
/// - WRITER_*** -> ERROR: [pager_error]
/// - ERROR -> OPEN: [pager_unlock]
///
/// OPEN:
///   O pager inicia neste estado. Nada é garantido neste estado: o arquivo pode
///   ou não estar bloqueado e o tamanho do banco de dados é desconhecido. O banco
///   de dados não pode ser lido ou escrito.
///
///   * Nenhuma transação de leitura ou escrita está ativa.
///   * Qualquer bloqueio, ou nenhum bloqueio, pode estar sendo mantido no arquivo do banco.
///   * As variáveis db_size, db_orig_size e db_file_size não podem ser confiáveis.
///
/// READER:
///   Neste estado, todos os requisitos para ler o banco de dados no modo de
///   reversão (não WAL) são atendidos. A menos que o pager esteja (ou tenha
///   estado recentemente) em modo de bloqueio exclusivo, uma transação de leitura
///   do nível do usuário está aberta. O tamanho do banco de dados é conhecido neste estado.
///
///   Uma conexão em execução com locking_mode=normal entra neste estado quando
///   abre uma transação de leitura no banco de dados e retorna ao estado OPEN
///   após a transação de leitura ser concluída. Porém, uma conexão em execução
///   em locking_mode=exclusive (incluindo bancos de dados temporários) permanece
///   neste estado mesmo após a transação de leitura ser fechada. A única maneira
///   de uma conexão locking_mode=exclusive fazer transição de READER para OPEN
///   é através do estado ERROR (veja abaixo).
///
///   * Uma transação de leitura pode estar ativa (mas uma transação de escrita não pode).
///   * Um bloqueio SHARED ou maior está sendo mantido no arquivo do banco de dados.
///   * A variável db_size pode ser confiável (mesmo se uma transação de leitura do
///     nível do usuário não está ativa). As variáveis db_orig_size e db_file_size
///     podem não ser confiáveis neste ponto.
///   * Se o banco de dados é um banco de dados WAL, então a conexão WAL está aberta.
///   * Mesmo se uma transação de leitura não estiver aberta, é garantido que
///     não haja um hot-journal no sistema de arquivos.
///
/// WRITER_LOCKED:
///   O pager se move para este estado a partir de READER quando uma transação de
///   escrita é aberta pela primeira vez no banco de dados. No estado WRITER_LOCKED,
///   todos os bloqueios necessários para iniciar uma transação de escrita são mantidos,
///   mas nenhuma modificação real no cache ou banco de dados foi feita.
///
///   No modo de reversão, um bloqueio RESERVED ou (se a transação foi aberta com
///   BEGIN EXCLUSIVE) EXCLUSIVE é obtido no arquivo do banco de dados ao se mover
///   para este estado, mas o arquivo de journal não é escrito neste estado. Se a
///   transação é confirmada ou revertida enquanto em estado WRITER_LOCKED, tudo
///   o que é necessário é desbloquear o arquivo do banco de dados.
///
///   No modo WAL, WalBeginWriteTransaction() é chamado para bloquear o arquivo de log.
///   Se a conexão está em execução com locking_mode=exclusive, tenta-se obter um
///   bloqueio EXCLUSIVE no arquivo do banco de dados.
///
///   * Uma transação de escrita está ativa.
///   * Se a conexão está aberta em modo de reversão, um bloqueio RESERVED ou maior
///     está sendo mantido no arquivo do banco de dados.
///   * Se a conexão está aberta em modo WAL, uma transação de escrita WAL está aberta
///     (ou seja, sqlite3_wal_begin_write_transaction() foi chamado com sucesso).
///   * As variáveis db_size, db_orig_size e db_file_size são todas válidas.
///   * O conteúdo do cache do pager não foi modificado.
///   * O arquivo de journal pode ou não estar aberto.
///   * Nada (nem mesmo o primeiro cabeçalho) foi escrito no journal.
///
/// WRITER_CACHEMOD:
///   Um pager se move de estado WRITER_LOCKED para este estado quando uma página
///   é modificada pela primeira vez pela camada superior. No modo de reversão,
///   o arquivo de journal é aberto (se ainda não estiver aberto) e um cabeçalho
///   é escrito no início dele. O arquivo do banco de dados em disco não foi modificado.
///
///   * Uma transação de escrita está ativa.
///   * Um bloqueio RESERVED ou maior está sendo mantido no arquivo do banco de dados.
///   * O arquivo de journal está aberto e o primeiro cabeçalho foi escrito nele,
///     mas o cabeçalho não foi sincronizado com o disco.
///   * O conteúdo do cache de páginas foi modificado.
///
/// WRITER_DBMOD:
///   O pager faz transição de WRITER_CACHEMOD para estado WRITER_DBMOD quando
///   modifica o conteúdo do arquivo do banco de dados. Conexões WAL nunca entram
///   neste estado (já que não modificam o arquivo do banco de dados, apenas o
///   arquivo de log).
///
///   * Uma transação de escrita está ativa.
///   * Um bloqueio EXCLUSIVE ou maior está sendo mantido no arquivo do banco de dados.
///   * O arquivo de journal está aberto e o primeiro cabeçalho foi escrito e
///     sincronizado com o disco.
///   * O conteúdo do cache de páginas foi modificado (e possivelmente escrito no disco).
///
/// WRITER_FINISHED:
///   Não é possível uma conexão WAL entrar neste estado.
///
///   Um pager em modo de reversão muda para estado WRITER_FINISHED a partir de
///   WRITER_DBMOD após toda a transação ter sido escrita com sucesso no arquivo
///   do banco de dados. Neste estado a transação pode ser confirmada simplesmente
///   finalizando o arquivo de journal. Uma vez em estado WRITER_FINISHED, não é
///   possível modificar o banco de dados ainda mais. Neste ponto, a camada superior
///   deve confirmar ou reverter a transação.
///
///   * Uma transação de escrita está ativa.
///   * Um bloqueio EXCLUSIVE ou maior está sendo mantido no arquivo do banco de dados.
///   * Toda a escrita e sincronização de dados de journal e banco de dados foi concluída.
///     Se nenhum erro ocorreu, tudo o que resta é finalizar o journal para confirmar
///     a transação. Se um erro ocorreu, o chamador precisará reverter a transação.
///
/// ERROR:
///   O estado ERROR é inserido quando um erro de E/S ou disco cheio (incluindo
///   SQLITE_IOERR_NOMEM) ocorre em um ponto do código que torna difícil garantir
///   que o estado do pager em memória (conteúdo do cache, tamanho do banco, etc.)
///   seja consistente com o conteúdo do sistema de arquivos.
///
///   Arquivos do pager temporário podem entrar no estado ERROR, mas pagers em memória não.
///
///   Por exemplo, se um erro de E/S ocorre ao realizar uma reversão, o conteúdo
///   do cache de páginas pode ser deixado em estado inconsistente. Neste ponto
///   seria perigoso mudar de volta para estado READER (como geralmente acontece
///   após uma reversão). Leitores subsequentes podem relatar corrupção do banco
///   de dados (devido ao cache inconsistente), e se fizerem upgrade para escritores,
///   podem inadvertidamente corromper o arquivo do banco de dados. Para evitar este
///   perigo, o pager muda para o estado ERROR em vez de READER após tal erro.
///
///   Uma vez tendo entrado no estado ERROR, qualquer tentativa de usar o pager
///   para ler ou escrever dados retorna um erro. Eventualmente, uma vez que todas
///   as transações pendentes tenham sido abandonadas, o pager pode fazer transição
///   de volta ao estado OPEN, descartando o conteúdo do cache de páginas e qualquer
///   outro estado em memória simultaneamente. Tudo é recarregado do disco (e, se
///   necessário, rollback de hot-journal realizado) quando uma transação de leitura
///   é aberta novamente no pager (fazendo transição do pager para estado READER).
///   Neste ponto, o sistema se recuperou do erro.
///
///   Especificamente, o pager pula para o estado ERROR se:
///
///   1. Um erro ocorre ao tentar uma reversão. Isto acontece na função
///      sqlite3_pager_rollback().
///
///   2. Um erro ocorre ao tentar finalizar um arquivo de journal após uma
///      confirmação na função sqlite3_pager_commit_phase_two().
///
///   3. Um erro ocorre ao tentar escrever no arquivo de journal ou banco de dados
///      na função pager_stress() para liberar memória.
///
///   Em outros casos, o erro é retornado à camada b-tree. A camada b-tree
///   tenta uma operação de reversão. Se a condição de erro persiste, o pager
///   entra no estado ERROR através da condição (1) acima.
///
///   A condição (3) é necessária porque pode ser acionada por uma instrução
///   somente leitura executada dentro de uma transação. Neste caso, se o código
///   de erro fosse simplesmente retornado ao usuário, a camada b-tree não
///   tentaria automaticamente uma reversão, já que assume que um erro em uma
///   instrução somente leitura não pode deixar o pager em estado internamente inconsistente.
///
///   * A variável Pager.err_code é definida como algo diferente de SQLITE_OK.
///   * Há uma ou mais referências pendentes a páginas (após a última referência
///     ser removida, o pager deve voltar ao estado OPEN).
///   * O pager não é um pager em memória.
pub const PAGER_OPEN: i32 = 0;
pub const PAGER_READER: i32 = 1;
pub const PAGER_WRITER_LOCKED: i32 = 2;
pub const PAGER_WRITER_CACHEMOD: i32 = 3;
pub const PAGER_WRITER_DBMOD: i32 = 4;
pub const PAGER_WRITER_FINISHED: i32 = 5;
pub const PAGER_ERROR: i32 = 6;

/// A variável Pager.e_lock quase sempre é definida como um dos seguintes estados
/// de bloqueio, de acordo com o bloqueio mantido no arquivo do banco de dados:
/// NO_LOCK, SHARED_LOCK, RESERVED_LOCK ou EXCLUSIVE_LOCK. Esta variável é mantida
/// atualizada conforme os bloqueios são tomados e liberados pelos wrappers
/// pager_lock_db() e pager_unlock_db().
///
/// Se o xLock() ou xUnlock() do VFS retorna um erro diferente de SQLITE_BUSY
/// (ou seja, um dos subtipos SQLITE_IOERR), não está claro se a operação foi bem
/// sucedida ou não. Nestas circunstâncias, pager_lock_db() e pager_unlock_db()
/// tomam uma abordagem conservadora: e_lock é sempre atualizado ao desbloquear o
/// arquivo, e é atualizado apenas ao bloquear o arquivo se a chamada do VFS for
/// bem sucedida. Desta forma, a variável Pager.e_lock pode ser definida como um
/// valor menos exclusivo (menor) do que o bloqueio realmente mantido no nível do
/// sistema, mas nunca é definida como um valor mais exclusivo.
///
/// Isto é geralmente seguro. Se um xUnlock falha ou parece falhar, pode haver
/// poucas chamadas xLock() redundantes ou um bloqueio pode ser mantido por mais
/// tempo do que necessário, mas nada realmente dá errado.
///
/// A exceção é quando o arquivo do banco de dados é desbloqueado conforme o pager
/// se move de estado ERROR para OPEN. Neste ponto pode haver um arquivo hot-journal
/// no sistema de arquivos que precisa ser revertido (como parte de uma transição
/// OPEN->SHARED, pelo mesmo pager ou outro). Se a chamada para xUnlock() falhar
/// neste ponto e o pager for deixado mantendo um bloqueio EXCLUSIVE, isto pode
/// confundir a chamada xCheckReservedLock() feita mais tarde como parte da detecção
/// de hot-journal.
///
/// xCheckReservedLock() é definido como retornando true "se há um bloqueio RESERVED
/// mantido por este processo ou qualquer outro". Assim, xCheckReservedLock pode
/// retornar true porque o chamador mesmo está mantendo um bloqueio EXCLUSIVE (mas
/// não sabe porque de um erro anterior em xUnlock). Se isto acontece, um hot-journal
/// pode ser confundido com um journal sendo criado por uma transação ativa em outro
/// processo, causando SQLite a ler do banco de dados sem revertê-lo.
///
/// Para contornar isto, se uma chamada a xUnlock() falha ao desbloquear o banco de
/// dados no estado ERROR, Pager.e_lock é definido como UNKNOWN_LOCK. Ele é apenas
/// mudado de volta para um estado de bloqueio real após uma chamada bem sucedida
/// a xLock(EXCLUSIVE). Também, o código para fazer a transição de estado OPEN->SHARED
/// omite a verificação de um hot-journal se Pager.e_lock é definido como UNKNOWN_LOCK.
/// Em vez disso, assume que um hot-journal existe e obtém um bloqueio EXCLUSIVE no
/// arquivo do banco de dados antes de tentar revertê-lo. Veja a função
/// pager_shared_lock() para mais detalhe.
///
/// Pager.e_lock pode ser definido como UNKNOWN_LOCK apenas quando o pager está em
/// estado PAGER_OPEN.
pub const UNKNOWN_LOCK: i32 = EXCLUSIVE_LOCK + 1;

/// O tamanho de setor máximo permitido. 64 KiB. Se o método xSectorsize()
/// retorna um valor maior do que isto, então MAX_SECTOR_SIZE é usado em seu lugar.
/// Isto poderia concebivelmente causar corrupção após uma falha de energia em
/// tal sistema. Este é atualmente um limite não documentado.
pub const MAX_SECTOR_SIZE: u32 = 0x10000;

/// Uma instância da seguinte estrutura é alocada para cada savepoint e transação
/// de instrução ativa no sistema. Todas essas estruturas são armazenadas no
/// array Pager.a_savepoint[], que é alocado e redimensionado usando
/// sqlite3_realloc().
///
/// Quando um savepoint é criado, o campo PagerSavepoint.i_hdr_offset é definido
/// como 0. Se um cabeçalho de journal for escrito no journal principal enquanto
/// o savepoint estiver ativo, então i_hdr_offset é definido como o deslocamento
/// em bytes imediatamente seguinte ao último registro de journal escrito no journal
/// principal antes do cabeçalho de journal. Isto é necessário durante a reversão
/// de savepoint (veja pager_playback_savepoint()).
pub struct PagerSavepoint {
    /// Deslocamento inicial no journal principal.
    pub i_offset: i64,
    /// Veja acima.
    pub i_hdr_offset: i64,
    /// Conjunto de páginas neste savepoint.
    pub p_in_savepoint: Option<Box<Bitvec>>,
    /// Número original de páginas no arquivo.
    pub n_orig: Pgno,
    /// Índice do primeiro registro no sub-journal.
    pub i_sub_rec: Pgno,
    /// Se o journal de instrução pode ser truncado ao RELEASE.
    pub b_truncate_on_release: i32,
    /// Dados de contexto do savepoint WAL.
    #[cfg(not(feature = "sqlite_omit_wal"))]
    pub a_wal_data: [u32; WAL_SAVEPOINT_NDATA],
}


// ---- part_001.rs ----

/// Bits da flag Pager.doNotSpill. Veja descrição mais abaixo.
pub const SPILLFLAG_OFF: u8 = 0x01;
pub const SPILLFLAG_ROLLBACK: u8 = 0x02;
pub const SPILLFLAG_NOSYNC: u8 = 0x04;

/// Um cache de página aberto é uma instância de struct Pager. Uma descrição de
/// alguns dos membros mais importantes segue:
///
/// eState: O estado atual do objeto pager. Veja o comentário e diagrama de estado
/// acima para uma descrição do estado do pager.
///
/// eLock: Para um banco de dados de disco real, o lock atual mantido no arquivo
/// de banco: NO_LOCK, SHARED_LOCK, RESERVED_LOCK ou EXCLUSIVE_LOCK.
/// Para um banco de dados temporário ou em memória (nenhum dos quais requer locks),
/// esta variável é sempre definida como EXCLUSIVE_LOCK. Como tais bancos sempre
/// têm Pager.exclusiveMode==1, isso engana a lógica do pager para pensar que já
/// tem todos os locks que terá (e nenhuma razão para liberá-los).
/// Em circunstâncias (obscuras) raras, esta variável também pode ser definida como
/// UNKNOWN_LOCK. Veja o comentário acima do #define de UNKNOWN_LOCK para detalhes.
///
/// changeCountDone: Esta variável booleana é usada para garantir que o campo
/// de contador de mudanças (o campo de cabeçalho de 4 bytes no deslocamento de
/// byte 24 do arquivo de banco de dados) não seja atualizado mais frequentemente
/// que o necessário.
/// É definida como verdade quando o campo de contador de mudanças é atualizado,
/// o que só pode acontecer se um lock exclusivo for mantido no arquivo de banco.
/// É limpa (definida como falsa) sempre que um lock exclusivo for liberado no
/// arquivo de banco. Cada vez que uma transação é confirmada, a flag
/// changeCountDone é inspecionada. Se for verdade, o trabalho de atualizar o
/// contador de mudanças é omitido para a transação atual.
/// Este mecanismo significa que ao executar em modo exclusivo, uma conexão só
/// precisa atualizar o contador de mudanças uma vez, para a primeira transação
/// confirmada.
///
/// setSuper: Quando PagerCommitPhaseOne() é chamado para confirmar uma transação,
/// ele pode (ou não) especificar um nome de super-journal a ser escrito no arquivo
/// de journal antes de ser sincronizado com o disco.
/// Se um arquivo de journal contém um ponteiro de super-journal ou não afeta
/// a forma como o arquivo de journal é finalizado depois que a transação é
/// confirmada ou desfeita ao executar em modo "journal_mode=PERSIST".
/// Se um arquivo de journal não contém um ponteiro de super-journal, é finalizado
/// sobrescrevendo o cabeçalho de journal anterior com zeros. Se ele contém um
/// ponteiro de super-journal, o arquivo de journal é finalizado truncando-o para
/// zero bytes, exatamente como se a conexão estivesse executando em modo
/// "journal_mode=truncate".
/// Arquivos de journal que contêm ponteiros de super-journal não podem ser
/// finalizados simplesmente sobrescrevendo o cabeçalho de journal anterior com
/// zeros, pois o ponteiro de super-journal poderia interferir na reversão de
/// hot-journal de qualquer transação subsequente interrompida que reutilize o
/// arquivo de journal.
/// A flag é limpa assim que o arquivo de journal é finalizado (por
/// PagerCommitPhaseTwo ou PagerRollback). Se um erro de IO impedir que o arquivo
/// de journal seja finalizado com sucesso, a flag setSuper é limpa mesmo assim
/// (e o pager se moverá para o estado ERROR).
///
/// doNotSpill: Estas variáveis controlam o comportamento de cache-spills (chamadas
/// feitas pelo módulo pcache para a rotina pagerStress() para escrever dados em
/// cache para o sistema de arquivos a fim de liberar memória).
/// Quando os bits SPILLFLAG_OFF ou SPILLFLAG_ROLLBACK de doNotSpill são definidos,
/// a escrita no banco de dados de pagerStress() é desabilitada completamente.
/// O caso SPILLFLAG_ROLLBACK é feito em um caso muito obscuro que surge durante
/// reversão de savepoint que requer o módulo pcache para alocar uma página nova
/// para impedir que o arquivo de journal seja escrito enquanto está sendo percorrido
/// pelo código em pager_playback(). O caso SPILLFLAG_OFF é uma preferência do
/// usuário.
/// Se o bit SPILLFLAG_NOSYNC for definido, a escrita no banco de dados de
/// pagerStress() é permitida, mas sincronizar o arquivo de journal não.
/// Esta flag é definida por sqlite3PagerWrite() quando o tamanho de setor do
/// sistema de arquivos é maior que o tamanho de página do banco de dados a fim de
/// impedir uma sincronização de journal acontecendo entre o jornaling de duas
/// páginas no mesmo setor.
///
/// subjInMemory: Esta é uma variável booleana. Se verdade, então qualquer
/// sub-journal necessário é aberto como um arquivo de journal em memória.
/// Se falsa, sub-journals em memória só são usados para arquivos pager em memória.
/// Esta variável é atualizada pela camada superior cada vez que uma nova
/// transação de escrita é aberta.
///
/// dbSize, dbOrigSize, dbFileSize: A variável dbSize é definida como o número de
/// páginas no arquivo de banco de dados. É válida em estados PAGER_READER e
/// superiores (todos os estados exceto OPEN e ERROR).
/// dbSize é definida baseada no tamanho do arquivo de banco de dados, que pode
/// ser maior que o tamanho do banco (o valor armazenado no deslocamento de byte
/// 28 do cabeçalho do banco pela btree). Se o tamanho do arquivo não for um
/// múltiplo inteiro do tamanho da página, o valor armazenado em dbSize é arredondado
/// para baixo (isto é, um arquivo de 5KB com tamanho de página de 2K tem dbSize==2).
/// Exceto, qualquer arquivo que seja maior que 0 bytes em tamanho é considerado
/// como tendo pelo menos uma página. (isto é, um arquivo de 1KB com tamanho de
/// página de 2K leva a dbSize==1).
/// Durante uma transação de escrita, se páginas com números de página maiores que
/// dbSize são modificadas no cache, dbSize é atualizada em conformidade.
/// Da mesma forma, se o banco de dados é truncado usando PagerTruncateImage(),
/// dbSize é atualizado.
/// As variáveis dbOrigSize e dbFileSize são válidas nos estados PAGER_WRITER_LOCKED
/// e superiores. dbOrigSize é uma cópia da variável dbSize no início da transação.
/// É usada durante reversão e para determinar se páginas precisam ser jornalizadas
/// antes de serem modificadas.
/// Durante uma transação de escrita, dbFileSize contém o tamanho do arquivo no
/// disco em páginas. É definida como uma cópia de dbSize quando a transação de
/// escrita é aberta pela primeira vez, e atualizada quando chamadas VFS são feitas
/// para escrever ou truncar o arquivo de banco de dados no disco.
/// A única razão pela qual a variável dbFileSize é necessária é suprimir chamadas
/// desnecessárias para xTruncate() depois de confirmar uma transação. Se, quando
/// uma transação é confirmada, a variável dbFileSize indicar que o arquivo de banco
/// de dados é maior que a imagem do banco (Pager.dbSize), pager_truncate() é chamada.
/// A chamada pager_truncate() usa xFilesize() para medir o arquivo de banco de dados
/// no disco, e depois o trunca se necessário. dbFileSize não é usado ao desfazer uma
/// transação. Neste caso, pager_truncate() é chamada incondicionalmente (o que
/// significa que pode haver uma chamada para xFilesize() que não é estritamente
/// necessária). Em qualquer caso, pager_truncate() pode fazer com que o arquivo
/// se torne menor ou maior.
///
/// dbHintSize: A variável dbHintSize é usada para limitar o número de chamadas
/// feitas ao método VFS xFileControl(FCNTL_SIZE_HINT).
/// dbHintSize é definida como uma cópia da variável dbSize quando uma transação de
/// escrita é aberta (no mesmo tempo que dbFileSize e dbOrigSize). Se o método
/// xFileControl(FCNTL_SIZE_HINT) é chamado, dbHintSize é aumentada para o número
/// de páginas que correspondem à dica de tamanho passada para a chamada do método.
/// Veja pager_write_pagelist() para detalhes.
///
/// errCode: A variável Pager.errCode é apenas usada no estado PAGER_ERROR.
/// É definida como zero em todos os outros estados. No estado PAGER_ERROR,
/// Pager.errCode é sempre definida como SQLITE_FULL, SQLITE_IOERR ou um dos
/// subcódigos SQLITE_IOERR_XXX.
///
/// syncFlags, walSyncFlags: syncFlags é SQLITE_SYNC_NORMAL (0x02) ou
/// SQLITE_SYNC_FULL (0x03). syncFlags é usado para modo rollback. walSyncFlags
/// é usado para modo WAL e contém as flags usadas para sincronizar as operações
/// de checkpoint nos dois bits inferiores, e as flags de sincronização usadas
/// para confirmações de transação no arquivo WAL nos bits 0x04 e 0x08. Em outras
/// palavras, para obter as flags de sincronização corretas para operações de
/// checkpoint, use (walSyncFlags&0x03) e para obter as flags de sincronização
/// corretas para confirmação de transação, use ((walSyncFlags>>2)&0x03). Note
/// que com synchronous=NORMAL em modo WAL, confirmação de transação não é
/// sincronizada, o que significa que os bits 0x04 e 0x08 são ambos zero.
pub struct Pager {
    pub p_vfs: Option<Box<Sqlite3Vfs>>,
    pub exclusive_mode: u8,
    pub journal_mode: u8,
    pub use_journal: u8,
    pub no_sync: u8,
    pub full_sync: u8,
    pub extra_sync: u8,
    pub sync_flags: u8,
    pub wal_sync_flags: u8,
    pub temp_file: u8,
    pub no_lock: u8,
    pub read_only: u8,
    pub mem_db: u8,
    pub mem_vfs: u8,

    pub e_state: u8,
    pub e_lock: u8,
    pub change_count_done: u8,
    pub set_super: u8,
    pub do_not_spill: u8,
    pub subj_in_memory: u8,
    pub b_use_fetch: u8,
    pub has_held_shared_lock: u8,
    pub db_size: Pgno,
    pub db_orig_size: Pgno,
    pub db_file_size: Pgno,
    pub db_hint_size: Pgno,
    pub err_code: i32,
    pub n_rec: i32,
    pub cksum_init: u32,
    pub n_sub_rec: u32,
    pub p_in_journal: Option<Box<Bitvec>>,
    pub fd: Option<Box<Sqlite3File>>,
    pub jfd: Option<Box<Sqlite3File>>,
    pub sjfd: Option<Box<Sqlite3File>>,
    pub journal_off: i64,
    pub journal_hdr: i64,
    pub p_backup: Option<Box<SqliteBackup>>,
    pub a_savepoint: Option<Box<[PagerSavepoint]>>,
    pub n_savepoint: i32,
    pub i_data_version: u32,
    pub db_file_vers: [u8; 16],

    pub n_mmap_out: i32,
    pub sz_mmap: i64,
    pub p_mmap_freelist: Option<PgHdrRef>,

    pub n_extra: u16,
    pub n_reserve: i16,
    pub vfs_flags: u32,
    pub sector_size: u32,
    pub mx_pgno: Pgno,
    pub lck_pgno: Pgno,
    pub page_size: i64,
    pub journal_size_limit: i64,
    pub z_filename: Vec<u8>,
    pub z_journal: Vec<u8>,
    pub x_busy_handler: Option<fn(&dyn std::any::Any) -> i32>,
    pub p_busy_handler_arg: Option<Rc<dyn std::any::Any>>,
    pub a_stat: [u32; 4],
    pub x_reiniter: Option<fn(&mut DbPage)>,
    pub x_get: Option<fn(&mut Pager, Pgno, &mut Option<PgHdrRef>, i32) -> i32>,
    pub p_tmp_space: Option<Vec<u8>>,
    pub p_p_cache: Option<PCacheRef>,
    pub p_wal: Option<Box<Wal>>,
    pub z_wal: Vec<u8>,
}

/// Referência compartilhada a um Pager (ponteiro de volta das páginas é `Weak`).
pub type PagerRef = Rc<RefCell<Pager>>;

/// Devolve o descritor aberto de um campo `fd`/`jfd`/`sjfd` do Pager. O C só
/// chama o VFS depois de `assert( isOpen(fd) )`, então fechado aqui é bug.
#[inline]
pub fn open_file_mut(fd: &mut Option<Box<Sqlite3File>>) -> &mut Sqlite3File {
    fd.as_deref_mut().expect("descritor de arquivo fechado")
}

/// Índices para uso com Pager.aStat[]. O array Pager.aStat[] contém
/// os valores acessados ao passar SQLITE_DBSTATUS_CACHE_HIT, CACHE_MISS
/// ou CACHE_WRITE para sqlite3_db_status().
pub const PAGER_STAT_HIT: usize = 0;
pub const PAGER_STAT_MISS: usize = 1;
pub const PAGER_STAT_WRITE: usize = 2;
pub const PAGER_STAT_SPILL: usize = 3;

/// Arquivo de journal começa com a seguinte string mágica. Os dados foram obtidos
/// de /dev/random. É usado apenas como um teste de sanidade.
/// Desde a versão 2.8.0, o formato de journal contém informações adicionais de
/// teste de sanidade. Se a energia falhar enquanto o journal está sendo escrito,
/// dados de lixo semi-aleatórios podem aparecer no arquivo de journal depois que
/// a energia é restaurada. Se uma tentativa for feita para reverter o journal,
/// o banco de dados pode ser corrompido. As informações adicionais de teste de
/// sanidade é uma tentativa de descobrir o lixo no journal e ignorá-lo.
/// A informação adicional de teste de sanidade para o novo formato de journal
/// consiste em uma soma de verificação de 32 bits em cada página de dados.
/// A soma de verificação cobre tanto o número da página quanto os
/// pPager->pageSize bytes de dados da página. Este cksum é inicializado para um valor
/// aleatório de 32 bits que aparece no arquivo de journal logo após o cabeçalho.
/// O inicializador aleatório é importante, porque dados de lixo que aparecem no
/// final de um journal provavelmente é dados que uma vez estavam em outros
/// arquivos que foram agora deletados. Se os dados de lixo vieram de um arquivo
/// de journal obsoleto, as somas de verificação podem estar corretas. Mas ao
/// inicializar a soma de verificação para um valor aleatório que é diferente para
/// cada journal, minimizamos esse risco.
pub const A_JOURNAL_MAGIC: &[u8] = &[
    0xd9, 0xd5, 0x05, 0xf9, 0x20, 0xa1, 0x63, 0xd7,
];

/// O tamanho de cada registro de página no journal é dado pela seguinte macro.
#[inline]
pub fn journal_pg_sz(p_pager: &Pager) -> i64 {
    p_pager.page_size + 8
}

/// O tamanho do cabeçalho de journal para este pager. Normalmente é do mesmo
/// tamanho que um único setor de disco. Veja também setSectorSize().
#[inline]
pub fn journal_hdr_sz(p_pager: &Pager) -> u32 {
    p_pager.sector_size as u32
}

/// A macro MEMDB é verdadeira se estamos lidando com um banco de dados em memória.
/// Fazemos isso como uma macro para que se a macro SQLITE_OMIT_MEMORYDB for definida,
/// o valor de MEMDB será uma constante e o compilador otimizará o código que nunca
/// seria executado.
#[inline]
pub fn memdb(p_pager: &Pager) -> bool {
    p_pager.mem_db != 0
}

/// A macro USEFETCH é verdadeira se nos é permitido usar as interfaces xFetch e
/// xUnfetch para acessar o banco de dados usando I/O mapeado em memória.
#[inline]
pub fn usefetch(x: &Pager) -> bool {
    x.b_use_fetch != 0
}

/// O argumento para esta macro é um descritor de arquivo (tipo Sqlite3File).
/// Retorna 0 se não está aberto, ou não-zero (mas não 1) se estiver.
/// Isto é para que expressões possam ser escritas como:
/// if is_open(p_pager.jfd) { ...
/// em vez de:
/// if p_pager.jfd.is_some() { ...
#[inline]
pub fn is_open(p_fd: &Option<Box<Sqlite3File>>) -> bool {
    p_fd.is_some()
}

/// Retorna verdade se a página pgno pode ser lida diretamente do arquivo de banco
/// de dados pela camada b-tree. Este é o caso se:
/// * o arquivo de banco de dados está aberto,
/// * não há páginas sujas no cache, e
/// * a página desejada não está atualmente no arquivo wal.
pub fn pager_direct_read_ok(p_pager: &Pager, pgno: Pgno) -> i32 {
    if p_pager.fd.is_none() {
        return 0;
    }
    if p_cache_is_dirty(p_pager.p_p_cache.as_ref().expect("pcache")) {
        return 0;
    }
    if let Some(p_wal) = p_pager.p_wal.as_deref() {
        let mut i_read: u32 = 0;
        let _ = wal_find_frame(p_wal, pgno, &mut i_read);
        return (i_read == 0) as i32;
    }
    1
}


// ---- part_002.rs ----

// O `#ifndef NDEBUG` de assert_pager_state, o `#ifdef SQLITE_DEBUG` de
// print_pager_state e o de pageInJournal não existem no Debian: o sqliteInt.h
// define NDEBUG quando SQLITE_DEBUG está ausente, então essas três funções (e
// as chamadas a elas dentro de assert()) somem na compilação.

/// `pagerUseWal(x)`: verdadeiro quando o pager tem um arquivo WAL aberto.
#[inline]
pub fn pager_use_wal(x: &Pager) -> bool {
    x.p_wal.is_some()
}

/// Define o método Pager.xGet com a rotina apropriada para buscar conteúdo do pager.
/// `get_page_normal`, `get_page_error` e `get_page_mmap` são definidas adiante,
/// em outras partes de pager.c.
pub fn set_getter_method(p_pager: &mut Pager) {
    if p_pager.err_code != 0 {
        p_pager.x_get = Some(get_page_error);
    } else if usefetch(p_pager) {
        p_pager.x_get = Some(get_page_mmap);
    } else {
        p_pager.x_get = Some(get_page_normal);
    }
}

/// Retorna verdadeiro se é necessário escrever a página *pPg no sub-journal.
/// Uma página precisa ser escrita no sub-journal se existe um ou mais
/// savepoints abertos para os quais:
///
///   * o número da página é menor ou igual a PagerSavepoint.nOrig, e
///   * o bit correspondente ao número da página não está definido em
///     PagerSavepoint.pInSavepoint.
pub fn subj_requires_page(p_pg: &PgHdr) -> i32 {
    let p_pager_ref = match p_pg.p_pager.as_ref().and_then(|w| w.upgrade()) {
        Some(r) => r,
        None => return 0,
    };
    let mut guard = p_pager_ref.borrow_mut();
    let p_pager: &mut Pager = &mut guard;
    let pgno: Pgno = p_pg.pgno;
    let n_savepoint = p_pager.n_savepoint;
    let a_savepoint = match p_pager.a_savepoint.as_mut() {
        Some(a) => a,
        None => return 0,
    };
    let mut i: i32 = 0;
    while i < n_savepoint {
        let p = &a_savepoint[i as usize];
        if p.n_orig >= pgno
            && 0 == p
                .p_in_savepoint
                .as_deref()
                .map_or(0, |b| bitvec_test_not_null(b, pgno))
        {
            i += 1;
            while i < n_savepoint {
                a_savepoint[i as usize].b_truncate_on_release = 0;
                i += 1;
            }
            return 1;
        }
        i += 1;
    }
    0
}

/// Lê um inteiro de 32 bits do descritor de arquivo dado. Armazena o inteiro
/// lido em *pRes. Retorna SQLITE_OK se tudo funcionou, ou um código de erro se
/// algo deu errado.
///
/// Todos os valores são armazenados em disco como big-endian.
pub fn read_32bits(fd: &mut Sqlite3File, offset: i64, p_res: &mut u32) -> i32 {
    let mut ac = [0u8; 4];
    let rc = os_read(fd, &mut ac, offset);
    if rc == SQLITE_OK {
        *p_res = get_4byte(&ac);
    }
    rc
}

/// Escreve um inteiro de 32 bits em um buffer em ordem de bytes big-endian
/// (a macro `put32bits(A,B)` do C).
pub use crate::prelude::put_4byte as put_32bits;

/// Escreve um inteiro de 32 bits no descritor de arquivo dado. Retorna
/// SQLITE_OK em caso de sucesso ou um código de erro se algo deu errado.
pub fn write_32bits(fd: &mut Sqlite3File, offset: i64, val: u32) -> i32 {
    let mut ac = [0u8; 4];
    put_32bits(&mut ac, val);
    os_write(fd, &ac, offset)
}

/// Destrava o arquivo de banco de dados até o nível eLock, que deve ser
/// NO_LOCK ou SHARED_LOCK. Independente de a chamada a xUnlock() ter sucesso ou
/// não, define a variável Pager.eLock para refletir o (suposto) novo nível.
///
/// Exceto que, se Pager.eLock for UNKNOWN_LOCK quando esta função é chamada,
/// ela não é modificada. Veja o comentário acima do #define de UNKNOWN_LOCK
/// para uma explicação.
pub fn pager_unlock_db(p_pager: &mut Pager, e_lock: i32) -> i32 {
    let mut rc = SQLITE_OK;

    debug_assert!(p_pager.exclusive_mode == 0 || p_pager.e_lock as i32 == e_lock);
    debug_assert!(e_lock == NO_LOCK || e_lock == SHARED_LOCK);
    debug_assert!(e_lock != NO_LOCK || !pager_use_wal(p_pager));
    if let Some(fd) = p_pager.fd.as_deref_mut() {
        debug_assert!(p_pager.e_lock as i32 >= e_lock);
        rc = if p_pager.no_lock != 0 {
            SQLITE_OK
        } else {
            os_unlock(fd, e_lock)
        };
        if p_pager.e_lock as i32 != UNKNOWN_LOCK {
            p_pager.e_lock = e_lock as u8;
        }
    }
    // ticket fb3b3024ea238d5c
    p_pager.change_count_done = p_pager.temp_file;
    rc
}

/// Trava o arquivo de banco de dados no nível eLock, que deve ser SHARED_LOCK,
/// RESERVED_LOCK ou EXCLUSIVE_LOCK. Se a chamada tem sucesso, define a variável
/// Pager.eLock para o novo estado de travamento.
///
/// Exceto que, se Pager.eLock for UNKNOWN_LOCK quando esta função é chamada,
/// ela não é modificada a menos que o novo estado seja EXCLUSIVE_LOCK. Veja o
/// comentário acima do #define de UNKNOWN_LOCK para uma explicação.
pub fn pager_lock_db(p_pager: &mut Pager, e_lock: i32) -> i32 {
    let mut rc = SQLITE_OK;

    debug_assert!(e_lock == SHARED_LOCK || e_lock == RESERVED_LOCK || e_lock == EXCLUSIVE_LOCK);
    if (p_pager.e_lock as i32) < e_lock || p_pager.e_lock as i32 == UNKNOWN_LOCK {
        rc = if p_pager.no_lock != 0 {
            SQLITE_OK
        } else {
            match p_pager.fd.as_deref_mut() {
                Some(fd) => os_lock(fd, e_lock),
                None => SQLITE_IOERR,
            }
        };
        if rc == SQLITE_OK && (p_pager.e_lock as i32 != UNKNOWN_LOCK || e_lock == EXCLUSIVE_LOCK) {
            p_pager.e_lock = e_lock as u8;
        }
    }
    rc
}

/// Esta função determina se a otimização atomic-write ou atomic-batch-write
/// pode ser usada com este pager. Sem SQLITE_ENABLE_ATOMIC_WRITE nem
/// SQLITE_ENABLE_BATCH_ATOMIC_WRITE (o caso do Debian), nenhuma das duas pode
/// ser usada e o valor retornado é sempre 0.
///
/// Com a otimização atomic-write, o valor retornado seria o tamanho do arquivo
/// de journal quando ele contém dados de rollback para exatamente uma página;
/// com atomic-batch-write, -1.
pub fn jrnl_buffer_size(p_pager: &Pager) -> i32 {
    debug_assert!(!memdb(p_pager));
    let _ = p_pager;
    0
}


// ---- part_003.rs ----

// O bloco `#ifdef SQLITE_CHECK_PAGES` (pager_datahash, pager_pagehash,
// pager_set_pagehash e check_page) não existe no Debian: sem a macro, o C
// define `pager_datahash`, `pager_pagehash`, `pager_set_pagehash` e
// `CHECK_PAGE` como expressões vazias ou constantes, e as chamadas somem.

/// Tenta ler o nome do super-journal do final do arquivo de journal.
///
/// Quando esta função é chamada, o arquivo de journal deve estar aberto.
/// Esta função tenta ler um nome de arquivo super-journal do final do arquivo
/// de journal e, se bem-sucedida, copia para memória fornecida pelo chamador.
/// Veja comentários acima de write_super_journal() para o formato usado.
///
/// z_super deve ser um buffer de pelo menos n_super bytes alocado pelo
/// chamador. Este buffer deve ter mxPathname+1 bytes para garantir espaço
/// suficiente para o nome do super-journal. Se o nome do super-journal no
/// journal for mais longo que n_super bytes (incluindo o terminador nul),
/// isto é tratado como se nenhum super-journal estivesse presente.
///
/// Se um nome de super-journal estiver presente no final do arquivo de
/// journal, ele é copiado para z_super, seguido de um byte terminador nul.
///
/// Se nenhum nome de super-journal for encontrado, z_super[0] é definido
/// como 0 e SQLITE_OK é retornado.
///
/// Se um erro ocorrer ao ler do arquivo de journal, um código de erro SQLite
/// é retornado.
pub fn read_super_journal(p_jrnl: &mut Sqlite3File, z_super: &mut [u8], n_super: u32) -> i32 {
    let mut rc: i32;
    let mut len: u32 = 0;
    let mut sz_j: i64 = 0;
    let mut cksum: u32 = 0;
    let mut a_magic: [u8; 8] = [0; 8];
    z_super[0] = 0;

    // A cadeia de `||` do C devolve o último `rc` atribuído: SQLITE_OK quando
    // a saída vem de uma das condições que não são chamadas de E/S.
    rc = os_file_size(p_jrnl, &mut sz_j);
    if rc != SQLITE_OK || sz_j < 16 {
        return rc;
    }
    rc = read_32bits(p_jrnl, sz_j - 16, &mut len);
    if rc != SQLITE_OK || len >= n_super || (len as i64) > sz_j - 16 || len == 0 {
        return rc;
    }
    rc = read_32bits(p_jrnl, sz_j - 12, &mut cksum);
    if rc != SQLITE_OK {
        return rc;
    }
    rc = os_read(p_jrnl, &mut a_magic, sz_j - 8);
    if rc != SQLITE_OK || a_magic[..] != A_JOURNAL_MAGIC[..] {
        return rc;
    }
    rc = os_read(p_jrnl, &mut z_super[0..len as usize], sz_j - 16 - len as i64);
    if rc != SQLITE_OK {
        return rc;
    }

    // Vê se o checksum bate com o nome do super-journal. `char` é com sinal
    // no x86, então bytes acima de 0x7f entram subtraindo o valor estendido.
    for u in 0..len as usize {
        cksum = cksum.wrapping_sub(z_super[u] as i8 as i32 as u32);
    }
    if cksum != 0 {
        // Se o checksum não fecha, um ou mais setores com o nome do
        // super-journal estão corrompidos. Isso significa reverter
        // definitivamente, então retorna SQLITE_OK e um nome nul.
        len = 0;
    }
    z_super[len as usize] = 0;
    z_super[len as usize + 1] = 0;

    SQLITE_OK
}

/// Retorna o deslocamento do limite de setor na posição ou imediatamente após
/// o valor em pPager->journalOff, assumindo setor de pPager->sectorSize bytes.
///
/// Por exemplo, para tamanho de setor de 512:
///
/// ```text
///   Pager.journalOff       Valor retornado
///   ---------------------------------------
///   0                      0
///   512                    512
///   100                    512
///   2000                   2048
/// ```
pub fn journal_hdr_offset(p_pager: &Pager) -> i64 {
    let mut offset: i64 = 0;
    let c: i64 = p_pager.journal_off;
    let hdr_sz: i64 = journal_hdr_sz(p_pager) as i64;

    if c != 0 {
        offset = ((c - 1) / hdr_sz + 1) * hdr_sz;
    }

    debug_assert!(offset % hdr_sz == 0);
    debug_assert!(offset >= c);
    debug_assert!((offset - c) < hdr_sz);

    offset
}

/// Zera ou trunca o cabeçalho do arquivo de journal.
///
/// O arquivo de journal deve estar aberto quando esta função é chamada.
/// Esta é uma operação nula se o arquivo de journal não foi escrito
/// na transação atual (ou seja, se Pager.journalOff==0).
///
/// Se do_truncate for diferente de zero ou Pager.journalSizeLimit for 0,
/// trunca o arquivo de journal para zero bytes. Caso contrário, zera o
/// cabeçalho de 28 bytes no início do arquivo. Em ambos os casos, se o pager
/// não está em modo no-sync, sincroniza o journal logo após escrever ou truncar.
///
/// Se Pager.journalSizeLimit for positivo e, após a truncagem ou zeragem acima,
/// o arquivo de journal for maior que este valor, trunca-o para
/// Pager.journalSizeLimit bytes. O arquivo não precisa ser sincronizado
/// após esta operação.
///
/// Se um erro de E/S ocorrer, abandona o processamento e retorna o código de
/// erro. Caso contrário, retorna SQLITE_OK.
pub fn zero_journal_hdr(p_pager: &mut Pager, do_truncate: i32) -> i32 {
    let mut rc: i32 = SQLITE_OK;
    debug_assert!(is_open(&p_pager.jfd));
    debug_assert!(!journal_is_in_memory(open_file_mut(&mut p_pager.jfd)));
    if p_pager.journal_off != 0 {
        let i_limit: i64 = p_pager.journal_size_limit;

        if do_truncate != 0 || i_limit == 0 {
            rc = os_truncate(open_file_mut(&mut p_pager.jfd), 0);
        } else {
            const ZERO_HDR: [u8; 28] = [0; 28];
            rc = os_write(open_file_mut(&mut p_pager.jfd), &ZERO_HDR, 0);
        }
        if rc == SQLITE_OK && p_pager.no_sync == 0 {
            rc = os_sync(
                open_file_mut(&mut p_pager.jfd),
                SQLITE_SYNC_DATAONLY | p_pager.sync_flags as i32,
            );
        }

        // Neste ponto a transação está confirmada, mas o bloqueio de escrita
        // ainda é mantido. Se há limite de tamanho para o journal persistente
        // e o arquivo consome mais que isso, trunca agora, sem sincronizar.
        if rc == SQLITE_OK && i_limit > 0 {
            let mut sz: i64 = 0;
            rc = os_file_size(open_file_mut(&mut p_pager.jfd), &mut sz);
            if rc == SQLITE_OK && sz > i_limit {
                rc = os_truncate(open_file_mut(&mut p_pager.jfd), i_limit);
            }
        }
    }
    rc
}

/// O arquivo de journal deve estar aberto quando esta rotina é chamada. Um
/// cabeçalho de journal (JOURNAL_HDR_SZ bytes) é escrito no arquivo de
/// journal na posição atual.
///
/// O formato do cabeçalho do journal é o seguinte:
/// - 8 bytes: Magic identificando o formato do journal.
/// - 4 bytes: Número de registros no journal, ou -1 se modo no-sync está ativo.
/// - 4 bytes: Número aleatório usado para hash de página.
/// - 4 bytes: Contagem de páginas inicial do banco de dados.
/// - 4 bytes: Tamanho de setor usado pelo processo que escreveu este journal.
/// - 4 bytes: Tamanho de página do banco de dados.
///
/// Seguido de (JOURNAL_HDR_SZ - 28) bytes de espaço não utilizado.
pub fn write_journal_hdr(p_pager: &mut Pager) -> i32 {
    let mut rc: i32 = SQLITE_OK;
    // O espaço temporário sai do Pager durante a escrita e volta no fim, para
    // não segurar um empréstimo do pager inteiro.
    let mut z_header: Vec<u8> = p_pager.p_tmp_space.take().expect("p_tmp_space");
    let mut n_header: u32 = p_pager.page_size as u32;
    let mut n_write: u32;

    debug_assert!(is_open(&p_pager.jfd));

    if n_header > journal_hdr_sz(p_pager) {
        n_header = journal_hdr_sz(p_pager);
    }

    // Se há savepoints ativos e algum foi criado desde o último cabeçalho de
    // journal escrito, atualiza os campos PagerSavepoint.iHdrOffset agora.
    let journal_off = p_pager.journal_off;
    if let Some(a_savepoint) = p_pager.a_savepoint.as_mut() {
        for ii in 0..p_pager.n_savepoint as usize {
            if a_savepoint[ii].i_hdr_offset == 0 {
                a_savepoint[ii].i_hdr_offset = journal_off;
            }
        }
    }

    p_pager.journal_off = journal_hdr_offset(p_pager);
    p_pager.journal_hdr = p_pager.journal_off;

    // Escreve o campo nRec: o número de registros de página que seguem este
    // cabeçalho. Normalmente grava zero, depois sobrescrito com o número real
    // (veja sync_journal()). A alternativa mais rápida é 0xFFFFFFFF, segura
    // em modo no-sync ou com SQLITE_IOCAP_SAFE_APPEND.
    debug_assert!(is_open(&p_pager.fd) || p_pager.no_sync != 0);
    let device_chars = p_pager.fd.as_deref().map_or(0, os_device_characteristics);
    if p_pager.no_sync != 0
        || p_pager.journal_mode == PAGER_JOURNALMODE_MEMORY
        || (device_chars & SQLITE_IOCAP_SAFE_APPEND) != 0
    {
        z_header[0..8].copy_from_slice(A_JOURNAL_MAGIC);
        put_32bits(&mut z_header[8..12], 0xffffffff);
    } else {
        z_header[0..12].fill(0);
    }

    // O inicializador aleatório do hash de verificação.
    if p_pager.journal_mode != PAGER_JOURNALMODE_MEMORY {
        let mut a_rand = [0u8; 4];
        api::randomness(&mut a_rand);
        p_pager.cksum_init = u32::from_ne_bytes(a_rand);
    }
    put_32bits(&mut z_header[12..16], p_pager.cksum_init);

    // O tamanho inicial do banco de dados.
    put_32bits(&mut z_header[16..20], p_pager.db_orig_size);
    // O tamanho de setor assumido por este processo.
    put_32bits(&mut z_header[20..24], p_pager.sector_size);
    // O tamanho de página.
    put_32bits(&mut z_header[24..28], p_pager.page_size as u32);

    // Inicializar o resto do buffer não é necessário, mas evita reclamação
    // do valgrind, então o custo é aceito.
    z_header[28..n_header as usize].fill(0);

    // Em teoria bastaria escrever os 28 bytes do cabeçalho e avançar
    // journalOff por JOURNAL_HDR_SZ. Em alguns sistemas isso é bem mais lento
    // que escrever contiguamente, então os bytes não usados também são
    // escritos. O laço é necessário caso o setor seja maior que a página, pois
    // o buffer tem só Pager.pageSize bytes.
    n_write = 0;
    while rc == SQLITE_OK && n_write < journal_hdr_sz(p_pager) {
        rc = os_write(
            open_file_mut(&mut p_pager.jfd),
            &z_header[0..n_header as usize],
            p_pager.journal_off,
        );
        debug_assert!(p_pager.journal_hdr <= p_pager.journal_off);
        p_pager.journal_off += n_header as i64;
        n_write += n_header;
    }

    p_pager.p_tmp_space = Some(z_header);
    rc
}

/// O arquivo de journal deve estar aberto quando isto é chamado. Um cabeçalho
/// de journal de JOURNAL_HDR_SZ bytes é lido da posição atual do journal, dada
/// por pPager->journalOff. Veja os comentários acima de write_journal_hdr()
/// para a descrição do formato.
///
/// Se o cabeçalho for lido com sucesso, *p_n_rec é definido como o número de
/// registros de página após este cabeçalho e *p_db_size como o tamanho do banco
/// de dados antes da transação começar, em páginas. Além disso,
/// pPager->cksumInit recebe o valor lido do cabeçalho. SQLITE_OK é retornado.
///
/// Se o cabeçalho parecer corrompido, SQLITE_DONE é retornado e *p_n_rec e
/// *p_db_size ficam indefinidos. Se JOURNAL_HDR_SZ bytes não puderem ser lidos
/// do arquivo de journal, um código de erro é retornado.
pub fn read_journal_hdr(
    p_pager: &mut Pager,
    is_hot: i32,
    journal_size: i64,
    p_n_rec: &mut u32,
    p_db_size: &mut u32,
) -> i32 {
    let mut rc: i32;
    let mut a_magic: [u8; 8] = [0; 8];
    let i_hdr_off: i64;

    debug_assert!(is_open(&p_pager.jfd));

    // Avança Pager.journalOff para o início do próximo setor. Se o journal é
    // pequeno demais para haver um cabeçalho neste ponto, retorna SQLITE_DONE.
    p_pager.journal_off = journal_hdr_offset(p_pager);
    if p_pager.journal_off + journal_hdr_sz(p_pager) as i64 > journal_size {
        return SQLITE_DONE;
    }
    i_hdr_off = p_pager.journal_off;

    // Lê os 8 primeiros bytes do cabeçalho. Se não casam com a string mágica,
    // retorna SQLITE_DONE. Se ocorre erro de E/S, retorna o código de erro.
    if is_hot != 0 || i_hdr_off != p_pager.journal_hdr {
        rc = os_read(open_file_mut(&mut p_pager.jfd), &mut a_magic, i_hdr_off);
        if rc != 0 {
            return rc;
        }
        if a_magic[..] != A_JOURNAL_MAGIC[..] {
            return SQLITE_DONE;
        }
    }

    // Lê os três primeiros campos de 32 bits: nRec, o inicializador do
    // checksum e o tamanho do banco no início da transação.
    rc = read_32bits(open_file_mut(&mut p_pager.jfd), i_hdr_off + 8, p_n_rec);
    if rc != SQLITE_OK {
        return rc;
    }
    rc = read_32bits(
        open_file_mut(&mut p_pager.jfd),
        i_hdr_off + 12,
        &mut p_pager.cksum_init,
    );
    if rc != SQLITE_OK {
        return rc;
    }
    rc = read_32bits(open_file_mut(&mut p_pager.jfd), i_hdr_off + 16, p_db_size);
    if rc != SQLITE_OK {
        return rc;
    }

    if p_pager.journal_off == 0 {
        let mut i_page_size: u32 = 0;
        let mut i_sector_size: u32 = 0;

        // Lê os campos de tamanho de página e de setor do cabeçalho.
        rc = read_32bits(open_file_mut(&mut p_pager.jfd), i_hdr_off + 20, &mut i_sector_size);
        if rc != SQLITE_OK {
            return rc;
        }
        rc = read_32bits(open_file_mut(&mut p_pager.jfd), i_hdr_off + 24, &mut i_page_size);
        if rc != SQLITE_OK {
            return rc;
        }

        // Versões do SQLite anteriores à 3.5.8 gravavam zero no campo de
        // tamanho de página. Nesse caso, assume que Pager.pageSize já está certo.
        if i_page_size == 0 {
            i_page_size = p_pager.page_size as u32;
        }

        // Confere se os valores estão no intervalo: potências de dois, maiores
        // ou iguais a 512 (página) e 32 (setor), e dentro dos máximos.
        if i_page_size < 512
            || i_sector_size < 32
            || i_page_size > SQLITE_MAX_PAGE_SIZE as u32
            || i_sector_size > MAX_SECTOR_SIZE
            || ((i_page_size - 1) & i_page_size) != 0
            || ((i_sector_size - 1) & i_sector_size) != 0
        {
            // Se o tamanho de página ou de setor do cabeçalho é inválido, o
            // processo que o escreveu caiu antes de sincronizá-lo. Para a
            // leitura do journal aqui.
            return SQLITE_DONE;
        }

        // Atualiza o tamanho de página para o valor lido do journal.
        rc = pager_set_pagesize(p_pager, &mut i_page_size, -1);

        // Atualiza o tamanho de setor assumido para o do processo que criou o
        // journal. Se foi outro processo, esta rotina roda dentro de
        // pager_playback(), que restaura o valor local de Pager.sectorSize.
        p_pager.sector_size = i_sector_size;
    }

    p_pager.journal_off += journal_hdr_sz(p_pager) as i64;
    rc
}


// ---- part_004.rs ----

/// Escreve o nome do super-journal no arquivo de journal do pager, na posição
/// atual. O nome do super-journal deve ser a última coisa escrita no arquivo de
/// journal. Se o pager está em modo full-sync, o descritor do journal avança até
/// a próxima fronteira de setor antes de qualquer escrita. O formato é:
///
///   + 4 bytes: PAGER_SJ_PGNO.
///   + N bytes: nome do arquivo super-journal em utf-8.
///   + 4 bytes: N (tamanho do nome em bytes, sem terminador nul).
///   + 4 bytes: checksum do nome do super-journal.
///   + 8 bytes: aJournalMagic[].
///
/// O checksum é a soma dos bytes do nome, cada byte interpretado como inteiro
/// de 8 bits com sinal.
///
/// Se `z_super` é None (ocorre em transação de um único banco), a chamada não
/// faz nada.
pub fn write_super_journal(p_pager: &mut Pager, z_super: Option<&[u8]>) -> i32 {
    let mut rc: i32;
    let n_super: usize;
    let i_hdr_off: i64;
    let mut jrnl_size: i64 = 0;
    let mut cksum: u32 = 0;

    debug_assert!(p_pager.set_super == 0);
    debug_assert!(!pager_use_wal(p_pager));

    let z_super = match z_super {
        Some(z) => z,
        None => return SQLITE_OK,
    };
    if p_pager.journal_mode as i32 == PAGER_JOURNALMODE_MEMORY || !is_open(&p_pager.jfd) {
        return SQLITE_OK;
    }
    p_pager.set_super = 1;
    debug_assert!(p_pager.journal_hdr <= p_pager.journal_off);

    // Calcula o tamanho em bytes e o checksum de z_super (o char do C tem sinal)
    let mut len = 0usize;
    while len < z_super.len() && z_super[len] != 0 {
        cksum = cksum.wrapping_add(z_super[len] as i8 as i32 as u32);
        len += 1;
    }
    n_super = len;

    // Em modo full-sync, avança até o próximo setor do disco antes de escrever
    // o nome do super-journal. Isso previne o caso em que a página anterior
    // escrita no journal já foi sincronizada.
    if p_pager.full_sync != 0 {
        p_pager.journal_off = journal_hdr_offset(p_pager);
    }
    i_hdr_off = p_pager.journal_off;

    // Escreve os dados do super-journal no fim do arquivo de journal. Se ocorrer
    // um erro, devolve o código ao chamador.
    let sj_pgno = pager_sj_pgno(p_pager);
    let jfd = p_pager.jfd.as_deref_mut().unwrap();
    rc = write_32bits(jfd, i_hdr_off, sj_pgno);
    if rc != 0 {
        return rc;
    }
    rc = os_write(jfd, &z_super[..n_super], i_hdr_off + 4);
    if rc != 0 {
        return rc;
    }
    rc = write_32bits(jfd, i_hdr_off + 4 + n_super as i64, n_super as u32);
    if rc != 0 {
        return rc;
    }
    rc = write_32bits(jfd, i_hdr_off + 4 + n_super as i64 + 4, cksum);
    if rc != 0 {
        return rc;
    }
    rc = os_write(jfd, &A_JOURNAL_MAGIC[..8], i_hdr_off + 4 + n_super as i64 + 8);
    if rc != 0 {
        return rc;
    }
    p_pager.journal_off += n_super as i64 + 20;

    // Se o pager está em modo de journal persistente, o arquivo físico pode
    // se estender além do nome do super-journal e dos 8 bytes mágicos recém
    // escritos. Isso é perigoso porque o código que reverte um hot-journal não
    // acharia o nome do super-journal para decidir se o journal está quente.
    // O mais fácil nesse cenário é truncar o arquivo para o tamanho exigido.
    let jfd = p_pager.jfd.as_deref_mut().unwrap();
    rc = os_file_size(jfd, &mut jrnl_size);
    if rc == SQLITE_OK && jrnl_size > p_pager.journal_off {
        rc = os_truncate(jfd, p_pager.journal_off);
    }
    rc
}

/// Descarta todo o conteúdo do cache de páginas em memória.
pub fn pager_reset(p_pager: &mut Pager) {
    p_pager.i_data_version = p_pager.i_data_version.wrapping_add(1);
    backup_restart(p_pager.p_backup.as_ref());
    pcache_clear(&mut p_pager.p_p_cache.as_ref().unwrap().borrow_mut());
}

/// Devolve o valor de pPager->iDataVersion.
pub fn pager_data_version(p_pager: &Pager) -> u32 {
    p_pager.i_data_version
}

/// Libera todas as estruturas do array Pager.aSavepoint[] e zera Pager.aSavepoint
/// e Pager.nSavepoint. Fecha o sub-journal se estiver aberto e o pager não
/// estiver em modo exclusivo.
pub fn release_all_savepoints(p_pager: &mut Pager) {
    if let Some(a_savepoint) = p_pager.a_savepoint.as_mut() {
        let mut ii = 0i32;
        while ii < p_pager.n_savepoint {
            bitvec_destroy(a_savepoint[ii as usize].p_in_savepoint.take());
            ii += 1;
        }
    }
    if p_pager.exclusive_mode == 0 || journal_is_in_memory(p_pager.sjfd.as_deref().unwrap()) != 0 {
        os_close(p_pager.sjfd.as_deref_mut().unwrap());
    }
    p_pager.a_savepoint = None;
    p_pager.n_savepoint = 0;
    p_pager.n_sub_rec = 0;
}

/// Define o bit pgno nos bitvecs PagerSavepoint.pInSavepoint de todos os
/// savepoints abertos. Devolve SQLITE_OK se tiver sucesso ou SQLITE_NOMEM se
/// uma alocação falhar.
pub fn add_to_savepoint_bitvecs(p_pager: &mut Pager, pgno: Pgno) -> i32 {
    let mut rc = SQLITE_OK;
    let n_savepoint = p_pager.n_savepoint;
    if let Some(a_savepoint) = p_pager.a_savepoint.as_mut() {
        let mut ii = 0i32;
        while ii < n_savepoint {
            let p = &mut a_savepoint[ii as usize];
            if pgno <= p.n_orig {
                rc |= bitvec_set(p.p_in_savepoint.as_deref_mut(), pgno);
                debug_assert!(rc == SQLITE_OK || rc == SQLITE_NOMEM);
            }
            ii += 1;
        }
    }
    rc
}

/// Esta função não faz nada se o pager está em modo exclusivo e fora do estado
/// ERROR. Caso contrário, leva o pager ao estado PAGER_OPEN.
///
/// Se o pager não está em modo de acesso exclusivo, o arquivo do banco de dados
/// é completamente destravado. Se o arquivo está destravado e o sistema de
/// arquivos não exibe a propriedade UNDELETABLE_WHEN_OPEN, o arquivo de journal
/// é fechado (se estiver aberto).
///
/// Se o pager está em estado ERROR quando esta função é chamada, o conteúdo do
/// cache é descartado antes de voltar ao estado OPEN. Seja o pager exclusivo
/// ou não, qualquer journal deixado no sistema de arquivos será tratado como
/// hot-journal e revertido na próxima vez que uma transação de leitura for
/// aberta (por esta ou por qualquer outra conexão).
pub fn pager_unlock(p_pager: &mut Pager) {
    debug_assert!(
        p_pager.e_state as i32 == PAGER_READER
            || p_pager.e_state as i32 == PAGER_OPEN
            || p_pager.e_state as i32 == PAGER_ERROR
    );

    bitvec_destroy(p_pager.p_in_journal.take());
    release_all_savepoints(p_pager);

    if pager_use_wal(p_pager) {
        debug_assert!(!is_open(&p_pager.jfd));
        wal_end_read_transaction(p_pager.p_wal.as_deref_mut().unwrap());
        p_pager.e_state = PAGER_OPEN as u8;
    } else if p_pager.exclusive_mode == 0 {
        let rc: i32; // Código de erro devolvido por pager_unlock_db()
        let i_dc: i32 = if is_open(&p_pager.fd) {
            os_device_characteristics(p_pager.fd.as_deref_mut().unwrap())
        } else {
            0
        };

        // Se o sistema operacional suporta apagar arquivos abertos, fecha o
        // arquivo de journal ao largar o bloqueio do banco. Caso contrário,
        // outra conexão com journal_mode=delete poderia apagar o arquivo debaixo
        // de nós.
        debug_assert!((PAGER_JOURNALMODE_MEMORY & 5) != 1);
        debug_assert!((PAGER_JOURNALMODE_OFF & 5) != 1);
        debug_assert!((PAGER_JOURNALMODE_WAL & 5) != 1);
        debug_assert!((PAGER_JOURNALMODE_DELETE & 5) != 1);
        debug_assert!((PAGER_JOURNALMODE_TRUNCATE & 5) == 1);
        debug_assert!((PAGER_JOURNALMODE_PERSIST & 5) == 1);
        if 0 == (i_dc & SQLITE_IOCAP_UNDELETABLE_WHEN_OPEN) || 1 != (p_pager.journal_mode as i32 & 5) {
            if let Some(jfd) = p_pager.jfd.as_deref_mut() {
                os_close(jfd);
            }
        }

        // Se o pager está no estado ERROR e a chamada para destravar o arquivo
        // do banco falha, define o bloqueio atual como UNKNOWN_LOCK. Veja o
        // comentário acima do #define de UNKNOWN_LOCK para a explicação.
        rc = pager_unlock_db(p_pager, NO_LOCK);
        if rc != SQLITE_OK && p_pager.e_state as i32 == PAGER_ERROR {
            p_pager.e_lock = UNKNOWN_LOCK as u8;
        }

        // O estado do pager pode mudar de PAGER_ERROR para PAGER_OPEN aqui sem
        // limpar o código de erro. Isso é intencional: o código de erro é limpo
        // e o cache resetado no bloco abaixo.
        debug_assert!(p_pager.err_code != 0 || p_pager.e_state as i32 != PAGER_ERROR);
        p_pager.e_state = PAGER_OPEN as u8;
    }

    // Se Pager.errCode está definido, o conteúdo do cache não é confiável.
    // Agora que não há referências pendentes ao pager, ele pode voltar com
    // segurança ao estado PAGER_OPEN. Isso acontece tanto no modo de bloqueio
    // normal quanto no exclusivo.
    debug_assert!(p_pager.err_code == SQLITE_OK || p_pager.mem_db == 0);
    if p_pager.err_code != 0 {
        if p_pager.temp_file == 0 {
            pager_reset(p_pager);
            p_pager.change_count_done = 0;
            p_pager.e_state = PAGER_OPEN as u8;
        } else {
            p_pager.e_state = if is_open(&p_pager.jfd) {
                PAGER_OPEN as u8
            } else {
                PAGER_READER as u8
            };
        }
        if usefetch(p_pager) {
            os_unfetch(p_pager.fd.as_deref_mut().unwrap(), 0);
        }
        p_pager.err_code = SQLITE_OK;
        set_getter_method(p_pager);
    }

    p_pager.journal_off = 0;
    p_pager.journal_hdr = 0;
    p_pager.set_super = 0;
}

/// Esta função é chamada sempre que um erro IOERR ou FULL que exige a transição
/// do pager para o estado ERROR pode ter ocorrido. O primeiro argumento é o
/// pager, o segundo o código de erro prestes a ser devolvido por uma função da
/// API do pager. O valor devolvido é uma cópia do segundo argumento.
///
/// Se o segundo argumento é SQLITE_FULL, SQLITE_IOERR ou um dos subcódigos de
/// IOERR, o pager entra no estado ERROR e o código é guardado em Pager.errCode.
/// Enquanto o pager permanece no estado ERROR, todas as chamadas principais da
/// API devolvem imediatamente Pager.errCode.
///
/// O estado ERROR indica que o conteúdo do cache não é confiável. Esse estado
/// pode ser limpo descartando completamente o cache. Se havia uma transação
/// ativa quando o erro persistente ocorreu, o journal de rollback pode precisar
/// ser reproduzido para restaurar o arquivo do banco (como se fosse um
/// hot-journal).
pub fn pager_error(p_pager: &mut Pager, rc: i32) -> i32 {
    let rc2 = rc & 0xff;
    debug_assert!(rc == SQLITE_OK || p_pager.mem_db == 0);
    debug_assert!(
        p_pager.err_code == SQLITE_FULL
            || p_pager.err_code == SQLITE_OK
            || (p_pager.err_code & 0xff) == SQLITE_IOERR
    );
    if rc2 == SQLITE_FULL || rc2 == SQLITE_IOERR {
        p_pager.err_code = rc;
        p_pager.e_state = PAGER_ERROR as u8;
        set_getter_method(p_pager);
    }
    rc
}

/// A transação de escrita aberta em pPager está sendo confirmada (bCommit==1)
/// ou revertida (bCommit==0).
///
/// Devolve verdadeiro se, e somente se, todas as páginas sujas devem ser
/// descarregadas para o disco.
///
/// Regras:
///
///   *  Para bancos não TEMP, sempre sincroniza com o disco. Isso é necessário
///      para que as transações sejam duráveis.
///
///   *  Sincroniza um banco TEMP apenas em COMMIT (não em ROLLBACK) quando o
///      arquivo de apoio já foi criado (por um spill em pagerStress()) e
///      quando o número de páginas sujas em memória excede 25% do tamanho
///      total do cache.
pub fn pager_flush_on_commit(p_pager: &Pager, b_commit: i32) -> i32 {
    if p_pager.temp_file == 0 {
        return 1;
    }
    if b_commit == 0 {
        return 0;
    }
    if !is_open(&p_pager.fd) {
        return 0;
    }
    (pcache_percent_dirty(p_pager.p_p_cache.as_ref().unwrap()) >= 25) as i32
}

/// Esta rotina encerra uma transação. Uma transação normalmente termina com um
/// COMMIT ou um ROLLBACK. A rotina pode ser chamada após o rollback de um
/// hot-journal, ou se ocorre um erro ao abrir o journal ou ao escrever o
/// primeiríssimo cabeçalho de journal de uma transação de banco.
///
/// Esta rotina nunca é chamada no estado PAGER_ERROR. Se é chamada em
/// PAGER_NONE ou PAGER_SHARED e o bloqueio mantido é menos exclusivo que um
/// RESERVED, não faz nada.
///
/// Caso contrário, os savepoints ativos são liberados.
///
/// Se o arquivo de journal está aberto, ele é "finalizado". Depois de
/// finalizado, não é possível usá-lo para reverter uma transação, nem ele será
/// considerado hot-journal por esta ou por qualquer outra conexão. Como o
/// journal é finalizado depende de o pager estar em modo exclusivo e do modo de
/// journal atual (valor de Pager.journalMode):
///
///   journalMode==MEMORY
///     O descritor do journal é simplesmente fechado, o que destrói o journal
///     em memória.
///
///   journalMode==TRUNCATE
///     O arquivo de journal é truncado para zero bytes.
///
///   journalMode==PERSIST
///     Os primeiros 28 bytes do arquivo de journal são zerados. Isso invalida
///     o primeiro cabeçalho e, portanto, o arquivo inteiro. Um journal inválido
///     não pode ser revertido.
///
///   journalMode==DELETE
///     O arquivo de journal é fechado e apagado com sqlite3OsDelete().
///
///     Se o pager está em modo exclusivo, esse método nunca é usado. Em vez
///     disso, se o journalMode é DELETE e o pager é exclusivo, usa-se o método
///     descrito em journalMode==PERSIST.
///
/// Depois que o journal é finalizado, o pager passa ao estado PAGER_READER. Em
/// modo rollback não exclusivo, o bloqueio no arquivo é rebaixado para
/// SHARED_LOCK.
///
/// Devolve SQLITE_OK se não houver erro. Se ocorrer um erro em qualquer das
/// operações de E/S para finalizar o journal ou destravar o banco, o código de
/// erro de E/S é devolvido. Se a finalização do journal falha, o código ainda
/// tenta destravar o arquivo do banco se não estiver em modo exclusivo. Se o
/// destravamento também falha, devolve-se o código do primeiro erro encontrado
/// (o da finalização do journal).
pub fn pager_end_transaction(p_pager: &mut Pager, has_super: i32, b_commit: i32) -> i32 {
    let mut rc = SQLITE_OK; // Código de erro da finalização do journal
    let mut rc2 = SQLITE_OK; // Código de erro do destravamento do arquivo do banco

    // Não faz nada se o pager não tem uma transação de escrita aberta ou ao
    // menos um bloqueio RESERVED. A função pode ser chamada sem transação de
    // escrita ativa mas com bloqueio RESERVED ou maior em duas circunstâncias:
    //
    //   1. Após um rollback bem-sucedido de hot-journal, é chamada com
    //      eState==PAGER_NONE e eLock==EXCLUSIVE_LOCK.
    //
    //   2. Se uma conexão com locking_mode=exclusive mantendo um bloqueio
    //      EXCLUSIVE volta para locking_mode=normal e executa uma transação de
    //      leitura, a função é chamada com eState==PAGER_READER e
    //      eLock==EXCLUSIVE_LOCK quando a transação de leitura é fechada.
    debug_assert!(p_pager.e_state as i32 != PAGER_ERROR);
    if (p_pager.e_state as i32) < PAGER_WRITER_LOCKED && (p_pager.e_lock as i32) < RESERVED_LOCK {
        return SQLITE_OK;
    }

    release_all_savepoints(p_pager);
    if is_open(&p_pager.jfd) {
        debug_assert!(!pager_use_wal(p_pager));

        // Finaliza o arquivo de journal.
        if journal_is_in_memory(p_pager.jfd.as_deref().unwrap()) != 0 {
            os_close(p_pager.jfd.as_deref_mut().unwrap());
        } else if p_pager.journal_mode as i32 == PAGER_JOURNALMODE_TRUNCATE {
            if p_pager.journal_off == 0 {
                rc = SQLITE_OK;
            } else {
                rc = os_truncate(p_pager.jfd.as_deref_mut().unwrap(), 0);
                if rc == SQLITE_OK && p_pager.full_sync != 0 {
                    // Garante que o novo tamanho do arquivo seja escrito no
                    // inode imediatamente. Caso contrário o journal poderia
                    // ressuscitar após uma queda de energia e causar o rollback
                    // da última transação. Veja
                    // https://bugzilla.mozilla.org/show_bug.cgi?id=1072773
                    rc = os_sync(p_pager.jfd.as_deref_mut().unwrap(), p_pager.sync_flags as i32);
                }
            }
            p_pager.journal_off = 0;
        } else if p_pager.journal_mode as i32 == PAGER_JOURNALMODE_PERSIST
            || (p_pager.exclusive_mode != 0 && p_pager.journal_mode as i32 != PAGER_JOURNALMODE_WAL)
        {
            rc = zero_journal_hdr(p_pager, (has_super != 0 || p_pager.temp_file != 0) as i32);
            p_pager.journal_off = 0;
        } else {
            // Este ramo pode ser executado com Pager.journalMode==MEMORY se um
            // hot-journal acabou de ser revertido. Nesse caso o arquivo de
            // journal deve ser fechado e apagado. Se esta conexão escrever no
            // arquivo do banco, ela o fará com um journal em memória.
            let b_delete = p_pager.temp_file == 0;
            debug_assert!(journal_is_in_memory(p_pager.jfd.as_deref().unwrap()) == 0);
            debug_assert!(
                p_pager.journal_mode as i32 == PAGER_JOURNALMODE_DELETE
                    || p_pager.journal_mode as i32 == PAGER_JOURNALMODE_MEMORY
                    || p_pager.journal_mode as i32 == PAGER_JOURNALMODE_WAL
            );
            os_close(p_pager.jfd.as_deref_mut().unwrap());
            if b_delete {
                let p_vfs = p_pager.p_vfs.clone().unwrap();
                rc = os_delete(&*p_vfs, &p_pager.z_journal, p_pager.extra_sync as i32);
            }
        }
    }

    bitvec_destroy(p_pager.p_in_journal.take());
    p_pager.n_rec = 0;
    if rc == SQLITE_OK {
        if p_pager.mem_db != 0 || pager_flush_on_commit(p_pager, b_commit) != 0 {
            pcache_clean_all(p_pager.p_p_cache.as_ref().unwrap());
        } else {
            pcache_clear_writable(p_pager.p_p_cache.as_ref().unwrap());
        }
        pcache_truncate(p_pager.p_p_cache.as_ref().unwrap(), p_pager.db_size);
    }

    if pager_use_wal(p_pager) {
        // Larga o bloqueio de escrita do WAL, se houver. Além disso, se a
        // conexão estava em locking_mode=exclusive mas já não está, larga o
        // bloqueio EXCLUSIVE mantido no arquivo do banco.
        rc2 = wal_end_write_transaction(p_pager.p_wal.as_deref_mut().unwrap());
        debug_assert!(rc2 == SQLITE_OK);
    } else if rc == SQLITE_OK && b_commit != 0 && p_pager.db_file_size > p_pager.db_size {
        // Este ramo é tomado ao confirmar uma transação em modo rollback-journal
        // se o arquivo do banco em disco é maior que a imagem do banco. Neste
        // ponto o journal foi finalizado e a transação confirmada com sucesso,
        // mas o bloqueio EXCLUSIVE ainda é mantido. Então é seguro truncar o
        // arquivo do banco ao tamanho mínimo necessário.
        debug_assert!(p_pager.e_lock as i32 == EXCLUSIVE_LOCK);
        rc = pager_truncate(p_pager, p_pager.db_size);
    }

    if rc == SQLITE_OK && b_commit != 0 {
        rc = os_file_control(
            p_pager.fd.as_deref_mut().unwrap(),
            SQLITE_FCNTL_COMMIT_PHASETWO,
            None,
        );
        if rc == SQLITE_NOTFOUND {
            rc = SQLITE_OK;
        }
    }

    if p_pager.exclusive_mode == 0
        && (!pager_use_wal(p_pager) || wal_exclusive_mode(p_pager.p_wal.as_deref_mut().unwrap(), 0) != 0)
    {
        rc2 = pager_unlock_db(p_pager, SHARED_LOCK);
    }
    p_pager.e_state = PAGER_READER as u8;
    p_pager.set_super = 0;

    if rc == SQLITE_OK {
        rc2
    } else {
        rc
    }
}


// ---- part_005.rs ----

/// Executa um rollback se uma transação está ativa e destrava o arquivo do
/// banco de dados.
///
/// Se o pager já entrou no estado ERROR, não tenta o rollback neste momento. Em
/// vez disso, chama pager_unlock(). A chamada a pager_unlock() descarta todas as
/// páginas em memória, destrava o arquivo do banco e leva o pager de volta ao
/// estado OPEN. Se isso significa que sobrou um hot-journal no sistema de
/// arquivos, a próxima conexão que obtiver um bloqueio compartilhado no pager
/// (que pode ser esta) o reverterá.
///
/// Se o pager ainda não entrou no estado ERROR, mas ocorre um erro de E/S ou de
/// malloc durante um rollback, isso próprio fará o pager entrar no estado ERROR,
/// que será limpo pela chamada a pager_unlock(), como descrito acima.
pub fn pager_unlock_and_rollback(p_pager: &mut Pager) {
    if p_pager.e_state as i32 != PAGER_ERROR && p_pager.e_state as i32 != PAGER_OPEN {
        if p_pager.e_state as i32 >= PAGER_WRITER_LOCKED {
            begin_benign_malloc();
            pager_rollback(p_pager);
            end_benign_malloc();
        } else if p_pager.exclusive_mode == 0 {
            debug_assert!(p_pager.e_state as i32 == PAGER_READER);
            pager_end_transaction(p_pager, 0, 0);
        }
    } else if p_pager.e_state as i32 == PAGER_ERROR
        && p_pager.journal_mode as i32 == PAGER_JOURNALMODE_MEMORY
        && is_open(&p_pager.jfd)
    {
        // Caso especial para um ROLLBACK causado por erro de E/S com journal em
        // memória: é preciso reverter imediatamente, antes de o journal ser
        // fechado, porque depois de fechado todo o conteúdo é esquecido.
        let err_code = p_pager.err_code;
        let e_lock = p_pager.e_lock;
        p_pager.e_state = PAGER_OPEN as u8;
        p_pager.err_code = SQLITE_OK;
        p_pager.e_lock = EXCLUSIVE_LOCK as u8;
        pager_playback(p_pager, 1);
        p_pager.err_code = err_code;
        p_pager.e_lock = e_lock;
    }
    pager_unlock(p_pager);
}

/// O parâmetro a_data deve apontar para um buffer de pPager->pageSize bytes de
/// dados. Calcula e devolve um checksum baseado no conteúdo da página e no valor
/// atual de pPager->cksumInit.
///
/// Este não é um checksum de verdade. É apenas a soma do valor inicial aleatório
/// (pPager->cksumInit) com cada 200º byte dos dados da página, começando no
/// deslocamento (pPager->pageSize%200). Cada byte é interpretado como inteiro
/// de 8 bits sem sinal.
///
/// Mudar a fórmula deste checksum resulta num formato de arquivo de journal
/// incompatível.
///
/// Se a corrupção do journal ocorre por falha de energia, o cenário mais provável
/// é que uma ponta ou outra do registro seja alterada. É bem menos provável que
/// as duas pontas do registro estejam corretas e o meio corrompido. Assim, este
/// esquema de "checksum", embora rápido e simples, pega o tipo de corrupção mais
/// provável.
pub fn pager_cksum(p_pager: &Pager, a_data: &[u8]) -> u32 {
    let mut cksum = p_pager.cksum_init; // Valor de checksum a devolver
    let mut i = p_pager.page_size - 200; // Contador do laço
    while i > 0 {
        cksum = cksum.wrapping_add(a_data[i as usize] as u32);
        i -= 200;
    }
    cksum
}

/// Escolhe o descritor do journal principal (is_main_jrnl) ou do sub-journal.
fn playback_jfd(p_pager: &mut Pager, is_main_jrnl: i32) -> &mut Sqlite3File {
    if is_main_jrnl != 0 {
        p_pager.jfd.as_deref_mut().unwrap()
    } else {
        p_pager.sjfd.as_deref_mut().unwrap()
    }
}

/// Lê uma única página do arquivo de journal (se isMainJrnl==1) ou do
/// sub-journal (se isMainJrnl==0) e faz o playback dessa página. A página começa
/// no deslocamento *p_offset do arquivo. O valor *p_offset é aumentado até o
/// início da próxima página do journal.
///
/// O journal de rollback principal usa checksums, o journal de instrução não.
///
/// Se o número de página do registro lido do (sub-)journal é maior que o valor
/// atual de Pager.dbSize, o playback é pulado e SQLITE_OK é devolvido.
///
/// Se p_done não é None, ele é o registro das páginas já reproduzidas. Se a
/// página em *p_offset já foi reproduzida (o bit correspondente de p_done está
/// definido), o playback é pulado. Garante que o bit de p_done da página de
/// *p_offset esteja definido antes de retornar.
///
/// Se o registro de página é lido com sucesso do (sub-)journal e reproduzido,
/// SQLITE_OK é devolvido. Se ocorre um erro de E/S ao ler o registro ou ao
/// escrever no arquivo do banco, o código de erro de E/S é devolvido. Se os
/// dados são lidos com sucesso mas parecem corrompidos, SQLITE_DONE é devolvido.
/// Os dados são considerados corrompidos em duas circunstâncias:
///
///   * Se o número de página do registro é ilegal (0 ou PAGER_SJ_PGNO), ou
///   * Se o registro está sendo revertido do journal principal e o campo de
///     checksum não bate com o conteúdo do registro.
///
/// Nenhum desses dois cenários é possível durante o rollback de um savepoint.
///
/// Se é um rollback de savepoint, esta função pode ter de alocar memória
/// dinamicamente. Se for o caso e a alocação falhar, SQLITE_NOMEM é devolvido.
pub fn pager_playback_one_page(
    p_pager: &mut Pager,
    p_offset: &mut i64,
    p_done: Option<&mut Bitvec>,
    is_main_jrnl: i32,
    is_savepnt: i32,
) -> i32 {
    // aData é pPager->pTmpSpace. O buffer sai do pager durante a chamada e
    // volta ao final, para que o pager continue emprestável.
    let mut a_data = p_pager.p_tmp_space.take().expect("pTmpSpace deve estar alocado");
    let rc = pager_playback_one_page_inner(p_pager, &mut a_data, p_offset, p_done, is_main_jrnl, is_savepnt);
    p_pager.p_tmp_space = Some(a_data);
    rc
}

fn pager_playback_one_page_inner(
    p_pager: &mut Pager,
    a_data: &mut [u8],
    p_offset: &mut i64,
    mut p_done: Option<&mut Bitvec>,
    is_main_jrnl: i32,
    is_savepnt: i32,
) -> i32 {
    let mut rc: i32;
    let mut p_pg: Option<PgHdrRef>; // Uma página existente no cache
    let mut pgno: Pgno = 0; // O número de página de uma página no journal
    let mut cksum: u32 = 0; // Checksum usado para verificação de sanidade
    let is_synced: bool; // Verdadeiro se a página do journal está sincronizada
    let page_size = p_pager.page_size as usize;

    debug_assert!((is_main_jrnl & !1) == 0); // isMainJrnl é 0 ou 1
    debug_assert!((is_savepnt & !1) == 0); // isSavepnt é 0 ou 1
    debug_assert!(is_main_jrnl != 0 || p_done.is_some()); // pDone sempre usado em sub-journals
    debug_assert!(is_savepnt != 0 || p_done.is_none()); // pDone nunca usado fora de savepoint

    debug_assert!(!pager_use_wal(p_pager) || (is_main_jrnl == 0 && is_savepnt != 0));

    // Ou o estado é maior que PAGER_WRITER_CACHEMOD (rollback de transação ou de
    // savepoint a pedido do chamador) ou este é um rollback de hot-journal. Num
    // rollback de hot-journal, o pager está no estado OPEN e mantém um bloqueio
    // EXCLUSIVE. O rollback de hot-journal só lê do journal principal, não do
    // sub-journal.
    debug_assert!(
        p_pager.e_state as i32 >= PAGER_WRITER_CACHEMOD
            || (p_pager.e_state as i32 == PAGER_OPEN && p_pager.e_lock as i32 == EXCLUSIVE_LOCK)
    );
    debug_assert!(p_pager.e_state as i32 >= PAGER_WRITER_CACHEMOD || is_main_jrnl != 0);

    // Lê o número e os dados da página do arquivo de journal ou sub-journal.
    // Devolve um código de erro ao chamador se ocorrer um erro de E/S.
    rc = read_32bits(playback_jfd(p_pager, is_main_jrnl), *p_offset, &mut pgno);
    if rc != SQLITE_OK {
        return rc;
    }
    rc = os_read(playback_jfd(p_pager, is_main_jrnl), &mut a_data[..page_size], *p_offset + 4);
    if rc != SQLITE_OK {
        return rc;
    }
    *p_offset += p_pager.page_size + 4 + (is_main_jrnl as i64) * 4;

    // Verificação de sanidade da página. Isso é mais importante do que eu
    // pensava originalmente. Se ocorre uma falha de energia enquanto o journal é
    // escrito, dados inválidos podem ir parar no journal. É preciso detectar esses
    // dados inválidos (com alta probabilidade) e ignorá-los.
    if pgno == 0 || pgno == pager_sj_pgno(p_pager) {
        debug_assert!(is_savepnt == 0);
        return SQLITE_DONE;
    }
    if pgno > p_pager.db_size || bitvec_test(p_done.as_deref(), pgno) != 0 {
        return SQLITE_OK;
    }
    if is_main_jrnl != 0 {
        rc = read_32bits(playback_jfd(p_pager, is_main_jrnl), *p_offset - 4, &mut cksum);
        if rc != 0 {
            return rc;
        }
        if is_savepnt == 0 && pager_cksum(p_pager, a_data) != cksum {
            return SQLITE_DONE;
        }
    }

    // Se esta página já foi reproduzida durante o rollback atual, não há por que
    // reproduzi-la de novo.
    if p_done.is_some() {
        rc = bitvec_set(p_done.as_deref_mut(), pgno);
        if rc != SQLITE_OK {
            return rc;
        }
    }

    // Ao reproduzir a página 1, restaura a configuração nReserve
    if pgno == 1 && p_pager.n_reserve != a_data[20] as i16 {
        p_pager.n_reserve = a_data[20] as i16;
    }

    // Se o pager está no estado CACHEMOD, deve existir uma cópia desta página no
    // cache do pager. Neste caso basta atualizar o cache, não o arquivo do banco.
    // A página fica marcada como suja.
    //
    // Uma exceção à regra acima: se o banco está em modo no-sync e uma página é
    // movida durante um vacuum incremental, a página pode não estar no cache do
    // pager. Depois: se ocorre um erro de malloc() ou de E/S durante uma chamada
    // a Movepage(), a página também pode não estar no cache. Assim, a condição
    // descrita no parágrafo acima não é assertável.
    //
    // Se está no estado WRITER_DBMOD, WRITER_FINISHED ou OPEN, atualiza-se o
    // cache do pager, se existir, e o arquivo principal. A página é então marcada
    // como não suja. Como este código só executa no estado PAGER_OPEN para um
    // rollback de hot-journal, é garantido que o cache está vazio se o pager está
    // no estado OPEN.
    //
    // Ticket #1171: o journal de instrução pode conter conteúdo de página
    // diferente do conteúdo no início da transação. Isso ocorre quando uma página
    // é alterada antes do início de uma instrução e alterada de novo dentro da
    // instrução. Ao reverter tal instrução, não se deve escrever no banco original
    // a menos que se saiba com certeza que o conteúdo original da página está
    // sincronizado no journal de rollback principal. Caso contrário, uma perda de
    // energia poderia deixar dados modificados no arquivo do banco sem uma entrada
    // no journal de rollback capaz de restaurar o banco à forma original. Duas
    // condições devem ser atendidas antes de escrever nos arquivos do banco: (1) o
    // banco deve estar bloqueado; (2) sabe-se que o conteúdo original da página
    // está totalmente sincronizado no journal principal, seja porque a página não
    // está no cache, seja porque está marcada com needSync==0.
    //
    // 2008-04-14: ao tentar aspirar um arquivo de banco corrompido, é possível
    // falhar uma instrução num banco que ainda não existe. Não tenta escrever se o
    // arquivo do banco nunca foi aberto.
    if pager_use_wal(p_pager) {
        p_pg = None;
    } else {
        p_pg = pager_lookup(p_pager, pgno);
    }
    debug_assert!(p_pg.is_some() || p_pager.mem_db == 0);
    debug_assert!(p_pager.e_state as i32 != PAGER_OPEN || p_pg.is_none() || p_pager.temp_file != 0);

    if is_main_jrnl != 0 {
        is_synced = p_pager.no_sync != 0 || *p_offset <= p_pager.journal_hdr;
    } else {
        is_synced = match p_pg.as_ref() {
            None => true,
            Some(p) => 0 == (p.borrow().flags & PGHDR_NEED_SYNC),
        };
    }
    if is_open(&p_pager.fd)
        && (p_pager.e_state as i32 >= PAGER_WRITER_DBMOD || p_pager.e_state as i32 == PAGER_OPEN)
        && is_synced
    {
        let ofst: i64 = (pgno as i64 - 1) * p_pager.page_size;
        debug_assert!(!pager_use_wal(p_pager));

        // Escreve de volta no arquivo do banco os dados lidos do journal. Isso
        // costuma ser seguro mesmo para um banco criptografado, pois os dados
        // foram criptografados antes de ir para o arquivo de journal. A exceção
        // é se os dados acabaram de ser lidos de um sub-journal em memória. Nesse
        // caso eles precisam ser criptografados aqui antes de copiados para o
        // arquivo do banco.
        rc = os_write(p_pager.fd.as_deref_mut().unwrap(), &a_data[..page_size], ofst);

        if pgno > p_pager.db_file_size {
            p_pager.db_file_size = pgno;
        }
        if p_pager.p_backup.is_some() {
            backup_update(p_pager.p_backup.as_ref(), pgno, &a_data[..page_size]);
        }
    } else if is_main_jrnl == 0 && p_pg.is_none() {
        // Se este é o rollback de um savepoint, os dados não foram escritos no
        // banco e a página não está em memória, há um problema potencial. Quando
        // a página for buscada em seguida pela camada b-tree, será lida do
        // arquivo do banco, que pode ou não estar atual.
        //
        // Há algumas maneiras de isso acontecer, todas bastante obscuras. Em
        // modo síncrono, só pode acontecer se a página está na free-list no início
        // da transação, depois é populada e depois movida com
        // sqlite3PagerMovepage().
        //
        // A solução é adicionar ao cache uma página em memória contendo os dados
        // recém-lidos do sub-journal. Marca a página como suja e, se o pager exige
        // um journal-sync, marca a página como exigindo um journal-sync antes de
        // ser escrita.
        debug_assert!(is_savepnt != 0);
        debug_assert!((p_pager.do_not_spill & SPILLFLAG_ROLLBACK) == 0);
        p_pager.do_not_spill |= SPILLFLAG_ROLLBACK;
        rc = pager_get(p_pager, pgno, &mut p_pg, 1);
        debug_assert!((p_pager.do_not_spill & SPILLFLAG_ROLLBACK) != 0);
        p_pager.do_not_spill &= !SPILLFLAG_ROLLBACK;
        if rc != SQLITE_OK {
            return rc;
        }
        pcache_make_dirty(p_pg.as_ref().unwrap());
    }
    if let Some(p_pg_ref) = p_pg.take() {
        // Nenhuma página em uso deve jamais ser revertida explicitamente, exceto a
        // página 1, que é mantida em uso para manter o bloqueio no banco ativo.
        // Porém, tal página pode ser revertida como resultado de um erro interno
        // que leva a uma chamada automática a sqlite3PagerRollback().
        {
            let mut pg = p_pg_ref.borrow_mut();
            pg.p_data[..page_size].copy_from_slice(&a_data[..page_size]);
            let x_reiniter = p_pager.x_reiniter.expect("xReiniter deve estar definido");
            x_reiniter(&mut pg);
            // Antes, sqlite3PcacheMakeClean(pPg) era chamado aqui. Mas essa
            // chamada era perigosa e sem benefício detectável, já que o cache
            // normalmente é limpo por sqlite3PcacheCleanAll() após o rollback, e
            // por isso foi removida.

            // Se era a página 1, restaura o valor de Pager.dbFileVers. Faz isso
            // antes de qualquer decodificação.
            if pgno == 1 {
                let n = p_pager.db_file_vers.len();
                p_pager.db_file_vers.copy_from_slice(&pg.p_data[24..24 + n]);
            }
        }
        pcache_release(&p_pg_ref);
    }
    rc
}

/// O parâmetro z_super é o nome de um arquivo super-journal. Um único arquivo de
/// journal que se referia ao super-journal acabou de ser revertido. Esta rotina
/// verifica se é possível apagar o arquivo super-journal e o apaga, se for.
///
/// O argumento z_super pode apontar para Pager.pTmpSpace em C. Por isso aquele
/// buffer não está disponível para uso dentro desta função: o chamador passa uma
/// cópia do nome (terminado em nul ou até o fim da fatia).
///
/// Quando um arquivo super-journal é criado, é preenchido com os nomes de todos
/// os seus journals filhos, um após o outro, em texto codificado em utf-8. O fim
/// de cada nome de journal filho é marcado com um byte terminador nul (0x00).
/// Ou seja, o conteúdo inteiro de um super-journal para uma transação envolvendo
/// dois bancos pode ser:
///
///   "/home/bill/a.db-journal\x00/home/bill/b.db-journal\x00"
///
/// Um arquivo super-journal só pode ser apagado depois que todos os seus
/// journals filhos tenham sido revertidos.
///
/// Esta função lê o conteúdo do super-journal em memória e percorre cada nome de
/// journal filho. Para cada filho, verifica se:
///
///   * o journal filho existe e, se sim,
///   * o journal filho contém uma referência ao super-journal z_super
///
/// Se for encontrado um journal filho que atenda aos dois critérios, a função
/// retorna sem fazer nada. Caso contrário, se nenhum filho assim é encontrado, o
/// arquivo z_super é apagado do sistema de arquivos com sqlite3OsDelete().
///
/// Se ocorre um erro de E/S dentro desta função, o código de erro é devolvido.
/// A função aloca memória. Se uma alocação falhar, SQLITE_NOMEM é devolvido. Caso
/// contrário, se não ocorre erro de E/S nem de malloc, SQLITE_OK é devolvido.
///
/// TODO do C: esta função aloca um único bloco de memória para carregar todo o
/// conteúdo do super-journal. Isso pode ter alguns quilobytes ou mais,
/// potencialmente maior que o tamanho de página.
pub fn pager_delsuper(p_pager: &Pager, z_super: &[u8]) -> i32 {
    let p_vfs = p_pager.p_vfs.clone().unwrap();
    let mut rc: i32; // Código de retorno
    let mut p_super = Sqlite3File::default(); // Descritor do super-journal
    let mut p_journal = Sqlite3File::default(); // Descritor do journal filho
    let mut n_super_journal: i64 = 0; // Tamanho do arquivo super-journal
    let n_super_ptr: usize; // Espaço alocado para z_super_ptr[]

    // Compara apenas até o terminador nul de z_super, como o strcmp do C
    let z_super = &z_super[..strlen30(Some(z_super)) as usize];

    // Abre o arquivo super-journal para leitura. (Os dois descritores, pSuper e
    // pJournal, são objetos separados aqui, sem alocação única como no C.)
    let flags = SQLITE_OPEN_READONLY | SQLITE_OPEN_SUPER_JOURNAL;
    rc = os_open(&*p_vfs, Some(z_super), &mut p_super, flags, None);

    // delsuper_out é o bloco final: fecha o super-journal e devolve rc.
    macro_rules! delsuper_out {
        () => {{
            os_close(&mut p_super);
            debug_assert!(p_journal.p_methods.is_none());
            return rc;
        }};
    }

    if rc != SQLITE_OK {
        delsuper_out!();
    }

    // Carrega o arquivo super-journal inteiro em um buffer. Também obtém espaço
    // suficiente (z_super_ptr) para guardar os nomes de super-journal extraídos
    // dos journals de rollback comuns.
    rc = os_file_size(&mut p_super, &mut n_super_journal);
    if rc != SQLITE_OK {
        delsuper_out!();
    }
    n_super_ptr = p_vfs.mx_pathname() as usize + 1;
    let n_sj = n_super_journal as usize;
    // Layout do C: 4 bytes zero, depois o conteúdo, depois 2 bytes nul. Os 4
    // bytes zero antes do nome são exigidos por sqlite3OsOpen() em alguns VFS.
    let mut z_free: Vec<u8> = vec![0u8; 4 + n_sj + 2];
    let mut z_super_ptr: Vec<u8> = vec![0u8; n_super_ptr + 2];
    rc = os_read(&mut p_super, &mut z_free[4..4 + n_sj], 0);
    if rc != SQLITE_OK {
        delsuper_out!();
    }
    z_free[4 + n_sj] = 0;
    z_free[4 + n_sj + 1] = 0;

    let mut off: usize = 4; // zJournal: posição do nome atual dentro de z_free
    while off - 4 < n_sj {
        let mut exists: i32 = 0;
        let len = strlen30(Some(&z_free[off..])) as usize;
        rc = os_access(&*p_vfs, &z_free[off..off + len], SQLITE_ACCESS_EXISTS, &mut exists);
        if rc != SQLITE_OK {
            delsuper_out!();
        }
        if exists != 0 {
            // Um dos journals apontados pelo super-journal existe. Abre-o e
            // verifica se aponta para o super-journal. Se sim, retorna sem apagar
            // o arquivo super-journal.
            // NB: zJournal é na verdade um MAIN_JOURNAL. Mas aqui o chamamos de
            // SUPER_JOURNAL para que o VFS não envie o nome zJournal a
            // sqlite3_database_file_object().
            let flags = SQLITE_OPEN_READONLY | SQLITE_OPEN_SUPER_JOURNAL;
            rc = os_open(&*p_vfs, Some(&z_free[off..off + len]), &mut p_journal, flags, None);
            if rc != SQLITE_OK {
                delsuper_out!();
            }

            rc = read_super_journal(&mut p_journal, &mut z_super_ptr[..n_super_ptr], n_super_ptr as u32);
            os_close(&mut p_journal);
            if rc != SQLITE_OK {
                delsuper_out!();
            }

            let ptr_len = strlen30(Some(&z_super_ptr[..])) as usize;
            let c = z_super_ptr[0] != 0 && &z_super_ptr[..ptr_len] == z_super;
            if c {
                // Há uma correspondência. Não apaga o arquivo super-journal.
                delsuper_out!();
            }
        }
        off += len + 1;
    }

    os_close(&mut p_super);
    rc = os_delete(&*p_vfs, z_super, 0);

    delsuper_out!();
}


// ---- part_006.rs ----

/// Esta função é usada para mudar o tamanho real do arquivo do banco de dados no
/// sistema de arquivos. Isso só acontece ao confirmar uma transação ou ao
/// reverter uma transação (incluindo o rollback de um hot-journal).
///
/// Se o arquivo principal do banco não está aberto, ou o pager não está nem no
/// estado DBMOD nem no OPEN, esta função não faz nada. Caso contrário, o tamanho
/// do arquivo muda para n_page páginas (n_page*pPager->pageSize bytes). Se o
/// arquivo em disco é maior que n_page páginas, usa-se o método xTruncate() do
/// VFS para truncá-lo.
///
/// Também pode ser que o arquivo em disco seja menor que n_page páginas. Algumas
/// implementações de sistema operacional se confundem se você tenta truncar um
/// arquivo para um tamanho maior que o atual, então este caso é detectado e um
/// único byte zero é escrito no fim do novo arquivo.
///
/// Se tiver sucesso, devolve SQLITE_OK. Se ocorrer um erro de E/S ao modificar o
/// arquivo do banco, devolve o código de erro ao chamador.
pub fn pager_truncate(p_pager: &mut Pager, n_page: Pgno) -> i32 {
    let mut rc = SQLITE_OK;
    debug_assert!(p_pager.e_state as i32 != PAGER_ERROR);
    debug_assert!(p_pager.e_state as i32 != PAGER_READER);

    if is_open(&p_pager.fd)
        && (p_pager.e_state as i32 >= PAGER_WRITER_DBMOD || p_pager.e_state as i32 == PAGER_OPEN)
    {
        let mut current_size: i64 = 0;
        let mut new_size: i64;
        let sz_page = p_pager.page_size;
        debug_assert!(p_pager.e_lock as i32 == EXCLUSIVE_LOCK);
        // TODO do C: é seguro usar Pager.dbFileSize aqui?
        rc = os_file_size(p_pager.fd.as_deref_mut().unwrap(), &mut current_size);
        new_size = sz_page * (n_page as i64);
        if rc == SQLITE_OK && current_size != new_size {
            if current_size > new_size {
                rc = os_truncate(p_pager.fd.as_deref_mut().unwrap(), new_size);
            } else if (current_size + sz_page) <= new_size {
                let mut p_tmp = p_pager.p_tmp_space.take().expect("pTmpSpace deve estar alocado");
                p_tmp[..sz_page as usize].fill(0);
                os_file_control_hint(
                    p_pager.fd.as_deref_mut().unwrap(),
                    SQLITE_FCNTL_SIZE_HINT,
                    Some(&mut new_size),
                );
                rc = os_write(
                    p_pager.fd.as_deref_mut().unwrap(),
                    &p_tmp[..sz_page as usize],
                    new_size - sz_page,
                );
                p_pager.p_tmp_space = Some(p_tmp);
            }
            if rc == SQLITE_OK {
                p_pager.db_file_size = n_page;
            }
        }
    }
    rc
}

/// Devolve uma versão sanitizada do tamanho de setor do arquivo OS p_file. O
/// valor devolvido está garantidamente entre 32 e MAX_SECTOR_SIZE.
pub fn sector_size(p_file: &mut Sqlite3File) -> i32 {
    let mut i_ret = os_sector_size(p_file);
    if i_ret < 32 {
        i_ret = 512;
    } else if i_ret > MAX_SECTOR_SIZE as i32 {
        debug_assert!(MAX_SECTOR_SIZE >= 512);
        i_ret = MAX_SECTOR_SIZE as i32;
    }
    i_ret
}

/// Define o valor da variável Pager.sectorSize do pager dado com base no valor
/// devolvido pelo método xSectorSize do arquivo de banco aberto. O tamanho de
/// setor será usado para determinar o tamanho e o alinhamento do cabeçalho de
/// journal e dos ponteiros de super-journal nos arquivos de journal criados.
///
/// Para arquivos temporários, o tamanho de setor efetivo é sempre 512 bytes.
///
/// Caso contrário, para arquivos não temporários, o tamanho de setor efetivo é o
/// valor devolvido por xSectorSize() arredondado para cima até 32 se for menor
/// que 32, ou para baixo até MAX_SECTOR_SIZE se for maior que MAX_SECTOR_SIZE.
///
/// Se o arquivo tem a propriedade SQLITE_IOCAP_POWERSAFE_OVERWRITE, o tamanho de
/// setor efetivo é o mínimo (512). O propósito de pPager->sectorSize é definir o
/// "raio de explosão" de bytes que podem mudar se ocorre uma queda enquanto se
/// escreve um único byte naquela faixa. Mas com POWERSAFE_OVERWRITE o raio de
/// explosão é zero (é isso que POWERSAFE_OVERWRITE significa), então o tamanho de
/// setor é minimizado. Por compatibilidade retroativa do formato do journal de
/// rollback, não se pode reduzir o tamanho efetivo abaixo de 512.
pub fn set_sector_size(p_pager: &mut Pager) {
    debug_assert!(is_open(&p_pager.fd) || p_pager.temp_file != 0);

    if p_pager.temp_file != 0
        || (os_device_characteristics(p_pager.fd.as_deref_mut().unwrap())
            & SQLITE_IOCAP_POWERSAFE_OVERWRITE)
            != 0
    {
        // O tamanho de setor não importa para arquivos temporários. Além disso,
        // o arquivo pode ainda não ter sido aberto, caso em que a chamada a
        // OsSectorSize() daria segfault.
        p_pager.sector_size = 512;
    } else {
        p_pager.sector_size = sector_size(p_pager.fd.as_deref_mut().unwrap()) as u32;
    }
}

/// Reproduz o journal e assim restaura o arquivo do banco ao estado em que
/// estava antes de começarmos a fazer alterações.
///
/// O formato do arquivo de journal é o seguinte:
///
///  (1)  Prefixo de 8 bytes. Uma cópia de aJournalMagic[].
///  (2)  Inteiro big-endian de 4 bytes: o número de registros de página válidos
///       no journal. Se este valor é 0xffffffff, o número de registros é
///       calculado a partir do tamanho do journal.
///  (3)  Inteiro big-endian de 4 bytes: o valor inicial do checksum de sanidade.
///  (4)  Inteiro de 4 bytes: o número de páginas para o qual truncar o banco
///       durante um rollback.
///  (5)  Inteiro big-endian de 4 bytes: o tamanho de setor. O cabeçalho tem este
///       tamanho em bytes.
///  (6)  Inteiro big-endian de 4 bytes: o tamanho de página.
///  (7)  Preenchimento de zeros até o próximo tamanho de setor.
///  (8)  Zero ou mais instâncias de página, cada uma assim:
///        +  4 bytes de número de página.
///        +  pPager->pageSize bytes de dados.
///        +  4 bytes de checksum
///
/// Quando falamos do cabeçalho do journal, referimo-nos aos 7 primeiros itens.
/// Cada entrada do journal é uma instância do 8º item.
///
/// Chame de "nRec" o valor do segundo item. nRec é o número de entradas de
/// página válidas no journal. Na maioria dos casos, dá para calcular nRec a
/// partir do tamanho do arquivo. Mas se ocorreu uma falha de energia enquanto o
/// journal era escrito, o tamanho do arquivo pode já ter aumentado sem que as
/// entradas extras tenham chegado ao disco. Nesse caso, o nRec calculado a partir
/// do tamanho seria grande demais. Por isso sempre se usa o nRec do cabeçalho.
///
/// Se o valor de nRec é 0xffffffff, ele deve ser calculado a partir do tamanho do
/// arquivo. Esse valor é usado quando o usuário seleciona a opção no-sync para o
/// journal. Uma falha de energia pode levar a corrupção nesse caso. Mas para
/// coisas como tabelas temporárias (que serão apagadas quando a energia voltar),
/// isso não importa.
///
/// Se o arquivo aberto como journal não é um journal bem formado, todas as
/// páginas até a primeira página corrompida são revertidas (ou nenhuma se o
/// cabeçalho está corrompido). O arquivo de journal é então apagado e SQLITE_OK é
/// devolvido, como se nenhuma corrupção tivesse sido encontrada.
///
/// Se ocorre um erro de E/S ou de malloc(), o journal não é apagado e um código
/// de erro é devolvido.
///
/// O parâmetro is_hot indica que se tenta reverter um journal que pode ser um
/// hot-journal. Ou pode ser que o journal tenha sido preservado por causa de
/// JOURNALMODE_PERSIST ou JOURNALMODE_TRUNCATE. Se o journal realmente é quente,
/// o cache do pager é resetado antes de reverter qualquer conteúdo. Se o journal
/// é apenas persistente, não é preciso resetar.
pub fn pager_playback(p_pager: &mut Pager, is_hot: i32) -> i32 {
    let p_vfs = p_pager.p_vfs.clone().unwrap();
    let mut sz_j: i64 = 0; // Tamanho do arquivo de journal em bytes
    let mut n_rec: u32 = 0; // Número de registros no journal
    let mut mx_pg: Pgno = 0; // Tamanho do arquivo original em páginas
    let mut rc: i32; // Código de resultado de uma sub-rotina
    let mut res: i32 = 1; // Valor devolvido por sqlite3OsAccess()
    let need_pager_reset_init: i32; // Verdadeiro para resetar a página antes do primeiro rollback
    let mut n_playback: i32 = 0; // Total de páginas restauradas do journal
    let mut saved_page_size: u32 = p_pager.page_size as u32;
    let n_super_buf = p_vfs.mx_pathname() as usize + 1;
    // Buffer do nome do super-journal. No C é pPager->pTmpSpace (com 4 bytes de
    // folga zerados antes do nome na segunda leitura); aqui é um buffer próprio.
    let mut z_super: Vec<u8> = vec![0u8; n_super_buf];
    let mut z_super_read = false; // z_super foi lido na segunda leitura (pós end_playback)

    'end_playback: {
        // Descobre quantos registros há no journal. Aborta cedo se o journal
        // está vazio.
        debug_assert!(is_open(&p_pager.jfd));
        rc = os_file_size(p_pager.jfd.as_deref_mut().unwrap(), &mut sz_j);
        if rc != SQLITE_OK {
            break 'end_playback;
        }

        // Lê do journal o nome do super-journal, se estiver presente. Se um nome
        // de super-journal é especificado mas o arquivo não está presente em
        // disco, o journal não é quente e não precisa de playback.
        //
        // TODO do C: tecnicamente o que segue é um erro porque assume que o
        // buffer Pager.pTmpSpace tem (mxPathname+1) bytes ou mais, i.e. que
        // (pPager->pageSize >= pPager->pVfs->mxPathname+1). Com os_unix.c,
        // mxPathname é 512, igual ao valor mínimo permitido de pageSize.
        rc = read_super_journal(
            p_pager.jfd.as_deref_mut().unwrap(),
            &mut z_super[..],
            n_super_buf as u32,
        );
        if rc == SQLITE_OK && z_super[0] != 0 {
            let len = strlen30(Some(&z_super[..])) as usize;
            rc = os_access(&*p_vfs, &z_super[..len], SQLITE_ACCESS_EXISTS, &mut res);
        }
        if rc != SQLITE_OK || res == 0 {
            break 'end_playback;
        }
        p_pager.journal_off = 0;
        need_pager_reset_init = is_hot;
        let mut need_pager_reset = need_pager_reset_init;

        // Este laço termina quando uma chamada a read_journal_hdr() ou a
        // pager_playback_one_page() devolve SQLITE_DONE ou ocorre um erro de E/S.
        loop {
            // Lê o próximo cabeçalho de journal do arquivo. Se não há bytes
            // suficientes no arquivo para um cabeçalho completo, ou ele está
            // corrompido, um processo deve ter falhado ao escrevê-lo. Isso indica
            // que nada mais precisa ser revertido.
            rc = read_journal_hdr(p_pager, is_hot, sz_j, &mut n_rec, &mut mx_pg);
            if rc != SQLITE_OK {
                if rc == SQLITE_DONE {
                    rc = SQLITE_OK;
                }
                break 'end_playback;
            }

            // Se nRec é 0xffffffff, este journal foi criado por um processo em
            // modo no-sync. Isso significa que o resto do arquivo são páginas e não
            // há mais cabeçalhos. Calcula nRec com base nessa suposição.
            if n_rec == 0xffffffff {
                debug_assert!(p_pager.journal_off == journal_hdr_sz(p_pager) as i64);
                n_rec = ((sz_j - journal_hdr_sz(p_pager) as i64) / journal_pg_sz(p_pager)) as i32 as u32;
            }

            // Se nRec é 0, este rollback é de uma transação criada por este
            // processo e este é o último cabeçalho do journal, isso significa que
            // esta parte do journal estava sendo preenchida mas ainda não foi
            // sincronizada. Calcula o número de páginas pelo tamanho restante do
            // arquivo.
            //
            // O terceiro termo do teste foi adicionado para corrigir o ticket
            // #2565. Ao reverter um hot-journal, nRec==0 sempre significa que o
            // próximo trecho do journal contém zero páginas para reverter. Mas ao
            // fazer um ROLLBACK em que o trecho com nRec==0 é o último do journal,
            // isso significa que o journal pode conter páginas adicionais a
            // reverter e que o número de páginas deve ser calculado pelo tamanho
            // do arquivo de journal.
            if n_rec == 0
                && is_hot == 0
                && p_pager.journal_hdr + journal_hdr_sz(p_pager) as i64 == p_pager.journal_off
            {
                n_rec = ((sz_j - p_pager.journal_off) / journal_pg_sz(p_pager)) as i32 as u32;
            }

            // Se este é o primeiro cabeçalho lido do journal, trunca o arquivo do
            // banco de volta ao tamanho original.
            if p_pager.journal_off == journal_hdr_sz(p_pager) as i64 {
                rc = pager_truncate(p_pager, mx_pg);
                if rc != SQLITE_OK {
                    break 'end_playback;
                }
                p_pager.db_size = mx_pg;
                if p_pager.mx_pgno < mx_pg {
                    p_pager.mx_pgno = mx_pg;
                }
            }

            // Copia as páginas originais do journal de volta para o arquivo do
            // banco e/ou para o cache de páginas.
            let mut u: u32 = 0;
            while u < n_rec {
                if need_pager_reset != 0 {
                    pager_reset(p_pager);
                    need_pager_reset = 0;
                }
                let mut journal_off = p_pager.journal_off;
                rc = pager_playback_one_page(p_pager, &mut journal_off, None, 1, 0);
                p_pager.journal_off = journal_off;
                if rc == SQLITE_OK {
                    n_playback += 1;
                } else if rc == SQLITE_DONE {
                    p_pager.journal_off = sz_j;
                    break;
                } else if rc == SQLITE_IOERR_SHORT_READ {
                    // Se o journal foi truncado, simplesmente para de ler e de
                    // processar o journal. Isso pode acontecer se o journal não
                    // foi completamente escrito e sincronizado antes de uma queda.
                    // Nesse caso o banco nunca deveria ter sido escrito, então é
                    // aceitável abandonar o rollback.
                    rc = SQLITE_OK;
                    break 'end_playback;
                } else {
                    // Se não é possível reverter, sai e devolve o código de erro.
                    // Isso fará o pager entrar no estado de erro para que nenhum
                    // dano adicional seja feito. Talvez o próximo processo consiga
                    // reverter o banco.
                    break 'end_playback;
                }
                u += 1;
            }
        }
    }

    // end_playback:
    if rc == SQLITE_OK {
        rc = pager_set_pagesize(p_pager, &mut saved_page_size, -1);
    }
    // Depois de um rollback, o arquivo do banco deve estar de volta ao estado
    // original anterior ao início da transação. (O SQLITE_FCNTL_DB_UNCHANGED só é
    // enviado sob SQLITE_DEBUG, que não vale no Debian.)

    // Se este playback acontece automaticamente como resultado de um erro de E/S
    // ou de malloc ocorrido depois de o change-counter ser atualizado mas antes de
    // a transação ser confirmada, a modificação do change-counter pode ter acabado
    // de ser revertida. Se isso acontece em modo exclusivo, as transações
    // seguintes da conexão não atualizarão o change-counter. Isso pode levar a
    // problemas de inconsistência de cache em outros processos no futuro. Então,
    // por precaução, limpa o flag changeCountDone agora.
    p_pager.change_count_done = p_pager.temp_file;

    if rc == SQLITE_OK {
        // No C, deixa 4 bytes de espaço antes do nome do super-journal em
        // memória, pois ele pode acabar sendo passado a sqlite3OsOpen(), que
        // exige 4 bytes 0x00 imediatamente antes do nome. Aqui o nome fica em
        // z_super a partir do índice 0 (pager_delsuper cuida do prefixo).
        z_super.iter_mut().for_each(|b| *b = 0);
        rc = read_super_journal(
            p_pager.jfd.as_deref_mut().unwrap(),
            &mut z_super[..],
            n_super_buf as u32,
        );
        z_super_read = true;
    }
    if rc == SQLITE_OK
        && (p_pager.e_state as i32 >= PAGER_WRITER_DBMOD || p_pager.e_state as i32 == PAGER_OPEN)
    {
        rc = pager_sync(p_pager, None);
    }
    if rc == SQLITE_OK {
        let has_super = z_super_read && z_super[0] != 0;
        rc = pager_end_transaction(p_pager, has_super as i32, 0);
    }
    if rc == SQLITE_OK && z_super_read && z_super[0] != 0 && res != 0 {
        // Se havia um super-journal e esta rotina vai devolver sucesso, vê se é
        // possível apagar o super-journal.
        rc = pager_delsuper(p_pager, &z_super[..]);
    }
    if is_hot != 0 && n_playback != 0 {
        let z_journal = String::from_utf8_lossy(&p_pager.z_journal[..strlen30(Some(&p_pager.z_journal[..])) as usize]).into_owned();
        let z_msg = format!("recovered {} pages from {}", n_playback, z_journal);
        api::log(SQLITE_NOTICE_RECOVER_ROLLBACK, z_msg.as_bytes());
    }

    // A variável Pager.sectorSize pode ter sido atualizada ao reverter um journal
    // criado por um processo com tamanho de setor diferente. Restaura o valor
    // correto para este processo.
    set_sector_size(p_pager);
    rc
}


// ---- part_007.rs ----

// Contrato de modelagem desta parte (para o integrador):
//
//  * No C, `pPg->pPager` é um ponteiro de volta; aqui `PgHdr.p_pager` é um `Weak`.
//    Como os chamadores já seguram o `&mut Pager` (vindo do `RefCell`), fazer
//    `upgrade().borrow()` dentro destas funções daria pânico por empréstimo duplo.
//    Por isso `read_db_page` e `pager_write_changecounter` recebem o pager
//    explicitamente, ao lado da página.
//  * `wal_undo` chama o callback com o pager inteiro emprestado, o que o `&mut Wal`
//    do próprio pager impede. A lista de páginas desfeitas é coletada pelo callback
//    de `wal_undo` e os `pager_undo_callback` rodam depois, na mesma ordem. É
//    equivalente: o cabeçalho do wal-index já foi restaurado quando o C chama o
//    callback, e `wal_find_frame` só enxerga frames até `hdr.mx_frame`.

/// Lê o conteúdo da página p_pg do arquivo do banco (ou do WAL, se é lá que está
/// a cópia mais recente) para p_pg.p_data. Um bloqueio compartilhado ou maior deve
/// ser mantido no arquivo do banco antes de chamar esta função.
///
/// Se a página 1 é lida, o valor de Pager.dbFileVers[] é definido com o valor lido
/// do arquivo do banco.
///
/// Se ocorre um erro de E/S, o erro é devolvido ao chamador. Caso contrário,
/// SQLITE_OK é devolvido.
pub fn read_db_page(p_pager: &mut Pager, p_pg: &mut PgHdr) -> i32 {
    let mut rc: i32;
    let mut i_frame: u32 = 0; // Frame do WAL que contém pgno
    let page_size = p_pager.page_size as usize;

    debug_assert!(p_pager.e_state as i32 >= PAGER_READER && p_pager.mem_db == 0);
    debug_assert!(is_open(&p_pager.fd));

    if pager_use_wal(p_pager) {
        rc = wal_find_frame(p_pager.p_wal.as_deref_mut().unwrap(), p_pg.pgno, &mut i_frame);
        if rc != 0 {
            return rc;
        }
    }
    if i_frame != 0 {
        rc = wal_read_frame(
            p_pager.p_wal.as_deref_mut().unwrap(),
            i_frame,
            page_size as i32,
            &mut p_pg.p_data[..page_size],
        );
    } else {
        let i_offset: i64 = (p_pg.pgno as i64 - 1) * p_pager.page_size;
        rc = os_read(
            p_pager.fd.as_deref_mut().unwrap(),
            &mut p_pg.p_data[..page_size],
            i_offset,
        );
        if rc == SQLITE_IOERR_SHORT_READ {
            rc = SQLITE_OK;
        }
    }

    if p_pg.pgno == 1 {
        if rc != 0 {
            // Se a leitura falha, define dbFileVers[] com algo que nunca será uma
            // versão de arquivo válida. dbFileVers[] é uma cópia dos bytes 24..39
            // do banco. Os bytes 28..31 devem ser sempre zero ou o tamanho do
            // banco em páginas. Os bytes 32..35 e 35..39 devem ser números de
            // página, que nunca são 0xffffffff. Então preencher
            // pPager->dbFileVers[] com bytes 0xff deve bastar.
            //
            // Para um banco criptografado a situação é mais complexa: os bytes
            // 24..39 do banco são ruído branco. Mas a probabilidade de o ruído
            // branco ser igual a 16 bytes 0xff é desprezível, então ainda deve
            // dar certo.
            p_pager.db_file_vers.fill(0xff);
        } else {
            let n = p_pager.db_file_vers.len();
            p_pager.db_file_vers.copy_from_slice(&p_pg.p_data[24..24 + n]);
        }
    }

    rc
}

/// Atualiza o valor do change-counter nos deslocamentos 24 e 92 do cabeçalho e o
/// número de versão do sqlite no deslocamento 96.
///
/// Esta é uma atualização incondicional. Veja também a rotina
/// pager_incr_changecounter(), que só atualiza o change-counter se a atualização
/// é de fato necessária, conforme o estado pPager->changeCountDone.
pub fn pager_write_changecounter(p_pager: &Pager, p_pg: &mut PgHdr) {
    // Incrementa o valor recém-lido e o escreve de volta no byte 24.
    let change_counter: u32 = get_4byte(&p_pager.db_file_vers).wrapping_add(1);
    put_32bits(&mut p_pg.p_data[24..28], change_counter);

    // Guarda também o número de versão do SQLite nos bytes 96..99 e, nos bytes
    // 92..95, o change-counter para o qual o número de versão é válido.
    put_32bits(&mut p_pg.p_data[92..96], change_counter);
    put_32bits(&mut p_pg.p_data[96..100], SQLITE_VERSION_NUMBER as u32);
}

/// Esta função é invocada uma vez para cada página que já foi escrita no arquivo
/// de log quando uma transação WAL é revertida. O parâmetro i_pg é o número de
/// página dessa página. O argumento p_ctx do C é o próprio Pager.
///
/// Se a página i_pg está presente no cache e não tem referências pendentes, ela é
/// descartada. Caso contrário, se há uma ou mais referências pendentes, o
/// conteúdo da página é recarregado do banco. Se a releitura é necessária e
/// falha, devolve um código de erro do SQLite. Caso contrário, SQLITE_OK.
pub fn pager_undo_callback(p_pager: &mut Pager, i_pg: Pgno) -> i32 {
    let mut rc = SQLITE_OK;

    debug_assert!(pager_use_wal(p_pager));
    if let Some(p_pg) = pager_lookup(p_pager, i_pg) {
        if pcache_page_refcount(&p_pg) == 1 {
            pcache_drop(&p_pg);
        } else {
            rc = read_db_page(p_pager, &mut p_pg.borrow_mut());
            if rc == SQLITE_OK {
                let x_reiniter = p_pager.x_reiniter.expect("xReiniter deve estar definido");
                x_reiniter(&mut p_pg.borrow_mut());
            }
            pager_unref_not_null(&p_pg);
        }
    }

    // Normalmente, se uma transação é revertida, qualquer processo de backup é
    // atualizado à medida que os dados são copiados do journal de rollback para o
    // banco. Isso em geral não é possível com um banco WAL, pois o rollback
    // consiste em simplesmente truncar o arquivo de log. Portanto, se um ou mais
    // frames já foram escritos no log (e portanto também copiados para os bancos
    // de backup) como parte desta transação, os backups devem ser reiniciados.
    backup_restart(p_pager.p_backup.as_ref());

    rc
}

/// Esta função é chamada para reverter uma transação em um banco WAL.
pub fn pager_rollback_wal(p_pager: &mut Pager) -> i32 {
    let mut rc: i32;

    // Para todas as páginas do cache que estão sujas ou que já foram escritas
    // (mas não confirmadas) no arquivo de log, faz uma das seguintes coisas:
    //
    //   + Descarta a página em cache (se refcount==0), ou
    //   + Recarrega o conteúdo da página do banco (se refcount>0).
    p_pager.db_size = p_pager.db_orig_size;
    let mut undone: Vec<Pgno> = Vec::new();
    rc = wal_undo(p_pager.p_wal.as_deref_mut().unwrap(), &mut |i_pg: u32| {
        undone.push(i_pg);
        SQLITE_OK
    });
    for i_pg in undone {
        if rc != SQLITE_OK {
            break;
        }
        rc = pager_undo_callback(p_pager, i_pg);
    }
    let mut p_list = pcache_dirty_list(p_pager.p_p_cache.as_ref().unwrap());
    while rc == SQLITE_OK {
        let p = match p_list {
            Some(p) => p,
            None => break,
        };
        let p_next = p.borrow().p_dirty.clone();
        let pgno = p.borrow().pgno;
        rc = pager_undo_callback(p_pager, pgno);
        p_list = p_next;
    }

    rc
}

/// Esta função é um invólucro em torno de sqlite3WalFrames(). Além de registrar o
/// conteúdo da lista de páginas encabeçada por p_list (ligadas por pDirty), ela
/// notifica os processos de backup ativos de que as páginas mudaram.
///
/// A lista de páginas passada a esta rotina está sempre ordenada por número de
/// página. Logo, se a página 1 aparece em algum ponto da lista, ela é a primeira.
pub fn pager_wal_frames(
    p_pager: &mut Pager,
    p_list: PgHdrRef,
    n_truncate: Pgno,
    is_commit: i32,
) -> i32 {
    let rc: i32; // Código de retorno
    let n_list: i32; // Número de páginas em p_list

    debug_assert!(p_pager.p_wal.is_some());

    // Percorre a cadeia pDirty a partir da cabeça
    let mut chain: Vec<PgHdrRef> = Vec::new();
    let mut cur = Some(p_list);
    while let Some(p) = cur {
        cur = p.borrow().p_dirty.clone();
        chain.push(p);
    }

    // (Verificação de ordem crescente: só existe sob SQLITE_DEBUG.)
    debug_assert!(chain.len() == 1 || is_commit != 0);

    let kept: Vec<PgHdrRef>;
    if is_commit != 0 {
        // Se uma transação WAL está sendo confirmada, não adianta escrever no
        // arquivo WAL nenhuma página com número maior que nTruncate. Elas nunca
        // serão lidas por nenhum cliente. Então são removidas da lista pDirty
        // aqui.
        kept = chain
            .into_iter()
            .filter(|p| p.borrow().pgno <= n_truncate)
            .collect();
        for i in 0..kept.len() {
            let next = kept.get(i + 1).cloned();
            kept[i].borrow_mut().p_dirty = next;
        }
        n_list = kept.len() as i32;
        debug_assert!(!kept.is_empty());
    } else {
        n_list = 1;
        kept = chain;
    }
    p_pager.a_stat[PAGER_STAT_WRITE] = p_pager.a_stat[PAGER_STAT_WRITE].wrapping_add(n_list as u32);

    if kept[0].borrow().pgno == 1 {
        pager_write_changecounter(p_pager, &mut kept[0].borrow_mut());
    }
    let page_size = p_pager.page_size as i32;
    let wal_sync_flags = p_pager.wal_sync_flags as i32;
    rc = wal_frames(
        p_pager.p_wal.as_deref_mut().unwrap(),
        page_size,
        &kept,
        n_truncate,
        is_commit,
        wal_sync_flags,
    );
    if rc == SQLITE_OK && p_pager.p_backup.is_some() {
        for p in kept.iter() {
            let pg = p.borrow();
            let ps = p_pager.page_size as usize;
            backup_update(p_pager.p_backup.as_ref(), pg.pgno, &pg.p_data[..ps]);
        }
    }

    rc
}

/// Começa uma transação de leitura no WAL.
///
/// Esta rotina se chamava "pagerOpenSnapshot()" porque essencialmente tira um
/// instantâneo do banco no ponto atual do tempo e o preserva para uso pelo leitor,
/// apesar de mudanças concorrentes por outros escritores ou checkpointers.
pub fn pager_begin_read_transaction(p_pager: &mut Pager) -> i32 {
    let rc: i32; // Código de retorno
    let mut changed: i32 = 0; // Verdadeiro se o cache precisa ser resetado

    debug_assert!(pager_use_wal(p_pager));
    debug_assert!(p_pager.e_state as i32 == PAGER_OPEN || p_pager.e_state as i32 == PAGER_READER);

    // sqlite3WalEndReadTransaction() não foi chamada para a transação anterior em
    // locking_mode=EXCLUSIVE. Então chama agora. Se estamos em
    // locking_mode=NORMAL e EndRead() foi chamada antes, a chamada duplicada é
    // inofensiva.
    wal_end_read_transaction(p_pager.p_wal.as_deref_mut().unwrap());

    rc = wal_begin_read_transaction(p_pager.p_wal.as_deref_mut().unwrap(), &mut changed);
    if rc != SQLITE_OK || changed != 0 {
        pager_reset(p_pager);
        if usefetch(p_pager) {
            os_unfetch(p_pager.fd.as_deref_mut().unwrap(), 0);
        }
    }

    rc
}

/// Esta função é chamada como parte da transição de PAGER_OPEN para PAGER_READER
/// para determinar o tamanho do arquivo do banco em páginas (supondo o tamanho de
/// página atualmente guardado em Pager.pageSize).
///
/// Se não há erro, devolve SQLITE_OK e o tamanho do banco em páginas é guardado em
/// *pn_page. Caso contrário, devolve um código de erro (talvez
/// SQLITE_IOERR_FSTAT) e *pn_page fica inalterado.
pub fn pager_pagecount(p_pager: &mut Pager, pn_page: &mut Pgno) -> i32 {
    let mut n_page: Pgno; // Valor a devolver via *pn_page

    // Consulta o subsistema WAL sobre o tamanho do banco. WalDbsize() devolve zero
    // se o WAL não está aberto (i.e. Pager.pWal==0) ou se o tamanho do banco não
    // está disponível. O tamanho do banco não está disponível no subsistema WAL
    // se o arquivo de log está vazio ou não contém transações confirmadas válidas.
    debug_assert!(p_pager.e_state as i32 == PAGER_OPEN);
    debug_assert!(p_pager.e_lock as i32 >= SHARED_LOCK);
    debug_assert!(is_open(&p_pager.fd));
    debug_assert!(p_pager.temp_file == 0);
    n_page = wal_dbsize(p_pager.p_wal.as_deref());

    // Se o número de páginas do banco não está disponível no subsistema WAL,
    // determina a contagem de páginas pelo tamanho do arquivo do banco. Se o
    // tamanho do arquivo não é múltiplo inteiro do tamanho de página, arredonda o
    // resultado para cima.
    if n_page == 0 && is_open(&p_pager.fd) {
        let mut n: i64 = 0; // Tamanho do arquivo do banco em bytes
        let rc = os_file_size(p_pager.fd.as_deref_mut().unwrap(), &mut n);
        if rc != SQLITE_OK {
            return rc;
        }
        n_page = ((n + p_pager.page_size - 1) / p_pager.page_size) as Pgno;
    }

    // Se o número atual de páginas do arquivo é maior que o número máximo de
    // páginas configurado, aumenta o limite permitido para que o arquivo possa ser
    // lido.
    if n_page > p_pager.mx_pgno {
        p_pager.mx_pgno = n_page;
    }

    *pn_page = n_page;
    SQLITE_OK
}

/// Verifica se o arquivo *-wal correspondente ao banco aberto por p_pager existe,
/// se o banco não está vazio, ou verifica que o arquivo *-wal não existe (apagando
/// o arquivo) se o banco está vazio.
///
/// Se o banco não está vazio e o arquivo *-wal existe, abre o pager em modo WAL.
/// Se o banco está vazio, ou se nenhum arquivo *-wal existe e não ocorre erro,
/// garante que Pager.journalMode não esteja definido como PAGER_JOURNALMODE_WAL.
///
/// Devolve SQLITE_OK ou um código de erro.
///
/// O chamador deve manter um bloqueio SHARED no arquivo do banco para chamar esta
/// função. Como é preciso um bloqueio EXCLUSIVE no arquivo do banco para apagar um
/// WAL num banco não vazio, isso garante que não há condição de corrida entre o
/// xAccess() abaixo e um xDelete() executado por alguma outra conexão.
pub fn pager_open_wal_if_present(p_pager: &mut Pager) -> i32 {
    let mut rc = SQLITE_OK;
    debug_assert!(p_pager.e_state as i32 == PAGER_OPEN);
    debug_assert!(p_pager.e_lock as i32 >= SHARED_LOCK);

    if p_pager.temp_file == 0 {
        let mut is_wal: i32 = 0; // Verdadeiro se o arquivo WAL existe
        let p_vfs = p_pager.p_vfs.clone().unwrap();
        rc = os_access(&*p_vfs, &p_pager.z_wal, SQLITE_ACCESS_EXISTS, &mut is_wal);
        if rc == SQLITE_OK {
            if is_wal != 0 {
                let mut n_page: Pgno = 0; // Tamanho do arquivo do banco

                rc = pager_pagecount(p_pager, &mut n_page);
                if rc != 0 {
                    return rc;
                }
                if n_page == 0 {
                    rc = os_delete(&*p_vfs, &p_pager.z_wal, 0);
                } else {
                    rc = pager_open_wal(p_pager, None);
                }
            } else if p_pager.journal_mode as i32 == PAGER_JOURNALMODE_WAL {
                p_pager.journal_mode = PAGER_JOURNALMODE_DELETE as u8;
            }
        }
    }
    rc
}


// ---- part_008.rs ----

/// Reproduz um savepoint. Ou, se pSavepoint for NULO, reproduz todo o arquivo de super-journal.
/// O caso pSavepoint nulo ocorre quando um comando ROLLBACK TO é invocado em um SAVEPOINT
/// que é um savepoint de transação.
///
/// Quando pSavepoint não é NULO (ou seja, um savepoint que não é de transação está sendo revertido),
/// então a reversão consiste em até três estágios, executados na ordem especificada:
///
/// * Páginas são reproduzidas do journal principal começando no deslocamento de byte
///   PagerSavepoint.iOffset e continuando até PagerSavepoint.iHdrOffset, ou até o fim do arquivo
///   do journal principal se PagerSavepoint.iHdrOffset for zero.
///
/// * Se PagerSavepoint.iHdrOffset não for zero, então páginas são reproduzidas começando do
///   cabeçalho do journal imediatamente após PagerSavepoint.iHdrOffset até o fim do arquivo
///   do journal principal.
///
/// * Páginas são então reproduzidas do arquivo do sub-journal, começando com o
///   PagerSavepoint.iSubRec e continuando até o fim do arquivo do journal.
///
/// Durante todo o processo de reversão, cada vez que uma página é revertida, o bit
/// correspondente é definido em uma estrutura bitvec (variável pDone na implementação
/// abaixo). Isto é usado para garantir que uma página seja revertida apenas na primeira
/// vez que é encontrada em qualquer um dos journals.
///
/// Se pSavepoint for NULO, então páginas são reproduzidas apenas do arquivo do journal principal.
/// Não há necessidade de um bitvec neste caso.
///
/// Em qualquer caso, antes do playback começar a variável Pager.dbSize é redefinida para
/// o valor que tinha no início do savepoint (ou transação). Nenhuma página com um número
/// de página maior que este valor é reproduzida. Se uma for encontrada, é simplesmente ignorada.
fn pager_playback_savepoint(p_pager: &mut Pager, p_savepoint: Option<&PagerSavepoint>) -> i32 {
    let sz_j: i64;                 /* Tamanho efetivo do journal principal */
    let i_hdr_off: i64;            /* Fim do primeiro segmento de registros do journal principal */
    let mut rc: i32 = SQLITE_OK;   /* Código de retorno */
    let mut p_done: Option<Bitvec> = None;  /* Bitvec para garantir que páginas sejam reproduzidas apenas uma vez */

    assert!(p_pager.e_state != PAGER_ERROR);
    assert!(p_pager.e_state >= PAGER_WRITER_LOCKED);

    /* Aloca um bitvec para armazenar o conjunto de páginas revertidas */
    if let Some(savepoint) = p_savepoint {
        p_done = bitvec_create(savepoint.n_orig);
        if p_done.is_none() {
            return SQLITE_NOMEM_BKPT;
        }
    }

    /* Redefinir o tamanho do banco de dados para o valor antes do savepoint
    ** sendo revertido ser aberto.
    */
    p_pager.db_size = if let Some(savepoint) = p_savepoint {
        savepoint.n_orig
    } else {
        p_pager.db_orig_size
    };
    p_pager.change_count_done = p_pager.temp_file;

    if p_savepoint.is_none() && pager_use_wal(p_pager) {
        return pager_rollback_wal(p_pager);
    }

    /* Usar pPager->journalOff como o tamanho efetivo do journal de reversão principal.
    ** O arquivo real pode ser maior em PAGER_JOURNALMODE_TRUNCATE ou PAGER_JOURNALMODE_PERSIST.
    ** Mas qualquer coisa além de pPager->journalOff está fora dos limites para nós.
    */
    sz_j = p_pager.journal_off;
    assert!(!pager_use_wal(p_pager) || sz_j == 0);

    /* Começar revertendo registros do journal principal começando em
    ** PagerSavepoint.iOffset e continuando até o próximo cabeçalho do journal.
    ** Pode haver registros no journal principal que tenham um número de página
    ** maior que o tamanho atual do banco de dados (pPager->dbSize) mas esses
    ** serão ignorados automaticamente. Páginas são adicionadas a pDone conforme
    ** são reproduzidas.
    */
    if let (Some(savepoint), false) = (p_savepoint, pager_use_wal(p_pager)) {
        i_hdr_off = if savepoint.i_hdr_offset != 0 {
            savepoint.i_hdr_offset
        } else {
            sz_j
        };
        p_pager.journal_off = savepoint.i_offset;
        /* O deslocamento viaja numa variável local: em C, pOffset aponta para
        ** pPager->journalOff, o que em Rust seriam dois empréstimos mutáveis. */
        while rc == SQLITE_OK && p_pager.journal_off < i_hdr_off {
            let mut off = p_pager.journal_off;
            rc = pager_playback_one_page(p_pager, &mut off, p_done.as_mut(), 1, 1);
            p_pager.journal_off = off;
        }
        assert!(rc != SQLITE_DONE);
    } else {
        p_pager.journal_off = 0;
    }

    /* Continuar revertendo registros do journal principal começando no
    ** primeiro cabeçalho do journal encontrado e continuando até o fim efetivo
    ** do arquivo do journal principal. Continuar a ignorar páginas fora do intervalo
    ** e continuar adicionando páginas revertidas a pDone.
    */
    while rc == SQLITE_OK && p_pager.journal_off < sz_j {
        let mut n_j_rec: u32 = 0;   /* Número de registros do journal */
        let mut dummy: u32 = 0;
        rc = read_journal_hdr(p_pager, 0, sz_j, &mut n_j_rec, &mut dummy);
        assert!(rc != SQLITE_DONE);

        /* O teste "pPager->journalHdr+JOURNAL_HDR_SZ(pPager)==pPager->journalOff"
        ** está relacionado ao ticket #2565. Veja a discussão na função
        ** pager_playback() para informações adicionais.
        */
        if n_j_rec == 0
            && p_pager.journal_hdr + journal_hdr_sz(p_pager) == p_pager.journal_off
        {
            n_j_rec = ((sz_j - p_pager.journal_off) / journal_pg_sz(p_pager)) as u32;
        }
        let mut ii: u32 = 0;
        while rc == SQLITE_OK && ii < n_j_rec && p_pager.journal_off < sz_j {
            let mut off = p_pager.journal_off;
            rc = pager_playback_one_page(p_pager, &mut off, p_done.as_mut(), 1, 1);
            p_pager.journal_off = off;
            ii = ii.wrapping_add(1);
        }
        assert!(rc != SQLITE_DONE);
    }
    assert!(rc != SQLITE_OK || p_pager.journal_off >= sz_j);

    /* Finalmente, reverter páginas do sub-journal. Páginas que foram
    ** previamente revertidas do journal principal (e estão portanto em pDone)
    ** serão ignoradas. Páginas fora do intervalo também são ignoradas.
    */
    if let Some(savepoint) = p_savepoint {
        let mut offset: i64 = savepoint.i_sub_rec as i64 * (4 + p_pager.page_size as i64);

        if pager_use_wal(p_pager) {
            rc = wal_savepoint_undo(&mut p_pager.p_wal, &savepoint.a_wal_data);
        }
        let mut ii: u32 = savepoint.i_sub_rec;
        while rc == SQLITE_OK && ii < p_pager.n_sub_rec {
            assert!(offset == ii as i64 * (4 + p_pager.page_size as i64));
            rc = pager_playback_one_page(p_pager, &mut offset, p_done.as_mut(), 0, 1);
            ii = ii.wrapping_add(1);
        }
        assert!(rc != SQLITE_DONE);
    }

    /* bitvec_destroy(p_done): o Bitvec é liberado ao sair de escopo. */
    drop(p_done);
    if rc == SQLITE_OK {
        p_pager.journal_off = sz_j;
    }

    return rc;
}

/// Alterar o número máximo de páginas em memória que são permitidas
/// antes de tentar reciclar páginas limpas e não utilizadas.
pub fn pager_set_cachesize(p_pager: &mut Pager, mx_page: i32) {
    pcache_set_cachesize(&mut p_pager.p_pcache, mx_page);
}

/// Alterar o número máximo de páginas em memória que são permitidas
/// antes de tentar derramar páginas para o journal.
pub fn pager_set_spillsize(p_pager: &mut Pager, mx_page: i32) -> i32 {
    return pcache_set_spillsize(&mut p_pager.p_pcache, mx_page);
}

/// Invocar SQLITE_FCNTL_MMAP_SIZE baseado no valor atual de szMmap.
fn pager_fix_maplimit(p_pager: &mut Pager) {
    if is_open(&p_pager.fd) && os_file_version(&p_pager.fd) >= 3 {
        let mut sz: i64 = p_pager.sz_mmap;
        p_pager.b_use_fetch = (sz > 0) as u8;
        set_getter_method(p_pager);
        os_file_control_hint(&mut p_pager.fd, SQLITE_FCNTL_MMAP_SIZE, &mut sz);
    }
}

/// Alterar o tamanho máximo de qualquer mapeamento de memória feito do arquivo de banco de dados.
pub fn pager_set_mmap_limit(p_pager: &mut Pager, sz_mmap: i64) {
    p_pager.sz_mmap = sz_mmap;
    pager_fix_maplimit(p_pager);
}

/// Liberar o máximo de memória possível do pager.
pub fn pager_shrink(p_pager: &mut Pager) {
    pcache_shrink(&mut p_pager.p_pcache);
}

/// Ajustar as configurações do pager para as especificadas no parâmetro pgFlags.
///
/// O "nível" em pgFlags & PAGER_SYNCHRONOUS_MASK define a robustez
/// do banco de dados contra danos por falhas do SO ou falta de energia
/// alterando o número de chamadas sync() ao escrever os journals.
/// Há quatro níveis:
///
/// OFF       sqlite3OsSync() nunca é chamado. Este é o padrão
///           para arquivos temporários e transitórios.
///
/// NORMAL    O journal é sincronizado uma vez antes de escritas começarem no
///           banco de dados. Esta é normalmente uma proteção adequada, mas
///           é teoricamente possível, embora muito improvável, que uma falha
///           de energia inoportuna pudesse deixar o journal em um estado
///           que causaria danos ao banco de dados quando revertido.
///
/// FULL      O journal é sincronizado duas vezes antes de escritas começarem
///           no banco de dados (com algumas informações adicionais, o campo nRec
///           do cabeçalho do journal, sendo escrito entre as duas sincronizações).
///           Se assumirmos que escrever um setor de disco único é atômico, então
///           este modo fornece garantia de que o journal não será corrompido ao ponto
///           de causar danos ao banco de dados durante reversão.
///
/// EXTRA     Isto é como FULL exceto que também sincroniza o diretório
///           que contém o journal de reversão após o journal de reversão ser removido.
///
/// O acima é para um modo de journal de reversão. Para modo WAL, OFF continua
/// significando que nenhuma sincronização ocorra. NORMAL significa que o WAL é
/// sincronizado antes do início do checkpoint e o arquivo de banco de dados é
/// sincronizado na conclusão do checkpoint se todo o conteúdo do WAL foi escrito
/// de volta ao banco de dados. Mas nenhuma operação de sincronização ocorre para
/// um commit ordinário no modo NORMAL com WAL. FULL significa que o arquivo WAL
/// é sincronizado após cada operação de commit, além das sincronizações associadas
/// com NORMAL. Não há diferença entre FULL e EXTRA para modo WAL.
///
/// Não confundir sincronismo FULL com SQLITE_SYNC_FULL. A macro SQLITE_SYNC_FULL
/// significa usar a sincronização completa estilo MacOSX usando fcntl(F_FULLFSYNC).
/// SQLITE_SYNC_NORMAL significa fazer uma chamada fsync() ordinária. Não há diferença
/// entre SQLITE_SYNC_FULL e SQLITE_SYNC_NORMAL em plataformas que não sejam MacOSX.
/// Mas a configuração síncrona FULL versus NORMAL determina quando o primitivo xSync
/// é chamado e é relevante para todas as plataformas.
///
/// Valores numéricos associados a estes estados são OFF==1, NORMAL==2,
/// e FULL==3.
pub fn pager_set_flags(p_pager: &mut Pager, pg_flags: u32) {
    let level: u32 = pg_flags & PAGER_SYNCHRONOUS_MASK;
    if p_pager.temp_file != 0 {
        p_pager.no_sync = 1;
        p_pager.full_sync = 0;
        p_pager.extra_sync = 0;
    } else {
        p_pager.no_sync = if level == PAGER_SYNCHRONOUS_OFF { 1 } else { 0 };
        p_pager.full_sync = if level >= PAGER_SYNCHRONOUS_FULL { 1 } else { 0 };
        p_pager.extra_sync = if level == PAGER_SYNCHRONOUS_EXTRA { 1 } else { 0 };
    }
    if p_pager.no_sync != 0 {
        p_pager.sync_flags = 0;
    } else if (pg_flags & PAGER_FULLFSYNC) != 0 {
        p_pager.sync_flags = SQLITE_SYNC_FULL;
    } else {
        p_pager.sync_flags = SQLITE_SYNC_NORMAL;
    }
    p_pager.wal_sync_flags = (p_pager.sync_flags << 2);
    if p_pager.full_sync != 0 {
        p_pager.wal_sync_flags |= p_pager.sync_flags;
    }
    if (pg_flags & PAGER_CKPT_FULLFSYNC) != 0 && p_pager.no_sync == 0 {
        p_pager.wal_sync_flags |= (SQLITE_SYNC_FULL << 2);
    }
    if (pg_flags & PAGER_CACHESPILL) != 0 {
        p_pager.do_not_spill &= !SPILLFLAG_OFF;
    } else {
        p_pager.do_not_spill |= SPILLFLAG_OFF;
    }
}

/* O contador sqlite3_opentemp_count só existe com SQLITE_TEST e não faz parte do porte. */

/// Abrir um arquivo temporário.
///
/// Escrever o descritor de arquivo em *pFile. Retornar SQLITE_OK no sucesso
/// ou algum outro código de erro se falharmos. O SO automaticamente
/// deletará o arquivo temporário quando for fechado.
///
/// Os flags passados para a chamada xOpen() da camada VFS são aqueles especificados
/// pelo parâmetro vfsFlags ORados com o seguinte:
///
/// SQLITE_OPEN_READWRITE
/// SQLITE_OPEN_CREATE
/// SQLITE_OPEN_EXCLUSIVE
/// SQLITE_OPEN_DELETEONCLOSE
///
/// Modelagem: em C o descritor chega por ponteiro (sempre pPager->fd, nos dois
/// chamadores); aqui a função abre direto em p_pager.fd.
pub fn pager_opentemp(p_pager: &mut Pager, vfs_flags: i32) -> i32 {
    let vfs_flags = vfs_flags
        | SQLITE_OPEN_READWRITE
        | SQLITE_OPEN_CREATE
        | SQLITE_OPEN_EXCLUSIVE
        | SQLITE_OPEN_DELETEONCLOSE;
    let rc = os_open(&p_pager.p_vfs, None, &mut p_pager.fd, vfs_flags, None);
    assert!(rc != SQLITE_OK || is_open(&p_pager.fd));
    rc
}

/// Definir a função manipulador de ocupado.
///
/// O pager invoca o manipulador de ocupado se sqlite3OsLock() retorna
/// SQLITE_BUSY ao tentar atualizar de nenhum bloqueio para bloqueio SHARED,
/// ou ao tentar atualizar de bloqueio RESERVED para bloqueio EXCLUSIVE.
/// Ele *não* invoca o manipulador de ocupado ao atualizar de SHARED para RESERVED,
/// ou ao atualizar de SHARED para EXCLUSIVE (que ocorre durante reversão de hot-journal).
/// Resumo:
///
/// Transição                        | Invoca xBusyHandler
/// --------------------------------------------------------
/// NO_LOCK       -> SHARED_LOCK      | Sim
/// SHARED_LOCK   -> RESERVED_LOCK    | Não
/// SHARED_LOCK   -> EXCLUSIVE_LOCK   | Não
/// RESERVED_LOCK -> EXCLUSIVE_LOCK   | Sim
///
/// Se o retorno de chamada do manipulador de ocupado retorna não zero, o bloqueio é
/// repetido. Se retorna zero, então o erro SQLITE_BUSY é retornado ao chamador
/// da função de API do pager.
///
/// Modelagem: o par (xBusyHandler, pBusyHandlerArg) vira um único fecho
/// `Rc<dyn Fn() -> i32>` que já carrega o argumento.
pub fn pager_set_busy_handler(p_pager: &mut Pager, x_busy_handler: Option<Rc<dyn Fn() -> i32>>) {
    p_pager.x_busy_handler = x_busy_handler.clone();
    os_file_control_busy_handler(&mut p_pager.fd, SQLITE_FCNTL_BUSYHANDLER, x_busy_handler);
}

/// Invoca o busy-handler do pager (em C: pPager->xBusyHandler(pPager->pBusyHandlerArg)).
/// Retorna verdadeiro se o handler pediu para repetir o bloqueio.
pub fn pager_call_busy_handler(p_pager: &Pager) -> bool {
    match &p_pager.x_busy_handler {
        Some(h) => h() != 0,
        None => false,
    }
}


// ---- part_009.rs ----

/// Altera o tamanho de página usado pelo objeto Pager. O novo tamanho de página
/// é passado em *p_page_size.
///
/// Se o pager estiver em estado de erro quando esta função é chamada, ela não
/// faz nada. O valor retornado é o código de erro do estado de erro (isto é,
/// SQLITE_IOERR, um subcódigo SQLITE_IOERR_xxx ou SQLITE_FULL).
///
/// Caso contrário, se todas as condições a seguir forem verdadeiras:
///
///   * o novo tamanho de página (valor de *p_page_size) é válido (uma potência
///     de dois entre 512 e SQLITE_MAX_PAGE_SIZE, inclusive), e
///
///   * não há referências de página pendentes, e
///
///   * o banco de dados não é em memória, ou é em memória e no momento tem zero páginas.
///
/// então o tamanho de página do pager passa a ser *p_page_size.
///
/// Se o tamanho de página mudar, esta função usa page_malloc() para obter um novo
/// buffer Pager.p_tmp_space. Se essa alocação falhar, SQLITE_NOMEM é retornado e o
/// tamanho de página não muda. Em todos os outros casos, SQLITE_OK é retornado.
///
/// Se o tamanho de página não mudar, seja porque uma das condições acima não é
/// verdadeira, o pager estava em estado de erro, ou a alocação falhou, então
/// *p_page_size recebe o tamanho de página antigo, mantido, antes de retornar.
pub fn pager_set_pagesize(p_pager: &mut Pager, p_page_size: &mut u32, n_reserve: i32) -> i32 {
    let mut n_reserve = n_reserve;
    let mut rc = SQLITE_OK;

    /* Não é possível fazer um assert_pager_state() completo aqui, pois esta
    ** função pode ser chamada de dentro de pager_open(), antes de o estado do
    ** objeto Pager estar internamente consistente.
    **
    ** Em algum momento esta função retornava erro se o pager estivesse em estado
    ** PAGER_ERROR. Mas como o estado PAGER_ERROR garante que há ao menos uma
    ** referência de página pendente, a função não faz nada nesse caso de qualquer jeito. */

    let page_size: u32 = *p_page_size;
    debug_assert!(page_size == 0 || (page_size >= 512 && page_size <= SQLITE_MAX_PAGE_SIZE as u32));
    if (p_pager.mem_db == 0 || p_pager.db_size == 0)
        && pcache_ref_count(&p_pager.p_pcache) == 0
        && page_size != 0
        && page_size != p_pager.page_size as u32
    {
        let mut p_new: Option<Vec<u8>> = None; /* Novo espaço temporário */
        let mut n_byte: i64 = 0;

        if p_pager.e_state > PAGER_OPEN && is_open(&p_pager.fd) {
            rc = os_file_size(&mut p_pager.fd, &mut n_byte);
        }
        if rc == SQLITE_OK {
            /* 8 bytes de espaço de estouro zerado bastam para que o analisador de
            ** cabeçalho de célula do b-tree nunca passe do fim da alocação */
            p_new = page_malloc(page_size as usize + 8);
            match p_new.as_mut() {
                None => rc = SQLITE_NOMEM_BKPT,
                Some(v) => v[page_size as usize..page_size as usize + 8].fill(0),
            }
        }

        if rc == SQLITE_OK {
            pager_reset(p_pager);
            rc = pcache_set_page_size(&mut p_pager.p_pcache, page_size);
        }
        if rc == SQLITE_OK {
            p_pager.p_tmp_space = p_new.unwrap_or_default();
            p_pager.db_size = ((n_byte + page_size as i64 - 1) / page_size as i64) as u32;
            p_pager.page_size = page_size as i32;
            p_pager.lck_pgno = (PENDING_BYTE / page_size) + 1;
        }
    }

    *p_page_size = p_pager.page_size as u32;
    if rc == SQLITE_OK {
        if n_reserve < 0 {
            n_reserve = p_pager.n_reserve as i32;
        }
        debug_assert!(n_reserve >= 0 && n_reserve < 1000);
        p_pager.n_reserve = n_reserve as i16;
        pager_fix_maplimit(p_pager);
    }
    rc
}

/// Retorna o buffer de "página temporária" mantido internamente pelo pager.
/// É um buffer grande o bastante para guardar todo o conteúdo de uma página do
/// banco de dados. É usado internamente durante o rollback e é sobrescrito
/// sempre que ocorre um rollback. Mas outros módulos podem usá-lo também,
/// desde que nenhum rollback esteja acontecendo.
pub fn pager_temp_space(p_pager: &mut Pager) -> &mut Vec<u8> {
    &mut p_pager.p_tmp_space
}

/// Tenta definir o número máximo de páginas do banco de dados se mx_page for
/// positivo. Não muda nada se mx_page for zero ou negativo. E nunca reduz o
/// máximo abaixo do tamanho atual do banco de dados.
///
/// Independente de mx_page, retorna o número máximo de páginas atual.
pub fn pager_max_page_count(p_pager: &mut Pager, mx_page: u32) -> u32 {
    if mx_page > 0 {
        p_pager.mx_pgno = mx_page;
    }
    debug_assert!(p_pager.e_state != PAGER_OPEN); /* Chamado só por OP_MAXPGCNT */
    /* OP_MAXPGCNT garante que o parâmetro passado a esta função não é menor que o
    ** total de páginas válidas do banco. Mas isso pode ser menor que Pager.db_size,
    ** então o assert comentado no C (mxPgno>=dbSize) não é válido. */
    p_pager.mx_pgno
}

/// Lê os primeiros n bytes do início do arquivo para a memória de p_dest.
///
/// Se o pager foi aberto sobre um arquivo transitório (z_filename==""), ou sobre
/// um arquivo com menos de n bytes, o buffer de saída é zerado e SQLITE_OK é
/// retornado. A razão é que esta função lê cabeçalhos de banco de dados, e um
/// banco novo transitório ou de tamanho zero tem cabeçalho feito só de zeros.
///
/// Se ocorrer qualquer erro de E/S que não seja SQLITE_IOERR_SHORT_READ, o código
/// de erro é retornado ao chamador e o conteúdo do buffer fica indefinido.
pub fn pager_read_fileheader(p_pager: &mut Pager, n: i32, p_dest: &mut [u8]) -> i32 {
    let mut rc = SQLITE_OK;
    p_dest[..n as usize].fill(0);
    debug_assert!(is_open(&p_pager.fd) || p_pager.temp_file != 0);

    /* Esta rotina só é chamada pelo btree logo depois de criar o objeto Pager.
    ** Ainda não houve oportunidade de passar ao modo WAL. */
    debug_assert!(!pager_use_wal(p_pager));

    if is_open(&p_pager.fd) {
        rc = os_read(&mut p_pager.fd, &mut p_dest[..n as usize], 0);
        if rc == SQLITE_IOERR_SHORT_READ {
            rc = SQLITE_OK;
        }
    }
    rc
}

/// Esta função só pode ser chamada quando há uma transação de leitura aberta no
/// pager. Retorna o número total de páginas do banco de dados.
///
/// Porém, se o arquivo tem entre 1 e <tamanho-da-página> bytes, é considerado
/// um arquivo de 1 página.
pub fn pager_pagecount(p_pager: &mut Pager, pn_page: &mut i32) {
    debug_assert!(p_pager.e_state >= PAGER_READER);
    debug_assert!(p_pager.e_state != PAGER_WRITER_FINISHED);
    *pn_page = p_pager.db_size as i32;
}

/// Tenta obter um bloqueio do tipo locktype no arquivo do banco de dados. Se um
/// bloqueio igual ou maior já estiver mantido, esta função não faz nada (retorna
/// SQLITE_OK de imediato).
///
/// Caso contrário, tenta obter o bloqueio com os_lock(). Invoca o callback de
/// ocupado se o bloqueio não estiver disponível no momento. Repete até o callback
/// devolver falso ou até a tentativa de obter o bloqueio ter sucesso.
///
/// Retorna SQLITE_OK em caso de sucesso e um código de erro se não conseguir o
/// bloqueio. Se o bloqueio for obtido, define Pager.e_state como locktype antes
/// de retornar.
pub fn pager_wait_on_lock(p_pager: &mut Pager, locktype: i32) -> i32 {
    let mut rc: i32; /* Código de retorno */

    /* Confere que isto é um no-op (porque o bloqueio pedido já é mantido) ou uma
    ** das transições durante as quais o busy-handler pode ser invocado, conforme
    ** o comentário de pager_set_busy_handler(). */
    debug_assert!(
        (p_pager.e_lock as i32 >= locktype)
            || (p_pager.e_lock as i32 == NO_LOCK && locktype == SHARED_LOCK)
            || (p_pager.e_lock as i32 == RESERVED_LOCK && locktype == EXCLUSIVE_LOCK)
    );

    loop {
        rc = pager_lock_db(p_pager, locktype);
        if !(rc == SQLITE_BUSY && pager_call_busy_handler(p_pager)) {
            break;
        }
    }
    rc
}

/// Trunca a imagem do arquivo do banco de dados em memória para n_page páginas.
/// Esta função não modifica de fato o arquivo no disco. Só define o estado interno
/// do objeto pager para que o truncamento seja feito quando a transação atual
/// for confirmada.
///
/// Esta função só é chamada logo antes de confirmar uma transação. Depois de
/// chamada, a transação precisa ser revertida ou confirmada. Não é seguro chamá-la
/// e depois continuar escrevendo no banco de dados.
pub fn pager_truncate_image(p_pager: &mut Pager, n_page: u32) {
    debug_assert!(p_pager.db_size >= n_page || corrupt_db());
    debug_assert!(p_pager.e_state >= PAGER_WRITER_CACHEMOD);
    p_pager.db_size = n_page;

    /* Em algum momento o código aqui chamava assert_truncate_constraint() para
    ** garantir que todas as páginas truncadas por esta operação estivessem, com
    ** savepoints abertos, presentes no journal de savepoint para poderem ser
    ** restauradas num rollback do savepoint. Isso não é mais necessário, pois esta
    ** função agora só é chamada logo antes de confirmar uma transação. Embora o
    ** Pager ainda possa ter savepoints abertos (Pager.n_savepoint!=0), eles não
    ** podem ser revertidos. Logo a chamada a assert_truncate_constraint() deixou
    ** de ser correta. */
}

/// Esta função é chamada antes de tentar um rollback de hot-journal. Ela
/// sincroniza o arquivo de journal no disco e então define Pager.journal_hdr como
/// o tamanho do arquivo de journal, para que a rotina pager_playback() saiba que
/// o arquivo de journal inteiro foi sincronizado.
///
/// Sincronizar um hot-journal no disco antes de tentar revertê-lo garante que, se
/// ocorrer uma queda de energia durante o rollback, o processo que tentar o
/// rollback após a recuperação do sistema veja o mesmo conteúdo de journal que
/// este processo.
///
/// Se tudo correr conforme o planejado, SQLITE_OK é retornado. Caso contrário,
/// um código de erro do SQLite.
pub fn pager_sync_hot_journal(p_pager: &mut Pager) -> i32 {
    let mut rc = SQLITE_OK;
    if p_pager.no_sync == 0 {
        rc = os_sync(&mut p_pager.jfd, SQLITE_SYNC_NORMAL);
    }
    if rc == SQLITE_OK {
        rc = os_file_size(&mut p_pager.jfd, &mut p_pager.journal_hdr);
    }
    rc
}

/// Obtém uma referência a um objeto de página mapeada em memória para o número de
/// página pgno. O novo objeto usa os bytes p_data, obtidos de xFetch(). Se tiver
/// sucesso, *pp_page passa a apontar para a nova referência de página e SQLITE_OK
/// é retornado. Caso contrário, retorna um código de erro do SQLite e *pp_page
/// fica None.
///
/// Referências de página obtidas por esta função devem ser liberadas chamando
/// pager_release_map_page().
///
/// Modelagem: o elo de volta PgHdr.p_pager (p->pPager = pPager no C) não pode ser
/// criado a partir de um &mut Pager; quem integra o preenche ao guardar o PgHdrRef.
pub fn pager_acquire_map_page(
    p_pager: &mut Pager,
    pgno: u32,
    p_data: Vec<u8>,
    pp_page: &mut Option<PgHdrRef>,
) -> i32 {
    let p: PgHdrRef; /* Página mapeada em memória a devolver */

    if let Some(p_free) = p_pager.p_mmap_freelist.take() {
        p_pager.p_mmap_freelist = p_free.borrow_mut().p_dirty.take();
        debug_assert!(p_pager.n_extra >= 8);
        p_free.borrow_mut().p_extra[..8].fill(0);
        p = p_free;
    } else {
        let mut p_extra: Vec<u8> = Vec::new();
        if p_extra.try_reserve_exact(p_pager.n_extra as usize).is_err() {
            *pp_page = None;
            os_unfetch(&mut p_pager.fd, (pgno as i64 - 1) * p_pager.page_size as i64);
            return SQLITE_NOMEM_BKPT;
        }
        p_extra.resize(p_pager.n_extra as usize, 0);
        let mut hdr = PgHdr::default();
        hdr.p_extra = p_extra;
        hdr.flags = PGHDR_MMAP;
        hdr.n_ref = 1;
        p = Rc::new(RefCell::new(hdr));
    }

    debug_assert!(p.borrow().p_page.is_none());
    debug_assert!(p.borrow().flags == PGHDR_MMAP);
    debug_assert!(p.borrow().n_ref == 1);

    p.borrow_mut().pgno = pgno;
    p.borrow_mut().p_data = p_data;
    p_pager.n_mmap_out += 1;

    *pp_page = Some(p);
    SQLITE_OK
}


// ---- part_010.rs ----

/// Libera uma referência à página p_pg. p_pg deve ter sido devolvida por uma
/// chamada anterior a pager_acquire_map_page().
pub fn pager_release_map_page(p_pager: &mut Pager, p_pg: PgHdrRef) {
    p_pager.n_mmap_out -= 1;
    p_pg.borrow_mut().p_dirty = p_pager.p_mmap_freelist.take();
    let pgno = p_pg.borrow().pgno;
    p_pager.p_mmap_freelist = Some(p_pg);

    debug_assert!(os_file_version(&p_pager.fd) >= 3);
    os_unfetch(&mut p_pager.fd, (pgno as i64 - 1) * p_pager.page_size as i64);
}

/// Libera todos os objetos PgHdr guardados na lista Pager.p_mmap_freelist.
pub fn pager_free_map_hdrs(p_pager: &mut Pager) {
    let mut p = p_pager.p_mmap_freelist.take();
    while let Some(cur) = p {
        p = cur.borrow_mut().p_dirty.take();
    }
}

/// Verifica que o arquivo do banco de dados não foi apagado nem renomeado por
/// baixo do pager. Retorna SQLITE_OK se o banco ainda está onde deveria estar no
/// disco. Retorna não zero (SQLITE_READONLY_DBMOVED ou outro código de erro de
/// os_access()) se o banco sumiu.
pub fn database_is_unmoved(p_pager: &mut Pager) -> i32 {
    let mut b_has_moved: i32 = 0;
    let mut rc: i32;

    if p_pager.temp_file != 0 {
        return SQLITE_OK;
    }
    if p_pager.db_size == 0 {
        return SQLITE_OK;
    }
    debug_assert!(!p_pager.z_filename.is_empty() && p_pager.z_filename[0] != 0);
    rc = os_file_control(&mut p_pager.fd, SQLITE_FCNTL_HAS_MOVED, &mut b_has_moved);
    if rc == SQLITE_NOTFOUND {
        /* Se o file-control HAS_MOVED não está implementado, assume que o arquivo
        ** não foi movido. Esse é o comportamento histórico do SQLite: antes da
        ** versão 3.8.3, ele nunca checava */
        rc = SQLITE_OK;
    } else if rc == SQLITE_OK && b_has_moved != 0 {
        rc = SQLITE_READONLY_DBMOVED;
    }
    rc
}

/// Desliga o cache de páginas. Libera toda a memória e fecha todos os arquivos.
///
/// Se uma transação estava em andamento quando esta rotina é chamada, ela é
/// revertida. Todas as páginas pendentes são invalidadas e sua memória é liberada.
///
/// Esta função sempre tem sucesso. Se há uma transação ativa, tenta-se revertê-la.
/// Se ocorrer um erro durante o rollback, um hot journal pode ficar no sistema de
/// arquivos, mas nenhum erro é devolvido ao chamador.
///
/// Modelagem: o Pager é dono dos seus campos; os "free" do C são quedas de valores.
/// BeginBenignMalloc/EndBenignMalloc e disable/enable_simulated_io_errors só têm
/// efeito em builds de teste e somem.
pub fn pager_close(mut p_pager: Box<Pager>, db: Option<&Sqlite3Ref>) -> i32 {
    let mut p_tmp: Vec<u8> = std::mem::take(&mut p_pager.p_tmp_space);
    debug_assert!(db.is_some() || !pager_use_wal(&p_pager));
    pager_free_map_hdrs(&mut p_pager);
    p_pager.exclusive_mode = 0;
    {
        debug_assert!(db.is_some() || p_pager.p_wal.is_none());
        let use_tmp = match db {
            Some(d) => {
                (d.borrow().flags & SQLITE_NOCKPTONCLOSE) == 0
                    && SQLITE_OK == database_is_unmoved(&mut p_pager)
            }
            None => false,
        };
        let p_wal = p_pager.p_wal.take();
        wal_close(
            p_wal,
            db,
            p_pager.wal_sync_flags,
            p_pager.page_size,
            if use_tmp { Some(&mut p_tmp[..]) } else { None },
        );
    }
    pager_reset(&mut p_pager);
    if p_pager.mem_db != 0 {
        pager_unlock(&mut p_pager);
    } else {
        /* Se estiver aberto, sincroniza o arquivo de journal antes de chamar
        ** pager_unlock_and_rollback(). Sem isso, uma parte não sincronizada do
        ** journal aberto pode ser reproduzida no banco de dados. Se faltar energia
        ** enquanto isso acontece, o banco pode ficar corrompido.
        **
        ** Se ocorrer um erro ao tentar sincronizar o journal, leva o pager ao
        ** estado ERROR. Isso faz pager_unlock_and_rollback() destravar o banco e
        ** fechar o journal sem tentar revertê-lo ou finalizá-lo. O próximo usuário
        ** do banco terá de fazer o rollback do hot-journal antes de acessar o
        ** arquivo do banco de dados. */
        if is_open(&p_pager.jfd) {
            let rc_sync = pager_sync_hot_journal(&mut p_pager);
            pager_error(&mut p_pager, rc_sync);
        }
        pager_unlock_and_rollback(&mut p_pager);
    }
    os_close(&mut p_pager.jfd);
    os_close(&mut p_pager.fd);
    drop(p_tmp);
    pcache_close(&mut p_pager.p_pcache);
    debug_assert!(p_pager.a_savepoint.is_empty() && p_pager.p_in_journal.is_none());
    debug_assert!(!is_open(&p_pager.jfd) && !is_open(&p_pager.sjfd));

    SQLITE_OK
}

/// Incrementa a contagem de referências da página p_pg.
pub fn pager_ref(p_pg: &mut PgHdr) {
    pcache_ref(p_pg);
}

/// Sincroniza o journal. Em outras palavras, garante que todas as páginas escritas
/// no journal realmente chegaram à superfície do disco e podem ser restauradas
/// num rollback de hot-journal.
///
/// Se o flag Pager.no_sync estiver definido, esta função não faz nada. Caso
/// contrário, as ações exigidas dependem do modo de journal e das características
/// do dispositivo do sistema de arquivos, assim:
///
///   * Se o arquivo de journal é um journal em memória, nada precisa ser feito.
///
///   * Caso contrário, se o dispositivo não suporta a propriedade SAFE_APPEND, o
///     campo nRec do cabeçalho de journal escrito mais recentemente é atualizado
///     para conter o número de registros escritos depois dele. Se o pager opera
///     em modo full-sync, o arquivo de journal é sincronizado antes de esse campo
///     ser atualizado.
///
///   * Se o dispositivo não suporta a propriedade SEQUENTIAL, o arquivo de journal
///     é sincronizado.
///
/// Ou, em pseudocódigo:
///
///   if( NOT <journal em memória> ){
///     if( NOT SAFE_APPEND ){
///       if( <modo full-sync> ) xSync(<arquivo de journal>);
///       <atualiza o campo nRec>
///     }
///     if( NOT SEQUENTIAL ) xSync(<arquivo de journal>);
///   }
///
/// Se tiver sucesso, esta rotina limpa o flag PGHDR_NEED_SYNC de toda página
/// mantida em memória antes de retornar SQLITE_OK. Se ocorrer um erro de E/S, o
/// código de erro é retornado ao chamador.
pub fn sync_journal(p_pager: &mut Pager, new_hdr: i32) -> i32 {
    let mut rc: i32; /* Código de retorno */

    debug_assert!(
        p_pager.e_state == PAGER_WRITER_CACHEMOD || p_pager.e_state == PAGER_WRITER_DBMOD
    );
    debug_assert!(!pager_use_wal(p_pager));

    rc = pager_exclusive_lock(p_pager);
    if rc != SQLITE_OK {
        return rc;
    }

    if p_pager.no_sync == 0 {
        debug_assert!(p_pager.temp_file == 0);
        if is_open(&p_pager.jfd) && p_pager.journal_mode != PAGER_JOURNALMODE_MEMORY {
            let i_dc: i32 = os_device_characteristics(&p_pager.fd);
            debug_assert!(is_open(&p_pager.jfd));

            if 0 == (i_dc & SQLITE_IOCAP_SAFE_APPEND) {
                /* Este bloco trata de um problema obscuro. Se a última conexão que
                ** escreveu neste banco operava em modo de journal persistente, o
                ** arquivo de journal pode, neste ponto, ser maior que Pager.journal_off
                ** bytes. Se a próxima coisa no arquivo for um cabeçalho de journal
                ** (escrito na transação da conexão anterior), e ocorrer uma queda ou
                ** falta de energia depois de nRec ser atualizado mas antes de esta
                ** conexão escrever qualquer outra coisa no journal (ou confirmar ou
                ** reverter a transação), o SQLite pode se confundir no rollback do
                ** hot-journal após a recuperação. Pode reverter todos os dados desta
                ** conexão e depois seguir revertendo os dados antigos e defasados que
                ** vêm em seguida. Corrupção do banco.
                **
                ** Para contornar, se o arquivo de journal parecer conter um cabeçalho
                ** válido depois de Pager.journal_off, escreve um byte 0x00 no início
                ** dele para impedir que seja reconhecido.
                **
                ** i_next_hdr_offset recebe o deslocamento em que esse cabeçalho
                ** problemático ocorrerá, se existir. a_magic é um buffer temporário
                ** para inspecionar os primeiros bytes do possível cabeçalho. */
                let i_next_hdr_offset: i64;
                let mut a_magic = [0u8; 8];
                let mut z_header = [0u8; A_JOURNAL_MAGIC.len() + 4];

                z_header[..A_JOURNAL_MAGIC.len()].copy_from_slice(&A_JOURNAL_MAGIC);
                put32bits(&mut z_header[A_JOURNAL_MAGIC.len()..], p_pager.n_rec);

                i_next_hdr_offset = journal_hdr_offset(p_pager);
                rc = os_read(&mut p_pager.jfd, &mut a_magic, i_next_hdr_offset);
                if rc == SQLITE_OK && a_magic[..] == A_JOURNAL_MAGIC[..8] {
                    let zerobyte: [u8; 1] = [0];
                    rc = os_write(&mut p_pager.jfd, &zerobyte, i_next_hdr_offset);
                }
                if rc != SQLITE_OK && rc != SQLITE_IOERR_SHORT_READ {
                    return rc;
                }

                /* Escreve o valor de nRec no cabeçalho do arquivo de journal. Em
                ** modo full-synchronous, sincroniza o journal antes. Isso garante que
                ** todos os dados realmente chegaram ao disco antes de nRec ser
                ** atualizado para marcá-lo como candidato a rollback.
                **
                ** Isso não é necessário se a mídia persistente suporta a propriedade
                ** SAFE_APPEND. Nesse caso não é possível anexar lixo ao arquivo, então
                ** o campo nRec é preenchido com 0xFFFFFFFF quando o cabeçalho do
                ** journal é escrito e nunca precisa ser atualizado. */
                if p_pager.full_sync != 0 && 0 == (i_dc & SQLITE_IOCAP_SEQUENTIAL) {
                    rc = os_sync(&mut p_pager.jfd, p_pager.sync_flags);
                    if rc != SQLITE_OK {
                        return rc;
                    }
                }
                rc = os_write(&mut p_pager.jfd, &z_header, p_pager.journal_hdr);
                if rc != SQLITE_OK {
                    return rc;
                }
            }
            if 0 == (i_dc & SQLITE_IOCAP_SEQUENTIAL) {
                rc = os_sync(
                    &mut p_pager.jfd,
                    p_pager.sync_flags
                        | (if p_pager.sync_flags == SQLITE_SYNC_FULL {
                            SQLITE_SYNC_DATAONLY
                        } else {
                            0
                        }),
                );
                if rc != SQLITE_OK {
                    return rc;
                }
            }

            p_pager.journal_hdr = p_pager.journal_off;
            if new_hdr != 0 && 0 == (i_dc & SQLITE_IOCAP_SAFE_APPEND) {
                p_pager.n_rec = 0;
                rc = write_journal_hdr(p_pager);
                if rc != SQLITE_OK {
                    return rc;
                }
            }
        } else {
            p_pager.journal_hdr = p_pager.journal_off;
        }
    }

    /* A menos que o pager esteja em modo no_sync, o arquivo de journal acabou de
    ** ser sincronizado com sucesso. De qualquer forma, limpa o flag PGHDR_NEED_SYNC
    ** em todas as páginas. */
    pcache_clear_sync_flags(&mut p_pager.p_pcache);
    p_pager.e_state = PAGER_WRITER_DBMOD;
    SQLITE_OK
}

/// Grava no arquivo do banco de dados uma única página da lista de páginas sujas
/// (corpo do laço de pager_write_pagelist).
fn pager_write_one_page(p_pager: &mut Pager, p_list: &mut PgHdr) -> i32 {
    let mut rc: i32 = SQLITE_OK;
    let pgno: u32 = p_list.pgno;

    /* Se há páginas sujas no cache com números maiores que Pager.db_size, é porque
    ** pager_truncate_image() foi chamada para diminuir o arquivo (presumivelmente
    ** pelo código de auto-vacuum). Não grava essas páginas no arquivo.
    **
    ** Também não grava páginas com o flag PGHDR_DONT_WRITE (definido por
    ** pager_dont_write()). */
    if pgno <= p_pager.db_size && 0 == (p_list.flags & PGHDR_DONT_WRITE) {
        let offset: i64 = (pgno as i64 - 1) * p_pager.page_size as i64; /* Deslocamento de escrita */

        debug_assert!((p_list.flags & PGHDR_NEED_SYNC) == 0);
        if p_list.pgno == 1 {
            pager_write_changecounter(p_list);
        }

        /* Escreve os dados da página. */
        rc = os_write(
            &mut p_pager.fd,
            &p_list.p_data[..p_pager.page_size as usize],
            offset,
        );

        /* Se a página 1 acabou de ser escrita, atualiza Pager.db_file_vers para
        ** casar com o valor agora guardado no arquivo do banco. Se escrever esta
        ** página fez o arquivo crescer, atualiza db_file_size. */
        if pgno == 1 {
            let n = p_pager.db_file_vers.len();
            p_pager.db_file_vers.copy_from_slice(&p_list.p_data[24..24 + n]);
        }
        if pgno > p_pager.db_file_size {
            p_pager.db_file_size = pgno;
        }
        p_pager.a_stat[PAGER_STAT_WRITE] += 1;

        /* Atualiza quaisquer objetos de backup que copiam o conteúdo deste pager. */
        backup_update(&p_pager.p_backup, pgno, &p_list.p_data);
    }
    rc
}

/// O argumento é o primeiro de uma lista encadeada de páginas sujas ligadas pelo
/// ponteiro PgHdr.p_dirty. Esta função grava no arquivo do banco de dados cada
/// página em memória da lista.
///
/// O pager deve manter ao menos um bloqueio RESERVED quando esta função é
/// chamada. Antes de gravar qualquer coisa no arquivo, esse bloqueio é elevado a
/// EXCLUSIVE. Se o bloqueio não puder ser obtido, SQLITE_BUSY é retornado e nenhum
/// dado é gravado no arquivo do banco de dados.
///
/// Se o pager é de arquivo temporário e o arquivo real do sistema de arquivos
/// ainda não está aberto, ele é criado e aberto antes de qualquer dado ser gravado.
///
/// Depois de elevar o bloqueio e, se preciso, abrir o arquivo, as páginas são
/// gravadas no arquivo do banco na ordem da lista. Gravar uma página é pulado se
/// ela atende a um destes critérios:
///
///   * O número da página é maior que Pager.db_size, ou
///   * O flag PGHDR_DONT_WRITE está definido na página.
///
/// Se gravar uma página faz o arquivo crescer, Pager.db_file_size é atualizado.
/// Se a página 1 é gravada, o valor em cache em Pager.db_file_vers[] é atualizado
/// para casar com o novo valor guardado no arquivo do banco.
///
/// Se tudo der certo, SQLITE_OK é retornado. Se ocorrer um erro de E/S, um código
/// de erro de E/S é retornado. Ou, se o bloqueio EXCLUSIVE não puder ser obtido,
/// SQLITE_BUSY é retornado.
///
/// Modelagem: a cabeça da lista chega como &mut PgHdr (a lista nunca é vazia no
/// uso real: o chamador sempre passa uma página); o restante segue por p_dirty.
pub fn pager_write_pagelist(p_pager: &mut Pager, p_list: &mut PgHdr) -> i32 {
    let mut rc = SQLITE_OK; /* Código de retorno */

    /* Esta função só é chamada para pagers de rollback em estado WRITER_DBMOD. */
    debug_assert!(!pager_use_wal(p_pager));
    debug_assert!(p_pager.temp_file != 0 || p_pager.e_state == PAGER_WRITER_DBMOD);
    debug_assert!(p_pager.e_lock as i32 == EXCLUSIVE_LOCK);
    debug_assert!(is_open(&p_pager.fd) || p_list.p_dirty.is_none());

    /* Se o arquivo é temporário e ainda não foi aberto, abre agora. Não é possível
    ** rc ser diferente de SQLITE_OK se este ramo for tomado, pois
    ** pager_wait_on_lock() não faz nada para arquivos temporários. */
    if !is_open(&p_pager.fd) {
        debug_assert!(p_pager.temp_file != 0 && rc == SQLITE_OK);
        let vfs_flags = p_pager.vfs_flags;
        rc = pager_opentemp(p_pager, vfs_flags);
    }

    /* Antes da primeira escrita, dá ao VFS uma dica do tamanho final do arquivo. */
    debug_assert!(rc != SQLITE_OK || is_open(&p_pager.fd));
    if rc == SQLITE_OK
        && p_pager.db_hint_size < p_pager.db_size
        && (p_list.p_dirty.is_some() || p_list.pgno > p_pager.db_hint_size)
    {
        let mut sz_file: i64 = p_pager.page_size as i64 * p_pager.db_size as i64;
        os_file_control_hint(&mut p_pager.fd, SQLITE_FCNTL_SIZE_HINT, &mut sz_file);
        p_pager.db_hint_size = p_pager.db_size;
    }

    if rc == SQLITE_OK {
        rc = pager_write_one_page(p_pager, p_list);
        let mut p_next: Option<PgHdrRef> = p_list.p_dirty.clone();
        while rc == SQLITE_OK {
            let p_cur = match p_next {
                Some(p) => p,
                None => break,
            };
            rc = pager_write_one_page(p_pager, &mut p_cur.borrow_mut());
            p_next = p_cur.borrow().p_dirty.clone();
        }
    }

    rc
}


// ---- part_011.rs ----

/// Garante que o arquivo de sub-journal esteja aberto. Se já estiver aberto,
/// esta função não faz nada.
///
/// SQLITE_OK é retornado se tudo correr bem. Um código de erro
/// SQLITE_IOERR_XXX é retornado se uma chamada a os_open() falhar.
pub fn open_sub_journal(p_pager: &mut Pager) -> i32 {
    let mut rc = SQLITE_OK;
    if !is_open(&p_pager.sjfd) {
        let flags: i32 = SQLITE_OPEN_SUBJOURNAL
            | SQLITE_OPEN_READWRITE
            | SQLITE_OPEN_CREATE
            | SQLITE_OPEN_EXCLUSIVE
            | SQLITE_OPEN_DELETEONCLOSE;
        let mut n_stmt_spill: i32 = sqlite3_config().n_stmt_spill;
        if p_pager.journal_mode == PAGER_JOURNALMODE_MEMORY || p_pager.subj_in_memory != 0 {
            n_stmt_spill = -1;
        }
        rc = journal_open(&p_pager.p_vfs, None, &mut p_pager.sjfd, flags, n_stmt_spill);
    }
    rc
}

/// Anexa um registro do estado atual da página pPg ao sub-journal.
///
/// Se bem sucedido, define o bit correspondente a pPg.pgno nos bitvecs de
/// todos os savepoints abertos antes de retornar.
///
/// Esta função retorna SQLITE_OK se tudo for bem sucedido, um código de erro
/// de E/S se a tentativa de escrever no sub-journal falhar, ou SQLITE_NOMEM
/// se um malloc falhar ao definir um bit em um bitvec de savepoint.
///
/// Modelagem: o Pager (que em C vem de pPg->pPager) é passado como argumento
/// explícito, para não haver dois empréstimos mutáveis ao mesmo tempo.
pub fn subjournal_page(p_pager: &mut Pager, p_pg: &mut PgHdr) -> i32 {
    let mut rc = SQLITE_OK;
    if p_pager.journal_mode != PAGER_JOURNALMODE_OFF {
        /* Abre o sub-journal, se ainda não tiver sido aberto */
        debug_assert!(p_pager.use_journal != 0);
        debug_assert!(is_open(&p_pager.jfd) || pager_use_wal(p_pager));
        debug_assert!(is_open(&p_pager.sjfd) || p_pager.n_sub_rec == 0);
        debug_assert!(
            pager_use_wal(p_pager)
                || page_in_journal(p_pager, p_pg)
                || p_pg.pgno > p_pager.db_orig_size
        );
        rc = open_sub_journal(p_pager);

        /* Se o sub-journal foi aberto com sucesso (ou já estava aberto),
        ** escreve o registro do journal no arquivo. */
        if rc == SQLITE_OK {
            let offset: i64 = (p_pager.n_sub_rec as i64) * (4 + p_pager.page_size as i64);
            rc = write32bits(&mut p_pager.sjfd, offset, p_pg.pgno);
            if rc == SQLITE_OK {
                rc = os_write(
                    &mut p_pager.sjfd,
                    &p_pg.p_data[..p_pager.page_size as usize],
                    offset + 4,
                );
            }
        }
    }
    if rc == SQLITE_OK {
        p_pager.n_sub_rec += 1;
        debug_assert!(p_pager.n_savepoint > 0);
        rc = add_to_savepoint_bitvecs(p_pager, p_pg.pgno);
    }
    rc
}

pub fn subjournal_page_if_required(p_pager: &mut Pager, p_pg: &mut PgHdr) -> i32 {
    if subj_requires_page(p_pager, p_pg) {
        subjournal_page(p_pager, p_pg)
    } else {
        SQLITE_OK
    }
}

/// Esta função é chamada pela camada pcache quando ela atinge algum limite
/// suave de memória. O primeiro argumento é o objeto Pager (em C, um void*).
/// O pager é sempre 'purgeable' (não é um banco de dados em memória). O segundo
/// argumento é uma referência a uma página que está suja mas não tem referências
/// pendentes. A página está sempre associada ao Pager do primeiro argumento.
///
/// O trabalho desta função é tornar pPg limpa escrevendo seu conteúdo no
/// arquivo do banco de dados, se possível. Isso pode envolver sincronizar o
/// arquivo de journal.
///
/// Se bem sucedido, pcache_make_clean() é chamado na página e SQLITE_OK é
/// retornado. Se ocorrer um erro de E/S ao tentar limpar a página, o código
/// de erro de E/S é retornado. Se a página não puder ser limpa por outro
/// motivo, mas sem erro, SQLITE_OK é retornado e pcache_make_clean() não é chamado.
pub fn pager_stress(p_pager: &mut Pager, p_pg: &mut PgHdr) -> i32 {
    let mut rc = SQLITE_OK;

    debug_assert!((p_pg.flags & PGHDR_DIRTY) != 0);

    /* O bit NOSYNC de doNotSpill é definido nos momentos em que sincronizar o
    ** journal (e adicionar um novo cabeçalho) não é permitido. Isso ocorre
    ** durante chamadas a pager_write() ao tentar gravar no journal várias
    ** páginas do mesmo setor.
    **
    ** Os bits ROLLBACK e OFF de doNotSpill inibem todo derramamento de cache,
    ** exigindo sincronização ou não. São definidos durante um rollback ou por
    ** pedido do usuário, respectivamente.
    **
    ** O derramamento também é proibido em estado de erro, pois poderia corromper
    ** o banco de dados. Na implementação atual é impossível pcache_fetch() ser
    ** chamado com createFlag==3 em estado de erro, logo é impossível esta rotina
    ** ser chamada nesse estado. Mesmo assim, o teste NEVER() do estado de erro
    ** fica como salvaguarda contra mudanças futuras. */
    if p_pager.err_code != 0 {
        return SQLITE_OK;
    }
    if p_pager.do_not_spill != 0
        && ((p_pager.do_not_spill & (SPILLFLAG_ROLLBACK | SPILLFLAG_OFF)) != 0
            || (p_pg.flags & PGHDR_NEED_SYNC) != 0)
    {
        return SQLITE_OK;
    }

    p_pager.a_stat[PAGER_STAT_SPILL] += 1;
    p_pg.p_dirty = None;
    if pager_use_wal(p_pager) {
        /* Escreve um único frame desta página no log. */
        rc = subjournal_page_if_required(p_pager, p_pg);
        if rc == SQLITE_OK {
            rc = pager_wal_frames(p_pager, Some(p_pg), 0, 0);
        }
    } else {
        /* Sincroniza o arquivo de journal se necessário. */
        if (p_pg.flags & PGHDR_NEED_SYNC) != 0 || p_pager.e_state == PAGER_WRITER_CACHEMOD {
            rc = sync_journal(p_pager, 1);
        }

        /* Escreve o conteúdo da página no arquivo do banco de dados. */
        if rc == SQLITE_OK {
            debug_assert!((p_pg.flags & PGHDR_NEED_SYNC) == 0);
            rc = pager_write_pagelist(p_pager, p_pg);
        }
    }

    /* Marca a página como limpa. */
    if rc == SQLITE_OK {
        pcache_make_clean(p_pg);
    }

    pager_error(p_pager, rc)
}

/// Descarrega no disco todas as páginas sujas sem referência.
pub fn pager_flush(p_pager: &mut Pager) -> i32 {
    let mut rc = p_pager.err_code;
    if p_pager.mem_db == 0 {
        let mut p_list = pcache_dirty_list(&p_pager.p_pcache);
        while rc == SQLITE_OK {
            let p_cur = match p_list {
                Some(p) => p,
                None => break,
            };
            let p_next = p_cur.borrow().p_dirty.clone();
            if p_cur.borrow().n_ref == 0 {
                rc = pager_stress(p_pager, &mut p_cur.borrow_mut());
            }
            p_list = p_next;
        }
    }

    rc
}

/// Comprimento até o primeiro byte zero (equivalente ao sqlite3Strlen30 sobre
/// um buffer terminado em zero).
fn pager_open_strlen(buf: &[u8], start: usize) -> usize {
    let mut n = 0;
    while start + n < buf.len() && buf[start + n] != 0 {
        n += 1;
    }
    n
}

/// Aloca e inicializa um novo objeto Pager e o devolve em *ppPager. O pager
/// deve ser liberado passando-o a pager_close().
///
/// O argumento z_filename é o caminho do arquivo do banco de dados a abrir, no
/// formato do SQLite: nome, byte zero, pares chave/valor da URI (cada um
/// terminado em zero) e um zero final. Se for None, um arquivo temporário de
/// nome aleatório é criado e usado como o arquivo em cache. Arquivos temporários
/// são apagados automaticamente ao serem fechados. Se for ":memory:", toda a
/// informação fica em cache e nunca é escrita no disco.
///
/// O parâmetro n_extra especifica o número de bytes de espaço alocado junto com
/// cada referência de página. Quando uma nova página é alocada, os 8 primeiros
/// bytes desse espaço são zerados, mas o restante não é inicializado. (O espaço
/// extra é usado pelo btree como o objeto MemPage.)
///
/// O argumento flags especifica propriedades que afetam a operação do pager:
/// uma combinação bit a bit dos flags PAGER_*.
///
/// O parâmetro vfs_flags é a máscara passada ao parâmetro flags do método xOpen()
/// do VFS ao abrir arquivos.
///
/// Se o objeto pager for alocado e o arquivo aberto com sucesso, SQLITE_OK é
/// retornado e *pp_pager aponta para o novo pager. Em caso de erro, *pp_pager
/// fica None e o código de erro é retornado. Pode retornar SQLITE_NOMEM,
/// SQLITE_CANTOPEN ou vários erros SQLITE_IOERR_XXX.
///
/// Modelagem: o bloco de memória único do C (Pager, PCache, três arquivos, nomes)
/// não existe sem ponteiros. Cada parte vira campo próprio do Pager: z_filename
/// (nome, zero, parâmetros da URI), z_journal e z_wal (sem o prefixo de quatro
/// zeros, que só serve para o sqlite3_filename_database() por aritmética de ponteiro).
pub fn pager_open(
    p_vfs: Rc<dyn Vfs>,
    pp_pager: &mut Option<Box<Pager>>,
    z_filename_in: Option<&[u8]>,
    n_extra: i32,
    flags: i32,
    mut vfs_flags: i32,
    x_reinit: Option<fn(&mut DbPage)>,
) -> i32 {
    let mut rc = SQLITE_OK; /* Código de retorno */
    let mut temp_file: i32 = 0; /* Verdadeiro para arquivos temporários (inclusive em memória) */
    let mut mem_db: i32 = 0; /* Verdadeiro se o arquivo é em memória */
    let mut mem_jm: i32 = 0; /* Modo de journal em memória */
    let mut read_only: i32 = 0; /* Verdadeiro se o arquivo é somente leitura */
    let mut z_pathname: Vec<u8> = Vec::new(); /* Caminho completo do banco de dados */
    let mut n_pathname: usize = 0; /* Número de bytes de z_pathname */
    let use_journal = (flags & PAGER_OMIT_JOURNAL) == 0; /* Falso para omitir o journal */
    let mut sz_page_dflt: u32 = SQLITE_DEFAULT_PAGE_SIZE as u32; /* Tamanho de página padrão */
    let mut z_uri: Option<usize> = None; /* Início dos argumentos da URI em z_filename */
    let mut n_uri_byte: usize = 1; /* Número de bytes dos argumentos da URI */
    let mut z_filename: Option<&[u8]> = z_filename_in;

    /* Define a variável de saída como None caso ocorra um erro. */
    *pp_pager = None;

    if (flags & PAGER_MEMORY) != 0 {
        mem_db = 1;
        if let Some(f) = z_filename {
            if !f.is_empty() && f[0] != 0 {
                let n = pager_open_strlen(f, 0);
                z_pathname = f[..n].to_vec();
                n_pathname = strlen30(&z_pathname);
                z_filename = None;
            }
        }
    }

    /* Calcula e guarda o caminho completo em z_pathname, de comprimento
    ** n_pathname. Ou, se for um arquivo temporário, deixa ambos em 0. */
    if let Some(f) = z_filename {
        if !f.is_empty() && f[0] != 0 {
            n_pathname = p_vfs.mx_pathname() as usize + 1;
            z_pathname = vec![0u8; n_pathname * 2];
            /* z_pathname[0] já é 0: garante inicialização mesmo que full_pathname() falhe */
            rc = os_full_pathname(&p_vfs, f, n_pathname as i32, &mut z_pathname);
            if rc != SQLITE_OK {
                if rc == SQLITE_OK_SYMLINK {
                    if (vfs_flags & SQLITE_OPEN_NOFOLLOW) != 0 {
                        rc = SQLITE_CANTOPEN_SYMLINK;
                    } else {
                        rc = SQLITE_OK;
                    }
                }
            }
            n_pathname = pager_open_strlen(&z_pathname, 0);
            z_pathname.truncate(n_pathname);
            let uri_start = pager_open_strlen(f, 0) + 1;
            z_uri = Some(uri_start);
            let mut z = uri_start;
            while z < f.len() && f[z] != 0 {
                z += pager_open_strlen(f, z) + 1;
                z += pager_open_strlen(f, z) + 1;
            }
            n_uri_byte = z + 1 - uri_start;
            debug_assert!(n_uri_byte >= 1);
            if rc == SQLITE_OK && (n_pathname as i32 + 8 > p_vfs.mx_pathname()) {
                /* Este ramo é tomado quando o caminho do journal exigido pelo banco
                ** de dados aberto passa de mxPathname bytes. O banco não pode ser
                ** aberto, pois não seria possível abrir o journal nem checar um
                ** hot-journal antes de ler. */
                rc = SQLITE_CANTOPEN_BKPT;
            }
            if rc != SQLITE_OK {
                return rc;
            }
        }
    }

    let mut p_pager: Box<Pager> = Box::new(Pager::default());

    /* Preenche Pager.z_filename: nome, zero, parâmetros da URI (ou um zero). */
    if n_pathname > 0 {
        let mut v = z_pathname[..n_pathname].to_vec();
        v.push(0);
        match (z_uri, z_filename) {
            (Some(u), Some(f)) => v.extend_from_slice(&f[u..u + n_uri_byte]),
            _ => v.push(0),
        }
        p_pager.z_filename = v;
    } else {
        p_pager.z_filename = Vec::new();
    }

    /* Preenche Pager.z_journal */
    if n_pathname > 0 {
        let mut v = z_pathname[..n_pathname].to_vec();
        v.extend_from_slice(b"-journal");
        v.push(0);
        p_pager.z_journal = Some(v);
    } else {
        p_pager.z_journal = None;
    }

    /* Preenche Pager.z_wal */
    if n_pathname > 0 {
        let mut v = z_pathname[..n_pathname].to_vec();
        v.extend_from_slice(b"-wal");
        v.push(0);
        p_pager.z_wal = Some(v);
    } else {
        p_pager.z_wal = None;
    }

    p_pager.p_vfs = p_vfs.clone();
    p_pager.vfs_flags = vfs_flags;

    /* Abre o arquivo do pager. */
    let mut act_like_temp_file = false;
    if let Some(f) = z_filename {
        if !f.is_empty() && f[0] != 0 {
            let mut fout: i32 = 0; /* Flags do VFS devolvidos por xOpen() */
            rc = os_open(
                &p_vfs,
                Some(&p_pager.z_filename),
                &mut p_pager.fd,
                vfs_flags,
                Some(&mut fout),
            );
            debug_assert!(mem_db == 0);
            mem_jm = ((fout & SQLITE_OPEN_MEMORY) != 0) as i32;
            p_pager.mem_vfs = mem_jm as u8;
            read_only = ((fout & SQLITE_OPEN_READONLY) != 0) as i32;

            /* Se o arquivo foi aberto com sucesso para leitura e escrita, escolhe
            ** um tamanho de página padrão para o caso de ser preciso criar o banco.
            ** O padrão é o máximo entre:
            **
            **    + SQLITE_DEFAULT_PAGE_SIZE,
            **    + o valor devolvido por os_sector_size()
            **    + o maior tamanho de página que pode ser escrito atomicamente. */
            if rc == SQLITE_OK {
                let i_dc: i32 = os_device_characteristics(&p_pager.fd);
                if read_only == 0 {
                    set_sector_size(&mut p_pager);
                    debug_assert!(SQLITE_DEFAULT_PAGE_SIZE <= SQLITE_MAX_DEFAULT_PAGE_SIZE);
                    if sz_page_dflt < p_pager.sector_size {
                        if p_pager.sector_size > SQLITE_MAX_DEFAULT_PAGE_SIZE as u32 {
                            sz_page_dflt = SQLITE_MAX_DEFAULT_PAGE_SIZE as u32;
                        } else {
                            sz_page_dflt = p_pager.sector_size;
                        }
                    }
                }
                p_pager.no_lock = api::uri_boolean(&p_pager.z_filename, b"nolock", 0) as u8;
                if (i_dc & SQLITE_IOCAP_IMMUTABLE) != 0
                    || api::uri_boolean(&p_pager.z_filename, b"immutable", 0) != 0
                {
                    vfs_flags |= SQLITE_OPEN_READONLY;
                    act_like_temp_file = true; /* goto act_like_temp_file */
                }
            }
        } else {
            act_like_temp_file = true;
        }
    } else {
        act_like_temp_file = true;
    }
    if act_like_temp_file {
        /* Se um arquivo temporário é pedido, ele não é aberto de imediato. Aceita-se
        ** o tamanho de página padrão e a abertura real é adiada até a primeira
        ** chamada a os_write().
        **
        ** Este ramo também roda para um banco em memória, que é igual a um arquivo
        ** temporário nunca escrito em disco e que usa um journal de rollback em
        ** memória.
        **
        ** Este ramo também roda para arquivos marcados como imutáveis. */
        temp_file = 1;
        p_pager.e_state = PAGER_READER; /* Finge que já temos um bloqueio */
        p_pager.e_lock = EXCLUSIVE_LOCK; /* Finge estar em modo EXCLUSIVE */
        p_pager.no_lock = 1; /* Não faz bloqueio */
        read_only = vfs_flags & SQLITE_OPEN_READONLY;
    }

    /* A chamada a pager_set_pagesize() define Pager.page_size e aloca o buffer
    ** Pager.p_tmp_space. */
    if rc == SQLITE_OK {
        debug_assert!(p_pager.mem_db == 0);
        rc = pager_set_pagesize(&mut p_pager, &mut sz_page_dflt, -1);
    }

    /* Inicializa o objeto PCache. */
    let n_extra = round8(n_extra);
    if rc == SQLITE_OK {
        debug_assert!(n_extra >= 8 && n_extra < 1000);
        let x_stress: Option<fn(&mut Pager, &mut PgHdr) -> i32> =
            if mem_db == 0 { Some(pager_stress) } else { None };
        rc = pcache_open(
            sz_page_dflt as i32,
            n_extra,
            (mem_db == 0) as i32,
            x_stress,
            &mut p_pager.p_pcache,
        );
    }

    /* Se ocorreu um erro acima, libera a estrutura Pager e fecha o arquivo. */
    if rc != SQLITE_OK {
        os_close(&mut p_pager.fd);
        return rc;
    }

    p_pager.use_journal = use_journal as u8;
    p_pager.mx_pgno = SQLITE_MAX_PAGE_COUNT;
    p_pager.temp_file = temp_file as u8;
    debug_assert!(
        temp_file == PAGER_LOCKINGMODE_NORMAL as i32
            || temp_file == PAGER_LOCKINGMODE_EXCLUSIVE as i32
    );
    debug_assert!(PAGER_LOCKINGMODE_EXCLUSIVE == 1);
    p_pager.exclusive_mode = temp_file as u8;
    p_pager.change_count_done = p_pager.temp_file;
    p_pager.mem_db = mem_db as u8;
    p_pager.read_only = read_only as u8;
    debug_assert!(use_journal || p_pager.temp_file != 0);
    pager_set_flags(&mut p_pager, ((SQLITE_DEFAULT_SYNCHRONOUS + 1) as u32) | PAGER_CACHESPILL);
    p_pager.n_extra = n_extra as u16;
    p_pager.journal_size_limit = SQLITE_DEFAULT_JOURNAL_SIZE_LIMIT;
    debug_assert!(is_open(&p_pager.fd) || temp_file != 0);
    set_sector_size(&mut p_pager);
    if !use_journal {
        p_pager.journal_mode = PAGER_JOURNALMODE_OFF;
    } else if mem_db != 0 || mem_jm != 0 {
        p_pager.journal_mode = PAGER_JOURNALMODE_MEMORY;
    }
    p_pager.x_reiniter = x_reinit;
    set_getter_method(&mut p_pager);

    *pp_pager = Some(p_pager);
    SQLITE_OK
}


// ---- part_012.rs ----

/// Retorna o `Sqlite3File` do banco de dados principal dado o pager correspondente
/// ao nome de WAL ou de journal passado ao xOpen (`sqlite3_database_file_object`).
///
/// No C, o VFS recebe o nome do arquivo e recua pelos quatro bytes nulos para ler o
/// ponteiro do `Pager` gravado antes do nome. Sem ponteiros, quem abre o arquivo
/// resolve o pager pelo nome e o passa aqui; o resultado é o mesmo `pPager->fd`.
pub fn database_file_object(p_pager: &Pager) -> Option<&Sqlite3File> {
    p_pager.fd.as_deref()
}

/// Esta função é chamada depois da transição de PAGER_UNLOCK para o estado
/// PAGER_SHARED. Testa se há um hot journal no sistema de arquivos para o pager
/// dado. Um hot journal é um que precisa ser reproduzido. Segundo esta função, um
/// arquivo de hot journal existe se os seguintes critérios forem atendidos:
///
///   * O arquivo de journal existe no sistema de arquivos, e
///   * Nenhum processo mantém um bloqueio RESERVED ou maior no arquivo do banco, e
///   * O próprio arquivo do banco tem mais de 0 bytes, e
///   * O primeiro byte do arquivo de journal existe e não é 0x00.
///
/// Se o tamanho atual do arquivo do banco é 0 mas existe um arquivo de journal,
/// provavelmente é um journal antigo deixado por um banco anterior com o mesmo nome.
/// Neste caso o arquivo de journal é apenas apagado com OsDelete, *pExists vira 0 e
/// SQLITE_OK é retornado.
///
/// Esta rotina não verifica se há um nome de super-journal no fim do arquivo. Se
/// houver, e esse super-journal não existir, o journal não é realmente hot. Neste
/// caso a rotina devolve um falso positivo. A rotina pager_playback() descobrirá que
/// o journal não é realmente hot e não fará o rollback.
///
/// Se um hot journal for encontrado, *pExists vira 1 e SQLITE_OK é retornado. Se não
/// houver hot journal, *pExists vira 0 e SQLITE_OK é retornado. Se ocorrer um erro de
/// E/S ao determinar se existe um hot journal, o código do erro é retornado e o
/// valor de *pExists é indefinido.
fn has_hot_journal(p_pager: &mut Pager, p_exists: &mut i32) -> i32 {
    let mut rc = SQLITE_OK;
    let mut exists: i32 = 1;
    let jrnl_open = is_open(&p_pager.jfd);

    debug_assert!(p_pager.use_journal != 0);
    debug_assert!(is_open(&p_pager.fd));
    debug_assert!(p_pager.e_state as i32 == PAGER_OPEN);

    debug_assert!(
        !jrnl_open
            || (p_pager.jfd.as_deref().map_or(0, os_device_characteristics)
                & SQLITE_IOCAP_UNDELETABLE_WHEN_OPEN)
                != 0
    );

    *p_exists = 0;
    if !jrnl_open {
        rc = os_access(
            p_pager.p_vfs.as_deref().expect("pVfs"),
            &p_pager.z_journal,
            SQLITE_ACCESS_EXISTS,
            &mut exists,
        );
    }
    if rc == SQLITE_OK && exists != 0 {
        let mut locked: i32 = 0; // Verdadeiro se algum processo mantém um bloqueio RESERVED

        // Condição de corrida aqui: outro processo pode estar segurando o bloqueio
        // RESERVED e ter um journal aberto na chamada a os_access() acima, mas depois
        // apagar o journal e soltar o bloqueio antes de chegarmos à chamada a
        // os_check_reserved_lock() abaixo. Se for o caso, esta rotina pode achar que
        // há um hot journal quando na verdade não há. Isto resulta em um falso
        // positivo que será tratado pela rotina de playback. Ticket #3883.
        rc = os_check_reserved_lock(p_pager.fd.as_deref_mut().expect("fd"), &mut locked);
        if rc == SQLITE_OK && locked == 0 {
            let mut n_page: Pgno = 0; // Número de páginas do arquivo do banco

            debug_assert!(p_pager.temp_file == 0);
            rc = pager_pagecount(p_pager, &mut n_page);
            if rc == SQLITE_OK {
                // Se o banco tem zero páginas, significa que (1) o journal é resto de
                // um banco anterior com o mesmo nome, cujo arquivo foi apagado mas o
                // journal não, ou (2) a transação inicial que popula um banco novo
                // está sofrendo rollback. Nos dois casos o arquivo de journal pode ser
                // apagado. Porém, cuidado para não apagar o journal se ele já está
                // aberto por causa de journal_mode=PERSIST.
                if n_page == 0 && !jrnl_open {
                    begin_benign_malloc();
                    if pager_lock_db(p_pager, RESERVED_LOCK) == SQLITE_OK {
                        os_delete(p_pager.p_vfs.as_deref().expect("pVfs"), &p_pager.z_journal, 0);
                        if p_pager.exclusive_mode == 0 {
                            pager_unlock_db(p_pager, SHARED_LOCK);
                        }
                    }
                    end_benign_malloc();
                } else {
                    // O arquivo de journal existe e nenhuma outra conexão tem um
                    // bloqueio reservado ou maior no arquivo do banco. Agora confira
                    // se há pelo menos um byte não nulo no começo do journal. Se
                    // houver, consideramos o journal hot. Se não, pode ser ignorado.
                    let mut local = Sqlite3File { p_methods: None };
                    if !jrnl_open {
                        let f = SQLITE_OPEN_READONLY | SQLITE_OPEN_MAIN_JOURNAL;
                        let mut f_out: i32 = 0;
                        rc = os_open(
                            p_pager.p_vfs.as_deref().expect("pVfs"),
                            Some(&p_pager.z_journal),
                            &mut local,
                            f,
                            &mut f_out,
                        );
                    }
                    if rc == SQLITE_OK {
                        let mut first = [0u8; 1];
                        let jfd: &mut Sqlite3File = if jrnl_open {
                            p_pager.jfd.as_deref_mut().expect("jfd")
                        } else {
                            &mut local
                        };
                        rc = os_read(jfd, &mut first, 0);
                        if rc == SQLITE_IOERR_SHORT_READ {
                            rc = SQLITE_OK;
                        }
                        if !jrnl_open {
                            os_close(jfd);
                        }
                        *p_exists = (first[0] != 0) as i32;
                    } else if rc == SQLITE_CANTOPEN {
                        // Se não conseguimos abrir o journal para ver se o cabeçalho
                        // é zero, pode ser um erro de E/S ou a condição de corrida
                        // descrita acima e no ticket #3883. De qualquer forma,
                        // assuma que o journal é hot. Pode ser um falso positivo. Mas
                        // se for, o mecanismo automático de playback e recuperação
                        // cuidará dele sob um bloqueio EXCLUSIVE, onde não precisamos
                        // nos preocupar tanto com condições de corrida.
                        *p_exists = 1;
                        rc = SQLITE_OK;
                    }
                }
            }
        }
    }

    rc
}

/// Esta função é chamada para obter um bloqueio compartilhado no arquivo do banco.
/// É ilegal chamar sqlite3PagerGet() antes de esta função ter sido chamada com
/// sucesso. Se um bloqueio compartilhado já é mantido quando esta função é chamada,
/// ela não faz nada.
///
/// As seguintes operações também são feitas por esta função.
///
///   1) Se o pager está no estado PAGER_OPEN (nenhum bloqueio mantido no arquivo do
///      banco), tenta-se obter um bloqueio SHARED. Logo depois de obtê-lo, o sistema
///      de arquivos é verificado por um hot journal, que é reproduzido se presente.
///      Depois de qualquer rollback de hot journal, o conteúdo do cache é validado
///      conferindo o campo 'change-counter' do cabeçalho do arquivo e descartado se
///      for inválido.
///
///   2) Se o pager está em modo exclusivo, não há referências pendentes a páginas e
///      ele está no estado de erro, tenta-se limpar o estado de erro descartando o
///      conteúdo do cache de páginas e revertendo qualquer arquivo de journal aberto.
///
/// Se tudo der certo, SQLITE_OK é retornado. Se ocorrer um erro de E/S ao bloquear o
/// banco, verificar um hot journal ou reverter um journal, o código do erro é
/// retornado.
pub fn pager_shared_lock(p_pager: &mut Pager) -> i32 {
    let mut rc = SQLITE_OK; // Código de retorno

    // Esta rotina só é chamada pela b-tree e só quando não há páginas pendentes. Isto
    // implica que o estado do pager deve ser OPEN ou READER. READER só é possível se o
    // pager está ou esteve em modo de acesso exclusivo.
    debug_assert!(
        pcache_ref_count(&p_pager.p_p_cache.as_ref().expect("pPCache").borrow()) == 0
    );
    debug_assert!(
        p_pager.e_state as i32 == PAGER_OPEN || p_pager.e_state as i32 == PAGER_READER
    );
    debug_assert!(p_pager.err_code == SQLITE_OK);

    // O rótulo `failed` do C: os goto viram `break 'failed`.
    'failed: {
        if !pager_use_wal(p_pager) && p_pager.e_state as i32 == PAGER_OPEN {
            let mut b_hot_journal: i32 = 1; // Verdadeiro se existe um arquivo de hot journal

            debug_assert!(!memdb(p_pager));
            debug_assert!(p_pager.temp_file == 0 || p_pager.e_lock as i32 == EXCLUSIVE_LOCK);

            rc = pager_wait_on_lock(p_pager, SHARED_LOCK);
            if rc != SQLITE_OK {
                debug_assert!(
                    p_pager.e_lock as i32 == NO_LOCK || p_pager.e_lock as i32 == UNKNOWN_LOCK
                );
                break 'failed;
            }

            // Se um arquivo de journal existe e não há bloqueio RESERVED no arquivo do
            // banco, ele precisa ser reproduzido ou apagado.
            if p_pager.e_lock as i32 <= SHARED_LOCK {
                rc = has_hot_journal(p_pager, &mut b_hot_journal);
            }
            if rc != SQLITE_OK {
                break 'failed;
            }
            if b_hot_journal != 0 {
                if p_pager.read_only != 0 {
                    rc = SQLITE_READONLY_ROLLBACK;
                    break 'failed;
                }

                // Obtém um bloqueio EXCLUSIVE no arquivo do banco. Neste ponto é
                // importante que um bloqueio RESERVED não seja obtido no caminho para o
                // EXCLUSIVE. Se fosse, outro processo poderia abrir o arquivo do banco,
                // detectar o bloqueio RESERVED e concluir que é seguro ler o banco
                // enquanto este processo ainda está revertendo o hot journal.
                //
                // Como o bloqueio RESERVED intermediário não é pedido, qualquer outro
                // processo que tente acessar o arquivo do banco chegará a este ponto do
                // código e falhará ao obter o próprio bloqueio EXCLUSIVE.
                //
                // A menos que o pager esteja em locking_mode=exclusive, o bloqueio é
                // rebaixado para SHARED_LOCK antes de esta função retornar.
                rc = pager_lock_db(p_pager, EXCLUSIVE_LOCK);
                if rc != SQLITE_OK {
                    break 'failed;
                }

                // Se ainda não está aberto e o arquivo existe em disco, abre o journal
                // para leitura e escrita. O acesso de escrita é necessário porque em
                // modo exclusivo o descritor fica aberto e possivelmente é usado numa
                // transação posterior. Além disso, o acesso de escrita costuma ser
                // necessário para finalizar o journal em journal_mode=persist (e
                // também em journal_mode=truncate em alguns sistemas).
                //
                // Se o journal não existe, em geral significa que outra conexão
                // conseguiu entrar e revertê-lo antes de esta obter o bloqueio
                // exclusivo acima. Ou pode significar que o pager estava no estado de
                // erro quando esta função foi chamada e o arquivo de journal não
                // existe.
                if !is_open(&p_pager.jfd) && p_pager.journal_mode as i32 != PAGER_JOURNALMODE_OFF {
                    let mut b_exists: i32 = 0; // Verdadeiro se o arquivo de journal existe
                    rc = os_access(
                        p_pager.p_vfs.as_deref().expect("pVfs"),
                        &p_pager.z_journal,
                        SQLITE_ACCESS_EXISTS,
                        &mut b_exists,
                    );
                    if rc == SQLITE_OK && b_exists != 0 {
                        let mut fout: i32 = 0;
                        let f = SQLITE_OPEN_READWRITE | SQLITE_OPEN_MAIN_JOURNAL;
                        let mut jfd = Sqlite3File { p_methods: None };
                        debug_assert!(p_pager.temp_file == 0);
                        rc = os_open(
                            p_pager.p_vfs.as_deref().expect("pVfs"),
                            Some(&p_pager.z_journal),
                            &mut jfd,
                            f,
                            &mut fout,
                        );
                        // jfd só fica aberto (is_open) se o xOpen teve sucesso.
                        if rc == SQLITE_OK {
                            p_pager.jfd = Some(Box::new(jfd));
                        }
                        debug_assert!(rc != SQLITE_OK || is_open(&p_pager.jfd));
                        if rc == SQLITE_OK && (fout & SQLITE_OPEN_READONLY) != 0 {
                            rc = sqlite_cantopen_bkpt(62418);
                            if let Some(jfd) = p_pager.jfd.as_deref_mut() {
                                os_close(jfd);
                            }
                            p_pager.jfd = None;
                        }
                    }
                }

                // Reproduz e apaga o journal. Solta o bloqueio de escrita do banco e
                // readquire o de leitura. Purga o cache antes de reproduzir o hot
                // journal para não acabar com um cache inconsistente. Sincroniza o hot
                // journal antes de reproduzi-lo, já que o processo que caiu e deixou o
                // hot journal provavelmente não o sincronizou e somos obrigados a
                // sempre sincronizar o journal antes de reproduzi-lo.
                if is_open(&p_pager.jfd) {
                    debug_assert!(rc == SQLITE_OK);
                    rc = pager_sync_hot_journal(p_pager);
                    if rc == SQLITE_OK {
                        rc = pager_playback(p_pager, (p_pager.temp_file == 0) as i32);
                        p_pager.e_state = PAGER_OPEN as u8;
                    }
                } else if p_pager.exclusive_mode == 0 {
                    pager_unlock_db(p_pager, SHARED_LOCK);
                }

                if rc != SQLITE_OK {
                    // Este ramo é tomado se ocorre um erro ao tentar abrir ou reverter
                    // um hot journal segurando um bloqueio EXCLUSIVE. A rotina
                    // pager_unlock() será chamada antes de retornar para desbloquear o
                    // arquivo. Se a tentativa de desbloqueio falhar, Pager.eLock deve
                    // virar UNKNOWN_LOCK (veja o comentário acima do #define de
                    // UNKNOWN_LOCK para a explicação).
                    //
                    // Para fazer pager_unlock() fazer isso, põe-se Pager.eState em
                    // PAGER_ERROR agora. Isto não conta de fato como uma transição para
                    // o estado ERROR do diagrama de estados no topo do arquivo, já que
                    // sabemos que a mesma chamada a pager_unlock() logo levará o objeto
                    // pager ao estado OPEN. Chamar assert_pager_state() agora falharia,
                    // como deve, pois não deveria ser possível estar no estado ERROR
                    // com zero referências pendentes a páginas.
                    pager_error(p_pager, rc);
                    break 'failed;
                }

                debug_assert!(p_pager.e_state as i32 == PAGER_OPEN);
                debug_assert!(
                    p_pager.e_lock as i32 == SHARED_LOCK
                        || (p_pager.exclusive_mode != 0 && p_pager.e_lock as i32 > SHARED_LOCK)
                );
            }

            if p_pager.temp_file == 0 && p_pager.has_held_shared_lock != 0 {
                // O bloqueio compartilhado acabou de ser adquirido, então confere se o
                // banco foi modificado. Se mudou, descarrega o cache. A flag
                // hasHeldSharedLock impede que isto ocorra no primeiro acesso a um
                // arquivo, para economizar uma chamada os_read() desnecessária na
                // partida.
                //
                // Mudanças no banco são detectadas olhando 16 bytes a partir do
                // deslocamento 24 do arquivo. Os 4 primeiros são um contador de 32 bits
                // incrementado a cada mudança. Os outros bytes mudam aleatoriamente a
                // cada mudança do arquivo quando um codec está em uso.
                //
                // Há uma chance ínfima de uma mudança não ser detectada. Ela é tão
                // pequena que pode ser desprezada.
                let mut db_file_vers = [0u8; 16];

                rc = os_read(p_pager.fd.as_deref_mut().expect("fd"), &mut db_file_vers, 24);
                if rc != SQLITE_OK {
                    if rc != SQLITE_IOERR_SHORT_READ {
                        break 'failed;
                    }
                    db_file_vers.fill(0);
                }

                if p_pager.db_file_vers != db_file_vers {
                    pager_reset(p_pager);

                    // Desmapeia o arquivo do banco. É possível que processos externos
                    // tenham truncado o arquivo e depois o estendido de volta ao
                    // tamanho original enquanto este processo não segurava um
                    // bloqueio. Neste caso pode existir um mapeamento Pager.pMap que
                    // parece ter o tamanho certo mas não é válido. Evita-se essa
                    // possibilidade desmapeando o banco aqui.
                    if usefetch(p_pager) {
                        os_unfetch(p_pager.fd.as_deref_mut().expect("fd"), 0, None);
                    }
                }
            }

            // Se há um arquivo WAL no sistema de arquivos, abre este banco em modo
            // WAL. Caso contrário, a chamada de função a seguir não faz nada.
            rc = pager_open_wal_if_present(p_pager);
            debug_assert!(p_pager.p_wal.is_none() || rc == SQLITE_OK);
        }

        if pager_use_wal(p_pager) {
            debug_assert!(rc == SQLITE_OK);
            rc = pager_begin_read_transaction(p_pager);
        }

        if p_pager.temp_file == 0 && p_pager.e_state as i32 == PAGER_OPEN && rc == SQLITE_OK {
            let mut db_size: Pgno = 0;
            rc = pager_pagecount(p_pager, &mut db_size);
            p_pager.db_size = db_size;
        }
    }

    // failed:
    if rc != SQLITE_OK {
        debug_assert!(!memdb(p_pager));
        pager_unlock(p_pager);
        debug_assert!(p_pager.e_state as i32 == PAGER_OPEN);
    } else {
        p_pager.e_state = PAGER_READER as u8;
        p_pager.has_held_shared_lock = 1;
    }
    rc
}


// ---- part_013.rs ----

// Pontes assumidas com outros módulos (nomes pela convenção do porte):
//   pager_weak(p_pager: &Pager) -> Weak<RefCell<Pager>>   (o `pPager` do C como referência
//                                                          fraca, para gravar em PgHdr.p_pager)
//   os_fetch(fd: &mut Sqlite3File, ofst: i64, amt: i32, pp: &mut usize) -> i32
//   os_unfetch(fd: &mut Sqlite3File, ofst: i64, p_data: usize) -> i32
//       (`void*` da região mapeada vira handle `usize`, 0 é NULL, como o handle do pcache)
//   pager_acquire_map_page(p_pager, pgno, p_data: usize, pp_page: &mut Option<PgHdrRef>) -> i32
//   pager_release_map_page(p_pg: &PgHdrRef)
//   read_db_page(p_pg: &PgHdrRef) -> i32
//   pager_unlock_and_rollback(p_pager: &mut Pager)
//   mem_journal_open(p_jfd: &mut Option<Box<Sqlite3File>>)
//   journal_open(p_vfs: Option<&Sqlite3Vfs>, z_name: &[u8], p_jfd: &mut Option<Box<Sqlite3File>>,
//                flags: i32, n_spill: i32) -> i32
//   database_is_unmoved(p_pager: &Pager) -> i32
//   wal_find_frame(p_wal: &mut Wal, pgno: Pgno, p_i_frame: &mut u32) -> i32
//   bitvec_create(n_bit: u32) -> Option<Box<Bitvec>>, bitvec_destroy(p: Option<Box<Bitvec>>)
// O campo Pager.x_get precisa ter o tipo
//   Option<fn(&mut Pager, Pgno, &mut Option<PgHdrRef>, i32) -> i32>
// porque o `DbPage **ppPage` do C é a saída `&mut Option<PgHdrRef>`.
// `pager_set_pagehash` (SQLITE_CHECK_PAGES), `assert_pager_state` (SQLITE_DEBUG) e IOTRACE
// não existem na compilação do Debian e somem.

/// Se a contagem de referências chegou a zero, desfaz qualquer transação ativa e destrava
/// o pager.
///
/// Exceto em locking_mode=EXCLUSIVE quando não há nada no journal de rollback: aí o
/// destravamento não é feito e não há nada a desfazer, e a rotina não faz nada.
pub fn pager_unlock_if_unused(p_pager: &mut Pager) {
    let n_ref = pcache_ref_count(&p_pager.p_p_cache.as_ref().expect("Pager sem PCache").borrow());
    if n_ref == 0 {
        // porque a página 1 nunca é mapeada em memória
        debug_assert!(p_pager.n_mmap_out == 0);
        pager_unlock_and_rollback(p_pager);
    }
}

/// Os métodos de busca de página tentam obter uma referência para a página `pgno`. Se a
/// referência é obtida, ela é copiada para `*pp_page` e retorna SQLITE_OK.
///
/// Há implementações diferentes conforme o estado atual do pager:
///
///     get_page_normal()  o getter normal
///     get_page_error()   usado se o pager está em estado de erro
///     get_page_mmap()    usado se o I/O mapeado em memória está habilitado
///
/// Se a página pedida já está no cache, ela é devolvida. Caso contrário, um novo objeto de
/// página é alocado e preenchido com dados lidos do arquivo de banco. Em alguns casos o
/// pcache pode preferir não alocar e reaproveitar um objeto sem referências.
///
/// Os dados extras anexados à página são zerados na primeira vez que ela entra em memória.
/// Se a página já estava no cache, os dados extras ficam como estavam na última vez.
///
/// Se a imagem do banco é menor que a página pedida, ou se `flags` tem o bit
/// PAGER_GET_NOCONTENT e a página não está no cache, nenhuma leitura em disco ocorre e a
/// imagem em memória da página é inicializada com zeros.
///
/// PAGER_GET_NOCONTENT significa que o conteúdo não importa, o que ocorre em dois cenários:
///
///   a) ao ler do banco uma página folha da lista livre, e
///   b) quando um savepoint está sendo desfeito e uma página nova precisa ser carregada no
///      cache para receber os dados lidos do journal do savepoint.
///
/// Com PAGER_GET_NOCONTENT os dados devolvidos são zerados em vez de lidos do banco, e os
/// bits de `pgno` em Pager.p_in_journal e nos PagerSavepoint.p_in_savepoint dos savepoints
/// abertos são ligados. Assim, se a página for tornada gravável depois (pager_write), o
/// conteúdo dela não será jornalizado, o que economiza E/S.
///
/// A obtenção pode falhar por vários motivos. Em todos os casos um código de erro
/// apropriado é devolvido e `*pp_page` vira None.
///
/// Veja também pager_lookup(): as duas procuram primeiro no cache em memória, mas esta vai
/// ao disco se a página não está lá (adquirindo o lock de leitura na primeira vez e
/// podendo reproduzir um journal antigo), enquanto a outra só devolve None.
pub fn get_page_normal(
    p_pager: &mut Pager,
    pgno: Pgno,
    pp_page: &mut Option<PgHdrRef>,
    flags: i32,
) -> i32 {
    let mut rc = SQLITE_OK;
    let mut p_pg: Option<PgHdrRef> = None;
    let p_cache: PCacheRef = p_pager.p_p_cache.clone().expect("Pager sem PCache");
    let mut p_base: usize;

    debug_assert!(p_pager.err_code == SQLITE_OK);
    debug_assert!(p_pager.e_state as i32 >= PAGER_READER);
    debug_assert!(p_pager.has_held_shared_lock == 1);

    if pgno == 0 {
        return SQLITE_CORRUPT_BKPT;
    }

    'pager_acquire_err: {
        p_base = pcache_fetch(&p_cache, pgno, 3);
        if p_base == 0 {
            // pPg = 0: p_pg já é None neste ponto
            rc = pcache_fetch_stress(&p_cache, pgno, &mut p_base);
            if rc != SQLITE_OK {
                break 'pager_acquire_err;
            }
            if p_base == 0 {
                rc = SQLITE_NOMEM_BKPT;
                break 'pager_acquire_err;
            }
        }
        let pg: PgHdrRef = pcache_fetch_finish(&p_cache, pgno, p_base);
        *pp_page = Some(pg.clone());
        p_pg = Some(pg.clone());
        debug_assert!(pg.borrow().pgno == pgno);

        // Verdadeiro se PAGER_GET_NOCONTENT está ligado.
        let no_content: bool = (flags & PAGER_GET_NOCONTENT) != 0;
        let has_pager: bool = pg.borrow().p_pager.is_some();
        if has_pager && !no_content {
            // O pcache já contém uma cópia inicializada da página. Devolve sem mais.
            debug_assert!(pgno != pager_sj_pgno(p_pager));
            p_pager.a_stat[PAGER_STAT_HIT] = p_pager.a_stat[PAGER_STAT_HIT].wrapping_add(1);
            return SQLITE_OK;
        } else {
            // O cache de páginas criou uma página nova. O conteúdo precisa ser
            // inicializado, mas antes algumas verificações de erro:
            //
            // (*) obsoleto. Era: o número máximo de página é 2^31
            // (2) nunca tenta buscar a página de locking
            if pgno == pager_sj_pgno(p_pager) {
                rc = SQLITE_CORRUPT_BKPT;
                break 'pager_acquire_err;
            }

            pg.borrow_mut().p_pager = Some(pager_weak(p_pager));

            debug_assert!(!is_open(&p_pager.fd) || !memdb(p_pager));
            if !is_open(&p_pager.fd) || p_pager.db_size < pgno || no_content {
                if pgno > p_pager.mx_pgno {
                    rc = SQLITE_FULL;
                    if pgno <= p_pager.db_size {
                        pcache_release(&pg);
                        p_pg = None;
                    }
                    break 'pager_acquire_err;
                }
                if no_content {
                    // Falhar ao ligar os bits dos bitvecs InJournal é benigno: só significa
                    // que talvez se faça trabalho extra para jornalizar uma página que não
                    // precisava. Mesmo assim, o caso de falha de malloc ao ligar um bit é
                    // testado.
                    begin_benign_malloc();
                    if pgno <= p_pager.db_orig_size {
                        let _ = bitvec_set(p_pager.p_in_journal.as_deref_mut(), pgno);
                    }
                    let _ = add_to_savepoint_bitvecs(p_pager, pgno);
                    end_benign_malloc();
                }
                let sz_page = p_pager.page_size as usize;
                pg.borrow_mut().p_data[..sz_page].fill(0);
            } else {
                p_pager.a_stat[PAGER_STAT_MISS] = p_pager.a_stat[PAGER_STAT_MISS].wrapping_add(1);
                rc = read_db_page(&pg);
                if rc != SQLITE_OK {
                    break 'pager_acquire_err;
                }
            }
        }
        return SQLITE_OK;
    }

    // pager_acquire_err:
    debug_assert!(rc != SQLITE_OK);
    if let Some(pg) = p_pg {
        pcache_drop(&pg);
    }
    pager_unlock_if_unused(p_pager);
    *pp_page = None;
    rc
}

/// O getter de página para quando o I/O mapeado em memória está habilitado
/// (SQLITE_MAX_MMAP_SIZE>0, o caso do Debian).
pub fn get_page_mmap(
    p_pager: &mut Pager,
    pgno: Pgno,
    pp_page: &mut Option<PgHdrRef>,
    flags: i32,
) -> i32 {
    let mut rc = SQLITE_OK;
    let mut p_pg: Option<PgHdrRef> = None;
    // Frame a ler do arquivo WAL
    let mut i_frame: u32 = 0;

    // É aceitável usar uma página somente leitura (mmap) para qualquer página exceto a 1 se
    // não há transação de escrita aberta ou se o chamador passou ACQUIRE_READONLY. E desde
    // que o banco não seja temporário nem em memória.
    let mmap_ok: bool =
        pgno > 1 && (p_pager.e_state as i32 == PAGER_READER || (flags & PAGER_GET_READONLY) != 0);

    debug_assert!(usefetch(p_pager));

    // Nota de otimização: o termo "pgno<=1" antes de "pgno==0" deixa o otimizador do
    // compilador reaproveitar o resultado do teste "pgno>1" da instrução anterior e evitar
    // testar pgno==0 no caso comum em que pgno é grande.
    if pgno <= 1 && pgno == 0 {
        return SQLITE_CORRUPT_BKPT;
    }
    debug_assert!(p_pager.e_state as i32 >= PAGER_READER);
    debug_assert!(p_pager.has_held_shared_lock == 1);
    debug_assert!(p_pager.err_code == SQLITE_OK);

    if mmap_ok && pager_use_wal(p_pager) {
        rc = wal_find_frame(p_pager.p_wal.as_deref_mut().expect("Pager sem Wal"), pgno, &mut i_frame);
        if rc != SQLITE_OK {
            *pp_page = None;
            return rc;
        }
    }
    if mmap_ok && i_frame == 0 {
        let mut p_data: usize = 0;
        let page_size: i64 = p_pager.page_size;
        let i_ofst: i64 = ((pgno - 1) as i64) * page_size;
        rc = os_fetch(
            p_pager.fd.as_deref_mut().expect("Pager sem arquivo"),
            i_ofst,
            page_size as i32,
            &mut p_data,
        );
        if rc == SQLITE_OK && p_data != 0 {
            if p_pager.e_state as i32 > PAGER_READER || p_pager.temp_file != 0 {
                p_pg = pager_lookup(p_pager, pgno);
            }
            if p_pg.is_none() {
                rc = pager_acquire_map_page(p_pager, pgno, p_data, &mut p_pg);
            } else {
                let _ = os_unfetch(p_pager.fd.as_deref_mut().expect("Pager sem arquivo"), i_ofst, p_data);
            }
            if p_pg.is_some() {
                debug_assert!(rc == SQLITE_OK);
                *pp_page = p_pg;
                return SQLITE_OK;
            }
        }
        if rc != SQLITE_OK {
            *pp_page = None;
            return rc;
        }
    }
    get_page_normal(p_pager, pgno, pp_page, flags)
}

/// O método de busca de página para quando o pager está em estado de erro.
pub fn get_page_error(
    p_pager: &mut Pager,
    _pgno: Pgno,
    pp_page: &mut Option<PgHdrRef>,
    _flags: i32,
) -> i32 {
    debug_assert!(p_pager.err_code != SQLITE_OK);
    *pp_page = None;
    p_pager.err_code
}

/// Despacha todo pedido de busca de página para o getter apropriado (a versão normal e
/// rápida de sqlite3PagerGet(); o trecho de rastreamento do C está sob `#if 0`).
pub fn pager_get(
    p_pager: &mut Pager,
    pgno: Pgno,
    pp_page: &mut Option<PgHdrRef>,
    flags: i32,
) -> i32 {
    let x_get = p_pager.x_get.expect("Pager.x_get não definido");
    x_get(p_pager, pgno, pp_page, flags)
}

/// Adquire uma página se ela já está no cache em memória. Não lê a página do disco.
/// Devolve a página, ou None se ela não está no cache.
///
/// Veja também pager_get(): a diferença é que aquela vai ao disco ler a página se ela não
/// está no cache. Esta devolve None se a página não está no cache ou se já ocorreu algum
/// erro de E/S.
pub fn pager_lookup(p_pager: &Pager, pgno: Pgno) -> Option<PgHdrRef> {
    let p_cache = p_pager.p_p_cache.as_ref().expect("Pager sem PCache");
    debug_assert!(pgno != 0);
    let p_page: usize = pcache_fetch(p_cache, pgno, 0);
    debug_assert!(p_page == 0 || p_pager.has_held_shared_lock != 0);
    if p_page == 0 {
        return None;
    }
    Some(pcache_fetch_finish(p_cache, pgno, p_page))
}

/// Libera uma referência de página.
///
/// pager_unref() e pager_unref_not_null() só podem ser usadas se a página liberada não é a
/// última referência à página 1. A camada b-tree mantém a página 1 aberta até o fim, então
/// estas duas rotinas servem para liberar qualquer página que não seja BtShared.p_page1. O
/// assert da marca 20230419-2 prova que essa restrição é sempre respeitada.
///
/// Use pager_unref_page_one() para liberar a página 1: ela confere o total de páginas
/// pendentes e, se chega a zero, solta o lock do banco.
pub fn pager_unref_not_null(p_pg: &PgHdrRef) {
    // TESTONLY: o pager da página, lido antes da liberação, só para o assert final.
    let p_pager_dbg: Option<PagerRef> = if cfg!(debug_assertions) {
        p_pg.borrow().p_pager.as_ref().and_then(|w| w.upgrade())
    } else {
        None
    };
    let flags: u16 = p_pg.borrow().flags;
    if flags & PGHDR_MMAP != 0 {
        // A página 1 nunca é mapeada em memória
        debug_assert!(p_pg.borrow().pgno != 1);
        pager_release_map_page(p_pg);
    } else {
        pcache_release(p_pg);
    }
    // Não use esta rotina para liberar a última referência à página 1 (tag-20230419-2)
    if let Some(p_pager_rc) = p_pager_dbg {
        if let Ok(p_pager) = p_pager_rc.try_borrow() {
            debug_assert!(
                pcache_ref_count(&p_pager.p_p_cache.as_ref().expect("Pager sem PCache").borrow()) > 0
            );
        }
    }
}

/// Como pager_unref_not_null(), mas aceita página ausente.
pub fn pager_unref(p_pg: Option<&PgHdrRef>) {
    if let Some(p_pg) = p_pg {
        pager_unref_not_null(p_pg);
    }
}

/// Libera a página 1, a única que nunca é mapeada em memória. Se o total de páginas
/// pendentes chega a zero, o lock do banco é solto.
pub fn pager_unref_page_one(p_pg: &PgHdrRef) {
    debug_assert!(p_pg.borrow().pgno == 1);
    // A página 1 nunca é mapeada em memória
    debug_assert!((p_pg.borrow().flags & PGHDR_MMAP) == 0);
    let p_pager_rc: PagerRef = p_pg
        .borrow()
        .p_pager
        .as_ref()
        .and_then(|w| w.upgrade())
        .expect("PgHdr sem Pager");
    pcache_release(p_pg);
    pager_unlock_if_unused(&mut p_pager_rc.borrow_mut());
}

/// Chamada no início de toda transação de escrita. Já deve haver um lock RESERVED ou
/// EXCLUSIVE no arquivo de banco.
///
/// Abre o arquivo de journal do pager e escreve o cabeçalho de journal no começo dele. Se há
/// savepoints ativos, abre também o sub-journal. Só é usada quando o journal é aberto para
/// escrever um log de rollback de uma transação, não ao abrir um journal quente para
/// desfazê-lo.
///
/// Se o arquivo de journal já está aberto (como pode estar em modo exclusivo), apenas
/// escreve o cabeçalho de journal no começo do arquivo já aberto.
///
/// Abrindo o journal ou não, o bitvec Pager.p_in_journal é alocado.
///
/// Devolve SQLITE_OK se tudo deu certo. Senão, SQLITE_NOMEM se a alocação de
/// Pager.p_in_journal falhou, ou um código de erro de E/S se abrir ou escrever o journal
/// falhou.
pub fn pager_open_journal(p_pager: &mut Pager) -> i32 {
    // Código de retorno
    let mut rc = SQLITE_OK;
    // O `pVfs` local do C era só um cache do ponteiro: aqui se usa p_pager.p_vfs direto,
    // para não segurar um empréstimo do pager inteiro durante as chamadas.

    debug_assert!(p_pager.e_state as i32 == PAGER_WRITER_LOCKED);
    debug_assert!(p_pager.p_in_journal.is_none());

    // Se já está em estado de erro, esta função não faz nada. Mas, por outro lado, esta
    // rotina nunca é chamada se já estamos em estado de erro.
    if p_pager.err_code != 0 {
        return p_pager.err_code;
    }

    if !pager_use_wal(p_pager) && p_pager.journal_mode as i32 != PAGER_JOURNALMODE_OFF {
        p_pager.p_in_journal = bitvec_create(p_pager.db_size);
        if p_pager.p_in_journal.is_none() {
            return SQLITE_NOMEM_BKPT;
        }

        // Abre o arquivo de journal se ele ainda não está aberto.
        if !is_open(&p_pager.jfd) {
            if p_pager.journal_mode as i32 == PAGER_JOURNALMODE_MEMORY {
                mem_journal_open(&mut p_pager.jfd);
            } else {
                let mut flags: i32 = SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE;
                let n_spill: i32;

                if p_pager.temp_file != 0 {
                    flags |= SQLITE_OPEN_DELETEONCLOSE | SQLITE_OPEN_TEMP_JOURNAL;
                    flags |= SQLITE_OPEN_EXCLUSIVE;
                    n_spill = SQLITE_CONFIG.n_stmt_spill;
                } else {
                    flags |= SQLITE_OPEN_MAIN_JOURNAL;
                    n_spill = jrnl_buffer_size(p_pager);
                }

                // Verifica se o banco ainda tem o mesmo nome de quando foi aberto.
                rc = database_is_unmoved(p_pager);
                if rc == SQLITE_OK {
                    rc = journal_open(
                        p_pager.p_vfs.as_deref(),
                        &p_pager.z_journal,
                        &mut p_pager.jfd,
                        flags,
                        n_spill,
                    );
                }
            }
            debug_assert!(rc != SQLITE_OK || is_open(&p_pager.jfd));
        }

        // Escreve o primeiro cabeçalho de journal no arquivo de journal e abre o
        // sub-journal se necessário.
        if rc == SQLITE_OK {
            // TODO do C: conferir se todas estas atribuições são mesmo necessárias.
            p_pager.n_rec = 0;
            p_pager.journal_off = 0;
            p_pager.set_super = 0;
            p_pager.journal_hdr = 0;
            rc = write_journal_hdr(p_pager);
        }
    }

    if rc != SQLITE_OK {
        bitvec_destroy(p_pager.p_in_journal.take());
        p_pager.journal_off = 0;
    } else {
        debug_assert!(p_pager.e_state as i32 == PAGER_WRITER_LOCKED);
        p_pager.e_state = PAGER_WRITER_CACHEMOD as u8;
    }

    rc
}


// ---- part_014.rs ----

/// Obtém o pager dono da página (o `pPg->pPager` do C, sempre válido enquanto
/// a página existe).
fn pg_pager(p_pg: &PgHdrRef) -> PagerRef {
    p_pg.borrow()
        .p_pager
        .as_ref()
        .and_then(|w| w.upgrade())
        .expect("PgHdr sem Pager")
}

/// Inicia uma transação de escrita no pager. Se uma transação de escrita já
/// tiver sido aberta, esta função não faz nada.
///
/// Se `ex_flag` for falso, adquire pelo menos um lock RESERVED no arquivo de
/// banco de dados. Se for verdadeiro, adquire pelo menos um lock EXCLUSIVE. Se
/// um lock assim já for mantido, nenhuma função de lock é chamada.
///
/// Se `subj_in_memory` for não zero, qualquer sub-journal aberto dentro desta
/// transação será um arquivo em memória. Isto não tem efeito se o sub-journal
/// já estiver aberto (como pode estar em modo exclusivo) ou se a transação não
/// precisar de sub-journal. Se for zero, o sub-journal necessário é em memória
/// quando o pager é de banco em memória, e arquivo temporário caso contrário.
pub fn pager_begin(p_pager: &mut Pager, ex_flag: i32, subj_in_memory: i32) -> i32 {
    let mut rc = SQLITE_OK;

    if p_pager.err_code != 0 {
        return p_pager.err_code;
    }
    debug_assert!(p_pager.e_state as i32 >= PAGER_READER && (p_pager.e_state as i32) < PAGER_ERROR);
    p_pager.subj_in_memory = subj_in_memory as u8;

    if p_pager.e_state as i32 == PAGER_READER {
        debug_assert!(p_pager.p_in_journal.is_none());

        if pager_use_wal(p_pager) {
            // Se o pager usa locking_mode=exclusive e o lock exclusivo no banco
            // ainda não é mantido, obtém agora.
            if p_pager.exclusive_mode != 0
                && p_pager.p_wal.as_deref_mut().map_or(0, |w| wal_exclusive_mode(w, -1)) != 0
            {
                rc = pager_lock_db(p_pager, EXCLUSIVE_LOCK);
                if rc != SQLITE_OK {
                    return rc;
                }
                if let Some(w) = p_pager.p_wal.as_deref_mut() {
                    let _ = wal_exclusive_mode(w, 1);
                }
            }

            // Pega o lock de escrita no arquivo de log. Se der certo, sobe para
            // o estado PAGER_RESERVED. Caso contrário, devolve o erro ao chamador.
            // O busy-handler não é invocado se outra conexão já mantém o lock de
            // escrita. Se possível, a camada superior o chamará.
            rc = match p_pager.p_wal.as_deref_mut() {
                Some(wal) => wal_begin_write_transaction(wal),
                None => SQLITE_OK,
            };
        } else {
            // Obtém um lock RESERVED no arquivo de banco. Se `ex_flag` for
            // verdadeiro, sobe imediatamente para EXCLUSIVE. O busy-handler pode
            // ser usado ao subir para EXCLUSIVE, mas não ao obter o RESERVED.
            rc = pager_lock_db(p_pager, RESERVED_LOCK);
            if rc == SQLITE_OK && ex_flag != 0 {
                rc = pager_wait_on_lock(p_pager, EXCLUSIVE_LOCK);
            }
        }

        if rc == SQLITE_OK {
            // Muda para o estado WRITER_LOCKED.
            //
            // O modo WAL põe Pager.eState em PAGER_WRITER_LOCKED ou CACHEMOD
            // quando tem uma transação aberta, mas nunca em DBMOD ou FINISHED.
            // Nesses estados o código que desfaz transações de savepoint pode
            // copiar dados do sub-journal para o arquivo de banco e também para
            // o cache de páginas, o que seria incorreto em modo WAL.
            p_pager.e_state = PAGER_WRITER_LOCKED as u8;
            p_pager.db_hint_size = p_pager.db_size;
            p_pager.db_file_size = p_pager.db_size;
            p_pager.db_orig_size = p_pager.db_size;
            p_pager.journal_off = 0;
        }

        debug_assert!(rc == SQLITE_OK || p_pager.e_state as i32 == PAGER_READER);
        debug_assert!(rc != SQLITE_OK || p_pager.e_state as i32 == PAGER_WRITER_LOCKED);
    }

    rc
}

/// Escreve a página `p_pg` no final do journal de rollback.
fn pager_add_page_to_rollback_journal(p_pg: &PgHdrRef) -> i32 {
    let pager_rc = pg_pager(p_pg);
    let mut pg = p_pg.borrow_mut();
    let mut p_pager = pager_rc.borrow_mut();
    let mut rc: i32;
    let i_off: i64 = p_pager.journal_off;

    // Nunca se escreve no journal a página que contém os locks do banco.
    debug_assert!(pg.pgno != pager_sj_pgno(&p_pager));

    debug_assert!(p_pager.journal_hdr <= p_pager.journal_off);
    let cksum: u32 = pager_cksum(&p_pager, &pg.p_data);

    // Mesmo que ocorra erro de E/S ou disco cheio ao jornalizar a página, a
    // flag need-sync fica ligada. Senão, ao desfazer a transação, a lógica de
    // playback_one_page() acharia que a página precisa ser restaurada no
    // arquivo de banco, e um erro de E/S nessa restauração causaria corrupção.
    pg.flags |= PGHDR_NEED_SYNC;

    rc = write32bits(&mut p_pager.jfd, i_off, pg.pgno);
    if rc != SQLITE_OK {
        return rc;
    }
    let page_size = p_pager.page_size;
    let sz_page = page_size as usize;
    rc = os_write(
        p_pager.jfd.as_deref_mut().expect("jfd"),
        &pg.p_data[..sz_page],
        i_off + 4,
    );
    if rc != SQLITE_OK {
        return rc;
    }
    rc = write32bits(&mut p_pager.jfd, i_off + page_size + 4, cksum);
    if rc != SQLITE_OK {
        return rc;
    }

    p_pager.journal_off += 8 + page_size;
    p_pager.n_rec += 1;
    debug_assert!(p_pager.p_in_journal.is_some());
    rc = bitvec_set(p_pager.p_in_journal.as_deref_mut(), pg.pgno);
    debug_assert!(rc == SQLITE_OK || rc == SQLITE_NOMEM);
    rc |= add_to_savepoint_bitvecs(&mut p_pager, pg.pgno);
    debug_assert!(rc == SQLITE_OK || rc == SQLITE_NOMEM);
    rc
}

/// Marca uma única página de dados como gravável. A página é escrita no
/// journal principal ou no sub-journal conforme necessário. Se for escrita em
/// um dos journals, o bit correspondente é ligado no bitvec Pager.pInJournal e
/// nos bitvecs PagerSavepoint.pInSavepoint dos savepoints abertos, conforme
/// apropriado.
///
/// Corresponde ao `pager_write` estático do C; o nome público
/// `sqlite3PagerWrite` ocupa `pager_write`, então este leva o sufixo `_inner`.
fn pager_write_inner(p_pg: &PgHdrRef) -> i32 {
    let pager_rc = pg_pager(p_pg);
    let mut rc = SQLITE_OK;

    // Esta rotina só é chamada com uma transação de escrita já iniciada. O
    // arquivo de journal pode ou não estar aberto. Nunca é chamada no estado
    // ERROR.
    let e_state = {
        let p = pager_rc.borrow();
        debug_assert!(
            p.e_state as i32 == PAGER_WRITER_LOCKED
                || p.e_state as i32 == PAGER_WRITER_CACHEMOD
                || p.e_state as i32 == PAGER_WRITER_DBMOD
        );
        debug_assert!(p.err_code == 0);
        debug_assert!(p.read_only == 0);
        p.e_state as i32
    };

    // O arquivo de journal precisa estar aberto. As rotinas de nível mais alto
    // já obtiveram os locks para começar a transação, mas o journal de rollback
    // pode ainda não estar aberto. Abre agora se for o caso.
    //
    // Isto é feito antes de pcache_make_dirty() na página. Se fosse depois, um
    // erro deixaria o pager em WRITER_LOCKED com páginas sujas no cache.
    if e_state == PAGER_WRITER_LOCKED {
        rc = pager_open_journal(&mut pager_rc.borrow_mut());
        if rc != SQLITE_OK {
            return rc;
        }
    }
    debug_assert!(pager_rc.borrow().e_state as i32 >= PAGER_WRITER_CACHEMOD);

    // Marca como suja a página que vai ser modificada.
    pcache_make_dirty(p_pg);

    // Se há journal de rollback em uso, garante que a página a mudar esteja
    // nele, ou, se for uma página nova além do fim do arquivo, que esteja
    // marcada com PGHDR_NEED_SYNC.
    let pgno = p_pg.borrow().pgno;
    let (has_journal, in_journal, db_orig_size, e_state_now) = {
        let p = pager_rc.borrow();
        debug_assert!(p.p_in_journal.is_some() == is_open(&p.jfd));
        let in_journal = match p.p_in_journal.as_deref() {
            Some(bv) => bitvec_test_not_null(bv, pgno) != 0,
            None => false,
        };
        (p.p_in_journal.is_some(), in_journal, p.db_orig_size, p.e_state as i32)
    };
    if has_journal && !in_journal {
        debug_assert!(!pager_use_wal(&pager_rc.borrow()));
        if pgno <= db_orig_size {
            rc = pager_add_page_to_rollback_journal(p_pg);
            if rc != SQLITE_OK {
                return rc;
            }
        } else if e_state_now != PAGER_WRITER_DBMOD {
            p_pg.borrow_mut().flags |= PGHDR_NEED_SYNC;
        }
    }

    // O bit PGHDR_DIRTY foi ligado acima, quando a página entrou na lista de
    // sujas e antes de ser escrita no journal de rollback. Só agora, depois de
    // jornalizada com sucesso, liga-se PGHDR_WRITEABLE, que indica que a página
    // pode ser modificada com segurança.
    p_pg.borrow_mut().flags |= PGHDR_WRITEABLE;

    // Se o journal de instrução está aberto e a página não está nele, escreve a
    // página no journal de instrução.
    if pager_rc.borrow().n_savepoint > 0 {
        rc = subjournal_page_if_required(p_pg);
    }

    // Atualiza o tamanho do banco e retorna.
    {
        let mut p = pager_rc.borrow_mut();
        if p.db_size < pgno {
            p.db_size = pgno;
        }
    }
    rc
}

/// Variante de `pager_write` que roda quando o tamanho do setor é maior que o
/// tamanho da página. O SQLite supõe (razoavelmente) que todos os bytes de um
/// setor são escritos juntos pelo hardware. Logo, todos os bytes de um setor
/// precisam ser jornalizados caso falte energia no meio de uma escrita.
///
/// Em geral o setor é menor ou igual à página, e as páginas podem ser escritas
/// individualmente. Esta rotina só roda no caso excepcional em que a página é
/// menor que o setor.
fn pager_write_large_sector(p_pg: &PgHdrRef) -> i32 {
    let mut rc = SQLITE_OK;
    let mut need_sync = false;
    let pager_rc = pg_pager(p_pg);
    let pg_pgno = p_pg.borrow().pgno;
    let n_page_per_sector: Pgno = {
        let p = pager_rc.borrow();
        p.sector_size / (p.page_size as u32)
    };

    // Liga o bit NOSYNC de doNotSpill. Não se pode permitir que um cabeçalho de
    // journal seja escrito entre as páginas jornalizadas por esta função.
    {
        let mut p = pager_rc.borrow_mut();
        debug_assert!(!memdb(&p));
        debug_assert!((p.do_not_spill & SPILLFLAG_NOSYNC) == 0);
        p.do_not_spill |= SPILLFLAG_NOSYNC;
    }

    // Este truque supõe que tamanho de página e de setor são potências de 2.
    // Põe em pg1 o identificador da primeira página do setor onde pPg está.
    let pg1: Pgno = ((pg_pgno - 1) & !(n_page_per_sector - 1)) + 1;

    let n_page_count: Pgno = pager_rc.borrow().db_size;
    let n_page: i32 = if pg_pgno > n_page_count {
        ((pg_pgno - pg1) + 1) as i32
    } else if (pg1 + n_page_per_sector - 1) > n_page_count {
        (n_page_count + 1 - pg1) as i32
    } else {
        n_page_per_sector as i32
    };
    debug_assert!(n_page > 0);
    debug_assert!(pg1 <= pg_pgno);
    debug_assert!((pg1 + n_page as u32) > pg_pgno);

    let mut ii: i32 = 0;
    while ii < n_page && rc == SQLITE_OK {
        let pg: Pgno = pg1.wrapping_add(ii as u32);
        let (not_in_journal, sj_pgno) = {
            let p = pager_rc.borrow();
            (bitvec_test(p.p_in_journal.as_deref(), pg) == 0, pager_sj_pgno(&p))
        };
        if pg == pg_pgno || not_in_journal {
            if pg != sj_pgno {
                let mut p_page: Option<PgHdrRef> = None;
                rc = pager_get(&mut pager_rc.borrow_mut(), pg, &mut p_page, 0);
                if rc == SQLITE_OK {
                    if let Some(page) = p_page.as_ref() {
                        rc = pager_write_inner(page);
                        if (page.borrow().flags & PGHDR_NEED_SYNC) != 0 {
                            need_sync = true;
                        }
                        pager_unref_not_null(page);
                    }
                }
            }
        } else {
            let p_page = pager_lookup(&mut pager_rc.borrow_mut(), pg);
            if let Some(page) = p_page.as_ref() {
                if (page.borrow().flags & PGHDR_NEED_SYNC) != 0 {
                    need_sync = true;
                }
                pager_unref_not_null(page);
            }
        }
        ii += 1;
    }

    // Se PGHDR_NEED_SYNC está ligada em qualquer das n_page páginas a partir de
    // pg1, ela precisa estar ligada em todas. Escrever em qualquer uma delas
    // pode danificar as outras, então o journal deve conter cópias sincronizadas
    // de todas antes de qualquer uma ir para o arquivo de banco.
    if rc == SQLITE_OK && need_sync {
        debug_assert!(!memdb(&pager_rc.borrow()));
        ii = 0;
        while ii < n_page {
            let p_page = pager_lookup(&mut pager_rc.borrow_mut(), pg1.wrapping_add(ii as u32));
            if let Some(page) = p_page.as_ref() {
                page.borrow_mut().flags |= PGHDR_NEED_SYNC;
                pager_unref_not_null(page);
            }
            ii += 1;
        }
    }

    {
        let mut p = pager_rc.borrow_mut();
        debug_assert!((p.do_not_spill & SPILLFLAG_NOSYNC) != 0);
        p.do_not_spill &= !SPILLFLAG_NOSYNC;
    }
    rc
}

/// Marca uma página de dados como gravável. Esta rotina deve ser chamada antes
/// de mudar uma página. O chamador deve conferir o valor de retorno e não mudar
/// nenhum dado da página a menos que ela retorne SQLITE_OK.
///
/// A diferença para `pager_write_inner` é que esta também trata o caso especial
/// em que 2 ou mais páginas cabem em um único setor de disco: todas as páginas
/// co-residentes devem ter sido escritas no journal antes do retorno.
///
/// Se ocorrer erro, retorna SQLITE_NOMEM ou um código de erro de E/S; senão,
/// SQLITE_OK.
pub fn pager_write(p_pg: &PgHdrRef) -> i32 {
    let pager_rc = pg_pager(p_pg);
    let (flags, pgno) = {
        let g = p_pg.borrow();
        (g.flags, g.pgno)
    };
    let (db_size, n_savepoint, err_code, sector_size, page_size) = {
        let p = pager_rc.borrow();
        debug_assert!(p.e_state as i32 >= PAGER_WRITER_LOCKED);
        (p.db_size, p.n_savepoint, p.err_code, p.sector_size, p.page_size)
    };
    debug_assert!((flags & PGHDR_MMAP) == 0);
    if (flags & PGHDR_WRITEABLE) != 0 && db_size >= pgno {
        if n_savepoint != 0 {
            return subjournal_page_if_required(p_pg);
        }
        SQLITE_OK
    } else if err_code != 0 {
        err_code
    } else if sector_size > (page_size as u32) {
        debug_assert!(pager_rc.borrow().temp_file == 0);
        pager_write_large_sector(p_pg)
    } else {
        pager_write_inner(p_pg)
    }
}

/// Retorna VERDADEIRO se a página dada foi passada antes a `pager_write`. Em
/// outras palavras, se é permitido mudar o conteúdo da página.
pub fn pager_iswriteable(p_pg: &PgHdrRef) -> i32 {
    (p_pg.borrow().flags & PGHDR_WRITEABLE) as i32
}


// ---- part_015.rs ----

// Opções de compilação do Debian: SQLITE_ENABLE_ATOMIC_WRITE e
// SQLITE_ENABLE_BATCH_ATOMIC_WRITE NÃO estão definidas. Portanto `DIRECT_MODE` é 0, `bBatch`
// é 0 e os blocos dessas duas opções somem. `pager_set_pagehash` (SQLITE_CHECK_PAGES),
// `assert_pager_state` (SQLITE_DEBUG), PAGERTRACE e IOTRACE também somem.
//
// Pontes assumidas com outros módulos (nomes pela convenção do porte):
//   pcache_dirty_list(&mut PCache) -> Option<PgHdrRef>
//   pcache_clean_all(&PCacheRef)
//   sync_journal(p_pager: &mut Pager, new_hdr: i32) -> i32
//   pager_write_pagelist(p_pager: &mut Pager, p_list: Option<&PgHdrRef>) -> i32
//   pager_truncate(p_pager: &mut Pager, n_page: Pgno) -> i32
//   fault_sim(i_test: i32) -> i32

/// Informa ao pager que não é necessário escrever as informações da página `p_pg` de volta
/// ao disco, mesmo que a página esteja marcada como suja. Isso acontece, por exemplo,
/// quando a página foi adicionada como folha da lista livre e portanto o conteúdo dela não
/// importa mais.
///
/// A camada superior chama esta rotina quando todos os dados da página não são usados. O
/// pager marca a página como limpa para que ela não seja escrita no disco.
///
/// Testes mostram que esta otimização pode quadruplicar a velocidade de operações DELETE
/// grandes.
///
/// Esta otimização não pode ser usada com arquivo temporário, pois a página pode ter estado
/// suja no início da transação. Nesse caso, se a pressão de memória força a página para fora
/// do cache, os dados precisam ser escritos no disco para poderem ser lidos de volta se a
/// transação atual for desfeita.
pub fn pager_dont_write(p_pg: &PgHdrRef) {
    let p_pager_rc: PagerRef = p_pg
        .borrow()
        .p_pager
        .as_ref()
        .and_then(|w| w.upgrade())
        .expect("PgHdr sem Pager");
    let p_pager = p_pager_rc.borrow();
    let mut pg = p_pg.borrow_mut();
    if p_pager.temp_file == 0 && (pg.flags & PGHDR_DIRTY) != 0 && p_pager.n_savepoint == 0 {
        pg.flags |= PGHDR_DONT_WRITE;
        pg.flags &= !PGHDR_WRITEABLE;
    }
}

/// Esta rotina incrementa o valor do contador de mudanças do arquivo de banco, guardado como
/// inteiro big-endian de 4 bytes a partir do deslocamento 24 do arquivo. O contador de
/// mudanças secundário em 92 também é atualizado, assim como o número de versão do SQLite no
/// deslocamento 96.
///
/// Mas isto só acontece se o flag `change_count_done` for falso. Para evitar agitação
/// excessiva da página 1, a atualização só ocorre uma vez. Veja também
/// `pager_write_changecounter()`, que atualiza os contadores incondicionalmente.
///
/// Se `is_direct_mode` for zero, a atualização chama `pager_write()` na página 1 e depois
/// modifica o conteúdo dela. Neste caso o arquivo é atualizado quando a transação é
/// confirmada.
///
/// `is_direct_mode` só pode ser diferente de zero se a biblioteca foi compilada com
/// SQLITE_ENABLE_ATOMIC_WRITE, o que não vale para o Debian: aqui é sempre 0.
fn pager_incr_changecounter(p_pager: &mut Pager, is_direct_mode: i32) -> i32 {
    let mut rc = SQLITE_OK;

    debug_assert!(
        p_pager.e_state as i32 == PAGER_WRITER_CACHEMOD
            || p_pager.e_state as i32 == PAGER_WRITER_DBMOD
    );

    // DIRECT_MODE é a constante 0 nesta compilação.
    debug_assert!(is_direct_mode == 0);

    if p_pager.change_count_done == 0 && p_pager.db_size > 0 {
        debug_assert!(p_pager.temp_file == 0 && is_open(&p_pager.fd));

        // Abre a página 1 do arquivo para escrita.
        let mut p_pg_hdr: Option<PgHdrRef> = None;
        rc = pager_get(p_pager, 1, &mut p_pg_hdr, 0);
        debug_assert!(p_pg_hdr.is_none() || rc == SQLITE_OK);

        // Se a página 1 foi obtida e a função não opera em modo direto, torna a página 1
        // gravável. Fora do modo direto a página 1 fica sempre no cache, então o pager_get()
        // acima sempre dá certo (daí o ALWAYS em rc==SQLITE_OK).
        if always(rc == SQLITE_OK) {
            if let Some(p_pg) = p_pg_hdr.as_ref() {
                rc = pager_write(p_pg);
            }
        }

        if rc == SQLITE_OK {
            // Faz de fato a atualização do contador de mudanças.
            if let Some(p_pg) = p_pg_hdr.as_ref() {
                pager_write_changecounter(&p_pg.borrow());
            }
            p_pager.change_count_done = 1;
        }

        // Solta a referência da página.
        pager_unref(p_pg_hdr.as_ref());
    }
    rc
}

/// Sincroniza o arquivo de banco com o disco. Não faz nada para bancos em memória nem para
/// pagers com o flag `no_sync`.
///
/// Se der certo, ou se a chamada for um no-op, devolve SQLITE_OK. Senão, um código de erro de
/// E/S.
pub fn pager_sync(p_pager: &mut Pager, z_super: Option<&[u8]>) -> i32 {
    // O `void *pArg = (void*)zSuper` do C vira uma cópia dona do nome, passada como `Any`.
    let mut p_arg: Option<Vec<u8>> = z_super.map(|s| s.to_vec());
    let mut rc = os_file_control(
        p_pager.fd.as_deref_mut().expect("fd"),
        SQLITE_FCNTL_SYNC,
        p_arg.as_mut().map(|v| v as &mut dyn std::any::Any),
    );
    if rc == SQLITE_NOTFOUND {
        rc = SQLITE_OK;
    }
    if rc == SQLITE_OK && p_pager.no_sync == 0 {
        debug_assert!(!memdb(p_pager));
        rc = os_sync(p_pager.fd.as_deref_mut().expect("fd"), p_pager.sync_flags as i32);
    }
    rc
}

/// Esta função só pode ser chamada enquanto uma transação de escrita está ativa em rollback.
/// Se a conexão está em modo WAL, a chamada é um no-op. Senão, se a conexão ainda não tem
/// lock EXCLUSIVE no arquivo de banco, tenta-se obter um.
///
/// Se o lock EXCLUSIVE já é mantido, ou a tentativa dá certo, ou a conexão está em modo WAL,
/// devolve SQLITE_OK. Senão, SQLITE_BUSY ou um código SQLITE_IOERR_XXX.
pub fn pager_exclusive_lock(p_pager: &mut Pager) -> i32 {
    let mut rc = p_pager.err_code;
    if rc == SQLITE_OK {
        debug_assert!(
            p_pager.e_state as i32 == PAGER_WRITER_CACHEMOD
                || p_pager.e_state as i32 == PAGER_WRITER_DBMOD
                || p_pager.e_state as i32 == PAGER_WRITER_LOCKED
        );
        if !pager_use_wal(p_pager) {
            rc = pager_wait_on_lock(p_pager, EXCLUSIVE_LOCK);
        }
    }
    rc
}

/// Sincroniza o arquivo de banco do pager. `z_super` é o nome de um arquivo de super-journal
/// que deve ser escrito no arquivo de journal individual. Pode ser None, o que significa que
/// não há super-journal (transação de um único banco).
///
/// Esta rotina garante que:
///
///   * o contador de mudanças do arquivo de banco é atualizado,
///   * o journal é sincronizado (a menos que a otimização de escrita atômica seja usada),
///   * todas as páginas sujas são escritas no arquivo de banco,
///   * o arquivo de banco é truncado (se necessário), e
///   * o arquivo de banco é sincronizado.
///
/// A única coisa que falta para confirmar a transação é finalizar (apagar, truncar ou zerar o
/// começo do) arquivo de journal (ou apagar o super-journal, se especificado).
///
/// Se `z_super` é None, isto não sobrescreve um valor passado antes a uma chamada de
/// `pager_commit_phase_one()`.
///
/// Se o parâmetro final `no_sync` é verdadeiro, o arquivo de banco em si não é sincronizado.
/// Neste caso o chamador deve chamar `pager_sync()` diretamente antes de chamar
/// `pager_commit_phase_two()` para apagar o journal.
pub fn pager_commit_phase_one(p_pager: &mut Pager, z_super: Option<&[u8]>, no_sync: i32) -> i32 {
    let mut rc = SQLITE_OK;

    debug_assert!(
        p_pager.e_state as i32 == PAGER_WRITER_LOCKED
            || p_pager.e_state as i32 == PAGER_WRITER_CACHEMOD
            || p_pager.e_state as i32 == PAGER_WRITER_DBMOD
            || p_pager.e_state as i32 == PAGER_ERROR
    );

    // Se ocorreu um erro antes, relata o mesmo erro de novo.
    if never(p_pager.err_code != 0) {
        return p_pager.err_code;
    }

    // Permite simular facilmente um erro de E/S durante testes.
    if fault_sim(400) != 0 {
        return SQLITE_IOERR;
    }

    // Se nenhuma mudança foi feita no banco, volta cedo.
    if (p_pager.e_state as i32) < PAGER_WRITER_CACHEMOD {
        return SQLITE_OK;
    }

    debug_assert!(!memdb(p_pager) || p_pager.temp_file != 0);
    debug_assert!(is_open(&p_pager.fd) || p_pager.temp_file != 0);

    // O rótulo `commit_phase_one_exit` do C: os goto viram `break 'commit_phase_one_exit`.
    'commit_phase_one_exit: {
        if pager_flush_on_commit(p_pager, 1) == 0 {
            // Se é banco em memória, ou nenhuma página foi escrita, ou esta função já foi
            // chamada, é quase um no-op. Porém, qualquer backup em andamento precisa ser
            // reiniciado.
            // Ponte: Pager.p_backup precisa ser Option<BackupRef> (lista de backups) para
            // casar com `backup_restart(Option<&BackupRef>)` de backup_c.
            backup_restart(p_pager.p_backup.as_ref());
        } else {
            if pager_use_wal(p_pager) {
                let mut p_page_one: Option<PgHdrRef> = None;
                let p_cache: PCacheRef = p_pager.p_p_cache.clone().expect("pPCache");
                let mut p_list: Option<PgHdrRef> = pcache_dirty_list(&mut p_cache.borrow_mut());
                if p_list.is_none() {
                    // Precisa haver pelo menos uma página para o flag de commit do WAL.
                    // Ticket [2d1a5c67dfc2363e44f29d9bbd57f] 2011-05-18
                    rc = pager_get(p_pager, 1, &mut p_page_one, 0);
                    p_list = p_page_one.clone();
                    if let Some(p) = p_list.as_ref() {
                        p.borrow_mut().p_dirty = None;
                    }
                }
                debug_assert!(rc == SQLITE_OK);
                if always(p_list.is_some()) {
                    rc = pager_wal_frames(p_pager, p_list.as_ref(), p_pager.db_size, 1);
                }
                pager_unref(p_page_one.as_ref());
                if rc == SQLITE_OK {
                    pcache_clean_all(&p_cache);
                }
            } else {
                // bBatch é a constante 0 nesta compilação (sem BATCH_ATOMIC_WRITE), e o
                // bloco de SQLITE_ENABLE_ATOMIC_WRITE some: o contador de mudanças é sempre
                // atualizado em modo indireto.
                rc = pager_incr_changecounter(p_pager, 0);
                if rc != SQLITE_OK {
                    break 'commit_phase_one_exit;
                }

                // Escreve o nome do super-journal no arquivo de journal. Se um nome de
                // super-journal já foi escrito, ou se `z_super` é None (sem super-journal),
                // esta chamada é um no-op.
                rc = write_super_journal(p_pager, z_super);
                if rc != SQLITE_OK {
                    break 'commit_phase_one_exit;
                }

                // Sincroniza o arquivo de journal e escreve todas as páginas sujas no banco.
                // Se a otimização de atualização atômica está em uso, este sync não cria o
                // journal nem faz E/S real.
                //
                // Como a página do contador de mudanças acabou de ser modificada, a menos
                // que a otimização atômica seja usada, é quase certo que o journal precisa
                // de sync aqui. Porém, em locking_mode=exclusive num sistema sob pressão de
                // memória é possível que não seja o caso. Aí é provável que o xSync()
                // redundante vire um no-op no próprio SO.
                rc = sync_journal(p_pager, 0);
                if rc != SQLITE_OK {
                    break 'commit_phase_one_exit;
                }

                let p_cache: PCacheRef = p_pager.p_p_cache.clone().expect("pPCache");
                let p_list: Option<PgHdrRef> = pcache_dirty_list(&mut p_cache.borrow_mut());

                // bBatch==0 sempre: escreve a lista de páginas.
                rc = pager_write_pagelist(p_pager, p_list.as_ref());
                if rc != SQLITE_OK {
                    debug_assert!(rc != SQLITE_IOERR_BLOCKED);
                    break 'commit_phase_one_exit;
                }
                pcache_clean_all(&p_cache);

                // Se o arquivo em disco é menor que a imagem do banco, usa pager_truncate
                // para aumentar o arquivo aqui. Isto pode ocorrer se a imagem foi estendida
                // na transação atual e depois a última página da imagem foi movida para a
                // lista livre. Neste caso a última página nunca é escrita em disco e o
                // arquivo fica pequeno demais. Corrige agora se for o caso.
                if p_pager.db_size > p_pager.db_file_size {
                    let n_new: Pgno = p_pager.db_size
                        - (p_pager.db_size == pager_sj_pgno(p_pager)) as Pgno;
                    debug_assert!(p_pager.e_state as i32 == PAGER_WRITER_DBMOD);
                    rc = pager_truncate(p_pager, n_new);
                    if rc != SQLITE_OK {
                        break 'commit_phase_one_exit;
                    }
                }

                // Por fim, sincroniza o arquivo de banco.
                if no_sync == 0 {
                    rc = pager_sync(p_pager, z_super);
                }
            }
        }
    }

    // commit_phase_one_exit:
    if rc == SQLITE_OK && !pager_use_wal(p_pager) {
        p_pager.e_state = PAGER_WRITER_FINISHED as u8;
    }
    rc
}


// ---- part_016.rs ----

/// Quando esta função é chamada, o arquivo de banco de dados foi completamente
/// atualizado para refletir as mudanças feitas pela transação atual e
/// sincronizado em disco. O arquivo de journal ainda existe no sistema de
/// arquivos, e se uma falha ocorrer neste ponto ele será eventualmente usado
/// como hot-journal e a transação atual será revertida.
///
/// Esta função finaliza o arquivo de journal, apagando, truncando ou zerando
/// parcialmente, de modo que ele não possa ser usado para reversão de
/// hot-journal. Feito isso, a transação está irrevogavelmente confirmada.
///
/// Se ocorrer um erro, um código de erro de E/S é retornado e o pager passa
/// ao estado de erro. Caso contrário, SQLITE_OK é retornado.
pub fn pager_commit_phase_two(p_pager: &mut Pager) -> i32 {
    // Esta rotina não deve ser chamada se um erro anterior ocorreu. Mas se
    // (por erro de codificação em outro lugar) ela for chamada, apenas
    // retorna o mesmo código de erro sem fazer nada.
    if never(p_pager.err_code != 0) {
        return p_pager.err_code;
    }
    p_pager.i_data_version = p_pager.i_data_version.wrapping_add(1);

    debug_assert!(
        p_pager.e_state as i32 == PAGER_WRITER_LOCKED
            || p_pager.e_state as i32 == PAGER_WRITER_FINISHED
            || (pager_use_wal(p_pager) && p_pager.e_state as i32 == PAGER_WRITER_CACHEMOD)
    );
    debug_assert!(assert_pager_state(p_pager));

    // Uma otimização. Se o banco de dados não foi realmente modificado durante
    // esta transação, o pager está em modo exclusivo e usa journals
    // persistentes, então esta função é uma operação vazia.
    //
    // O início do arquivo de journal contém atualmente um único cabeçalho de
    // journal com o campo nRec igual a 0. Se tal journal for usado como
    // hot-journal durante a reversão, 0 mudanças serão feitas no arquivo de
    // banco de dados. Então não há necessidade de zerar o cabeçalho do journal.
    // Como o pager está em modo exclusivo, também não há necessidade de
    // liberar nenhum lock.
    if p_pager.e_state as i32 == PAGER_WRITER_LOCKED
        && p_pager.exclusive_mode != 0
        && p_pager.journal_mode as i32 == PAGER_JOURNALMODE_PERSIST
    {
        debug_assert!(p_pager.journal_off == journal_hdr_sz(p_pager) as i64 || p_pager.journal_off == 0);
        p_pager.e_state = PAGER_READER as u8;
        return SQLITE_OK;
    }

    let set_super = p_pager.set_super as i32;
    let rc = pager_end_transaction(p_pager, set_super, 1);
    pager_error(p_pager, rc)
}

/// Se uma transação de escrita estiver aberta, todas as mudanças feitas na
/// transação são revertidas e a transação de escrita atual é fechada. O pager
/// volta ao estado PAGER_READER se bem-sucedido, ou ao estado PAGER_ERROR se
/// ocorrer um erro.
///
/// Se o pager já estiver no estado PAGER_ERROR quando esta função for chamada,
/// ela retorna Pager.errCode imediatamente. Nenhum trabalho é feito neste caso.
///
/// Caso contrário, no modo rollback, esta função faz duas coisas:
///
///   1) Reverte o arquivo de journal, restaurando todas as páginas do arquivo
///      de banco de dados e do cache em memória ao estado em que estavam
///      quando a transação foi aberta, e
///
///   2) Finaliza o arquivo de journal, de modo que ele não seja usado para
///      reversão a quente em nenhum momento futuro.
///
/// A finalização do arquivo de journal (tarefa 2) só é feita se a reversão
/// for bem-sucedida.
///
/// No modo WAL, todas as entradas de cache contendo dados modificados dentro
/// da transação atual são expulsas do cache ou revertidas ao estado anterior
/// à transação relendo do banco de dados ou dos arquivos WAL. A transação WAL
/// é então fechada.
pub fn pager_rollback(p_pager: &mut Pager) -> i32 {
    let mut rc = SQLITE_OK;

    // PagerRollback() é uma operação vazia se chamada nos estados READER ou
    // OPEN. Se o pager já estiver no estado ERROR, a reversão não é tentada
    // aqui. Em vez disso, o código de erro é retornado ao chamador.
    debug_assert!(assert_pager_state(p_pager));
    if p_pager.e_state as i32 == PAGER_ERROR {
        return p_pager.err_code;
    }
    if p_pager.e_state as i32 <= PAGER_READER {
        return SQLITE_OK;
    }

    if pager_use_wal(p_pager) {
        rc = pager_savepoint(p_pager, SAVEPOINT_ROLLBACK, -1);
        let set_super = p_pager.set_super as i32;
        let rc2 = pager_end_transaction(p_pager, set_super, 0);
        if rc == SQLITE_OK {
            rc = rc2;
        }
    } else if !is_open(&p_pager.jfd) || p_pager.e_state as i32 == PAGER_WRITER_LOCKED {
        let e_state = p_pager.e_state as i32;
        rc = pager_end_transaction(p_pager, 0, 0);
        if !memdb(p_pager) && e_state > PAGER_WRITER_LOCKED {
            // Isto pode acontecer usando journal_mode=off. Move o pager para o
            // estado de erro para indicar que o conteúdo do cache pode não ser
            // confiável. Qualquer leitor ativo receberá SQLITE_ABORT.
            p_pager.err_code = SQLITE_ABORT;
            p_pager.e_state = PAGER_ERROR as u8;
            set_getter_method(p_pager);
            return rc;
        }
    } else {
        rc = pager_playback(p_pager, 0);
    }

    debug_assert!(p_pager.e_state as i32 == PAGER_READER || rc != SQLITE_OK);
    debug_assert!(
        rc == SQLITE_OK
            || rc == SQLITE_FULL
            || rc == SQLITE_CORRUPT
            || rc == SQLITE_NOMEM
            || (rc & 0xFF) == SQLITE_IOERR
            || rc == SQLITE_CANTOPEN
    );

    // Se um erro ocorrer durante um ROLLBACK, não podemos mais confiar no
    // cache do pager. Então chama pager_error() na saída para tornar
    // qualquer erro persistente.
    pager_error(p_pager, rc)
}

/// Retorna VERDADEIRO se o arquivo de banco de dados foi aberto somente para
/// leitura. Retorna FALSO se o banco de dados é (em teoria) gravável.
pub fn pager_isreadonly(p_pager: &Pager) -> u8 {
    p_pager.read_only
}

/// Retorna o número aproximado de bytes de memória atualmente usados pelo
/// pager e seu cache associado.
///
/// O tamanho de PgHdr mais cinco ponteiros é 48 + 5*8 em 64 bits. O valor de
/// sqlite3MallocSize(pPager) vem de `pager_struct_alloc_size`, que o integrador
/// define sobre a estrutura Pager (o C usa o tamanho da alocação do malloc).
pub fn pager_mem_used(p_pager: &Pager) -> i32 {
    let per_page_size: i32 =
        p_pager.page_size as i32 + p_pager.n_extra as i32 + (PGHDR_SIZEOF as i32 + 5 * 8);
    let n_page = match p_pager.p_p_cache.as_ref() {
        Some(c) => pcache_pagecount(c),
        None => 0,
    };
    per_page_size
        .wrapping_mul(n_page)
        .wrapping_add(pager_struct_alloc_size(p_pager))
        .wrapping_add(p_pager.page_size as i32)
}

/// Retorna o número de referências à página especificada.
pub fn pager_page_refcount(p_page: &DbPageRef) -> i32 {
    pcache_page_refcount(p_page) as i32
}

/// O parâmetro e_stat deve ser um de SQLITE_DBSTATUS_CACHE_HIT, _MISS, _WRITE
/// ou _WRITE+1. O caso SQLITE_DBSTATUS_CACHE_WRITE+1 é uma tradução de
/// SQLITE_DBSTATUS_CACHE_SPILL. O caso _SPILL não é contíguo porque foi
/// adicionado depois.
///
/// Antes de retornar, *pn_val é incrementado pela contagem atual de acertos ou
/// falhas do cache, conforme e_stat. Se o parâmetro reset for diferente de
/// zero, a contagem é zerada antes de retornar.
pub fn pager_cache_stat(p_pager: &mut Pager, e_stat: i32, reset: i32, pn_val: &mut u64) {
    debug_assert!(
        e_stat == SQLITE_DBSTATUS_CACHE_HIT
            || e_stat == SQLITE_DBSTATUS_CACHE_MISS
            || e_stat == SQLITE_DBSTATUS_CACHE_WRITE
            || e_stat == SQLITE_DBSTATUS_CACHE_WRITE + 1
    );
    debug_assert!(SQLITE_DBSTATUS_CACHE_HIT + 1 == SQLITE_DBSTATUS_CACHE_MISS);
    debug_assert!(SQLITE_DBSTATUS_CACHE_HIT + 2 == SQLITE_DBSTATUS_CACHE_WRITE);
    debug_assert!(
        PAGER_STAT_HIT == 0 && PAGER_STAT_MISS == 1 && PAGER_STAT_WRITE == 2 && PAGER_STAT_SPILL == 3
    );

    let idx = (e_stat - SQLITE_DBSTATUS_CACHE_HIT) as usize;
    *pn_val = pn_val.wrapping_add(p_pager.a_stat[idx] as u64);
    if reset != 0 {
        p_pager.a_stat[idx] = 0;
    }
}

/// Retorna verdadeiro se este é um pager em memória ou apoiado por arquivo
/// temporário.
pub fn pager_is_memdb(p_pager: &Pager) -> i32 {
    (p_pager.temp_file != 0 || p_pager.mem_vfs != 0) as i32
}

/// Verifica se há pelo menos n_savepoint savepoints abertos. Se há atualmente
/// menos que n_savepoints abertos, abre um ou mais savepoints para compensar a
/// diferença. Se o número de savepoints já é igual a n_savepoint, esta função
/// é uma operação vazia.
///
/// Se uma alocação de memória falhar, SQLITE_NOMEM é retornado. Se ocorrer um
/// erro ao abrir o arquivo de sub-journal, um código de erro de E/S é
/// retornado. Caso contrário, SQLITE_OK.
fn pager_open_savepoint_static(p_pager: &mut Pager, n_savepoint: i32) -> i32 {
    let rc = SQLITE_OK;
    let n_current = p_pager.n_savepoint;

    debug_assert!(p_pager.e_state as i32 >= PAGER_WRITER_LOCKED);
    debug_assert!(assert_pager_state(p_pager));
    debug_assert!(n_savepoint > n_current && p_pager.use_journal != 0);

    // Cresce o array Pager.aSavepoint (o realloc do C). A porção nova é
    // zerada para o caso de uma falha de malloc ao preenchê-la no laço abaixo.
    let mut a_new: Vec<PagerSavepoint> = match p_pager.a_savepoint.take() {
        Some(old) => old.into_vec(),
        None => Vec::new(),
    };
    a_new.truncate(n_current as usize);
    while a_new.len() < n_savepoint as usize {
        a_new.push(PagerSavepoint {
            i_offset: 0,
            i_hdr_offset: 0,
            p_in_savepoint: None,
            n_orig: 0,
            i_sub_rec: 0,
            b_truncate_on_release: 0,
            a_wal_data: [0; WAL_SAVEPOINT_NDATA],
        });
    }

    // Preenche as estruturas PagerSavepoint recém alocadas.
    let mut ii = n_current;
    while ii < n_savepoint {
        let iu = ii as usize;
        a_new[iu].n_orig = p_pager.db_size;
        if is_open(&p_pager.jfd) && p_pager.journal_off > 0 {
            a_new[iu].i_offset = p_pager.journal_off;
        } else {
            a_new[iu].i_offset = journal_hdr_sz(p_pager) as i64;
        }
        a_new[iu].i_sub_rec = p_pager.n_sub_rec;
        a_new[iu].p_in_savepoint = bitvec_create(p_pager.db_size);
        a_new[iu].b_truncate_on_release = 1;
        if a_new[iu].p_in_savepoint.is_none() {
            p_pager.a_savepoint = Some(a_new.into_boxed_slice());
            return SQLITE_NOMEM_BKPT;
        }
        if pager_use_wal(p_pager) {
            if let Some(p_wal) = p_pager.p_wal.as_ref() {
                wal_savepoint(p_wal, &mut a_new[iu].a_wal_data);
            }
        }
        p_pager.n_savepoint = ii + 1;
        ii += 1;
    }
    p_pager.a_savepoint = Some(a_new.into_boxed_slice());
    debug_assert!(p_pager.n_savepoint == n_savepoint);
    rc
}

pub fn pager_open_savepoint(p_pager: &mut Pager, n_savepoint: i32) -> i32 {
    debug_assert!(p_pager.e_state as i32 >= PAGER_WRITER_LOCKED);
    debug_assert!(assert_pager_state(p_pager));

    if n_savepoint > p_pager.n_savepoint && p_pager.use_journal != 0 {
        pager_open_savepoint_static(p_pager, n_savepoint)
    } else {
        SQLITE_OK
    }
}

/// Esta função é chamada para reverter ou liberar (confirmar) um savepoint.
/// O savepoint a liberar ou reverter não precisa ser o mais recentemente
/// criado.
///
/// O parâmetro op é sempre SAVEPOINT_ROLLBACK ou SAVEPOINT_RELEASE. Se for
/// SAVEPOINT_RELEASE, libera e destrói o savepoint de índice i_savepoint. Se
/// for SAVEPOINT_ROLLBACK, reverte todas as mudanças ocorridas desde que o
/// savepoint especificado foi criado.
///
/// O savepoint a reverter ou liberar é identificado pelo parâmetro
/// i_savepoint. O valor 0 significa operar no savepoint mais externo (o
/// primeiro criado). O valor (Pager.nSavepoint-1) significa operar no
/// savepoint mais recentemente criado. Se i_savepoint for maior que
/// (Pager.nSavepoint-1), esta função é uma operação vazia.
///
/// Se um valor negativo for passado, a transação atual é revertida. Isto é
/// diferente de chamar pager_rollback() porque esta função não termina a
/// transação nem destrava o banco de dados, apenas restaura o conteúdo do
/// banco de dados ao seu estado original.
///
/// Em qualquer caso, todos os savepoints com índice maior que i_savepoint são
/// destruídos. Se for uma liberação (op==SAVEPOINT_RELEASE), o savepoint
/// i_savepoint também é destruído.
///
/// Esta função pode retornar SQLITE_NOMEM se uma alocação falhar, ou um código
/// de erro de E/S se ocorrer um erro de E/S ao reverter um savepoint. Se não
/// ocorrer nenhum erro, SQLITE_OK é retornado.
pub fn pager_savepoint(p_pager: &mut Pager, op: i32, i_savepoint: i32) -> i32 {
    let mut rc = p_pager.err_code;

    debug_assert!(op == SAVEPOINT_RELEASE || op == SAVEPOINT_ROLLBACK);
    debug_assert!(i_savepoint >= 0 || op == SAVEPOINT_ROLLBACK);

    if rc == SQLITE_OK && i_savepoint < p_pager.n_savepoint {
        // Calcula quantos savepoints ainda estarão ativos depois desta
        // operação e guarda em n_new. Depois libera os recursos associados aos
        // savepoints destruídos por esta operação.
        let n_new: i32 = i_savepoint + if op == SAVEPOINT_RELEASE { 0 } else { 1 };
        let mut ii = n_new;
        while ii < p_pager.n_savepoint {
            let sp = &mut p_pager.a_savepoint.as_mut().unwrap()[ii as usize];
            bitvec_destroy(sp.p_in_savepoint.take());
            ii += 1;
        }
        p_pager.n_savepoint = n_new;

        // Trunca o sub-journal para que inclua só as partes ainda em uso.
        if op == SAVEPOINT_RELEASE {
            let (b_truncate, i_sub_rec) = {
                let p_rel = &p_pager.a_savepoint.as_ref().unwrap()[n_new as usize];
                (p_rel.b_truncate_on_release, p_rel.i_sub_rec)
            };
            if b_truncate != 0 && is_open(&p_pager.sjfd) {
                // Só trunca se for um sub-journal em memória.
                if journal_is_in_memory(open_file_mut(&mut p_pager.sjfd)) != 0 {
                    let sz: i64 = (p_pager.page_size + 4) * (i_sub_rec as i64);
                    rc = os_truncate(open_file_mut(&mut p_pager.sjfd), sz);
                    debug_assert!(rc == SQLITE_OK);
                }
                p_pager.n_sub_rec = i_sub_rec;
            }
        }
        // Senão, é uma operação de reversão: reproduz o savepoint
        // especificado. Se for um arquivo temporário, é possível que o arquivo
        // de journal ainda não tenha sido aberto. Neste caso não houve mudanças
        // no arquivo de banco de dados, então a reprodução pode ser pulada.
        else if pager_use_wal(p_pager) || is_open(&p_pager.jfd) {
            // pager_playback_savepoint recebe o índice do savepoint dentro de
            // p_pager.a_savepoint (None quando n_new == 0), pois o C passa um
            // ponteiro para o próprio array do pager.
            let p_savepoint = if n_new == 0 { None } else { Some((n_new - 1) as usize) };
            rc = pager_playback_savepoint_at(p_pager, p_savepoint);
            debug_assert!(rc != SQLITE_DONE);
        }
    }

    rc
}


// ---- part_017.rs ----

/// Retorna o caminho completo do arquivo de banco de dados.
///
/// Exceto, se o pager for apenas em memória, então retorna uma string vazia se
/// null_if_mem_db for verdadeiro. Esta rotina é chamada com null_if_mem_db==1 quando
/// usada para relatar o nome do arquivo ao usuário, por compatibilidade com o
/// comportamento legado. Mas quando a Btree precisa saber o nome do arquivo para
/// correspondência com cache compartilhado, ela usa null_if_mem_db==0 para que
/// bancos de dados em memória possam participar do cache compartilhado.
///
/// O valor de retorno para esta rotina é sempre seguro para usar com
/// sqlite3_uri_parameter() e sqlite3_filename_database() e amigos.
pub fn pager_filename(p_pager: &Pager, null_if_mem_db: i32) -> &[u8] {
    static Z_FAKE: [u8; 8] = [0, 0, 0, 0, 0, 0, 0, 0];
    if null_if_mem_db != 0 && (memdb(p_pager) || is_memdb(p_pager.p_vfs.as_ref())) {
        &Z_FAKE[4..]
    } else {
        &p_pager.z_filename
    }
}

/// Retorna a estrutura VFS do pager.
pub fn pager_vfs(p_pager: &Pager) -> Option<&Sqlite3Vfs> {
    p_pager.p_vfs.as_ref().map(|b| b.as_ref())
}

/// Retorna o manipulador de arquivo do arquivo de banco de dados associado
/// ao pager. Isto pode retornar NULL se o arquivo ainda não foi aberto.
pub fn pager_file(p_pager: &Pager) -> Option<&Sqlite3File> {
    p_pager.fd.as_ref().map(|b| b.as_ref())
}

/// Retorna o manipulador de arquivo do arquivo de journal (se existir).
/// Isto será o journal de reversão ou o arquivo WAL.
pub fn pager_jrnl_file(p_pager: &Pager) -> Option<&Sqlite3File> {
    if let Some(ref p_wal) = p_pager.p_wal {
        wal_file(p_wal)
    } else {
        p_pager.jfd.as_ref().map(|b| b.as_ref())
    }
}

/// Retorna o caminho completo do arquivo de journal.
pub fn pager_journalname(p_pager: &Pager) -> &[u8] {
    &p_pager.z_journal
}

/// Move a página pPg para a localização pgno no arquivo.
///
/// Não deve haver referências à página previamente localizada em
/// pgno (que chamamos pPgOld), embora essa página possa estar em cache. Se a página
/// previamente localizada em pgno não está já no journal de reversão, ela não é
/// colocada lá por esta rotina.
///
/// Referências à página pPg permanecem válidas. Atualizar qualquer
/// metadado associado com pPg (isto é, dados armazenados nos nBytes
/// alocados junto com a página) é responsabilidade do chamador.
///
/// Uma transação deve estar ativa quando esta rotina é chamada. Costumava ser
/// necessário que uma transação de instrução não estivesse ativa, mas esta restrição
/// foi removida (CREATE INDEX precisa mover uma página quando uma transação de
/// instrução está ativa).
///
/// Se o quarto argumento, is_commit, for diferente de zero, então esta página está
/// sendo movida como parte de uma reorganização de banco de dados pouco antes da transação
/// estar sendo confirmada. Neste caso, é garantido que a página de banco de dados
/// pPg refere-se não será escrita novamente dentro desta transação.
///
/// Esta função pode retornar SQLITE_NOMEM ou um código de erro de E/S se um erro
/// ocorrer. Caso contrário, retorna SQLITE_OK.
pub fn pager_movepage(
    p_pager: &mut Pager,
    p_pg: &PgHdrRef,
    pgno: Pgno,
    is_commit: i32,
) -> i32 {
    let mut need_sync_pgno: Pgno = 0;
    let mut rc: i32;
    let orig_pgno: Pgno;

    debug_assert!(p_pg.borrow().n_ref > 0);
    debug_assert!(
        p_pager.e_state == PAGER_WRITER_CACHEMOD as u8
            || p_pager.e_state == PAGER_WRITER_DBMOD as u8
    );
    debug_assert!(assert_pager_state(p_pager));

    debug_assert!(p_pager.temp_file != 0 || !memdb(p_pager));
    if p_pager.temp_file != 0 {
        rc = pager_write(p_pg);
        if rc != SQLITE_OK {
            return rc;
        }
    }

    if (p_pg.borrow().flags & PGHDR_DIRTY) != 0
        && SQLITE_OK != (rc = subjournal_page_if_required(p_pg))
    {
        return rc;
    }

    if (p_pg.borrow().flags & PGHDR_NEED_SYNC) != 0 && is_commit == 0 {
        need_sync_pgno = p_pg.borrow().pgno;
        debug_assert!(
            p_pager.journal_mode as i32 == PAGER_JOURNALMODE_OFF
                || page_in_journal(p_pager, p_pg)
                || p_pg.borrow().pgno > p_pager.db_orig_size
        );
        debug_assert!((p_pg.borrow().flags & PGHDR_DIRTY) != 0);
    }

    p_pg.borrow_mut().flags &= !PGHDR_NEED_SYNC;
    let p_pg_old: Option<PgHdrRef> = pager_lookup(p_pager, pgno);
    debug_assert!(p_pg_old.is_none() || p_pg_old.as_ref().unwrap().borrow().n_ref == 1 || corrupt_db());
    if let Some(ref p_pg_old_ref) = p_pg_old {
        if p_pg_old_ref.borrow().n_ref > 1 {
            pager_unref_not_null(p_pg_old_ref);
            return SQLITE_CORRUPT_BKPT;
        }
        p_pg.borrow_mut().flags |= p_pg_old_ref.borrow().flags & PGHDR_NEED_SYNC;
        if p_pager.temp_file != 0 {
            pcache_move(p_pg_old_ref, p_pager.db_size.wrapping_add(1));
        } else {
            pcache_drop(p_pg_old_ref);
        }
    }

    orig_pgno = p_pg.borrow().pgno;
    pcache_move(p_pg, pgno);
    pcache_make_dirty(p_pg);

    if p_pager.temp_file != 0 && p_pg_old.is_some() {
        let p_pg_old_ref = p_pg_old.unwrap();
        pcache_move(&p_pg_old_ref, orig_pgno);
        pager_unref_not_null(&p_pg_old_ref);
    }

    if need_sync_pgno != 0 {
        let mut p_pg_hdr: Option<PgHdrRef> = None;
        rc = pager_get(p_pager, need_sync_pgno, &mut p_pg_hdr, 0);
        if rc != SQLITE_OK {
            if need_sync_pgno <= p_pager.db_orig_size {
                debug_assert!(p_pager.p_tmp_space.is_some());
                bitvec_clear(
                    p_pager.p_in_journal.as_deref_mut(),
                    need_sync_pgno,
                    p_pager.p_tmp_space.as_deref_mut().unwrap_or(&mut []),
                );
            }
            return rc;
        }
        if let Some(p_pg_hdr) = p_pg_hdr {
            p_pg_hdr.borrow_mut().flags |= PGHDR_NEED_SYNC;
            pcache_make_dirty(&p_pg_hdr);
            pager_unref_not_null(&p_pg_hdr);
        }
    }

    SQLITE_OK
}

/// O manipulador de página passado como primeiro argumento refere-se a uma página suja
/// com um número de página diferente de i_new. Esta função muda o número de página da página
/// para i_new e define o valor do campo PgHdr.flags para
/// o valor passado como terceiro parâmetro.
pub fn pager_rekey(p_pg: &PgHdrRef, i_new: Pgno, flags: u16) {
    debug_assert!(p_pg.borrow().pgno != i_new);
    p_pg.borrow_mut().flags = flags;
    pcache_move(p_pg, i_new);
}

/// Retorna um ponteiro para os dados da página especificada.
pub fn pager_get_data(p_pg: &PgHdrRef) -> &[u8] {
    let pg = p_pg.borrow();
    debug_assert!(pg.n_ref > 0 || pg.p_pager.as_ref().map_or(false, |w| {
        w.upgrade().map_or(false, |p| {
            p.borrow().mem_db != 0
        })
    }));
    &pg.p_data
}

/// Retorna um ponteiro para os nExtra bytes de espaço "extra"
/// alocados junto com a página especificada.
pub fn pager_get_extra(p_pg: &PgHdrRef) -> &[u8] {
    &p_pg.borrow().p_extra
}

/// Pega/define o modo de bloqueio para este pager. Parâmetro e_mode deve ser um de
/// PAGER_LOCKINGMODE_QUERY, PAGER_LOCKINGMODE_NORMAL ou
/// PAGER_LOCKINGMODE_EXCLUSIVE. Se o parâmetro não for _QUERY, então
/// o modo de bloqueio é definido para o valor especificado.
///
/// O valor retornado é PAGER_LOCKINGMODE_NORMAL ou
/// PAGER_LOCKINGMODE_EXCLUSIVE, indicando o modo de bloqueio atual (possivelmente
/// atualizado).
pub fn pager_locking_mode(p_pager: &mut Pager, e_mode: i32) -> i32 {
    debug_assert!(
        e_mode == PAGER_LOCKINGMODE_QUERY
            || e_mode == PAGER_LOCKINGMODE_NORMAL
            || e_mode == PAGER_LOCKINGMODE_EXCLUSIVE
    );
    debug_assert!(PAGER_LOCKINGMODE_QUERY < 0);
    debug_assert!(
        PAGER_LOCKINGMODE_NORMAL >= 0 && PAGER_LOCKINGMODE_EXCLUSIVE >= 0
    );
    debug_assert!(
        p_pager.exclusive_mode != 0
            || wal_heap_memory(p_pager.p_wal.as_ref()) == 0
    );
    if e_mode >= 0 && p_pager.temp_file == 0 && wal_heap_memory(p_pager.p_wal.as_ref()) == 0
    {
        p_pager.exclusive_mode = (e_mode as u8);
    }
    p_pager.exclusive_mode as i32
}

/// Define o modo de journal para este pager. Parâmetro e_mode deve ser um de:
///
///    PAGER_JOURNALMODE_DELETE
///    PAGER_JOURNALMODE_TRUNCATE
///    PAGER_JOURNALMODE_PERSIST
///    PAGER_JOURNALMODE_OFF
///    PAGER_JOURNALMODE_MEMORY
///    PAGER_JOURNALMODE_WAL
///
/// O modo de journal é definido para o valor especificado se a mudança for permitida.
/// A mudança pode ser não permitida pelos seguintes motivos:
///
///   *  Um banco de dados em memória pode ter seu journal_mode definido apenas para _OFF
///      ou _MEMORY.
///
///   *  Bancos de dados temporários não podem ter modo de journalmode _WAL.
///
/// O retorno indica o modo de journal atual (possivelmente atualizado).
pub fn pager_set_journal_mode(p_pager: &mut Pager, e_mode: i32) -> i32 {
    let e_old: u8 = p_pager.journal_mode;

    debug_assert!(
        e_mode == PAGER_JOURNALMODE_DELETE
            || e_mode == PAGER_JOURNALMODE_PERSIST
            || e_mode == PAGER_JOURNALMODE_OFF
            || e_mode == PAGER_JOURNALMODE_TRUNCATE
            || e_mode == PAGER_JOURNALMODE_MEMORY
            || e_mode == PAGER_JOURNALMODE_WAL
    );

    debug_assert!(p_pager.temp_file == 0 || e_mode != PAGER_JOURNALMODE_WAL);

    let mut e_mode = e_mode;
    if memdb(p_pager) {
        debug_assert!(
            e_old as i32 == PAGER_JOURNALMODE_MEMORY || e_old as i32 == PAGER_JOURNALMODE_OFF
        );
        if e_mode != PAGER_JOURNALMODE_MEMORY && e_mode != PAGER_JOURNALMODE_OFF {
            e_mode = e_old as i32;
        }
    }

    if e_mode != (e_old as i32) {
        debug_assert!(p_pager.e_state != PAGER_ERROR as u8);
        p_pager.journal_mode = (e_mode as u8);

        debug_assert!(((PAGER_JOURNALMODE_TRUNCATE & 5) as u8) == 1);
        debug_assert!(((PAGER_JOURNALMODE_PERSIST & 5) as u8) == 1);
        debug_assert!(((PAGER_JOURNALMODE_DELETE & 5) as u8) == 0);
        debug_assert!(((PAGER_JOURNALMODE_MEMORY & 5) as u8) == 4);
        debug_assert!(((PAGER_JOURNALMODE_OFF & 5) as u8) == 0);
        debug_assert!(((PAGER_JOURNALMODE_WAL & 5) as u8) == 5);

        debug_assert!(is_open(&p_pager.fd) || p_pager.exclusive_mode != 0);
        if p_pager.exclusive_mode == 0
            && ((e_old as i32) & 5) == 1
            && (e_mode & 1) == 0
        {
            os_close(&mut p_pager.jfd);
            if p_pager.e_lock as i32 >= RESERVED_LOCK {
                os_delete(
                    p_pager.p_vfs.as_ref(),
                    &p_pager.z_journal,
                    0,
                );
            } else {
                let mut rc: i32 = SQLITE_OK;
                let state: i32 = p_pager.e_state as i32;
                debug_assert!(state == PAGER_OPEN || state == PAGER_READER);
                if state == PAGER_OPEN {
                    rc = pager_shared_lock(p_pager);
                }
                if p_pager.e_state as i32 == PAGER_READER {
                    debug_assert!(rc == SQLITE_OK);
                    rc = pager_lock_db(p_pager, RESERVED_LOCK);
                }
                if rc == SQLITE_OK {
                    os_delete(
                        p_pager.p_vfs.as_ref(),
                        &p_pager.z_journal,
                        0,
                    );
                }
                if rc == SQLITE_OK && state == PAGER_READER {
                    pager_unlock_db(p_pager, SHARED_LOCK);
                } else if state == PAGER_OPEN {
                    pager_unlock(p_pager);
                }
                debug_assert!(state == p_pager.e_state as i32);
            }
        } else if e_mode == PAGER_JOURNALMODE_OFF || e_mode == PAGER_JOURNALMODE_MEMORY {
            os_close(&mut p_pager.jfd);
        }
    }

    p_pager.journal_mode as i32
}


// ---- part_018.rs ----

/// Retorna o modo de journal atual.
pub fn pager_get_journal_mode(p_pager: &Pager) -> i32 {
    p_pager.journal_mode as i32
}

/// Retorna verdadeiro se o pager está em um estado onde é ok mudar o
/// modo de journal. Mudanças de modo de journal só podem acontecer quando
/// o banco de dados não foi modificado.
pub fn pager_ok_to_change_journal_mode(p_pager: &Pager) -> i32 {
    debug_assert!(assert_pager_state(p_pager));
    if p_pager.e_state >= PAGER_WRITER_CACHEMOD as u8 {
        return 0;
    }
    if never(is_open(&p_pager.jfd) && p_pager.journal_off > 0) {
        return 0;
    }
    1
}

/// Obtém ou define o limite de tamanho usado para arquivos de journal persistentes.
///
/// Definir o limite de tamanho para -1 significa que nenhum limite é aplicado.
/// Uma tentativa de definir um limite menor que -1 é uma operação vazia.
pub fn pager_journal_size_limit(p_pager: &mut Pager, i_limit: i64) -> i64 {
    if i_limit >= -1 {
        p_pager.journal_size_limit = i_limit;
        wal_limit(p_pager.p_wal.as_mut(), i_limit);
    }
    p_pager.journal_size_limit
}

/// Retorna uma referência mutável ao campo p_backup do pager. O módulo
/// de backup em backup.c mantém o conteúdo desta variável. Este módulo
/// a usa de forma opaca como argumento para sqlite3BackupRestart() e
/// sqlite3BackupUpdate() apenas.
pub fn pager_backup_ptr(p_pager: &mut Pager) -> &mut Option<Box<SqliteBackup>> {
    &mut p_pager.p_backup
}

/// A menos que este seja um banco de dados em memória ou temporário,
/// limpe o cache do pager.
pub fn pager_clear_cache(p_pager: &mut Pager) {
    debug_assert!(!memdb(p_pager) || p_pager.temp_file != 0);
    if p_pager.temp_file == 0 {
        pager_reset(p_pager);
    }
}

/// Esta função é chamada quando o usuário invoca "PRAGMA wal_checkpoint",
/// "PRAGMA wal_blocking_checkpoint" ou chama as funções API sqlite3_wal_checkpoint()
/// ou wal_blocking_checkpoint().
///
/// O parâmetro e_mode é um dos SQLITE_CHECKPOINT_PASSIVE, FULL ou RESTART.
pub fn pager_checkpoint(
    p_pager: &mut Pager,
    db: Option<&Sqlite3Ref>,
    e_mode: i32,
    p_n_log: &mut i32,
    p_n_ckpt: &mut i32,
) -> i32 {
    let mut rc = SQLITE_OK;
    if p_pager.p_wal.is_none() && p_pager.journal_mode as i32 == PAGER_JOURNALMODE_WAL {
        // Isto só acontece quando um arquivo de banco de dados de zero bytes é aberto e
        // então "PRAGMA journal_mode=WAL" é executado e então sqlite3_wal_checkpoint()
        // é invocado sem transações intervindo. Precisamos iniciar
        // uma transação para inicializar p_wal. A declaração PRAGMA table_list é
        // usada para isso, pois ela inicia transações em cada arquivo de banco de dados,
        // incluindo todos os bancos de dados ATTACH. Isto parece caro para uma única
        // chamada sqlite3_wal_checkpoint(), mas acontece muito raramente.
        // https://sqlite.org/forum/forumpost/fd0f19d229156939
        if let Some(db_ref) = db {
            api::exec(db_ref, "PRAGMA table_list", None, None, None);
        }
    }
    if let Some(ref mut p_wal) = p_pager.p_wal {
        rc = wal_checkpoint(
            p_wal,
            db,
            e_mode,
            if e_mode == SQLITE_CHECKPOINT_PASSIVE {
                None
            } else {
                p_pager.x_busy_handler
            },
            p_pager.p_busy_handler_arg.as_deref(),
            p_pager.wal_sync_flags,
            p_pager.page_size,
            p_pager.p_tmp_space.as_deref(),
            p_n_log,
            p_n_ckpt,
        );
    }
    rc
}

/// Retorna o resultado da chamada a wal_callback para o WAL
/// associado ao pager (que também zera o contador do WAL, por isso &mut).
pub fn pager_wal_callback(p_pager: &mut Pager) -> i32 {
    wal_callback(p_pager.p_wal.as_mut())
}

/// Retorna verdadeiro se o VFS subjacente para o pager fornecido
/// suporta os primitivos necessários para logging antecipado de escrita.
pub fn pager_wal_supported(p_pager: &Pager) -> i32 {
    if let Some(ref fd) = p_pager.fd {
        if p_pager.no_lock != 0 {
            return 0;
        }
        let p_methods = fd.p_methods.as_ref();
        if p_pager.exclusive_mode != 0 {
            return 1;
        }
        if let Some(methods) = p_methods {
            if methods.i_version >= 2 && methods.x_shm_map.is_some() {
                return 1;
            }
        }
    }
    0
}

/// Tenta obter um lock exclusivo no arquivo de banco de dados. Se um lock
/// PENDING for obtido, libere-o imediatamente.
fn pager_exclusive_lock(p_pager: &mut Pager) -> i32 {
    debug_assert!(p_pager.e_lock as i32 >= SHARED_LOCK);
    let e_orig_lock = p_pager.e_lock;
    let rc = pager_lock_db(p_pager, EXCLUSIVE_LOCK);
    if rc != SQLITE_OK {
        // Se a tentativa de obter o lock exclusivo falhou, libere o
        // lock pendente que pode ter sido obtido.
        pager_unlock_db(p_pager, e_orig_lock as i32);
    }

    rc
}

/// Chama sqlite3WalOpen() para abrir o handle WAL. Se o pager estiver em
/// modo de locking exclusivo quando esta função for chamada, obtenha um
/// lock EXCLUSIVE no arquivo de banco de dados e use memória heap para
/// armazenar o wal-index. Caso contrário, use a memória compartilhada normal.
fn pager_open_wal_static(p_pager: &mut Pager) -> i32 {
    let mut rc = SQLITE_OK;

    debug_assert!(p_pager.p_wal.is_none() && p_pager.temp_file == 0);
    debug_assert!(
        p_pager.e_lock as i32 == SHARED_LOCK || p_pager.e_lock as i32 == EXCLUSIVE_LOCK
    );

    // Se o pager já está em modo exclusivo, o módulo WAL usará
    // memória heap para o wal-index em vez da implementação de memória
    // compartilhada do VFS. Obtenha o lock exclusivo agora, antes de
    // abrir o arquivo WAL, para garantir que isto é seguro.
    if p_pager.exclusive_mode != 0 {
        rc = pager_exclusive_lock(p_pager);
    }

    // Abra a conexão com o arquivo de log. Se esta operação falhar,
    // (por exemplo, devido a falha de malloc()), retorne um código de erro.
    if rc == SQLITE_OK {
        rc = wal_open(
            p_pager.p_vfs.as_ref(),
            p_pager.fd.as_ref(),
            &p_pager.z_wal,
            p_pager.exclusive_mode != 0,
            p_pager.journal_size_limit,
            &mut p_pager.p_wal,
        );
    }
    pager_fix_map_limit(p_pager);

    rc
}

/// O chamador deve estar mantendo um lock SHARED no arquivo de banco de dados
/// para chamar esta função.
///
/// Se o pager passado como primeiro argumento está aberto em um arquivo de banco
/// de dados real (não um arquivo temporário ou um banco de dados em memória), e
/// o arquivo WAL ainda não está aberto, faça uma tentativa de abri-lo agora.
/// Se bem-sucedido, retorne SQLITE_OK. Se um erro ocorrer ou o VFS usado pelo
/// pager não suportar os métodos xShmXXX(), retorne um código de erro.
/// *pbOpen não é modificado em qualquer caso.
///
/// Se o pager está aberto em um arquivo temporário (ou banco de dados em memória),
/// ou se o arquivo WAL já está aberto, defina *pbOpen para 1 e retorne SQLITE_OK
/// sem fazer nada.
pub fn pager_open_wal(p_pager: &mut Pager, p_b_open: Option<&mut i32>) -> i32 {
    let mut rc = SQLITE_OK;

    debug_assert!(assert_pager_state(p_pager));
    debug_assert!(p_pager.e_state as i32 == PAGER_OPEN || p_b_open.is_some());
    debug_assert!(p_pager.e_state as i32 == PAGER_READER || p_b_open.is_none());
    debug_assert!(p_b_open.as_ref().map_or(true, |b| **b == 0));
    debug_assert!(
        p_b_open.is_some() || (p_pager.temp_file == 0 && p_pager.p_wal.is_none())
    );

    if p_pager.temp_file == 0 && p_pager.p_wal.is_none() {
        if pager_wal_supported(p_pager) == 0 {
            return SQLITE_CANTOPEN;
        }

        // Feche qualquer arquivo de rollback journal previamente aberto
        os_close(&mut p_pager.jfd);

        rc = pager_open_wal_static(p_pager);
        if rc == SQLITE_OK {
            p_pager.journal_mode = PAGER_JOURNALMODE_WAL as u8;
            p_pager.e_state = PAGER_OPEN as u8;
        }
    } else {
        if let Some(pb_open) = p_b_open {
            *pb_open = 1;
        }
    }

    rc
}

/// Esta função é chamada para fechar a conexão com o arquivo de log antes
/// de mudar do modo WAL para modo de rollback.
///
/// Antes de fechar o arquivo de log, esta função tenta obter um
/// lock EXCLUSIVE no arquivo de banco de dados. Se isto não puder ser
/// obtido, um erro (SQLITE_BUSY) é retornado e a conexão de log não é
/// fechada. Se bem-sucedido, o lock EXCLUSIVE não é liberado antes de retornar.
pub fn pager_close_wal(p_pager: &mut Pager, db: Option<&Sqlite3Ref>) -> i32 {
    let mut rc = SQLITE_OK;

    debug_assert!(p_pager.journal_mode as i32 == PAGER_JOURNALMODE_WAL);

    // Se o arquivo de log não está aberto, mas existe no sistema de
    // arquivos, pode precisar ser feito checkpoint antes da conexão poder
    // mudar para modo de rollback. Abra-o agora para que isto possa acontecer.
    if p_pager.p_wal.is_none() {
        let mut logexists = 0;
        rc = pager_lock_db(p_pager, SHARED_LOCK);
        if rc == SQLITE_OK {
            rc = os_access(
                p_pager.p_vfs.as_ref(),
                &p_pager.z_wal,
                SQLITE_ACCESS_EXISTS,
                &mut logexists,
            );
        }
        if rc == SQLITE_OK && logexists != 0 {
            rc = pager_open_wal_static(p_pager);
        }
    }

    // Faça checkpoint e feche o log. Como um lock EXCLUSIVE é mantido no
    // arquivo de banco de dados, os arquivos de log e resumo de log serão
    // deletados.
    if rc == SQLITE_OK && p_pager.p_wal.is_some() {
        rc = pager_exclusive_lock(p_pager);
        if rc == SQLITE_OK {
            if let Some(mut p_wal) = p_pager.p_wal.take() {
                rc = wal_close(
                    &mut p_wal,
                    db,
                    p_pager.wal_sync_flags,
                    p_pager.page_size,
                    p_pager.p_tmp_space.as_deref(),
                );
                pager_fix_map_limit(p_pager);
                if rc != SQLITE_OK && p_pager.exclusive_mode == 0 {
                    pager_unlock_db(p_pager, SHARED_LOCK);
                }
            }
        }
    }
    rc
}

// As rotinas de SETLK_TIMEOUT, SNAPSHOT, ZIPVFS e SEH (pager_wal_write_lock,
// pager_wal_db, pager_snapshot_*, pager_wal_framesize, pager_wal_system_errno)
// não existem na compilação do Debian 13 e foram omitidas.

// Fim da parte 018.


// ---- part_019.rs ----

