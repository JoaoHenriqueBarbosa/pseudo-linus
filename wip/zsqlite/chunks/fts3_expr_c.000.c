/*
** 2008 Nov 28
**
** The author disclaims copyright to this source code.  In place of
** a legal notice, here is a blessing:
**
**    May you do good and not evil.
**    May you find forgiveness for yourself and forgive others.
**    May you share freely, never taking more than you give.
**
******************************************************************************
**
** This module contains code that implements a parser for fts3 query strings
** (the right-hand argument to the MATCH operator). Because the supported
** syntax is relatively simple, the whole tokenizer/parser system is
** hand-coded.
*/
/* #include "fts3Int.h" */
#if !defined(SQLITE_CORE) || defined(SQLITE_ENABLE_FTS3)

/*
** By default, this module parses the legacy syntax that has been
** traditionally used by fts3. Or, if SQLITE_ENABLE_FTS3_PARENTHESIS
** is defined, then it uses the new syntax. The differences between
** the new and the old syntaxes are:
**
**  a) The new syntax supports parenthesis. The old does not.
**
**  b) The new syntax supports the AND and NOT operators. The old does not.
**
**  c) The old syntax supports the "-" token qualifier. This is not
**     supported by the new syntax (it is replaced by the NOT operator).
**
**  d) When using the old syntax, the OR operator has a greater precedence
**     than an implicit AND. When using the new, both implicity and explicit
**     AND operators have a higher precedence than OR.
**
** If compiled with SQLITE_TEST defined, then this module exports the
** symbol "int sqlite3_fts3_enable_parentheses". Setting this variable
** to zero causes the module to use the old syntax. If it is set to
** non-zero the new syntax is activated. This is so both syntaxes can
** be tested using a single build of testfixture.
**
** The following describes the syntax supported by the fts3 MATCH
** operator in a similar format to that used by the lemon parser
** generator. This module does not use actually lemon, it uses a
** custom parser.
**
**   query ::= andexpr (OR andexpr)*.
**
**   andexpr ::= notexpr (AND? notexpr)*.
**
**   notexpr ::= nearexpr (NOT nearexpr|-TOKEN)*.
**   notexpr ::= LP query RP.
**
**   nearexpr ::= phrase (NEAR distance_opt nearexpr)*.
**
**   distance_opt ::= .
**   distance_opt ::= / INTEGER.
**
**   phrase ::= TOKEN.
**   phrase ::= COLUMN:TOKEN.
**   phrase ::= "TOKEN TOKEN TOKEN...".
*/

#ifdef SQLITE_TEST
SQLITE_API int sqlite3_fts3_enable_parentheses = 0;
#else
# ifdef SQLITE_ENABLE_FTS3_PARENTHESIS
#  define sqlite3_fts3_enable_parentheses 1
# else
#  define sqlite3_fts3_enable_parentheses 0
# endif
#endif

/*
** Default span for NEAR operators.
*/
#define SQLITE_FTS3_DEFAULT_NEAR_PARAM 10

/* #include <string.h> */
/* #include <assert.h> */

/*
** isNot:
**   This variable is used by function getNextNode(). When getNextNode() is
**   called, it sets ParseContext.isNot to true if the 'next node' is a
**   FTSQUERY_PHRASE with a unary "-" attached to it. i.e. "mysql" in the
**   FTS3 query "sqlite -mysql". Otherwise, ParseContext.isNot is set to
**   zero.
*/
typedef struct ParseContext ParseContext;
struct ParseContext {
  sqlite3_tokenizer *pTokenizer;      /* Tokenizer module */
  int iLangid;                        /* Language id used with tokenizer */
  const char **azCol;                 /* Array of column names for fts3 table */
  int bFts4;                          /* True to allow FTS4-only syntax */
  int nCol;                           /* Number of entries in azCol[] */
  int iDefaultCol;                    /* Default column to query */
  int isNot;                          /* True if getNextNode() sees a unary - */
  sqlite3_context *pCtx;              /* Write error message here */
  int nNest;                          /* Number of nested brackets */
};

/*
** This function is equivalent to the standard isspace() function.
**
** The standard isspace() can be awkward to use safely, because although it
** is defined to accept an argument of type int, its behavior when passed
** an integer that falls outside of the range of the unsigned char type
** is undefined (and sometimes, "undefined" means segfault). This wrapper
** is defined to accept an argument of type char, and always returns 0 for
** any values that fall outside of the range of the unsigned char type (i.e.
** negative values).
*/
static int fts3isspace(char c){
  return c==' ' || c=='\t' || c=='\n' || c=='\r' || c=='\v' || c=='\f';
}

/*
** Allocate nByte bytes of memory using sqlite3_malloc(). If successful,
** zero the memory before returning a pointer to it. If unsuccessful,
** return NULL.
*/
SQLITE_PRIVATE void *sqlite3Fts3MallocZero(sqlite3_int64 nByte){
  void *pRet = sqlite3_malloc64(nByte);
  if( pRet ) memset(pRet, 0, nByte);
  return pRet;
}

