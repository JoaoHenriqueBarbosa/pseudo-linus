// Mesclado das partes traduzidas de vdbesort_c (as partes quebram funções grandes no meio).
#![allow(unused_imports, ambiguous_glob_reexports)]
use crate::prelude::*;

// ---- part_000.rs ----


// Modelo de memória deste módulo (ver CONVENTIONS.md):
//  - `sqlite3_file*` compartilhado entre SorterFile, PmaReader e IncrMerger vira
//    `SorterFd = Rc<RefCell<Sqlite3File>>`; quem fecha é o dono (SortSubtask/IncrMerger).
//  - `u8*` de PmaReader (aAlloc, aBuffer, aMap, aKey) vira `Vec<u8>`; vazio equivale a NULL.
//    aMap é a cópia devolvida por `os_fetch`; aKey guarda exatamente nKey bytes.
//  - `SortSubtask*` compartilhado (MergeEngine, IncrMerger) vira `SortSubtaskRef`.
//  - O que as sub-tarefas leem do VdbeSorter (pgsz, db, pKeyInfo) fica em `SorterShared`,
//    apontado por `SortSubtask.p_sorter`, para a sub-tarefa não precisar do VdbeSorter inteiro.
//  - Threads de fundo não existem: `Rc` não cruza threads em Rust seguro, então a tarefa que o C
//    lançaria numa thread roda na hora (ver vdbe_sorter_create_thread). O resultado observável
//    (as linhas e a ordem) é o mesmo.

/// Handle compartilhado de arquivo temporário (`sqlite3_file*`).
pub type SorterFd = Rc<RefCell<Sqlite3File>>;
/// Sub-tarefa compartilhada entre o VdbeSorter, os MergeEngine e os IncrMerger.
pub type SortSubtaskRef = Rc<RefCell<SortSubtask>>;

// Este trecho de vdbesort.c contém o código do objeto VdbeSorter, usado em
// conjunto com VdbeCursor para classificar grandes números de chaves para
// instruções CREATE INDEX ou por SELECT com cláusula ORDER BY que não podem
// ser satisfeitas usando índices e sem cláusulas LIMIT.
//
// O objeto VdbeSorter implementa um algoritmo de merge sort externo
// multi-thread que é eficiente mesmo se o número de elementos sendo
// classificados exceder a memória disponível.
//
// Aqui está a interface (interna, não-API) entre este módulo e o restante
// do sistema SQLite:
//
//    sqlite3VdbeSorterInit()       Cria um novo objeto VdbeSorter.
//
//    sqlite3VdbeSorterWrite()      Adiciona uma única nova linha ao objeto
//                                  VdbeSorter. A linha é um blob binário
//                                  no formato OP_MakeRecord que contém ambas
//                                  as colunas chave ORDER BY e colunas de
//                                  resultado no caso de SELECT com ORDER BY,
//                                  ou o registro completo para uma entrada
//                                  de índice no caso de CREATE INDEX.
//
//    sqlite3VdbeSorterRewind()     Classifica todo conteúdo adicionado
//                                  anteriormente. Posiciona o cursor de leitura
//                                  no primeiro elemento classificado.
//
//    sqlite3VdbeSorterNext()       Avança o cursor de leitura para o próximo
//                                  elemento classificado.
//
//    sqlite3VdbeSorterRowkey()     Retorna o blob binário completo para a
//                                  linha atualmente sob o cursor de leitura.
//
//    sqlite3VdbeSorterCompare()    Compara o blob binário para a linha
//                                  atualmente sob o cursor de leitura contra
//                                  outro blob binário X e relata se X é
//                                  estritamente menor que o cursor de leitura.
//                                  Usado para enforçar unicidade em uma
//                                  instrução CREATE UNIQUE INDEX.
//
//    sqlite3VdbeSorterClose()      Fecha o objeto VdbeSorter e recupera
//                                  todos os recursos.
//
//    sqlite3VdbeSorterReset()      Reforma o VdbeSorter para reutilização.
//                                  Isto é como Close() seguido por Init()
//                                  mas muito mais rápido.
//
// As interfaces acima devem ser chamadas em uma ordem particular. Write()
// só pode ocorrer entre Init()/Reset() e Rewind(). Next(), Rowkey() e
// Compare() só podem ocorrer entre Rewind() e Close()/Reset(). Isto é,
//
//   Init()
//   para cada registro: Write()
//   Rewind()
//     Rowkey()/Compare()
//   Next()
//   Close()
//
// Algoritmo:
//
// Registros passados para o classificador via chamadas a Write() são
// inicialmente mantidos não classificados em memória principal. Assumindo
// que a quantidade de memória usada nunca exceda um limite, quando Rewind()
// é chamado o conjunto de registros é classificado usando um merge sort
// em memória. Neste caso, nenhum arquivo temporário é requerido e chamadas
// subsequentes para Rowkey(), Next() e Compare() leem registros diretamente
// da memória principal.
//
// Se a quantidade de espaço usado para armazenar registros em memória
// principal exceder o limite, então o conjunto de registros atualmente
// em memória são classificados e escritos em um arquivo temporário no
// formato "Packed Memory Array" (PMA). Um PMA criado neste ponto é conhecido
// como um "PMA de nível 0". Níveis superiores de PMAs podem ser criados
// mesclando PMAs existentes juntos, por exemplo mesclando dois ou mais PMAs
// de nível 0 cria um PMA de nível 1.
//
// O limite para a quantidade de memória principal a usar antes de liberar
// registros para um PMA é aproximadamente o mesmo que o limite configurado
// para o cache de página do banco de dados principal. Especificamente, o
// limite é definido como o valor retornado por "PRAGMA main.page_size"
// multiplicado pelo valor retornado por "PRAGMA main.cache_size", em bytes.
//
// Se o classificador está rodando em modo de thread único, então todos os
// PMAs gerados são anexados a um único arquivo temporário. Ou, se o
// classificador está rodando em modo multi-thread então até (N+1) arquivos
// temporários podem ser abertos, onde N é o número configurado de threads
// de trabalho. Neste caso, em vez de classificar os registros e escrever
// o PMA em um arquivo temporário ela mesma, a thread chamadora geralmente
// lança uma thread de trabalho para fazer assim. Exceto, se já há N threads
// de trabalho rodando, a thread principal faz o trabalho ela mesma.
//
// O classificador está rodando em modo multi-thread se (a) a biblioteca foi
// compilada com símbolo pré-processador SQLITE_MAX_WORKER_THREADS definido
// para um valor maior que zero, e (b) threads de trabalho foram habilitadas
// em tempo de execução chamando "PRAGMA threads=N" com algum valor de N maior
// que 0.
//
// Quando Rewind() é chamado, qualquer dado restante em memória é liberado
// para um PMA final. Então neste ponto os dados são armazenados em algum
// número de PMAs classificadas dentro de arquivos temporários no disco.
//
// Se há menos de SORTER_MAX_MERGE_COUNT PMAs no total e o classificador está
// rodando em modo de thread único, então estes PMAs são mesclados
// incrementalmente conforme chaves são recuperadas do classificador pelo VDBE.
// O objeto MergeEngine, descrito em mais detalhes abaixo, realiza esta mescla.
//
// Ou, se rodando em modo multi-thread, então uma thread de fundo é lançada
// para mesclar os PMAs existentes. Uma vez que a thread de fundo tenha
// mesclado T bytes de dados em um único PMA classificado, a thread principal
// começa lendo chaves deste PMA enquanto a thread de fundo continua mesclando
// os próximos T bytes de dados. E assim por diante.
//
// Parâmetro T é definido para metade do valor do limite de memória usado por
// Write() acima para determinar quando criar um novo PMA.
//
// Se há mais de SORTER_MAX_MERGE_COUNT PMAs no total quando Rewind() é
// chamado, então uma hierarquia de mesclas incrementais é usada. Primeiro,
// T bytes de dados a partir dos primeiros SORTER_MAX_MERGE_COUNT PMAs no
// disco são mesclados juntos. Então T bytes de dados a partir do segundo
// conjunto, e assim por diante, tal que nenhuma operação jamais mescla mais
// de SORTER_MAX_MERGE_COUNT PMAs por vez. Isto é feito para melhorar
// localidade.
//
// Se rodando em modo multi-thread e há mais de SORTER_MAX_MERGE_COUNT PMAs
// no disco quando Rewind() é chamado, então mais de uma thread de fundo pode
// ser criada. Especificamente, pode haver uma thread de fundo para cada
// arquivo temporário no disco, e uma thread de fundo para mesclar a saída de
// cada uma das outras em um único PMA para a thread principal ler.

/// Quantidade máxima permitida de dados em memória antes do flush para PMA.
/// O propósito deste limite é prevenir vários overflows de inteiros. 512MiB.
pub const SQLITE_MAX_PMASZ: i64 = 1 << 29;

/// Container para um handle de arquivo temporário e a quantidade atual de
/// dados armazenados no arquivo.
#[derive(Default, Clone)]
pub struct SorterFile {
    /// Handle de arquivo.
    pub p_fd: Option<SorterFd>,
    /// Bytes de dados armazenados em p_fd.
    pub i_eof: i64,
}

/// Uma lista em memória de objetos a serem classificados.
///
/// Se aMemory==None então cada objeto é alocado separadamente e os objetos
/// são conectados usando SorterRecord.u.pNext. Se aMemory!=None então todos
/// os objetos são armazenados em aMemory[] memória em massa, um bem depois do
/// outro, e são conectados usando SorterRecord.u.iNext.
///
/// No modo aMemory o cabeçalho de cada registro ocupa SORTER_RECORD_HDR bytes em a_memory
/// (nVal e iNext, 4 bytes cada, little endian) e o dado vem logo depois; a cabeça da lista
/// é o deslocamento `i_list` (None é NULL). Depois de `vdbe_sorter_sort` a lista passa a ser
/// a cadeia de `p_list` nos dois modos, como o C faz ao converter iNext em pNext.
#[derive(Default)]
pub struct SorterList {
    /// Lista encadeada de registros (modo pNext).
    pub p_list: Option<Box<SorterRecord>>,
    /// Cabeça da lista no modo aMemory: deslocamento do primeiro registro em a_memory.
    pub i_list: Option<usize>,
    /// Se não-None, memória em massa para armazenar pList.
    pub a_memory: Option<Vec<u8>>,
    /// Tamanho de pList como PMA em bytes.
    pub sz_pma: i64,
}

/// O objeto MergeEngine é usado para combinar dois ou mais PMAs menores em
/// um grande PMA usando uma operação de mescla. PMAs separadas precisam ser
/// combinadas em um grande PMA para poder passo através dos registros
/// classificados em ordem.
///
/// O array aReadr[] contém um objeto PmaReader para cada um dos PMAs sendo
/// mesclados. Um objeto aReadr[] aponta para uma chave válida ou está no EOF.
/// ("EOF" significa "End Of File". Quando aReadr[] está no EOF não há mais
/// dados.) Para os propósitos dos parágrafos abaixo, supomos que o array é
/// na verdade N elementos em tamanho, onde N é a menor potência de 2 maior
/// ou igual ao número de PMAs sendo mesclados. Os elementos aReadr[] extra
/// são tratados como se estivessem vazios (sempre no EOF).
///
/// O array aTree[] também é N elementos em tamanho. O valor de N é armazenado
/// na variável MergeEngine.nTree.
///
/// Os elementos finais (N/2) de aTree[] contêm os resultados de comparar
/// pares de chaves PMA juntas. Elemento i contém o resultado de comparar
/// aReadr[2*i-N] e aReadr[2*i-N+1]. Qualquer que seja a chave menor, o
/// elemento aTree é definido para o índice dela.
///
/// Para os propósitos desta comparação, EOF é considerado maior que qualquer
/// outro valor de chave. Se as chaves são iguais (só possível com dois
/// valores EOF), não importa qual índice é armazenado.
///
/// Os elementos (N/4) de aTree[] que precedem os elementos finais (N/2)
/// descritos acima contêm o índice do menor de cada bloco de 4 PmaReaders
/// E assim por diante. Então aTree[1] contém o índice do PmaReader que
/// atualmente aponta para o menor valor de chave. aTree[0] é não utilizado.
///
/// Exemplo:
///
///     aReadr[0] -> Banana
///     aReadr[1] -> Feijoa
///     aReadr[2] -> Elderberry
///     aReadr[3] -> Currant
///     aReadr[4] -> Grapefruit
///     aReadr[5] -> Apple
///     aReadr[6] -> Durian
///     aReadr[7] -> EOF
///
///     aTree[] = { X, 5   0, 5    0, 3, 5, 6 }
///
/// O elemento atual é "Apple" (o valor da chave indicado por PmaReader 5).
/// Quando a operação Next() é invocada, PmaReader 5 será avançado para a
/// próxima chave em seu segmento. Digamos que a próxima chave seja "Eggplant":
///
///     aReadr[5] -> Eggplant
///
/// O conteúdo de aTree[] são atualizados primeiro comparando a nova chave
/// PmaReader 5 para a chave atual de PmaReader 4 (ainda "Grapefruit"). O
/// valor PmaReader 5 ainda é menor, então aTree[6] é definido para 5. E
/// assim por diante acima da árvore. O valor de PmaReader 6 - "Durian" -
/// é agora menor que o de PmaReader 5, então aTree[3] é definido para 6.
/// Chave 0 é menor que chave 6 (Banana<Durian), então o valor escrito no
/// elemento 1 do array é 0. Como segue:
///
///     aTree[] = { X, 0   0, 6    0, 3, 5, 6 }
///
/// Em outras palavras, cada vez que avançamos para o próximo elemento do
/// classificador, log2(N) operações de comparação de chave são requeridas,
/// onde N é o número de segmentos sendo mesclados (arredondado para a
/// próxima potência de 2).
pub struct MergeEngine {
    /// Tamanho usado de aTree/aReadr (potência de 2).
    pub n_tree: i32,
    /// Usado por esta thread somente.
    pub p_task: Option<SortSubtaskRef>,
    /// Estado atual de mescla incremental.
    pub a_tree: Vec<i32>,
    /// Array de PmaReaders para mesclar dados de.
    pub a_readr: Vec<PmaReader>,
}

