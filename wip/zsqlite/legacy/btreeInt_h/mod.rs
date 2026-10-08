// Mesclado das partes traduzidas de btreeInt_h (as partes quebram funções grandes no meio).
#![allow(unused_imports, ambiguous_glob_reexports)]
use crate::prelude::*;

// ---- part_000.rs ----

/// O tamanho máximo de célula, assumindo o tamanho máximo de página acima.
#[inline]
pub fn mx_cell_size(page_size: u32) -> i32 {
    (page_size - 8) as i32
}

/// O número máximo de células em uma única página do banco de dados.
/// Assume um tamanho mínimo de célula de 6 bytes (4 bytes para a célula em si
/// mais 2 bytes para o índice da célula no cabeçalho da página).
/// Células tão pequenas serão raras, mas são possíveis.
#[inline]
pub fn mx_cell(page_size: u32) -> u32 {
    (page_size - 8) / 6
}

/// String mágica que aparece no início de cada banco de dados SQLite
/// para identificar o arquivo como um banco de dados real.
///
/// Pode ser alterado em tempo de compilação especificando
/// -DSQLITE_FILE_HEADER="..." na linha de comando do compilador.
/// O cabeçalho deve ter exatamente 16 bytes incluindo o finalizador nulo,
/// portanto a string em si deve ter 15 caracteres de comprimento.
/// Se você alterar o cabeçalho, sua biblioteca personalizada não será capaz
/// de ler bancos de dados gerados pelas ferramentas padrão e as ferramentas padrão
/// não serão capazes de ler bancos de dados criados pela sua biblioteca personalizada.
pub const SQLITE_FILE_HEADER: &[u8; 16] = b"SQLite format 3\0";

/// Flags de tipo de página. Uma combinação com OR desses flags aparece como
/// o primeiro byte da imagem em disco de cada página BTree.
pub const PTF_INTKEY: u8 = 0x01;
pub const PTF_ZERODATA: u8 = 0x02;
pub const PTF_LEAFDATA: u8 = 0x04;
pub const PTF_LEAF: u8 = 0x08;

/// Uma instância deste objeto armazena informações sobre cada página de banco de dados
/// que foi carregada na memória. As informações neste objeto
/// são derivadas do conteúdo da página em disco bruta.
///
/// Conforme cada página de banco de dados é carregada na memória, o paginador aloca uma
/// instância deste objeto e zera os primeiros 8 bytes. (Esta é a
/// informação "extra" associada a cada página do paginador.)
///
/// O acesso a todos os campos desta estrutura é controlado pelo mutex
/// armazenado em MemPage.p_bt->mutex.
pub struct MemPage {
    /// True se previamente inicializado. DEVE SER O PRIMEIRO!
    pub is_init: u8,
    /// True se tabelas b-trees. False para índice b-trees
    pub int_key: u8,
    /// True se a folha de uma tabela intKey
    pub int_key_leaf: u8,
    /// Número da página para esta página
    pub pgno: u32,
    /// Apenas os primeiros 8 bytes (acima) são zerados pelo pager.c quando uma nova página
    /// é alocada. Todos os campos que se seguem devem ser inicializados antes do uso
    /// True se uma página folha
    pub leaf: u8,
    /// 100 para página 1. 0 de outro modo
    pub hdr_offset: u8,
    /// 0 se leaf==1. 4 se leaf==0
    pub child_ptr_size: u8,
    /// min(max_local,127)
    pub max1byte_payload: u8,
    /// Número de corpos de célula de overflow em a_cell[]
    pub n_overflow: u8,
    /// Cópia de BtShared.max_local ou BtShared.max_leaf
    pub max_local: u16,
    /// Cópia de BtShared.min_local ou BtShared.min_leaf
    pub min_local: u16,
    /// Índice em a_data do primeiro ponteiro de célula
    pub cell_offset: u16,
    /// Número de bytes livres na página. -1 para desconhecido
    pub n_free: i32,
    /// Número de células nesta página, local e overflow
    pub n_cell: u16,
    /// Máscara para deslocamento de página
    pub mask_page: u16,
    /// Insira a i-ésima célula de overflow antes da célula não-overflow ai_ovfl-th
    pub ai_ovfl: [u16; 4],
    /// Ponteiros para o corpo de células de overflow
    pub ap_ovfl: [Option<Box<[u8]>>; 4],
    /// Ponteiro para BtShared do qual esta página faz parte
    pub p_bt: Option<std::rc::Weak<std::cell::RefCell<BtShared>>>,
    /// Ponteiro para imagem em disco dos dados da página
    pub a_data: Vec<u8>,
    /// Um byte após o final de toda a página, não apenas o
    /// espaço utilizável, toda a página. Usado para prevenir
    /// transbordamento de buffer induzido por corrupção.
    pub a_data_end: usize,
    /// A área de índice de célula (índice em a_data, não uma cópia)
    pub a_cell_idx: usize,
    /// O mesmo que a_data para folhas. a_data+4 para interior
    pub a_data_ofst: usize,
    /// Handle de página do paginador (compartilhado com o paginador)
    pub p_db_page: Option<PgHdrRef>,
    /// Ponteiro para função: método cellSizePtr
    pub x_cell_size: fn(&MemPage, &[u8]) -> u16,
    /// Ponteiro para função: método btreeParseCell
    pub x_parse_cell: fn(&MemPage, &[u8], &mut CellInfo),
}

