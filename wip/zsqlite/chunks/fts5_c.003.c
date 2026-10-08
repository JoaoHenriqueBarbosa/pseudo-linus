

/*
** End of interface to code in fts5_varint.c.
**************************************************************************/


/**************************************************************************
** Interface to code in fts5_main.c.
*/

/*
** Virtual-table object.
*/
typedef struct Fts5Table Fts5Table;
struct Fts5Table {
  sqlite3_vtab base;              /* Base class used by SQLite core */
  Fts5Config *pConfig;            /* Virtual table configuration */
  Fts5Index *pIndex;              /* Full-text index */
};

static int sqlite3Fts5GetTokenizer(
  Fts5Global*,
  const char **azArg,
  int nArg,
  Fts5Config*,
  char **pzErr
);

static Fts5Table *sqlite3Fts5TableFromCsrid(Fts5Global*, i64);

static int sqlite3Fts5FlushToDisk(Fts5Table*);

/*
** End of interface to code in fts5.c.
**************************************************************************/

/**************************************************************************
** Interface to code in fts5_hash.c.
*/
typedef struct Fts5Hash Fts5Hash;

/*
** Create a hash table, free a hash table.
*/
static int sqlite3Fts5HashNew(Fts5Config*, Fts5Hash**, int *pnSize);
static void sqlite3Fts5HashFree(Fts5Hash*);

static int sqlite3Fts5HashWrite(
  Fts5Hash*,
  i64 iRowid,                     /* Rowid for this entry */
  int iCol,                       /* Column token appears in (-ve -> delete) */
  int iPos,                       /* Position of token within column */
  char bByte,
  const char *pToken, int nToken  /* Token to add or remove to or from index */
);

/*
** Empty (but do not delete) a hash table.
*/
static void sqlite3Fts5HashClear(Fts5Hash*);

/*
** Return true if the hash is empty, false otherwise.
*/
static int sqlite3Fts5HashIsEmpty(Fts5Hash*);

static int sqlite3Fts5HashQuery(
  Fts5Hash*,                      /* Hash table to query */
  int nPre,
  const char *pTerm, int nTerm,   /* Query term */
  void **ppObj,                   /* OUT: Pointer to doclist for pTerm */
  int *pnDoclist                  /* OUT: Size of doclist in bytes */
);

static int sqlite3Fts5HashScanInit(
  Fts5Hash*,                      /* Hash table to query */
  const char *pTerm, int nTerm    /* Query prefix */
);
static void sqlite3Fts5HashScanNext(Fts5Hash*);
static int sqlite3Fts5HashScanEof(Fts5Hash*);
static void sqlite3Fts5HashScanEntry(Fts5Hash *,
  const char **pzTerm,            /* OUT: term (nul-terminated) */
  int *pnTerm,                    /* OUT: Size of term in bytes */
  const u8 **ppDoclist,           /* OUT: pointer to doclist */
  int *pnDoclist                  /* OUT: size of doclist in bytes */
);



/*
** End of interface to code in fts5_hash.c.
**************************************************************************/

/**************************************************************************
** Interface to code in fts5_storage.c. fts5_storage.c contains contains
** code to access the data stored in the %_content and %_docsize tables.
*/

#define FTS5_STMT_SCAN_ASC  0     /* SELECT rowid, * FROM ... ORDER BY 1 ASC */
#define FTS5_STMT_SCAN_DESC 1     /* SELECT rowid, * FROM ... ORDER BY 1 DESC */
#define FTS5_STMT_LOOKUP    2     /* SELECT rowid, * FROM ... WHERE rowid=? */

typedef struct Fts5Storage Fts5Storage;

static int sqlite3Fts5StorageOpen(Fts5Config*, Fts5Index*, int, Fts5Storage**, char**);
static int sqlite3Fts5StorageClose(Fts5Storage *p);
static int sqlite3Fts5StorageRename(Fts5Storage*, const char *zName);

static int sqlite3Fts5DropAll(Fts5Config*);
static int sqlite3Fts5CreateTable(Fts5Config*, const char*, const char*, int, char **);