/// Função de comparação usada por SortSubtask. Os tamanhos nKey1 e nKey2 do C são os
/// comprimentos das fatias.
pub type SorterCompare = fn(&mut SortSubtask, &mut i32, &[u8], &[u8]) -> i32;

/// O que as sub-tarefas leem do VdbeSorter que as possui: tamanho de página, conexão e
/// KeyInfo (fixos depois de `vdbe_sorter_init`).
pub struct SorterShared {
    /// Tamanho de página do banco de dados principal.
    pub pgsz: i32,
    /// Conexão do banco de dados.
    pub db: Weak<RefCell<Sqlite3>>,
    /// Como comparar registros.
    pub p_key_info: KeyInfoRef,
}

/// Este objeto representa uma única thread de controle em uma operação de
/// classificação. Exatamente VdbeSorter.nTask instâncias deste objeto são
/// alocadas como parte de cada objeto VdbeSorter. Instâncias nunca são
/// alocadas de nenhuma outra forma. VdbeSorter.nTask é definido para o
/// número de threads de trabalho permitidas (ver SQLITE_CONFIG_WORKER_THREADS)
/// mais um (a thread principal). Assim para operação de thread única, há
/// exatamente uma instância deste objeto e para operação multi-thread há
/// duas ou mais instâncias.
///
/// Essencialmente, esta estrutura contém todos aqueles campos da estrutura
/// VdbeSorter para qual cada thread requer uma instância separada. Por
/// exemplo, cada thread requer seu próprio objeto UnpackedRecord para
/// desempacotar registros como parte de operações de comparação.
///
/// Antes de uma thread de fundo ser lançada, a variável bDone é definida
/// para 0. Então, bem antes dela sair, a thread ela mesma define bDone
/// para 1. Isto é usado para dois propósitos:
///
///   1. Ao liberar o conteúdo da memória para um PMA de nível 0 no disco,
///      para tentar selecionar um SortSubtask para qual não já há uma thread
///      de fundo ativa (já que fazer assim faz a thread principal bloquear
///      até ela terminar).
///
///   2. Se SQLITE_DEBUG_SORTER_THREADS é definido, para determinar se uma
///      chamada para sqlite3ThreadJoin() é provável que bloqueie. Casos
///      que são prováveis de bloquear provocam saída de debug.
///
/// Em ambos os casos, os efeitos da thread principal vendo (bDone==0) ainda
/// depois que a thread terminou não são graves. Então nós não nos preocupamos
/// com barreiras de memória e tal aqui.
#[derive(Default)]
pub struct SortSubtask {
    /// Thread de fundo, se houver: como a tarefa roda na hora, guarda o valor de retorno que
    /// `sqlite3ThreadJoin` devolveria.
    pub p_thread: Option<i32>,
    /// Definir se thread terminou mas não foi juntada.
    pub b_done: i32,
    /// Número de PMAs atualmente no arquivo.
    pub n_pma: i32,
    /// Dados do classificador que possui esta sub-tarefa.
    pub p_sorter: Option<Rc<SorterShared>>,
    /// Espaço para desempacotar um registro.
    pub p_unpacked: Option<Box<UnpackedRecord>>,
    /// Lista para thread escrever para um PMA.
    pub list: SorterList,
    /// Função de comparação a usar.
    pub x_compare: Option<SorterCompare>,
    /// Arquivo temporário para PMAs de nível 0.
    pub file: SorterFile,
    /// Espaço para outros PMAs.
    pub file2: SorterFile,
}

/// Estrutura principal do classificador. Uma única instância disto é alocada
/// para cada cursor classificador criado pelo VDBE.
///
/// mxKeysize:
///   Conforme registros são adicionados ao classificador por chamadas a
///   sqlite3VdbeSorterWrite(), esta variável é atualizada então para ser
///   definida para o tamanho em disco do maior registro no classificador.
#[derive(Default)]
pub struct VdbeSorter {
    /// Tamanho PMA mínimo, em bytes.
    pub mn_pma_size: i32,
    /// Tamanho PMA máximo, em bytes. 0==sem limite.
    pub mx_pma_size: i32,
    /// Maior chave serializada vista até agora.
    pub mx_keysize: i32,
    /// Tamanho de página do banco de dados principal.
    pub pgsz: i32,
    /// Leitor de dados daqui depois de Rewind().
    pub p_reader: Option<Box<PmaReader>>,
    /// Ou daqui, se bUseThreads==0.
    pub p_merger: Option<Box<MergeEngine>>,
    /// Conexão do banco de dados.
    pub db: Option<Weak<RefCell<Sqlite3>>>,
    /// Como comparar registros.
    pub p_key_info: Option<KeyInfoRef>,
    /// Cópia compartilhada com as sub-tarefas (pgsz, db, pKeyInfo).
    pub p_shared: Option<Rc<SorterShared>>,
    /// Usado por VdbeSorterCompare().
    pub p_unpacked: Option<Box<UnpackedRecord>>,
    /// Lista de registros em memória.
    pub list: SorterList,
    /// Offset do espaço livre em list.aMemory.
    pub i_memory: i32,
    /// Tamanho da alocação list.aMemory em bytes.
    pub n_memory: i32,
    /// Verdadeiro se um ou mais PMAs criados.
    pub b_use_pma: u8,
    /// Verdadeiro para usar threads de fundo.
    pub b_use_threads: u8,
    /// Thread anterior usada para liberar PMA.
    pub i_prev: u8,
    /// Tamanho do array aTask[].
    pub n_task: u8,
    /// Máscara de tipo para otimização.
    pub type_mask: u8,
    /// Uma ou mais sub-tarefas.
    pub a_task: Vec<SortSubtaskRef>,
}

/// Máscara de tipo inteiro para classificador.
pub const SORTER_TYPE_INTEGER: u8 = 0x01;
/// Máscara de tipo texto para classificador.
pub const SORTER_TYPE_TEXT: u8 = 0x02;

/// Uma instância do seguinte objeto é usada para ler registros de um PMA,
/// em ordem classificada. A próxima chave a ser lida é armazenada em cache
/// em nKey/aKey. aKey pode apontar para aMap ou para aBuffer. Se nenhuma
/// daquelas localizações contém uma representação contígua da chave, então
/// aAlloc é alocado e a chave é copiada para aAlloc e aKey é feito apontar
/// para aAlloc.
///
/// pFd==None no EOF.
#[derive(Default)]
pub struct PmaReader {
    /// Offset de leitura atual.
    pub i_read_off: i64,
    /// 1 byte depois do EOF para este PmaReader.
    pub i_eof: i64,
    /// Bytes de espaço em aAlloc.
    pub n_alloc: i32,
    /// Número de bytes em chave.
    pub n_key: i32,
    /// Handle de arquivo que estamos lendo de.
    pub p_fd: Option<SorterFd>,
    /// Espaço para aKey se aBuffer e pMap não funcionarem (vazio é NULL).
    pub a_alloc: Vec<u8>,
    /// Cópia da chave atual, com exatamente n_key bytes.
    pub a_key: Vec<u8>,
    /// Buffer de leitura atual (vazio é NULL).
    pub a_buffer: Vec<u8>,
    /// Tamanho do buffer de leitura em bytes.
    pub n_buffer: i32,
    /// Mapeamento do arquivo inteiro devolvido por os_fetch (vazio é NULL).
    pub a_map: Vec<u8>,
    /// Mescla incremental.
    pub p_incr: Option<Box<IncrMerger>>,
}


// ---- part_001.rs ----

/// Normalmente, um objeto PmaReader itera através de uma PMA existente armazenada
/// dentro de um arquivo temporário. Porém, se a variável PmaReader.pIncr aponta para
/// um objeto do tipo abaixo, ele pode ser usado para iterar/mesclar através de
/// várias PMAs simultaneamente.
///
/// Há dois tipos de objeto IncrMerger: simples (bUseThread==0) e multi-thread
/// (bUseThread==1).
///
/// Um IncrMerger multi-thread usa dois arquivos temporários, aFile[0] e aFile[1].
/// Nenhum dos arquivos pode crescer além de mxSz bytes. Quando o IncrMerger é
/// inicializado, ele lê de pMerger dados suficientes para popular aFile[0]. Depois
/// ajusta as variáveis do PmaReader correspondente para ler daquele arquivo e dispara
/// uma thread de fundo para popular aFile[1] com os próximos mxSz bytes de registros
/// ordenados de pMerger.
///
/// Quando o PmaReader chega ao fim de aFile[0], ele bloqueia até a thread de fundo
/// terminar de popular aFile[1]. Então troca o conteúdo de aFile[0] e aFile[1], ajusta
/// os campos do PmaReader para ler do novo aFile[0] e dispara outra thread de fundo
/// para popular o novo aFile[1]. E assim até o conteúdo de pMerger se esgotar.
///
/// Um IncrMerger de thread única não abre arquivos temporários próprios. Em vez
/// disso, tem acesso exclusivo a mxSz bytes de espaço a partir do deslocamento
/// iStartOff do arquivo pTask->file2. E em vez de usar uma thread de fundo para
/// preparar dados para o PmaReader, a parte alocada de pTask->file2 é "reabastecida"
/// com chaves de pMerger pela thread chamadora sempre que o PmaReader fica sem dados.
#[derive(Default)]
pub struct IncrMerger {
    /// Tarefa dona deste merger.
    pub p_task: Option<SortSubtaskRef>,
    /// Motor de mescla de onde a thread lê dados.
    pub p_merger: Option<Box<MergeEngine>>,
    /// Deslocamento onde começar a escrever o arquivo.
    pub i_start_off: i64,
    /// Máximo de bytes de dados a armazenar.
    pub mx_sz: i32,
    /// Verdadeiro quando a mescla terminou.
    pub b_eof: i32,
    /// Verdadeiro para usar uma thread de fundo para este objeto.
    pub b_use_thread: i32,
    /// aFile[0] para leitura, [1] para escrita.
    pub a_file: [SorterFile; 2],
}

/// Uma instância deste objeto é usada para escrever uma PMA.
///
/// A PMA é escrita um registro por vez. Cada registro tem tamanho arbitrário. Mas a E/S
/// é mais eficiente se ocorre em blocos do tamanho de página alinhados em fronteira de
/// página. Este objeto guarda em cache as escritas na PMA para que blocos alinhados do
/// tamanho de página sejam escritos.
#[derive(Default)]
pub struct PmaWriter {
    /// Diferente de zero se em estado de erro.
    pub e_fw_err: i32,
    /// Buffer de escrita.
    pub a_buffer: Vec<u8>,
    /// Tamanho do buffer de escrita em bytes.
    pub n_buffer: i32,
    /// Primeiro byte do buffer a escrever.
    pub i_buf_start: i32,
    /// Último byte do buffer a escrever.
    pub i_buf_end: i32,
    /// Deslocamento do início do buffer no arquivo.
    pub i_write_off: i64,
    /// Handle de arquivo onde escrever.
    pub p_fd: Option<SorterFd>,
}

/// Cabeçalho de um registro em memória: nVal e iNext (4 bytes cada) em a_memory.
pub const SORTER_RECORD_HDR: usize = 8;

/// Ligação do registro: pNext (modo de alocação separada) ou iNext (deslocamento em
/// aMemory), a union `u` do C.
#[derive(Default)]
pub struct SorterRecordLink {
    /// Próximo registro da lista.
    pub p_next: Option<Box<SorterRecord>>,
    /// Deslocamento em aMemory do próximo registro.
    pub i_next: i32,
}

/// Este objeto é o cabeçalho de um único registro enquanto ele está mantido em memória
/// e antes de ser escrito como parte de uma PMA.
///
/// Como a lista encadeada é conectada depende de como a memória é gerenciada por este
/// módulo. Com alocação separada para cada registro (VdbeSorter.list.aMemory==0), a lista
/// é sempre conectada pelos ponteiros SorterRecord.u.pNext.
///
/// Com a alocação única grande (VdbeSorter.list.aMemory!=0), enquanto os registros são
/// acumulados a lista é ligada pelo deslocamento SorterRecord.u.iNext, porque o array
/// aMemory[] pode ser realocado. Quando a VM termina de passar registros ou o buffer
/// enche, a lista é ordenada e convertida para usar os ponteiros pNext (ver
/// vdbe_sorter_sort). Aqui o dado do registro mora em `data` (SRVAL), com `data.len()==n_val`.
#[derive(Default)]
pub struct SorterRecord {
    /// Tamanho do registro em bytes.
    pub n_val: i32,
    /// pNext ou iNext.
    pub u: SorterRecordLink,
    /// Os dados do registro (o que o C guarda logo depois do cabeçalho).
    pub data: Vec<u8>,
}

impl Drop for SorterRecord {
    // Desmonta a lista de forma iterativa para uma lista longa não estourar a pilha.
    fn drop(&mut self) {
        let mut next = self.u.p_next.take();
        while let Some(mut rec) = next {
            next = rec.u.p_next.take();
        }
    }
}

/// Os dados do registro de SorterRecord p (`SRVAL(p)` do C).
#[inline]
pub fn srval(p: &SorterRecord) -> &[u8] {
    &p.data
}

/// Número máximo de PMAs que um único MergeEngine pode mesclar.
pub const SORTER_MAX_MERGE_COUNT: i32 = 16;

/// Libera toda a memória pertencente ao objeto PmaReader passado como argumento.
/// Todos os campos da estrutura são zerados antes do retorno.
pub fn vdbe_pma_reader_clear(p_readr: &mut PmaReader) {
    // aAlloc e aBuffer são liberados pelo Drop na atribuição final.
    if !p_readr.a_map.is_empty() {
        if let Some(p_fd) = &p_readr.p_fd {
            os_unfetch(&mut p_fd.borrow_mut(), 0);
        }
    }
    vdbe_incr_free(p_readr.p_incr.take());
    *p_readr = PmaReader::default();
}

