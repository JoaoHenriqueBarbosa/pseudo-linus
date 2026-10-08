// Mesclado das partes traduzidas de whereInt_h (as partes quebram funções grandes no meio).
#![allow(unused_imports, ambiguous_glob_reexports)]
use crate::prelude::*;

// ---- part_000.rs ----

// Apelidos dos objetos compartilhados do planejador (ponteiros do C viram Rc/Weak).
pub type WhereClauseRef = Rc<RefCell<WhereClause>>;
pub type WhereTermRef = Rc<RefCell<WhereTerm>>;
pub type WhereLoopRef = Rc<RefCell<WhereLoop>>;
pub type WhereOrSetRef = Rc<RefCell<WhereOrSet>>;

/// Cabeçalho de um bloco de memória alocada que é liberado automaticamente
/// quando o objeto WInfo é destruído.
pub struct WhereMemBlock {
    /// Próximo bloco da corrente
    pub p_next: Option<Box<WhereMemBlock>>,
    /// Bytes de espaço
    pub sz: u64,
}

/// Informação extra anexada a um WhereLevel que é um RIGHT JOIN.
pub struct WhereRightJoin {
    /// Cursor usado para determinar linhas já casadas antes
    pub i_match: i32,
    /// Filtro de Bloom para iRJMatch
    pub reg_bloom: i32,
    /// Registro de retorno da sub-rotina interna
    pub reg_return: i32,
    /// Endereço inicial da sub-rotina interna
    pub addr_subrtn: i32,
    /// O último opcode da sub-rotina interna
    pub end_subrtn: i32,
}

/// Informação sobre cada operador IN aninhado (struct InLoop do C).
#[derive(Clone, Default)]
pub struct InLoop {
    /// Cursor VDBE usado por este operador IN
    pub i_cur: i32,
    /// Topo do loop IN
    pub addr_in_top: i32,
    /// Registro base do registro de chave múltipla do índice
    pub i_base: i32,
    /// Número de entradas anteriores na chave
    pub n_prefix: i32,
    /// Terminador do loop IN: OP_Next ou OP_Prev
    pub e_end_loop_op: u8,
}

/// União `u` de WhereLevel: depende de pWLoop->wsFlags.
pub enum WhereLevelUnion {
    /// Usada quando pWLoop->wsFlags&WHERE_IN_ABLE (campo `in` do C; `n_in` é a_in_loop.len())
    In {
        /// Número de entradas em a_in_loop[]
        n_in: i32,
        /// Informação sobre cada operador IN aninhado
        a_in_loop: Vec<InLoop>,
    },
    /// Possível índice de cobertura para WHERE_MULTI_OR
    CoveringIdx(Option<IndexRef>),
}

/// Informação necessária para implementar um único loop aninhado na cláusula WHERE.
///
/// Contraste com WhereLoop: este objeto descreve a implementação do loop, o WhereLoop
/// descreve o algoritmo. Este objeto aponta para o WhereLoop como um de seus elementos.
///
/// O WhereInfo contém uma instância deste objeto para cada termo da cláusula FROM
/// (ou seja, para cada loop aninhado como implementado). A ordem dos WhereLevel
/// determina a ordem de aninhamento: WhereInfo.a[0] é o loop externo e
/// WhereInfo.a[WhereInfo.n_level-1] é o loop interno.
pub struct WhereLevel {
    /// Célula de memória usada para implementar LEFT OUTER JOIN
    pub i_left_join: i32,
    /// Cursor VDBE usado para acessar a tabela
    pub i_tab_cur: i32,
    /// Cursor VDBE usado para acessar p_idx
    pub i_idx_cur: i32,
    /// Salta aqui para sair do loop
    pub addr_brk: i32,
    /// Salta aqui para iniciar a próxima combinação IN
    pub addr_nxt: i32,
    /// Salta aqui para a próxima iteração do skip-scan
    pub addr_skip: i32,
    /// Salta aqui para continuar com o próximo ciclo do loop
    pub addr_cont: i32,
    /// Primeira instrução do interior do loop
    pub addr_first: i32,
    /// Início do corpo deste loop
    pub addr_body: i32,
    /// Registro de flag big-null: verdadeiro se uma varredura de NULL é necessária
    pub reg_bignull: i32,
    /// Salta aqui para a próxima parte da varredura big-null
    pub addr_bignull: i32,
    /// Registro do contador de processamento de intervalo LIKE (vezes 2)
    pub i_like_rep_cntr: u32,
    /// Endereço do processamento de intervalo LIKE
    pub addr_like_rep: i32,
    /// Filtro de Bloom
    pub reg_filter: i32,
    /// Informação extra para RIGHT JOIN
    pub p_rj: Option<Box<WhereRightJoin>>,
    /// Qual entrada da cláusula FROM
    pub i_from: u8,
    /// Opcode que termina o loop
    pub op: u8,
    /// P3 do opcode que termina o loop
    pub p3: u8,
    /// P5 do opcode que termina o loop
    pub p5: u8,
    /// Operando P1 do opcode usado para terminar o loop
    pub p1: i32,
    /// Operando P2 do opcode usado para terminar o loop
    pub p2: i32,
    /// Informação que depende de p_w_loop.ws_flags
    pub u: WhereLevelUnion,
    /// O objeto WhereLoop selecionado
    pub p_w_loop: Option<WhereLoopRef>,
    /// Entradas FROM não utilizáveis neste nível
    pub not_ready: Bitmask,
}