SQLITE_PRIVATE int sqlite3Fts3OpenTokenizer(
  sqlite3_tokenizer *pTokenizer,
  int iLangid,
  const char *z,
  int n,
  sqlite3_tokenizer_cursor **ppCsr
){
  sqlite3_tokenizer_module const *pModule = pTokenizer->pModule;
  sqlite3_tokenizer_cursor *pCsr = 0;
  int rc;

  rc = pModule->xOpen(pTokenizer, z, n, &pCsr);
  assert( rc==SQLITE_OK || pCsr==0 );
  if( rc==SQLITE_OK ){
    pCsr->pTokenizer = pTokenizer;
    if( pModule->iVersion>=1 ){
      rc = pModule->xLanguageid(pCsr, iLangid);
      if( rc!=SQLITE_OK ){
        pModule->xClose(pCsr);
        pCsr = 0;
      }
    }
  }
  *ppCsr = pCsr;
  return rc;
}

/*
** Function getNextNode(), which is called by fts3ExprParse(), may itself
** call fts3ExprParse(). So this forward declaration is required.
*/
static int fts3ExprParse(ParseContext *, const char *, int, Fts3Expr **, int *);

/*
** Extract the next token from buffer z (length n) using the tokenizer
** and other information (column names etc.) in pParse. Create an Fts3Expr
** structure of type FTSQUERY_PHRASE containing a phrase consisting of this
** single token and set *ppExpr to point to it. If the end of the buffer is
** reached before a token is found, set *ppExpr to zero. It is the
** responsibility of the caller to eventually deallocate the allocated
** Fts3Expr structure (if any) by passing it to sqlite3_free().
**
** Return SQLITE_OK if successful, or SQLITE_NOMEM if a memory allocation
** fails.
*/
static int getNextToken(
  ParseContext *pParse,                   /* fts3 query parse context */
  int iCol,                               /* Value for Fts3Phrase.iColumn */
  const char *z, int n,                   /* Input string */
  Fts3Expr **ppExpr,                      /* OUT: expression */
  int *pnConsumed                         /* OUT: Number of bytes consumed */
){
  sqlite3_tokenizer *pTokenizer = pParse->pTokenizer;
  sqlite3_tokenizer_module const *pModule = pTokenizer->pModule;
  int rc;
  sqlite3_tokenizer_cursor *pCursor;
  Fts3Expr *pRet = 0;
  int i = 0;

  /* Set variable i to the maximum number of bytes of input to tokenize. */
  for(i=0; i<n; i++){
    if( sqlite3_fts3_enable_parentheses && (z[i]=='(' || z[i]==')') ) break;
    if( z[i]=='"' ) break;
  }

  *pnConsumed = i;
  rc = sqlite3Fts3OpenTokenizer(pTokenizer, pParse->iLangid, z, i, &pCursor);
  if( rc==SQLITE_OK ){
    const char *zToken;
    int nToken = 0, iStart = 0, iEnd = 0, iPosition = 0;
    sqlite3_int64 nByte;                    /* total space to allocate */

    rc = pModule->xNext(pCursor, &zToken, &nToken, &iStart, &iEnd, &iPosition);
    if( rc==SQLITE_OK ){
      nByte = sizeof(Fts3Expr) + sizeof(Fts3Phrase) + nToken;
      pRet = (Fts3Expr *)sqlite3Fts3MallocZero(nByte);
      if( !pRet ){
        rc = SQLITE_NOMEM;
      }else{
        pRet->eType = FTSQUERY_PHRASE;
        pRet->pPhrase = (Fts3Phrase *)&pRet[1];
        pRet->pPhrase->nToken = 1;
        pRet->pPhrase->iColumn = iCol;
        pRet->pPhrase->aToken[0].n = nToken;
        pRet->pPhrase->aToken[0].z = (char *)&pRet->pPhrase[1];
        memcpy(pRet->pPhrase->aToken[0].z, zToken, nToken);

        if( iEnd<n && z[iEnd]=='*' ){
          pRet->pPhrase->aToken[0].isPrefix = 1;
          iEnd++;
        }

        while( 1 ){
          if( !sqlite3_fts3_enable_parentheses
           && iStart>0 && z[iStart-1]=='-'
          ){
            pParse->isNot = 1;
            iStart--;
          }else if( pParse->bFts4 && iStart>0 && z[iStart-1]=='^' ){
            pRet->pPhrase->aToken[0].bFirst = 1;
            iStart--;
          }else{
            break;
          }
        }

      }
      *pnConsumed = iEnd;
    }else if( i && rc==SQLITE_DONE ){
      rc = SQLITE_OK;
    }

    pModule->xClose(pCursor);
  }

  *ppExpr = pRet;
  return rc;
}


/*
** Enlarge a memory allocation.  If an out-of-memory allocation occurs,
** then free the old allocation.
*/
static void *fts3ReallocOrFree(void *pOrig, sqlite3_int64 nNew){
  void *pRet = sqlite3_realloc64(pOrig, nNew);
  if( !pRet ){
    sqlite3_free(pOrig);
  }
  return pRet;
}