/// Uma lista encadeada das estruturas a seguir é armazenada em BtShared.p_lock.
/// Os bloqueios são adicionados (ou atualizados de READ_LOCK para WRITE_LOCK) quando um cursor
/// é aberto na tabela com página raiz BtShared.i_table. Os bloqueios são removidos
/// desta lista quando uma transação é confirmada ou revertida, ou quando
/// um identificador btree é fechado.
pub struct BtLock {
    /// Identificador de Btree segurando este bloqueio
    pub p_btree: Option<std::rc::Weak<std::cell::RefCell<Btree>>>,
    /// Página raiz da tabela
    pub i_table: u32,
    /// READ_LOCK ou WRITE_LOCK
    pub e_lock: u8,
    /// Próximo em BtShared.p_lock list
    pub p_next: Option<Box<BtLock>>,
}

/// Valores candidatos para BtLock.e_lock
pub const READ_LOCK: u8 = 1;
pub const WRITE_LOCK: u8 = 2;

/// Um identificador de Btree
///
/// Uma conexão de banco de dados contém um ponteiro para uma instância
/// deste objeto para cada arquivo de banco de dados que ele tem aberto.
/// Esta estrutura é opaca para a conexão do banco de dados.
/// A conexão do banco de dados não pode ver os internals desta estrutura
/// e apenas lida com ponteiros para esta estrutura.
///
/// Para alguns arquivos de banco de dados, o mesmo cache de banco de dados subjacente
/// pode ser compartilhado entre várias conexões. Nesse caso, cada conexão
/// tem sua própria instância deste objeto. Mas cada instância deste objeto
/// aponta para o mesmo objeto BtShared. O cache do banco de dados e o
/// esquema associado ao arquivo de banco de dados estão todos contidos dentro
/// do objeto BtShared.
///
/// Todos os campos desta estrutura são acessados sob sqlite3.mutex.
/// O ponteiro p_bt em si não pode ser alterado enquanto existem cursores
/// em BtShared referenciado que apontam para trás para este Btree, uma vez que esses
/// cursores têm que passar por este Btree para encontrar seu BtShared e
/// frequentemente o fazem sem conter sqlite3.mutex.
pub struct Btree {
    /// A conexão de banco de dados segurando este btree
    pub db: Option<std::rc::Weak<std::cell::RefCell<sqlite3>>>,
    /// Conteúdo compartilhável deste btree
    pub p_bt: Option<std::rc::Rc<std::cell::RefCell<BtShared>>>,
    /// TRANS_NONE, TRANS_READ ou TRANS_WRITE
    pub in_trans: u8,
    /// True se podemos compartilhar p_bt com outro db
    pub sharable: u8,
    /// True se db tem p_bt atualmente bloqueado
    pub locked: u8,
    /// True se existem um ou mais cursores Incrblob
    pub has_incrblob_cur: u8,
    /// Número de chamadas aninhadas para sqlite3BtreeEnter()
    pub want_to_lock: i32,
    /// Número de operações de backup lendo este btree
    pub n_backup: i32,
    /// Combinado com pBt->pPager->i_data_version
    pub i_b_data_version: u32,
    /// Lista de outros Btrees compartilháveis do mesmo db
    pub p_next: Option<BtreeRef>,
    /// Ponteiro para trás da mesma lista
    pub p_prev: Option<std::rc::Weak<std::cell::RefCell<Btree>>>,
    /// Objeto usado para bloquear a página 1 (embutido por valor, como no C)
    pub lock: BtLock,
}


