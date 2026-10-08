// Mesclado das partes traduzidas de wal_c (as partes quebram funções grandes no meio).
#![allow(unused_imports, ambiguous_glob_reexports)]
use crate::prelude::*;

// ---- part_000.rs ----

// Arquivo: wal.c
//
// Implementação de write-ahead log (WAL) usado em "journal_mode=WAL".
//
// FORMATO DO ARQUIVO WAL
//
// Um arquivo WAL consiste de um cabeçalho seguido por zero ou mais "frames".
// Cada frame registra o conteúdo revisado de uma única página do arquivo de banco de dados.
// Todas as mudanças no banco são gravadas escrevendo frames no WAL. Transações são confirmadas
// quando um frame com marcador de commit é gravado. Um único WAL pode e normalmente grava
// múltiplas transações. Periodicamente, o conteúdo do WAL é transferido de volta para o arquivo
// de banco em uma operação chamada "checkpoint".
//
// Um único arquivo WAL pode ser usado múltiplas vezes. Em outras palavras, o WAL pode preencher
// com frames e depois ser checkpointed e depois novos frames podem sobrescrever os antigos.
// Um WAL sempre cresce do início em direção ao final. Checksums e contadores anexados a cada
// frame são usados para determinar quais frames dentro do WAL são válidos e quais são restos
// de checkpoints anteriores.
//
// O cabeçalho WAL tem tamanho de 32 bytes e consiste dos seguintes oito valores inteiros
// sem sinal de 32 bits em ordem big-endian:
//
//     0: Número mágico. 0x377f0682 ou 0x377f0683
//     4: Versão do formato de arquivo. Atualmente 3007000
//     8: Tamanho da página do banco de dados. Exemplo: 1024
//    12: Número de sequência de checkpoint
//    16: Salt-1, inteiro aleatório incrementado a cada checkpoint
//    20: Salt-2, inteiro aleatório diferente mudando a cada checkpoint
//    24: Checksum-1 (primeira parte do checksum dos primeiros 24 bytes do cabeçalho).
//    28: Checksum-2 (segunda parte do checksum dos primeiros 24 bytes do cabeçalho).
//
// Imediatamente seguindo o cabeçalho wal estão zero ou mais frames. Cada frame consiste
// de um cabeçalho de frame de 24 bytes seguido por bytes de dados de página. O cabeçalho de
// frame são seis valores inteiros sem sinal de 32 bits em ordem big-endian, como segue:
//
//     0: Número da página.
//     4: Para registros de commit, o tamanho da imagem de banco após o commit em páginas.
//        Para todos os outros registros, zero.
//     8: Salt-1 (copiado do cabeçalho)
//    12: Salt-2 (copiado do cabeçalho)
//    16: Checksum-1.
//    20: Checksum-2.
//
// Um frame é considerado válido se e somente se as seguintes condições são verdadeiras:
//
//    (1) Os valores salt-1 e salt-2 no cabeçalho de frame correspondem aos valores
//        salt no cabeçalho wal
//
//    (2) Os valores checksum nos 8 bytes finais do cabeçalho de frame correspondem
//        exatamente ao checksum calculado consecutivamente no cabeçalho WAL e os primeiros
//        8 bytes e conteúdo de todos os frames até e incluindo o frame atual.
//
// O checksum é calculado usando inteiros big-endian de 32 bits se o número mágico nos
// primeiros 4 bytes do WAL for 0x377f0683 e é calculado usando little-endian se o número
// mágico for 0x377f0682. Os valores checksum sempre são armazenados no cabeçalho de frame
// em formato big-endian independentemente de qual ordem de bytes é usada para calcular
// o checksum. O checksum é calculado interpretando a entrada como um número par de inteiros
// sem sinal de 32 bits: x[0] até x[N]. O algoritmo usado para o checksum é como segue:
//
//   for i from 0 to n-1 step 2:
//     s0 += x[i] + s1;
//     s1 += x[i+1] + s0;
//   endfor
//
// Observe que s0 e s1 são ambos checksums ponderados usando pesos fibonacci em ordem reversa
// (o maior peso fibonacci ocorre no primeiro elemento da sequência sendo somada.) O valor s1
// abrange todos os termos de 32 bits da sequência enquanto s0 omite o termo final.
//
// Em um checkpoint, o WAL é primeiro VFS.xSync-ed, depois o conteúdo válido do WAL é
// transferido para o banco, depois o banco é VFS.xSync-ed. As operações VFS.xSync funcionam
// como barreiras de escrita, todas as escritas iniciadas antes do xSync devem ser concluídas
// antes de qualquer escrita que inicia após o xSync começar.
//
// Após cada checkpoint, o valor salt-1 é incrementado e o valor salt-2 é aleatorizado.
// Isso evita que frames antigos e novos no WAL sejam considerados válidos ao mesmo tempo
// e sendo checkpointados juntos seguindo um crash.
//
// ALGORITMO DE LEITOR
//
// Para ler uma página do banco (chame-a de número de página P), um leitor primeiro verifica
// o WAL para ver se ele contém a página P. Se sim, então a última instância válida de página P
// que é seguida por um frame de commit ou é um frame de commit em si torna-se o valor lido.
// Se o WAL não contém cópias de página P que são válidas e que são um frame de commit ou
// são seguidas por um frame de commit, então página P é lida do arquivo de banco.
//
// Para iniciar uma transação de leitura, o leitor registra o índice do último frame válido
// no WAL. O leitor usa este valor "mxFrame" registrado para todas as operações de leitura
// subsequentes. Novas transações podem ser anexadas ao WAL, mas enquanto o leitor usar seu
// valor mxFrame original e ignorar o conteúdo recém-anexado, ele verá um snapshot consistente
// do banco em um único ponto no tempo. Essa técnica permite que múltiplos leitores concorrentes
// vejam diferentes versões do conteúdo do banco simultaneamente.
//
// O algoritmo de leitor nos parágrafos anteriores funciona corretamente, mas porque frames para
// página P podem aparecer em qualquer lugar dentro do WAL, o leitor tem que varrer o WAL inteiro
// procurando por frames de página P. Se o WAL é grande (múltiplos megabytes é típico) aquela
// varredura pode ser lenta e o desempenho de leitura sofre. Para superar esse problema, uma
// estrutura de dados separada chamada wal-index é mantida para acelerar a busca por frames de
// uma página particular.
//
// FORMATO DO WAL-INDEX
//
// Conceitualmente, o wal-index é memória compartilhada, embora implementações VFS possam
// escolher implementar o wal-index usando um arquivo mmapped. Porque o wal-index é memória
// compartilhada, SQLite não suporta journal_mode=WAL em um sistema de arquivos de rede.
// Todos os usuários do banco devem ser capazes de compartilhar memória.
//
// Na implementação padrão unix e windows, o wal-index é um arquivo mmapped cujo nome é o nome
// do banco com sufixo "-shm" adicionado. Por essa razão, o wal-index às vezes é chamado de
// arquivo "shm".
//
// O wal-index é transitório. Após um crash, o wal-index pode (e deve) ser reconstruído do
// arquivo WAL original. De fato, o VFS é necessário para truncar ou zerar o cabeçalho do
// wal-index quando a última conexão com ele fecha. Porque o wal-index é transitório, ele pode
// usar um formato específico da arquitetura; ele não precisa ser multiplataforma. Logo, diferente
// dos formatos de arquivo de banco e WAL que armazenam todos os valores como big endian, o
// wal-index pode armazenar valores multi-byte na ordem de bytes nativa do computador hospedeiro.
//
// O propósito do wal-index é responder essa pergunta rapidamente: Dado um número de página P
// e um índice máximo de frame M, retorne o índice do último frame no wal antes do frame M para
// página P no WAL, ou retorne NULL se não houver frames para página P no WAL anterior a M.
//
// O wal-index consiste de uma região de cabeçalho, seguida por um ou mais blocos de índice.
//
// O cabeçalho wal-index contém o número total de frames dentro do WAL no campo mxFrame.
//
// Cada bloco de índice exceto pelo primeiro contém informação sobre HASHTABLE_NPAGE frames.
// O primeiro bloco de índice contém informação sobre HASHTABLE_NPAGE_ONE frames. Os valores de
// HASHTABLE_NPAGE_ONE e HASHTABLE_NPAGE são selecionados de modo que junto o cabeçalho
// wal-index e primeiro bloco de índice tenham o mesmo tamanho que todos os outros blocos de
// índice no wal-index. Os valores são:
//
//   HASHTABLE_NPAGE      4096
//   HASHTABLE_NPAGE_ONE  4062
//
// Cada bloco de índice contém duas seções, um mapeamento de página que contém o número de
// página de banco associado com cada frame wal, e uma tabela hash que permite leitores
// consultar um bloco de índice para um número de página específico. O mapeamento de página
// é um array de HASHTABLE_NPAGE (ou HASHTABLE_NPAGE_ONE para o primeiro bloco de índice)
// números de página de 32 bits. A primeira entrada no primeiro bloco de índice contém o número
// de página de banco correspondente ao primeiro frame no arquivo WAL. A primeira entrada no
// segundo bloco de índice no arquivo WAL corresponde ao (HASHTABLE_NPAGE_ONE+1)ésimo frame no
// log, e assim por diante.
//
// O último bloco de índice em um wal-index usualmente contém menos do que o complemento completo
// de HASHTABLE_NPAGE (ou HASHTABLE_NPAGE_ONE) números de página, dependendo do conteúdo do
// arquivo WAL. Isso não muda o tamanho alocado do array de mapeamento de página, o array de
// mapeamento de página meramente contém entradas não usadas.
//
// Mesmo sem usar a tabela hash, o último frame para página P pode ser encontrado varrendo as
// seções de mapeamento de página de cada bloco de índice começando com o último bloco de índice
// e movendo-se em direção ao primeiro, e dentro de cada bloco de índice, começando no final e
// movendo-se em direção ao começo. A primeira entrada que é igual a P corresponde ao frame
// mantendo o conteúdo para aquela página.
//
// A tabela hash consiste de HASHTABLE_NSLOT inteiros sem sinal de 16 bits.
// HASHTABLE_NSLOT = 2*HASHTABLE_NPAGE, e há uma entrada na tabela hash para cada número de
// página na seção de mapeamento, de modo que a tabela hash nunca está mais da metade cheia.
// O número esperado de colisões antes de encontrar um match é 1. Cada entrada da tabela hash
// é um índice de 1 base de uma entrada na seção de mapeamento do mesmo bloco de índice.
// Deixe K ser o índice de 1 base da maior entrada na seção de mapeamento. (Para blocos de
// índice outros que o último, K será sempre exatamente HASHTABLE_NPAGE (4096) e para o último
// bloco de índice K será (mxFrame%HASHTABLE_NPAGE).) Slots não usados da tabela hash contêm
// um valor de 0.
//
// Para procurar por página P na tabela hash, primeiro compute uma chave de hash iKey em
// P como segue:
//
//      iKey = (P * 383) % HASHTABLE_NSLOT
//
// Depois comece varrendo entradas da tabela hash, começando com iKey (enrolando para o começo
// quando o final da tabela hash é alcançado) até um slot de hash não usado ser encontrado.
// Deixe o primeiro slot não usado estar no índice iUnused. (iUnused pode ser menor que iKey se
// houve enrolamento.) Porque a tabela hash nunca está mais da metade cheia, a busca é garantida
// de eventualmente acertar uma entrada não usada. Deixe iMax ser o valor entre iKey e iUnused,
// mais próximo de iUnused, onde aHash[iMax]==P. Se não houver entrada iMax (se não existir
// nenhum slot de hash tal que aHash[i]==p) então página P não está no bloco de índice atual.
// Caso contrário a entrada de mapeamento iMax-ésima do bloco de índice atual corresponde à
// última entrada que referencia página P.
//
// Uma busca em hash começa com o último bloco de índice e move-se em direção ao primeiro bloco
// de índice, procurando por entradas correspondentes à página P. Em média, apenas dois ou três
// slots em cada bloco de índice precisam ser examinados a fim de encontrar a última entrada
// para página P ou estabelecer que nenhuma tal entrada existe no bloco. Cada bloco de índice
// segura sobre 4000 entradas. De modo que dois ou três blocos de índice são suficientes para
// cobrir um arquivo WAL típico de 10 megabytes, assumindo páginas de 1K. 8 ou 10 comparações
// (em média) são suficientes para localizar um frame no WAL ou estabelecer que o frame não
// existe no WAL. Isso é muito mais rápido que varrer o WAL inteiro de 10MB.
//
// Observe que entradas são adicionadas em ordem de K crescente. Logo, um leitor pode estar
// usando algum valor K0 e um segundo leitor que iniciou em um tempo posterior (após transações
// adicionais serem adicionadas ao WAL e ao wal-index) pode estar usando um valor diferente K1,
// onde K1>K0. Ambos os leitores podem usar a mesma tabela hash e seção de mapeamento para
// obter o resultado correto. Pode haver entradas na tabela hash com K>K0 mas para o primeiro
// leitor, aquelas entradas aparecerão ser slots não usados na tabela hash e de modo que o
// primeiro leitor terá uma resposta como se nenhum valores maiores que K0 tivessem sido
// inseridos na tabela hash em primeiro lugar, o que é o que o leitor um quer. Entretanto,
// o segundo leitor usando K1 verá valores adicionais que foram inseridos depois, que é
// exatamente o que o leitor dois quer.
//
// Quando um rollback ocorre, o valor de K é diminuído. Entradas de tabela hash que correspondem
// a frames maiores que o novo valor K são removidas da tabela hash neste ponto.

// Versões máximas (e únicas) do formato wal e wal-index que podem ser interpretadas por
// esta versão do SQLite.
//
// Se um cliente começa a recuperar um arquivo WAL e descobre que (a) os valores checksum no
// cabeçalho wal estão corretos e (b) o campo de versão não é WAL_MAX_VERSION, recuperação
// falha e SQLite retorna SQLITE_CANTOPEN.
//
// Similarmente, se um cliente lê com sucesso um cabeçalho wal-index (isto é, o teste checksum
// é bem sucedido) e descobre que o campo de versão não é WALINDEX_MAX_VERSION, então nenhuma
// transação de leitura é aberta e SQLite retorna SQLITE_CANTOPEN.

pub const WAL_MAX_VERSION: u32 = 3007000;
pub const WALINDEX_MAX_VERSION: u32 = 3007000;

// Números de índice para vários bytes de lock. WAL_NREADER é o número de locks de leitura
// disponíveis e deve ser pelo menos 3. O padrão é SQLITE_SHM_NLOCK==8 e WAL_NREADER==5.
//
// Tecnicamente, os vários VFSes são livres para implementar esses locks do jeito que acharem
// melhor. Porém, compatibilidade é encorajada de modo que VFSes possam interoperar. A
// implementação padrão usada em unix e windows é para o número de índice indicar um offset
// de byte no array WalCkptInfo.a_lock[] no cabeçalho wal-index. Em outras palavras, todos
// os locks estão no arquivo shm. A constante WALINDEX_LOCK_OFFSET (que deve ser 120) é a
// localização no arquivo shm para o primeiro byte de lock.

pub const WAL_WRITE_LOCK: u32 = 0;
pub const WAL_ALL_BUT_WRITE: u32 = 1;
pub const WAL_CKPT_LOCK: u32 = 1;

#[inline]
pub fn wal_read_lock(i: u32) -> u32 {
    3 + i
}

// WAL_NREADER é SQLITE_SHM_NLOCK - 3, isto é, 8 - 3 = 5 na configuração do Debian 13
pub const WAL_NREADER: usize = 5;

// Declarações de tipo para estruturas do WAL opacas (neste arquivo, o cabeçalho wal.h não
// expõe essas definições).
//
// WalIndexHdr, WalIterator, WalCkptInfo são definidas aqui em wal.c.

// Objeto que mantém uma cópia do conteúdo de cabeçalho wal-index.
//
// O cabeçalho real no wal-index consiste de duas cópias deste objeto seguidas por uma
// instância do objeto WalCkptInfo. Para todas as versões do SQLite através de 3.10.0 e
// provavelmente além, os bytes de lock (WalCkptInfo.a_lock) começam no offset 120 e o
// tamanho total do cabeçalho é 136 bytes.
//
// O valor sz_page pode ser qualquer potência de 2 entre 512 e 32768, inclusive.
// Ou pode ser 1 para representar uma página de 65536 bytes. Este último caso foi adicionado
// em 3.7.1 quando suporte para páginas de 64K foi adicionado.

#[derive(Clone, Debug, Default)]
pub struct WalIndexHdr {
    /// Versão wal-index
    pub i_version: u32,
    /// Campo não usado (padding)
    pub unused: u32,
    /// Contador incrementado a cada transação
    pub i_change: u32,
    /// 1 quando inicializado
    pub is_init: u8,
    /// Verdadeiro se checksums no WAL são big-endian
    pub big_end_cksum: u8,
    /// Tamanho de página de banco em bytes. 1==64K
    pub sz_page: u16,
    /// Índice do último frame válido no WAL
    pub mx_frame: u32,
    /// Tamanho do banco em páginas
    pub n_page: u32,
    /// Checksum do último frame no log
    pub a_frame_cksum: [u32; 2],
    /// Dois valores salt copiados do cabeçalho WAL
    pub a_salt: [u32; 2],
    /// Checksum sobre todos os campos anteriores
    pub a_cksum: [u32; 2],
}

// Uma cópia do objeto seguinte ocorre no wal-index imediatamente seguindo a segunda cópia
// de WalIndexHdr. Este objeto armazena informação usada por checkpoint.
//
// n_backfill é o número de frames no WAL que foram escritos de volta no banco.
// (Chamamos o ato de mover conteúdo de WAL para banco de "backfilling".) O número
// n_backfill nunca é maior que WalIndexHdr.mx_frame. n_backfill só pode ser aumentado
// por threads mantendo o lock WAL_CKPT_LOCK (que inclui uma thread de recuperação).
// Porém, uma thread WAL_WRITE_LOCK pode mover o valor de n_backfill de mx_frame de volta
// para zero quando o WAL é resetado.
//
// n_backfill_attempted é o maior valor de n_backfill que um checkpoint tentou alcançar.
// Normalmente n_backfill==n_backfill_attempted, porém n_backfill_attempted é definido antes
// de qualquer backfill ser feito e n_backfill é apenas definido após todo backfill completar.
// De modo que se um checkpoint faz crash, n_backfill_attempted pode ser maior que n_backfill.
// O WalIndexHdr.mx_frame nunca deve ser menor que n_backfill_attempted.
//
// O campo a_lock[] é um conjunto de bytes usados para locking. Esses bytes nunca devem ser
// lidos ou escritos.
//
// Há uma entrada em a_read_mark[] para cada lock de leitura. Se um leitor segura lock de
// leitura K, então o valor em a_read_mark[K] não é maior que o mx_frame para aquele leitor.
// O valor READMARK_NOT_USED (0xffffffff) para qualquer a_read_mark[] significa que entrada
// é não usada. a_read_mark[0] é um caso especial; seu valor nunca é usado e ele existe como
// um espaço reservado para evitar ter que fazer offset de índices a_read_mark[] por um.
// Leitores segurando WAL_READ_LOCK(0) sempre ignoram o WAL inteiro e leem todo conteúdo
// direto do banco.
//
// O valor de a_read_mark[K] pode apenas ser alterado por uma thread que está segurando um
// lock exclusivo em WAL_READ_LOCK(K). Assim, o valor de a_read_mark[K] não pode ser alterado
// enquanto há um leitor usando aquela marcação já que o leitor estará segurando um lock
// compartilhado em WAL_READ_LOCK(K).
//
// O checkpointer pode apenas transferir frames do WAL para banco onde os números de frame
// são menores que ou iguais a cada a_read_mark[] que está em uso (isto é, cada a_read_mark[j]
// para o qual há um WAL_READ_LOCK(j) correspondente). Novos leitores (usualmente) escolhem
// o a_read_mark[] com o maior valor e aumentarão um a_read_mark[] não usado para mx_frame
// se ainda não houver um a_read_mark[] igual a mx_frame. A exceção à sentença anterior é
// quando n_backfill é igual a mx_frame (significando que tudo no WAL foi backfilled no banco)
// então novos leitores escolherão a_read_mark[0] que tem valor 0 e logo tal leitor terá
// todo o conteúdo direto do arquivo banco e ignorará o WAL.
//
// Escritores normalmente anexam novos frames ao final do WAL. Porém, se n_backfill é igual
// a mx_frame (significando que todo conteúdo WAL foi escrito de volta no banco) e se nenhum
// leitor está usando o WAL (em outras palavras, se não há WAL_READ_LOCK(i) onde i>0) então
// o escritor primeiro fará um "reset" do WAL de volta ao começo e começará escrevendo novo
// conteúdo começando no frame 1.
//
// Assumimos que loads de 32-bit são atômicos e de modo que nenhum lock é necessário a fim
// de ler de qualquer entrada a_read_mark[].

#[derive(Clone, Debug, Default)]
pub struct WalCkptInfo {
    /// Número de frames WAL backfilled no banco
    pub n_backfill: u32,
    /// Marcas de leitor
    pub a_read_mark: [u32; 5],
    /// Espaço reservado para locks
    pub a_lock: [u8; 8],
    /// Frames WAL talvez escritos, ou talvez não
    pub n_backfill_attempted: u32,
    /// Disponível para futuras melhorias
    pub not_used_0: u32,
}


// ---- part_001.rs ----

/// Marca de leitura não utilizada (0xffffffff).
pub const READMARK_NOT_USED: u32 = 0xffffffff;

// Esquema do cabeçalho completo de 136 bytes do arquivo wal-index (arquivo -shm):
//
//    0: primeira cópia do WalIndexHdr (48 bytes)
//   48: segunda cópia do WalIndexHdr (48 bytes)
//   96: n_backfill
//  100: 5 marcas de leitura (a_read_mark)
//  120: 8 bytes de lock (Write, Ckpt, Rcvr, Rd0, Read1, Read2, Rd3, Rd4)
//  128: n_backfill_attempted
//  132: not_used_0 (padding)

/// Um bloco de WALINDEX_LOCK_RESERVED bytes começando em WALINDEX_LOCK_OFFSET é reservado
/// para locks. Como alguns sistemas só suportam locks de arquivo obrigatórios, não lemos nem
/// escrevemos dados na região do arquivo em que os locks são aplicados.
/// Equivale a `sizeof(WalIndexHdr)*2 + offsetof(WalCkptInfo, aLock)` do C (48*2+24).
pub const WALINDEX_LOCK_OFFSET: usize = 120;

/// Equivale a `sizeof(WalIndexHdr)*2 + sizeof(WalCkptInfo)` do C (48*2+40).
pub const WALINDEX_HDR_SIZE: usize = 136;

/// Tamanho do cabeçalho antes de cada frame no wal.
pub const WAL_FRAME_HDRSIZE: usize = 24;

/// Tamanho do cabeçalho do write-ahead log, incluindo o checksum.
pub const WAL_HDRSIZE: usize = 32;