/// Parte `btree` da união `u` de WhereLoop, e a parte `vtab`.
pub enum WhereLoopUnion {
    /// Informação para tabelas internas btree
    Btree {
        /// Número de restrições de igualdade
        n_eq: u16,
        /// Tamanho do vetor BTM
        n_btm: u16,
        /// Tamanho do vetor TOP
        n_top: u16,
        /// Colunas do índice usadas para ordenar em DISTINCT
        n_distinct_col: u16,
        /// Índice usado, ou None
        p_index: Option<IndexRef>,
    },
    /// Informação para tabelas virtuais
    Vtab {
        /// Número do índice
        idx_num: i32,
        /// Verdadeiro se sqlite3_free(idxStr) é necessário (bit de 1 bit no C)
        need_free: bool,
        /// Verdadeiro para deixar a tabela virtual tratar o OFFSET (bit de 1 bit no C)
        b_omit_offset: bool,
        /// Verdadeiro se satisfaz ORDER BY
        is_ordered: i8,
        /// Termos que podem ser omitidos
        omit_mask: u16,
        /// Identificador do índice (idxStr)
        idx_str: Vec<u8>,
        /// Termos a tratar como IN(...) em vez de ==
        m_handle_in: u32,
    },
}

/// Cada instância deste objeto representa um algoritmo para avaliar um termo de um join.
/// Todo termo da cláusula FROM tem ao menos um WhereLoop (a menos que restrições
/// INDEXED BY impeçam uma solução, o que é um erro) e muitos têm vários, cada um
/// descrevendo uma forma possível de implementar o termo, com dependências e
/// estimativas de custo.
///
/// O planejamento consiste em montar uma coleção de WhereLoop e depois computar uma
/// sequência, com um WhereLoop por termo FROM, que satisfaça as dependências e
/// minimize o custo total.
///
/// Os campos até `n_skip` são os copiados por where_loop_xfer() (WHERE_LOOP_XFER_SZ
/// é o deslocamento de n_l_slot no C); os de `n_l_slot` em diante não são copiados.
pub struct WhereLoop {
    /// Bitmask de outros loops que precisam rodar antes
    pub prereq: Bitmask,
    /// Bitmask que identifica a tabela i_tab
    pub mask_self: Bitmask,
    /// Posição na cláusula FROM da tabela deste loop
    pub i_tab: u8,
    /// Número do índice de ordenação. 0 significa nenhum
    pub i_sort_idx: u8,
    /// Custo de preparação única (ex: criar índice transitório)
    pub r_setup: LogEst,
    /// Custo de rodar cada loop
    pub r_run: LogEst,
    /// Número estimado de linhas de saída
    pub n_out: LogEst,
    /// Informação específica do tipo de loop (btree ou vtab)
    pub u: WhereLoopUnion,
    /// Flags WHERE_* que descrevem o plano
    pub ws_flags: u32,
    /// Número de entradas em a_l_term[]
    pub n_l_term: u16,
    /// Número de entradas NULL em a_l_term[]
    pub n_skip: u16,
    // **** where_loop_xfer() copia os campos acima ****
    /// Número de slots alocados para a_l_term[]
    pub n_l_slot: u16,
    /// WhereTerms usados
    pub a_l_term: Vec<Option<WhereTermRef>>,
    /// Próximo WhereLoop na WhereClause
    pub p_next_loop: Option<WhereLoopRef>,
    /// Espaço inicial para a_l_term[]
    pub a_l_term_space: [Option<WhereTermRef>; 3],
}