// ---- part_001.rs ----

/// Estados da transação da estrutura Btree.
///
/// Se a extensão de dados compartilhados está habilitada, pode haver múltiplos
/// usuários de Btree. No máximo um pode ter uma transação de escrita aberta,
/// mas qualquer número pode ter transações de leitura ativas.
///
/// Esses valores precisam corresponder a SQLITE_TXN_NONE, SQLITE_TXN_READ e
/// SQLITE_TXN_WRITE.
pub const TRANS_NONE: u8 = 0;
pub const TRANS_READ: u8 = 1;
pub const TRANS_WRITE: u8 = 2;

/// Instância que representa um único arquivo de banco de dados.
///
/// Um arquivo de banco de dados pode estar em uso simultaneamente por duas ou
/// mais conexões. Quando múltiplas conexões compartilham o mesmo arquivo de
/// banco de dados, cada conexão tem seu próprio objeto Btree privado para o
/// arquivo e cada um desses Btrees aponta para este objeto BtShared.
/// BtShared.nRef é o número de conexões que compartilham este arquivo de banco.
///
/// Campos desta estrutura são acessados sob o mutex BtShared.mutex, exceto
/// nRef e pNext que são acessados sob o mutex global SQLITE_MUTEX_STATIC_MAIN.
/// O campo pPager não pode ser modificado uma vez que é configurado inicialmente
/// enquanto nRef > 0. O campo pSchema pode ser configurado uma vez sob
/// BtShared.mutex e depois permanece inalterado enquanto nRef > 0.
///
/// isPending: Se um cliente de BtShared falha ao obter um travamento de escrita
/// em uma tabela de banco de dados (porque há um ou mais travamentos de leitura
/// na tabela), o cache compartilhado entra em estado de 'travamento pendente' e
/// isPending é definido como verdadeiro. O cache compartilhado sai do estado de
/// 'travamento pendente' quando uma das seguintes ocorre:
/// 1) O escritor atual (BtShared.pWriter) conclui sua transação, OU
/// 2) O número de travamentos mantidos por outras conexões cai para zero.
/// Enquanto no estado de 'travamento pendente', nenhuma conexão pode iniciar
/// uma nova transação. Este recurso inclui ajuda para evitar inanição de escritor.
pub struct BtShared {
    /// Cache de páginas
    pub p_pager: Option<PagerRef>,
    /// Conexão de banco de dados usando este Btree no momento
    pub db: Option<std::rc::Weak<std::cell::RefCell<sqlite3>>>,
    /// Lista de todos os cursores abertos
    pub p_cursor: Option<BtCursorRef>,
    /// Primeira página do banco de dados
    pub p_page_1: Option<MemPageRef>,
    /// Bandeiras para sqlite3BtreeOpen()
    pub open_flags: u8,
    /// Verdadeiro se vácuo automático está habilitado
    pub auto_vacuum: u8,
    /// Verdadeiro se vácuo incremental está habilitado
    pub incr_vacuum: u8,
    /// Verdadeiro para truncar o banco de dados ao confirmar
    pub b_do_truncate: u8,
    /// Estado da transação
    pub in_transaction: u8,
    /// Carga máxima do primeiro byte da célula para uma carga útil de 1 byte
    pub max1byte_payload: u8,
    /// Número desejado de bytes extras por página
    pub n_reserve_wanted: u8,
    /// Parâmetros booleanos. Veja macros BTS_*
    pub bts_flags: u16,
    /// Carga útil local máxima em tabelas não-LEAFDATA
    pub max_local: u16,
    /// Carga útil local mínima em tabelas não-LEAFDATA
    pub min_local: u16,
    /// Carga útil local máxima em tabela LEAFDATA
    pub max_leaf: u16,
    /// Carga útil local mínima em tabela LEAFDATA
    pub min_leaf: u16,
    /// Número total de bytes em uma página
    pub page_size: u32,
    /// Número de bytes utilizáveis em cada página
    pub usable_size: u32,
    /// Número de transações abertas (leitura e escrita)
    pub n_transaction: i32,
    /// Número de páginas no banco de dados
    pub n_page: u32,
    /// Ponteiro para espaço alocado por sqlite3BtreeSchema()
    pub p_schema: Option<SchemaRef>,
    /// Destruidor para BtShared.pSchema
    pub x_free_schema: Option<fn(SchemaRef)>,
    /// Mutex não-recursivo necessário para acessar este objeto
    pub mutex: Option<std::rc::Rc<std::cell::RefCell<sqlite3_mutex>>>,
    /// Conjunto de páginas movidas para lista livre esta transação
    pub p_has_content: Option<Box<Bitvec>>,
    /// Número de referências para esta estrutura
    pub n_ref: i32,
    /// Próximo em lista de estruturas BtShared compartilháveis
    pub p_next: Option<BtSharedRef>,
    /// Lista de travamentos mantidos nesta estrutura btree compartilhada
    pub p_lock: Option<Box<BtLock>>,
    /// Btree com transação de escrita aberta no momento
    pub p_writer: Option<std::rc::Weak<std::cell::RefCell<Btree>>>,
    /// Espaço temporário suficiente para conter uma única célula
    pub p_tmp_space: Vec<u8>,
    /// Tamanho da última célula escrita por TransferRow()
    pub n_preformat_size: i32,
}

