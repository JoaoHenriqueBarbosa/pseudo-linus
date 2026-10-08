#endif /* YYFALLBACK */

/* The following structure represents a single element of the
** parser's stack.  Information stored includes:
**
**   +  The state number for the parser at this level of the stack.
**
**   +  The value of the token stored at this level of the stack.
**      (In other words, the "major" token.)
**
**   +  The semantic value stored at this level of the stack.  This is
**      the information used by the action routines in the grammar.
**      It is sometimes called the "minor" token.
**
** After the "shift" half of a SHIFTREDUCE action, the stateno field
** actually contains the reduce action for the second half of the
** SHIFTREDUCE.
*/
struct yyStackEntry {
  YYACTIONTYPE stateno;  /* The state-number, or reduce action in SHIFTREDUCE */
  YYCODETYPE major;      /* The major token value.  This is the code
                         ** number for the token at this stack level */
  YYMINORTYPE minor;     /* The user-supplied minor token value.  This
                         ** is the value of the token  */
};
typedef struct yyStackEntry yyStackEntry;

/* The state of the parser is completely contained in an instance of
** the following structure */
struct yyParser {
  yyStackEntry *yytos;          /* Pointer to top element of the stack */
#ifdef YYTRACKMAXSTACKDEPTH
  int yyhwm;                    /* High-water mark of the stack */
#endif
#ifndef YYNOERRORRECOVERY
  int yyerrcnt;                 /* Shifts left before out of the error */
#endif
  sqlite3ParserARG_SDECL                /* A place to hold %extra_argument */
  sqlite3ParserCTX_SDECL                /* A place to hold %extra_context */
  yyStackEntry *yystackEnd;           /* Last entry in the stack */
  yyStackEntry *yystack;              /* The parser stack */
  yyStackEntry yystk0[YYSTACKDEPTH];  /* Initial stack space */
};
typedef struct yyParser yyParser;

/* #include <assert.h> */
#ifndef NDEBUG
/* #include <stdio.h> */
static FILE *yyTraceFILE = 0;
static char *yyTracePrompt = 0;
#endif /* NDEBUG */

#ifndef NDEBUG
/*
** Turn parser tracing on by giving a stream to which to write the trace
** and a prompt to preface each trace message.  Tracing is turned off
** by making either argument NULL
**
** Inputs:
** <ul>
** <li> A FILE* to which trace output should be written.
**      If NULL, then tracing is turned off.
** <li> A prefix string written at the beginning of every
**      line of trace output.  If NULL, then tracing is
**      turned off.
** </ul>
**
** Outputs:
** None.
*/
SQLITE_PRIVATE void sqlite3ParserTrace(FILE *TraceFILE, char *zTracePrompt){
  yyTraceFILE = TraceFILE;
  yyTracePrompt = zTracePrompt;
  if( yyTraceFILE==0 ) yyTracePrompt = 0;
  else if( yyTracePrompt==0 ) yyTraceFILE = 0;
}
#endif /* NDEBUG */