/// Pré-requisitos e custo de rodar uma subconsulta em um operando de um OR
/// na cláusula WHERE. Ver WhereOrSet.
#[derive(Clone, Copy, Default)]
pub struct WhereOrCost {
    /// Pré-requisitos
    pub prereq: Bitmask,
    /// Custo de rodar esta subconsulta
    pub r_run: LogEst,
    /// Número de saídas desta subconsulta
    pub n_out: LogEst,
}

/// O WhereOrSet guarda um conjunto de possíveis WhereOrCost que correspondem às
/// subconsultas do processamento de cláusula OR. Só os N_OR_COST melhores são retidos.
pub const N_OR_COST: usize = 3;

#[derive(Clone, Copy, Default)]
pub struct WhereOrSet {
    /// Número de entradas válidas em a[]
    pub n: u16,
    /// Conjunto dos melhores custos
    pub a: [WhereOrCost; N_OR_COST],
}

/// Cada instância deste objeto guarda uma sequência de WhereLoop que implementam
/// parte ou todo um plano de consulta.
///
/// Cada WhereLoop é um nó de um grafo, com arcos que mostram dependências e custos
/// de viagem entre nós. Um WherePath é um caminho pelo grafo que visita alguns ou
/// todos os WhereLoop uma vez.
///
/// O "solver" cria os N melhores WherePath de comprimento 1, usa-os de base para os
/// N melhores de comprimento 2, e assim por diante até o comprimento igualar o número
/// de nós da cláusula FROM. O de menor custo no final é o plano escolhido.
#[derive(Clone, Default)]
pub struct WherePath {
    /// Bitmask de todos os WhereLoop deste caminho
    pub mask_loop: Bitmask,
    /// aLoop[]s que devem ser invertidos para ORDER BY
    pub rev_loop: Bitmask,
    /// Número estimado de linhas geradas por este caminho
    pub n_row: LogEst,
    /// Custo total deste caminho
    pub r_cost: LogEst,
    /// Custo total deste caminho ignorando os custos de ordenação
    pub r_unsorted: LogEst,
    /// Número de termos ORDER BY satisfeitos. -1 para desconhecido
    pub is_ordered: i8,
    /// WhereLoop que implementam este caminho
    pub a_loop: Vec<WhereLoopRef>,
}

/// União `u` de WhereTerm.
pub enum WhereTermUnion {
    /// Operador diferente de OP_OR e OP_AND (campo `x` do C)
    X {
        /// Número da coluna de X em "X <op> <expr>"
        left_column: i32,
        /// Campo em (?,?,?) IN (SELECT...) vetorial
        i_field: i32,
    },
    /// Informação extra se (e_operator & WO_OR)!=0
    OrInfo(Option<Box<WhereOrInfo>>),
    /// Informação extra se (e_operator & WO_AND)!=0
    AndInfo(Option<Box<WhereAndInfo>>),
}