/// Valores permitidos para BtShared.btsFlags
pub const BTS_READ_ONLY: u16 = 0x0001;
pub const BTS_PAGESIZE_FIXED: u16 = 0x0002;
pub const BTS_SECURE_DELETE: u16 = 0x0004;
pub const BTS_OVERWRITE: u16 = 0x0008;
pub const BTS_FAST_SECURE: u16 = 0x000c;
pub const BTS_INITIALLY_EMPTY: u16 = 0x0010;
pub const BTS_NO_WAL: u16 = 0x0020;
pub const BTS_EXCLUSIVE: u16 = 0x0040;
pub const BTS_PENDING: u16 = 0x0080;

/// Instância desta estrutura é usada para manter informações sobre uma célula.
/// A função parseCellPtr() preenche esta estrutura baseada em informações
/// extraídas da página bruta do disco.
#[derive(Clone, Default)]
pub struct CellInfo {
    /// A chave para tabelas INTKEY, ou nPayload caso contrário
    pub n_key: i64,
    /// Índice do início da carga útil no buffer da célula (no lugar do ponteiro)
    pub p_payload: usize,
    /// Bytes de carga útil
    pub n_payload: u32,
    /// Quantidade de carga útil mantida localmente, não em transbordamento
    pub n_local: u16,
    /// Tamanho do conteúdo da célula na página principal de b-tree
    pub n_size: u16,
}

/// Profundidade máxima de uma estrutura B-Tree do SQLite. Qualquer B-Tree mais
/// profunda que isto será declarada corrupta. Este valor é calculado baseado em
/// um tamanho de banco de dados máximo de 2^31 páginas e um fator de ramificação
/// mínimo de 2 para um nó raiz e 3 para todos outros nós internos.
///
/// Se uma árvore que parece ser mais alta que isto for encontrada, presume-se
/// que o banco de dados está corrompido.
pub const BTCURSOR_MAX_DEPTH: usize = 20;