/// Lê os próximos n_byte bytes de dados da PMA p. Se bem-sucedido, copia os dados para
/// *pp_out e retorna SQLITE_OK. Caso contrário, se ocorre um erro, retorna um código de
/// erro SQLite.
///
/// O conteúdo de *pp_out só vale até a próxima chamada a esta função (no C é um ponteiro
/// para o buffer; aqui é uma cópia).
pub fn vdbe_pma_read_blob(p: &mut PmaReader, n_byte: i32, pp_out: &mut Vec<u8>) -> i32 {
    pp_out.clear();

    if !p.a_map.is_empty() {
        let i_off = p.i_read_off as usize;
        pp_out.extend_from_slice(&p.a_map[i_off..i_off + n_byte as usize]);
        p.i_read_off += n_byte as i64;
        return SQLITE_OK;
    }

    assert!(!p.a_buffer.is_empty());

    // Se não há mais dados a ler do buffer, lê os próximos p->nBuffer bytes do arquivo
    // para dentro dele. Ou, se restam menos de p->nBuffer bytes na PMA, lê todo o resto.
    let i_buf: i32 = (p.i_read_off % p.n_buffer as i64) as i32;
    if i_buf == 0 {
        // Determina quantos bytes de dados ler.
        let n_read: i32 = if (p.i_eof - p.i_read_off) > p.n_buffer as i64 {
            p.n_buffer
        } else {
            (p.i_eof - p.i_read_off) as i32
        };
        assert!(n_read > 0);

        // Lê dados do arquivo. Retorna cedo se ocorrer erro.
        let rc = match &p.p_fd {
            Some(p_fd) => os_read(
                &mut p_fd.borrow_mut(),
                &mut p.a_buffer[..n_read as usize],
                p.i_read_off,
            ),
            None => SQLITE_ERROR,
        };
        assert!(rc != SQLITE_IOERR_SHORT_READ);
        if rc != SQLITE_OK {
            return rc;
        }
    }
    let n_avail: i32 = p.n_buffer - i_buf;

    if n_byte <= n_avail {
        // Os dados pedidos estão disponíveis no buffer em memória. Neste caso não há
        // necessidade de copiar, só devolver o trecho do buffer ao chamador.
        let i = i_buf as usize;
        pp_out.extend_from_slice(&p.a_buffer[i..i + n_byte as usize]);
        p.i_read_off += n_byte as i64;
    } else {
        // Os dados pedidos não estão todos disponíveis no buffer em memória. Neste caso
        // aloca espaço em p->aAlloc[] para copiar o intervalo pedido e devolve a cópia.

        // Estende a alocação de p->aAlloc[] se necessário.
        if p.n_alloc < n_byte {
            let mut n_new: i64 = std::cmp::max(128, 2 * p.n_alloc as i64);
            while n_byte as i64 > n_new {
                n_new *= 2;
            }
            p.a_alloc.resize(n_new as usize, 0);
            p.n_alloc = n_new as i32;
        }

        // Copia o tanto de dados disponível no buffer para o início de p->aAlloc[].
        let i = i_buf as usize;
        p.a_alloc[..n_avail as usize].copy_from_slice(&p.a_buffer[i..i + n_avail as usize]);
        p.i_read_off += n_avail as i64;
        let mut n_rem: i32 = n_byte - n_avail;

        // O laço a seguir copia até p->nBuffer bytes por iteração para p->aAlloc[].
        let mut a_next: Vec<u8> = Vec::new();
        while n_rem > 0 {
            let n_copy: i32 = if n_rem > p.n_buffer { p.n_buffer } else { n_rem };
            let rc = vdbe_pma_read_blob(p, n_copy, &mut a_next);
            if rc != SQLITE_OK {
                return rc;
            }
            let o = (n_byte - n_rem) as usize;
            p.a_alloc[o..o + n_copy as usize].copy_from_slice(&a_next[..n_copy as usize]);
            n_rem -= n_copy;
        }

        pp_out.extend_from_slice(&p.a_alloc[..n_byte as usize]);
    }

    SQLITE_OK
}

/// Lê um varint do fluxo de dados acessado por p. Define *pn_out com o valor lido.
pub fn vdbe_pma_read_varint(p: &mut PmaReader, pn_out: &mut u64) -> i32 {
    if !p.a_map.is_empty() {
        let o = p.i_read_off as usize;
        p.i_read_off += get_varint(&p.a_map[o..], pn_out) as i64;
    } else {
        let i_buf: i32 = (p.i_read_off % p.n_buffer as i64) as i32;
        if i_buf != 0 && (p.n_buffer - i_buf) >= 9 {
            p.i_read_off += get_varint(&p.a_buffer[i_buf as usize..], pn_out) as i64;
        } else {
            let mut a_varint = [0u8; 16];
            let mut i: usize = 0;
            let mut a: Vec<u8> = Vec::new();
            loop {
                let rc = vdbe_pma_read_blob(p, 1, &mut a);
                if rc != 0 {
                    return rc;
                }
                a_varint[i & 0xf] = a[0];
                i += 1;
                if (a[0] & 0x80) == 0 {
                    break;
                }
            }
            get_varint(&a_varint, pn_out);
        }
    }

    SQLITE_OK
}

/// Tenta mapear em memória o arquivo p_file. Se bem-sucedido, define *pp com o novo
/// mapeamento e retorna SQLITE_OK. Se o mapeamento não é tentado (porque o arquivo é
/// grande demais ou a camada VFS está configurada para não usar mmap), retorna SQLITE_OK
/// e deixa *pp vazio (NULL).
///
/// Ou, se ocorre um erro, retorna um código de erro SQLite. O valor final de *pp é
/// indefinido neste caso.
fn vdbe_sorter_map_file(p_task: &SortSubtask, p_file: &SorterFile, pp: &mut Vec<u8>) -> i32 {
    let mut rc = SQLITE_OK;
    let n_max_sorter_mmap = match p_task.p_sorter.as_ref().and_then(|s| s.db.upgrade()) {
        Some(db) => db.borrow().n_max_sorter_mmap,
        None => 0,
    };
    if p_file.i_eof <= n_max_sorter_mmap as i64 {
        if let Some(p_fd) = &p_file.p_fd {
            let i_version = p_fd.borrow().p_methods.as_ref().map_or(0, |m| m.i_version);
            if i_version >= 3 {
                let mut p_map: Option<Vec<u8>> = None;
                rc = os_fetch(&mut p_fd.borrow_mut(), 0, p_file.i_eof as i32, &mut p_map);
                *pp = p_map.unwrap_or_default();
            }
        }
    }
    rc
}

/// Anexa o PmaReader p_readr ao arquivo p_file (se ainda não estiver anexado a esse
/// arquivo) e o posiciona no deslocamento i_off dentro do arquivo. Retorna SQLITE_OK se
/// bem-sucedido, ou um código de erro SQLite se ocorrer um erro.
pub fn vdbe_pma_reader_seek(
    p_task: &SortSubtask,
    p_readr: &mut PmaReader,
    p_file: &SorterFile,
    i_off: i64,
) -> i32 {
    let mut rc: i32;

    assert!(p_readr.p_incr.as_ref().map_or(true, |p| p.b_eof == 0));

    if fault_sim(201) != 0 {
        return SQLITE_IOERR_READ;
    }
    if !p_readr.a_map.is_empty() {
        if let Some(p_fd) = &p_readr.p_fd {
            os_unfetch(&mut p_fd.borrow_mut(), 0);
        }
        p_readr.a_map = Vec::new();
    }
    p_readr.i_read_off = i_off;
    p_readr.i_eof = p_file.i_eof;
    p_readr.p_fd = p_file.p_fd.clone();

    rc = vdbe_sorter_map_file(p_task, p_file, &mut p_readr.a_map);
    if rc == SQLITE_OK && p_readr.a_map.is_empty() {
        let pgsz: i32 = p_task.p_sorter.as_ref().unwrap().pgsz;
        let i_buf: i32 = (p_readr.i_read_off % pgsz as i64) as i32;
        if p_readr.a_buffer.is_empty() {
            p_readr.a_buffer = vec![0u8; pgsz as usize];
            p_readr.n_buffer = pgsz;
        }
        if rc == SQLITE_OK && i_buf != 0 {
            let mut n_read: i32 = pgsz - i_buf;
            if (p_readr.i_read_off + n_read as i64) > p_readr.i_eof {
                n_read = (p_readr.i_eof - p_readr.i_read_off) as i32;
            }
            let i = i_buf as usize;
            rc = match &p_readr.p_fd {
                Some(p_fd) => os_read(
                    &mut p_fd.borrow_mut(),
                    &mut p_readr.a_buffer[i..i + n_read as usize],
                    p_readr.i_read_off,
                ),
                None => SQLITE_ERROR,
            };
        }
    }

    rc
}

/// Avança o PmaReader p_readr para a próxima chave em sua PMA. Retorna SQLITE_OK se
/// não ocorrer erro, ou um código de erro SQLite se ocorrer.
pub fn vdbe_pma_reader_next(p_readr: &mut PmaReader) -> i32 {
    let mut rc: i32 = SQLITE_OK; // Código de retorno
    let mut n_rec: u64 = 0; // Tamanho do registro em bytes

    if p_readr.i_read_off >= p_readr.i_eof {
        let mut b_eof = 1;
        if p_readr.p_incr.is_some() {
            rc = vdbe_incr_swap(p_readr.p_incr.as_mut().unwrap());
            if rc == SQLITE_OK && p_readr.p_incr.as_ref().unwrap().b_eof == 0 {
                // Tira o IncrMerger do leitor durante a chamada para o leitor e o merger
                // poderem ser emprestados ao mesmo tempo, e o devolve em seguida.
                let p_incr = p_readr.p_incr.take().unwrap();
                let p_task = p_incr.p_task.clone().unwrap();
                rc = vdbe_pma_reader_seek(
                    &p_task.borrow(),
                    p_readr,
                    &p_incr.a_file[0],
                    p_incr.i_start_off,
                );
                p_readr.p_incr = Some(p_incr);
                b_eof = 0;
            }
        }

        if b_eof {
            // Esta é uma condição de EOF.
            vdbe_pma_reader_clear(p_readr);
            return rc;
        }
    }

    if rc == SQLITE_OK {
        rc = vdbe_pma_read_varint(p_readr, &mut n_rec);
    }
    if rc == SQLITE_OK {
        p_readr.n_key = n_rec as i32;
        let mut a_key: Vec<u8> = Vec::new();
        rc = vdbe_pma_read_blob(p_readr, n_rec as i32, &mut a_key);
        p_readr.a_key = a_key;
    }

    rc
}


// ---- part_002.rs ----

/// Inicializa o PmaReader p_readr para percorrer a PMA armazenada no arquivo p_file,
/// começando no deslocamento i_start e terminando no deslocamento i_eof-1. Esta função
/// deixa o PmaReader apontando para a primeira chave na PMA (ou EOF se a PMA estiver vazia).
///
/// O parâmetro pn_byte é IN/OUT: o tamanho da PMA é somado a ele. (O comentário do C diz
/// que NULL é aceito, mas o código sempre escreve em *pnByte.)
pub fn vdbe_pma_reader_init(
    p_task: &SortSubtask,
    p_file: &SorterFile,
    i_start: i64,
    p_readr: &mut PmaReader,
    pn_byte: &mut i64,
) -> i32 {
    let mut rc: i32;

    assert!(p_file.i_eof > i_start);
    assert!(p_readr.a_alloc.is_empty() && p_readr.n_alloc == 0);
    assert!(p_readr.a_buffer.is_empty());
    assert!(p_readr.a_map.is_empty());

    rc = vdbe_pma_reader_seek(p_task, p_readr, p_file, i_start);
    if rc == SQLITE_OK {
        let mut n_byte: u64 = 0; // Tamanho da PMA em bytes
        rc = vdbe_pma_read_varint(p_readr, &mut n_byte);
        p_readr.i_eof = p_readr.i_read_off.wrapping_add(n_byte as i64);
        *pn_byte = pn_byte.wrapping_add(n_byte as i64);
    }

    if rc == SQLITE_OK {
        rc = vdbe_pma_reader_next(p_readr);
    }
    rc
}

/// Versão de vdbe_sorter_compare() que assume que já foi determinado que o primeiro campo
/// de key1 é igual ao primeiro campo de key2.
fn vdbe_sorter_compare_tail(
    p_task: &mut SortSubtask,
    pb_key2_cached: &mut i32,
    p_key1: &[u8],
    p_key2: &[u8],
) -> i32 {
    if *pb_key2_cached == 0 {
        let p_key_info = p_task.p_sorter.as_ref().unwrap().p_key_info.clone();
        vdbe_record_unpack(
            &p_key_info,
            p_key2.len() as i32,
            p_key2,
            p_task.p_unpacked.as_mut().unwrap(),
        );
        *pb_key2_cached = 1;
    }
    vdbe_record_compare_with_skip(
        p_key1.len() as i32,
        p_key1,
        p_task.p_unpacked.as_mut().unwrap(),
        1,
    )
}

/// Compara key1 (buffer p_key1) com key2 (buffer p_key2). Usa (pTask->pKeyInfo) para
/// as sequências de colação usadas pela comparação. Retorna o resultado da comparação.
///
/// Se o parâmetro IN/OUT *pb_key2_cached for verdadeiro quando esta função é chamada,
/// assume-se que (pTask->pUnpacked) contém a versão desempacotada de key2. Se for
/// falso, (pTask->pUnpacked) é preenchido com a versão desempacotada de key2 e
/// *pb_key2_cached é definido como verdadeiro antes do retorno.
///
/// Se um erro OOM for encontrado, (pTask->pUnpacked->err_code) é definido como
/// SQLITE_NOMEM.
pub fn vdbe_sorter_compare(
    p_task: &mut SortSubtask,
    pb_key2_cached: &mut i32,
    p_key1: &[u8],
    p_key2: &[u8],
) -> i32 {
    if *pb_key2_cached == 0 {
        let p_key_info = p_task.p_sorter.as_ref().unwrap().p_key_info.clone();
        vdbe_record_unpack(
            &p_key_info,
            p_key2.len() as i32,
            p_key2,
            p_task.p_unpacked.as_mut().unwrap(),
        );
        *pb_key2_cached = 1;
    }
    vdbe_record_compare(p_key1.len() as i32, p_key1, p_task.p_unpacked.as_mut().unwrap())
}