/// Valor mágico do WAL. Este valor, ou o mesmo valor com o bit menos significativo também
/// ligado (WAL_MAGIC | 0x00000001), é armazenado em formato big-endian de 32 bits nos
/// primeiros 4 bytes de um arquivo WAL.
///
/// Se o bit menos significativo estiver ligado, os checksums de cada frame dentro do arquivo
/// WAL são calculados tratando todos os dados como um array de palavras big-endian de 32 bits.
/// Caso contrário, são calculados interpretando todos os dados como palavras little-endian
/// de 32 bits.
pub const WAL_MAGIC: u32 = 0x377f0682;

/// Devolve o deslocamento do frame `i_frame` no arquivo de write-ahead log, assumindo
/// páginas de banco de `sz_page` bytes. O deslocamento devolvido é o do início do
/// cabeçalho do frame.
#[inline]
pub fn wal_frame_offset(i_frame: u32, sz_page: u32) -> i64 {
    // Em C, (iFrame)-1 é aritmética de u32 e só depois vira i64.
    WAL_HDRSIZE as i64
        + (i_frame.wrapping_sub(1) as i64) * ((sz_page.wrapping_add(WAL_FRAME_HDRSIZE as u32)) as i64)
}

/// Um arquivo de write-ahead log aberto é representado por uma instância deste objeto.
///
/// Modelo de memória: o wal-index (`apWiData`) vira um vetor de páginas de `u32`; os VFS e os
/// arquivos seguem o módulo `os` (`Sqlite3File` com `p_methods`).
#[derive(Default)]
pub struct Wal {
    /// O VFS usado para criar p_db_fd.
    pub p_vfs: Option<Rc<dyn Vfs>>,
    /// Identificador de arquivo do banco de dados (compartilhado com o pager).
    pub p_db_fd: Option<Rc<RefCell<Sqlite3File>>>,
    /// Identificador de arquivo do WAL.
    pub p_wal_fd: Option<Box<Sqlite3File>>,
    /// Valor a passar ao callback de log (ou 0).
    pub i_callback: u32,
    /// Truncar o WAL para este tamanho após o reset.
    pub mx_wal_size: i64,
    /// Tamanho do array ap_wi_data.
    pub n_wi_data: i32,
    /// Tamanho do primeiro bloco escrito no arquivo WAL.
    pub sz_first_block: i32,
    /// Conteúdo do wal-index em memória (uma entrada por página de WALINDEX_PGSZ bytes).
    pub ap_wi_data: Vec<Option<Vec<u32>>>,
    /// Visão tipada da região de cabeçalho da página 0 do wal-index: as duas cópias de
    /// WalIndexHdr (índice 0 e 1). Lida por `wal_index_hdr` e escrita por `wal_index_write_hdr`.
    pub wi_hdr: [WalIndexHdr; 2],
    /// Visão tipada do WalCkptInfo que segue as duas cópias de WalIndexHdr no wal-index.
    pub wi_ckpt: WalCkptInfo,
    /// Tamanho da página do banco de dados.
    pub sz_page: u32,
    /// Qual lock de leitura está sendo mantido. -1 para nenhum.
    pub read_lock: i16,
    /// Flags usados para sincronizar escritas do cabeçalho.
    pub sync_flags: u8,
    /// Não zero se a conexão está em modo exclusivo.
    pub exclusive_mode: u8,
    /// Verdadeiro se em uma transação de escrita.
    pub write_lock: u8,
    /// Verdadeiro se mantendo um lock de checkpoint.
    pub ckpt_lock: u8,
    /// WAL_RDWR, WAL_RDONLY ou WAL_SHM_RDONLY.
    pub read_only: u8,
    /// Verdadeiro para truncar o arquivo WAL no commit.
    pub truncate_on_commit: u8,
    /// Fazer fsync do cabeçalho do WAL se verdadeiro.
    pub sync_header: u8,
    /// Completar transações até o próximo limite de setor.
    pub pad_to_sector_boundary: u8,
    /// Conteúdo SHM é somente leitura e não confiável.
    pub b_shm_unreliable: u8,
    /// Cabeçalho do wal-index da transação atual.
    pub hdr: WalIndexHdr,
    /// Ignorar frames do wal antes deste.
    pub min_frame: u32,
    /// No commit, recalcular checksums a partir daqui.
    pub i_re_cksum: u32,
    /// Nome do arquivo WAL.
    pub z_wal_name: Vec<u8>,
    /// Contador de sequência de checkpoint no cabeçalho do wal.
    pub n_ckpt: u32,
    /// Começar a transação aqui se não for None (SQLITE_ENABLE_SNAPSHOT).
    pub p_snapshot: Option<Box<WalIndexHdr>>,
}

/// Valores candidatos para Wal.exclusive_mode.
pub const WAL_NORMAL_MODE: u8 = 0;
pub const WAL_EXCLUSIVE_MODE: u8 = 1;
pub const WAL_HEAPMEMORY_MODE: u8 = 2;

/// Valores possíveis para Wal.read_only.
/// Conexão normal de leitura e escrita.
pub const WAL_RDWR: u8 = 0;
/// O arquivo WAL é somente leitura.
pub const WAL_RDONLY: u8 = 1;
/// O arquivo SHM é somente leitura.
pub const WAL_SHM_RDONLY: u8 = 2;

/// Cada página do mapeamento do wal-index contém uma tabela hash feita de um array de
/// HASHTABLE_NSLOT elementos deste tipo.
pub type HtSlot = u16;

/// Apelido com o nome do C (`ht_slot`), usado por outras partes da tradução.
#[allow(non_camel_case_types)]
pub type ht_slot = HtSlot;

/// Iterador que percorre todos os frames do WAL em ordem de página do banco. Quando dois ou
/// mais frames correspondem à mesma página, o iterador visita só o frame escrito mais
/// recentemente no WAL (o de maior índice).
///
/// Os internos desta estrutura só são acessados por:
///
///   wal_iterator_init() - cria um novo iterador,
///   wal_iterator_next() - avança um iterador,
///   wal_iterator_free() - libera um iterador.
///
/// Esta funcionalidade é usada pelo código de checkpoint (ver wal_checkpoint()).
#[derive(Default)]
pub struct WalIterator {
    /// Último resultado devolvido pelo iterador.
    pub i_prior: u32,
    /// Número de entradas em a_segment[].
    pub n_segment: i32,
    /// Um para cada página de 32KB do wal-index.
    pub a_segment: Vec<WalSegment>,
}

/// Segmento do iterador (a `struct WalSegment` aninhada em `WalIterator` no C).
#[derive(Default)]
pub struct WalSegment {
    /// Próximo slot em a_index[] ainda não devolvido.
    pub i_next: i32,
    /// i0, i1, i2... tais que a_pgno[iN] ascende.
    pub a_index: Vec<HtSlot>,
    /// Array de números de página.
    pub a_pgno: Vec<u32>,
    /// Número de entradas em a_pgno[] e a_index[].
    pub n_entry: i32,
    /// Número do frame associado a a_pgno[0].
    pub i_zero: i32,
}

/// Parâmetros das tabelas hash no arquivo wal-index. Há uma tabela hash após cada
/// HASHTABLE_NPAGE números de página no wal-index.
///
/// Alterar qualquer uma destas constantes altera o formato do wal-index e cria
/// incompatibilidades.
/// Deve ser potência de 2.
pub const HASHTABLE_NPAGE: usize = 4096;
/// Deve ser primo.
pub const HASHTABLE_HASH_1: u32 = 383;
/// Deve ser potência de 2.
pub const HASHTABLE_NSLOT: usize = HASHTABLE_NPAGE * 2;

/// O bloco de números de página associado à primeira tabela hash de um wal-index é menor
/// que o usual. Assim há uma tabela hash completa em cada página alinhada de 32KB do
/// wal-index.
pub const HASHTABLE_NPAGE_ONE: usize = HASHTABLE_NPAGE - (WALINDEX_HDR_SIZE / 4);

/// O wal-index é dividido em páginas de WALINDEX_PGSZ bytes cada.
pub const WALINDEX_PGSZ: usize = 2 * HASHTABLE_NSLOT + HASHTABLE_NPAGE * 4;

/// Obtém a página `i_page` do wal-index. O wal-index é quebrado em páginas de WALINDEX_PGSZ
/// bytes, numeradas a partir de zero.
///
/// Se o wal-index for menor que `i_page` páginas, seu tamanho pode ser aumentado, mas só se
/// for seguro. É seguro ampliar o wal-index se `write_lock` for verdadeiro ou
/// `exclusive_mode == WAL_HEAPMEMORY_MODE`.
///
/// Três cenários de resultado:
///
///   (1)  rc==SQLITE_OK    e a página é a pedida
///   (2)  rc>=SQLITE_ERROR e a página é None
///   (3)  rc==SQLITE_OK    e a página é None  // só se i_page==0
///
/// O cenário (3) só ocorre quando `write_lock` é falso e `i_page==0`.
pub fn wal_index_page_realloc(p_wal: &mut Wal, i_page: i32) -> (i32, Option<Vec<u32>>) {
    let mut rc = SQLITE_OK;

    // Ampliar o array p_wal.ap_wi_data[] se necessário.
    if p_wal.n_wi_data <= i_page {
        p_wal.ap_wi_data.resize(i_page as usize + 1, None);
        p_wal.n_wi_data = i_page + 1;
    }

    // Pedir ao VFS um ponteiro para a página necessária.
    debug_assert!(p_wal.ap_wi_data[i_page as usize].is_none());
    if p_wal.exclusive_mode == WAL_HEAPMEMORY_MODE {
        p_wal.ap_wi_data[i_page as usize] = Some(vec![0u32; WALINDEX_PGSZ / 4]);
    } else {
        let mut p_shm: Option<Rc<RefCell<Vec<u8>>>> = None;
        let b_extend = p_wal.write_lock as i32;
        rc = os_shm_map(
            &mut p_wal.p_db_fd.as_ref().unwrap().borrow_mut(),
            i_page,
            WALINDEX_PGSZ as i32,
            b_extend,
            &mut p_shm,
        );
        debug_assert!(p_shm.is_some() || rc != SQLITE_OK || (p_wal.write_lock == 0 && i_page == 0));
        if let Some(shm) = p_shm {
            // A memória compartilhada do VFS é em bytes na ordem nativa; o wal-index em
            // memória guarda palavras u32 nativas.
            let bytes = shm.borrow();
            p_wal.ap_wi_data[i_page as usize] = Some(
                bytes
                    .chunks_exact(4)
                    .map(|c| u32::from_ne_bytes([c[0], c[1], c[2], c[3]]))
                    .collect(),
            );
        }
        if rc == SQLITE_OK {
            if i_page > 0 && fault_sim(600) != 0 {
                rc = SQLITE_NOMEM;
            }
        } else if (rc & 0xff) == SQLITE_READONLY {
            p_wal.read_only |= WAL_SHM_RDONLY;
            if rc == SQLITE_READONLY {
                rc = SQLITE_OK;
            }
        }
    }

    let p_page = p_wal.ap_wi_data[i_page as usize].clone();
    debug_assert!(i_page == 0 || p_page.is_some() || rc != SQLITE_OK);
    (rc, p_page)
}


// ---- part_002.rs ----

/// Obtém a página `i_page` do wal-index. Se ainda não estiver mapeada, chama
/// `wal_index_page_realloc`. Depois da chamada a página fica em `ap_wi_data[i_page]`
/// (`None` no erro).
pub fn wal_index_page(p_wal: &mut Wal, i_page: i32) -> i32 {
    if p_wal.n_wi_data <= i_page || p_wal.ap_wi_data[i_page as usize].is_none() {
        let (rc, _pp_page) = wal_index_page_realloc(p_wal, i_page);
        return rc;
    }
    SQLITE_OK
}

/// Devolve a estrutura WalCkptInfo do wal-index.
pub fn wal_ckpt_info(p_wal: &mut Wal) -> &mut WalCkptInfo {
    debug_assert!(p_wal.n_wi_data > 0 && p_wal.ap_wi_data[0].is_some());
    &mut p_wal.wi_ckpt
}

/// Devolve a estrutura WalIndexHdr do wal-index (a primeira cópia).
pub fn wal_index_hdr(p_wal: &Wal) -> &WalIndexHdr {
    debug_assert!(p_wal.n_wi_data > 0 && p_wal.ap_wi_data[0].is_some());
    &p_wal.wi_hdr[0]
}

/// O argumento deve ser u32. Numa arquitetura little-endian, devolve o valor u32 que resulta
/// de interpretar os 4 bytes como um valor big-endian. Numa big-endian, devolve o valor que
/// seria produzido interpretando os 4 bytes da entrada como um inteiro little-endian.
#[inline]
pub fn byteswap32(x: u32) -> u32 {
    ((x & 0x000000FF) << 24)
        .wrapping_add((x & 0x0000FF00) << 8)
        .wrapping_add((x & 0x00FF0000) >> 8)
        .wrapping_add((x & 0xFF000000) >> 24)
}

/// Gera ou estende um checksum de 8 bytes com base nos dados do array `a` e nos valores
/// iniciais de `a_in[0]` e `a_in[1]` (ou 0 e 0 se `a_in` for None).
///
/// O checksum é escrito de volta em `a_out` antes de retornar.
///
/// `n_byte` deve ser um múltiplo positivo de 8.
pub fn wal_checksum_bytes(
    native_cksum: i32,
    a: &[u8],
    n_byte: i32,
    a_in: Option<&[u32; 2]>,
    a_out: &mut [u32; 2],
) {
    let (mut s1, mut s2): (u32, u32) = match a_in {
        Some(v) => (v[0], v[1]),
        None => (0, 0),
    };
    let n_byte = n_byte as usize;
    let word = |i: usize| -> u32 { u32::from_ne_bytes([a[i * 4], a[i * 4 + 1], a[i * 4 + 2], a[i * 4 + 3]]) };
    let n_words = n_byte / 4;
    let mut i: usize = 0;

    debug_assert!(n_byte >= 8);
    debug_assert!((n_byte & 0x00000007) == 0);
    debug_assert!(n_byte <= 65536);
    debug_assert!(n_byte % 4 == 0);

    if native_cksum == 0 {
        loop {
            s1 = s1.wrapping_add(byteswap32(word(i))).wrapping_add(s2);
            s2 = s2.wrapping_add(byteswap32(word(i + 1))).wrapping_add(s1);
            i += 2;
            if i >= n_words {
                break;
            }
        }
    } else if n_byte % 64 == 0 {
        loop {
            // Oito pares por volta, como o laço desenrolado do C.
            for _ in 0..8 {
                s1 = s1.wrapping_add(word(i)).wrapping_add(s2);
                s2 = s2.wrapping_add(word(i + 1)).wrapping_add(s1);
                i += 2;
            }
            if i >= n_words {
                break;
            }
        }
    } else {
        loop {
            s1 = s1.wrapping_add(word(i)).wrapping_add(s2);
            s2 = s2.wrapping_add(word(i + 1)).wrapping_add(s1);
            i += 2;
            if i >= n_words {
                break;
            }
        }
    }
    debug_assert!(i == n_words);

    a_out[0] = s1;
    a_out[1] = s2;
}

/// Se há possibilidade de acesso concorrente ao arquivo SHM por várias threads e/ou
/// processos, faz uma barreira de memória.
pub fn wal_shm_barrier(p_wal: &mut Wal) {
    if p_wal.exclusive_mode != WAL_HEAPMEMORY_MODE {
        os_shm_barrier(&mut p_wal.p_db_fd.as_ref().unwrap().borrow_mut());
    }
}

/// Serializa os primeiros 40 bytes de um WalIndexHdr (tudo antes de a_cksum) na ordem de
/// bytes nativa, como `(u8*)&pWal->hdr` no C.
fn wal_index_hdr_prefix_bytes(h: &WalIndexHdr) -> [u8; 40] {
    let mut b = [0u8; 40];
    b[0..4].copy_from_slice(&h.i_version.to_ne_bytes());
    b[4..8].copy_from_slice(&h.unused.to_ne_bytes());
    b[8..12].copy_from_slice(&h.i_change.to_ne_bytes());
    b[12] = h.is_init;
    b[13] = h.big_end_cksum;
    b[14..16].copy_from_slice(&h.sz_page.to_ne_bytes());
    b[16..20].copy_from_slice(&h.mx_frame.to_ne_bytes());
    b[20..24].copy_from_slice(&h.n_page.to_ne_bytes());
    b[24..28].copy_from_slice(&h.a_frame_cksum[0].to_ne_bytes());
    b[28..32].copy_from_slice(&h.a_frame_cksum[1].to_ne_bytes());
    b[32..36].copy_from_slice(&h.a_salt[0].to_ne_bytes());
    b[36..40].copy_from_slice(&h.a_salt[1].to_ne_bytes());
    b
}

/// Escreve a informação de cabeçalho de `p_wal.hdr` no wal-index.
///
/// O checksum de `p_wal.hdr` é atualizado antes da escrita.
pub fn wal_index_write_hdr(p_wal: &mut Wal) {
    const N_CKSUM: i32 = 40;

    debug_assert!(p_wal.n_wi_data > 0 && p_wal.ap_wi_data[0].is_some());
    debug_assert!(p_wal.write_lock != 0);
    p_wal.hdr.is_init = 1;
    p_wal.hdr.i_version = WALINDEX_MAX_VERSION;
    let a_bytes = wal_index_hdr_prefix_bytes(&p_wal.hdr);
    let mut a_out = [0u32; 2];
    wal_checksum_bytes(1, &a_bytes, N_CKSUM, None, &mut a_out);
    p_wal.hdr.a_cksum = a_out;
    p_wal.wi_hdr[1] = p_wal.hdr.clone();
    wal_shm_barrier(p_wal);
    p_wal.wi_hdr[0] = p_wal.hdr.clone();
}

/// Codifica o cabeçalho de um único frame e o escreve em um buffer fornecido pelo chamador.
/// Um cabeçalho de frame é feito de uma série de inteiros big-endian de 4 bytes:
///
///     0: Número da página.
///     4: Para registros de commit, o tamanho da imagem do banco em páginas depois do commit.
///        Para todos os outros registros, zero.
///     8: Salt-1 (copiado do cabeçalho do wal)
///    12: Salt-2 (copiado do cabeçalho do wal)
///    16: Checksum-1.
///    20: Checksum-2.
pub fn wal_encode_frame(p_wal: &mut Wal, i_page: u32, n_truncate: u32, a_data: &[u8], a_frame: &mut [u8]) {
    let mut a_cksum = p_wal.hdr.a_frame_cksum;
    debug_assert!(WAL_FRAME_HDRSIZE == 24);
    put4byte(&mut a_frame[0..4], i_page);
    put4byte(&mut a_frame[4..8], n_truncate);
    if p_wal.i_re_cksum == 0 {
        a_frame[8..12].copy_from_slice(&p_wal.hdr.a_salt[0].to_ne_bytes());
        a_frame[12..16].copy_from_slice(&p_wal.hdr.a_salt[1].to_ne_bytes());

        let native_cksum = (p_wal.hdr.big_end_cksum == SQLITE_BIGENDIAN as u8) as i32;
        let prev = a_cksum;
        wal_checksum_bytes(native_cksum, a_frame, 8, Some(&prev), &mut a_cksum);
        let prev = a_cksum;
        wal_checksum_bytes(native_cksum, a_data, p_wal.sz_page as i32, Some(&prev), &mut a_cksum);

        put4byte(&mut a_frame[16..20], a_cksum[0]);
        put4byte(&mut a_frame[20..24], a_cksum[1]);
        p_wal.hdr.a_frame_cksum = a_cksum;
    } else {
        a_frame[8..24].fill(0);
    }
}

/// Verifica se o frame com cabeçalho em `a_frame` e conteúdo em `a_data` é válido. Se for,
/// preenche `pi_page` e `pn_truncate` e devolve 1. Devolve 0 se o frame não for válido.
pub fn wal_decode_frame(
    p_wal: &Wal,
    pi_page: &mut u32,
    pn_truncate: &mut u32,
    a_data: &[u8],
    a_frame: &[u8],
) -> i32 {
    let mut a_cksum = p_wal.hdr.a_frame_cksum;
    debug_assert!(WAL_FRAME_HDRSIZE == 24);

    // Um frame só é válido se os valores de salt do cabeçalho do frame coincidem com os do
    // cabeçalho do wal.
    if a_frame[8..12] != p_wal.hdr.a_salt[0].to_ne_bytes() || a_frame[12..16] != p_wal.hdr.a_salt[1].to_ne_bytes() {
        return 0;
    }

    // Um frame só é válido se o número de página for maior que zero.
    let pgno = get4byte(&a_frame[0..4]);
    if pgno == 0 {
        return 0;
    }

    // Um frame só é válido se o checksum do cabeçalho do WAL, de todos os frames anteriores,
    // dos primeiros 16 bytes do cabeçalho deste frame e dos dados do frame coincidir com o
    // checksum nos últimos 8 bytes do cabeçalho do frame.
    let native_cksum = (p_wal.hdr.big_end_cksum == SQLITE_BIGENDIAN as u8) as i32;
    let prev = a_cksum;
    wal_checksum_bytes(native_cksum, a_frame, 8, Some(&prev), &mut a_cksum);
    let prev = a_cksum;
    wal_checksum_bytes(native_cksum, a_data, p_wal.sz_page as i32, Some(&prev), &mut a_cksum);
    if a_cksum[0] != get4byte(&a_frame[16..20]) || a_cksum[1] != get4byte(&a_frame[20..24]) {
        // Checksum falhou.
        return 0;
    }

    // Se chegamos aqui, o frame é válido. Devolve o número da página e o novo tamanho do banco.
    *pi_page = pgno;
    *pn_truncate = get4byte(&a_frame[4..8]);
    1
}

/// Liga ou solta locks no WAL. Locks são compartilhados ou exclusivos. Um lock não pode ser
/// movido diretamente entre compartilhado e exclusivo: precisa passar pelo estado destravado.
///
/// Em locking_mode=EXCLUSIVE, todas estas rotinas viram no-ops.
pub fn wal_lock_shared(p_wal: &mut Wal, lock_idx: i32) -> i32 {
    if p_wal.exclusive_mode != 0 {
        return SQLITE_OK;
    }
    os_shm_lock(
        &mut p_wal.p_db_fd.as_ref().unwrap().borrow_mut(),
        lock_idx,
        1,
        SQLITE_SHM_LOCK | SQLITE_SHM_SHARED,
    )
}

pub fn wal_unlock_shared(p_wal: &mut Wal, lock_idx: i32) {
    if p_wal.exclusive_mode != 0 {
        return;
    }
    let _ = os_shm_lock(
        &mut p_wal.p_db_fd.as_ref().unwrap().borrow_mut(),
        lock_idx,
        1,
        SQLITE_SHM_UNLOCK | SQLITE_SHM_SHARED,
    );
}

pub fn wal_lock_exclusive(p_wal: &mut Wal, lock_idx: i32, n: i32) -> i32 {
    if p_wal.exclusive_mode != 0 {
        return SQLITE_OK;
    }
    os_shm_lock(
        &mut p_wal.p_db_fd.as_ref().unwrap().borrow_mut(),
        lock_idx,
        n,
        SQLITE_SHM_LOCK | SQLITE_SHM_EXCLUSIVE,
    )
}