static int sqlite3Fts5StorageDelete(Fts5Storage *p, i64, sqlite3_value**);
static int sqlite3Fts5StorageContentInsert(Fts5Storage *p, sqlite3_value**, i64*);
static int sqlite3Fts5StorageIndexInsert(Fts5Storage *p, sqlite3_value**, i64);

static int sqlite3Fts5StorageIntegrity(Fts5Storage *p, int iArg);

static int sqlite3Fts5StorageStmt(Fts5Storage *p, int eStmt, sqlite3_stmt**, char**);
static void sqlite3Fts5StorageStmtRelease(Fts5Storage *p, int eStmt, sqlite3_stmt*);

static int sqlite3Fts5StorageDocsize(Fts5Storage *p, i64 iRowid, int *aCol);
static int sqlite3Fts5StorageSize(Fts5Storage *p, int iCol, i64 *pnAvg);
static int sqlite3Fts5StorageRowCount(Fts5Storage *p, i64 *pnRow);

static int sqlite3Fts5StorageSync(Fts5Storage *p);
static int sqlite3Fts5StorageRollback(Fts5Storage *p);

static int sqlite3Fts5StorageConfigValue(
    Fts5Storage *p, const char*, sqlite3_value*, int
);

static int sqlite3Fts5StorageDeleteAll(Fts5Storage *p);
static int sqlite3Fts5StorageRebuild(Fts5Storage *p);
static int sqlite3Fts5StorageOptimize(Fts5Storage *p);
static int sqlite3Fts5StorageMerge(Fts5Storage *p, int nMerge);
static int sqlite3Fts5StorageReset(Fts5Storage *p);

/*
** End of interface to code in fts5_storage.c.
**************************************************************************/


/**************************************************************************
** Interface to code in fts5_expr.c.
*/
typedef struct Fts5Expr Fts5Expr;
typedef struct Fts5ExprNode Fts5ExprNode;
typedef struct Fts5Parse Fts5Parse;
typedef struct Fts5Token Fts5Token;
typedef struct Fts5ExprPhrase Fts5ExprPhrase;
typedef struct Fts5ExprNearset Fts5ExprNearset;

struct Fts5Token {
  const char *p;                  /* Token text (not NULL terminated) */
  int n;                          /* Size of buffer p in bytes */
};

/* Parse a MATCH expression. */
static int sqlite3Fts5ExprNew(
  Fts5Config *pConfig,
  int bPhraseToAnd,
  int iCol,                       /* Column on LHS of MATCH operator */
  const char *zExpr,
  Fts5Expr **ppNew,
  char **pzErr
);
static int sqlite3Fts5ExprPattern(
  Fts5Config *pConfig,
  int bGlob,
  int iCol,
  const char *zText,
  Fts5Expr **pp
);

/*
** for(rc = sqlite3Fts5ExprFirst(pExpr, pIdx, bDesc);
**     rc==SQLITE_OK && 0==sqlite3Fts5ExprEof(pExpr);
**     rc = sqlite3Fts5ExprNext(pExpr)
** ){
**   // The document with rowid iRowid matches the expression!
**   i64 iRowid = sqlite3Fts5ExprRowid(pExpr);
** }
*/
static int sqlite3Fts5ExprFirst(Fts5Expr*, Fts5Index *pIdx, i64 iMin, int bDesc);
static int sqlite3Fts5ExprNext(Fts5Expr*, i64 iMax);
static int sqlite3Fts5ExprEof(Fts5Expr*);
static i64 sqlite3Fts5ExprRowid(Fts5Expr*);

static void sqlite3Fts5ExprFree(Fts5Expr*);
static int sqlite3Fts5ExprAnd(Fts5Expr **pp1, Fts5Expr *p2);

/* Called during startup to register a UDF with SQLite */
static int sqlite3Fts5ExprInit(Fts5Global*, sqlite3*);