/// O gerador de consultas usa um array destas estruturas para analisar as
/// subexpressões da cláusula WHERE. Cada subexpressão é separada das outras por AND,
/// normalmente, ou às vezes por OR.
///
/// Todos os WhereTerm ficam numa única WhereClause. Vale a identidade
/// WhereTerm.p_wc.a[WhereTerm.idx] == WhereTerm.
///
/// Quando o termo tem a forma `X <op> <expr>`, com X nome de coluna e <op> um certo
/// operador, WhereTerm.left_cursor e WhereTerm.u.left_column guardam o cursor e a
/// coluna de X, e e_operator guarda <op> como bitmask WO_xxx, o que permite achar
/// rápido termos de vários operadores.
///
/// Um WhereTerm também pode ser dois ou mais subtermos ligados por OR. Nesse caso
/// wt_flags tem o bit TERM_ORINFO, e_operator==WO_OR e u.p_or_info aponta para a
/// informação auxiliar coletada da cláusula OR.
///
/// Se o termo não cai em nenhuma das duas categorias, e_operator==0: p_expr e wt_flags
/// continuam valendo, mas nenhum outro campo é significativo.
///
/// Quando e_operator!=0, prereq_right e prereq_all guardam conjuntos de cursores
/// indiretamente: um WhereMaskSet traduz os números de cursor (esparsos) em bits
/// consecutivos a partir de 0, para aproveitar ao máximo os bits do Bitmask. O limite
/// padrão é de 64 bits, logo só se processam joins de até 64 tabelas.
pub struct WhereTerm {
    /// Subexpressão que é este termo
    pub p_expr: Option<ExprRef>,
    /// A cláusula da qual este termo faz parte
    pub p_wc: Weak<RefCell<WhereClause>>,
    /// Probabilidade de verdade desta expressão
    pub truth_prob: LogEst,
    /// Flags TERM_xxx. Ver abaixo
    pub wt_flags: u16,
    /// Valor WO_xx que descreve <op>
    pub e_operator: u16,
    /// Número de filhos que devem nos desabilitar
    pub n_child: u8,
    /// Operador para termos MATCH/LIKE/GLOB/REGEXP de vtab
    pub e_match_op: u8,
    /// Desabilitar p_wc.a[i_parent] quando este termo for desabilitado
    pub i_parent: i32,
    /// Número do cursor de X em "X <op> <expr>"
    pub left_cursor: i32,
    /// Informação que depende do operador
    pub u: WhereTermUnion,
    /// Bitmask das tabelas usadas por p_expr.p_right
    pub prereq_right: Bitmask,
    /// Bitmask das tabelas referenciadas por p_expr
    pub prereq_all: Bitmask,
}

// Valores permitidos de WhereTerm.wt_flags
/// Precisa chamar expr_delete(db, p_expr)
pub const TERM_DYNAMIC: u16 = 0x0001;
/// Adicionado pelo otimizador. Não codificar
pub const TERM_VIRTUAL: u16 = 0x0002;
/// Este termo já está codificado
pub const TERM_CODED: u16 = 0x0004;
/// Tem um filho
pub const TERM_COPIED: u16 = 0x0008;
/// Precisa liberar o objeto WhereTerm.u.p_or_info
pub const TERM_ORINFO: u16 = 0x0010;
/// Precisa liberar o objeto WhereTerm.u.p_and_info
pub const TERM_ANDINFO: u16 = 0x0020;
/// Usado durante o processamento de cláusula OR
pub const TERM_OK: u16 = 0x0040;
/// Termo x>NULL ou x<=NULL fabricado
pub const TERM_VNULL: u16 = 0x0080;
/// Termos virtuais da otimização LIKE
pub const TERM_LIKEOPT: u16 = 0x0100;
/// Este operador LIKE vale condicionalmente
pub const TERM_LIKECOND: u16 = 0x0200;
/// O operador LIKE original
pub const TERM_LIKE: u16 = 0x0400;
/// Term.p_expr é um operador IS
pub const TERM_IS: u16 = 0x0800;
/// Term.p_expr contém uma subconsulta correlacionada
pub const TERM_VARSELECT: u16 = 0x1000;
/// Probabilidade de verdade heurística usada
pub const TERM_HEURTRUTH: u16 = 0x2000;
/// Só usado com STAT4 (desligado no Debian 13)
pub const TERM_HIGHTRUTH: u16 = 0;
/// Uma fatia de uma comparação row-value/vetorial
pub const TERM_SLICE: u16 = 0x8000;

/// Iterador usado para localizar termos da cláusula WHERE úteis ao planejador.
pub struct WhereScan {
    /// WhereClause original, a mais interna
    pub p_orig_wc: Option<WhereClauseRef>,
    /// WhereClause varrida no momento
    pub p_wc: Option<WhereClauseRef>,
    /// Sequência de colação exigida, se houver
    pub z_coll_name: Option<Vec<u8>>,
    /// Procurar por esta expressão de índice
    pub p_idx_expr: Option<ExprRef>,
    /// Retomar a varredura em this.p_wc.a[this.k]
    pub k: i32,
    /// Operadores aceitáveis
    pub op_mask: u32,
    /// Deve casar com esta afinidade, se z_coll_name for Some
    pub idxaff: u8,
    /// Slot atual em ai_cur[] e ai_column[]
    pub i_equiv: u8,
    /// Número de entradas em ai_cur[] e ai_column[]
    pub n_equiv: u8,
    /// Cursores da classe de equivalência
    pub ai_cur: [i32; 11],
    /// Número de coluna correspondente na classe de equivalência
    pub ai_column: [i16; 11],
}