/// Versão especialmente otimizada de vdbe_sorter_compare() que assume que o primeiro
/// campo de cada chave é um valor TEXT e que a sequência de colação para compará-los
/// é BINARY.
pub fn vdbe_sorter_compare_text(
    p_task: &mut SortSubtask,
    pb_key2_cached: &mut i32,
    p_key1: &[u8],
    p_key2: &[u8],
) -> i32 {
    let p1 = p_key1;
    let p2 = p_key2;
    let v1 = p1[0] as usize; // Deslocamento do valor 1
    let v2 = p2[0] as usize; // Deslocamento do valor 2

    let mut n1: u32 = 0;
    let mut n2: u32 = 0;

    get_varint32_nr(&p1[1..], &mut n1);
    get_varint32_nr(&p2[1..], &mut n2);

    // memcmp(v1, v2, (MIN(n1, n2) - 13)/2)
    let n_cmp = ((std::cmp::min(n1, n2) as i32 - 13) / 2) as usize;
    let mut res: i32 = 0;
    for i in 0..n_cmp {
        let d = p1[v1 + i] as i32 - p2[v2 + i] as i32;
        if d != 0 {
            res = d;
            break;
        }
    }
    if res == 0 {
        res = (n1 as i32).wrapping_sub(n2 as i32);
    }

    let (n_key_field, sort_flag0) = {
        let ki = p_task.p_sorter.as_ref().unwrap().p_key_info.borrow();
        (ki.n_key_field, ki.a_sort_flags[0])
    };
    if res == 0 {
        if n_key_field > 1 {
            res = vdbe_sorter_compare_tail(p_task, pb_key2_cached, p_key1, p_key2);
        }
    } else {
        assert!((sort_flag0 & KEYINFO_ORDER_BIGNULL) == 0);
        if sort_flag0 != 0 {
            res *= -1;
        }
    }

    res
}

/// Versão especialmente otimizada de vdbe_sorter_compare() que assume que o primeiro
/// campo de cada chave é um valor INTEGER.
pub fn vdbe_sorter_compare_int(
    p_task: &mut SortSubtask,
    pb_key2_cached: &mut i32,
    p_key1: &[u8],
    p_key2: &[u8],
) -> i32 {
    let p1 = p_key1;
    let p2 = p_key2;
    let s1 = p1[1] as i32; // Tipo serial do lado esquerdo
    let s2 = p2[1] as i32; // Tipo serial do lado direito
    let v1 = &p1[p1[0] as usize..]; // Valor 1
    let v2 = &p2[p2[0] as usize..]; // Valor 2
    let mut res: i32; // Valor de retorno

    assert!((s1 > 0 && s1 < 7) || s1 == 8 || s1 == 9);
    assert!((s2 > 0 && s2 < 7) || s2 == 8 || s2 == 9);

    if s1 == s2 {
        // Os dois valores têm o mesmo sinal. Compara usando memcmp().
        const A_LEN: [u8; 10] = [0, 1, 2, 3, 4, 6, 8, 0, 0, 0];
        let n = A_LEN[s1 as usize] as usize;
        res = 0;
        for i in 0..n {
            res = v1[i] as i32 - v2[i] as i32;
            if res != 0 {
                if ((v1[0] ^ v2[0]) & 0x80) != 0 {
                    res = if (v1[0] & 0x80) != 0 { -1 } else { 1 };
                }
                break;
            }
        }
    } else if s1 > 7 && s2 > 7 {
        res = s1 - s2;
    } else {
        if s2 > 7 {
            res = 1;
        } else if s1 > 7 {
            res = -1;
        } else {
            res = s1 - s2;
        }
        assert!(res != 0);

        if res > 0 {
            if (v1[0] & 0x80) != 0 {
                res = -1;
            }
        } else if (v2[0] & 0x80) != 0 {
            res = 1;
        }
    }

    let (n_key_field, sort_flag0) = {
        let ki = p_task.p_sorter.as_ref().unwrap().p_key_info.borrow();
        (ki.n_key_field, ki.a_sort_flags[0])
    };
    if res == 0 {
        if n_key_field > 1 {
            res = vdbe_sorter_compare_tail(p_task, pb_key2_cached, p_key1, p_key2);
        }
    } else if sort_flag0 != 0 {
        assert!((sort_flag0 & KEYINFO_ORDER_BIGNULL) == 0);
        res *= -1;
    }

    res
}

/// Inicializa o cursor de índice temporário recém-aberto como um cursor de sorter.
///
/// Normalmente, o módulo de sorter usa o valor de (pCsr->pKeyInfo->nKeyField)
/// para determinar o número de campos que devem ser comparados dos registros
/// sendo ordenados. Porém, se o valor passado como argumento n_field for diferente
/// de zero e o sorter puder garantir uma classificação estável, n_field é usado em
/// vez disso. Isto é usado ao ordenar registros para uma instrução CREATE INDEX.
/// Neste caso, as chaves são sempre entregues ao sorter na ordem da chave primária,
/// que por acaso compõe a parte final dos registros sendo ordenados. Portanto, se a
/// classificação for estável, nunca há motivo para comparar campos PK e eles podem ser
/// ignorados para um pequeno ganho de desempenho.
///
/// O sorter pode garantir uma classificação estável ao executar em modo
/// thread único, mas não em modo multi-thread.
///
/// SQLITE_OK é retornado se bem-sucedido, ou um código de erro SQLite caso contrário.
pub fn vdbe_sorter_init(db: &Sqlite3Ref, n_field: i32, p_csr: &mut VdbeCursor) -> i32 {
    let pgsz: i32; // Tamanho de página do banco de dados principal
    let rc = SQLITE_OK;

    // Inicializa o limite superior do número de threads de trabalho
    let n_worker: i32 = if temp_in_memory(&db.borrow()) != 0 || SQLITE_CONFIG.b_core_mutex == 0 {
        0
    } else {
        db.borrow().a_limit[SQLITE_LIMIT_WORKER_THREADS as usize]
    };

    // O teste "SQLITE_MAX_WORKER_THREADS>=SORTER_MAX_MERGE_COUNT" é falso no Debian
    // (8 < 16), então o limite de threads totais não precisa ser aparado aqui.

    assert!(p_csr.p_key_info.is_some());
    assert!(p_csr.is_ephemeral == 0);
    assert!(p_csr.e_cur_type == CURTYPE_SORTER);

    // Cópia de pCsr->pKeyInfo com db==0 (o memcpy do C).
    let mut ki = {
        let src = p_csr.p_key_info.as_ref().unwrap();
        KeyInfo {
            n_ref: src.n_ref,
            enc: src.enc,
            n_key_field: src.n_key_field,
            n_all_field: src.n_all_field,
            db: Weak::new(),
            a_sort_flags: src.a_sort_flags.clone(),
            a_coll: src.a_coll.clone(),
        }
    };
    if n_field != 0 && n_worker == 0 {
        ki.n_key_field = n_field as u16;
    }

    let p_bt = db.borrow().a_db[0].p_bt.clone().unwrap();
    btree_enter(&mut p_bt.borrow_mut());
    pgsz = btree_get_page_size(&p_bt.borrow());
    btree_leave(&mut p_bt.borrow_mut());

    // typeMask depende só do KeyInfo e da colação padrão da conexão.
    let b_dflt_coll = match (&ki.a_coll[0], &db.borrow().p_dflt_coll) {
        (None, _) => true,
        (Some(a), Some(b)) => Rc::ptr_eq(a, b),
        _ => false,
    };
    let b_type_mask = ki.n_all_field < 13
        && b_dflt_coll
        && (ki.a_sort_flags[0] & KEYINFO_ORDER_BIGNULL) == 0;

    let p_key_info: KeyInfoRef = Rc::new(RefCell::new(ki));
    let p_shared = Rc::new(SorterShared {
        pgsz,
        db: Rc::downgrade(db),
        p_key_info: p_key_info.clone(),
    });

    let mut p_sorter = Box::new(VdbeSorter::default());
    p_sorter.p_key_info = Some(p_key_info);
    p_sorter.pgsz = pgsz;
    p_sorter.n_task = (n_worker + 1) as u8;
    p_sorter.i_prev = (n_worker - 1) as u8;
    p_sorter.b_use_threads = (p_sorter.n_task > 1) as u8;
    p_sorter.db = Some(Rc::downgrade(db));
    p_sorter.p_shared = Some(p_shared.clone());
    for _ in 0..p_sorter.n_task {
        let p_task = SortSubtask {
            p_sorter: Some(p_shared.clone()),
            ..Default::default()
        };
        p_sorter.a_task.push(Rc::new(RefCell::new(p_task)));
    }

    if temp_in_memory(&db.borrow()) == 0 {
        let sz_pma: u32 = SQLITE_CONFIG.sz_pma;
        p_sorter.mn_pma_size = sz_pma.wrapping_mul(pgsz as u32) as i32;

        let mut mx_cache: i64 = db.borrow().a_db[0]
            .p_schema
            .as_ref()
            .unwrap()
            .borrow()
            .cache_size as i64;
        if mx_cache < 0 {
            // Um valor de cache-size negativo C indica que o cache tem abs(C) KiB.
            mx_cache *= -1024;
        } else {
            mx_cache *= pgsz as i64;
        }
        mx_cache = std::cmp::min(mx_cache, SQLITE_MAX_PMASZ);
        p_sorter.mx_pma_size = std::cmp::max(p_sorter.mn_pma_size, mx_cache as i32);

        // Evita alocações de memória grandes se a aplicação pediu
        // SQLITE_CONFIG_SMALL_MALLOC.
        if SQLITE_CONFIG.b_small_malloc == 0 {
            assert!(p_sorter.i_memory == 0);
            p_sorter.n_memory = pgsz;
            p_sorter.list.a_memory = Some(vec![0u8; pgsz as usize]);
        }
    }

    if b_type_mask {
        p_sorter.type_mask = SORTER_TYPE_INTEGER | SORTER_TYPE_TEXT;
    }

    p_csr.uc = VdbeCursorCursorUnion::PSorter(p_sorter);
    rc
}

/// Libera a lista de registros ordenados começando em p_record. Cada nó é liberado pelo
/// Drop de SorterRecord (iterativo); o parâmetro db só existia para a contabilidade de memória.
pub fn vdbe_sorter_record_free(db: Option<&Sqlite3Ref>, p_record: Option<Box<SorterRecord>>) {
    let _ = db;
    drop(p_record);
}

/// Fecha e libera o handle de arquivo temporário (`sqlite3OsCloseFree`). A memória do
/// handle sai quando a última referência compartilhada some.
pub fn sorter_close_free(p_fd: SorterFd) {
    os_close(&mut p_fd.borrow_mut());
}

/// Libera todos os recursos possuídos pelo objeto indicado pelo argumento p_task. Todos
/// os campos de *p_task são zerados antes do retorno.
pub fn vdbe_sort_subtask_cleanup(db: Option<&Sqlite3Ref>, p_task: &mut SortSubtask) {
    drop(p_task.p_unpacked.take());
    // pTask->list.aMemory só pode ser não-nulo se recebeu memória da thread principal.
    // Isso só ocorre com SQLITE_MAX_WORKER_THREADS>0, que vale no Debian.
    if p_task.list.a_memory.is_some() {
        p_task.list.a_memory = None;
    } else {
        vdbe_sorter_record_free(db, p_task.list.p_list.take());
    }
    if let Some(p_fd) = p_task.file.p_fd.take() {
        sorter_close_free(p_fd);
    }
    if let Some(p_fd) = p_task.file2.p_fd.take() {
        sorter_close_free(p_fd);
    }
    *p_task = SortSubtask::default();
}


// ---- part_003.rs ----


// SQLITE_DEBUG_SORTER_THREADS não é definido no Debian: vdbeSorterWorkDebug,
// vdbeSorterRewindDebug, vdbeSorterPopulateDebug e vdbeSorterBlockDebug viram macros vazias
// no C e não existem aqui.

/// Junta a thread pTask->thread. Como a tarefa roda na hora (ver
/// vdbe_sorter_create_thread), "juntar" é recolher o valor de retorno que ela guardou.
pub fn vdbe_sorter_join_thread(p_task: &mut SortSubtask) -> i32 {
    let mut rc = SQLITE_OK;
    if let Some(p_ret) = p_task.p_thread.take() {
        rc = p_ret;
        assert!(p_task.b_done == 1);
        p_task.b_done = 0;
    }
    rc
}

/// Lança uma thread de fundo para rodar x_task(p_task). Sem threads em Rust seguro com
/// estado em Rc, a rotina roda aqui mesmo, antes do retorno; ela mesma marca b_done=1 ao
/// terminar, como a rotina de thread do C, e o valor devolvido é o que o join entregará.
pub fn vdbe_sorter_create_thread(
    p_task: &mut SortSubtask,
    x_task: &mut dyn FnMut(&mut SortSubtask) -> i32,
) -> i32 {
    assert!(p_task.p_thread.is_none() && p_task.b_done == 0);
    let rc = x_task(p_task);
    p_task.p_thread = Some(rc);
    SQLITE_OK
}

/// Junta todas as threads pendentes lançadas por SorterWrite() para criar PMAs de nível 0.
pub fn vdbe_sorter_join_all(p_sorter: &VdbeSorter, rcin: i32) -> i32 {
    let mut rc = rcin;

    // Esta função é sempre chamada pela thread principal do usuário.
    //
    // Se está sendo chamada depois de SorterRewind(), é possível que a thread
    // pSorter->aTask[pSorter->nTask-1].pThread esteja tentando juntar uma das outras
    // threads. Para evitar uma condição de corrida em que esta thread também tente
    // juntar o mesmo objeto, junta primeiro a thread
    // pSorter->aTask[pSorter->nTask-1].pThread.
    for i in (0..p_sorter.n_task as usize).rev() {
        let rc2 = vdbe_sorter_join_thread(&mut p_sorter.a_task[i].borrow_mut());
        if rc == SQLITE_OK {
            rc = rc2;
        }
    }
    rc
}

/// Aloca um novo objeto MergeEngine capaz de tratar até n_reader entradas PmaReader.
///
/// n_reader é arredondado automaticamente para a próxima potência de dois.
/// n_reader não pode exceder SORTER_MAX_MERGE_COUNT mesmo depois do arredondamento.
pub fn vdbe_merge_engine_new(n_reader: i32) -> Option<Box<MergeEngine>> {
    let mut n: i32 = 2; // Menor potência de dois >= n_reader

    assert!(n_reader <= SORTER_MAX_MERGE_COUNT);

    while n < n_reader {
        n += n;
    }

    if fault_sim(100) != 0 {
        return None;
    }
    Some(Box::new(MergeEngine {
        n_tree: n,
        p_task: None,
        a_tree: vec![0; n as usize],
        a_readr: (0..n).map(|_| PmaReader::default()).collect(),
    }))
}

