// Mesclado das partes traduzidas de vdbeInt_h (as partes quebram funções grandes no meio).
#![allow(unused_imports, ambiguous_glob_reexports)]
use crate::prelude::*;

// ---- part_000.rs ----


/// Número máximo de vezes que uma declaração tenta reparsar a si mesma
/// antes de desistir e retornar SQLITE_SCHEMA. O Debian 13 compila com 25.
pub const SQLITE_MAX_SCHEMA_RETRY: i32 = 25;

/// Verdadeiro quando a lógica de exibição P4 de "explain" está habilitada.
/// No Debian 13 `SQLITE_OMIT_EXPLAIN` não está definido, então vale sempre 1.
pub const VDBE_DISPLAY_P4: u8 = 1;

/// Cada instrução do programa é uma instância de `VdbeOp`.
pub type Op = VdbeOp;

/// Valores booleanos (`typedef unsigned Bool`).
pub type Bool = u32;

/// Referência compartilhada a um cursor do VDBE (`apCsr[]` e `pAltCursor`
/// apontam para o mesmo objeto).
pub type VdbeCursorRef = Rc<RefCell<VdbeCursor>>;

/// Tipos de cursores VDBE.
pub const CURTYPE_BTREE: u8 = 0;
pub const CURTYPE_SORTER: u8 = 1;
pub const CURTYPE_VTAB: u8 = 2;
pub const CURTYPE_PSEUDO: u8 = 3;

/// Um VdbeCursor é uma superclasse (um envoltório) para vários objetos de cursor:
/// * Um cursor de árvore B:
///   - no banco de dados principal ou em um banco de dados efêmero
///   - em um índice ou em uma tabela
/// * Um classificador
/// * Uma tabela virtual
/// * Uma "pseudotabela" de uma linha armazenada em um único registro.
pub struct VdbeCursor {
    /// Um dos valores CURTYPE_*.
    pub e_cur_type: u8,
    /// Índice do banco de dados do cursor em db->a_db[].
    pub i_db: i8,
    /// Verdadeiro se apontando para uma linha sem dados.
    pub null_row: u8,
    /// Uma chamada para btree_moveto() é necessária.
    pub deferred_moveto: u8,
    /// Verdadeiro para tabelas rowid. Falso para índices.
    pub is_table: u8,
    /// Verdadeiro para uma tabela efêmera (campo de 1 bit no C).
    pub is_ephemeral: Bool,
    /// Gera números de registro semialeatoriamente (campo de 1 bit no C).
    pub use_random_rowid: Bool,
    /// Verdadeiro se a tabela não é BTREE_UNORDERED (campo de 1 bit no C).
    pub is_ordered: Bool,
    /// OpenEphemeral não pode reusar este cursor (campo de 1 bit no C).
    pub no_reuse: Bool,
    /// O campo p_cache está inicializado e não é nulo (campo de 1 bit no C).
    pub col_cache: Bool,
    /// Ver opcodes OP_SeekHit e OP_IfNoHope.
    pub seek_hit: u16,
    /// pBtx para is_ephemeral, pAltMap caso contrário.
    pub ub: VdbeCursorUnion,
    /// Contador de sequência.
    pub seq_count: i64,

    /// O cache parseado de OP_Column só é válido se cache_status for igual a
    /// Vdbe.cache_ctr. Vdbe.cache_ctr nunca assume o valor de CACHE_STALE (0),
    /// portanto atribuir cache_status=CACHE_STALE garante que o cache está
    /// desatualizado.
    pub cache_status: u32,

    /// Resultado do btree_moveto() anterior, ou 0 se não houve buscas anteriores.
    /// Para CURTYPE_PSEUDO, seek_result é o registro que contém o registro.
    pub seek_result: i32,

    /// Cursor de índice associado do qual ler (compartilhado com ap_csr).
    pub p_alt_cursor: Option<VdbeCursorRef>,