/// Um cursor é um ponteiro para uma entrada particular dentro de um b-tree
/// particular dentro de um arquivo de banco de dados.
///
/// A entrada é identificada por seu MemPage e o índice no MemPage.aCell[]
/// da entrada.
///
/// Um arquivo de banco de dados pode ser compartilhado por duas ou mais
/// conexões de banco de dados, mas cursores não podem ser compartilhados.
/// Cada cursor está associado com uma conexão de banco de dados particular
/// identificada por BtCursor.pBtree.db.
///
/// Campos desta estrutura são acessados sob o mutex BtShared.mutex
/// encontrado em self->pBt->mutex.
///
/// Significado de skipNext: O significado de skipNext depende do valor de eState:
/// eState=VALID: skipNext é sem significado e é ignorado
/// eState=INVALID: skipNext é sem significado e é ignorado
/// eState=SKIPNEXT: sqlite3BtreeNext() é um não-op se skipNext > 0 e
///                   sqlite3BtreePrevious() é não-op se skipNext < 0.
/// eState=REQUIRESEEK: restoreCursorPosition() restaura o cursor para
///                      eState=SKIPNEXT se skipNext != 0
/// eState=FAULT: skipNext mantém o código de erro de falha do cursor.
pub struct BtCursor {
    /// Um dos constantes CURSOR_XXX (veja abaixo)
    pub e_state: u8,
    /// Zero ou mais bandeiras BTCF_* definidas abaixo
    pub cur_flags: u8,
    /// Bandeiras para enviar a sqlite3PagerGet()
    pub cur_pager_flags: u8,
    /// Como configurado por CursorSetHints()
    pub hints: u8,
    /// Prev() é não-op se negativo. Next() é não-op se positivo.
    /// Código de erro se eState==CURSOR_FAULT
    pub skip_next: i32,
    /// O Btree ao qual este cursor pertence
    pub p_btree: Option<BtreeRef>,
    /// Cache de localizações de página de transbordamento
    pub a_overflow: Vec<u32>,
    /// Chave salva da última posição conhecida do cursor
    pub p_key: Vec<u8>,
    /// Todos os campos acima são zerados quando o cursor é alocado. Veja
    /// sqlite3BtreeCursorZero(). Campos que seguem precisam ser inicializados
    /// manualmente (BTCURSOR_FIRST_UNINIT é p_bt).
    /// BtShared este cursor aponta para
    pub p_bt: Option<BtSharedRef>,
    /// Forma uma lista encadeada de todos os cursores
    pub p_next: Option<BtCursorRef>,
    /// Uma análise da célula para a qual estamos apontando
    pub info: CellInfo,
    /// Tamanho de pKey, ou última chave inteira
    pub n_key: i64,
    /// A página raiz dessa árvore
    pub pgno_root: u32,
    /// Índice da página atual em apPage
    pub i_page: i8,
    /// Valor de apPage[0]->intKey
    pub cur_int_key: u8,
    /// Índice atual para apPage[iPage]
    pub ix: u16,
    /// Índice atual em apPage[i]
    pub ai_idx: [u16; BTCURSOR_MAX_DEPTH - 1],
    /// Argumento passado para função de comparação
    pub p_key_info: Option<std::rc::Rc<KeyInfo>>,
    /// Página atual
    pub p_page: Option<MemPageRef>,
    /// Pilha de pais da página atual
    pub ap_page: [Option<MemPageRef>; BTCURSOR_MAX_DEPTH - 1],
}

pub type MemPageRef = std::rc::Rc<std::cell::RefCell<MemPage>>;
pub type BtreeRef = std::rc::Rc<std::cell::RefCell<Btree>>;
pub type BtSharedRef = std::rc::Rc<std::cell::RefCell<BtShared>>;
pub type BtCursorRef = std::rc::Rc<std::cell::RefCell<BtCursor>>;

/// Valores legais para BtCursor.curFlags
pub const BTCF_WRITE_FLAG: u8 = 0x01;
pub const BTCF_VALID_NKEY: u8 = 0x02;
pub const BTCF_VALID_OVFL: u8 = 0x04;
pub const BTCF_AT_LAST: u8 = 0x08;
pub const BTCF_INCRBLOB: u8 = 0x10;
pub const BTCF_MULTIPLE: u8 = 0x20;
pub const BTCF_PINNED: u8 = 0x40;

/// Valores potenciais para BtCursor.eState.
///
/// CURSOR_INVALID: Cursor não aponta para uma entrada válida. Isto pode
/// acontecer (por exemplo) porque a tabela está vazia ou porque
/// BtreeCursorFirst() não foi chamado.
///
/// CURSOR_VALID: Cursor aponta para uma entrada válida. getPayload() etc.
/// podem ser chamados.
///
/// CURSOR_SKIPNEXT: Cursor é válido exceto que o campo Cursor.skipNext é
/// não-zero indicando que a próxima operação sqlite3BtreeNext() ou
/// sqlite3BtreePrevious() deve ser um não-op.
///
/// CURSOR_REQUIRESEEK: A tabela na qual este cursor foi aberto ainda existe,
/// mas foi modificada desde o último uso do cursor. A posição do cursor é
/// salva em variáveis BtCursor.pKey e BtCursor.nKey. Quando um cursor está
/// neste estado, restoreCursorPosition() pode ser chamada para tentar procurar
/// o cursor para a posição salva.
///
/// CURSOR_FAULT: Um erro irrecuperável (erro de E/S ou falha de malloc)
/// ocorreu em uma conexão diferente que compartilha o cache BtShared com este
/// cursor. O erro deixou o cache em estado inconsistente. Não faça mais nada
/// com este cursor. Qualquer tentativa de usar o cursor deve retornar o código
/// de erro armazenado em BtCursor.skipNext
pub const CURSOR_VALID: u8 = 0;
pub const CURSOR_INVALID: u8 = 1;
pub const CURSOR_SKIPNEXT: u8 = 2;
pub const CURSOR_REQUIRESEEK: u8 = 3;
pub const CURSOR_FAULT: u8 = 4;