pub fn wal_unlock_exclusive(p_wal: &mut Wal, lock_idx: i32, n: i32) {
    if p_wal.exclusive_mode != 0 {
        return;
    }
    let _ = os_shm_lock(
        &mut p_wal.p_db_fd.as_ref().unwrap().borrow_mut(),
        lock_idx,
        n,
        SQLITE_SHM_UNLOCK | SQLITE_SHM_EXCLUSIVE,
    );
}

/// Calcula um hash sobre um número de página. O valor resultante deve cair entre 0 e
/// (HASHTABLE_NSLOT-1). A função `wal_next_hash` avança o hash para o próximo valor em caso
/// de colisão.
pub fn wal_hash(i_page: u32) -> i32 {
    debug_assert!(i_page > 0);
    debug_assert!((HASHTABLE_NSLOT & (HASHTABLE_NSLOT - 1)) == 0);
    (i_page.wrapping_mul(HASHTABLE_HASH_1) & (HASHTABLE_NSLOT as u32 - 1)) as i32
}

pub fn wal_next_hash(i_prior_hash: i32) -> i32 {
    (i_prior_hash + 1) & (HASHTABLE_NSLOT as i32 - 1)
}

/// Descreve a localização de uma tabela hash de páginas no wal-index. É o valor de retorno
/// de `wal_hash_get`.
///
/// Guarda posições em vez de ponteiros: `a_hash` é o índice, em palavras u32, do início da
/// tabela hash dentro da página do wal-index; `a_pgno` é o índice, em palavras u32, de
/// aPgno[0] dentro da mesma página; `i_zero` é uma unidade a menos que o número do primeiro
/// frame indexado.
#[derive(Clone, Copy, Debug, Default)]
pub struct WalHashLoc {
    /// Início da tabela hash do wal-index.
    pub a_hash: usize,
    /// aPgno[0] é a página do primeiro frame indexado.
    pub a_pgno: usize,
    /// Uma unidade a menos que o número do primeiro frame indexado.
    pub i_zero: u32,
}

/// Devolve a localização da tabela hash e do array de números de página guardados na página
/// `i_hash` do wal-index. O wal-index é quebrado em páginas de 32KB numeradas a partir de 0.
///
/// `a_hash` aponta para o início da tabela hash no arquivo do wal-index. `i_zero` é uma
/// unidade a menos que o número do primeiro frame indexado por esta tabela hash. Se um slot
/// da tabela hash vale N, ele se refere ao frame número (i_zero+N) do log.
///
/// Por fim, `a_pgno` é ajustado de modo que aPgno[0] seja o número de página do primeiro
/// frame indexado pela tabela hash, o frame (i_zero).
pub fn wal_hash_get(p_wal: &mut Wal, i_hash: i32, p_loc: &mut WalHashLoc) -> i32 {
    let mut rc = wal_index_page(p_wal, i_hash);
    debug_assert!(rc == SQLITE_OK || i_hash > 0);

    let has_page = p_wal
        .ap_wi_data
        .get(i_hash as usize)
        .map_or(false, |p| p.is_some());
    if has_page {
        p_loc.a_pgno = 0;
        p_loc.a_hash = HASHTABLE_NPAGE;
        if i_hash == 0 {
            p_loc.a_pgno = WALINDEX_HDR_SIZE / 4;
            p_loc.i_zero = 0;
        } else {
            p_loc.i_zero = (HASHTABLE_NPAGE_ONE + (i_hash as usize - 1) * HASHTABLE_NPAGE) as u32;
        }
    } else if rc == SQLITE_OK {
        rc = SQLITE_ERROR;
    }
    rc
}


// ---- part_003.rs ----

// Contrato assumido com as outras partes de wal_c (o tech lead reconcilia):
//
// - `WalHashLoc` (parte 2) guarda posições, não ponteiros: `a_hash` é o índice, em
//   palavras u32, do início da tabela de hash dentro da página do wal-index
//   (sempre HASHTABLE_NPAGE); `a_pgno` é o índice, em palavras u32, de aPgno[0]
//   (WALINDEX_HDR_SIZE/4 na página 0, 0 nas demais); `i_zero` é o iZero do C.
//   A página em si é `p_wal.ap_wi_data[i_hash]`.
// - `wal_hash_get(&mut Wal, i32, &mut WalHashLoc) -> i32` e `wal_index_page(&mut Wal, i32) -> i32`
//   (depois da chamada, a página fica em `ap_wi_data[i_page]`, `None` no erro).
// - `wal_ckpt_info(&mut Wal) -> &mut WalCkptInfo` e `wal_index_write_hdr(&mut Wal)`.
// - `wal_hash(u32) -> i32` e `wal_next_hash(i32) -> i32`.
// - `wal_decode_frame(&Wal, &mut u32, &mut u32, &[u8], &[u8]) -> i32`.
// - `wal_checksum_bytes(i32, &[u8], i32, Option<&[u32; 2]>, &mut [u32; 2])`.

/// Lê o slot `k` da tabela de hash que começa na palavra `a_hash` da página
/// (cada ht_slot é um u16; o wal-index usa a ordem de bytes nativa, little-endian no Debian).
#[inline]
fn wal_ht_slot_get(page: &[u32], a_hash: usize, k: usize) -> u16 {
    let word = page[a_hash + (k >> 1)];
    if k & 1 == 0 {
        word as u16
    } else {
        (word >> 16) as u16
    }
}

/// Grava o slot `k` da tabela de hash que começa na palavra `a_hash` da página.
#[inline]
fn wal_ht_slot_set(page: &mut [u32], a_hash: usize, k: usize, value: u16) {
    let word = &mut page[a_hash + (k >> 1)];
    if k & 1 == 0 {
        *word = (*word & 0xffff_0000) | value as u32;
    } else {
        *word = (*word & 0x0000_ffff) | ((value as u32) << 16);
    }
}

/// Página `i_page` do wal-index, já mapeada em `ap_wi_data`.
#[inline]
fn wal_wi_page(p_wal: &Wal, i_page: i32) -> &[u32] {
    p_wal.ap_wi_data[i_page as usize].as_ref().unwrap()
}

/// Página `i_page` do wal-index, já mapeada em `ap_wi_data`, para escrita.
#[inline]
fn wal_wi_page_mut(p_wal: &mut Wal, i_page: i32) -> &mut [u32] {
    p_wal.ap_wi_data[i_page as usize].as_mut().unwrap()
}

/// Retorna o número da página do wal-index que contém a tabela de hash e o
/// array de números de página com as entradas correspondentes ao frame iFrame.
/// O wal-index é dividido em páginas de 32KB, numeradas a partir de 0.
fn wal_frame_page(i_frame: u32) -> i32 {
    let i_hash = ((i_frame as usize + HASHTABLE_NPAGE - HASHTABLE_NPAGE_ONE - 1) / HASHTABLE_NPAGE) as i32;
    debug_assert!(
        (i_hash == 0 || i_frame as usize > HASHTABLE_NPAGE_ONE)
            && (i_hash >= 1 || i_frame as usize <= HASHTABLE_NPAGE_ONE)
            && (i_hash <= 1 || i_frame as usize > (HASHTABLE_NPAGE_ONE + HASHTABLE_NPAGE))
            && (i_hash >= 2 || i_frame as usize <= HASHTABLE_NPAGE_ONE + HASHTABLE_NPAGE)
            && (i_hash <= 2 || i_frame as usize > (HASHTABLE_NPAGE_ONE + 2 * HASHTABLE_NPAGE))
    );
    debug_assert!(i_hash >= 0);
    i_hash
}

/// Retorna o número de página associado ao frame iFrame neste WAL.
fn wal_frame_pgno(p_wal: &Wal, i_frame: u32) -> u32 {
    let i_hash = wal_frame_page(i_frame);
    if i_hash == 0 {
        return wal_wi_page(p_wal, 0)[WALINDEX_HDR_SIZE / std::mem::size_of::<u32>() + i_frame as usize - 1];
    }
    wal_wi_page(p_wal, i_hash)[(i_frame as usize - 1 - HASHTABLE_NPAGE_ONE) % HASHTABLE_NPAGE]
}

/// Remove da tabela de hash as entradas que apontam para slots do WAL maiores
/// que pWal->hdr.mxFrame.
///
/// Chamada sempre que pWal->hdr.mxFrame diminui por causa de um rollback ou
/// savepoint. No máximo a tabela de hash que contém pWal->hdr.mxFrame precisa
/// ser atualizada: as tabelas seguintes são zeradas automaticamente quando
/// mxFrame avança até o ponto em que elas passam a ser necessárias.
fn wal_cleanup_hash(p_wal: &mut Wal) {
    let mut s_loc = WalHashLoc::default();

    debug_assert!(p_wal.write_lock != 0);

    if p_wal.hdr.mx_frame == 0 {
        return;
    }

    // Localiza a tabela de hash e o array de números de página que contêm a
    // entrada do frame pWal->hdr.mxFrame. A página já está mapeada (1).
    let i_page = wal_frame_page(p_wal.hdr.mx_frame);
    debug_assert!(p_wal.n_wi_data > i_page);
    debug_assert!(p_wal.ap_wi_data[i_page as usize].is_some());
    let i = wal_hash_get(p_wal, i_page, &mut s_loc);
    if i != 0 {
        // Defesa em profundidade, caso (1) acima esteja errado.
        return;
    }

    // Zera as entradas da tabela de hash que correspondem a frames maiores
    // que pWal->hdr.mxFrame.
    let i_limit = (p_wal.hdr.mx_frame - s_loc.i_zero) as usize;
    debug_assert!(i_limit > 0);
    let page = wal_wi_page_mut(p_wal, i_page);
    for i in 0..HASHTABLE_NSLOT {
        if wal_ht_slot_get(page, s_loc.a_hash, i) as usize > i_limit {
            wal_ht_slot_set(page, s_loc.a_hash, i, 0);
        }
    }

    // Zera as entradas de aPgno[] que correspondem a frames maiores que
    // pWal->hdr.mxFrame: da posição aPgno[iLimit] até o início da tabela de hash.
    page[s_loc.a_pgno + i_limit..s_loc.a_hash].fill(0);
}

/// Cria no wal-index uma entrada que mapeia o número de página de banco
/// iPage para o frame iFrame do WAL.
fn wal_index_append(p_wal: &mut Wal, i_frame: u32, i_page: u32) -> i32 {
    let mut s_loc = WalHashLoc::default();
    let i_wi_page = wal_frame_page(i_frame);

    let rc = wal_hash_get(p_wal, i_wi_page, &mut s_loc);

    // Supondo que o arquivo do wal-index foi mapeado com sucesso, preenche o
    // array de números de página e a entrada da tabela de hash.
    if rc == SQLITE_OK {
        let idx = (i_frame - s_loc.i_zero) as usize;
        debug_assert!(idx <= HASHTABLE_NSLOT / 2 + 1);

        // Se esta é a primeira entrada da tabela de hash, zera a tabela inteira
        // e o array aPgno[] antes de prosseguir: de aPgno[0] até o fim da tabela.
        if idx == 1 {
            let end = s_loc.a_hash + HASHTABLE_NSLOT * std::mem::size_of::<HtSlot>() / std::mem::size_of::<u32>();
            wal_wi_page_mut(p_wal, i_wi_page)[s_loc.a_pgno..end].fill(0);
        }

        // Se a entrada em aPgno[] já está preenchida, o escritor anterior
        // terminou de forma inesperada no meio de uma transação (depois de
        // gravar páginas sujas no WAL para liberar memória). Remove da tabela
        // de hash os restos dessa transação não confirmada antes de gravar.
        if wal_wi_page(p_wal, i_wi_page)[s_loc.a_pgno + idx - 1] != 0 {
            wal_cleanup_hash(p_wal);
            debug_assert!(wal_wi_page(p_wal, i_wi_page)[s_loc.a_pgno + idx - 1] == 0);
        }

        // Grava a entrada de aPgno[] e o slot da tabela de hash.
        let mut n_collide = idx as i32;
        let mut i_key = wal_hash(i_page);
        while wal_ht_slot_get(wal_wi_page(p_wal, i_wi_page), s_loc.a_hash, i_key as usize) != 0 {
            let prior = n_collide;
            n_collide -= 1;
            if prior == 0 {
                return SQLITE_CORRUPT_BKPT;
            }
            i_key = wal_next_hash(i_key);
        }
        let page = wal_wi_page_mut(p_wal, i_wi_page);
        page[s_loc.a_pgno + idx - 1] = i_page;
        wal_ht_slot_set(page, s_loc.a_hash, i_key as usize, idx as HtSlot);
    }

    rc
}

/// Recupera o wal-index lendo o arquivo write-ahead log.
///
/// Primeiro tenta obter um lock exclusivo sobre o wal-index para impedir que
/// outras threads ou processos mexam no WAL ou no wal-index durante a
/// recuperação. O WAL_RECOVER_LOCK também é mantido, para que as outras threads
/// saibam que esta está recuperando. Se não conseguir os locks, retorna SQLITE_BUSY.
fn wal_index_recover(p_wal: &mut Wal) -> i32 {
    let mut a_frame_cksum: [u32; 2] = [0, 0];

    // Obtém um lock exclusivo sobre todos os bytes da faixa de locks que o
    // chamador ainda não travou. O chamador garante ter travado o byte
    // WAL_WRITE_LOCK e pode ter travado também o WAL_CKPT_LOCK. Se der certo,
    // os mesmos bytes travados aqui são destravados antes de a função retornar.
    debug_assert!(p_wal.ckpt_lock == 1 || p_wal.ckpt_lock == 0);
    debug_assert!(WAL_ALL_BUT_WRITE == WAL_WRITE_LOCK + 1);
    debug_assert!(WAL_CKPT_LOCK == WAL_ALL_BUT_WRITE);
    debug_assert!(p_wal.write_lock != 0);
    let i_lock = WAL_ALL_BUT_WRITE as i32 + p_wal.ckpt_lock as i32;
    let mut rc = wal_lock_exclusive(p_wal, i_lock, wal_read_lock(0) as i32 - i_lock);
    if rc != 0 {
        return rc;
    }

    // memset(&pWal->hdr, 0, sizeof(WalIndexHdr))
    p_wal.hdr = WalIndexHdr {
        i_version: 0,
        unused: 0,
        i_change: 0,
        is_init: 0,
        big_end_cksum: 0,
        sz_page: 0,
        mx_frame: 0,
        n_page: 0,
        a_frame_cksum: [0, 0],
        a_salt: [0, 0],
        a_cksum: [0, 0],
    };

    'recovery_error: {
        let mut n_size: i64 = 0;
        rc = os_file_size(p_wal.p_wal_fd.as_deref_mut().unwrap(), &mut n_size);
        if rc != SQLITE_OK {
            break 'recovery_error;
        }

        'finished: {
            if n_size > WAL_HDRSIZE as i64 {
                let mut a_buf = [0u8; WAL_HDRSIZE]; // Buffer para carregar o cabeçalho do WAL

                // Lê o cabeçalho do WAL.
                rc = os_read(p_wal.p_wal_fd.as_deref_mut().unwrap(), &mut a_buf, 0);
                if rc != SQLITE_OK {
                    break 'recovery_error;
                }

                // Se o tamanho de página do banco não é potência de dois, ou é maior
                // que SQLITE_MAX_PAGE_SIZE, conclui que o WAL não tem dados válidos.
                // Do mesmo modo, se o valor 'magic' é inválido, ignora o WAL inteiro.
                let magic: u32 = get4byte(&a_buf[0..]);
                let sz_page: i32 = get4byte(&a_buf[8..]) as i32;
                if (magic & 0xFFFFFFFE) != WAL_MAGIC
                    || (sz_page & sz_page.wrapping_sub(1)) != 0
                    || sz_page > SQLITE_MAX_PAGE_SIZE
                    || sz_page < 512
                {
                    break 'finished;
                }
                p_wal.hdr.big_end_cksum = (magic & 0x00000001) as u8;
                p_wal.sz_page = sz_page as u32;
                p_wal.n_ckpt = get4byte(&a_buf[12..]);
                p_wal.hdr.a_salt[0] = u32::from_ne_bytes([a_buf[16], a_buf[17], a_buf[18], a_buf[19]]);
                p_wal.hdr.a_salt[1] = u32::from_ne_bytes([a_buf[20], a_buf[21], a_buf[22], a_buf[23]]);

                // Verifica se o checksum do cabeçalho do WAL está correto.
                let native_cksum = (p_wal.hdr.big_end_cksum == SQLITE_BIGENDIAN as u8) as i32;
                wal_checksum_bytes(
                    native_cksum,
                    &a_buf,
                    (WAL_HDRSIZE - 2 * 4) as i32,
                    None,
                    &mut p_wal.hdr.a_frame_cksum,
                );
                if p_wal.hdr.a_frame_cksum[0] != get4byte(&a_buf[24..])
                    || p_wal.hdr.a_frame_cksum[1] != get4byte(&a_buf[28..])
                {
                    break 'finished;
                }

                // Verifica se o número de versão do formato do WAL é um que
                // sabemos interpretar.
                let version: u32 = get4byte(&a_buf[4..]);
                if version != WAL_MAX_VERSION {
                    rc = SQLITE_CANTOPEN_BKPT;
                    break 'finished;
                }

                // Aloca um buffer para ler os frames (no C, o mesmo malloc traz
                // depois do frame a cópia privada da página do wal-index).
                let sz_frame: i32 = sz_page + WAL_FRAME_HDRSIZE as i32;
                let mut a_frame: Vec<u8> = Vec::new();
                let mut a_private: Vec<u32> = Vec::new();
                if a_frame.try_reserve_exact(sz_frame as usize).is_err()
                    || a_private.try_reserve_exact(WALINDEX_PGSZ / std::mem::size_of::<u32>()).is_err()
                {
                    rc = SQLITE_NOMEM_BKPT;
                    break 'recovery_error;
                }
                a_frame.resize(sz_frame as usize, 0);
                a_private.resize(WALINDEX_PGSZ / std::mem::size_of::<u32>(), 0);

                // Lê todos os frames do arquivo de log.
                let i_last_frame: u32 = ((n_size - WAL_HDRSIZE as i64) / sz_frame as i64) as u32;
                let mut i_pg: u32 = 0;
                while i_pg <= wal_frame_page(i_last_frame) as u32 {
                    let i_last: u32 = std::cmp::min(
                        i_last_frame as usize,
                        HASHTABLE_NPAGE_ONE + i_pg as usize * HASHTABLE_NPAGE,
                    ) as u32;
                    let i_first: u32 = 1 + if i_pg == 0 {
                        0
                    } else {
                        (HASHTABLE_NPAGE_ONE + (i_pg as usize - 1) * HASHTABLE_NPAGE) as u32
                    };
                    rc = wal_index_page(p_wal, i_pg as i32);
                    let a_share = match p_wal.ap_wi_data.get_mut(i_pg as usize).and_then(|slot| slot.take()) {
                        Some(page) => page,
                        None => {
                            debug_assert!(rc != SQLITE_OK);
                            break;
                        }
                    };
                    p_wal.ap_wi_data[i_pg as usize] = Some(std::mem::take(&mut a_private));

                    let mut i_frame: u32 = i_first;
                    while i_frame <= i_last {
                        let i_offset: i64 = wal_frame_offset(i_frame, sz_page as u32);
                        let mut pgno: u32 = 0; // Número de página do banco para o frame
                        let mut n_truncate: u32 = 0; // Campo dbsize do cabeçalho do frame

                        // Lê e decodifica o próximo frame do log.
                        rc = os_read(p_wal.p_wal_fd.as_deref_mut().unwrap(), &mut a_frame, i_offset);
                        if rc != SQLITE_OK {
                            break;
                        }
                        let is_valid = wal_decode_frame(
                            p_wal,
                            &mut pgno,
                            &mut n_truncate,
                            &a_frame[WAL_FRAME_HDRSIZE..],
                            &a_frame,
                        );
                        if is_valid == 0 {
                            break;
                        }
                        rc = wal_index_append(p_wal, i_frame, pgno);
                        if rc != SQLITE_OK {
                            break;
                        }

                        // Se nTruncate é diferente de zero, este é um registro de commit.
                        if n_truncate != 0 {
                            p_wal.hdr.mx_frame = i_frame;
                            p_wal.hdr.n_page = n_truncate;
                            p_wal.hdr.sz_page = ((sz_page & 0xff00) | (sz_page >> 16)) as u16;
                            a_frame_cksum[0] = p_wal.hdr.a_frame_cksum[0];
                            a_frame_cksum[1] = p_wal.hdr.a_frame_cksum[1];
                        }
                        i_frame += 1;
                    }
                    a_private = p_wal.ap_wi_data[i_pg as usize].take().unwrap();
                    p_wal.ap_wi_data[i_pg as usize] = Some(a_share);
                    let n_hdr: usize = if i_pg == 0 { WALINDEX_HDR_SIZE } else { 0 };
                    let n_hdr32: usize = n_hdr / std::mem::size_of::<u32>();
                    // O memcpy() funciona bem aqui em todas as implementações razoáveis
                    // (SQLITE_SAFER_WALINDEX_RECOVERY não está definido no Debian).
                    wal_wi_page_mut(p_wal, i_pg as i32)[n_hdr32..WALINDEX_PGSZ / std::mem::size_of::<u32>()]
                        .copy_from_slice(&a_private[n_hdr32..WALINDEX_PGSZ / std::mem::size_of::<u32>()]);
                    if i_frame <= i_last {
                        break;
                    }
                    i_pg += 1;
                }
            }
        }

        // finished:
        if rc == SQLITE_OK {
            p_wal.hdr.a_frame_cksum[0] = a_frame_cksum[0];
            p_wal.hdr.a_frame_cksum[1] = a_frame_cksum[1];
            wal_index_write_hdr(p_wal);

            // Reinicia o cabeçalho de checkpoint. É seguro porque esta thread
            // mantém locks que excluem todos os outros escritores e checkpointers.
            // Depois define os valores dos slots de read-mark de 1 até N.
            wal_ckpt_info(p_wal).n_backfill = 0;
            let mx_frame = p_wal.hdr.mx_frame;
            wal_ckpt_info(p_wal).n_backfill_attempted = mx_frame;
            wal_ckpt_info(p_wal).a_read_mark[0] = 0;
            for i in 1..WAL_NREADER {
                rc = wal_lock_exclusive(p_wal, wal_read_lock(i as u32) as i32, 1);
                if rc == SQLITE_OK {
                    let mark = if i == 1 && p_wal.hdr.mx_frame != 0 {
                        p_wal.hdr.mx_frame
                    } else {
                        READMARK_NOT_USED
                    };
                    wal_ckpt_info(p_wal).a_read_mark[i] = mark;
                    wal_unlock_exclusive(p_wal, wal_read_lock(i as u32) as i32, 1);
                } else if rc != SQLITE_BUSY {
                    break 'recovery_error;
                }
            }

            // Se mais de um frame foi recuperado do arquivo de log, reporta um
            // evento via sqlite3_log(). Ajuda a identificar problemas de desempenho
            // causados por aplicações que encerram rotineiramente sem fazer
            // checkpoint do log.
            if p_wal.hdr.n_page != 0 {
                api_log(
                    SQLITE_NOTICE_RECOVER_WAL,
                    b"recovered %d frames from WAL file %s",
                    &[
                        Value::Int(p_wal.hdr.mx_frame as i64),
                        Value::Text(p_wal.z_wal_name.clone()),
                    ],
                );
            }
        }
    }

    // recovery_error:
    wal_unlock_exclusive(p_wal, i_lock, wal_read_lock(0) as i32 - i_lock);
    rc
}