    /// Cursor de árvore B (CURTYPE_BTREE ou _PSEUDO), cursor de tabela virtual
    /// (CURTYPE_VTAB) ou objeto classificador (CURTYPE_SORTER).
    pub uc: VdbeCursorCursorUnion,

    /// Informação sobre as chaves de índice necessárias pelos cursores de índice.
    pub p_key_info: Option<Rc<KeyInfo>>,

    /// Deslocamento para o próximo byte não parseado do cabeçalho.
    pub i_hdr_offset: u32,
    /// Página raiz do cursor de árvore B aberto.
    pub pgno_root: u32,
    /// Número de campos no cabeçalho.
    pub n_field: i16,
    /// Número de campos de cabeçalho parseados até agora.
    pub n_hdr_parsed: u16,
    /// Argumento para o btree_moveto() diferido.
    pub moveto_target: i64,
    /// Equivale a aType[n_field]: n_field+1 posições.
    pub a_offset: Vec<u32>,
    /// Dados da linha atual, se estiver toda em uma página.
    pub a_row: Option<Vec<u8>>,
    /// Número total de bytes no registro.
    pub payload_size: u32,
    /// Bytes disponíveis em a_row.
    pub sz_row: u32,
    /// Cache de valores TEXT ou BLOB grandes.
    pub p_cache: Option<Box<VdbeTxtBlbCache>>,
    /// Valores de tipo da decodificação do registro: n_field posições.
    /// DEVE SER O ÚLTIMO CAMPO no C; aqui é um Vec dimensionado pelo chamador.
    pub a_type: Vec<u32>,
}

/// União do C para pBtx (efêmero) ou pAltMap (caso contrário), modelada como enum.
pub enum VdbeCursorUnion {
    /// Arquivo separado com a tabela temporária.
    PBtx(BtreeRef),
    /// Mapeamento de colunas da tabela para colunas do índice.
    AAltMap(Vec<u32>),
    /// Nenhum dos dois ainda (memória zerada).
    None,
}

/// União do C para o tipo de cursor: Btree, Vtab ou Sorter, modelada como enum.
pub enum VdbeCursorCursorUnion {
    /// CURTYPE_BTREE ou _PSEUDO: cursor de árvore B.
    PCursor(Box<BtCursor>),
    /// CURTYPE_VTAB: cursor de tabela virtual.
    PVCur(Box<sqlite3_vtab_cursor>),
    /// CURTYPE_SORTER: objeto classificador.
    PSorter(Box<VdbeSorter>),
    /// Ainda não inicializado.
    None,
}

/// Retorna verdadeiro se P é um cursor apenas nulo.
#[inline]
pub fn is_null_cursor(p: &VdbeCursor) -> bool {
    p.e_cur_type == CURTYPE_PSEUDO && p.null_row != 0 && p.seek_result == 0
}

/// Um valor para VdbeCursor.cache_status que significa que o cache é sempre inválido.
pub const CACHE_STALE: u32 = 0;

/// Valores TEXT ou BLOB grandes podem ser lentos para carregar, portanto queremos
/// evitar carregá-los mais de uma vez. Por esse motivo, eles podem ser guardados
/// em um cache definido por este objeto e anexado ao VdbeCursor pelo campo p_cache.
pub struct VdbeTxtBlbCache {
    /// Um buffer RCStr para manter o valor.
    pub p_c_value: Option<Vec<u8>>,
    /// Deslocamento de arquivo da linha em cache.
    pub i_offset: i64,
    /// Coluna para a qual o cache é válido.
    pub i_col: i32,
    /// Valor de Vdbe.cache_ctr.
    pub cache_status: u32,
    /// Contador do cache de coluna.
    pub col_cache_ctr: u32,
}