static int sqlite3Fts5ExprPhraseCount(Fts5Expr*);
static int sqlite3Fts5ExprPhraseSize(Fts5Expr*, int iPhrase);
static int sqlite3Fts5ExprPoslist(Fts5Expr*, int, const u8 **);

typedef struct Fts5PoslistPopulator Fts5PoslistPopulator;
static Fts5PoslistPopulator *sqlite3Fts5ExprClearPoslists(Fts5Expr*, int);
static int sqlite3Fts5ExprPopulatePoslists(
    Fts5Config*, Fts5Expr*, Fts5PoslistPopulator*, int, const char*, int
);
static void sqlite3Fts5ExprCheckPoslists(Fts5Expr*, i64);

static int sqlite3Fts5ExprClonePhrase(Fts5Expr*, int, Fts5Expr**);

static int sqlite3Fts5ExprPhraseCollist(Fts5Expr *, int, const u8 **, int *);

static int sqlite3Fts5ExprQueryToken(Fts5Expr*, int, int, const char**, int*);
static int sqlite3Fts5ExprInstToken(Fts5Expr*, i64, int, int, int, int, const char**, int*);
static void sqlite3Fts5ExprClearTokens(Fts5Expr*);

/*******************************************
** The fts5_expr.c API above this point is used by the other hand-written
** C code in this module. The interfaces below this point are called by
** the parser code in fts5parse.y.  */

static void sqlite3Fts5ParseError(Fts5Parse *pParse, const char *zFmt, ...);

static Fts5ExprNode *sqlite3Fts5ParseNode(
  Fts5Parse *pParse,
  int eType,
  Fts5ExprNode *pLeft,
  Fts5ExprNode *pRight,
  Fts5ExprNearset *pNear
);

static Fts5ExprNode *sqlite3Fts5ParseImplicitAnd(
  Fts5Parse *pParse,
  Fts5ExprNode *pLeft,
  Fts5ExprNode *pRight
);

static Fts5ExprPhrase *sqlite3Fts5ParseTerm(
  Fts5Parse *pParse,
  Fts5ExprPhrase *pPhrase,
  Fts5Token *pToken,
  int bPrefix
);

static void sqlite3Fts5ParseSetCaret(Fts5ExprPhrase*);

static Fts5ExprNearset *sqlite3Fts5ParseNearset(
  Fts5Parse*,
  Fts5ExprNearset*,
  Fts5ExprPhrase*
);

static Fts5Colset *sqlite3Fts5ParseColset(
  Fts5Parse*,
  Fts5Colset*,
  Fts5Token *
);

static void sqlite3Fts5ParsePhraseFree(Fts5ExprPhrase*);
static void sqlite3Fts5ParseNearsetFree(Fts5ExprNearset*);
static void sqlite3Fts5ParseNodeFree(Fts5ExprNode*);

static void sqlite3Fts5ParseSetDistance(Fts5Parse*, Fts5ExprNearset*, Fts5Token*);
static void sqlite3Fts5ParseSetColset(Fts5Parse*, Fts5ExprNode*, Fts5Colset*);
static Fts5Colset *sqlite3Fts5ParseColsetInvert(Fts5Parse*, Fts5Colset*);
static void sqlite3Fts5ParseFinished(Fts5Parse *pParse, Fts5ExprNode *p);
static void sqlite3Fts5ParseNear(Fts5Parse *pParse, Fts5Token*);

/*
** End of interface to code in fts5_expr.c.
**************************************************************************/



/**************************************************************************
** Interface to code in fts5_aux.c.
*/

static int sqlite3Fts5AuxInit(fts5_api*);
/*
** End of interface to code in fts5_aux.c.
**************************************************************************/

/**************************************************************************
** Interface to code in fts5_tokenizer.c.
*/

static int sqlite3Fts5TokenizerInit(fts5_api*);
static int sqlite3Fts5TokenizerPattern(
    int (*xCreate)(void*, const char**, int, Fts5Tokenizer**),
    Fts5Tokenizer *pTok
);
/*
** End of interface to code in fts5_tokenizer.c.
**************************************************************************/