// ---- part_004.rs ----

/// Fecha um wal-index aberto.
pub fn wal_index_close(p_wal: &mut Wal, is_delete: i32) {
    if p_wal.exclusive_mode == WAL_HEAPMEMORY_MODE || p_wal.b_shm_unreliable != 0 {
        for i in 0..p_wal.n_wi_data as usize {
            // Soltar a página (dono único) equivale ao sqlite3_free() do C.
            p_wal.ap_wi_data[i] = None;
        }
    }
    if p_wal.exclusive_mode != WAL_HEAPMEMORY_MODE {
        // O retorno de sqlite3OsShmUnmap() é descartado no C.
        let _ = os_shm_unmap(&mut p_wal.p_db_fd.as_ref().unwrap().borrow_mut(), is_delete);
    }
}

/// Abre uma conexão com o arquivo WAL `z_wal_name`. O arquivo de banco de dados
/// já deve estar aberto na conexão `p_db_fd`. O nome é copiado para o objeto Wal,
/// então o buffer do chamador não precisa sobreviver à chamada.
///
/// Um lock SHARED deve ser mantido no arquivo de banco de dados quando esta
/// função é chamada. O propósito desse lock é impedir que qualquer outro cliente
/// remova o arquivo WAL ou o arquivo wal-index. Se outro processo fizesse isso
/// logo depois de este cliente abrir um desses arquivos, o sistema ficaria
/// seriamente quebrado.
///
/// Se o arquivo de log for aberto com sucesso, retorna SQLITE_OK e `*pp_wal`
/// recebe o novo identificador WAL. Se ocorrer um erro, retorna um código de
/// erro do SQLite e `*pp_wal` fica como `None`.
pub fn wal_open(
    p_vfs: &VfsRef,
    p_db_fd: &Rc<RefCell<Sqlite3File>>,
    z_wal_name: &[u8],
    b_no_shm: i32,
    mx_wal_size: i64,
    pp_wal: &mut Option<Box<Wal>>,
) -> i32 {
    let rc: i32;
    let mut flags: i32;

    debug_assert!(!z_wal_name.is_empty());

    // Verifica os valores de várias constantes. Qualquer mudança nelas resultaria
    // em um formato incompatível em disco para o arquivo -shm. Qualquer mudança
    // que faça uma destas asserções falhar é um problema de compatibilidade
    // retroativa, mesmo que a mudança funcione de outro modo.
    //
    // Esta tabela também serve como referência cruzada ao interpretar dumps
    // hexadecimais do arquivo -shm. As conferências de tamanho de WalIndexHdr e
    // WalCkptInfo (48 e 40) ficam com o módulo que define as estruturas.
    debug_assert!(WALINDEX_LOCK_OFFSET as i64 == 120);
    debug_assert!(WALINDEX_HDR_SIZE as i64 == 136);
    debug_assert!(HASHTABLE_NPAGE as i64 == 4096);
    debug_assert!(HASHTABLE_NPAGE_ONE as i64 == 4062);
    debug_assert!(HASHTABLE_NSLOT as i64 == 8192);
    debug_assert!(HASHTABLE_HASH_1 as i64 == 383);
    debug_assert!(WALINDEX_PGSZ as i64 == 32768);
    debug_assert!(SQLITE_SHM_NLOCK as i64 == 8);
    debug_assert!(WAL_NREADER as i64 == 5);
    debug_assert!(WAL_FRAME_HDRSIZE as i64 == 24);
    debug_assert!(WAL_HDRSIZE as i64 == 32);
    debug_assert!(WALINDEX_LOCK_OFFSET as i64 + WAL_WRITE_LOCK as i64 == 120);
    debug_assert!(WALINDEX_LOCK_OFFSET as i64 + WAL_CKPT_LOCK as i64 == 121);
    debug_assert!(WALINDEX_LOCK_OFFSET as i64 + WAL_RECOVER_LOCK as i64 == 122);
    debug_assert!(WALINDEX_LOCK_OFFSET as i64 + wal_read_lock(0) as i64 == 123);
    debug_assert!(WALINDEX_LOCK_OFFSET as i64 + wal_read_lock(1) as i64 == 124);
    debug_assert!(WALINDEX_LOCK_OFFSET as i64 + wal_read_lock(2) as i64 == 125);
    debug_assert!(WALINDEX_LOCK_OFFSET as i64 + wal_read_lock(3) as i64 == 126);
    debug_assert!(WALINDEX_LOCK_OFFSET as i64 + wal_read_lock(4) as i64 == 127);

    // No amalgamation, os_unix.c vem antes deste arquivo: o deslocamento dos
    // bytes de lock em os_unix.c (UNIX_SHM_BASE) concorda com
    // WALINDEX_LOCK_OFFSET (120). A conferência vive junto de UNIX_SHM_BASE.

    // Aloca uma instância de Wal para retornar. A falta de memória do C
    // (SQLITE_NOMEM_BKPT) não existe aqui: a alocação em Rust não retorna nulo.
    *pp_wal = None;
    let mut p_ret = Box::new(Wal::default());

    p_ret.p_vfs = Some(Rc::clone(p_vfs));
    p_ret.p_db_fd = Some(Rc::clone(p_db_fd));
    p_ret.read_lock = -1;
    p_ret.mx_wal_size = mx_wal_size;
    p_ret.z_wal_name = z_wal_name.to_vec();
    p_ret.sync_header = 1;
    p_ret.pad_to_sector_boundary = 1;
    p_ret.exclusive_mode = if b_no_shm != 0 { WAL_HEAPMEMORY_MODE } else { WAL_NORMAL_MODE };

    // Abre o identificador de arquivo do arquivo de write-ahead log.
    // No C o mesmo `flags` entra e sai (pOutFlags aponta para ele).
    flags = SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE | SQLITE_OPEN_WAL;
    let mut p_wal_fd = Box::new(Sqlite3File::default());
    let open_flags = flags;
    rc = os_open(&**p_vfs, Some(z_wal_name), &mut p_wal_fd, open_flags, Some(&mut flags));
    p_ret.p_wal_fd = Some(p_wal_fd);
    if rc == SQLITE_OK && (flags & SQLITE_OPEN_READONLY) != 0 {
        p_ret.read_only = WAL_RDONLY;
    }

    if rc != SQLITE_OK {
        wal_index_close(&mut p_ret, 0);
        os_close(p_ret.p_wal_fd.as_mut().unwrap());
        // `p_ret` é solto ao sair do escopo (sqlite3_free).
    } else {
        let i_dc = os_device_characteristics(&mut p_db_fd.borrow_mut());
        if (i_dc & SQLITE_IOCAP_SEQUENTIAL) != 0 {
            p_ret.sync_header = 0;
        }
        if (i_dc & SQLITE_IOCAP_POWERSAFE_OVERWRITE) != 0 {
            p_ret.pad_to_sector_boundary = 0;
        }
        *pp_wal = Some(p_ret);
    }
    rc
}

/// Muda o tamanho para o qual o arquivo WAL é truncado a cada reset.
pub fn wal_limit(p_wal: Option<&mut Wal>, i_limit: i64) {
    if let Some(p_wal) = p_wal {
        p_wal.mx_wal_size = i_limit;
    }
}

/// Encontra o menor número de página, dentre todas as páginas mantidas no WAL,
/// que ainda não foi retornado por nenhuma chamada anterior deste método sobre o
/// mesmo WalIterator. Escreve em `*pi_frame` o índice do quadro onde essa página
/// foi escrita pela última vez no WAL e em `*pi_page` o número da página.
///
/// Retorna 0 em caso de sucesso. Se não houver páginas no WAL com número maior
/// que `*pi_page`, retorna 1.
pub fn wal_iterator_next(p: &mut WalIterator, pi_page: &mut u32, pi_frame: &mut u32) -> i32 {
    let i_min: u32; // O pgno do resultado precisa ser maior que i_min
    let mut i_ret: u32 = 0xFFFFFFFF; // 0xffffffff nunca é um número de página válido

    i_min = p.i_prior;
    debug_assert!(i_min < 0xffffffff);
    for i in (0..p.n_segment).rev() {
        let p_segment = &mut p.a_segment[i as usize];
        while p_segment.i_next < p_segment.n_entry {
            let i_pg: u32 = p_segment.a_pgno[p_segment.a_index[p_segment.i_next as usize] as usize];
            if i_pg > i_min {
                if i_pg < i_ret {
                    i_ret = i_pg;
                    *pi_frame = (p_segment.i_zero + p_segment.a_index[p_segment.i_next as usize] as i32) as u32;
                }
                break;
            }
            p_segment.i_next += 1;
        }
    }

    p.i_prior = i_ret;
    *pi_page = i_ret;
    (i_ret == 0xFFFFFFFF) as i32
}

/// Esta função mescla duas listas ordenadas em uma única lista ordenada.
///
/// `a_left` e a lista direita são vetores de índices; as duas moram no mesmo
/// buffer `a_list`, e `left` e `*right` são os deslocamentos de início de cada
/// uma (no C, ponteiros para dentro do mesmo vetor). A chave de ordenação é
/// `a_content[a_list[..]]`. Na entrada, vale para todo J<K:
///
///        a_content[esq[J]] < a_content[esq[K]]
///        a_content[dir[J]] < a_content[dir[K]]
///
/// A rotina sobrescreve a lista direita com uma nova sequência (provavelmente
/// mais longa) de índices, de modo que ela contenha todo índice que aparece na
/// lista esquerda ou na direita antiga, mantendo a segunda condição acima.
///
/// Os valores `a_content[esq[X]]` são únicos para todo X, e os da direita também.
/// Mas pode haver combinações de X e Y tais que
///
///      esq[X]!=dir[Y]  &&  a_content[esq[X]] == a_content[dir[Y]]
///
/// Quando isso acontece, omite-se `esq[X]` e usa-se o índice `dir[Y]`.
///
/// Na saída, `*right` aponta para o início da lista esquerda (onde o resultado
/// foi copiado) e `*pn_right` é o número de elementos do resultado.
pub fn wal_merge(
    a_content: &[u32],
    a_list: &mut [HtSlot],
    left: usize,
    n_left: i32,
    right: &mut usize,
    pn_right: &mut i32,
    a_tmp: &mut [HtSlot],
) {
    let mut i_left: i32 = 0; // Índice atual em a_left
    let mut i_right: i32 = 0; // Índice atual em a_right
    let mut i_out: i32 = 0; // Índice atual no buffer de saída
    let n_right: i32 = *pn_right;
    let a_right: usize = *right;

    debug_assert!(n_left > 0 && n_right > 0);
    while i_right < n_right || i_left < n_left {
        let logpage: HtSlot;
        let dbpage: Pgno;

        if i_left < n_left
            && (i_right >= n_right
                || a_content[a_list[left + i_left as usize] as usize]
                    < a_content[a_list[a_right + i_right as usize] as usize])
        {
            logpage = a_list[left + i_left as usize];
            i_left += 1;
        } else {
            logpage = a_list[a_right + i_right as usize];
            i_right += 1;
        }
        dbpage = a_content[logpage as usize];

        a_tmp[i_out as usize] = logpage;
        i_out += 1;
        if i_left < n_left && a_content[a_list[left + i_left as usize] as usize] == dbpage {
            i_left += 1;
        }

        debug_assert!(i_left >= n_left || a_content[a_list[left + i_left as usize] as usize] > dbpage);
        debug_assert!(i_right >= n_right || a_content[a_list[a_right + i_right as usize] as usize] > dbpage);
    }

    *right = left;
    *pn_right = i_out;
    a_list[left..left + i_out as usize].copy_from_slice(&a_tmp[..i_out as usize]);
}

/// Ordena os elementos da lista `a_list` usando `a_content[]` como chave.
/// Remove elementos com chaves duplicadas, preferindo manter os maiores valores
/// de `a_list[]`.
///
/// As entradas de `a_list[]` são índices em `a_content[]`. Os valores de
/// `a_list[]` são ordenados de modo que, para todo J<K:
///
///      a_content[a_list[J]] < a_content[a_list[K]]
///
/// Para quaisquer X e Y tais que
///
///      a_content[a_list[X]] == a_content[a_list[Y]]
///
/// mantém-se o maior dos dois valores a_list[X] e a_list[Y] e descarta-se o menor.
pub fn wal_mergesort(
    a_content: &[u32],
    a_buffer: &mut [HtSlot],
    a_list: &mut [HtSlot],
    pn_list: &mut i32,
) {
    // Sub-lista: número de elementos e deslocamento de início dentro de a_list.
    #[derive(Clone, Copy, Default)]
    struct Sublist {
        n_list: i32,
        a_list: usize,
    }

    let n_list: i32 = *pn_list; // Tamanho da lista de entrada
    let mut n_merge: i32 = 0; // Número de elementos na lista a_merge
    let mut a_merge: usize = 0; // Lista a ser mesclada (deslocamento em a_list)
    let mut i_sub: u32 = 0; // Índice no vetor a_sub
    let mut a_sub: [Sublist; 13] = [Sublist::default(); 13]; // Vetor de sub-listas

    debug_assert!(n_list <= HASHTABLE_NPAGE as i32 && n_list > 0);
    debug_assert!(HASHTABLE_NPAGE as usize == (1usize << (a_sub.len() - 1)));

    for i_list in 0..n_list {
        n_merge = 1;
        a_merge = i_list as usize;
        i_sub = 0;
        while (i_list & (1 << i_sub)) != 0 {
            debug_assert!((i_sub as usize) < a_sub.len());
            let p = a_sub[i_sub as usize];
            debug_assert!(p.n_list <= (1 << i_sub));
            debug_assert!(p.a_list == (i_list & !((2 << i_sub) - 1)) as usize);
            wal_merge(a_content, a_list, p.a_list, p.n_list, &mut a_merge, &mut n_merge, a_buffer);
            i_sub += 1;
        }
        a_sub[i_sub as usize].a_list = a_merge;
        a_sub[i_sub as usize].n_list = n_merge;
    }

    i_sub += 1;
    while (i_sub as usize) < a_sub.len() {
        if (n_list & (1 << i_sub)) != 0 {
            let p = a_sub[i_sub as usize];
            debug_assert!(p.n_list <= (1 << i_sub));
            debug_assert!(p.a_list == (n_list & !((2 << i_sub) - 1)) as usize);
            wal_merge(a_content, a_list, p.a_list, p.n_list, &mut a_merge, &mut n_merge, a_buffer);
        }
        i_sub += 1;
    }
    debug_assert!(a_merge == 0);
    *pn_list = n_merge;
}

/// Libera um iterador alocado por `wal_iterator_init()`. O iterador é dono de
/// seus vetores, então a liberação acontece ao sair do escopo.
pub fn wal_iterator_free(_p: Box<WalIterator>) {}

/// Constrói um objeto WalIterator que pode ser usado para percorrer, em ordem
/// crescente, todas as páginas do WAL que seguem o quadro `n_backfill`. Quadros
/// `n_backfill` ou anteriores podem ser incluídos; excluí-los é só uma
/// otimização. O chamador precisa manter o lock de checkpoint.
///
/// Em caso de sucesso, `*pp` recebe o novo objeto WalIterator e retorna
/// SQLITE_OK. Caso contrário, retorna um código de erro e `*pp` fica como `None`.
///
/// A rotina chamadora deve invocar `wal_iterator_free()` para destruir o objeto
/// WalIterator quando terminar de usá-lo.
pub fn wal_iterator_init(p_wal: &mut Wal, n_backfill: u32, pp: &mut Option<Box<WalIterator>>) -> i32 {
    let mut rc: i32 = SQLITE_OK; // Código de retorno

    // Esta rotina só roda enquanto se mantém o lock de checkpoint, e só roda se
    // houver conteúdo no log (mx_frame>0).
    debug_assert!(p_wal.ckpt_lock != 0 && p_wal.hdr.mx_frame > 0);
    let i_last: u32 = p_wal.hdr.mx_frame; // Último quadro no log

    // Aloca o objeto WalIterator. Os vetores de índices de cada segmento são
    // próprios do segmento; o buffer temporário do merge sort é compartilhado.
    let n_segment: i32 = wal_frame_page(i_last) + 1; // Número de segmentos a mesclar
    let mut p = Box::new(WalIterator {
        i_prior: 0,
        n_segment,
        a_segment: (0..n_segment)
            .map(|_| WalSegment { i_next: 0, a_index: Vec::new(), a_pgno: Vec::new(), n_entry: 0, i_zero: 0 })
            .collect(),
    });
    let n_tmp: usize = if i_last > HASHTABLE_NPAGE as u32 { HASHTABLE_NPAGE as usize } else { i_last as usize };
    let mut a_tmp: Vec<HtSlot> = vec![0; n_tmp]; // Espaço temporário do merge sort

    let mut i: i32 = wal_frame_page(n_backfill + 1);
    while rc == SQLITE_OK && i < n_segment {
        let mut s_loc = WalHashLoc::default();

        rc = wal_hash_get(p_wal, i, &mut s_loc);
        if rc == SQLITE_OK {
            // Número de entradas neste segmento. No último segmento vai até
            // i_last; nos demais é a distância entre a_pgno e a_hash em palavras
            // de 32 bits: HASHTABLE_NPAGE_ONE na primeira página do wal-index
            // (a_pgno começa depois do cabeçalho) e HASHTABLE_NPAGE nas outras.
            let mut n_entry: i32 = if (i + 1) == n_segment {
                (i_last - s_loc.i_zero) as i32
            } else {
                // (u32*)sLoc.aHash - (u32*)sLoc.aPgno: as duas posições são
                // índices de palavras u32 dentro da página (contrato de WalHashLoc).
                (s_loc.a_hash - s_loc.a_pgno) as i32
            };
            // Índice ordenado deste segmento.
            let mut a_index: Vec<HtSlot> = (0..n_entry).map(|j| j as HtSlot).collect();
            s_loc.i_zero += 1;

            let a_pgno: Vec<u32> =
                wal_wi_page(p_wal, i)[s_loc.a_pgno..s_loc.a_pgno + n_entry as usize].to_vec();
            wal_mergesort(&a_pgno, &mut a_tmp, &mut a_index, &mut n_entry);
            let p_segment = &mut p.a_segment[i as usize];
            p_segment.i_zero = s_loc.i_zero as i32;
            p_segment.n_entry = n_entry;
            p_segment.a_index = a_index;
            p_segment.a_pgno = a_pgno;
        }
        i += 1;
    }
    *pp = if rc != SQLITE_OK { None } else { Some(p) };
    rc
}


// ---- part_005.rs ----

// Na configuração do Debian 13 SQLITE_ENABLE_SETLK_TIMEOUT não está definido: o ramo #else
// do C vale. Os quatro macros viram funções que não fazem nada (sqlite3WalWriteLock e
// sqlite3WalDb não existem nessa configuração).

/// `walEnableBlocking(x)` sem SQLITE_ENABLE_SETLK_TIMEOUT: sempre 0.
#[inline]
pub fn wal_enable_blocking(_p_wal: &mut Wal) -> i32 {
    0
}

/// `walDisableBlocking(x)` sem SQLITE_ENABLE_SETLK_TIMEOUT: não faz nada.
#[inline]
pub fn wal_disable_blocking(_p_wal: &mut Wal) {}

/// `walEnableBlockingMs(pWal, ms)` sem SQLITE_ENABLE_SETLK_TIMEOUT: sempre 0.
#[inline]
pub fn wal_enable_blocking_ms(_p_wal: &mut Wal, _n_ms: i32) -> i32 {
    0
}

/// Tenta obter o lock exclusivo do WAL definido pelos parâmetros `lock_idx` e `n`.
/// Se a tentativa falha e `x_busy` não é `None`, ele é um tratador de ocupado:
/// invoca-o e tenta de novo até o lock ser obtido ou o tratador retornar 0.
/// (`xBusy(pBusyArg)` do C vira um fechamento sem argumento.)
pub fn wal_busy_lock(
    p_wal: &mut Wal,
    mut x_busy: Option<&mut dyn FnMut() -> i32>,
    lock_idx: i32, // Deslocamento do primeiro byte a travar
    n: i32,        // Número de bytes a travar
) -> i32 {
    let mut rc: i32;
    loop {
        rc = wal_lock_exclusive(p_wal, lock_idx, n);
        if !(rc == SQLITE_BUSY
            && match x_busy.as_mut() {
                Some(x) => x() != 0,
                None => false,
            })
        {
            break;
        }
    }
    rc
}

/// O cache do cabeçalho do wal-index precisa ser válido para chamar esta função.
/// Retorna o tamanho de página em bytes usado pelo banco de dados.
pub fn wal_pagesize(p_wal: &Wal) -> i32 {
    let sz = p_wal.hdr.sz_page as i32;
    (sz & 0xfe00) + ((sz & 0x0001) << 16)
}

/// Vale o seguinte quando esta função é chamada:
///
///   a) o lock WRITER é mantido,
///   b) o arquivo de log inteiro passou por checkpoint, e
///   c) quaisquer leitores existentes leem exclusivamente do arquivo de banco de
///      dados: nenhum leitor pode tentar ler um frame do arquivo de log.
///
/// Esta função atualiza as estruturas de memória compartilhada de modo que o
/// próximo cliente a escrever no banco (que pode ser este) o faça gravando frames
/// no início do arquivo de log.
///
/// O valor de `salt1` é usado como aSalt[1] no novo cabeçalho do wal-index. Deve
/// receber um valor pseudoaleatório (obtido de sqlite3_randomness()).
pub fn wal_restart_hdr(p_wal: &mut Wal, salt1: u32) {
    // aSalt[0] é guardado em big-endian: lê-se e grava-se os quatro bytes como
    // sqlite3Get4byte/sqlite3Put4byte, o que em u32 nativo é from_be/to_be.
    p_wal.n_ckpt = p_wal.n_ckpt.wrapping_add(1);
    p_wal.hdr.mx_frame = 0;
    p_wal.hdr.a_salt[0] = u32::from_be(p_wal.hdr.a_salt[0]).wrapping_add(1).to_be();
    p_wal.hdr.a_salt[1] = salt1;
    wal_index_write_hdr(p_wal);
    let p_info = wal_ckpt_info(p_wal);
    p_info.n_backfill = 0;
    p_info.n_backfill_attempted = 0;
    p_info.a_read_mark[1] = 0;
    for i in 2..WAL_NREADER {
        p_info.a_read_mark[i] = READMARK_NOT_USED;
    }
    debug_assert!(p_info.a_read_mark[0] == 0);
}