/// Quando um subprograma é executado (OP_Program), uma estrutura deste tipo
/// é alocada para guardar o valor atual do contador de programa, o array de
/// células de memória atual e vários outros valores específicos do quadro
/// guardados na estrutura Vdbe. Quando o subprograma termina, esses valores são
/// copiados de volta para o Vdbe a partir do VdbeFrame, restaurando o estado da
/// VM como era antes de o subprograma começar.
///
/// A memória de um objeto VdbeFrame é gerenciada por uma célula de memória no
/// quadro pai (chamador). Quando a célula é apagada ou sobrescrita, o VdbeFrame
/// não é liberado de imediato: ele é encadeado na lista Vdbe.p_del_frame, cujo
/// conteúdo é apagado quando a VM é reiniciada em vdbe_halt(). Isso evita
/// chamadas recursivas a vdbe_mem_release() quando as células do quadro filho
/// são liberadas.
///
/// O quadro em execução fica em Vdbe.p_frame, que é None quando o quadro em
/// execução é o programa principal.
pub struct VdbeFrame {
    /// VM à qual este quadro pertence (ponteiro de volta, fraco).
    pub v: Option<Weak<RefCell<Vdbe>>>,
    /// Pai deste quadro, ou None se o pai for o programa principal.
    pub p_parent: Option<Rc<RefCell<VdbeFrame>>>,
    /// Instruções do programa do quadro pai.
    pub a_op: Vec<Op>,
    /// Array de células de memória do quadro pai.
    pub a_mem: Vec<MemRef>,
    /// Array de cursores Vdbe do quadro pai.
    pub ap_csr: Vec<Option<VdbeCursorRef>>,
    /// Máscara de bits usada por OP_Once.
    pub a_once: Vec<u8>,
    /// Cópia de SubProgram.token (identificador opaco, só comparado por igualdade).
    pub token: usize,
    /// Último rowid inserido (sqlite3.last_rowid).
    pub last_rowid: i64,
    /// Lista encadeada de alocações auxdata.
    pub p_aux_data: Option<Box<AuxData>>,
    /// Número de entradas em ap_csr.
    pub n_cursor: i32,
    /// Contador de programa no quadro pai (chamador).
    pub pc: i32,
    /// Tamanho do array a_op.
    pub n_op: i32,
    /// Número de entradas em a_mem.
    pub n_mem: i32,
    /// Número de células de memória do quadro filho.
    pub n_child_mem: i32,
    /// Número de cursores do quadro filho.
    pub n_child_csr: i32,
    /// Mudanças da declaração (Vdbe.n_change).
    pub n_change: i64,
    /// Valor de db->n_change.
    pub n_db_change: i64,
    /// Registradores alocados logo após o VdbeFrame no C (área apontada por
    /// VdbeFrameMem); aqui ficam em um Vec próprio.
    pub a_frame_mem: Vec<MemRef>,
}

/// Número mágico para verificação de sanidade em objetos VdbeFrame.
pub const SQLITE_FRAME_MAGIC: u32 = 0x879fb71e;

/// Retorna o array de registradores alocado para uso por um VdbeFrame.
#[inline]
pub fn vdbe_frame_mem(p: &mut VdbeFrame) -> &mut Vec<MemRef> {
    &mut p.a_frame_mem
}

/// Internamente, o vdbe manipula quase todos os valores SQL como estruturas Mem.
/// Cada Mem pode guardar várias representações (texto, inteiro etc.) do mesmo valor.
#[derive(Clone)]
pub struct Mem {
    /// Valor (união MemValue do C).
    pub u: MemValue,
    /// Valor texto ou BLOB.
    pub z: Vec<u8>,
    /// Número de caracteres no texto, excluindo o '\0'.
    pub n: i32,
    /// Combinação de MEM_NULL, MEM_STR, MEM_DYN etc.
    pub flags: u16,
    /// SQLITE_UTF8, SQLITE_UTF16BE, SQLITE_UTF16LE.
    pub enc: u8,
    /// Subtipo deste valor.
    pub e_subtype: u8,
    /// Conexão de banco de dados associada (ponteiro de volta, fraco).
    pub db: Option<Weak<RefCell<sqlite3>>>,
    /// Tamanho da alocação z_malloc.
    pub sz_malloc: i32,
    /// Armazenamento transiente do serial_type em OP_MakeRecord.
    pub u_temp: u32,
    /// Espaço para guardar MEM_STR ou MEM_BLOB se sz_malloc>0.
    pub z_malloc: Vec<u8>,
    /// Destrutor de Mem.z, válido apenas com MEM_DYN.
    pub x_del: Option<fn(Vec<u8>)>,
}