/// A página de banco de dados que PENDING_BYTE ocupa. Esta página nunca é usada.
#[inline]
pub fn pending_byte_page(p_bt: &BtShared) -> u32 {
    ((PENDING_BYTE as u32) / p_bt.page_size) + 1
}

/// Estas macros definem a localização da entrada de mapa-de-ponteiro para uma
/// página de banco de dados. O primeiro argumento para cada uma é o número de
/// bytes utilizáveis em cada página do banco de dados (frequentemente 1024).
/// O segundo é o número de página a procurar no mapa-de-ponteiro.
///
/// PTRMAP_PAGENO retorna o número de página de banco de dados da página
/// mapa-de-ponteiro que armazena o ponteiro necessário. PTRMAP_PTROFFSET
/// retorna o deslocamento da entrada de mapa solicitada.
///
/// Se o argumento pgno passado para PTRMAP_PAGENO é uma página mapa-de-ponteiro,
/// então pgno é retornado. Então (pgno==PTRMAP_PAGENO(pgsz, pgno)) pode ser
/// usado para testar se pgno é uma página mapa-de-ponteiro. PTRMAP_ISPAGE
/// implementa este teste.
/// PTRMAP_PAGENO é a função `ptrmap_pageno` de btree.c (o chamador a usa direto).
#[inline]
pub fn ptrmap_ptroffset(pgptrmap: u32, pgno: u32) -> u32 {
    5u32.wrapping_mul(pgno.wrapping_sub(pgptrmap).wrapping_sub(1))
}

#[inline]
pub fn ptrmap_ispage(p_bt: &BtShared, pgno: u32) -> bool {
    ptrmap_pageno(p_bt, pgno) == pgno
}

/// O mapa-de-ponteiro é uma tabela de consulta que identifica a página pai para
/// cada página filha no arquivo de banco de dados. A página pai é a página que
/// contém um ponteiro para a filha. Cada página no banco de dados contém
/// 0 ou 1 páginas pai. (Neste contexto, 'página de banco de dados' refere-se
/// a qualquer página que não faz parte do mapa-de-ponteiro em si.) Cada entrada
/// de mapa-de-ponteiro consiste de um único byte 'tipo' e um número de página
/// pai de 4 bytes. Os identificadores PTRMAP_XXX abaixo são os tipos válidos.
///
/// O propósito do mapa-de-ponteiro é facilitar a movimentação de páginas de
/// uma posição no arquivo para outra como parte de vácuo automático. Quando uma
/// página é movida, o ponteiro em sua página pai precisa ser atualizado para
/// apontar para a nova localização. O mapa-de-ponteiro é usado para localizar
/// a página pai rapidamente.
///
/// PTRMAP_ROOTPAGE: A página de banco de dados é uma página raiz. O número de
///                  página não é usado neste caso.
///
/// PTRMAP_FREEPAGE: A página de banco de dados é uma página não usada (livre).
///                  O número de página não é usado neste caso.
///
/// PTRMAP_OVERFLOW1: A página de banco de dados é a primeira página em uma
///                   lista de páginas de transbordamento. O número de página
///                   identifica a página que contém a célula com um ponteiro
///                   para esta página de transbordamento.
///
/// PTRMAP_OVERFLOW2: A página de banco de dados é a segunda ou posterior página
///                   em uma lista de páginas de transbordamento. O número de
///                   página identifica a página anterior na lista de página de
///                   transbordamento.
///
/// PTRMAP_BTREE: A página de banco de dados é uma página btree não-raiz. O
///               número de página identifica a página pai no btree.
pub const PTRMAP_ROOTPAGE: u8 = 1;
pub const PTRMAP_FREEPAGE: u8 = 2;
pub const PTRMAP_OVERFLOW1: u8 = 3;
pub const PTRMAP_OVERFLOW2: u8 = 4;
pub const PTRMAP_BTREE: u8 = 5;