/// Copia o máximo de conteúdo possível do WAL de volta para o arquivo de banco de
/// dados em resposta a um pedido de sqlite3_wal_checkpoint() ou equivalente.
///
/// A quantidade de informação copiada do WAL para o banco pode ser limitada por
/// leitores ativos. Esta rotina nunca sobrescreve uma página do banco que um
/// leitor concorrente possa estar usando.
///
/// Todas as operações de barreira de E/S (os fsyncs) ocorrem nesta rotina quando o
/// SQLite está em modo WAL com synchronous=NORMAL. Isso significa que, se os
/// checkpoints sempre rodam em uma thread ou processo de segundo plano, as threads
/// de primeiro plano nunca bloqueiam em um fsync demorado.
///
/// O fsync é chamado no WAL antes de escrever conteúdo do WAL no banco. Isso garante
/// que, se o conteúdo novo é persistente no WAL, ele pode ser recuperado após uma
/// queda de energia ou reset forçado.
///
/// O fsync também é chamado no arquivo de banco se (e somente se) todo o conteúdo do
/// WAL foi copiado para o banco. Esse segundo fsync torna seguro apagar o WAL, pois
/// o conteúdo novo vai persistir no arquivo de banco.
///
/// Esta rotina usa e atualiza o campo nBackfill do cabeçalho do wal-index. É a única
/// rotina que aumenta o valor de nBackfill. (Um reset ou recuperação do WAL volta
/// nBackfill a zero, mas não o aumenta.)
///
/// O chamador precisa manter locks suficientes para garantir que nenhum outro
/// checkpoint rode (em outra thread ou processo) ao mesmo tempo.
///
/// `db` é o identificador em que se verifica interrupção; `z_buf` é o buffer
/// temporário (ao menos do tamanho de uma página). Este é o `walCheckpoint` estático
/// do C; o nome `wal_checkpoint` fica com `sqlite3WalCheckpoint`, que o chama.
pub fn wal_checkpoint_static(
    p_wal: &mut Wal,
    db: Option<Sqlite3Ref>,
    e_mode: i32,
    mut x_busy: Option<&mut dyn FnMut() -> i32>,
    sync_flags: i32,
    z_buf: &mut [u8],
) -> i32 {
    let mut rc: i32 = SQLITE_OK; // Código de retorno
    let mut p_iter: Option<Box<WalIterator>> = None; // Contexto do iterador do Wal
    let mut i_dbpage: u32 = 0; // Próxima página do banco a escrever
    let mut i_frame: u32 = 0; // Frame do Wal com os dados de i_dbpage
    // O arquivo de banco é compartilhado com o pager: solta-se a referência no fim.
    let p_db_fd = p_wal.p_db_fd.clone().unwrap();

    let sz_page: i32 = wal_pagesize(p_wal); // Tamanho de página do banco
    'walcheckpoint_out: {
        if wal_ckpt_info(p_wal).n_backfill < p_wal.hdr.mx_frame {
            // EVIDENCE-OF: R-62920-47450 O callback de ocupado nunca é invocado
            // no modo SQLITE_CHECKPOINT_PASSIVE.
            debug_assert!(e_mode != SQLITE_CHECKPOINT_PASSIVE || x_busy.is_none());

            // Calcula em mx_safe_frame o índice do último frame do WAL que é seguro
            // escrever no banco. Frames além de mx_safe_frame poderiam sobrescrever
            // páginas do banco em uso por leitores ativos e portanto não podem
            // passar por backfill a partir do WAL.
            let mut mx_safe_frame: u32 = p_wal.hdr.mx_frame; // Último frame que pode passar por backfill
            let mx_page: u32 = p_wal.hdr.n_page; // Maior página do banco a escrever
            for i in 1..WAL_NREADER {
                let y: u32 = wal_ckpt_info(p_wal).a_read_mark[i];
                if mx_safe_frame > y {
                    debug_assert!(y <= p_wal.hdr.mx_frame);
                    rc = wal_busy_lock(p_wal, x_busy.as_deref_mut(), wal_read_lock(i as u32) as i32, 1);
                    if rc == SQLITE_OK {
                        let i_mark: u32 = if i == 1 { mx_safe_frame } else { READMARK_NOT_USED };
                        wal_ckpt_info(p_wal).a_read_mark[i] = i_mark;
                        wal_unlock_exclusive(p_wal, wal_read_lock(i as u32) as i32, 1);
                    } else if rc == SQLITE_BUSY {
                        mx_safe_frame = y;
                        x_busy = None;
                    } else {
                        break 'walcheckpoint_out;
                    }
                }
            }

            // Aloca o iterador
            if wal_ckpt_info(p_wal).n_backfill < mx_safe_frame {
                let n_backfill_now = wal_ckpt_info(p_wal).n_backfill;
                rc = wal_iterator_init(p_wal, n_backfill_now, &mut p_iter);
                debug_assert!(rc == SQLITE_OK || p_iter.is_none());
            }

            let mut got_read_lock0 = false;
            if p_iter.is_some() {
                rc = wal_busy_lock(p_wal, x_busy.as_deref_mut(), wal_read_lock(0) as i32, 1);
                got_read_lock0 = rc == SQLITE_OK;
            }
            if got_read_lock0 {
                let n_backfill: u32 = wal_ckpt_info(p_wal).n_backfill;
                wal_ckpt_info(p_wal).n_backfill_attempted = mx_safe_frame;

                // Sincroniza o WAL em disco
                rc = os_sync(p_wal.p_wal_fd.as_mut().unwrap(), ckpt_sync_flags(sync_flags));

                // Se o banco pode crescer em consequência deste checkpoint, avisa a
                // camada do VFS sobre o tamanho final esperado do arquivo de banco.
                if rc == SQLITE_OK {
                    let mut n_req: i64 = (mx_page as i64) * (sz_page as i64);
                    let mut n_size: i64 = 0; // Tamanho atual do arquivo de banco
                    os_file_control(&mut p_db_fd.borrow_mut(), SQLITE_FCNTL_CKPT_START, None);
                    rc = os_file_size(&mut p_db_fd.borrow_mut(), &mut n_size);
                    if rc == SQLITE_OK && n_size < n_req {
                        if (n_size + 65536 + (p_wal.hdr.mx_frame as i64) * (sz_page as i64)) < n_req {
                            // Se o tamanho do banco final é maior que o banco atual mais
                            // a quantidade de dados no arquivo wal, mais o tamanho máximo
                            // da página do byte pendente (65536 bytes), deve haver
                            // corrupção em algum lugar.
                            rc = SQLITE_CORRUPT_BKPT;
                        } else {
                            os_file_control_hint(&mut p_db_fd.borrow_mut(), SQLITE_FCNTL_SIZE_HINT, Some(&mut n_req));
                        }
                    }
                }

                // Percorre o conteúdo do WAL, copiando os dados para o arquivo de banco
                while rc == SQLITE_OK && 0 == wal_iterator_next(p_iter.as_mut().unwrap(), &mut i_dbpage, &mut i_frame) {
                    debug_assert!(wal_frame_pgno(p_wal, i_frame) == i_dbpage);
                    if let Some(db) = &db {
                        let (interrupted, malloc_failed) = {
                            let d = db.borrow();
                            (d.u1.is_interrupted != 0, d.malloc_failed != 0)
                        };
                        if interrupted {
                            rc = if malloc_failed { SQLITE_NOMEM_BKPT } else { SQLITE_INTERRUPT };
                            break;
                        }
                    }
                    if i_frame <= n_backfill || i_frame > mx_safe_frame || i_dbpage > mx_page {
                        continue;
                    }
                    let mut i_offset: i64 = wal_frame_offset(i_frame, sz_page as u32) + WAL_FRAME_HDRSIZE as i64;
                    // testcase( IS_BIG_INT(iOffset) ): exigiria um arquivo WAL de 4GiB
                    rc = os_read(p_wal.p_wal_fd.as_mut().unwrap(), &mut z_buf[..sz_page as usize], i_offset);
                    if rc != SQLITE_OK {
                        break;
                    }
                    i_offset = (i_dbpage.wrapping_sub(1) as i64) * (sz_page as i64);
                    rc = os_write(&mut p_db_fd.borrow_mut(), &z_buf[..sz_page as usize], i_offset);
                    if rc != SQLITE_OK {
                        break;
                    }
                }
                os_file_control(&mut p_db_fd.borrow_mut(), SQLITE_FCNTL_CKPT_DONE, None);

                // Se algum trabalho foi realmente feito...
                if rc == SQLITE_OK {
                    if mx_safe_frame == wal_index_hdr(p_wal).mx_frame {
                        let sz_db: i64 = (p_wal.hdr.n_page as i64) * (sz_page as i64);
                        rc = os_truncate(&mut p_db_fd.borrow_mut(), sz_db);
                        if rc == SQLITE_OK {
                            rc = os_sync(&mut p_db_fd.borrow_mut(), ckpt_sync_flags(sync_flags));
                        }
                    }
                    if rc == SQLITE_OK {
                        wal_ckpt_info(p_wal).n_backfill = mx_safe_frame;
                    }
                }

                // Libera o lock de leitor mantido durante o backfill
                wal_unlock_exclusive(p_wal, wal_read_lock(0) as i32, 1);
            }

            if rc == SQLITE_BUSY {
                // Zera o código de retorno para não reportar falha de checkpoint
                // só porque há leitores ativos.
                rc = SQLITE_OK;
            }
        }

        // Se esta é uma operação SQLITE_CHECKPOINT_RESTART ou TRUNCATE, e o arquivo
        // wal inteiro foi copiado para o banco, bloqueia até todos os leitores
        // terminarem de usar o arquivo wal. Isso garante que o próximo processo a
        // escrever no banco reinicie o arquivo wal.
        if rc == SQLITE_OK && e_mode != SQLITE_CHECKPOINT_PASSIVE {
            debug_assert!(p_wal.write_lock != 0);
            if wal_ckpt_info(p_wal).n_backfill < p_wal.hdr.mx_frame {
                rc = SQLITE_BUSY;
            } else if e_mode >= SQLITE_CHECKPOINT_RESTART {
                let mut salt_bytes = [0u8; 4];
                api_randomness(&mut salt_bytes);
                let salt1: u32 = u32::from_ne_bytes(salt_bytes);
                debug_assert!(wal_ckpt_info(p_wal).n_backfill == p_wal.hdr.mx_frame);
                rc = wal_busy_lock(p_wal, x_busy.as_deref_mut(), wal_read_lock(1) as i32, WAL_NREADER as i32 - 1);
                if rc == SQLITE_OK {
                    if e_mode == SQLITE_CHECKPOINT_TRUNCATE {
                        // IMPLEMENTATION-OF: R-44699-57140 Este modo funciona como
                        // SQLITE_CHECKPOINT_RESTART e, além disso, trunca o arquivo de
                        // log para zero bytes logo antes de um retorno bem-sucedido.
                        //
                        // Em teoria seria seguro fazer isso sem atualizar o cabeçalho do
                        // wal-index em memória compartilhada, pois todos os clientes
                        // leitores ou escritores seguintes veriam que o arquivo de log
                        // inteiro passou por checkpoint e se comportariam de acordo.
                        // Parece inseguro, porém, pois deixaria o sistema em um estado em
                        // que o conteúdo do cabeçalho do wal-index não bate com o do
                        // sistema de arquivos. Para evitar isso, atualiza o cabeçalho do
                        // wal-index para indicar que o arquivo de log tem zero frames
                        // válidos.
                        wal_restart_hdr(p_wal, salt1);
                        rc = os_truncate(p_wal.p_wal_fd.as_mut().unwrap(), 0);
                    }
                    wal_unlock_exclusive(p_wal, wal_read_lock(1) as i32, WAL_NREADER as i32 - 1);
                }
            }
        }
    }

    // walcheckpoint_out:
    if let Some(it) = p_iter {
        wal_iterator_free(it);
    }
    rc
}


// ---- part_006.rs ----

/// Se o arquivo WAL é atualmente maior que `n_max` bytes, trunca-o para exatamente
/// `n_max` bytes. Se ocorrer um erro durante isso, ele é ignorado (apenas registrado).
pub fn wal_limit_size(p_wal: &mut Wal, n_max: i64) {
    let mut sz: i64 = 0;
    let mut rx: i32;

    begin_benign_malloc();
    rx = os_file_size(p_wal.p_wal_fd.as_mut().unwrap(), &mut sz);
    if rx == SQLITE_OK && sz > n_max {
        rx = os_truncate(p_wal.p_wal_fd.as_mut().unwrap(), n_max);
    }
    end_benign_malloc();
    if rx != 0 {
        api_log(
            rx,
            b"cannot limit WAL size: %s",
            &[Value::Text(p_wal.z_wal_name.clone())],
        );
    }
}

// As funções walHandleException, walAssertLockmask (versão real) e
// sqlite3WalSystemErrno só existem sob SQLITE_USE_SEH, que é exclusivo do Windows e
// não vale no Debian 13. Sem SEH, walAssertLockmask(x) é o literal 1, e o
// sqlite3WalSystemErrno de wal.h é a constante 0. Por isso nada disso é traduzido.

/// Fecha uma conexão com um arquivo de log.
///
/// `p_wal` é consumido (o `sqlite3_free(pWal)` do C é a soltura do `Box`).
pub fn wal_close(
    p_wal: Option<Box<Wal>>,
    db: Option<&Sqlite3Ref>, // Para a flag de interrupção
    sync_flags: i32,         // Flags para OsSync() (ou 0)
    n_buf: i32,
    z_buf: Option<&mut [u8]>, // Buffer de ao menos n_buf bytes
) -> i32 {
    let mut rc = SQLITE_OK;
    if let Some(mut p_wal) = p_wal {
        let mut is_delete: i32 = 0; // Verdadeiro para apagar os arquivos wal e wal-index

        // Se um lock EXCLUSIVE pode ser obtido no arquivo de banco (usando os métodos
        // de lock comuns do modo rollback), isso garante que a conexão associada a
        // este arquivo de log é a única conexão com o banco. Neste caso faz checkpoint
        // do banco e apaga os arquivos wal e wal-index.
        //
        // O lock EXCLUSIVE não é liberado antes de retornar.
        if let Some(z_buf) = z_buf {
            let p_db_fd = p_wal.p_db_fd.clone().unwrap();
            rc = os_lock(&mut p_db_fd.borrow_mut(), SQLITE_LOCK_EXCLUSIVE);
            if rc == SQLITE_OK {
                if p_wal.exclusive_mode == WAL_NORMAL_MODE {
                    p_wal.exclusive_mode = WAL_EXCLUSIVE_MODE;
                }
                rc = wal_checkpoint(
                    &mut p_wal,
                    db.cloned(),
                    SQLITE_CHECKPOINT_PASSIVE,
                    None,
                    sync_flags,
                    n_buf,
                    z_buf,
                    None,
                    None,
                );
                if rc == SQLITE_OK {
                    let mut b_persist: i32 = -1;
                    os_file_control_hint(&mut p_db_fd.borrow_mut(), SQLITE_FCNTL_PERSIST_WAL, Some(&mut b_persist));
                    if b_persist != 1 {
                        // Tenta apagar o arquivo WAL se o checkpoint terminou e deu
                        // fsync (rc==SQLITE_OK) e se não estamos em modo persistent-wal
                        // (!b_persist)
                        is_delete = 1;
                    } else if p_wal.mx_wal_size >= 0 {
                        // Tenta truncar o arquivo WAL para zero bytes se o checkpoint
                        // terminou e deu fsync (rc==SQLITE_OK), se estamos em modo
                        // persistent WAL (b_persist) e se o PRAGMA journal_size_limit é
                        // não negativo (p_wal.mx_wal_size>=0). Trunca-se para zero bytes
                        // porque truncar para o journal_size_limit poderia deixar um
                        // arquivo WAL corrompido em disco.
                        wal_limit_size(&mut p_wal, 0);
                    }
                }
            }
        }

        wal_index_close(&mut p_wal, is_delete);
        os_close(p_wal.p_wal_fd.as_mut().unwrap());
        if is_delete != 0 {
            begin_benign_malloc();
            os_delete(&**p_wal.p_vfs.as_ref().unwrap(), &p_wal.z_wal_name, 0);
            end_benign_malloc();
        }
        // p_wal.ap_wi_data e o próprio p_wal são soltos ao sair do escopo.
    }
    rc
}

/// Tenta ler o cabeçalho do wal-index. Retorna 0 em caso de sucesso e 1 se houver
/// um problema.
///
/// O wal-index está em memória compartilhada. Outra thread ou processo pode estar
/// escrevendo o cabeçalho ao mesmo tempo em que este procedimento tenta lê-lo, o
/// que pode gerar inconsistência. Uma leitura suja é detectada verificando que as
/// duas cópias do cabeçalho são iguais e também por um checksum do cabeçalho.
///
/// Se e somente se a leitura é consistente e o cabeçalho é diferente de `p_wal.hdr`,
/// então `p_wal.hdr` é atualizado com o conteúdo do novo cabeçalho e `*p_changed`
/// recebe 1.
///
/// Se o checksum não pode ser verificado retorna não zero. Se o cabeçalho é lido
/// com sucesso e o checksum confere, retorna zero.
pub fn wal_index_try_hdr(p_wal: &mut Wal, p_changed: &mut i32) -> i32 {
    let mut a_cksum: [u32; 2] = [0, 0]; // Checksum do conteúdo do cabeçalho

    // A primeira página do wal-index precisa estar mapeada neste ponto.
    debug_assert!(p_wal.n_wi_data > 0 && p_wal.ap_wi_data[0].is_some());

    // Lê o cabeçalho. Isso pode acontecer concorrentemente com uma escrita na mesma
    // área de memória compartilhada em outra CPU de um SMP, o que significa que é
    // possível ler um instantâneo inconsistente do arquivo. Se isso acontecer,
    // retorna não zero.
    //
    // tag-20200519-1:
    // Há duas cópias do cabeçalho no início do wal-index. Na leitura, lê-se [0] e
    // depois [1]. As escritas são na ordem inversa. Barreiras de memória impedem o
    // compilador e o hardware de reordenar leituras e escritas. As duas cópias
    // ficam em `wi_hdr` (visão tipada da página 0).
    let h1 = p_wal.wi_hdr[0].clone();
    wal_shm_barrier(p_wal);
    let h2 = p_wal.wi_hdr[1].clone();

    if h1 != h2 {
        return 1; // Leitura suja
    }
    if h1.is_init == 0 {
        return 1; // Cabeçalho malformado, provavelmente só zeros
    }
    // Checksum sobre sizeof(h1)-sizeof(h1.aCksum) = 40 bytes de h1, na mesma
    // disposição de memória da struct do C.
    let h1_bytes = wal_index_hdr_prefix_bytes(&h1);
    wal_checksum_bytes(1, &h1_bytes, 40, None, &mut a_cksum);
    if a_cksum[0] != h1.a_cksum[0] || a_cksum[1] != h1.a_cksum[1] {
        return 1; // O checksum não confere
    }

    if p_wal.hdr != h1 {
        *p_changed = 1;
        p_wal.hdr = h1;
        let sz = p_wal.hdr.sz_page as u32;
        p_wal.sz_page = (sz & 0xfe00) + ((sz & 0x0001) << 16);
    }

    // O cabeçalho foi lido com sucesso. Retorna zero.
    0
}

/// Valor que wal_try_begin_read retorna quando precisa ser repetida.
pub const WAL_RETRY: i32 = -1;

/// Lê o cabeçalho do wal-index do wal-index para `p_wal.hdr`. Se o cabeçalho do wal
/// parece corrompido, tenta reconstruir o wal-index a partir do WAL antes de retornar.
///
/// Define `*p_changed` como 1 se o valor do cabeçalho do wal-index em `p_wal.hdr`
/// foi mudado por esta operação. Se `p_wal.hdr` não mudou, define `*p_changed` como 0.
///
/// Se o cabeçalho do wal-index é lido com sucesso, retorna SQLITE_OK. Caso contrário
/// retorna um código de erro do SQLite.
pub fn wal_index_read_hdr(p_wal: &mut Wal, p_changed: &mut i32) -> i32 {
    let mut rc: i32; // Código de retorno
    let mut bad_hdr: i32; // Verdadeiro se a leitura do cabeçalho falhou

    // Garante que a página 0 do wal-index (a que contém o cabeçalho) está mapeada.
    // Retorna cedo se ocorrer um erro aqui. Depois da chamada a página fica em
    // ap_wi_data[0], ou None (o `page0==0` do C).
    rc = wal_index_page(p_wal, 0);
    if rc != SQLITE_OK {
        debug_assert!(rc != SQLITE_READONLY); // READONLY vira OK em wal_index_page
        if rc == SQLITE_READONLY_CANTINIT {
            // O retorno SQLITE_READONLY_CANTINIT significa que a memória compartilhada
            // pôde ser aberta mas não é gravável, e esta thread não consegue confirmar
            // que outra conexão com capacidade de escrita a tem aberta; logo o conteúdo
            // da memória compartilhada não é confiável, pois pode estar inconsistente
            // com o arquivo WAL e não há escritor à mão para consertá-la.
            debug_assert!(p_wal.ap_wi_data[0].is_none());
            debug_assert!(p_wal.write_lock == 0);
            debug_assert!((p_wal.read_only & WAL_SHM_RDONLY) != 0);
            p_wal.b_shm_unreliable = 1;
            p_wal.exclusive_mode = WAL_HEAPMEMORY_MODE;
            *p_changed = 1;
        } else {
            return rc; // Qualquer outro retorno diferente de OK é só um erro
        }
    }
    // page0 pode ser nula se a SHM tem zero bytes e write_lock é zero, o que impede
    // a SHM de crescer.
    let page0_mapped = p_wal.ap_wi_data[0].is_some();
    debug_assert!(page0_mapped || p_wal.write_lock == 0);

    // Se a primeira página do wal-index foi mapeada, tenta ler o cabeçalho do
    // wal-index imediatamente, sem manter nenhum lock. Isso costuma funcionar, mas
    // pode falhar se o cabeçalho está corrompido ou sendo modificado por outra
    // thread ou processo.
    bad_hdr = if page0_mapped { wal_index_try_hdr(p_wal, p_changed) } else { 1 };

    // Se a primeira tentativa falhou, pode ter sido por uma corrida com um escritor.
    // Então obtém um lock de ESCRITA e tenta de novo.
    if bad_hdr != 0 {
        if p_wal.b_shm_unreliable == 0 && (p_wal.read_only & WAL_SHM_RDONLY) != 0 {
            rc = wal_lock_shared(p_wal, WAL_WRITE_LOCK as i32);
            if rc == SQLITE_OK {
                wal_unlock_shared(p_wal, WAL_WRITE_LOCK as i32);
                rc = SQLITE_READONLY_RECOVERY;
            }
        } else {
            let b_write_lock: u8 = p_wal.write_lock;
            let mut locked = b_write_lock != 0;
            if !locked {
                rc = wal_lock_exclusive(p_wal, WAL_WRITE_LOCK as i32, 1);
                locked = rc == SQLITE_OK;
            }
            if locked {
                p_wal.write_lock = 1;
                rc = wal_index_page(p_wal, 0);
                if rc == SQLITE_OK {
                    bad_hdr = wal_index_try_hdr(p_wal, p_changed);
                    if bad_hdr != 0 {
                        // Se o cabeçalho do wal-index continua malformado mesmo com o
                        // lock de ESCRITA, só pode significar que ele está corrompido e
                        // precisa ser reconstruído. Então roda a recuperação para fazer
                        // exatamente isso. Desativa antes os locks bloqueantes.
                        wal_disable_blocking(p_wal);
                        rc = wal_index_recover(p_wal);
                        *p_changed = 1;
                    }
                }
                if b_write_lock == 0 {
                    p_wal.write_lock = 0;
                    wal_unlock_exclusive(p_wal, WAL_WRITE_LOCK as i32, 1);
                }
            }
        }
    }

    // Se o cabeçalho foi lido com sucesso, confere o número de versão para garantir
    // que o wal-index não foi construído com algum formato futuro que esta versão do
    // SQLite não entende.
    if bad_hdr == 0 && p_wal.hdr.i_version != WALINDEX_MAX_VERSION {
        rc = SQLITE_CANTOPEN_BKPT;
    }
    if p_wal.b_shm_unreliable != 0 {
        if rc != SQLITE_OK {
            wal_index_close(p_wal, 0);
            p_wal.b_shm_unreliable = 0;
            debug_assert!(p_wal.n_wi_data > 0 && p_wal.ap_wi_data[0].is_none());
            // wal_index_recover() pode ter retornado SHORT_READ se um escritor
            // concorrente truncou o WAL por baixo dele. Se isso acontecer, indica que
            // um escritor consertou o arquivo SHM para nós, então tenta de novo.
            if rc == SQLITE_IOERR_SHORT_READ {
                rc = WAL_RETRY;
            }
        }
        p_wal.exclusive_mode = WAL_NORMAL_MODE;
    }

    rc
}