/// União do C para as representações de um valor Mem. As variantes do C
/// compartilham armazenamento; aqui cada uma tem seu campo e o código que
/// depende do aliasing (r lido como i) deve converter explicitamente.
#[derive(Clone, Default)]
pub struct MemValue {
    /// Valor real, usado quando MEM_REAL está em flags.
    pub r: f64,
    /// Valor inteiro, usado quando MEM_INT está em flags.
    pub i: i64,
    /// Bytes zero extras quando MEM_ZERO e MEM_BLOB estão em flags.
    pub n_zero: i32,
    /// Tipo do ponteiro quando MEM_TERM|MEM_SUBTYPE|MEM_NULL.
    pub z_p_type: Option<&'static [u8]>,
    /// Usado apenas quando flags==MEM_AGG.
    pub p_def: Option<Rc<FuncDef>>,
}

/// Flags que indicam as representações do valor guardado na estrutura Mem.
///
/// * MEM_NULL: um valor SQL NULL
/// * MEM_NULL|MEM_ZERO: um SQL NULL com a flag de "sem mudança" do UPDATE de
///   tabela virtual
/// * MEM_NULL|MEM_TERM|MEM_SUBTYPE: um SQL NULL que também carrega um ponteiro
///   acessível por value_pointer()
/// * MEM_NULL|MEM_CLEARED: NULL especial que compara diferente de outros NULLs,
///   mesmo com o operador IS
/// * MEM_STR: texto em Mem.z com comprimento Mem.n, terminado em zero se MEM_TERM
///   estiver ligado. Incompatível com MEM_BLOB e MEM_NULL, mas pode coexistir
///   com MEM_INT, MEM_REAL e MEM_INTREAL.
/// * MEM_BLOB: blob em Mem.z com comprimento Mem.n. Incompatível com MEM_STR,
///   MEM_NULL, MEM_INT, MEM_REAL e MEM_INTREAL.
/// * MEM_BLOB|MEM_ZERO: blob de comprimento Mem.n mais Mem.u.n_zero bytes 0x00
///   no final.
/// * MEM_INT: inteiro em Mem.u.i.
/// * MEM_REAL: real em Mem.u.r.
/// * MEM_INTREAL: real guardado como inteiro em Mem.u.i.
pub const MEM_UNDEFINED: u16 = 0x0000;
pub const MEM_NULL: u16 = 0x0001;
pub const MEM_STR: u16 = 0x0002;
pub const MEM_INT: u16 = 0x0004;
pub const MEM_REAL: u16 = 0x0008;
pub const MEM_BLOB: u16 = 0x0010;
pub const MEM_INTREAL: u16 = 0x0020;
pub const MEM_AFFMASK: u16 = 0x003f;

/// Bits extras que modificam o significado dos tipos de dados acima.
pub const MEM_FROMBIND: u16 = 0x0040;
// 0x0080 está disponível.
pub const MEM_CLEARED: u16 = 0x0100;
pub const MEM_TERM: u16 = 0x0200;
pub const MEM_ZERO: u16 = 0x0400;
pub const MEM_SUBTYPE: u16 = 0x0800;
pub const MEM_TYPEMASK: u16 = 0x0dbf;

/// Bits que determinam o armazenamento de Mem.z para texto, blob ou
/// acumulador de função agregada.
pub const MEM_DYN: u16 = 0x1000;
pub const MEM_STATIC: u16 = 0x2000;
pub const MEM_EPHEM: u16 = 0x4000;
pub const MEM_AGG: u16 = 0x8000;

