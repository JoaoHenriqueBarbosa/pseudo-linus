#ifndef fts5YYSTACKDEPTH
#define fts5YYSTACKDEPTH 100
#endif
#define sqlite3Fts5ParserARG_SDECL Fts5Parse *pParse;
#define sqlite3Fts5ParserARG_PDECL ,Fts5Parse *pParse
#define sqlite3Fts5ParserARG_PARAM ,pParse
#define sqlite3Fts5ParserARG_FETCH Fts5Parse *pParse=fts5yypParser->pParse;
#define sqlite3Fts5ParserARG_STORE fts5yypParser->pParse=pParse;
#define fts5YYREALLOC realloc
#define fts5YYFREE free
#define fts5YYDYNSTACK 0
#define sqlite3Fts5ParserCTX_SDECL
#define sqlite3Fts5ParserCTX_PDECL
#define sqlite3Fts5ParserCTX_PARAM
#define sqlite3Fts5ParserCTX_FETCH
#define sqlite3Fts5ParserCTX_STORE
#define fts5YYNSTATE             35
#define fts5YYNRULE              28
#define fts5YYNRULE_WITH_ACTION  28
#define fts5YYNFTS5TOKEN             16
#define fts5YY_MAX_SHIFT         34
#define fts5YY_MIN_SHIFTREDUCE   52
#define fts5YY_MAX_SHIFTREDUCE   79
#define fts5YY_ERROR_ACTION      80
#define fts5YY_ACCEPT_ACTION     81
#define fts5YY_NO_ACTION         82
#define fts5YY_MIN_REDUCE        83
#define fts5YY_MAX_REDUCE        110
#define fts5YY_MIN_DSTRCTR       16
#define fts5YY_MAX_DSTRCTR       24
/************* End control #defines *******************************************/
#define fts5YY_NLOOKAHEAD ((int)(sizeof(fts5yy_lookahead)/sizeof(fts5yy_lookahead[0])))

/* Define the fts5yytestcase() macro to be a no-op if is not already defined
** otherwise.
**
** Applications can choose to define fts5yytestcase() in the %include section
** to a macro that can assist in verifying code coverage.  For production
** code the fts5yytestcase() macro should be turned off.  But it is useful
** for testing.
*/
#ifndef fts5yytestcase
# define fts5yytestcase(X)
#endif

/* Macro to determine if stack space has the ability to grow using
** heap memory.
*/
#if fts5YYSTACKDEPTH<=0 || fts5YYDYNSTACK
# define fts5YYGROWABLESTACK 1
#else
# define fts5YYGROWABLESTACK 0
#endif

/* Guarantee a minimum number of initial stack slots.
*/
#if fts5YYSTACKDEPTH<=0
# undef fts5YYSTACKDEPTH
# define fts5YYSTACKDEPTH 2  /* Need a minimum stack size */
#endif