/// Libera o objeto MergeEngine passado como único argumento.
pub fn vdbe_merge_engine_free(p_merger: Option<Box<MergeEngine>>) {
    if let Some(mut p_merger) = p_merger {
        for i in 0..p_merger.n_tree as usize {
            vdbe_pma_reader_clear(&mut p_merger.a_readr[i]);
        }
    }
}

/// Libera todos os recursos associados ao objeto IncrMerger indicado pelo primeiro
/// argumento.
pub fn vdbe_incr_free(p_incr: Option<Box<IncrMerger>>) {
    if let Some(mut p_incr) = p_incr {
        if p_incr.b_use_thread != 0 {
            if let Some(p_task) = &p_incr.p_task {
                vdbe_sorter_join_thread(&mut p_task.borrow_mut());
            }
            if let Some(p_fd) = p_incr.a_file[0].p_fd.take() {
                sorter_close_free(p_fd);
            }
            if let Some(p_fd) = p_incr.a_file[1].p_fd.take() {
                sorter_close_free(p_fd);
            }
        }
        vdbe_merge_engine_free(p_incr.p_merger.take());
    }
}

/// Reinicia um cursor de classificação para seu estado vazio original.
pub fn vdbe_sorter_reset(db: &Sqlite3Ref, p_sorter: &mut VdbeSorter) {
    let _ = vdbe_sorter_join_all(p_sorter, SQLITE_OK);
    assert!(p_sorter.b_use_threads != 0 || p_sorter.p_reader.is_none());
    if let Some(mut p_reader) = p_sorter.p_reader.take() {
        vdbe_pma_reader_clear(&mut p_reader);
    }
    vdbe_merge_engine_free(p_sorter.p_merger.take());
    for i in 0..p_sorter.n_task as usize {
        let mut p_task = p_sorter.a_task[i].borrow_mut();
        vdbe_sort_subtask_cleanup(Some(db), &mut p_task);
        p_task.p_sorter = p_sorter.p_shared.clone();
    }
    if p_sorter.list.a_memory.is_none() {
        vdbe_sorter_record_free(None, p_sorter.list.p_list.take());
    }
    p_sorter.list.p_list = None;
    p_sorter.list.i_list = None;
    p_sorter.list.sz_pma = 0;
    p_sorter.b_use_pma = 0;
    p_sorter.i_memory = 0;
    p_sorter.mx_keysize = 0;
    p_sorter.p_unpacked = None;
}

/// Libera quaisquer componentes de cursor alocados pelas rotinas sqlite3VdbeSorterXXX.
pub fn vdbe_sorter_close(db: &Sqlite3Ref, p_csr: &mut VdbeCursor) {
    assert!(p_csr.e_cur_type == CURTYPE_SORTER);
    if let VdbeCursorCursorUnion::PSorter(mut p_sorter) =
        std::mem::replace(&mut p_csr.uc, VdbeCursorCursorUnion::None)
    {
        vdbe_sorter_reset(db, &mut p_sorter);
        p_sorter.list.a_memory = None;
    }
}

/// O primeiro argumento é um handle de arquivo aberto sobre um arquivo temporário. O
/// arquivo tem garantidamente n_byte bytes ou menos. Esta função tenta estender o arquivo
/// para n_byte bytes e garantir que o VFS o tenha mapeado em memória.
///
/// Se o arquivo termina mapeado em memória depende, claro, da implementação específica
/// do VFS.
fn vdbe_sorter_extend_file(db: &Sqlite3Ref, p_fd: &mut Sqlite3File, mut n_byte: i64) {
    let i_version = p_fd.p_methods.as_ref().map_or(0, |m| m.i_version);
    if n_byte <= db.borrow().n_max_sorter_mmap as i64 && i_version >= 3 {
        let mut p: Option<Vec<u8>> = None;
        let mut chunksize: i32 = 4 * 1024;
        os_file_control_hint(p_fd, SQLITE_FCNTL_CHUNK_SIZE, Some(&mut chunksize as &mut dyn Any));
        os_file_control_hint(p_fd, SQLITE_FCNTL_SIZE_HINT, Some(&mut n_byte as &mut dyn Any));
        os_fetch(p_fd, 0, n_byte as i32, &mut p);
        if p.is_some() {
            os_unfetch(p_fd, 0);
        }
    }
}

/// Aloca espaço para um handle de arquivo e abre um arquivo temporário. Se bem-sucedido,
/// define *pp_fd para o handle alocado e retorna SQLITE_OK. Caso contrário, define *pp_fd
/// como None e retorna um código de erro SQLite.
pub fn vdbe_sorter_open_temp_file(
    db: &Sqlite3Ref,
    n_extend: i64,
    pp_fd: &mut Option<SorterFd>,
) -> i32 {
    if fault_sim(202) != 0 {
        return SQLITE_IOERR_ACCESS;
    }
    let p_vfs = db.borrow().p_vfs.clone().unwrap();
    let mut p_file: Option<Box<Sqlite3File>> = None;
    // O C passa &rc como pOutFlags, mas logo em seguida sobrescreve rc com o retorno.
    let rc = os_open_malloc(
        &*p_vfs,
        None,
        &mut p_file,
        SQLITE_OPEN_TEMP_JOURNAL
            | SQLITE_OPEN_READWRITE
            | SQLITE_OPEN_CREATE
            | SQLITE_OPEN_EXCLUSIVE
            | SQLITE_OPEN_DELETEONCLOSE,
        None,
    );
    *pp_fd = p_file.map(|f| Rc::new(RefCell::new(*f)));
    if rc == SQLITE_OK {
        let mut max: i64 = SQLITE_MAX_MMAP_SIZE as i64;
        let p_fd = pp_fd.as_ref().unwrap();
        os_file_control_hint(
            &mut p_fd.borrow_mut(),
            SQLITE_FCNTL_MMAP_SIZE,
            Some(&mut max as &mut dyn Any),
        );
        if n_extend > 0 {
            vdbe_sorter_extend_file(db, &mut p_fd.borrow_mut(), n_extend);
        }
    }
    rc
}

/// Se ainda não foi alocada, aloca a estrutura UnpackedRecord em pTask->pUnpacked.
/// Retorna SQLITE_OK se bem-sucedido (ou se nenhuma alocação foi necessária), ou
/// SQLITE_NOMEM caso contrário.
pub fn vdbe_sort_alloc_unpacked(p_task: &mut SortSubtask) -> i32 {
    if p_task.p_unpacked.is_none() {
        let p_key_info = p_task.p_sorter.as_ref().unwrap().p_key_info.clone();
        p_task.p_unpacked = vdbe_alloc_unpacked_record(&p_key_info);
        match p_task.p_unpacked.as_mut() {
            None => return SQLITE_NOMEM_BKPT,
            Some(p_unpacked) => {
                p_unpacked.n_field = p_key_info.borrow().n_key_field;
                p_unpacked.err_code = 0;
            }
        }
    }
    SQLITE_OK
}

/// Mescla as duas listas ordenadas p1 e p2 em uma única lista.
pub fn vdbe_sorter_merge(
    p_task: &mut SortSubtask, // Contexto da thread chamadora
    p1: Box<SorterRecord>,    // Primeira lista a mesclar
    p2: Box<SorterRecord>,    // Segunda lista a mesclar
) -> Box<SorterRecord> {
    let mut p_final: Option<Box<SorterRecord>> = None;
    let mut pp: &mut Option<Box<SorterRecord>> = &mut p_final;
    let mut b_cached: i32 = 0;
    let x_compare = p_task.x_compare.unwrap();
    let mut p1 = Some(p1);
    let mut p2 = Some(p2);

    loop {
        let res = x_compare(
            p_task,
            &mut b_cached,
            srval(p1.as_ref().unwrap()),
            srval(p2.as_ref().unwrap()),
        );

        if res <= 0 {
            let mut p_cur = p1.take().unwrap();
            p1 = p_cur.u.p_next.take();
            *pp = Some(p_cur);
            pp = &mut pp.as_mut().unwrap().u.p_next;
            if p1.is_none() {
                *pp = p2;
                break;
            }
        } else {
            let mut p_cur = p2.take().unwrap();
            p2 = p_cur.u.p_next.take();
            *pp = Some(p_cur);
            pp = &mut pp.as_mut().unwrap().u.p_next;
            b_cached = 0;
            if p2.is_none() {
                *pp = p1;
                break;
            }
        }
    }
    p_final.unwrap()
}

/// Retorna a função SorterCompare para comparar valores coletados pelo objeto de
/// classificação passado como único argumento.
pub fn vdbe_sorter_get_compare(p: &VdbeSorter) -> SorterCompare {
    if p.type_mask == SORTER_TYPE_INTEGER {
        vdbe_sorter_compare_int
    } else if p.type_mask == SORTER_TYPE_TEXT {
        vdbe_sorter_compare_text
    } else {
        vdbe_sorter_compare
    }
}

/// Ordena a lista encadeada de registros de p_list. Retorna SQLITE_OK se bem-sucedido, ou
/// um código de erro SQLite (por ex. SQLITE_NOMEM) se ocorrer um erro.
///
/// No modo aMemory a lista (ligada por iNext dentro de a_memory, da cabeça i_list) é lida
/// registro a registro e cada um vira um nó com pNext, como o C faz ao converter. O fim da
/// lista é o registro no deslocamento 0 (o primeiro escrito), como `(u8*)p==pList->aMemory`.
///
/// O C lê `pTask->pSorter->typeMask` aqui (`vdbeSorterGetCompare`); a sub-tarefa não vê o
/// VdbeSorter, então o chamador passa `x_compare = vdbe_sorter_get_compare(p_sorter)`.
pub fn vdbe_sorter_sort(
    p_task: &mut SortSubtask,
    p_list: &mut SorterList,
    x_compare: SorterCompare,
) -> i32 {
    let rc = vdbe_sort_alloc_unpacked(p_task);
    if rc != SQLITE_OK {
        return rc;
    }

    let b_mem = p_list.a_memory.is_some();
    let mut i_off: Option<usize> = if b_mem { p_list.i_list.take() } else { None };
    let mut p_chain: Option<Box<SorterRecord>> = if b_mem { None } else { p_list.p_list.take() };
    p_task.x_compare = Some(x_compare);
    let mut a_slot: [Option<Box<SorterRecord>>; 64] = std::array::from_fn(|_| None);

    loop {
        let mut p: Box<SorterRecord>;
        if b_mem {
            let o = match i_off {
                Some(o) => o,
                None => break,
            };
            let a_memory = p_list.a_memory.as_ref().unwrap();
            let n_val = i32::from_le_bytes(a_memory[o..o + 4].try_into().unwrap());
            let i_next = i32::from_le_bytes(a_memory[o + 4..o + 8].try_into().unwrap());
            p = Box::new(SorterRecord {
                n_val,
                u: SorterRecordLink { p_next: None, i_next },
                data: a_memory[o + SORTER_RECORD_HDR..o + SORTER_RECORD_HDR + n_val as usize]
                    .to_vec(),
            });
            i_off = if o == 0 {
                None
            } else {
                assert!((i_next as i64) < malloc_size(a_memory) as i64);
                Some(i_next as usize)
            };
        } else {
            match p_chain.take() {
                Some(mut p_rec) => {
                    p_chain = p_rec.u.p_next.take();
                    p = p_rec;
                }
                None => break,
            }
        }

        p.u.p_next = None;
        let mut i = 0;
        while a_slot[i].is_some() {
            p = vdbe_sorter_merge(p_task, p, a_slot[i].take().unwrap());
            i += 1;
        }
        a_slot[i] = Some(p);
    }

    let mut p: Option<Box<SorterRecord>> = None;
    for i in 0..a_slot.len() {
        let p_slot = match a_slot[i].take() {
            Some(p_slot) => p_slot,
            None => continue,
        };
        p = match p {
            Some(p_prev) => Some(vdbe_sorter_merge(p_task, p_prev, p_slot)),
            None => Some(p_slot),
        };
    }
    p_list.p_list = p;

    let err_code = p_task.p_unpacked.as_ref().unwrap().err_code as i32;
    assert!(err_code == SQLITE_OK || err_code == SQLITE_NOMEM);
    err_code
}



// ---- part_004.rs ----

// CONTRATO DAS PARTES 004 A 007 (para o integrador reconciliar com as partes 000 a 003)
//
// 1. Sem threads: vale SQLITE_MAX_WORKER_THREADS==0. Threads de fundo não existem no modelo
//    sem ponteiros (Rc/RefCell não são Send) e PRAGMA threads vale 0 por padrão, então
//    n_task==1, b_use_threads==0, p_reader==None e a saída em disco/tela é idêntica.
// 2. O C acessa o VdbeSorter por pTask->pSorter e MergeEngine.pTask/IncrMerger.pTask. Aqui os
//    ponteiros de volta somem: o SortSubtask (`p_task`) e os parâmetros do sorter (`cfg`,
//    ver SorterCfg) descem como argumentos. Os campos p_task de MergeEngine/IncrMerger e
//    p_sorter de SortSubtask só serviam a asserts e podem ser removidos.
// 3. Handle de arquivo compartilhado: `pub type SorterFd = Rc<RefCell<Sqlite3File>>`.
//    SorterFile { p_fd: Option<SorterFd>, i_eof: i64 } e PmaReader.p_fd: Option<SorterFd>.
//    SorterFile precisa de #[derive(Clone, Default)] (o C copia a struct por valor).
// 4. PmaWriter { e_fw_err: i32, a_buffer: Vec<u8> (vazio == NULL), n_buffer: i32,
//    i_buf_start: i32, i_buf_end: i32, i_write_off: i64, p_fd: Option<SorterFd> } com Default.
// 5. SorterRecord { n_val: i32, u_next: Option<Box<SorterRecord>>, data: Vec<u8> };
//    `srval(&SorterRecord) -> &[u8]` devolve `data`. O modo de memória em massa (a_memory)
//    só contabiliza tamanho (i_memory, n_memory), as decisões de flush são as do C.
// 6. IncrMerger { p_merger: Option<Box<MergeEngine>>, i_start_off: i64, mx_sz: i32, b_eof: i32,
//    b_use_thread: i32, a_file: [SorterFile; 2] }. PmaReader.a_key: Vec<u8> (n_key bytes).
// 7. Assinaturas assumidas de fora destas partes (todas com o contexto explícito):
//    vdbe_pma_reader_next(p_task, cfg, p_readr) -> i32 (chama vdbe_incr_swap da parte 005);
//    vdbe_pma_reader_init(p_task, cfg, p_file: &SorterFile, i_start, p_readr, pn_byte: &mut i64);
//    vdbe_sorter_open_temp_file(db: &Sqlite3Ref, n_extend: i64, pp_fd: &mut Option<SorterFd>);
//    vdbe_sorter_extend_file(db: &Sqlite3Ref, p_fd: &SorterFd, n_byte: i64);
//    os_write(&mut Sqlite3File, &[u8], i64) -> i32; SORTER_MAX_MERGE_COUNT: i32;
//    x_compare: fn(&mut SortSubtask, &mut i32, &[u8], &[u8]) -> i32.
// 8. VdbeSorter.db: Weak<RefCell<Sqlite3>>. Colisão de nome: o `vdbe_sorter_compare` público
//    (parte 007) colide com o estático da parte 002, que precisa ser renomeado.