/// Um lote de asserções para verificar que as variáveis de estado da transação
/// de um identificador p (tipo Btree*) são internamente consistentes.
#[inline]
pub fn btree_integrity(p: &Btree) -> bool {
    let bt = p.p_bt.as_ref().unwrap().borrow();
    (bt.in_transaction != TRANS_NONE || bt.n_transaction == 0) && (bt.in_transaction >= p.in_trans)
}

/// A macro ISAUTOVACUUM é usada dentro de balance_nonroot() para determinar
/// se o banco de dados suporta vácuo automático ou não. Porque ela é usada
/// dentro de uma expressão que é um argumento para outra macro (sqliteMallocRaw),
/// não é possível usar compilação condicional. Então, esta macro é definida
/// em vez disso.
#[inline]
pub fn is_autovacuum(p_bt: &BtShared) -> bool {
    p_bt.auto_vacuum != 0
}

/// Esta estrutura é passada através de todas as rotinas de verificação
/// integridade de PRAGMA em ordem de manter controle de algumas informações
/// de estado global.
///
/// O array aRef[] é alocado de forma que há 1 bit para cada página no banco
/// de dados. Conforme a verificação de integridade prossegue, para cada página
/// usada no banco de dados o bit correspondente é definido. Isto permite que
/// verificação de integridade detecte páginas que são usadas duas vezes e
/// páginas órfãs (ambas indicando corrupção).
pub struct IntegrityCk {
    /// A árvore sendo verificada
    pub p_bt: Option<BtSharedRef>,
    /// O pager associado. Também acessível por pBt->pPager
    pub p_pager: Option<PagerRef>,
    /// 1 bit por página no banco de dados (veja acima)
    pub a_pg_ref: Vec<u8>,
    /// Páginas no banco de dados. 0 para verificação parcial
    pub n_ck_page: u32,
    /// Parar de acumular erros quando isto alcança zero
    pub mx_err: i32,
    /// Número de mensagens escritas para zErrMsg até agora
    pub n_err: i32,
    /// SQLITE_OK, SQLITE_NOMEM, ou SQLITE_INTERRUPT
    pub rc: i32,
    /// Número de etapas no processo de integrity_check
    pub n_step: u32,
    /// Prefixo de mensagem de erro
    pub z_pfx: Option<Vec<u8>>,
    /// Valor para primeira substituição %u em zPfx (página raiz)
    pub v0: u32,
    /// Valor para segunda substituição %u em zPfx (página atual)
    pub v1: u32,
    /// Valor para terceira substituição %d em zPfx
    pub v2: i32,
    /// Acumular o texto da mensagem de erro aqui
    pub err_msg: StrAccum,
    /// Min-heap usada para analisar cobertura de célula
    pub heap: Vec<u32>,
    /// Conexão de banco de dados executando a verificação
    pub db: Option<SqliteRef>,
    /// Número de linhas visitadas na árvore atual
    pub n_row: i64,
}


// ---- part_002.rs ----

/// Lê um inteiro big-endian de dois bytes.
#[inline]
pub fn get2byte(x: &[u8]) -> u16 {
    ((x[0] as u16) << 8) | (x[1] as u16)
}

/// Escreve um inteiro big-endian de dois bytes.
#[inline]
pub fn put2byte(p: &mut [u8], v: u16) {
    p[0] = (v >> 8) as u8;
    p[1] = v as u8;
}

/// `get4byte` e `put4byte` são `sqlite3Get4byte` e `sqlite3Put4byte` (util.c),
/// que pela convenção de nomes já se chamam `get4byte` e `put4byte`: não há
/// definição aqui.

/// Lê um inteiro big-endian de dois bytes a partir de um endereço alinhado.
///
/// Diferentemente de `get2byte()`, requer que seu argumento aponte para um
/// endereço alinhado a dois bytes. É usado apenas para acessar os endereços de
/// célula no cabeçalho de uma b-tree. O ramo portável do C (`x[0]<<8 | x[1]`)
/// é o único que existe sobre fatias.
pub use self::get2byte as get2byte_aligned;