/* Next are the tables used to determine what action to take based on the
** current state and lookahead token.  These tables are used to implement
** functions that take a state number and lookahead value and return an
** action integer.
**
** Suppose the action integer is N.  Then the action is determined as
** follows
**
**   0 <= N <= fts5YY_MAX_SHIFT             Shift N.  That is, push the lookahead
**                                      token onto the stack and goto state N.
**
**   N between fts5YY_MIN_SHIFTREDUCE       Shift to an arbitrary state then
**     and fts5YY_MAX_SHIFTREDUCE           reduce by rule N-fts5YY_MIN_SHIFTREDUCE.
**
**   N == fts5YY_ERROR_ACTION               A syntax error has occurred.
**
**   N == fts5YY_ACCEPT_ACTION              The parser accepts its input.
**
**   N == fts5YY_NO_ACTION                  No such action.  Denotes unused
**                                      slots in the fts5yy_action[] table.
**
**   N between fts5YY_MIN_REDUCE            Reduce by rule N-fts5YY_MIN_REDUCE
**     and fts5YY_MAX_REDUCE
**
** The action table is constructed as a single large table named fts5yy_action[].
** Given state S and lookahead X, the action is computed as either:
**
**    (A)   N = fts5yy_action[ fts5yy_shift_ofst[S] + X ]
**    (B)   N = fts5yy_default[S]
**
** The (A) formula is preferred.  The B formula is used instead if
** fts5yy_lookahead[fts5yy_shift_ofst[S]+X] is not equal to X.
**
** The formulas above are for computing the action when the lookahead is
** a terminal symbol.  If the lookahead is a non-terminal (as occurs after
** a reduce action) then the fts5yy_reduce_ofst[] array is used in place of
** the fts5yy_shift_ofst[] array.
**
** The following are the tables generated in this section:
**
**  fts5yy_action[]        A single table containing all actions.
**  fts5yy_lookahead[]     A table containing the lookahead for each entry in
**                     fts5yy_action.  Used to detect hash collisions.
**  fts5yy_shift_ofst[]    For each state, the offset into fts5yy_action for
**                     shifting terminals.
**  fts5yy_reduce_ofst[]   For each state, the offset into fts5yy_action for
**                     shifting non-terminals after a reduce.
**  fts5yy_default[]       Default action for each state.
**
*********** Begin parsing tables **********************************************/
#define fts5YY_ACTTAB_COUNT (105)
static const fts5YYACTIONTYPE fts5yy_action[] = {
 /*     0 */    81,   20,   96,    6,   28,   99,   98,   26,   26,   18,
 /*    10 */    96,    6,   28,   17,   98,   56,   26,   19,   96,    6,
 /*    20 */    28,   14,   98,   14,   26,   31,   92,   96,    6,   28,
 /*    30 */   108,   98,   25,   26,   21,   96,    6,   28,   78,   98,
 /*    40 */    58,   26,   29,   96,    6,   28,  107,   98,   22,   26,
 /*    50 */    24,   16,   12,   11,    1,   13,   13,   24,   16,   23,
 /*    60 */    11,   33,   34,   13,   97,    8,   27,   32,   98,    7,
 /*    70 */    26,    3,    4,    5,    3,    4,    5,    3,   83,    4,
 /*    80 */     5,    3,   63,    5,    3,   62,   12,    2,   86,   13,
 /*    90 */     9,   30,   10,   10,   54,   57,   75,   78,   78,   53,
 /*   100 */    57,   15,   82,   82,   71,
};
static const fts5YYCODETYPE fts5yy_lookahead[] = {
 /*     0 */    16,   17,   18,   19,   20,   22,   22,   24,   24,   17,
 /*    10 */    18,   19,   20,    7,   22,    9,   24,   17,   18,   19,
 /*    20 */    20,    9,   22,    9,   24,   13,   17,   18,   19,   20,
 /*    30 */    26,   22,   24,   24,   17,   18,   19,   20,   15,   22,
 /*    40 */     9,   24,   17,   18,   19,   20,   26,   22,   21,   24,
 /*    50 */     6,    7,    9,    9,   10,   12,   12,    6,    7,   21,
 /*    60 */     9,   24,   25,   12,   18,    5,   20,   14,   22,    5,
 /*    70 */    24,    3,    1,    2,    3,    1,    2,    3,    0,    1,
 /*    80 */     2,    3,   11,    2,    3,   11,    9,   10,    5,   12,
 /*    90 */    23,   24,   10,   10,    8,    9,    9,   15,   15,    8,
 /*   100 */     9,    9,   27,   27,   11,   27,   27,   27,   27,   27,
 /*   110 */    27,   27,   27,   27,   27,   27,   27,   27,   27,   27,
 /*   120 */    27,
};
#define fts5YY_SHIFT_COUNT    (34)
#define fts5YY_SHIFT_MIN      (0)
#define fts5YY_SHIFT_MAX      (93)
static const unsigned char fts5yy_shift_ofst[] = {
 /*     0 */    44,   44,   44,   44,   44,   44,   51,   77,   43,   12,
 /*    10 */    14,   83,   82,   14,   23,   23,   31,   31,   71,   74,
 /*    20 */    78,   81,   86,   91,    6,   53,   53,   60,   64,   68,
 /*    30 */    53,   87,   92,   53,   93,
};
#define fts5YY_REDUCE_COUNT (17)
#define fts5YY_REDUCE_MIN   (-17)
#define fts5YY_REDUCE_MAX   (67)
static const signed char fts5yy_reduce_ofst[] = {
 /*     0 */   -16,   -8,    0,    9,   17,   25,   46,  -17,  -17,   37,
 /*    10 */    67,    4,    4,    8,    4,   20,   27,   38,
};
static const fts5YYACTIONTYPE fts5yy_default[] = {
 /*     0 */    80,   80,   80,   80,   80,   80,   95,   80,   80,  105,
 /*    10 */    80,  110,  110,   80,  110,  110,   80,   80,   80,   80,
 /*    20 */    80,   91,   80,   80,   80,  101,  100,   80,   80,   90,
 /*    30 */   103,   80,   80,  104,   80,
};
/********** End of lemon-generated parsing tables *****************************/