/// Guarda toda a informação sobre uma cláusula WHERE. Em geral é um contêiner de
/// WhereTerm.
///
/// Sobre p_outer: numa cláusula `a AND ((b AND c) OR (d AND e)) AND f` há objetos
/// WhereClause separados para a cláusula inteira e para as subcláusulas "(b AND c)"
/// e "(d AND e)"; o p_outer das subcláusulas aponta para a cláusula inteira.
///
/// O espaço estático `aStatic[8]` do C não existe: `a` é um Vec e `n_slot` guarda a
/// capacidade lógica como no C.
pub struct WhereClause {
    /// Contexto de processamento da cláusula WHERE
    pub p_w_info: Weak<RefCell<WhereInfo>>,
    /// Conjunção externa
    pub p_outer: Option<Weak<RefCell<WhereClause>>>,
    /// Operador de divisão. TK_AND ou TK_OR
    pub op: u8,
    /// Verdadeiro se algum a[].e_operator é WO_OR
    pub has_or: u8,
    /// Número de termos
    pub n_term: i32,
    /// Número de entradas em a[]
    pub n_slot: i32,
    /// Número de termos até o último não virtual
    pub n_base: i32,
    /// Cada a[] descreve um termo da cláusula WHERE
    pub a: Vec<WhereTermRef>,
}


// ---- part_001.rs ----

pub type WhereInfoRef = Rc<RefCell<WhereInfo>>;

/// Um WhereTerm com e_operator==WO_OR tem seu u.p_or_info apontando para uma
/// instância alocada desta estrutura.
pub struct WhereOrInfo {
    /// Decomposição em subtermos
    pub wc: WhereClause,
    /// Bitmask de todas as tabelas indexáveis na cláusula
    pub indexable: Bitmask,
}

/// Um WhereTerm com e_operator==WO_AND tem seu u.p_and_info apontando para uma
/// instância alocada desta estrutura.
pub struct WhereAndInfo {
    /// A subexpressão decomposta
    pub wc: WhereClause,
}

/// Mapeamento entre números de cursor VDBE e bits dos bitmasks em WhereTerm.
///
/// Os números de cursor VDBE são inteiros pequenos guardados em SrcItem.i_cursor e
/// Expr.i_table. Numa cláusula WHERE eles podem não começar em 0 e ter lacunas, mas
/// queremos usar ao máximo os bits dos bitmasks. Esta estrutura mapeia os cursores
/// esparsos em inteiros consecutivos a partir de 0.
///
/// Se WhereMaskSet.ix[A]==B, o A-ésimo bit do Bitmask corresponde ao cursor VDBE B.
/// O A-ésimo bit é 1<<A. O mapeamento não é necessariamente ordenado: o que importa
/// é que os cursores esparsos virem bits consecutivos a partir de 0, sem lacunas.
pub struct WhereMaskSet {
    /// Usado por where_expr_usage()
    pub b_var_select: i32,
    /// Número de valores de cursor atribuídos
    pub n: i32,
    /// Cursor atribuído a cada bit
    pub ix: [i32; BMS],
}

/// Invólucro de conveniência com toda a informação necessária para construir
/// objetos WhereLoop de uma consulta particular.
pub struct WhereLoopBuilder {
    /// Informação sobre este WHERE
    pub p_w_info: WhereInfoRef,
    /// Termos da cláusula WHERE
    pub p_wc: WhereClauseRef,
    /// WhereLoop modelo
    pub p_new: WhereLoopRef,
    /// Registra aqui os melhores loops, se não for None
    pub p_or_set: Option<WhereOrSetRef>,
    /// Primeiro conjunto de flags SQLITE_BLDF*
    pub bld_flags1: u8,
    /// Segundo conjunto de flags SQLITE_BLDF*
    pub bld_flags2: u8,
    /// Limitador de busca
    pub i_plan_limit: u32,
}

// Valores permitidos de WhereLoopBuilder.bld_flags
/// Um índice é usado
pub const SQLITE_BLDF1_INDEXED: u8 = 0x0001;
/// Todas as chaves de um índice UNIQUE usadas
pub const SQLITE_BLDF1_UNIQUE: u8 = 0x0002;
/// Segunda passada do construtor necessária
pub const SQLITE_BLDF2_2NDPASS: u8 = 0x0004;