// ---- part_007.rs ----

// Na configuração do Debian 13 não valem SQLITE_ENABLE_SETLK_TIMEOUT nem
// SQLITE_ENABLE_SNAPSHOT: os ramos desses #ifdef somem (os bloqueios com timeout
// viram as funções vazias de part_005, e `p_snapshot` não entra na escolha do mxFrame).

/// Limite de tentativas antes de wal_try_begin_read devolver SQLITE_PROTOCOL.
pub const WAL_RETRY_PROTOCOL_LIMIT: i32 = 100;

/// Sem SQLITE_ENABLE_SETLK_TIMEOUT a máscara é zero.
pub const WAL_RETRY_BLOCKED_MASK: i32 = 0;

/// Abre uma transação em uma conexão onde a memória compartilhada é somente leitura
/// e onde não é possível verificar que há uma conexão separada com capacidade de
/// escrita à mão para manter a memória compartilhada atualizada com o arquivo WAL.
///
/// Isso pode acontecer, por exemplo, quando a memória compartilhada é implementada
/// por mapeamento de memória de um arquivo `*-shm`, em que um escritor anterior
/// desligou e deixou o arquivo `*-shm` em disco, e agora a conexão presente tenta
/// usar esse banco sem permissão de escrita nele. Outros cenários também são
/// possíveis, dependendo da implementação do VFS.
///
/// Pré-condição:
///
///    O arquivo `*-wal` foi lido e um wal-index apropriado foi construído em
///    `p_wal.ap_wi_data` usando memória de heap em vez de memória compartilhada.
///
/// Se esta função retorna SQLITE_OK, a transação de leitura foi aberta com sucesso.
/// Neste caso a variável de saída `*p_changed` recebe verdadeiro antes de retornar
/// se o chamador deve descartar o conteúdo do cache de páginas antes de prosseguir.
/// Ou, se retorna WAL_RETRY, o wal-index em memória de heap foi descartado e o
/// chamador deve tentar abrir a transação de leitura desde o início (inclusive
/// tentando mapear o arquivo `*-shm`).
///
/// Se ocorrer um erro, um código de erro do SQLite é retornado.
pub fn wal_begin_shm_unreliable(p_wal: &mut Wal, p_changed: &mut i32) -> i32 {
    let mut sz_wal: i64 = 0; // Tamanho do arquivo wal em disco, em bytes
    let mut a_buf = [0u8; WAL_HDRSIZE]; // Buffer para carregar o cabeçalho do WAL
    let mut rc: i32; // Código de retorno
    let p_db_fd = p_wal.p_db_fd.clone().unwrap();

    debug_assert!(p_wal.b_shm_unreliable != 0);
    debug_assert!((p_wal.read_only & WAL_SHM_RDONLY) != 0);
    debug_assert!(p_wal.n_wi_data > 0 && p_wal.ap_wi_data[0].is_some());

    'begin_unreliable_shm_out: {
        // Obtém WAL_READ_LOCK(0). Isso impede que qualquer escritor rode um
        // checkpoint, mas não os impede de rodar a recuperação.
        rc = wal_lock_shared(p_wal, wal_read_lock(0) as i32);
        if rc != SQLITE_OK {
            if rc == SQLITE_BUSY {
                rc = WAL_RETRY;
            }
            break 'begin_unreliable_shm_out;
        }
        p_wal.read_lock = 0;

        // Verifica se um escritor separado se anexou à área de memória compartilhada,
        // tornando-a "confiável" de novo. Faz isso invocando a rotina xShmMap() do
        // VFS e vendo se o retorno é SQLITE_READONLY em vez de
        // SQLITE_READONLY_CANTINIT.
        //
        // Se a memória compartilhada agora é "confiável" retorna WAL_RETRY, o que
        // faz o wal-index em memória de heap ser descartado e a memória compartilhada
        // real ser usada no lugar.
        //
        // Este passo é importante porque, mesmo que esta conexão segure o
        // WAL_READ_LOCK(0), que impede um checkpoint, um escritor pode já ter feito
        // checkpoint do arquivo WAL e, enquanto a conexão atual está ativa, dar a
        // volta no WAL e começar a sobrescrever frames que este processo quer usar.
        //
        // Depois que sqlite3OsShmMap() foi chamado para um sqlite3_file e retornou
        // qualquer valor SQLITE_READONLY, ele só pode retornar SQLITE_READONLY,
        // SQLITE_READONLY_CANTINIT ou algum erro em todas as chamadas seguintes,
        // mesmo que um agente externo faça um "chmod" para tornar a memória
        // compartilhada gravável por nós, até sqlite3OsShmUnmap() ser chamado. Isto
        // é um requisito sobre a implementação do VFS.
        let mut p_dummy: Option<Rc<RefCell<Vec<u8>>>> = None;
        rc = os_shm_map(&mut p_db_fd.borrow_mut(), 0, WALINDEX_PGSZ as i32, 0, &mut p_dummy);
        debug_assert!(rc != SQLITE_OK); // SQLITE_OK não é possível em conexão somente leitura
        if rc != SQLITE_READONLY_CANTINIT {
            rc = if rc == SQLITE_READONLY { WAL_RETRY } else { rc };
            break 'begin_unreliable_shm_out;
        }

        // Só chegamos aqui se a memória compartilhada real ainda é não confiável.
        // Assume que o substituto do wal-index em memória está correto e o carrega
        // em p_wal.hdr.
        p_wal.hdr = wal_index_hdr(p_wal).clone();

        // Garante que nenhum escritor entrou e mudou o arquivo WAL por baixo de nós,
        // e depois se desconectou, enquanto não estávamos olhando.
        rc = os_file_size(p_wal.p_wal_fd.as_mut().unwrap(), &mut sz_wal);
        if rc != SQLITE_OK {
            break 'begin_unreliable_shm_out;
        }
        if sz_wal < WAL_HDRSIZE as i64 {
            // Se o arquivo wal é pequeno demais para conter um cabeçalho wal e o
            // cabeçalho do wal-index tem mxFrame==0, deve ser seguro prosseguir
            // lendo só o arquivo de banco. Porém o cache de páginas não é confiável,
            // pois uma conexão de leitura e escrita pode ter conectado, escrito no
            // banco, rodado um checkpoint, truncado o arquivo wal e desconectado
            // desde a última transação de leitura deste cliente.
            *p_changed = 1;
            rc = if p_wal.hdr.mx_frame == 0 { SQLITE_OK } else { WAL_RETRY };
            break 'begin_unreliable_shm_out;
        }

        // Confere se as chaves salt no início do arquivo wal ainda coincidem.
        rc = os_read(p_wal.p_wal_fd.as_mut().unwrap(), &mut a_buf, 0);
        if rc != SQLITE_OK {
            break 'begin_unreliable_shm_out;
        }
        let mut salt_bytes = [0u8; 8]; // memcmp(&pWal->hdr.aSalt, &aBuf[16], 8)
        salt_bytes[..4].copy_from_slice(&p_wal.hdr.a_salt[0].to_ne_bytes());
        salt_bytes[4..].copy_from_slice(&p_wal.hdr.a_salt[1].to_ne_bytes());
        if salt_bytes != a_buf[16..24] {
            // Algum escritor deu a volta no arquivo WAL enquanto não olhávamos.
            // Retorna WAL_RETRY, o que faz o wal-index em memória ser reconstruído.
            rc = WAL_RETRY;
            break 'begin_unreliable_shm_out;
        }

        // Aloca um buffer para ler os frames
        debug_assert!((p_wal.sz_page & (p_wal.sz_page - 1)) == 0);
        debug_assert!(p_wal.sz_page >= 512 && p_wal.sz_page <= 65536);
        let sz_frame: i32 = p_wal.sz_page as i32 + WAL_FRAME_HDRSIZE as i32; // Bytes em a_frame
        let mut a_frame: Vec<u8> = vec![0u8; sz_frame as usize]; // Buffer para carregar um frame inteiro

        // Verifica se uma transação completa foi anexada ao arquivo wal desde que o
        // wal-index em memória de heap foi criado. Se sim, o wal-index em heap é
        // descartado e WAL_RETRY é retornado ao chamador.
        let a_save_cksum: [u32; 2] = p_wal.hdr.a_frame_cksum;
        let mut i_offset: i64 = wal_frame_offset(p_wal.hdr.mx_frame + 1, p_wal.sz_page);
        while i_offset + sz_frame as i64 <= sz_wal {
            let mut pgno: u32 = 0; // Número de página do banco para o frame
            let mut n_truncate: u32 = 0; // Campo dbsize do cabeçalho do frame

            // Lê e decodifica o próximo frame do log.
            rc = os_read(p_wal.p_wal_fd.as_mut().unwrap(), &mut a_frame, i_offset);
            if rc != SQLITE_OK {
                break;
            }
            if wal_decode_frame(p_wal, &mut pgno, &mut n_truncate, &a_frame[WAL_FRAME_HDRSIZE..], &a_frame) == 0 {
                break;
            }

            // Se n_truncate é diferente de zero, uma transação completa foi anexada
            // a este arquivo wal. Define rc como WAL_RETRY e sai do laço.
            if n_truncate != 0 {
                rc = WAL_RETRY;
                break;
            }
            i_offset += sz_frame as i64;
        }
        p_wal.hdr.a_frame_cksum = a_save_cksum;
    }

    // begin_unreliable_shm_out:
    if rc != SQLITE_OK {
        for i in 0..p_wal.n_wi_data as usize {
            p_wal.ap_wi_data[i] = None;
        }
        p_wal.b_shm_unreliable = 0;
        wal_end_read_transaction(p_wal);
        *p_changed = 1;
    }
    rc
}

/// Tenta iniciar uma transação de leitura. Pode falhar por uma corrida ou outra
/// condição transitória. Quando isso acontece, retorna WAL_RETRY para indicar ao
/// chamador que é seguro tentar de novo imediatamente.
///
/// Em caso de sucesso retorna SQLITE_OK. Em uma falha permanente (como um erro de
/// E/S ou um SQLITE_BUSY porque outro processo está rodando a recuperação) retorna
/// um código de erro positivo.
///
/// O parâmetro `use_wal` é verdadeiro para forçar o uso do WAL e desabilitar o caso
/// em que o WAL é ignorado por ter sido completamente copiado para o banco. Se
/// `use_wal==0` esta rotina chama wal_index_read_hdr() para copiar o cabeçalho do
/// wal-index em `p_wal.hdr`. Se o cabeçalho mudou, `*p_changed` recebe 1 (como
/// indicação ao chamador de que o cache de páginas local está obsoleto e precisa ser
/// descartado). Quando `use_wal==1` assume-se que o cabeçalho do wal-index já foi
/// carregado e o parâmetro `p_changed` não é usado.
///
/// O chamador precisa definir `*p_cnt` como o número de chamadas anteriores a esta
/// rotina, durante a tentativa de leitura atual, que retornaram WAL_RETRY. Esta
/// rotina passa a tomar medidas mais agressivas para limpar as condições de corrida
/// depois de vários retornos WAL_RETRY e, depois de um número excessivo de erros,
/// acaba retornando SQLITE_PROTOCOL. O retorno SQLITE_PROTOCOL indica que algum
/// outro processo saiu do controle e não está respeitando o protocolo de locks. Há
/// uma chance minúscula de SQLITE_PROTOCOL ser retornado por uma sequência de azar
/// quando há muita contenção pelo wal-index, mas essa possibilidade é tão pequena
/// que pode ser desprezada com segurança, acreditamos.
///
/// Em caso de sucesso, esta rotina obtém um lock de leitura em
/// WAL_READ_LOCK(p_wal.read_lock). O inteiro `read_lock` fica no intervalo
/// 0 <= read_lock < WAL_NREADER. Se `read_lock==-1` o Wal não segura nenhum lock de
/// leitura. O leitor não pode acessar nenhuma página do banco modificada por um
/// frame WAL até e incluindo o frame número a_read_mark[read_lock]. O leitor usa
/// frames WAL até e incluindo `hdr.mx_frame` se `read_lock>0`. Se `read_lock==0`, o
/// leitor ignora o WAL por completo e obtém todo o conteúdo direto do arquivo de
/// banco. Se `use_wal` é 1, o WAL nunca é ignorado e esta rotina sempre define
/// `read_lock>0` em caso de sucesso. Quando a transação de leitura termina, o
/// chamador precisa soltar o lock em WAL_READ_LOCK(read_lock) e definir `read_lock`
/// como -1.
///
/// Esta rotina usa os campos n_backfill e a_read_mark[] do cabeçalho para escolher
/// um WAL_READ_LOCK() particular que se esforça para deixar o processo de checkpoint
/// fazer o máximo de trabalho. Pode atualizar valores do array a_read_mark[] no
/// cabeçalho, mas, se o faz, tem o cuidado de manter um lock exclusivo no
/// WAL_READ_LOCK() correspondente enquanto muda os valores.
pub fn wal_try_begin_read(p_wal: &mut Wal, p_changed: &mut i32, use_wal: i32, p_cnt: &mut i32) -> i32 {
    let mut rc: i32 = SQLITE_OK; // Código de retorno

    debug_assert!(p_wal.read_lock < 0); // Não está travado no momento

    // use_wal só pode ser definido para conexões de leitura e escrita
    debug_assert!((p_wal.read_only & WAL_SHM_RDONLY) == 0 || use_wal == 0);

    // Toma medidas para evitar girar para sempre se houver um erro de protocolo.
    //
    // As circunstâncias que causam um RETRY só deveriam durar pouquíssimo tempo.
    // Nenhuma E/S ou outra chamada de sistema é feita enquanto os locks são mantidos,
    // então os locks não devem ficar presos por muito tempo. Mas, com azar, outro
    // processo que segura um lock pode ser retirado da memória ou tomar uma falha de
    // página demorada de resolver, durante os poucos nanossegundos em que segura o
    // lock. Nesse caso, pode levar mais que o normal para o lock ser liberado.
    //
    // Depois de 5 RETRYs começa-se a chamar sqlite3OsSleep(). As primeiras chamadas
    // têm atraso de 1 microssegundo. Na verdade isso é mais um yield do escalonador
    // que um atraso de fato. Mas da 10a tentativa em diante os atrasos vão ficando
    // cada vez mais longos, de modo que na 100a (e última) RETRY o atraso é de 323
    // milissegundos. O tempo total de atraso antes de desistir é menor que 10 segundos.
    *p_cnt += 1;
    if *p_cnt > 5 {
        let mut n_delay: i32 = 1; // Tempo de pausa em microssegundos
        let cnt: i32 = *p_cnt & !WAL_RETRY_BLOCKED_MASK;
        if cnt > WAL_RETRY_PROTOCOL_LIMIT {
            return SQLITE_PROTOCOL;
        }
        if *p_cnt >= 10 {
            n_delay = (cnt - 9) * (cnt - 9) * 39;
        }
        os_sleep(&**p_wal.p_vfs.as_ref().unwrap(), n_delay);
        *p_cnt &= !WAL_RETRY_BLOCKED_MASK;
    }

    if use_wal == 0 {
        debug_assert!(rc == SQLITE_OK);
        if p_wal.b_shm_unreliable == 0 {
            rc = wal_index_read_hdr(p_wal, p_changed);
        }
        if rc == SQLITE_BUSY {
            // Se não há recuperação rodando em outra thread ou processo, converte
            // erros BUSY em WAL_RETRY. Se se sabe que há recuperação rodando,
            // converte BUSY em BUSY_RECOVERY. Há uma corrida aqui que pode fazer
            // WAL_RETRY ser retornado mesmo que BUSY_RECOVERY fosse tecnicamente
            // correto. Mas a corrida é benigna, pois com WAL_RETRY esta rotina será
            // chamada de novo e provavelmente acertará na segunda iteração.
            if p_wal.ap_wi_data.first().map_or(true, |p| p.is_none()) {
                // Este ramo é tomado quando o método xShmMap() retorna SQLITE_BUSY.
                // Assume-se que é uma condição transitória, então retorna WAL_RETRY.
                // A implementação de xShmMap() dos módulos padrão unix e win32 pode
                // retornar SQLITE_BUSY por uma condição de corrida no código que
                // decide se a região de memória compartilhada deve ser zerada antes
                // de a página pedida ser devolvida.
                rc = WAL_RETRY;
            } else {
                rc = wal_lock_shared(p_wal, WAL_RECOVER_LOCK as i32);
                if rc == SQLITE_OK {
                    wal_unlock_shared(p_wal, WAL_RECOVER_LOCK as i32);
                    rc = WAL_RETRY;
                } else if rc == SQLITE_BUSY {
                    rc = SQLITE_BUSY_RECOVERY;
                }
            }
        }
        if rc != SQLITE_OK {
            return rc;
        } else if p_wal.b_shm_unreliable != 0 {
            return wal_begin_shm_unreliable(p_wal, p_changed);
        }
    }

    debug_assert!(p_wal.n_wi_data > 0);
    debug_assert!(p_wal.ap_wi_data[0].is_some());
    if use_wal == 0 && wal_ckpt_info(p_wal).n_backfill == p_wal.hdr.mx_frame {
        // O WAL foi completamente copiado para o banco (ou está vazio) e pode ser
        // ignorado com segurança.
        rc = wal_lock_shared(p_wal, wal_read_lock(0) as i32);
        wal_shm_barrier(p_wal);
        if rc == SQLITE_OK {
            if *wal_index_hdr(p_wal) != p_wal.hdr {
                // Não é seguro deixar o leitor continuar aqui se frames podem ter
                // sido anexados ao log antes de READ_LOCK(0) ser obtido. Ao segurar
                // READ_LOCK(0), o leitor ignora o arquivo de log inteiro, o que
                // implica que o arquivo de banco contém um instantâneo confiável.
                // Como segurar READ_LOCK(0) impede um checkpoint, isso costuma ser
                // correto.
                //
                // Porém, se frames foram anexados ao log (ou se o log deu a volta e
                // foi escrito, para todos os efeitos) antes de READ_LOCK(0) ser
                // obtido, isso não é necessariamente verdade. Um checkpointer pode
                // ter começado a copiar os frames anexados e travado antes de
                // terminar, deixando uma imagem corrompida no arquivo de banco.
                wal_unlock_shared(p_wal, wal_read_lock(0) as i32);
                return WAL_RETRY;
            }
            p_wal.read_lock = 0;
            return SQLITE_OK;
        } else if rc != SQLITE_BUSY {
            return rc;
        }
    }

    // Se chegamos até aqui, o leitor vai querer usar o WAL para obter conteúdo de
    // commits recentes. O trabalho agora é escolher uma das entradas a_read_mark[]
    // mais próxima de hdr.mx_frame sem ultrapassá-la, e travar essa entrada.
    let mut mx_read_mark: u32 = 0; // Maior valor de a_read_mark[]
    let mut mx_i: usize = 0; // Índice do maior valor de a_read_mark[]
    let mx_frame: u32 = p_wal.hdr.mx_frame; // Frame do Wal a travar
    for i in 1..WAL_NREADER {
        let this_mark: u32 = wal_ckpt_info(p_wal).a_read_mark[i];
        if mx_read_mark <= this_mark && this_mark <= mx_frame {
            debug_assert!(this_mark != READMARK_NOT_USED);
            mx_read_mark = this_mark;
            mx_i = i;
        }
    }
    if (p_wal.read_only & WAL_SHM_RDONLY) == 0 && (mx_read_mark < mx_frame || mx_i == 0) {
        for i in 1..WAL_NREADER {
            rc = wal_lock_exclusive(p_wal, wal_read_lock(i as u32) as i32, 1);
            if rc == SQLITE_OK {
                wal_ckpt_info(p_wal).a_read_mark[i] = mx_frame;
                mx_read_mark = mx_frame;
                mx_i = i;
                wal_unlock_exclusive(p_wal, wal_read_lock(i as u32) as i32, 1);
                break;
            } else if rc != SQLITE_BUSY {
                return rc;
            }
        }
    }
    if mx_i == 0 {
        debug_assert!(rc == SQLITE_BUSY || (p_wal.read_only & WAL_SHM_RDONLY) != 0);
        return if rc == SQLITE_BUSY { WAL_RETRY } else { SQLITE_READONLY_CANTINIT };
    }

    // (void)walEnableBlockingMs(pWal, nBlockTmout) vale 0 e walDisableBlocking(pWal)
    // não faz nada nesta configuração.
    rc = wal_lock_shared(p_wal, wal_read_lock(mx_i as u32) as i32);
    if rc != 0 {
        debug_assert!(rc != SQLITE_BUSY_TIMEOUT);
        debug_assert!((rc & 0xFF) != SQLITE_BUSY || rc == SQLITE_BUSY || rc == SQLITE_BUSY_TIMEOUT);
        return if (rc & 0xFF) == SQLITE_BUSY { WAL_RETRY } else { rc };
    }
    // Agora que o lock de leitura foi obtido, confere que nem o valor no array
    // a_read_mark[] nem o conteúdo do cabeçalho do wal-index mudaram.
    //
    // É preciso conferir que o cabeçalho do wal-index não mudou entre o momento em
    // que foi lido e a obtenção do lock compartilhado em WAL_READ_LOCK(mx_i), para
    // levar em conta a possibilidade de o arquivo de log ter dado a volta por um
    // escritor, ou de frames posteriores a hdr.mx_frame terem sido copiados para o
    // banco por um checkpointer. Se uma dessas coisas aconteceu, ler o banco com o
    // valor atual de hdr.mx_frame arrisca ler um instantâneo corrompido. Então tenta
    // de novo.
    //
    // Antes de conferir que o cabeçalho vivo do wal-index não mudou desde a leitura,
    // define Wal.min_frame como o primeiro frame do arquivo wal que ainda não passou
    // por checkpoint. Este cliente não precisará ler frames anteriores a min_frame do
    // arquivo wal: eles podem ser lidos com segurança direto do arquivo de banco.
    //
    // Como há uma chamada ShmBarrier() entre tomar a cópia de n_backfill e conferir
    // que o cabeçalho do wal em memória compartilhada ainda coincide com o guardado em
    // p_wal.hdr, é garantido que o checkpointer que definiu n_backfill não trabalhava
    // com um cabeçalho de wal-index mais novo que o guardado em p_wal.hdr. Se
    // trabalhasse, isso poderia causar um problema. O checkpointer poderia deixar de
    // copiar uma versão da página X anterior a p_wal.min_frame (chame-a de versão A)
    // por haver uma versão mais nova (versão B) da mesma página mais adiante no
    // arquivo wal. Mas, se a versão B estiver depois do frame p_wal.hdr.mx_frame, o
    // cliente assumiria incorretamente que pode ler a versão A do arquivo de banco.
    // Porém, como podemos garantir que o checkpointer que definiu n_backfill não via
    // nenhuma página além de p_wal.hdr.mx_frame, esse problema não surge.
    p_wal.min_frame = wal_ckpt_info(p_wal).n_backfill + 1;
    wal_shm_barrier(p_wal);
    if wal_ckpt_info(p_wal).a_read_mark[mx_i] != mx_read_mark || *wal_index_hdr(p_wal) != p_wal.hdr {
        wal_unlock_shared(p_wal, wal_read_lock(mx_i as u32) as i32);
        return WAL_RETRY;
    } else {
        debug_assert!(mx_read_mark <= p_wal.hdr.mx_frame);
        p_wal.read_lock = mx_i as i16;
    }
    rc
}