/// sizeof(SorterRecord) no x86_64 (int nVal, padding e união de 8 bytes). Entra nas contas de
/// memória do sorter, que decidem quando descarregar um PMA.
pub const SORTER_RECORD_SIZE: usize = 16;

/// Parâmetros do VdbeSorter que o C lê por pTask->pSorter. Montados no início de cada
/// operação para não precisar do ponteiro de volta.
pub struct SorterCfg {
    pub db: Sqlite3Ref,
    pub pgsz: i32,
    pub mx_keysize: i32,
    pub mx_pma_size: i32,
}

/// Monta o SorterCfg a partir do sorter.
pub fn sorter_cfg(p_sorter: &VdbeSorter) -> SorterCfg {
    SorterCfg {
        db: p_sorter
            .db
            .upgrade()
            .expect("conexão do sorter já foi liberada"),
        pgsz: p_sorter.pgsz,
        mx_keysize: p_sorter.mx_keysize,
        mx_pma_size: p_sorter.mx_pma_size,
    }
}

/// Inicializa um objeto PMA-writer.
fn vdbe_pma_writer_init(p_fd: Option<SorterFd>, p: &mut PmaWriter, n_buf: i32, i_start: i64) {
    *p = PmaWriter::default();
    // sqlite3Malloc(0) devolve NULL, o único caso de falha que sobra em Rust.
    if n_buf <= 0 {
        p.e_fw_err = SQLITE_NOMEM_BKPT;
    } else {
        p.a_buffer = vec![0u8; n_buf as usize];
        p.i_buf_start = (i_start % n_buf as i64) as i32;
        p.i_buf_end = p.i_buf_start;
        p.i_write_off = i_start - p.i_buf_start as i64;
        p.n_buffer = n_buf;
        p.p_fd = p_fd;
    }
}

/// Grava o trecho do buffer do writer ainda não escrito e devolve o código do os_write.
fn vdbe_pma_writer_flush_buffer(p: &PmaWriter) -> i32 {
    let i_start = p.i_buf_start as usize;
    let i_end = p.i_buf_end as usize;
    let p_fd = p.p_fd.as_ref().expect("PmaWriter sem arquivo");
    let rc = os_write(
        &mut p_fd.borrow_mut(),
        &p.a_buffer[i_start..i_end],
        p.i_write_off + p.i_buf_start as i64,
    );
    rc
}

/// Escreve os bytes de p_data no PMA. Em caso de erro, o código fica em e_fw_err.
fn vdbe_pma_write_blob(p: &mut PmaWriter, p_data: &[u8]) {
    let n_data = p_data.len() as i32;
    let mut n_rem = n_data;
    while n_rem > 0 && p.e_fw_err == 0 {
        let mut n_copy = n_rem;
        if n_copy > (p.n_buffer - p.i_buf_end) {
            n_copy = p.n_buffer - p.i_buf_end;
        }

        let i_src = (n_data - n_rem) as usize;
        let i_dst = p.i_buf_end as usize;
        p.a_buffer[i_dst..i_dst + n_copy as usize]
            .copy_from_slice(&p_data[i_src..i_src + n_copy as usize]);
        p.i_buf_end += n_copy;
        if p.i_buf_end == p.n_buffer {
            p.e_fw_err = vdbe_pma_writer_flush_buffer(p);
            p.i_buf_start = 0;
            p.i_buf_end = 0;
            p.i_write_off += p.n_buffer as i64;
        }
        debug_assert!(p.i_buf_end < p.n_buffer);

        n_rem -= n_copy;
    }
}

/// Descarrega os dados em buffer para o disco e limpa o objeto PMA-writer. O uso do writer
/// depois desta chamada é indefinido. Devolve SQLITE_OK se a descarga der certo ou não for
/// necessária, senão um código de erro.
///
/// Antes de retornar, grava em *pi_eof o offset logo depois do último byte escrito.
fn vdbe_pma_writer_finish(p: &mut PmaWriter, pi_eof: &mut i64) -> i32 {
    if p.e_fw_err == 0 && !p.a_buffer.is_empty() && p.i_buf_end > p.i_buf_start {
        p.e_fw_err = vdbe_pma_writer_flush_buffer(p);
    }
    *pi_eof = p.i_write_off + p.i_buf_end as i64;
    let rc = p.e_fw_err;
    *p = PmaWriter::default();
    rc
}

/// Escreve o valor i_val codificado como varint no PMA.
fn vdbe_pma_write_varint(p: &mut PmaWriter, i_val: u64) {
    let mut a_byte = [0u8; 10];
    let n_byte = put_varint(&mut a_byte, i_val);
    vdbe_pma_write_blob(p, &a_byte[..n_byte as usize]);
}

/// Escreve o conteúdo atual da lista encadeada em memória p_list num PMA de nível 0 no
/// arquivo temporário da sub-tarefa p_task. Devolve SQLITE_OK ou um código de erro.
///
/// O formato de um PMA é:
///
///   * Um varint com o total de bytes de conteúdo do PMA (sem contar o próprio varint).
///
///   * Um ou mais registros em ordem crescente de chave. Cada registro é um varint seguido
///     de um blob (a chave). O varint é o número de bytes do blob.
fn vdbe_sorter_list_to_pma(p_task: &mut SortSubtask, cfg: &SorterCfg, p_list: &mut SorterList) -> i32 {
    let mut rc = SQLITE_OK;
    let mut writer = PmaWriter::default();

    debug_assert!(p_list.sz_pma > 0);

    // Se o primeiro arquivo PMA temporário ainda não foi aberto, abre agora.
    if p_task.file.p_fd.is_none() {
        rc = vdbe_sorter_open_temp_file(&cfg.db, 0, &mut p_task.file.p_fd);
        debug_assert!(rc != SQLITE_OK || p_task.file.p_fd.is_some());
        debug_assert!(p_task.file.i_eof == 0);
        debug_assert!(p_task.n_pma == 0);
    }

    // Tenta mapear o arquivo em memória.
    if rc == SQLITE_OK {
        let p_fd = p_task.file.p_fd.as_ref().unwrap();
        vdbe_sorter_extend_file(&cfg.db, p_fd, p_task.file.i_eof + p_list.sz_pma + 9);
    }

    // Ordena a lista.
    if rc == SQLITE_OK {
        rc = vdbe_sorter_sort(p_task, p_list);
    }

    if rc == SQLITE_OK {
        vdbe_pma_writer_init(p_task.file.p_fd.clone(), &mut writer, cfg.pgsz, p_task.file.i_eof);
        p_task.n_pma += 1;
        vdbe_pma_write_varint(&mut writer, p_list.sz_pma as u64);
        let mut p = p_list.p_list.take();
        while let Some(mut p_rec) = p {
            p = p_rec.u_next.take();
            vdbe_pma_write_varint(&mut writer, p_rec.n_val as u64);
            vdbe_pma_write_blob(&mut writer, &srval(&p_rec)[..p_rec.n_val as usize]);
            // sqlite3_free(p) quando aMemory==0: o Box é liberado ao sair do escopo.
        }
        p_list.p_list = p;
        rc = vdbe_pma_writer_finish(&mut writer, &mut p_task.file.i_eof);
    }

    debug_assert!(rc != SQLITE_OK || p_list.p_list.is_none());
    rc
}

/// Avança o MergeEngine para a próxima entrada. Grava em *pb_eof verdadeiro se não há próxima
/// entrada porque o MergeEngine chegou ao fim de todas as suas entradas.
///
/// Devolve SQLITE_OK se der certo ou um código de erro.
fn vdbe_merge_engine_step(
    p_merger: &mut MergeEngine,
    p_task: &mut SortSubtask,
    cfg: &SorterCfg,
    pb_eof: &mut i32,
) -> i32 {
    let i_prev = p_merger.a_tree[1] as usize; // Índice do PmaReader a avançar

    // Avança o PmaReader atual.
    let rc = vdbe_pma_reader_next(p_task, cfg, &mut p_merger.a_readr[i_prev]);

    // Atualiza o conteúdo de aTree[].
    if rc == SQLITE_OK {
        let x_compare = p_task.x_compare.expect("x_compare não definido");
        let mut b_cached: i32 = 0;

        // Acha os dois primeiros PmaReaders a comparar: o que acabou de avançar (iPrev) e o
        // vizinho dele no array.
        let mut i_readr1 = i_prev & 0xFFFE;
        let mut i_readr2 = i_prev | 0x0001;

        let mut i = (p_merger.n_tree as usize + i_prev) / 2;
        while i > 0 {
            // Compara pReadr1 e pReadr2. O resultado vai para i_res.
            let i_res: i32 = if p_merger.a_readr[i_readr1].p_fd.is_none() {
                1
            } else if p_merger.a_readr[i_readr2].p_fd.is_none() {
                -1
            } else {
                let r1 = &p_merger.a_readr[i_readr1];
                let r2 = &p_merger.a_readr[i_readr2];
                x_compare(
                    p_task,
                    &mut b_cached,
                    &r1.a_key[..r1.n_key as usize],
                    &r2.a_key[..r2.n_key as usize],
                )
            };

            // Se pReadr1 tinha o menor valor, aTree[i] recebe o índice dele e pReadr2 passa a
            // ser o próximo PmaReader a comparar com pReadr1. Nesse caso não há cache de
            // pReadr2 em pTask->pUnpacked.
            //
            // Se pReadr2 tem o menor dos dois, aTree[i] recebe o índice dele e pReadr1 é
            // atualizado. Se o comparador foi chamado acima, pTask->pUnpacked agora contém um
            // valor equivalente a pReadr2, então a chave dele fica em cache.
            //
            // Se os dois valores são iguais, o do PMA mais antigo é o menor. O array aReadr[]
            // vai do mais antigo ao mais novo, então pReadr1 é mais antigo que pReadr2 se
            // (pReadr1<pReadr2).
            if i_res < 0 || (i_res == 0 && i_readr1 < i_readr2) {
                p_merger.a_tree[i] = i_readr1 as i32;
                i_readr2 = p_merger.a_tree[i ^ 0x0001] as usize;
                b_cached = 0;
            } else {
                if p_merger.a_readr[i_readr1].p_fd.is_some() {
                    b_cached = 0;
                }
                p_merger.a_tree[i] = i_readr2 as i32;
                i_readr1 = p_merger.a_tree[i ^ 0x0001] as usize;
            }
            i /= 2;
        }
        *pb_eof = p_merger.a_readr[p_merger.a_tree[1] as usize].p_fd.is_none() as i32;
    }

    if rc == SQLITE_OK {
        p_task.p_unpacked.as_ref().unwrap().err_code
    } else {
        rc
    }
}

/// Descarrega o conteúdo atual de VdbeSorter.list para um novo PMA. Sem threads de fundo,
/// sempre usa a sub-tarefa 0.
fn vdbe_sorter_flush_pma(p_sorter: &mut VdbeSorter) -> i32 {
    p_sorter.b_use_pma = 1;
    let cfg = sorter_cfg(p_sorter);
    vdbe_sorter_list_to_pma(&mut p_sorter.a_task[0], &cfg, &mut p_sorter.list)
}

/// Acrescenta um registro ao sorter.
pub fn vdbe_sorter_write(p_csr: &mut VdbeCursor, p_val: &Mem) -> i32 {
    let mut rc = SQLITE_OK; // Código de retorno

    debug_assert!(p_csr.e_cur_type == CURTYPE_SORTER);
    let VdbeCursorCursorUnion::PSorter(p_sorter) = &mut p_csr.uc else {
        unreachable!()
    };
    let mut t_u32: u32 = 0; // Tipo serial do primeiro campo do registro
    get_varint32_nr(&p_val.z[1..], &mut t_u32);
    let t = t_u32 as i32;
    if t > 0 && t < 10 && t != 7 {
        p_sorter.type_mask &= SORTER_TYPE_INTEGER;
    } else if t > 10 && (t & 0x01) != 0 {
        p_sorter.type_mask &= SORTER_TYPE_TEXT;
    } else {
        p_sorter.type_mask = 0;
    }

    // Decide se o conteúdo atual da memória deve ser descarregado num PMA antes de seguir.
    //
    // No modo de alocação única grande (pSorter->aMemory!=0), descarrega se (a) já há ao
    // menos um valor em memória e (b) o novo valor não cabe.
    //
    // No modo de alocações separadas, descarrega se:
    //
    //   * a memória total da lista é maior que (tamanho de página * tamanho do cache), ou
    //
    //   * é maior que (tamanho de página * 10) e sqlite3HeapNearlyFull() é verdadeiro.
    let n_req: i64 = p_val.n as i64 + SORTER_RECORD_SIZE as i64; // Bytes de memória necessários
    let n_pma: i64 = p_val.n as i64 + varint_len(p_val.n as u64) as i64; // Bytes de PMA necessários
    if p_sorter.mx_pma_size != 0 {
        let b_flush = if p_sorter.list.a_memory.is_some() {
            p_sorter.i_memory != 0 && (p_sorter.i_memory as i64 + n_req) > p_sorter.mx_pma_size as i64
        } else {
            p_sorter.list.sz_pma > p_sorter.mx_pma_size as i64
                || (p_sorter.list.sz_pma > p_sorter.mn_pma_size as i64 && heap_nearly_full() != 0)
        };
        if b_flush {
            rc = vdbe_sorter_flush_pma(p_sorter);
            p_sorter.list.sz_pma = 0;
            p_sorter.i_memory = 0;
            debug_assert!(rc != SQLITE_OK || p_sorter.list.p_list.is_none());
        }
    }

    p_sorter.list.sz_pma += n_pma;
    if n_pma > p_sorter.mx_keysize as i64 {
        p_sorter.mx_keysize = n_pma as i32;
    }

    if p_sorter.list.a_memory.is_some() {
        let n_min = (p_sorter.i_memory as i64 + n_req) as i32;

        if n_min > p_sorter.n_memory {
            let mut n_new: i64 = 2 * p_sorter.n_memory as i64;
            while n_new < n_min as i64 {
                n_new *= 2;
            }
            if n_new > p_sorter.mx_pma_size as i64 {
                n_new = p_sorter.mx_pma_size as i64;
            }
            if n_new < n_min as i64 {
                n_new = n_min as i64;
            }
            // sqlite3Realloc: o conteúdo antigo é preservado e os registros são Box,
            // então não há ponteiros a reajustar.
            p_sorter
                .list
                .a_memory
                .as_mut()
                .unwrap()
                .resize(n_new as usize, 0);
            p_sorter.n_memory = n_new as i32;
        }

        p_sorter.i_memory += ((n_req + 7) & !7) as i32; // ROUND8(nReq)
    }

    let p_new = Box::new(SorterRecord {
        n_val: p_val.n,
        u_next: p_sorter.list.p_list.take(),
        data: p_val.z[..p_val.n as usize].to_vec(),
    });
    p_sorter.list.p_list = Some(p_new);

    rc
}