/// Retorna verdadeiro se o Mem X contém conteúdo alocado dinamicamente, ou seja,
/// algo que precisa ser liberado para não vazar.
#[inline]
pub fn vdbe_mem_dynamic(x: &Mem) -> bool {
    (x.flags & (MEM_AGG | MEM_DYN)) != 0
}

/// Limpa os bits de tipo existentes de um Mem e os substitui por f.
#[inline]
pub fn mem_set_type_flag(p: &mut Mem, f: u16) {
    p.flags = (p.flags & !(MEM_TYPEMASK | MEM_ZERO)) | f;
}

/// Verdadeiro se o Mem X é do tipo NULL "sem mudança".
#[inline]
pub fn mem_null_nochng(x: &Mem) -> bool {
    (x.flags & MEM_TYPEMASK) == (MEM_NULL | MEM_ZERO) && x.n == 0 && x.u.n_zero == 0
}

/// Cópia rasa (o que no C é o memcpy dos MEMCELLSIZE primeiros bytes, até o
/// campo db): copia u, z, n, flags, enc e e_subtype, e só esses.
#[inline]
pub fn mem_shallow_copy(to: &mut Mem, from: &Mem) {
    to.u = from.u.clone();
    to.z = from.z.clone();
    to.n = from.n;
    to.flags = from.flags;
    to.enc = from.enc;
    to.e_subtype = from.e_subtype;
}

/// Cada ponteiro de dados auxiliares guardado por uma função definida pelo
/// usuário que chama set_auxdata() fica em uma instância desta estrutura.
/// Todas as estruturas de uma mesma VM formam uma lista encadeada com cabeça em
/// Vdbe.p_aux_data. Todas são destruídas quando a VM é interrompida (se não antes).
pub struct AuxData {
    /// Número da instrução do opcode OP_Function.
    pub i_aux_op: i32,
    /// Índice do argumento da função.
    pub i_aux_arg: i32,
    /// Dados auxiliares.
    pub p_aux: Option<Rc<dyn Any>>,
    /// Destrutor dos dados auxiliares.
    pub x_delete_aux: Option<fn(Rc<dyn Any>)>,
    /// Próximo elemento da lista.
    pub p_next_aux: Option<Box<AuxData>>,
}


// ---- part_001.rs ----


/// Referência compartilhada a um registrador (Mem). `sqlite3_value` é o mesmo
/// objeto que `Mem`: ponteiros para células de `Vdbe.a_mem` viram esta referência.
pub type MemRef = Rc<RefCell<Mem>>;

/// Referência compartilhada a uma VM.
pub type VdbeRef = Rc<RefCell<Vdbe>>;

/// Referência compartilhada a um quadro de subprograma.
pub type VdbeFrameRef = Rc<RefCell<VdbeFrame>>;

/// Argumento "contexto" de uma função instalável. Uma instância desta estrutura
/// é o primeiro argumento das rotinas que implementam as funções SQL.
///
/// Existe um typedef para esta estrutura em sqlite.h, então todas as rotinas,
/// inclusive a interface pública, podem usar um ponteiro para ela. Mas este
/// arquivo é o único lugar onde os detalhes internos são conhecidos.
///
/// A estrutura é definida em vdbeInt.h porque usa subestruturas (Mem) que só
/// são definidas ali.
pub struct sqlite3_context {
    /// O valor de retorno é guardado aqui.
    pub p_out: MemRef,
    /// Informação da função.
    pub p_func: Rc<FuncDef>,
    /// Célula de memória usada para guardar o contexto de agregação.
    pub p_mem: Option<MemRef>,
    /// A VM dona deste contexto (ponteiro de volta, fraco).
    pub p_vdbe: Weak<RefCell<Vdbe>>,
    /// Número da instrução de OP_Function.
    pub i_op: i32,
    /// Código de erro retornado pela função.
    pub is_error: i32,
    /// Codificação a usar nos resultados.
    pub enc: u8,
    /// Pula o carregamento do acumulador se verdadeiro.
    pub skip_flag: u8,
    /// Número de argumentos.
    pub argc: u8,
    /// Conjunto de argumentos (argc posições).
    pub argv: Vec<MemRef>,
}