// WhereLoopBuilder.i_plan_limit limita o número de combinações índice+restrição que
// o planejador considera numa consulta. SQLITE_QUERY_PLANNER_LIMIT é o limite base,
// aumentado de SQLITE_QUERY_PLANNER_LIMIT_INCR antes de cada termo da cláusula FROM,
// para que toda tabela do join possa propor algumas combinações mesmo que o limite
// base tenha sido esgotado pelas tabelas anteriores.
pub const SQLITE_QUERY_PLANNER_LIMIT: u32 = 20000;
pub const SQLITE_QUERY_PLANNER_LIMIT_INCR: u32 = 1000;

/// A rotina de processamento da cláusula WHERE tem duas metades: a primeira inicia o
/// loop WHERE e a segunda faz o final. Uma instância desta estrutura é devolvida pela
/// primeira e passada à segunda para dar continuidade. Guarda o estado completo do
/// planejador de consultas.
pub struct WhereInfo {
    /// Contexto de análise e geração de código
    pub p_parse: ParseRef,
    /// Lista de tabelas do join
    pub p_tab_list: SrcListRef,
    /// Cláusula ORDER BY ou None
    pub p_order_by: Option<ExprListRef>,
    /// Conjunto de resultados da consulta
    pub p_result_set: Option<ExprListRef>,
    /// SELECT inteiro que contém o WHERE
    pub p_select: Option<SelectRef>,
    /// Cursores OP_OpenWrite da otimização ONEPASS
    pub ai_cur_one_pass: [i32; 2],
    /// Salta aqui para continuar com o próximo registro
    pub i_continue: i32,
    /// Salta aqui para sair do loop
    pub i_break: i32,
    /// p_parse.n_query_loop fora do loop WHERE
    pub saved_n_query_loop: i32,
    /// Flags passadas originalmente a where_begin()
    pub wctrl_flags: u16,
    /// LIMIT se wctrl_flags tem WHERE_USE_LIMIT
    pub i_limit: LogEst,
    /// Número de loops aninhados
    pub n_level: u8,
    /// Número de termos ORDER BY satisfeitos por índices
    pub n_ob_sat: i8,
    /// ONEPASS_OFF, ou _SINGLE, ou _MULTI
    pub e_one_pass: u8,
    /// Um dos valores WHERE_DISTINCT_*
    pub e_distinct: u8,
    /// Usa OP_DeferredSeek
    pub b_deferred_seek: bool,
    /// Nem todos os termos WHERE foram resolvidos pelo loop externo
    pub untested_terms: bool,
    /// Verdadeiro se só o loop mais interno está ordenado
    pub b_ordered_inner_loop: bool,
    /// Verdadeiro se realmente ordenado (não só agrupado)
    pub sorted: bool,
    /// Número estimado de linhas de saída
    pub n_row_out: LogEst,
    /// O começo de todo o loop WHERE
    pub i_top: i32,
    /// Fim da própria cláusula WHERE
    pub i_end_where: i32,
    /// Lista de todos os objetos WhereLoop
    pub p_loops: Option<WhereLoopRef>,
    /// Memória a liberar quando este objeto for destruído
    pub p_mem_to_free: Option<Box<WhereMemBlock>>,
    /// Máscara dos termos ORDER BY que precisam ser invertidos
    pub rev_mask: Bitmask,
    /// Decomposição da cláusula WHERE
    pub s_wc: WhereClause,
    /// Mapa de números de cursor para bitmasks
    pub s_mask_set: WhereMaskSet,
    /// Informação sobre cada loop aninhado (array flexível a[1] do C; tem n_level itens)
    pub a: Vec<WhereLevel>,
}