// ---- part_005.rs ----

/// Lê chaves de pIncr->pMerger e popula pIncr->aFile[1]. O formato dos dados em aFile[1] é o
/// mesmo dos PMAs comuns, exceto que o varint de número de bytes é omitido no início.
fn vdbe_incr_populate(p_incr: &mut IncrMerger, p_task: &mut SortSubtask, cfg: &SorterCfg) -> i32 {
    let mut rc = SQLITE_OK;
    let i_start = p_incr.i_start_off;
    let mx_sz = p_incr.mx_sz as i64;
    let mut writer = PmaWriter::default();
    debug_assert!(p_incr.b_eof == 0);

    vdbe_pma_writer_init(p_incr.a_file[1].p_fd.clone(), &mut writer, cfg.pgsz, i_start);
    let p_merger = p_incr.p_merger.as_mut().expect("IncrMerger sem MergeEngine");
    while rc == SQLITE_OK {
        let mut dummy: i32 = 0;
        let i_reader = p_merger.a_tree[1] as usize;
        let n_key = p_merger.a_readr[i_reader].n_key;
        let i_eof = writer.i_write_off + writer.i_buf_end as i64;

        // Confere se o arquivo de saída encheu ou se a entrada acabou. Nos dois casos sai
        // do laço.
        if p_merger.a_readr[i_reader].p_fd.is_none() {
            break;
        }
        if (i_eof + n_key as i64 + varint_len(n_key as u64) as i64) > (i_start + mx_sz) {
            break;
        }

        // Escreve a próxima chave na saída.
        vdbe_pma_write_varint(&mut writer, n_key as u64);
        vdbe_pma_write_blob(&mut writer, &p_merger.a_readr[i_reader].a_key[..n_key as usize]);
        rc = vdbe_merge_engine_step(p_merger, p_task, cfg, &mut dummy);
    }

    let rc2 = vdbe_pma_writer_finish(&mut writer, &mut p_incr.a_file[1].i_eof);
    if rc == SQLITE_OK {
        rc = rc2;
    }
    rc
}

/// Chamada quando o PmaReader correspondente a p_incr terminou de ler o conteúdo de aFile[0].
/// Serve para "reabastecer" aFile[0] de modo que o PmaReader releia desde o início.
///
/// Em objetos single-thread, isso é feito lendo chaves de pIncr->pMerger e repopulando
/// aFile[0] (aqui sempre é o caso, não há threads de fundo).
///
/// Devolve SQLITE_OK se der certo ou um código de erro.
fn vdbe_incr_swap(p_incr: &mut IncrMerger, p_task: &mut SortSubtask, cfg: &SorterCfg) -> i32 {
    let rc = vdbe_incr_populate(p_incr, p_task, cfg);
    p_incr.a_file[0] = p_incr.a_file[1].clone();
    if p_incr.a_file[0].i_eof == p_incr.i_start_off {
        p_incr.b_eof = 1;
    }
    rc
}

/// Aloca um novo IncrMerger para ler dados de p_merger e o grava em *pp_out.
///
/// Se ocorrer OOM, *pp_out fica None e p_merger é liberado antes de retornar.
fn vdbe_incr_merger_new(
    p_task: &mut SortSubtask,
    cfg: &SorterCfg,
    p_merger: Box<MergeEngine>,
    pp_out: &mut Option<Box<IncrMerger>>,
) -> i32 {
    let mut rc = SQLITE_OK;
    if fault_sim(100) != 0 {
        *pp_out = None;
        vdbe_merge_engine_free(p_merger);
        rc = SQLITE_NOMEM_BKPT;
    } else {
        let mx_sz = std::cmp::max(cfg.mx_keysize + 9, cfg.mx_pma_size / 2);
        *pp_out = Some(Box::new(IncrMerger {
            p_merger: Some(p_merger),
            i_start_off: 0,
            mx_sz,
            b_eof: 0,
            b_use_thread: 0,
            a_file: [SorterFile::default(), SorterFile::default()],
        }));
        p_task.file2.i_eof += mx_sz as i64;
    }
    debug_assert!(pp_out.is_some() || rc != SQLITE_OK);
    rc
}

/// Recalcula pMerger->aTree[iOut] comparando as próximas chaves dos dois PmaReaders que
/// alimentam essa entrada. Nenhum dos PmaReaders avança, a rotina só compara.
fn vdbe_merge_engine_compare(p_merger: &mut MergeEngine, p_task: &mut SortSubtask, i_out: usize) {
    debug_assert!(i_out < p_merger.n_tree as usize && i_out > 0);

    let (i1, i2) = if i_out >= (p_merger.n_tree as usize / 2) {
        let i1 = (i_out - p_merger.n_tree as usize / 2) * 2;
        (i1, i1 + 1)
    } else {
        (
            p_merger.a_tree[i_out * 2] as usize,
            p_merger.a_tree[i_out * 2 + 1] as usize,
        )
    };

    let p1 = &p_merger.a_readr[i1];
    let p2 = &p_merger.a_readr[i2];

    let i_res = if p1.p_fd.is_none() {
        i2
    } else if p2.p_fd.is_none() {
        i1
    } else {
        let mut b_cached: i32 = 0;
        debug_assert!(p_task.p_unpacked.is_some()); // vindo de vdbeSortSubtaskMain()
        let x_compare = p_task.x_compare.expect("x_compare não definido");
        let res = x_compare(
            p_task,
            &mut b_cached,
            &p1.a_key[..p1.n_key as usize],
            &p2.a_key[..p2.n_key as usize],
        );
        if res <= 0 {
            i1
        } else {
            i2
        }
    };

    p_merger.a_tree[i_out] = i_res as i32;
}

/// Valores permitidos para o parâmetro e_mode de vdbe_merge_engine_init() e
/// vdbe_pma_reader_incr_merge_init().
///
/// Só INCRINIT_NORMAL é válido sem threads (SQLITE_MAX_WORKER_THREADS==0).
pub const INCRINIT_NORMAL: i32 = 0;
pub const INCRINIT_TASK: i32 = 1;
pub const INCRINIT_ROOT: i32 = 2;

/// Inicializa o MergeEngine p_merger. Quando a função retorna, a primeira chave dos dados
/// mesclados pode ser lida do MergeEngine da forma usual.
///
/// Se e_mode for INCRINIT_ROOT, presume-se que os IncrMerger dos PmaReaders já foram
/// populados (só existe com threads). Caso contrário, usa vdbe_pma_reader_incr_init() para
/// inicializar cada PmaReader que alimenta p_merger.
///
/// Devolve SQLITE_OK se der certo ou um código de erro.
fn vdbe_merge_engine_init(
    p_task: &mut SortSubtask,
    cfg: &SorterCfg,
    p_merger: &mut MergeEngine,
    e_mode: i32,
) -> i32 {
    // e_mode é sempre INCRINIT_NORMAL sem threads.
    debug_assert!(e_mode == INCRINIT_NORMAL);

    let n_tree = p_merger.n_tree as usize;
    for i in 0..n_tree {
        let rc = vdbe_pma_reader_incr_init(&mut p_merger.a_readr[i], p_task, cfg, INCRINIT_NORMAL);
        if rc != SQLITE_OK {
            return rc;
        }
    }

    for i in (1..n_tree).rev() {
        vdbe_merge_engine_compare(p_merger, p_task, i);
    }
    p_task.p_unpacked.as_ref().unwrap().err_code
}

/// O PmaReader p_readr é garantidamente um leitor incremental (pReadr->pIncr!=0). Esta
/// função abre e/ou inicializa os campos de arquivo temporário do IncrMerger em
/// (pReadr->pIncr).
///
/// Com INCRINIT_NORMAL, todos os PmaReaders da subárvore de p_readr também são
/// inicializados. Os dados são então carregados nos buffers de p_readr e ele passa a apontar
/// para a primeira chave do seu intervalo.
///
/// Devolve SQLITE_OK se der certo ou um código de erro.
fn vdbe_pma_reader_incr_merge_init(
    p_readr: &mut PmaReader,
    p_task: &mut SortSubtask,
    cfg: &SorterCfg,
    e_mode: i32,
) -> i32 {
    // e_mode é sempre INCRINIT_NORMAL sem threads.
    debug_assert!(e_mode == INCRINIT_NORMAL);

    let mut rc;
    {
        let p_incr = p_readr.p_incr.as_mut().expect("PmaReader não é incremental");
        rc = vdbe_merge_engine_init(
            p_task,
            cfg,
            p_incr.p_merger.as_mut().expect("IncrMerger sem MergeEngine"),
            e_mode,
        );

        // Prepara os arquivos de p_incr. Um objeto single-thread só precisa de uma região de
        // pTask->file2.
        if rc == SQLITE_OK {
            let mx_sz = p_incr.mx_sz;
            if p_task.file2.p_fd.is_none() {
                debug_assert!(p_task.file2.i_eof > 0);
                rc = vdbe_sorter_open_temp_file(&cfg.db, p_task.file2.i_eof, &mut p_task.file2.p_fd);
                p_task.file2.i_eof = 0;
            }
            if rc == SQLITE_OK {
                p_incr.a_file[1].p_fd = p_task.file2.p_fd.clone();
                p_incr.i_start_off = p_task.file2.i_eof;
                p_task.file2.i_eof += mx_sz as i64;
            }
        }
    }

    if rc == SQLITE_OK {
        rc = vdbe_pma_reader_next(p_task, cfg, p_readr);
    }

    rc
}


// ---- part_006.rs ----

/// Se o PmaReader p_readr não é um leitor incremental (pReadr->pIncr==0), esta função não faz
/// nada. Caso contrário, invoca vdbe_pma_reader_incr_merge_init() com os parâmetros recebidos
/// para inicializar a mesclagem incremental. Sem threads, é sempre a thread atual que roda.
fn vdbe_pma_reader_incr_init(
    p_readr: &mut PmaReader,
    p_task: &mut SortSubtask,
    cfg: &SorterCfg,
    e_mode: i32,
) -> i32 {
    let mut rc = SQLITE_OK; // Código de retorno
    if p_readr.p_incr.is_some() {
        rc = vdbe_pma_reader_incr_merge_init(p_readr, p_task, cfg, e_mode);
    }
    rc
}

/// Aloca um novo MergeEngine para mesclar o conteúdo de n_pma PMAs de nível 0 de
/// pTask->file. Se não houver erro, grava o objeto em *pp_out e devolve SQLITE_OK. Se houver,
/// *pp_out fica None e devolve um código de erro.
///
/// Na chamada, *pi_offset é o offset do primeiro PMA a ler de pTask->file. Sem erro, ao
/// retornar vale o offset logo depois do último byte do último PMA. Com erro, o valor final
/// de *pi_offset é indefinido.
fn vdbe_merge_engine_level0(
    p_task: &mut SortSubtask,
    cfg: &SorterCfg,
    n_pma: i32,
    pi_offset: &mut i64,
    pp_out: &mut Option<Box<MergeEngine>>,
) -> i32 {
    let mut i_off = *pi_offset;
    let mut rc = SQLITE_OK;

    *pp_out = vdbe_merge_engine_new(n_pma);
    if pp_out.is_none() {
        rc = SQLITE_NOMEM_BKPT;
    }

    // O C passa &pTask->file e pTask juntos. A cópia (que só compartilha o handle) evita o
    // aliasing, e o arquivo não muda durante a leitura dos PMAs de nível 0.
    let file = p_task.file.clone();
    let mut i = 0;
    while i < n_pma && rc == SQLITE_OK {
        let mut n_dummy: i64 = 0;
        let p_readr = &mut pp_out.as_mut().unwrap().a_readr[i as usize];
        rc = vdbe_pma_reader_init(p_task, cfg, &file, i_off, p_readr, &mut n_dummy);
        i_off = p_readr.i_eof;
        i += 1;
    }

    if rc != SQLITE_OK {
        if let Some(p_new) = pp_out.take() {
            vdbe_merge_engine_free(p_new);
        }
    }
    *pi_offset = i_off;
    rc
}

/// Devolve a profundidade de uma árvore com n_pma PMAs, supondo fanout de
/// SORTER_MAX_MERGE_COUNT. O valor não inclui as folhas.
///
/// isto é:
///
///   nPMA<=16    -> TreeDepth() == 0
///   nPMA<=256   -> TreeDepth() == 1
///   nPMA<=65536 -> TreeDepth() == 2
fn vdbe_sorter_tree_depth(n_pma: i32) -> i32 {
    let mut n_depth = 0;
    let mut n_div: i64 = SORTER_MAX_MERGE_COUNT as i64;
    while n_div < n_pma as i64 {
        n_div *= SORTER_MAX_MERGE_COUNT as i64;
        n_depth += 1;
    }
    n_depth
}