/* The next table maps tokens (terminal symbols) into fallback tokens.
** If a construct like the following:
**
**      %fallback ID X Y Z.
**
** appears in the grammar, then ID becomes a fallback token for X, Y,
** and Z.  Whenever one of the tokens X, Y, or Z is input to the parser
** but it does not parse, the type of the token is changed to ID and
** the parse is retried before an error is thrown.
**
** This feature can be used, for example, to cause some keywords in a language
** to revert to identifiers if they keyword does not apply in the context where
** it appears.
*/
#ifdef fts5YYFALLBACK
static const fts5YYCODETYPE fts5yyFallback[] = {
};
#endif /* fts5YYFALLBACK */

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
struct fts5yyStackEntry {
  fts5YYACTIONTYPE stateno;  /* The state-number, or reduce action in SHIFTREDUCE */
  fts5YYCODETYPE major;      /* The major token value.  This is the code
                         ** number for the token at this stack level */
  fts5YYMINORTYPE minor;     /* The user-supplied minor token value.  This
                         ** is the value of the token  */
};
typedef struct fts5yyStackEntry fts5yyStackEntry;

/* The state of the parser is completely contained in an instance of
** the following structure */
struct fts5yyParser {
  fts5yyStackEntry *fts5yytos;          /* Pointer to top element of the stack */
#ifdef fts5YYTRACKMAXSTACKDEPTH
  int fts5yyhwm;                    /* High-water mark of the stack */
#endif
#ifndef fts5YYNOERRORRECOVERY
  int fts5yyerrcnt;                 /* Shifts left before out of the error */
#endif
  sqlite3Fts5ParserARG_SDECL                /* A place to hold %extra_argument */
  sqlite3Fts5ParserCTX_SDECL                /* A place to hold %extra_context */
  fts5yyStackEntry *fts5yystackEnd;           /* Last entry in the stack */
  fts5yyStackEntry *fts5yystack;              /* The parser stack */
  fts5yyStackEntry fts5yystk0[fts5YYSTACKDEPTH];  /* Initial stack space */
};
typedef struct fts5yyParser fts5yyParser;

/* #include <assert.h> */
#ifndef NDEBUG
/* #include <stdio.h> */
static FILE *fts5yyTraceFILE = 0;
static char *fts5yyTracePrompt = 0;
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
static void sqlite3Fts5ParserTrace(FILE *TraceFILE, char *zTracePrompt){
  fts5yyTraceFILE = TraceFILE;
  fts5yyTracePrompt = zTracePrompt;
  if( fts5yyTraceFILE==0 ) fts5yyTracePrompt = 0;
  else if( fts5yyTracePrompt==0 ) fts5yyTraceFILE = 0;
}
#endif /* NDEBUG */