// Bitmasks dos operadores em objetos WhereTerm: todos os operadores de interesse do
// planejador. Uma combinação OR destes valores serve para procurar WhereTerm numa
// WhereClause.
//
// Restrições de valor:
//     WO_EQ == SQLITE_INDEX_CONSTRAINT_EQ, e o mesmo para LT, LE, GT e GE.
pub const WO_IN: u16 = 0x0001;
pub const WO_EQ: u16 = 0x0002;
pub const WO_LT: u16 = WO_EQ << ((TK_LT - TK_EQ) as u32);
pub const WO_LE: u16 = WO_EQ << ((TK_LE - TK_EQ) as u32);
pub const WO_GT: u16 = WO_EQ << ((TK_GT - TK_EQ) as u32);
pub const WO_GE: u16 = WO_EQ << ((TK_GE - TK_EQ) as u32);
/// Operador útil só para tabelas virtuais
pub const WO_AUX: u16 = 0x0040;
pub const WO_IS: u16 = 0x0080;
pub const WO_ISNULL: u16 = 0x0100;
/// Dois ou mais termos ligados por OR
pub const WO_OR: u16 = 0x0200;
/// Dois ou mais termos ligados por AND
pub const WO_AND: u16 = 0x0400;
/// Da forma A==B, ambos colunas
pub const WO_EQUIV: u16 = 0x0800;
/// Este termo não restringe o espaço de busca
pub const WO_NOOP: u16 = 0x1000;
/// Um termo row-value
pub const WO_ROWVAL: u16 = 0x2000;

/// Máscara de todos os valores WO_* possíveis
pub const WO_ALL: u16 = 0x3fff;
/// Máscara de todos os valores WO_* não compostos
pub const WO_SINGLE: u16 = 0x01ff;

// Bits do campo WhereLoop.ws_flags. A combinação de bits de cada WhereLoop ajuda a
// determinar o algoritmo que ele representa.
/// x=EXPR
pub const WHERE_COLUMN_EQ: u32 = 0x00000001;
/// x<EXPR e/ou x>EXPR
pub const WHERE_COLUMN_RANGE: u32 = 0x00000002;
/// x IN (...)
pub const WHERE_COLUMN_IN: u32 = 0x00000004;
/// x IS NULL
pub const WHERE_COLUMN_NULL: u32 = 0x00000008;
/// Qualquer dos valores WHERE_COLUMN_xxx
pub const WHERE_CONSTRAINT: u32 = 0x0000000f;
/// Restrição x<EXPR ou x<=EXPR
pub const WHERE_TOP_LIMIT: u32 = 0x00000010;
/// Restrição x>EXPR ou x>=EXPR
pub const WHERE_BTM_LIMIT: u32 = 0x00000020;
/// Ambos x>EXPR e x<EXPR
pub const WHERE_BOTH_LIMIT: u32 = 0x00000030;
/// Usa só o índice, omite a tabela
pub const WHERE_IDX_ONLY: u32 = 0x00000040;
/// x é o INTEGER PRIMARY KEY
pub const WHERE_IPK: u32 = 0x00000100;
/// WhereLoop.u.btree.p_index é válido
pub const WHERE_INDEXED: u32 = 0x00000200;
/// WhereLoop.u.vtab é válido
pub const WHERE_VIRTUALTABLE: u32 = 0x00000400;
/// Capaz de suportar um operador IN
pub const WHERE_IN_ABLE: u32 = 0x00000800;
/// Seleciona no máximo uma linha
pub const WHERE_ONEROW: u32 = 0x00001000;
/// OR com vários índices
pub const WHERE_MULTI_OR: u32 = 0x00002000;
/// Usa um índice efêmero
pub const WHERE_AUTO_INDEX: u32 = 0x00004000;
/// Usa o algoritmo skip-scan
pub const WHERE_SKIPSCAN: u32 = 0x00008000;
/// WHERE_ONEROW teria ajudado
pub const WHERE_UNQ_WANTED: u32 = 0x00010000;
/// O índice automático é parcial
pub const WHERE_PARTIALIDX: u32 = 0x00020000;
/// Talvez sair cedo dos loops IN
pub const WHERE_IN_EARLYOUT: u32 = 0x00040000;
/// A coluna n_eq do índice é BIGNULL
pub const WHERE_BIGNULL_SORT: u32 = 0x00080000;
/// Otimização seek-scan para IN
pub const WHERE_IN_SEEKSCAN: u32 = 0x00100000;
/// Usa uma restrição transitiva
pub const WHERE_TRANSCONS: u32 = 0x00200000;
/// Considerar o uso de um filtro de Bloom
pub const WHERE_BLOOMFILTER: u32 = 0x00400000;
/// n_out reduzido por termos WHERE extras
pub const WHERE_SELFCULL: u32 = 0x00800000;
/// Zera o contador de offset
pub const WHERE_OMIT_OFFSET: u32 = 0x01000000;
// 0x02000000 disponível para reuso
/// Usa um índice sobre expressões
pub const WHERE_EXPRIDX: u32 = 0x04000000;