// ---- part_008.rs ----

// Notas de tradução deste trecho (chunks/wal_c.008.c):
//
// - `SQLITE_ENABLE_SNAPSHOT` não está na lista de opções do Debian 13, então
//   `walSnapshotRecover`, `sqlite3WalSnapshotRecover` e todos os ramos de snapshot
//   de `walBeginReadTransaction` somem.
// - `SEH_TRY`/`SEH_EXCEPT` só existem sob `SQLITE_USE_SEH` (Windows). Fora dele os
//   invólucros públicos `sqlite3WalBeginReadTransaction` e `sqlite3WalFindFrame` são
//   idênticos às funções estáticas que chamam, e os dois nomes colidiriam na regra de
//   nomes. Cada par vira uma única função, com o nome público.

/// Inicia uma transação de leitura no banco de dados.
///
/// Esta rotina chamava-se sqlite3OpenSnapshot(): ela tira um instantâneo do
/// estado do WAL e do wal-index no instante atual. A thread corrente continua
/// usando esse instantâneo. Outras threads podem anexar conteúdo novo ao WAL e ao
/// wal-index, mas esse conteúdo extra é ignorado pela thread corrente.
///
/// Se o conteúdo do banco mudou desde a transação de leitura anterior,
/// `*p_changed` vale 1 ao retornar. A camada do Pager usa isso para saber que o
/// cache está velho e precisa ser descartado.
pub fn wal_begin_read_transaction(p_wal: &mut Wal, p_changed: &mut i32) -> i32 {
    // Número de tentativas de TryBeginRead
    let mut cnt: i32 = 0;

    assert!(p_wal.ckpt_lock == 0);

    let mut rc: i32;
    loop {
        rc = wal_try_begin_read(p_wal, p_changed, 0, &mut cnt);
        if rc != WAL_RETRY {
            break;
        }
    }
    rc
}

/// Termina com uma transação de leitura. Tudo o que isto faz é liberar o
/// bloqueio de leitura.
pub fn wal_end_read_transaction(p_wal: &mut Wal) {
    wal_end_write_transaction(p_wal);
    if p_wal.read_lock >= 0 {
        wal_unlock_shared(p_wal, wal_read_lock(p_wal.read_lock as i32));
        p_wal.read_lock = -1;
    }
}

/// Procura a página `pgno` no arquivo WAL. Se achar, define `*pi_read` como o
/// frame que contém a página. Caso contrário, se `pgno` não está no arquivo WAL,
/// define `*pi_read` como zero.
///
/// Retorna SQLITE_OK se bem sucedido, ou um código de erro. Se um erro ocorre, o
/// valor final de `*pi_read` é indefinido.
pub fn wal_find_frame(p_wal: &mut Wal, pgno: u32, pi_read: &mut u32) -> i32 {
    // Se != 0, frame do WAL de onde ler os dados
    let mut i_read: u32 = 0;
    // Última página do WAL para este leitor
    let i_last: u32 = p_wal.hdr.mx_frame;

    // Esta rotina só é chamada dentro de uma transação de leitura.
    // (assert( readLock>=0 || lockError ) só existe com SQLITE_DEBUG: some.)

    // Se o campo "última página" do instantâneo do cabeçalho do wal-index é 0,
    // nenhum dado será lido do WAL em circunstância alguma. Retorna cedo como
    // otimização. Do mesmo modo, se pWal->readLock==0, o WAL é ignorado pelo
    // leitor, então retorna cedo como se o WAL estivesse vazio.
    if i_last == 0 || (p_wal.read_lock == 0 && p_wal.b_shm_unreliable == 0) {
        *pi_read = 0;
        return SQLITE_OK;
    }

    // Procura na tabela (ou tabelas) de hash uma entrada que case com o número de
    // página pgno. Cada iteração do laço for abaixo procura uma tabela de hash
    // (cada tabela indexa até HASHTABLE_NPAGE frames).
    //
    // Este código pode rodar concorrentemente com o de walIndexAppend() que
    // adiciona entradas ao wal-index (e possivelmente a esta tabela de hash).
    // Isso significa que o valor recém lido do slot (aHash[iKey]) pode ter sido
    // adicionado antes ou depois de a transação de leitura corrente abrir. Valores
    // adicionados depois podem ter sido escritos incorretamente, isto é, esses
    // slots podem conter lixo. Porém, assume-se que os slots escritos antes da
    // abertura da transação de leitura permanecem inalterados.
    //
    // Pelas razões acima, a condição if(...) do laço interno é mais rigorosa do
    // que seria necessária com acesso exclusivo à tabela de hash:
    //
    //   (aPgno[iFrame]==pgno):
    //     Esta condição filtra as colisões normais da tabela de hash.
    //
    //   (iFrame<=iLast):
    //     Esta condição filtra as entradas adicionadas à tabela de hash depois
    //     que a transação de leitura corrente havia começado.
    let i_min_hash: i32 = wal_frame_page(p_wal.min_frame);
    let mut i_hash: i32 = wal_frame_page(i_last);
    while i_hash >= i_min_hash {
        // Localização da tabela de hash
        let mut s_loc = WalHashLoc::default();

        let rc = wal_hash_get(p_wal, i_hash, &mut s_loc);
        if rc != SQLITE_OK {
            return rc;
        }
        // Número de colisões de hash restantes
        let mut n_collide: i32 = HASHTABLE_NSLOT as i32;
        // Índice do slot de hash
        let mut i_key: i32 = wal_hash(pgno);
        loop {
            let i_h: u32 = wal_ht_slot_get(wal_wi_page(p_wal, i_hash), s_loc.a_hash, i_key as usize) as u32;
            if i_h == 0 {
                break;
            }
            let i_frame: u32 = i_h + s_loc.i_zero;
            if i_frame <= i_last
                && i_frame >= p_wal.min_frame
                && wal_wi_page(p_wal, i_hash)[s_loc.a_pgno + (i_h as usize) - 1] == pgno
            {
                // assert( iFrame>iRead || CORRUPT_DB ): CORRUPT_DB é controle de teste, some.
                i_read = i_frame;
            }
            let prior = n_collide;
            n_collide -= 1;
            if prior == 0 {
                *pi_read = 0;
                return SQLITE_CORRUPT_BKPT;
            }
            i_key = wal_next_hash(i_key);
        }
        if i_read != 0 {
            break;
        }
        i_hash -= 1;
    }

    // O bloco de busca linear sob SQLITE_ENABLE_EXPENSIVE_ASSERT não existe no
    // Debian e some.

    *pi_read = i_read;
    SQLITE_OK
}


// ---- part_009.rs ----

// Notas de tradução deste trecho (chunks/wal_c.009.c):
//
// - `SEH_TRY`/`SEH_EXCEPT` só existem sob `SQLITE_USE_SEH` (Windows): somem.
// - `SQLITE_ENABLE_SETLK_TIMEOUT` não está ligado: o ramo de retorno antecipado de
//   `sqlite3WalBeginWriteTransaction` some.
// - `WalWriter` não guarda ponteiros (`pWal`, `pFd`): quem escreve recebe o `Wal`
//   emprestado e usa `p_wal.p_wal_fd`, que é o `pFd` do C.
// - `xUndo` e `pUndoCtx` viram um único fecho `FnMut(u32) -> i32` que já captura o
//   contexto.

/// Lê o conteúdo do frame `i_read` do arquivo WAL para o buffer `p_out`
/// (de tamanho `n_out` bytes). Retorna SQLITE_OK se bem sucedido, ou um código de
/// erro caso contrário.
pub fn wal_read_frame(p_wal: &mut Wal, i_read: u32, n_out: i32, p_out: &mut [u8]) -> i32 {
    let mut sz: i32 = p_wal.hdr.sz_page as i32;
    sz = (sz & 0xfe00) + ((sz & 0x0001) << 16);
    let i_offset: i64 = wal_frame_offset(i_read, sz as u32) + WAL_FRAME_HDRSIZE as i64;
    // testcase( IS_BIG_INT(iOffset) ): requer um WAL de 4GiB, é só cobertura.
    let n_bytes = if n_out > sz { sz } else { n_out };
    os_read(
        p_wal.p_wal_fd.as_mut().unwrap(),
        &mut p_out[..n_bytes as usize],
        i_offset,
    )
}

/// Retorna o tamanho do banco de dados em páginas (ou zero, se desconhecido).
pub fn wal_dbsize(p_wal: Option<&Wal>) -> u32 {
    if let Some(wal) = p_wal {
        // ALWAYS(pWal->readLock>=0)
        if wal.read_lock >= 0 {
            return wal.hdr.n_page;
        }
    }
    0
}

/// Esta função inicia uma transação de escrita no WAL.
///
/// Uma transação de leitura deve ter sido iniciada por uma chamada anterior
/// a wal_begin_read_transaction().
///
/// Se outra thread ou processo escreveu no banco desde que a transação de
/// leitura foi iniciada, então não é possível para esta thread escrever, pois
/// isso causaria um fork. Então esta rotina retorna SQLITE_BUSY nesse caso
/// e nenhuma transação de escrita é iniciada.
///
/// Pode haver apenas um escritor ativo de cada vez.
pub fn wal_begin_write_transaction(p_wal: &mut Wal) -> i32 {
    // Não é possível iniciar uma transação de escrita sem antes manter uma
    // transação de leitura.
    assert!(p_wal.read_lock >= 0);
    assert!(p_wal.write_lock == 0 && p_wal.i_re_cksum == 0);

    if p_wal.read_only != 0 {
        return SQLITE_READONLY;
    }

    // Apenas um escritor permitido de cada vez. Obtém o write lock. Retorna
    // SQLITE_BUSY se não conseguir.
    let mut rc = wal_lock_exclusive(p_wal, WAL_WRITE_LOCK, 1);
    if rc != SQLITE_OK {
        return rc;
    }
    p_wal.write_lock = 1;

    // Se outra conexão escreveu no arquivo de banco desde que a transação de
    // leitura nesta conexão foi iniciada, a escrita não é permitida.
    if p_wal.hdr != wal_index_hdr(p_wal) {
        rc = SQLITE_BUSY_SNAPSHOT;
    }

    if rc != SQLITE_OK {
        wal_unlock_exclusive(p_wal, WAL_WRITE_LOCK, 1);
        p_wal.write_lock = 0;
    }
    rc
}

/// Termina uma transação de escrita. O commit já foi feito. Esta rotina
/// apenas libera o lock.
pub fn wal_end_write_transaction(p_wal: &mut Wal) -> i32 {
    if p_wal.write_lock != 0 {
        wal_unlock_exclusive(p_wal, WAL_WRITE_LOCK, 1);
        p_wal.write_lock = 0;
        p_wal.i_re_cksum = 0;
        p_wal.truncate_on_commit = 0;
    }
    SQLITE_OK
}

/// Se quaisquer dados foram escritos (mas não confirmados) no arquivo de log,
/// esta função move o ponteiro de escrita de volta ao início da transação.
///
/// Adicionalmente, o callback é invocado para cada frame escrito no WAL desde o
/// início da transação. Se o callback retorna diferente de SQLITE_OK, ele não é
/// invocado novamente e o código de erro é retornado ao chamador.
///
/// De outra forma, se o callback não retorna um erro, esta função retorna
/// SQLITE_OK.
pub fn wal_undo(p_wal: &mut Wal, x_undo: &mut dyn FnMut(u32) -> i32) -> i32 {
    let mut rc = SQLITE_OK;
    // ALWAYS(pWal->writeLock)
    if p_wal.write_lock != 0 {
        let i_max: u32 = p_wal.hdr.mx_frame;

        // Restaura o cache do cliente do cabeçalho wal-index para o estado em que
        // estava antes de o cliente começar a escrever no banco.
        p_wal.hdr = wal_index_hdr(p_wal);

        let mut i_frame: u32 = p_wal.hdr.mx_frame + 1;
        while rc == SQLITE_OK && i_frame <= i_max {
            // Esta chamada não pode falhar. A menos que a página cujo número é
            // passado como segundo argumento esteja (a) no cache e (b) tenha uma
            // referência pendente, xUndo é um no-op (se (a) é falso) ou apenas
            // expulsa a página do cache (se (b) é falso).
            //
            // Se a camada superior está fazendo um rollback, é garantido que não
            // há referências pendentes a nenhuma página além da página 1. E a
            // página 1 nunca é escrita no log até a transação ser confirmada.
            // Como resultado, a chamada a xUndo não pode falhar.
            assert!(wal_frame_pgno(p_wal, i_frame) != 1);
            rc = x_undo(wal_frame_pgno(p_wal, i_frame));
            i_frame += 1;
        }
        if i_max != p_wal.hdr.mx_frame {
            wal_cleanup_hash(p_wal);
        }
    }
    rc
}

/// O argumento `a_wal_data` deve apontar para um array de WAL_SAVEPOINT_NDATA
/// valores u32. Esta função popula o array com os valores necessários para
/// "desfazer" a posição de escrita do handle WAL de volta ao ponto atual, no caso
/// de um rollback de savepoint (via wal_savepoint_undo()).
pub fn wal_savepoint(p_wal: &Wal, a_wal_data: &mut [u32]) {
    assert!(p_wal.write_lock != 0);
    a_wal_data[0] = p_wal.hdr.mx_frame;
    a_wal_data[1] = p_wal.hdr.a_frame_cksum[0];
    a_wal_data[2] = p_wal.hdr.a_frame_cksum[1];
    a_wal_data[3] = p_wal.n_ckpt;
}

/// Move a posição de escrita do WAL de volta ao ponto identificado pelos valores
/// do array `a_wal_data`. Ele deve ter sido populado antes por uma chamada a
/// wal_savepoint().
pub fn wal_savepoint_undo(p_wal: &mut Wal, a_wal_data: &mut [u32]) -> i32 {
    let rc = SQLITE_OK;

    assert!(p_wal.write_lock != 0);
    assert!(a_wal_data[3] != p_wal.n_ckpt || a_wal_data[0] <= p_wal.hdr.mx_frame);

    if a_wal_data[3] != p_wal.n_ckpt {
        // Este savepoint foi aberto imediatamente depois de a transação de escrita
        // começar. Logo depois, o escritor decidiu dar a volta para o início do
        // log. Atualiza os valores do savepoint para corresponder.
        a_wal_data[0] = 0;
        a_wal_data[3] = p_wal.n_ckpt;
    }

    if a_wal_data[0] < p_wal.hdr.mx_frame {
        p_wal.hdr.mx_frame = a_wal_data[0];
        p_wal.hdr.a_frame_cksum[0] = a_wal_data[1];
        p_wal.hdr.a_frame_cksum[1] = a_wal_data[2];
        wal_cleanup_hash(p_wal);
    }

    rc
}

/// Esta função é chamada logo antes de escrever um conjunto de frames no arquivo
/// de log (veja wal_frames()). Ela verifica se, em vez de anexar ao arquivo de
/// log atual, é possível sobrescrever o início do arquivo existente com os novos
/// frames (isto é, "reiniciar" o log). Se sim, define pWal->hdr.mxFrame como 0.
/// Caso contrário, pWal->hdr.mxFrame fica inalterado.
///
/// SQLITE_OK é retornado se nenhum erro ocorre (independente de pWal->hdr.mxFrame
/// ser modificado ou não). Um código de erro SQLite é retornado se um erro ocorre.
fn wal_restart_log(p_wal: &mut Wal) -> i32 {
    let mut rc = SQLITE_OK;

    if p_wal.read_lock == 0 {
        let n_backfill: u32 = wal_ckpt_info(p_wal).n_backfill;
        assert!(n_backfill == p_wal.hdr.mx_frame);
        if n_backfill > 0 {
            let mut salt = [0u8; 4];
            api::randomness(&mut salt);
            let salt1: u32 = u32::from_ne_bytes(salt);
            rc = wal_lock_exclusive(p_wal, wal_read_lock(1), (WAL_NREADER - 1) as i32);
            if rc == SQLITE_OK {
                // Se todos os leitores estão usando WAL_READ_LOCK(0) (em outras
                // palavras, se nenhum leitor está usando o WAL), os frames da
                // transação sobrescrevem o início do log existente. Atualiza o
                // cabeçalho do wal-index para refletir isso.
                //
                // Em teoria seria Ok atualizar só o cache do cabeçalho neste
                // ponto. Mas atualizar o cabeçalho wal-index de verdade também é
                // seguro e significa que não há caso especial para
                // wal_undo() tratar se esta transação sofrer rollback.
                wal_restart_hdr(p_wal, salt1);
                wal_unlock_exclusive(p_wal, wal_read_lock(1), (WAL_NREADER - 1) as i32);
            } else if rc != SQLITE_BUSY {
                return rc;
            }
        }
        wal_unlock_shared(p_wal, wal_read_lock(0));
        p_wal.read_lock = -1;
        let mut cnt: i32 = 0;
        loop {
            let mut not_used: i32 = 0;
            rc = wal_try_begin_read(p_wal, &mut not_used, 1, &mut cnt);
            if rc != WAL_RETRY {
                break;
            }
        }
        // BUSY não é possível quando useWal==1
        assert!((rc & 0xff) != SQLITE_BUSY);
    }
    rc
}

/// Informação sobre o estado atual do arquivo WAL e onde o próximo fsync deve
/// ocorrer, passada de wal_frames() para wal_write_to_log(). O `pWal` e o `pFd`
/// do C são o próprio `Wal` emprestado e o seu `p_wal_fd`.
#[derive(Debug)]
pub struct WalWriter {
    /// Fsync neste offset
    pub i_sync_point: i64,
    /// Flags para o fsync
    pub sync_flags: i32,
    /// Tamanho de uma página
    pub sz_page: i32,
}

/// Escreve `i_amt` bytes de conteúdo no arquivo WAL começando em `i_offset`.
/// Faz um sync ao cruzar o limite p->iSyncPoint.
///
/// Em outras palavras, se iSyncPoint está entre iOffset e iOffset+iAmt, primeiro
/// escreve a parte antes de iSyncPoint, depois faz o sync, depois escreve o resto.
fn wal_write_to_log(
    p_wal: &mut Wal,
    p: &WalWriter,
    p_content: &[u8],
    mut i_amt: i32,
    mut i_offset: i64,
) -> i32 {
    let mut rc: i32;
    let mut content_offset: usize = 0;
    if i_offset < p.i_sync_point && i_offset + i_amt as i64 >= p.i_sync_point {
        let i_first_amt: i32 = (p.i_sync_point - i_offset) as i32;
        rc = os_write(
            p_wal.p_wal_fd.as_mut().unwrap(),
            &p_content[..i_first_amt as usize],
            i_offset,
        );
        if rc != SQLITE_OK {
            return rc;
        }
        i_offset += i_first_amt as i64;
        i_amt -= i_first_amt;
        content_offset += i_first_amt as usize;
        assert!(wal_sync_flags(p.sync_flags) != 0);
        rc = os_sync(p_wal.p_wal_fd.as_mut().unwrap(), wal_sync_flags(p.sync_flags));
        if i_amt == 0 || rc != SQLITE_OK {
            return rc;
        }
    }
    os_write(
        p_wal.p_wal_fd.as_mut().unwrap(),
        &p_content[content_offset..content_offset + i_amt as usize],
        i_offset,
    )
}

/// Escreve um único frame do WAL
fn wal_write_one_frame(
    p_wal: &mut Wal,
    p: &WalWriter,
    p_page: &PgHdr,
    n_truncate: u32,
    i_offset: i64,
) -> i32 {
    // Buffer onde montar o cabeçalho do frame
    let mut a_frame = [0u8; WAL_FRAME_HDRSIZE];
    wal_encode_frame(p_wal, p_page.pgno, n_truncate, &p_page.p_data, &mut a_frame);
    let mut rc = wal_write_to_log(p_wal, p, &a_frame, WAL_FRAME_HDRSIZE as i32, i_offset);
    if rc != SQLITE_OK {
        return rc;
    }
    // Escreve os dados da página
    rc = wal_write_to_log(
        p_wal,
        p,
        &p_page.p_data,
        p.sz_page,
        i_offset + WAL_FRAME_HDRSIZE as i64,
    );
    rc
}

/// Esta função é chamada como parte do commit de uma transação na qual um ou mais
/// frames foram sobrescritos. Ela atualiza os checksums de todos os frames
/// escritos no arquivo wal pela transação corrente, começando pelo primeiro a ter
/// sido sobrescrito.
///
/// SQLITE_OK é retornado se bem sucedido, ou um código de erro SQLite caso
/// contrário.
fn wal_rewrite_checksums(p_wal: &mut Wal, i_last: u32) -> i32 {
    // Tamanho de página do banco
    let sz_page: u32 = p_wal.sz_page;
    // Buffer onde carregar dados do arquivo wal
    let mut a_buf = vec![0u8; sz_page as usize + WAL_FRAME_HDRSIZE];
    // Buffer onde montar os cabeçalhos de frame
    let mut a_frame = [0u8; WAL_FRAME_HDRSIZE];

    // Encontra os valores de checksum a usar como entrada para recalcular o
    // primeiro checksum. Se o primeiro frame é o frame 1 (implicando que a
    // transação corrente reiniciou o arquivo wal), esses valores devem ser lidos
    // do cabeçalho do arquivo wal. Caso contrário, lê-os do cabeçalho do frame
    // anterior.
    assert!(p_wal.i_re_cksum > 0);
    let i_cksum_off: i64 = if p_wal.i_re_cksum == 1 {
        24
    } else {
        wal_frame_offset(p_wal.i_re_cksum - 1, sz_page) + 16
    };
    // Como no C, o resultado da leitura só é conferido na condição do laço: os
    // valores de checksum são copiados do buffer mesmo se a leitura falhou.
    let mut rc = os_read(p_wal.p_wal_fd.as_mut().unwrap(), &mut a_buf[..8], i_cksum_off);
    p_wal.hdr.a_frame_cksum[0] = get4byte(&a_buf, 0);
    p_wal.hdr.a_frame_cksum[1] = get4byte(&a_buf, 4);

    // Próximo frame a ler do arquivo wal
    let mut i_read: u32 = p_wal.i_re_cksum;
    p_wal.i_re_cksum = 0;
    while rc == SQLITE_OK && i_read <= i_last {
        let i_off: i64 = wal_frame_offset(i_read, sz_page);
        rc = os_read(p_wal.p_wal_fd.as_mut().unwrap(), &mut a_buf, i_off);
        if rc == SQLITE_OK {
            let i_pgno: u32 = get4byte(&a_buf, 0);
            let n_db_size: u32 = get4byte(&a_buf, 4);

            wal_encode_frame(p_wal, i_pgno, n_db_size, &a_buf[WAL_FRAME_HDRSIZE..], &mut a_frame);
            rc = os_write(p_wal.p_wal_fd.as_mut().unwrap(), &a_frame, i_off);
        }
        i_read += 1;
    }

    rc
}