#if defined(fts5YYCOVERAGE) || !defined(NDEBUG)
/* For tracing shifts, the names of all terminals and nonterminals
** are required.  The following table supplies these names */
static const char *const fts5yyTokenName[] = {
  /*    0 */ "$",
  /*    1 */ "OR",
  /*    2 */ "AND",
  /*    3 */ "NOT",
  /*    4 */ "TERM",
  /*    5 */ "COLON",
  /*    6 */ "MINUS",
  /*    7 */ "LCP",
  /*    8 */ "RCP",
  /*    9 */ "STRING",
  /*   10 */ "LP",
  /*   11 */ "RP",
  /*   12 */ "CARET",
  /*   13 */ "COMMA",
  /*   14 */ "PLUS",
  /*   15 */ "STAR",
  /*   16 */ "input",
  /*   17 */ "expr",
  /*   18 */ "cnearset",
  /*   19 */ "exprlist",
  /*   20 */ "colset",
  /*   21 */ "colsetlist",
  /*   22 */ "nearset",
  /*   23 */ "nearphrases",
  /*   24 */ "phrase",
  /*   25 */ "neardist_opt",
  /*   26 */ "star_opt",
};
#endif /* defined(fts5YYCOVERAGE) || !defined(NDEBUG) */

#ifndef NDEBUG
/* For tracing reduce actions, the names of all rules are required.
*/
static const char *const fts5yyRuleName[] = {
 /*   0 */ "input ::= expr",
 /*   1 */ "colset ::= MINUS LCP colsetlist RCP",
 /*   2 */ "colset ::= LCP colsetlist RCP",
 /*   3 */ "colset ::= STRING",
 /*   4 */ "colset ::= MINUS STRING",
 /*   5 */ "colsetlist ::= colsetlist STRING",
 /*   6 */ "colsetlist ::= STRING",
 /*   7 */ "expr ::= expr AND expr",
 /*   8 */ "expr ::= expr OR expr",
 /*   9 */ "expr ::= expr NOT expr",
 /*  10 */ "expr ::= colset COLON LP expr RP",
 /*  11 */ "expr ::= LP expr RP",
 /*  12 */ "expr ::= exprlist",
 /*  13 */ "exprlist ::= cnearset",
 /*  14 */ "exprlist ::= exprlist cnearset",
 /*  15 */ "cnearset ::= nearset",
 /*  16 */ "cnearset ::= colset COLON nearset",
 /*  17 */ "nearset ::= phrase",
 /*  18 */ "nearset ::= CARET phrase",
 /*  19 */ "nearset ::= STRING LP nearphrases neardist_opt RP",
 /*  20 */ "nearphrases ::= phrase",
 /*  21 */ "nearphrases ::= nearphrases phrase",
 /*  22 */ "neardist_opt ::=",
 /*  23 */ "neardist_opt ::= COMMA STRING",
 /*  24 */ "phrase ::= phrase PLUS STRING star_opt",
 /*  25 */ "phrase ::= STRING star_opt",
 /*  26 */ "star_opt ::= STAR",
 /*  27 */ "star_opt ::=",
};
#endif /* NDEBUG */


#if fts5YYGROWABLESTACK
/*
** Try to increase the size of the parser stack.  Return the number
** of errors.  Return 0 on success.
*/
static int fts5yyGrowStack(fts5yyParser *p){
  int oldSize = 1 + (int)(p->fts5yystackEnd - p->fts5yystack);
  int newSize;
  int idx;
  fts5yyStackEntry *pNew;

  newSize = oldSize*2 + 100;
  idx = (int)(p->fts5yytos - p->fts5yystack);
  if( p->fts5yystack==p->fts5yystk0 ){
    pNew = fts5YYREALLOC(0, newSize*sizeof(pNew[0]));
    if( pNew==0 ) return 1;
    memcpy(pNew, p->fts5yystack, oldSize*sizeof(pNew[0]));
  }else{
    pNew = fts5YYREALLOC(p->fts5yystack, newSize*sizeof(pNew[0]));
    if( pNew==0 ) return 1;
  }
  p->fts5yystack = pNew;
  p->fts5yytos = &p->fts5yystack[idx];
#ifndef NDEBUG
  if( fts5yyTraceFILE ){
    fprintf(fts5yyTraceFILE,"%sStack grows from %d to %d entries.\n",
            fts5yyTracePrompt, oldSize, newSize);
  }
#endif
  p->fts5yystackEnd = &p->fts5yystack[newSize-1];
  return 0;
}