/// Tipo de campo de bits para uso dentro de estruturas (`typedef unsigned bft`).
/// No C vem sempre seguido de `:N`; aqui o valor é mantido dentro da largura.
pub type Bft = u32;

/// O objeto ScanStatus guarda um único valor para a interface
/// sqlite3_stmt_scanstatus().
///
/// aAddrRange[]:
///   Este array é usado por elementos ScanStatus associados a notas EQP que
///   tornam disponível um valor SQLITE_SCANSTAT_NCYCLE. É um array de até 3
///   intervalos de endereços da VM cujos valores Vdbe.anCycle[] devem ser somados
///   para calcular o NCYCLE. Cada par de endereços inteiros é um endereço inicial
///   e final (ambos inclusivos) de um intervalo de instruções. Um valor inicial
///   0 indica um intervalo vazio.
pub struct ScanStatus {
    /// OP_Explain do loop.
    pub addr_explain: i32,
    pub a_addr_range: [i32; 6],
    /// Endereço do contador de "loops".
    pub addr_loop: i32,
    /// Endereço do contador de "linhas visitadas".
    pub addr_visit: i32,
    /// O "Select-ID" deste loop.
    pub i_select_id: i32,
    /// Linhas de saída estimadas por loop.
    pub n_est: LogEst,
    /// Nome da tabela ou do índice.
    pub z_name: Option<Vec<u8>>,
}

/// O objeto DblquoteStr guarda o texto de uma string entre aspas duplas de um
/// prepared statement. Uma lista encadeada desses objetos é construída durante o
/// parse da declaração e fica em Vdbe.p_dbl_str. Ao calcular o SQL normalizado
/// de uma declaração, a lista é consultada para cada identificador entre aspas
/// duplas, para ver se ele deveria ser um literal de string.
pub struct DblquoteStr {
    /// Próximo literal de string da lista.
    pub p_next_str: Option<Box<DblquoteStr>>,
    /// Valor sem aspas da string (no C, array de tamanho variável).
    pub z: Vec<u8>,
}

/// Uma instância da máquina virtual. Esta estrutura contém o estado completo da
/// máquina virtual.
///
/// O ponteiro "sqlite3_stmt" retornado por prepare() é na verdade um ponteiro
/// para uma instância desta estrutura.
pub struct Vdbe {
    /// A conexão de banco de dados dona desta declaração (ponteiro de volta, fraco).
    pub db: Weak<RefCell<sqlite3>>,
    /// Lista encadeada de VDBEs com o mesmo Vdbe.db: elo anterior (ppVPrev).
    pub pp_v_prev: Option<Weak<RefCell<Vdbe>>>,
    /// Lista encadeada de VDBEs com o mesmo Vdbe.db: próximo (pVNext).
    pub p_v_next: Option<VdbeRef>,
    /// Contexto de parse usado para criar este Vdbe.
    pub p_parse: Option<Weak<RefCell<Parse>>>,
    /// Número de entradas em a_var[].
    pub n_var: YnVar,
    /// Número de posições de memória alocadas no momento.
    pub n_mem: i32,
    /// Número de slots em ap_csr[].
    pub n_cursor: i32,
    /// Contador de geração do cache de linha do VdbeCursor.
    pub cache_ctr: u32,
    /// O contador de programa.
    pub pc: i32,
    /// Valor a retornar.
    pub rc: i32,
    /// Número de mudanças no banco desde o último reset.
    pub n_change: i64,
    /// Número da declaração (ou 0 se não tem declaração aberta).
    pub i_statement: i32,
    /// Valor de julianday('now') para esta declaração.
    pub i_current_time: i64,
    /// Número de restrições FK imediatas desta VM.
    pub n_fk_constraint: i64,
    /// Número de restrições diferidas quando a declaração começou.
    pub n_stmt_def_cons: i64,
    /// Número de restrições diferidas imediatas quando a declaração começou.
    pub n_stmt_def_imm_cons: i64,
    /// As posições de memória.
    pub a_mem: Vec<MemRef>,
    /// Argumentos da função de usuário em execução.
    pub ap_arg: Vec<MemRef>,
    /// Um elemento para cada cursor aberto.
    pub ap_csr: Vec<Option<VdbeCursorRef>>,
    /// Valores do opcode OP_Variable.
    pub a_var: Vec<MemRef>,