/**************************************************************************
** Interface to code in fts5_vocab.c.
*/

static int sqlite3Fts5VocabInit(Fts5Global*, sqlite3*);

/*
** End of interface to code in fts5_vocab.c.
**************************************************************************/


/**************************************************************************
** Interface to automatically generated code in fts5_unicode2.c.
*/
static int sqlite3Fts5UnicodeIsdiacritic(int c);
static int sqlite3Fts5UnicodeFold(int c, int bRemoveDiacritic);

static int sqlite3Fts5UnicodeCatParse(const char*, u8*);
static int sqlite3Fts5UnicodeCategory(u32 iCode);
static void sqlite3Fts5UnicodeAscii(u8*, u8*);
/*
** End of interface to code in fts5_unicode2.c.
**************************************************************************/

#endif

#define FTS5_OR                               1
#define FTS5_AND                              2
#define FTS5_NOT                              3
#define FTS5_TERM                             4
#define FTS5_COLON                            5
#define FTS5_MINUS                            6
#define FTS5_LCP                              7
#define FTS5_RCP                              8
#define FTS5_STRING                           9
#define FTS5_LP                              10
#define FTS5_RP                              11
#define FTS5_CARET                           12
#define FTS5_COMMA                           13
#define FTS5_PLUS                            14
#define FTS5_STAR                            15

/* This file is automatically generated by Lemon from input grammar
** source file "fts5parse.y".
*/
/*
** 2000-05-29
**
** The author disclaims copyright to this source code.  In place of
** a legal notice, here is a blessing:
**
**    May you do good and not evil.
**    May you find forgiveness for yourself and forgive others.
**    May you share freely, never taking more than you give.
**
*************************************************************************
** Driver template for the LEMON parser generator.
**
** The "lemon" program processes an LALR(1) input grammar file, then uses
** this template to construct a parser.  The "lemon" program inserts text
** at each "%%" line.  Also, any "P-a-r-s-e" identifer prefix (without the
** interstitial "-" characters) contained in this template is changed into
** the value of the %name directive from the grammar.  Otherwise, the content
** of this template is copied straight through into the generate parser
** source file.
**
** The following is the concatenation of all %include directives from the
** input grammar file:
*/
/************ Begin %include sections from the grammar ************************/

/* #include "fts5Int.h" */
/* #include "fts5parse.h" */

/*
** Disable all error recovery processing in the parser push-down
** automaton.
*/
#define fts5YYNOERRORRECOVERY 1

/*
** Make fts5yytestcase() the same as testcase()
*/
#define fts5yytestcase(X) testcase(X)

/*
** Indicate that sqlite3ParserFree() will never be called with a null
** pointer.
*/
#define fts5YYPARSEFREENOTNULL 1

/*
** Alternative datatype for the argument to the malloc() routine passed
** into sqlite3ParserAlloc().  The default is size_t.
*/
#define fts5YYMALLOCARGTYPE  u64

/**************** End of %include directives **********************************/
/* These constants specify the various numeric values for terminal symbols.
***************** Begin token definitions *************************************/
#ifndef FTS5_OR
#define FTS5_OR                              1
#define FTS5_AND                             2
#define FTS5_NOT                             3
#define FTS5_TERM                            4
#define FTS5_COLON                           5
#define FTS5_MINUS                           6
#define FTS5_LCP                             7
#define FTS5_RCP                             8
#define FTS5_STRING                          9
#define FTS5_LP                             10
#define FTS5_RP                             11
#define FTS5_CARET                          12
#define FTS5_COMMA                          13
#define FTS5_PLUS                           14
#define FTS5_STAR                           15
#endif
/**************** End token definitions ***************************************/