/*
** Buffer zInput, length nInput, contains the contents of a quoted string
** that appeared as part of an fts3 query expression. Neither quote character
** is included in the buffer. This function attempts to tokenize the entire
** input buffer and create an Fts3Expr structure of type FTSQUERY_PHRASE
** containing the results.
**
** If successful, SQLITE_OK is returned and *ppExpr set to point at the
** allocated Fts3Expr structure. Otherwise, either SQLITE_NOMEM (out of memory
** error) or SQLITE_ERROR (tokenization error) is returned and *ppExpr set
** to 0.
*/
static int getNextString(
  ParseContext *pParse,                   /* fts3 query parse context */
  const char *zInput, int nInput,         /* Input string */
  Fts3Expr **ppExpr                       /* OUT: expression */
){
  sqlite3_tokenizer *pTokenizer = pParse->pTokenizer;
  sqlite3_tokenizer_module const *pModule = pTokenizer->pModule;
  int rc;
  Fts3Expr *p = 0;
  sqlite3_tokenizer_cursor *pCursor = 0;
  char *zTemp = 0;
  int nTemp = 0;

  const int nSpace = sizeof(Fts3Expr) + sizeof(Fts3Phrase);
  int nToken = 0;

  /* The final Fts3Expr data structure, including the Fts3Phrase,
  ** Fts3PhraseToken structures token buffers are all stored as a single
  ** allocation so that the expression can be freed with a single call to
  ** sqlite3_free(). Setting this up requires a two pass approach.
  **
  ** The first pass, in the block below, uses a tokenizer cursor to iterate
  ** through the tokens in the expression. This pass uses fts3ReallocOrFree()
  ** to assemble data in two dynamic buffers:
  **
  **   Buffer p: Points to the Fts3Expr structure, followed by the Fts3Phrase
  **             structure, followed by the array of Fts3PhraseToken
  **             structures. This pass only populates the Fts3PhraseToken array.
  **
  **   Buffer zTemp: Contains copies of all tokens.
  **
  ** The second pass, in the block that begins "if( rc==SQLITE_DONE )" below,
  ** appends buffer zTemp to buffer p, and fills in the Fts3Expr and Fts3Phrase
  ** structures.
  */
  rc = sqlite3Fts3OpenTokenizer(
      pTokenizer, pParse->iLangid, zInput, nInput, &pCursor);
  if( rc==SQLITE_OK ){
    int ii;
    for(ii=0; rc==SQLITE_OK; ii++){
      const char *zByte;
      int nByte = 0, iBegin = 0, iEnd = 0, iPos = 0;
      rc = pModule->xNext(pCursor, &zByte, &nByte, &iBegin, &iEnd, &iPos);
      if( rc==SQLITE_OK ){
        Fts3PhraseToken *pToken;

        p = fts3ReallocOrFree(p, nSpace + ii*sizeof(Fts3PhraseToken));
        if( !p ) goto no_mem;

        zTemp = fts3ReallocOrFree(zTemp, nTemp + nByte);
        if( !zTemp ) goto no_mem;

        assert( nToken==ii );
        pToken = &((Fts3Phrase *)(&p[1]))->aToken[ii];
        memset(pToken, 0, sizeof(Fts3PhraseToken));

        memcpy(&zTemp[nTemp], zByte, nByte);
        nTemp += nByte;

        pToken->n = nByte;
        pToken->isPrefix = (iEnd<nInput && zInput[iEnd]=='*');
        pToken->bFirst = (iBegin>0 && zInput[iBegin-1]=='^');
        nToken = ii+1;
      }
    }

    pModule->xClose(pCursor);
    pCursor = 0;
  }

  if( rc==SQLITE_DONE ){
    int jj;
    char *zBuf = 0;

    p = fts3ReallocOrFree(p, nSpace + nToken*sizeof(Fts3PhraseToken) + nTemp);
    if( !p ) goto no_mem;
    memset(p, 0, (char *)&(((Fts3Phrase *)&p[1])->aToken[0])-(char *)p);
    p->eType = FTSQUERY_PHRASE;
    p->pPhrase = (Fts3Phrase *)&p[1];
    p->pPhrase->iColumn = pParse->iDefaultCol;
    p->pPhrase->nToken = nToken;

    zBuf = (char *)&p->pPhrase->aToken[nToken];
    if( zTemp ){
      memcpy(zBuf, zTemp, nTemp);
      sqlite3_free(zTemp);
    }else{
      assert( nTemp==0 );
    }

    for(jj=0; jj<p->pPhrase->nToken; jj++){
      p->pPhrase->aToken[jj].z = zBuf;
      zBuf += p->pPhrase->aToken[jj].n;
    }
    rc = SQLITE_OK;
  }

  *ppExpr = p;
  return rc;
no_mem:

  if( pCursor ){
    pModule->xClose(pCursor);
  }
  sqlite3_free(zTemp);
  sqlite3_free(p);
  *ppExpr = 0;
  return SQLITE_NOMEM;
}