    // Ao alocar um novo objeto Vdbe, todos os campos abaixo devem ser
    // inicializados com zero ou None.
    /// Espaço para o programa da máquina virtual.
    pub a_op: Vec<Op>,
    /// Número de instruções do programa.
    pub n_op: i32,
    /// Slots alocados para a_op[].
    pub n_op_alloc: i32,
    /// Nomes de coluna a retornar.
    pub a_col_name: Vec<MemRef>,
    /// Linha de saída atual (índice em a_mem no C é um ponteiro; aqui a referência).
    pub p_result_row: Option<MemRef>,
    /// Mensagem de erro escrita aqui.
    pub z_err_msg: Option<Vec<u8>>,
    /// Nomes das variáveis (VList do C, array de inteiros).
    pub p_v_list: Option<Vec<i32>>,
    /// Hora em que a consulta começou, usada em profiling (SQLITE_OMIT_TRACE não definido).
    pub start_time: i64,
    /// Número de colunas em uma linha do conjunto de resultados.
    pub n_res_column: u16,
    /// Slots de coluna alocados para a_col_name[].
    pub n_res_alloc: u16,
    /// Ação de recuperação em caso de erro.
    pub error_action: u8,
    /// Formato de arquivo mínimo para bancos graváveis.
    pub min_write_file_format: u8,
    /// Flags SQLITE_PREPARE_*.
    pub prep_flags: u8,
    /// Um dos valores VDBE_*_STATE.
    pub e_vdbe_state: u8,
    /// Campo de 2 bits: 1 recompila a VM imediatamente, 2 quando conveniente.
    pub expired: Bft,
    /// Campo de 2 bits: 0 normal, 1 EXPLAIN, 2 EXPLAIN QUERY PLAN.
    pub explain: Bft,
    /// Verdadeiro para atualizar o contador de mudanças (campo de 1 bit).
    pub change_cnt_on: Bft,
    /// Verdadeiro se usa journal de declaração (campo de 1 bit).
    pub uses_stmt_journal: Bft,
    /// Verdadeiro para declarações que não escrevem (campo de 1 bit).
    pub read_only: Bft,
    /// Verdadeiro para declarações que leem (campo de 1 bit).
    pub b_is_reader: Bft,
    /// O bytecode suporta EXPLAIN QUERY PLAN (campo de 1 bit).
    pub have_eqp_ops: Bft,
    /// Máscara de bits das entradas de db->a_db[] referenciadas.
    pub btree_mask: YDbMask,
    /// Subconjunto de btree_mask que exige bloqueio.
    pub lock_mask: YDbMask,
    /// Contadores usados por stmt_status().
    pub a_counter: [u32; 9],
    /// Texto da declaração SQL que gerou esta VM.
    pub z_sql: Option<Vec<u8>>,
    // p_free (void* liberado junto com a VM) não existe: a posse é do Rust.
    /// Quadro pai.
    pub p_frame: Option<VdbeFrameRef>,
    /// Lista de objetos de quadro a liberar no reset da VM.
    pub p_del_frame: Option<VdbeFrameRef>,
    /// Número de quadros na lista p_frame.
    pub n_frame: i32,
    /// Fazer bind nestas variáveis invalida a VM.
    pub expmask: u32,
    /// Lista encadeada de todos os subprogramas usados pela VM.
    pub p_program: Option<Rc<SubProgram>>,
    /// Lista encadeada de alocações auxdata.
    pub p_aux_data: Option<Box<AuxData>>,
}