/* The next sections is a series of control #defines.
** various aspects of the generated parser.
**    fts5YYCODETYPE         is the data type used to store the integer codes
**                       that represent terminal and non-terminal symbols.
**                       "unsigned char" is used if there are fewer than
**                       256 symbols.  Larger types otherwise.
**    fts5YYNOCODE           is a number of type fts5YYCODETYPE that is not used for
**                       any terminal or nonterminal symbol.
**    fts5YYFALLBACK         If defined, this indicates that one or more tokens
**                       (also known as: "terminal symbols") have fall-back
**                       values which should be used if the original symbol
**                       would not parse.  This permits keywords to sometimes
**                       be used as identifiers, for example.
**    fts5YYACTIONTYPE       is the data type used for "action codes" - numbers
**                       that indicate what to do in response to the next
**                       token.
**    sqlite3Fts5ParserFTS5TOKENTYPE     is the data type used for minor type for terminal
**                       symbols.  Background: A "minor type" is a semantic
**                       value associated with a terminal or non-terminal
**                       symbols.  For example, for an "ID" terminal symbol,
**                       the minor type might be the name of the identifier.
**                       Each non-terminal can have a different minor type.
**                       Terminal symbols all have the same minor type, though.
**                       This macros defines the minor type for terminal
**                       symbols.
**    fts5YYMINORTYPE        is the data type used for all minor types.
**                       This is typically a union of many types, one of
**                       which is sqlite3Fts5ParserFTS5TOKENTYPE.  The entry in the union
**                       for terminal symbols is called "fts5yy0".
**    fts5YYSTACKDEPTH       is the maximum depth of the parser's stack.  If
**                       zero the stack is dynamically sized using realloc()
**    sqlite3Fts5ParserARG_SDECL     A static variable declaration for the %extra_argument
**    sqlite3Fts5ParserARG_PDECL     A parameter declaration for the %extra_argument
**    sqlite3Fts5ParserARG_PARAM     Code to pass %extra_argument as a subroutine parameter
**    sqlite3Fts5ParserARG_STORE     Code to store %extra_argument into fts5yypParser
**    sqlite3Fts5ParserARG_FETCH     Code to extract %extra_argument from fts5yypParser
**    sqlite3Fts5ParserCTX_*         As sqlite3Fts5ParserARG_ except for %extra_context
**    fts5YYREALLOC          Name of the realloc() function to use
**    fts5YYFREE             Name of the free() function to use
**    fts5YYDYNSTACK         True if stack space should be extended on heap
**    fts5YYERRORSYMBOL      is the code number of the error symbol.  If not
**                       defined, then do no error processing.
**    fts5YYNSTATE           the combined number of states.
**    fts5YYNRULE            the number of rules in the grammar
**    fts5YYNFTS5TOKEN           Number of terminal symbols
**    fts5YY_MAX_SHIFT       Maximum value for shift actions
**    fts5YY_MIN_SHIFTREDUCE Minimum value for shift-reduce actions
**    fts5YY_MAX_SHIFTREDUCE Maximum value for shift-reduce actions
**    fts5YY_ERROR_ACTION    The fts5yy_action[] code for syntax error
**    fts5YY_ACCEPT_ACTION   The fts5yy_action[] code for accept
**    fts5YY_NO_ACTION       The fts5yy_action[] code for no-op
**    fts5YY_MIN_REDUCE      Minimum value for reduce actions
**    fts5YY_MAX_REDUCE      Maximum value for reduce actions
**    fts5YY_MIN_DSTRCTR     Minimum symbol value that has a destructor
**    fts5YY_MAX_DSTRCTR     Maximum symbol value that has a destructor
*/
#ifndef INTERFACE
# define INTERFACE 1
#endif
/************* Begin control #defines *****************************************/
#define fts5YYCODETYPE unsigned char
#define fts5YYNOCODE 27
#define fts5YYACTIONTYPE unsigned char
#define sqlite3Fts5ParserFTS5TOKENTYPE Fts5Token
typedef union {
  int fts5yyinit;
  sqlite3Fts5ParserFTS5TOKENTYPE fts5yy0;
  int fts5yy4;
  Fts5Colset* fts5yy11;
  Fts5ExprNode* fts5yy24;
  Fts5ExprNearset* fts5yy46;
  Fts5ExprPhrase* fts5yy53;
} fts5YYMINORTYPE;