/// p_root é a raiz de uma árvore de mesclagem incremental de profundidade n_depth (segundo
/// vdbe_sorter_tree_depth()). p_leaf é a i_seq-ésima folha a entrar na árvore, contando de
/// zero. Esta função acrescenta p_leaf à árvore.
///
/// Se der certo, devolve SQLITE_OK. Se ocorrer erro, devolve um código de erro e p_leaf é
/// liberado.
fn vdbe_sorter_add_to_tree(
    p_task: &mut SortSubtask,
    cfg: &SorterCfg,
    n_depth: i32,
    i_seq: i32,
    p_root: &mut MergeEngine,
    p_leaf: Box<MergeEngine>,
) -> i32 {
    let mut n_div: i32 = 1;
    let mut p: &mut MergeEngine = p_root;
    let mut p_incr: Option<Box<IncrMerger>> = None;

    let mut rc = vdbe_incr_merger_new(p_task, cfg, p_leaf, &mut p_incr);

    for _ in 1..n_depth {
        n_div *= SORTER_MAX_MERGE_COUNT;
    }

    let mut i = 1;
    while i < n_depth && rc == SQLITE_OK {
        let i_iter = ((i_seq / n_div) % SORTER_MAX_MERGE_COUNT) as usize;
        let p_readr = &mut p.a_readr[i_iter];

        if p_readr.p_incr.is_none() {
            match vdbe_merge_engine_new(SORTER_MAX_MERGE_COUNT) {
                None => rc = SQLITE_NOMEM_BKPT,
                Some(p_new) => {
                    rc = vdbe_incr_merger_new(p_task, cfg, p_new, &mut p_readr.p_incr);
                }
            }
        }
        if rc == SQLITE_OK {
            p = p_readr
                .p_incr
                .as_mut()
                .unwrap()
                .p_merger
                .as_mut()
                .unwrap();
            n_div /= SORTER_MAX_MERGE_COUNT;
        }
        i += 1;
    }

    if rc == SQLITE_OK {
        p.a_readr[(i_seq % SORTER_MAX_MERGE_COUNT) as usize].p_incr = p_incr;
    } else if let Some(p_incr) = p_incr {
        vdbe_incr_free(p_incr);
    }
    rc
}

/// Chamada como parte de um SorterRewind() num sorter que já escreveu dois ou mais PMAs de
/// nível 0 em um ou mais arquivos temporários. Monta uma árvore de objetos
/// MergeEngine/IncrMerger/PmaReader que mescla incrementalmente todos os PMAs em disco.
///
/// Se der certo, devolve SQLITE_OK e grava a raiz da árvore em *pp_out. Se ocorrer erro,
/// devolve um código de erro e o valor final de *pp_out é indefinido.
fn vdbe_sorter_merge_tree_build(
    p_sorter: &mut VdbeSorter,
    pp_out: &mut Option<Box<MergeEngine>>,
) -> i32 {
    let mut p_main: Option<Box<MergeEngine>> = None;
    let mut rc = SQLITE_OK;
    let cfg = sorter_cfg(p_sorter);

    let mut i_task = 0usize;
    while rc == SQLITE_OK && i_task < p_sorter.n_task as usize {
        let p_task = &mut p_sorter.a_task[i_task];
        debug_assert!(p_task.n_pma > 0);
        // Com SQLITE_MAX_WORKER_THREADS==0 a condição (nPMA) sempre vale.
        let mut p_root: Option<Box<MergeEngine>> = None; // Raiz da árvore desta tarefa
        let n_depth = vdbe_sorter_tree_depth(p_task.n_pma);
        let mut i_read_off: i64 = 0;

        if p_task.n_pma <= SORTER_MAX_MERGE_COUNT {
            let n_pma = p_task.n_pma;
            rc = vdbe_merge_engine_level0(p_task, &cfg, n_pma, &mut i_read_off, &mut p_root);
        } else {
            let mut i_seq = 0;
            p_root = vdbe_merge_engine_new(SORTER_MAX_MERGE_COUNT);
            if p_root.is_none() {
                rc = SQLITE_NOMEM_BKPT;
            }
            let mut i = 0;
            while i < p_task.n_pma && rc == SQLITE_OK {
                let mut p_merger: Option<Box<MergeEngine>> = None; // Novo mesclador de PMA nível 0

                // Número de PMAs de nível 0 a mesclar
                let n_reader = std::cmp::min(p_task.n_pma - i, SORTER_MAX_MERGE_COUNT);
                rc = vdbe_merge_engine_level0(p_task, &cfg, n_reader, &mut i_read_off, &mut p_merger);
                if rc == SQLITE_OK {
                    rc = vdbe_sorter_add_to_tree(
                        p_task,
                        &cfg,
                        n_depth,
                        i_seq,
                        p_root.as_mut().unwrap(),
                        p_merger.take().unwrap(),
                    );
                    i_seq += 1;
                }
                i += SORTER_MAX_MERGE_COUNT;
            }
        }

        if rc == SQLITE_OK {
            debug_assert!(p_main.is_none());
            p_main = p_root;
        } else if let Some(p_root) = p_root {
            vdbe_merge_engine_free(p_root);
        }
        i_task += 1;
    }

    if rc != SQLITE_OK {
        if let Some(p_main) = p_main.take() {
            vdbe_merge_engine_free(p_main);
        }
    }
    *pp_out = p_main;
    rc
}

/// Chamada como parte de um sqlite3VdbeSorterRewind() num sorter que escreveu dois ou mais
/// PMAs em arquivos temporários. Prepara VdbeSorter.pMerger (sorters single-thread) para
/// iterar por todos os registros guardados no sorter.
///
/// Devolve SQLITE_OK se der certo ou um código de erro.
fn vdbe_sorter_setup_merge(p_sorter: &mut VdbeSorter) -> i32 {
    let mut p_main: Option<Box<MergeEngine>> = None;

    let mut rc = vdbe_sorter_merge_tree_build(p_sorter, &mut p_main);
    if rc == SQLITE_OK {
        let cfg = sorter_cfg(p_sorter);
        rc = vdbe_merge_engine_init(
            &mut p_sorter.a_task[0],
            &cfg,
            p_main.as_mut().expect("árvore de mesclagem vazia"),
            INCRINIT_NORMAL,
        );
        p_sorter.p_merger = p_main.take();
    }

    if rc != SQLITE_OK {
        if let Some(p_main) = p_main {
            vdbe_merge_engine_free(p_main);
        }
    }
    rc
}

/// Depois que o sorter foi populado por chamadas a sqlite3VdbeSorterWrite, esta função
/// prepara a iteração pelos registros em ordem.
pub fn vdbe_sorter_rewind(p_csr: &mut VdbeCursor, pb_eof: &mut i32) -> i32 {
    let mut rc; // Código de retorno

    debug_assert!(p_csr.e_cur_type == CURTYPE_SORTER);
    let VdbeCursorCursorUnion::PSorter(p_sorter) = &mut p_csr.uc else {
        unreachable!()
    };

    // Se nenhum dado foi escrito em disco, não escreve agora. Em vez disso ordena a lista
    // em memória. A camada vdbe lê os dados direto da lista.
    if p_sorter.b_use_pma == 0 {
        rc = SQLITE_OK;
        if p_sorter.list.p_list.is_some() {
            *pb_eof = 0;
            rc = vdbe_sorter_sort(&mut p_sorter.a_task[0], &mut p_sorter.list);
        } else {
            *pb_eof = 1;
        }
        return rc;
    }

    // Escreve a lista em memória num PMA. Quando vdbe_sorter_write() descarrega a memória
    // para o disco, cria logo em seguida uma lista nova com uma única chave. Portanto a lista
    // nunca está vazia neste ponto.
    debug_assert!(p_sorter.list.p_list.is_some());
    rc = vdbe_sorter_flush_pma(p_sorter);

    // Junta todas as threads (nenhuma existe, só repassa o código).
    rc = vdbe_sorter_join_all(p_sorter, rc);

    // Sem erro, monta a estrutura de mesclagem que lê e mescla incrementalmente os PMAs
    // restantes.
    debug_assert!(p_sorter.p_reader.is_none());
    if rc == SQLITE_OK {
        rc = vdbe_sorter_setup_merge(p_sorter);
        *pb_eof = 0;
    }

    rc
}


// ---- part_007.rs ----

/// Avança para o próximo elemento do sorter. Valor de retorno:
///
///    SQLITE_OK     sucesso
///    SQLITE_DONE   fim dos dados
///    caso contrário algum tipo de erro.
pub fn vdbe_sorter_next(db: &Sqlite3Ref, p_csr: &mut VdbeCursor) -> i32 {
    let rc: i32; // Código de retorno

    debug_assert!(p_csr.e_cur_type == CURTYPE_SORTER);
    let VdbeCursorCursorUnion::PSorter(p_sorter) = &mut p_csr.uc else {
        unreachable!()
    };
    debug_assert!(
        p_sorter.b_use_pma != 0 || (p_sorter.p_reader.is_none() && p_sorter.p_merger.is_none())
    );
    if p_sorter.b_use_pma != 0 {
        debug_assert!(p_sorter.p_reader.is_none() || p_sorter.p_merger.is_none());
        // Sem threads (SQLITE_MAX_WORKER_THREADS==0), b_use_threads é sempre 0.
        debug_assert!(p_sorter.b_use_threads == 0);
        debug_assert!(p_sorter.p_merger.is_some());
        let cfg = sorter_cfg(p_sorter);
        let mut res: i32 = 0;
        let mut rc1 = vdbe_merge_engine_step(
            p_sorter.p_merger.as_mut().unwrap(),
            &mut p_sorter.a_task[0],
            &cfg,
            &mut res,
        );
        if rc1 == SQLITE_OK && res != 0 {
            rc1 = SQLITE_DONE;
        }
        rc = rc1;
    } else {
        let mut p_free = p_sorter.list.p_list.take().unwrap();
        p_sorter.list.p_list = p_free.u_next.take();
        if p_sorter.list.a_memory.is_none() {
            vdbe_sorter_record_free(Some(db), Some(p_free));
        }
        rc = if p_sorter.list.p_list.is_some() {
            SQLITE_OK
        } else {
            SQLITE_DONE
        };
    }
    rc
}

/// Devolve o buffer do sorter com a chave atual. O comprimento do buffer devolvido é o tamanho
/// da chave em bytes (o `*pnKey` do C). O nome tem o sufixo `_buf` porque o
/// `sqlite3VdbeSorterRowkey` público já ocupa `vdbe_sorter_rowkey` neste módulo.
fn vdbe_sorter_rowkey_buf(p_sorter: &VdbeSorter) -> Vec<u8> {
    if p_sorter.b_use_pma != 0 {
        // Sem threads, a chave atual sempre vem do mesclador.
        let p_merger = p_sorter.p_merger.as_ref().unwrap();
        let p_reader = &p_merger.a_readr[p_merger.a_tree[1] as usize];
        p_reader.a_key[..p_reader.n_key as usize].to_vec()
    } else {
        let p_list = p_sorter.list.p_list.as_ref().unwrap();
        srval(p_list)[..p_list.n_val as usize].to_vec()
    }
}

/// Copia a chave atual do sorter para a célula de memória p_out.
pub fn vdbe_sorter_rowkey(p_csr: &VdbeCursor, p_out: &mut Mem) -> i32 {
    debug_assert!(p_csr.e_cur_type == CURTYPE_SORTER);
    let VdbeCursorCursorUnion::PSorter(p_sorter) = &p_csr.uc else {
        unreachable!()
    };
    let p_key = vdbe_sorter_rowkey_buf(p_sorter); // Chave do sorter a copiar para p_out
    let n_key = p_key.len() as i32;
    if vdbe_mem_clear_and_resize(p_out, n_key) != 0 {
        return SQLITE_NOMEM_BKPT;
    }
    p_out.n = n_key;
    mem_set_type_flag(p_out, MEM_BLOB);
    p_out.z[..p_key.len()].copy_from_slice(&p_key);

    SQLITE_OK
}

/// Compara a chave da célula de memória p_val com a chave para a qual o cursor do sorter
/// passado como primeiro argumento aponta no momento. Para efeito da comparação, o campo rowid
/// no fim de cada registro é ignorado.
///
/// Se a chave do cursor do sorter tiver algum valor NULL, ela é considerada menor que p_val,
/// mesmo que p_val também tenha valores NULL.
///
/// Se ocorrer um erro, devolve um código de erro do SQLite (isto é, SQLITE_NOMEM). Caso
/// contrário, define *p_res como um valor negativo, zero ou positivo conforme a chave em p_val
/// seja menor, igual ou maior que a chave atual do sorter.
///
/// Esta rotina forma o núcleo do opcode OP_SorterCompare, que por sua vez é usado para
/// verificar a unicidade ao construir um UNIQUE INDEX.
pub fn vdbe_sorter_compare(
    p_csr: &mut VdbeCursor,
    p_val: &Mem,
    n_key_col: i32,
    p_res: &mut i32,
) -> i32 {
    debug_assert!(p_csr.e_cur_type == CURTYPE_SORTER);
    let p_key_info = p_csr.p_key_info.as_ref();
    let VdbeCursorCursorUnion::PSorter(p_sorter) = &mut p_csr.uc else {
        unreachable!()
    };
    if p_sorter.p_unpacked.is_none() {
        match vdbe_alloc_unpacked_record(p_key_info) {
            None => return SQLITE_NOMEM_BKPT,
            Some(mut r2) => {
                r2.n_field = n_key_col as u16;
                p_sorter.p_unpacked = Some(r2);
            }
        }
    }
    debug_assert!(p_sorter.p_unpacked.as_ref().unwrap().n_field as i32 == n_key_col);

    let p_key = vdbe_sorter_rowkey_buf(p_sorter); // Chave do sorter a comparar com p_val
    let r2 = p_sorter.p_unpacked.as_mut().unwrap();
    vdbe_record_unpack(p_key_info, p_key.len() as i32, &p_key, r2);
    for i in 0..n_key_col as usize {
        if r2.a_mem[i].flags & MEM_NULL != 0 {
            *p_res = -1;
            return SQLITE_OK;
        }
    }

    *p_res = vdbe_record_compare(p_val.n, &p_val.z, r2);
    SQLITE_OK
}