#if defined(YYCOVERAGE) || !defined(NDEBUG)
/* For tracing shifts, the names of all terminals and nonterminals
** are required.  The following table supplies these names */
static const char *const yyTokenName[] = {
  /*    0 */ "$",
  /*    1 */ "SEMI",
  /*    2 */ "EXPLAIN",
  /*    3 */ "QUERY",
  /*    4 */ "PLAN",
  /*    5 */ "BEGIN",
  /*    6 */ "TRANSACTION",
  /*    7 */ "DEFERRED",
  /*    8 */ "IMMEDIATE",
  /*    9 */ "EXCLUSIVE",
  /*   10 */ "COMMIT",
  /*   11 */ "END",
  /*   12 */ "ROLLBACK",
  /*   13 */ "SAVEPOINT",
  /*   14 */ "RELEASE",
  /*   15 */ "TO",
  /*   16 */ "TABLE",
  /*   17 */ "CREATE",
  /*   18 */ "IF",
  /*   19 */ "NOT",
  /*   20 */ "EXISTS",
  /*   21 */ "TEMP",
  /*   22 */ "LP",
  /*   23 */ "RP",
  /*   24 */ "AS",
  /*   25 */ "COMMA",
  /*   26 */ "WITHOUT",
  /*   27 */ "ABORT",
  /*   28 */ "ACTION",
  /*   29 */ "AFTER",
  /*   30 */ "ANALYZE",
  /*   31 */ "ASC",
  /*   32 */ "ATTACH",
  /*   33 */ "BEFORE",
  /*   34 */ "BY",
  /*   35 */ "CASCADE",
  /*   36 */ "CAST",
  /*   37 */ "CONFLICT",
  /*   38 */ "DATABASE",
  /*   39 */ "DESC",
  /*   40 */ "DETACH",
  /*   41 */ "EACH",
  /*   42 */ "FAIL",
  /*   43 */ "OR",
  /*   44 */ "AND",
  /*   45 */ "IS",
  /*   46 */ "MATCH",
  /*   47 */ "LIKE_KW",
  /*   48 */ "BETWEEN",
  /*   49 */ "IN",
  /*   50 */ "ISNULL",
  /*   51 */ "NOTNULL",
  /*   52 */ "NE",
  /*   53 */ "EQ",
  /*   54 */ "GT",
  /*   55 */ "LE",
  /*   56 */ "LT",
  /*   57 */ "GE",
  /*   58 */ "ESCAPE",
  /*   59 */ "ID",
  /*   60 */ "COLUMNKW",
  /*   61 */ "DO",
  /*   62 */ "FOR",
  /*   63 */ "IGNORE",
  /*   64 */ "INITIALLY",
  /*   65 */ "INSTEAD",
  /*   66 */ "NO",
  /*   67 */ "KEY",
  /*   68 */ "OF",
  /*   69 */ "OFFSET",
  /*   70 */ "PRAGMA",
  /*   71 */ "RAISE",
  /*   72 */ "RECURSIVE",
  /*   73 */ "REPLACE",
  /*   74 */ "RESTRICT",
  /*   75 */ "ROW",
  /*   76 */ "ROWS",
  /*   77 */ "TRIGGER",
  /*   78 */ "VACUUM",
  /*   79 */ "VIEW",
  /*   80 */ "VIRTUAL",
  /*   81 */ "WITH",
  /*   82 */ "NULLS",
  /*   83 */ "FIRST",
  /*   84 */ "LAST",
  /*   85 */ "CURRENT",
  /*   86 */ "FOLLOWING",
  /*   87 */ "PARTITION",
  /*   88 */ "PRECEDING",
  /*   89 */ "RANGE",
  /*   90 */ "UNBOUNDED",
  /*   91 */ "EXCLUDE",
  /*   92 */ "GROUPS",
  /*   93 */ "OTHERS",
  /*   94 */ "TIES",
  /*   95 */ "GENERATED",
  /*   96 */ "ALWAYS",
  /*   97 */ "MATERIALIZED",
  /*   98 */ "REINDEX",
  /*   99 */ "RENAME",
  /*  100 */ "CTIME_KW",
  /*  101 */ "ANY",
  /*  102 */ "BITAND",
  /*  103 */ "BITOR",
  /*  104 */ "LSHIFT",
  /*  105 */ "RSHIFT",
  /*  106 */ "PLUS",
  /*  107 */ "MINUS",
  /*  108 */ "STAR",
  /*  109 */ "SLASH",
  /*  110 */ "REM",
  /*  111 */ "CONCAT",
  /*  112 */ "PTR",
  /*  113 */ "COLLATE",
  /*  114 */ "BITNOT",
  /*  115 */ "ON",
  /*  116 */ "INDEXED",
  /*  117 */ "STRING",
  /*  118 */ "JOIN_KW",
  /*  119 */ "CONSTRAINT",
  /*  120 */ "DEFAULT",
  /*  121 */ "NULL",
  /*  122 */ "PRIMARY",
  /*  123 */ "UNIQUE",
  /*  124 */ "CHECK",
  /*  125 */ "REFERENCES",
  /*  126 */ "AUTOINCR",
  /*  127 */ "INSERT",
  /*  128 */ "DELETE",
  /*  129 */ "UPDATE",
  /*  130 */ "SET",
  /*  131 */ "DEFERRABLE",
  /*  132 */ "FOREIGN",
  /*  133 */ "DROP",
  /*  134 */ "UNION",
  /*  135 */ "ALL",
  /*  136 */ "EXCEPT",
  /*  137 */ "INTERSECT",
  /*  138 */ "SELECT",
  /*  139 */ "VALUES",
  /*  140 */ "DISTINCT",
  /*  141 */ "DOT",
  /*  142 */ "FROM",
  /*  143 */ "JOIN",
  /*  144 */ "USING",
  /*  145 */ "ORDER",
  /*  146 */ "GROUP",
  /*  147 */ "HAVING",
  /*  148 */ "LIMIT",
  /*  149 */ "WHERE",
  /*  150 */ "RETURNING",
  /*  151 */ "INTO",
  /*  152 */ "NOTHING",
  /*  153 */ "FLOAT",
  /*  154 */ "BLOB",
  /*  155 */ "INTEGER",
  /*  156 */ "VARIABLE",
  /*  157 */ "CASE",
  /*  158 */ "WHEN",
  /*  159 */ "THEN",
  /*  160 */ "ELSE",
  /*  161 */ "INDEX",
  /*  162 */ "ALTER",
  /*  163 */ "ADD",
  /*  164 */ "WINDOW",
  /*  165 */ "OVER",
  /*  166 */ "FILTER",
  /*  167 */ "COLUMN",
  /*  168 */ "AGG_FUNCTION",
  /*  169 */ "AGG_COLUMN",
  /*  170 */ "TRUEFALSE",
  /*  171 */ "ISNOT",
  /*  172 */ "FUNCTION",
  /*  173 */ "UPLUS",
  /*  174 */ "UMINUS",
  /*  175 */ "TRUTH",
  /*  176 */ "REGISTER",
  /*  177 */ "VECTOR",
  /*  178 */ "SELECT_COLUMN",
  /*  179 */ "IF_NULL_ROW",
  /*  180 */ "ASTERISK",
  /*  181 */ "SPAN",
  /*  182 */ "ERROR",
  /*  183 */ "QNUMBER",
  /*  184 */ "SPACE",
  /*  185 */ "ILLEGAL",
  /*  186 */ "input",
  /*  187 */ "cmdlist",
  /*  188 */ "ecmd",
  /*  189 */ "cmdx",
  /*  190 */ "explain",
  /*  191 */ "cmd",
  /*  192 */ "transtype",
  /*  193 */ "trans_opt",
  /*  194 */ "nm",
  /*  195 */ "savepoint_opt",
  /*  196 */ "create_table",
  /*  197 */ "create_table_args",
  /*  198 */ "createkw",
  /*  199 */ "temp",
  /*  200 */ "ifnotexists",
  /*  201 */ "dbnm",
  /*  202 */ "columnlist",
  /*  203 */ "conslist_opt",
  /*  204 */ "table_option_set",
  /*  205 */ "select",
  /*  206 */ "table_option",
  /*  207 */ "columnname",
  /*  208 */ "carglist",
  /*  209 */ "typetoken",
  /*  210 */ "typename",
  /*  211 */ "signed",
  /*  212 */ "plus_num",
  /*  213 */ "minus_num",
  /*  214 */ "scanpt",
  /*  215 */ "scantok",
  /*  216 */ "ccons",
  /*  217 */ "term",
  /*  218 */ "expr",
  /*  219 */ "onconf",
  /*  220 */ "sortorder",
  /*  221 */ "autoinc",
  /*  222 */ "eidlist_opt",
  /*  223 */ "refargs",
  /*  224 */ "defer_subclause",
  /*  225 */ "generated",
  /*  226 */ "refarg",
  /*  227 */ "refact",
  /*  228 */ "init_deferred_pred_opt",
  /*  229 */ "conslist",
  /*  230 */ "tconscomma",
  /*  231 */ "tcons",
  /*  232 */ "sortlist",
  /*  233 */ "eidlist",
  /*  234 */ "defer_subclause_opt",
  /*  235 */ "orconf",
  /*  236 */ "resolvetype",
  /*  237 */ "raisetype",
  /*  238 */ "ifexists",
  /*  239 */ "fullname",
  /*  240 */ "selectnowith",
  /*  241 */ "oneselect",
  /*  242 */ "wqlist",
  /*  243 */ "multiselect_op",
  /*  244 */ "distinct",
  /*  245 */ "selcollist",
  /*  246 */ "from",
  /*  247 */ "where_opt",
  /*  248 */ "groupby_opt",
  /*  249 */ "having_opt",
  /*  250 */ "orderby_opt",
  /*  251 */ "limit_opt",
  /*  252 */ "window_clause",
  /*  253 */ "values",
  /*  254 */ "nexprlist",
  /*  255 */ "mvalues",
  /*  256 */ "sclp",
  /*  257 */ "as",
  /*  258 */ "seltablist",
  /*  259 */ "stl_prefix",
  /*  260 */ "joinop",
  /*  261 */ "on_using",
  /*  262 */ "indexed_by",
  /*  263 */ "exprlist",
  /*  264 */ "xfullname",
  /*  265 */ "idlist",
  /*  266 */ "indexed_opt",
  /*  267 */ "nulls",
  /*  268 */ "with",
  /*  269 */ "where_opt_ret",
  /*  270 */ "setlist",
  /*  271 */ "insert_cmd",
  /*  272 */ "idlist_opt",
  /*  273 */ "upsert",
  /*  274 */ "returning",
  /*  275 */ "filter_over",
  /*  276 */ "likeop",
  /*  277 */ "between_op",
  /*  278 */ "in_op",
  /*  279 */ "paren_exprlist",
  /*  280 */ "case_operand",
  /*  281 */ "case_exprlist",
  /*  282 */ "case_else",
  /*  283 */ "uniqueflag",
  /*  284 */ "collate",
  /*  285 */ "vinto",
  /*  286 */ "nmnum",
  /*  287 */ "trigger_decl",
  /*  288 */ "trigger_cmd_list",
  /*  289 */ "trigger_time",
  /*  290 */ "trigger_event",
  /*  291 */ "foreach_clause",
  /*  292 */ "when_clause",
  /*  293 */ "trigger_cmd",
  /*  294 */ "trnm",
  /*  295 */ "tridxby",
  /*  296 */ "database_kw_opt",
  /*  297 */ "key_opt",
  /*  298 */ "add_column_fullname",
  /*  299 */ "kwcolumn_opt",
  /*  300 */ "create_vtab",
  /*  301 */ "vtabarglist",
  /*  302 */ "vtabarg",
  /*  303 */ "vtabargtoken",
  /*  304 */ "lp",
  /*  305 */ "anylist",
  /*  306 */ "wqitem",
  /*  307 */ "wqas",
  /*  308 */ "withnm",
  /*  309 */ "windowdefn_list",
  /*  310 */ "windowdefn",
  /*  311 */ "window",
  /*  312 */ "frame_opt",
  /*  313 */ "part_opt",
  /*  314 */ "filter_clause",
  /*  315 */ "over_clause",
  /*  316 */ "range_or_rows",
  /*  317 */ "frame_bound",
  /*  318 */ "frame_bound_s",
  /*  319 */ "frame_bound_e",
  /*  320 */ "frame_exclude_opt",
  /*  321 */ "frame_exclude",
};