// ---- part_010.rs ----

// Notas de tradução deste trecho (chunks/wal_c.010.c):
//
// - `walFrames` e `sqlite3WalFrames` são a mesma função fora do SEH (Windows), então
//   viram uma só, `wal_frames`.
// - `sqlite3WalDb`, `walEnableBlocking` e `walDisableBlocking` só existem sob
//   `SQLITE_ENABLE_SETLK_TIMEOUT`, que não está ligado: somem junto com a conversão
//   de SQLITE_BUSY_TIMEOUT.
// - `pList` (cadeia `pDirty`) é uma fatia de `PgHdrRef` na ordem da cadeia: "p->pDirty
//   não é nulo" vira "não é o último elemento da fatia".
// - A função estática `walCheckpoint` colide com `sqlite3WalCheckpoint` na regra de
//   nomes (as duas dariam `wal_checkpoint`). Aqui a pública fica `wal_checkpoint` e a
//   estática é chamada como `wal_checkpoint_static`; o integrador reconcilia com a
//   parte que traduz a estática.
// - O par `xBusy`/`pBusyArg` vira um fecho `FnMut() -> i32` que já captura o argumento.

/// Escreve um conjunto de frames no log. O chamador deve manter o write-lock
/// do arquivo de log (obtido com wal_begin_write_transaction()).
pub fn wal_frames(
    p_wal: &mut Wal,
    sz_page: i32,
    p_list: &[PgHdrRef],
    n_truncate: u32,
    is_commit: i32,
    sync_flags: i32,
) -> i32 {
    // Último frame da lista
    let mut p_last: Option<PgHdrRef> = None;
    // Número de cópias extras da última página
    let mut n_extra: i32 = 0;
    // Primeiro frame que pode ser sobrescrito
    let mut i_first: u32 = 0;

    assert!(!p_list.is_empty());
    assert!(p_wal.write_lock != 0);

    // Se este conjunto de frames completa uma transação, nTruncate>0. Se
    // nTruncate==0, o conjunto não completa a transação.
    assert!((is_commit != 0) == (n_truncate != 0));

    let live = wal_index_hdr(p_wal);
    if p_wal.hdr != live {
        i_first = live.mx_frame + 1;
    }

    // Vê se é possível escrever estes frames no início do arquivo de log, em vez de
    // anexar em pWal->hdr.mxFrame.
    let mut rc: i32 = wal_restart_log(p_wal);
    if rc != SQLITE_OK {
        return rc;
    }

    // Se este é o primeiro frame escrito no log, escreve o cabeçalho do WAL no
    // início do arquivo. Veja os comentários no topo do arquivo de origem para a
    // descrição do formato do cabeçalho do WAL.
    let mut i_frame: u32 = p_wal.hdr.mx_frame;
    if i_frame == 0 {
        // Buffer onde montar o cabeçalho do wal
        let mut a_wal_hdr = [0u8; WAL_HDRSIZE];
        // Checksum do cabeçalho do wal
        let mut a_cksum = [0u32; 2];

        put4byte(&mut a_wal_hdr, 0, WAL_MAGIC | SQLITE_BIGENDIAN);
        put4byte(&mut a_wal_hdr, 4, WAL_MAX_VERSION);
        put4byte(&mut a_wal_hdr, 8, sz_page as u32);
        put4byte(&mut a_wal_hdr, 12, p_wal.n_ckpt);
        if p_wal.n_ckpt == 0 {
            let mut salt = [0u8; 8];
            api::randomness(&mut salt);
            p_wal.hdr.a_salt[0] = u32::from_ne_bytes([salt[0], salt[1], salt[2], salt[3]]);
            p_wal.hdr.a_salt[1] = u32::from_ne_bytes([salt[4], salt[5], salt[6], salt[7]]);
        }
        a_wal_hdr[16..20].copy_from_slice(&p_wal.hdr.a_salt[0].to_ne_bytes());
        a_wal_hdr[20..24].copy_from_slice(&p_wal.hdr.a_salt[1].to_ne_bytes());
        wal_checksum_bytes(1, &a_wal_hdr, (WAL_HDRSIZE - 2 * 4) as i32, None, &mut a_cksum);
        put4byte(&mut a_wal_hdr, 24, a_cksum[0]);
        put4byte(&mut a_wal_hdr, 28, a_cksum[1]);

        p_wal.sz_page = sz_page as u32;
        p_wal.hdr.big_end_cksum = SQLITE_BIGENDIAN as u8;
        p_wal.hdr.a_frame_cksum[0] = a_cksum[0];
        p_wal.hdr.a_frame_cksum[1] = a_cksum[1];
        p_wal.truncate_on_commit = 1;

        rc = os_write(p_wal.p_wal_fd.as_mut().unwrap(), &a_wal_hdr, 0);
        if rc != SQLITE_OK {
            return rc;
        }

        // Sincroniza o cabeçalho (a menos que SQLITE_IOCAP_SEQUENTIAL seja
        // verdadeiro ou que toda a sincronização esteja desligada por PRAGMA
        // synchronous=OFF). Caso contrário, uma escrita fora de ordem após um
        // reinício do WAL poderia corromper o banco. Veja o ticket:
        //
        //     https://sqlite.org/src/info/ff5be73dee
        if p_wal.sync_header != 0 {
            rc = os_sync(p_wal.p_wal_fd.as_mut().unwrap(), ckpt_sync_flags(sync_flags));
            if rc != SQLITE_OK {
                return rc;
            }
        }
    }
    if p_wal.sz_page as i32 != sz_page {
        // Caso de teste do TH3: cov1/corrupt155.test
        return SQLITE_CORRUPT_BKPT;
    }

    // Prepara a informação necessária para escrever os frames no WAL
    let mut w = WalWriter {
        i_sync_point: 0,
        sync_flags,
        sz_page,
    };
    let mut i_offset: i64 = wal_frame_offset(i_frame + 1, sz_page as u32);
    // Tamanho de um único frame
    let sz_frame: i32 = sz_page + WAL_FRAME_HDRSIZE as i32;

    // Escreve todos os frames no arquivo de log exatamente uma vez
    for (idx, p_ref) in p_list.iter().enumerate() {
        // Equivale a p->pDirty != 0
        let has_next = idx + 1 < p_list.len();

        // Verifica se esta página já foi escrita no arquivo wal pela transação
        // corrente. Se sim, sobrescreve o frame existente e define iReCksum,
        // indicando que os checksums devem ser recalculados no commit.
        if i_first != 0 && (has_next || is_commit == 0) {
            let mut i_write: u32 = 0;
            // VVA_ONLY(rc =): em produção o retorno é descartado, como no C.
            let rc_find = wal_find_frame(p_wal, p_ref.borrow().pgno, &mut i_write);
            debug_assert!(rc_find == SQLITE_OK || i_write == 0);
            if i_write >= i_first {
                let i_off: i64 = wal_frame_offset(i_write, sz_page as u32) + WAL_FRAME_HDRSIZE as i64;
                if p_wal.i_re_cksum == 0 || i_write < p_wal.i_re_cksum {
                    p_wal.i_re_cksum = i_write;
                }
                rc = {
                    let p = p_ref.borrow();
                    os_write(
                        p_wal.p_wal_fd.as_mut().unwrap(),
                        &p.p_data[..sz_page as usize],
                        i_off,
                    )
                };
                if rc != SQLITE_OK {
                    return rc;
                }
                p_ref.borrow_mut().flags &= !PGHDR_WAL_APPEND;
                continue;
            }
        }

        i_frame += 1;
        assert!(i_offset == wal_frame_offset(i_frame, sz_page as u32));
        // 0 normalmente. Positivo == flag de commit
        let n_db_size: u32 = if is_commit != 0 && !has_next { n_truncate } else { 0 };
        rc = wal_write_one_frame(p_wal, &w, &p_ref.borrow(), n_db_size, i_offset);
        if rc != SQLITE_OK {
            return rc;
        }
        p_last = Some(p_ref.clone());
        i_offset += sz_frame as i64;
        p_ref.borrow_mut().flags |= PGHDR_WAL_APPEND;
    }

    // Recalcula os checksums dentro do arquivo wal, se necessário.
    if is_commit != 0 && p_wal.i_re_cksum != 0 {
        rc = wal_rewrite_checksums(p_wal, i_frame);
        if rc != SQLITE_OK {
            return rc;
        }
    }

    // Se este é o fim de uma transação, pode ser preciso preencher (padding) a
    // transação e/ou sincronizar o arquivo WAL.
    //
    // Padding e sincronização só ocorrem se este conjunto de frames completa uma
    // transação e se PRAGMA synchronous=FULL. Se synchronous==NORMAL ou
    // synchronous==OFF, nenhum dos dois é necessário.
    //
    // Se SQLITE_IOCAP_POWERSAFE_OVERWRITE está definido, o padding não é
    // necessário e só o sync é feito. Se o padding é necessário, o frame final é
    // repetido (com sua marca de commit) até cruzar o próximo limite de setor.
    // Só a parte do WAL anterior ao último limite de setor é sincronizada; a parte
    // do último frame que passa do limite é escrita depois do sync.
    if is_commit != 0 && wal_sync_flags(sync_flags) != 0 {
        let mut b_sync = true;
        if p_wal.pad_to_sector_boundary != 0 {
            let sector_size: i64 = sector_size(p_wal.p_wal_fd.as_ref().unwrap()) as i64;
            w.i_sync_point = ((i_offset + sector_size - 1) / sector_size) * sector_size;
            b_sync = w.i_sync_point == i_offset;
            while i_offset < w.i_sync_point {
                assert!(p_last.is_some());
                let last = p_last.as_ref().unwrap().clone();
                rc = wal_write_one_frame(p_wal, &w, &last.borrow(), n_truncate, i_offset);
                if rc != SQLITE_OK {
                    return rc;
                }
                i_offset += sz_frame as i64;
                n_extra += 1;
            }
        }
        if b_sync {
            assert!(rc == SQLITE_OK);
            rc = os_sync(p_wal.p_wal_fd.as_mut().unwrap(), wal_sync_flags(sync_flags));
        }
    }

    // Se este conjunto de frames completa a primeira transação do WAL e se PRAGMA
    // journal_size_limit está definido, trunca o WAL no limite de tamanho do
    // journal, se possível.
    if is_commit != 0 && p_wal.truncate_on_commit != 0 && p_wal.mx_wal_size >= 0 {
        let mut sz: i64 = p_wal.mx_wal_size;
        let end: i64 = wal_frame_offset(i_frame + n_extra as u32 + 1, sz_page as u32);
        if end > p_wal.mx_wal_size {
            sz = end;
        }
        wal_limit_size(p_wal, sz);
        p_wal.truncate_on_commit = 0;
    }

    // Anexa os dados ao wal-index. Não é preciso travar o wal-index para isso: o
    // bloqueio SQLITE_SHM_WRITE mantido sobre o wal-index garante que não há
    // outros escritores, e nenhum dado em uso por leitores existentes está sendo
    // sobrescrito.
    i_frame = p_wal.hdr.mx_frame;
    for p_ref in p_list.iter() {
        if rc != SQLITE_OK {
            break;
        }
        if (p_ref.borrow().flags & PGHDR_WAL_APPEND) == 0 {
            continue;
        }
        i_frame += 1;
        rc = wal_index_append(p_wal, i_frame, p_ref.borrow().pgno);
    }
    assert!(p_last.is_some() || n_extra == 0);
    while rc == SQLITE_OK && n_extra > 0 {
        i_frame += 1;
        n_extra -= 1;
        rc = wal_index_append(p_wal, i_frame, p_last.as_ref().unwrap().borrow().pgno);
    }

    if rc == SQLITE_OK {
        // Atualiza a cópia privada do cabeçalho.
        p_wal.hdr.sz_page = ((sz_page & 0xff00) | (sz_page >> 16)) as u16;
        p_wal.hdr.mx_frame = i_frame;
        if is_commit != 0 {
            p_wal.hdr.i_change = p_wal.hdr.i_change.wrapping_add(1);
            p_wal.hdr.n_page = n_truncate;
        }
        // Se é um commit, atualiza também o cabeçalho do wal-index.
        if is_commit != 0 {
            wal_index_write_hdr(p_wal);
            p_wal.i_callback = i_frame;
        }
    }

    rc
}

/// Esta rotina é chamada para implementar sqlite3_wal_checkpoint() e interfaces
/// relacionadas.
///
/// Obtém um bloqueio CHECKPOINT e então copia (backfill) para o banco de dados o
/// máximo de informação possível do WAL.
///
/// Se `x_busy` não é None, é um callback de busy-handler. Neste caso esta função
/// executa um checkpoint bloqueante.
pub fn wal_checkpoint(
    p_wal: &mut Wal,
    db: Option<Sqlite3Ref>,
    e_mode: i32,
    x_busy: Option<&mut dyn FnMut() -> i32>,
    sync_flags: i32,
    n_buf: i32,
    z_buf: &mut [u8],
    pn_log: Option<&mut i32>,
    pn_ckpt: Option<&mut i32>,
) -> i32 {
    // Verdadeiro se um novo cabeçalho wal-index foi carregado
    let mut is_changed: i32 = 0;
    // Modo a passar para walCheckpoint()
    let mut e_mode2: i32 = e_mode;

    assert!(p_wal.ckpt_lock == 0);
    assert!(p_wal.write_lock == 0);

    // EVIDENCE-OF: R-62920-47450 O callback do busy-handler nunca é invocado no
    // modo SQLITE_CHECKPOINT_PASSIVE.
    assert!(e_mode != SQLITE_CHECKPOINT_PASSIVE || x_busy.is_none());

    if p_wal.read_only != 0 {
        return SQLITE_READONLY;
    }

    // Busy-handler para eMode2
    let mut x_busy2 = x_busy;

    // IMPLEMENTATION-OF: R-62028-47212 Todas as chamadas obtêm um bloqueio
    // exclusivo de "checkpoint" no arquivo de banco de dados.
    // EVIDENCE-OF: R-10421-19736 Se qualquer outro processo estiver rodando um
    // checkpoint ao mesmo tempo, o bloqueio não pode ser obtido e SQLITE_BUSY é
    // retornado.
    // EVIDENCE-OF: R-53820-33897 Mesmo que haja um busy-handler configurado, ele
    // não será invocado neste caso.
    let mut rc: i32 = wal_lock_exclusive(p_wal, WAL_CKPT_LOCK, 1);
    if rc == SQLITE_OK {
        p_wal.ckpt_lock = 1;

        // IMPLEMENTATION-OF: R-59782-36818 Os modos SQLITE_CHECKPOINT_FULL,
        // RESTART e TRUNCATE também obtêm o bloqueio exclusivo de "escritor" no
        // arquivo de banco de dados.
        //
        // EVIDENCE-OF: R-60642-04082 Se o bloqueio de escritor não puder ser
        // obtido imediatamente e houver um busy-handler configurado, ele é
        // invocado e o bloqueio é tentado de novo até que o busy-handler retorne 0
        // ou o bloqueio seja obtido.
        if e_mode != SQLITE_CHECKPOINT_PASSIVE {
            rc = wal_busy_lock(p_wal, x_busy2.as_deref_mut(), WAL_WRITE_LOCK, 1);
            if rc == SQLITE_OK {
                p_wal.write_lock = 1;
            } else if rc == SQLITE_BUSY {
                e_mode2 = SQLITE_CHECKPOINT_PASSIVE;
                x_busy2 = None;
                rc = SQLITE_OK;
            }
        }
    }

    // Lê o cabeçalho do wal-index.
    if rc == SQLITE_OK {
        rc = wal_index_read_hdr(p_wal, &mut is_changed);
        if is_changed != 0
            && p_wal
                .p_db_fd
                .as_ref()
                .unwrap()
                .p_methods
                .as_ref()
                .map_or(false, |m| m.i_version >= 3)
        {
            os_unfetch(p_wal.p_db_fd.as_mut().unwrap(), 0, None);
        }
    }

    // Copia os dados do log para o arquivo de banco de dados.
    if rc == SQLITE_OK {
        if p_wal.hdr.mx_frame != 0 && wal_pagesize(p_wal) != n_buf {
            rc = SQLITE_CORRUPT_BKPT;
        } else {
            rc = wal_checkpoint_static(p_wal, db, e_mode2, x_busy2.as_deref_mut(), sync_flags, z_buf);
        }

        // Se não ocorreu erro, define as variáveis de saída.
        if rc == SQLITE_OK || rc == SQLITE_BUSY {
            if let Some(p) = pn_log {
                *p = p_wal.hdr.mx_frame as i32;
            }
            if let Some(p) = pn_ckpt {
                *p = wal_ckpt_info(p_wal).n_backfill as i32;
            }
        }
    }

    if is_changed != 0 {
        // Se um novo cabeçalho wal-index foi carregado antes de o checkpoint ser
        // feito, o cache de páginas associado a pWal está desatualizado. Então
        // zera o cabeçalho wal-index em cache para garantir que, na próxima vez
        // que o pager abrir um instantâneo deste banco, ele saiba que o cache
        // precisa ser reiniciado.
        p_wal.hdr = WalIndexHdr::default();
    }

    // Libera os bloqueios.
    wal_end_write_transaction(p_wal);
    if p_wal.ckpt_lock != 0 {
        wal_unlock_exclusive(p_wal, WAL_CKPT_LOCK, 1);
        p_wal.ckpt_lock = 0;
    }
    if rc == SQLITE_OK && e_mode != e_mode2 {
        SQLITE_BUSY
    } else {
        rc
    }
}


// ---- part_011.rs ----

// Notas de tradução deste trecho (chunks/wal_c.011.c):
//
// - `SQLITE_ENABLE_SNAPSHOT` e `SQLITE_ENABLE_ZIPVFS` não estão na lista de opções
//   do Debian 13: `sqlite3WalSnapshotGet/Open/Check/Unlock`, `sqlite3_snapshot_cmp` e
//   `sqlite3WalFramesize` somem.
// - O `assert( readLock>=0 || lockError )` só existe fora de SQLITE_USE_SEH e usa um
//   campo de depuração, então não é traduzido.

/// Retorna o valor a passar a um callback sqlite3_wal_hook: o número de frames no
/// WAL no ponto do último commit desde que wal_callback() foi chamado. Se nenhum
/// commit ocorreu desde a última chamada, retorna 0.
pub fn wal_callback(p_wal: Option<&mut Wal>) -> i32 {
    let mut ret: u32 = 0;
    if let Some(wal) = p_wal {
        ret = wal.i_callback;
        wal.i_callback = 0;
    }
    ret as i32
}

/// Esta função é chamada para mudar o subsistema WAL para dentro ou para fora de
/// locking_mode=EXCLUSIVE.
///
/// Se `op` é zero, tenta mudar de locking_mode=EXCLUSIVE para
/// locking_mode=NORMAL. Isto significa que devemos adquirir um bloqueio no byte
/// pWal->readLock. Se o WAL já está em locking_mode=NORMAL ou se a aquisição do
/// bloqueio falha, retorna 0. Se a saída do modo exclusivo é bem sucedida,
/// retorna 1. Esta operação deve ocorrer enquanto o pager ainda mantém o bloqueio
/// exclusivo no arquivo de banco principal.
///
/// Se `op` é um, muda de locking_mode=NORMAL para locking_mode=EXCLUSIVE. Isto
/// significa que pWal->readLock deve ser liberado. Retorna 1 se a transição é
/// feita e 0 se o WAL já está em modo de bloqueio exclusivo, significando que esta
/// rotina é um no-op. O pager já deve manter o bloqueio exclusivo no arquivo de
/// banco principal antes de invocar esta operação.
///
/// Se `op` é negativo, faz um ensaio do caso op==1 sem mudar nada. O pager usa
/// isto para ver se deve adquirir o bloqueio exclusivo do banco antes de invocar o
/// caso op==1.
pub fn wal_exclusive_mode(p_wal: &mut Wal, op: i32) -> i32 {
    let rc: i32;
    assert!(p_wal.write_lock == 0);
    assert!(p_wal.exclusive_mode != WAL_HEAPMEMORY_MODE || op == -1);

    // pWal->readLock normalmente está definido, mas pode ser -1 se houve um erro
    // anterior ao tentar adquirir um bloqueio de leitura. Isto não pode acontecer
    // se a conexão está de fato em modo exclusivo (pois nenhum bloqueio xShmLock é
    // tomado nesse caso). O pager também não deve tentar passar a modo exclusivo
    // depois de tal erro.
    assert!(p_wal.read_lock >= 0 || (op <= 0 && p_wal.exclusive_mode == 0));

    if op == 0 {
        if p_wal.exclusive_mode != WAL_NORMAL_MODE {
            p_wal.exclusive_mode = WAL_NORMAL_MODE;
            if wal_lock_shared(p_wal, wal_read_lock(p_wal.read_lock as i32)) != SQLITE_OK {
                p_wal.exclusive_mode = WAL_EXCLUSIVE_MODE;
            }
            rc = (p_wal.exclusive_mode == WAL_NORMAL_MODE) as i32;
        } else {
            // Já em locking_mode=NORMAL
            rc = 0;
        }
    } else if op > 0 {
        assert!(p_wal.exclusive_mode == WAL_NORMAL_MODE);
        assert!(p_wal.read_lock >= 0);
        wal_unlock_shared(p_wal, wal_read_lock(p_wal.read_lock as i32));
        p_wal.exclusive_mode = WAL_EXCLUSIVE_MODE;
        rc = 1;
    } else {
        rc = (p_wal.exclusive_mode == WAL_NORMAL_MODE) as i32;
    }
    rc
}

/// Retorna verdadeiro se o argumento é não nulo e o módulo WAL está usando
/// memória heap para o wal-index. Caso contrário, se o argumento é None ou o
/// módulo WAL está usando memória compartilhada, retorna falso.
pub fn wal_heap_memory(p_wal: Option<&Wal>) -> i32 {
    match p_wal {
        Some(wal) => (wal.exclusive_mode == WAL_HEAPMEMORY_MODE) as i32,
        None => 0,
    }
}

/// Retorna o objeto sqlite3_file do arquivo WAL
pub fn wal_file(p_wal: &mut Wal) -> Option<&mut Sqlite3File> {
    p_wal.p_wal_fd.as_deref_mut()
}