/// Valores permitidos para Vdbe.e_vdbe_state.
/// Declaração preparada em construção.
pub const VDBE_INIT_STATE: u8 = 0;
/// Pronta para executar, mas ainda não iniciada.
pub const VDBE_READY_STATE: u8 = 1;
/// Execução em andamento.
pub const VDBE_RUN_STATE: u8 = 2;
/// Terminada. Precisa de reset() ou finalize().
pub const VDBE_HALT_STATE: u8 = 3;

/// Estrutura usada para guardar o contexto exigido pelas funções da API
/// preupdate_*().
pub struct PreUpdate {
    /// A VM em execução (ponteiro de volta, fraco).
    pub v: Weak<RefCell<Vdbe>>,
    /// Cursor de onde ler os valores antigos.
    pub p_csr: Option<VdbeCursorRef>,
    /// Um de SQLITE_INSERT, UPDATE, DELETE.
    pub op: i32,
    /// Registro de banco de dados old.*.
    pub a_record: Vec<u8>,
    pub keyinfo: KeyInfo,
    /// Versão desempacotada de a_record[].
    pub p_unpacked: Option<Box<UnpackedRecord>>,
    /// Versão desempacotada do registro new.*.
    pub p_new_unpacked: Option<Box<UnpackedRecord>>,
    /// Registrador dos valores new.*.
    pub i_new_reg: i32,
    /// Valor retornado por preupdate_blobwrite().
    pub i_blob_write: i32,
    /// Primeiro valor de chave passado ao hook.
    pub i_key1: i64,
    /// Segundo valor de chave passado ao hook.
    pub i_key2: i64,
    /// Array de valores new.*.
    pub a_new: Vec<MemRef>,
    /// Objeto de esquema sendo atualizado.
    pub p_tab: Option<TableRef>,
    /// Índice PK se p_tab for WITHOUT ROWID.
    pub p_pk: Option<IndexRef>,
}

/// Uma instância deste objeto passa um vetor de valores a OP_VFilter, o método
/// xFilter de uma tabela virtual. O vetor é o conjunto de valores do lado direito
/// de uma restrição IN.
///
/// O valor passado a xFilter é um sqlite3_value do tipo "pointer", como os
/// gerados por result_pointer() e lidos por value_pointer(). Esses valores têm
/// MEM_TERM|MEM_SUBTYPE|MEM_NULL e subtipo 'p'. As interfaces vtab_in_first() e
/// _next() sabem usar este objeto para percorrer todos os valores do operando
/// direito da restrição IN.
pub struct ValueList {
    /// Uma tabela efêmera com todos os valores.
    pub p_csr: Option<BtCursorRef>,
    /// Registrador que guarda cada valor decodificado de saída.
    pub p_out: Option<MemRef>,
}

// Os protótipos de função de vdbeInt.h (vdbe_error, vdbe_exec, vdbe_mem_* etc.)
// não geram código aqui: cada um é traduzido no módulo do arquivo C que o
// define (vdbe.c, vdbeaux.c, vdbemem.c, vdbesort.c, ...), com o nome pela
// regra de nomes, e chega por `crate::prelude::*`. Também não há tradução de
// sqlite3SmallTypeSizes (tabela definida em global.c) nem de
// swapMixedEndianFloat (SQLITE_MIXED_ENDIAN_64BIT_FLOAT não está definido).


// ---- part_002.rs ----

/// Expande o blob se ele tem a flag MEM_ZERO (SQLITE_OMIT_INCRBLOB não está
/// definido no Debian 13). `vdbe_mem_expand_blob` é traduzida em vdbemem.c.
#[inline]
pub fn expand_blob(p: &mut Mem) -> i32 {
    if (p.flags & MEM_ZERO) != 0 {
        vdbe_mem_expand_blob(p)
    } else {
        0
    }
}

