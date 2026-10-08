
static int sqlite3Fts5ConfigSetValue(
  Fts5Config *pConfig,
  const char *zKey,
  sqlite3_value *pVal,
  int *pbBadkey
){
  int rc = SQLITE_OK;

  if( 0==sqlite3_stricmp(zKey, "pgsz") ){
    int pgsz = 0;
    if( SQLITE_INTEGER==sqlite3_value_numeric_type(pVal) ){
      pgsz = sqlite3_value_int(pVal);
    }
    if( pgsz<32 || pgsz>FTS5_MAX_PAGE_SIZE ){
      *pbBadkey = 1;
    }else{
      pConfig->pgsz = pgsz;
    }
  }

  else if( 0==sqlite3_stricmp(zKey, "hashsize") ){
    int nHashSize = -1;
    if( SQLITE_INTEGER==sqlite3_value_numeric_type(pVal) ){
      nHashSize = sqlite3_value_int(pVal);
    }
    if( nHashSize<=0 ){
      *pbBadkey = 1;
    }else{
      pConfig->nHashSize = nHashSize;
    }
  }

  else if( 0==sqlite3_stricmp(zKey, "automerge") ){
    int nAutomerge = -1;
    if( SQLITE_INTEGER==sqlite3_value_numeric_type(pVal) ){
      nAutomerge = sqlite3_value_int(pVal);
    }
    if( nAutomerge<0 || nAutomerge>64 ){
      *pbBadkey = 1;
    }else{
      if( nAutomerge==1 ) nAutomerge = FTS5_DEFAULT_AUTOMERGE;
      pConfig->nAutomerge = nAutomerge;
    }
  }

  else if( 0==sqlite3_stricmp(zKey, "usermerge") ){
    int nUsermerge = -1;
    if( SQLITE_INTEGER==sqlite3_value_numeric_type(pVal) ){
      nUsermerge = sqlite3_value_int(pVal);
    }
    if( nUsermerge<2 || nUsermerge>16 ){
      *pbBadkey = 1;
    }else{
      pConfig->nUsermerge = nUsermerge;
    }
  }

  else if( 0==sqlite3_stricmp(zKey, "crisismerge") ){
    int nCrisisMerge = -1;
    if( SQLITE_INTEGER==sqlite3_value_numeric_type(pVal) ){
      nCrisisMerge = sqlite3_value_int(pVal);
    }
    if( nCrisisMerge<0 ){
      *pbBadkey = 1;
    }else{
      if( nCrisisMerge<=1 ) nCrisisMerge = FTS5_DEFAULT_CRISISMERGE;
      if( nCrisisMerge>=FTS5_MAX_SEGMENT ) nCrisisMerge = FTS5_MAX_SEGMENT-1;
      pConfig->nCrisisMerge = nCrisisMerge;
    }
  }

  else if( 0==sqlite3_stricmp(zKey, "deletemerge") ){
    int nVal = -1;
    if( SQLITE_INTEGER==sqlite3_value_numeric_type(pVal) ){
      nVal = sqlite3_value_int(pVal);
    }else{
      *pbBadkey = 1;
    }
    if( nVal<0 ) nVal = FTS5_DEFAULT_DELETE_AUTOMERGE;
    if( nVal>100 ) nVal = 0;
    pConfig->nDeleteMerge = nVal;
  }

  else if( 0==sqlite3_stricmp(zKey, "rank") ){
    const char *zIn = (const char*)sqlite3_value_text(pVal);
    char *zRank;
    char *zRankArgs;
    rc = sqlite3Fts5ConfigParseRank(zIn, &zRank, &zRankArgs);
    if( rc==SQLITE_OK ){
      sqlite3_free(pConfig->zRank);
      sqlite3_free(pConfig->zRankArgs);
      pConfig->zRank = zRank;
      pConfig->zRankArgs = zRankArgs;
    }else if( rc==SQLITE_ERROR ){
      rc = SQLITE_OK;
      *pbBadkey = 1;
    }
  }

  else if( 0==sqlite3_stricmp(zKey, "secure-delete") ){
    int bVal = -1;
    if( SQLITE_INTEGER==sqlite3_value_numeric_type(pVal) ){
      bVal = sqlite3_value_int(pVal);
    }
    if( bVal<0 ){
      *pbBadkey = 1;
    }else{
      pConfig->bSecureDelete = (bVal ? 1 : 0);
    }
  }else{
    *pbBadkey = 1;
  }
  return rc;
}

/*
** Load the contents of the %_config table into memory.
*/
static int sqlite3Fts5ConfigLoad(Fts5Config *pConfig, int iCookie){
  const char *zSelect = "SELECT k, v FROM %Q.'%q_config'";
  char *zSql;
  sqlite3_stmt *p = 0;
  int rc = SQLITE_OK;
  int iVersion = 0;

  /* Set default values */
  pConfig->pgsz = FTS5_DEFAULT_PAGE_SIZE;
  pConfig->nAutomerge = FTS5_DEFAULT_AUTOMERGE;
  pConfig->nUsermerge = FTS5_DEFAULT_USERMERGE;
  pConfig->nCrisisMerge = FTS5_DEFAULT_CRISISMERGE;
  pConfig->nHashSize = FTS5_DEFAULT_HASHSIZE;
  pConfig->nDeleteMerge = FTS5_DEFAULT_DELETE_AUTOMERGE;

  zSql = sqlite3Fts5Mprintf(&rc, zSelect, pConfig->zDb, pConfig->zName);
  if( zSql ){
    rc = sqlite3_prepare_v2(pConfig->db, zSql, -1, &p, 0);
    sqlite3_free(zSql);
  }

  assert( rc==SQLITE_OK || p==0 );
  if( rc==SQLITE_OK ){
    while( SQLITE_ROW==sqlite3_step(p) ){
      const char *zK = (const char*)sqlite3_column_text(p, 0);
      sqlite3_value *pVal = sqlite3_column_value(p, 1);
      if( 0==sqlite3_stricmp(zK, "version") ){
        iVersion = sqlite3_value_int(pVal);
      }else{
        int bDummy = 0;
        sqlite3Fts5ConfigSetValue(pConfig, zK, pVal, &bDummy);
      }
    }
    rc = sqlite3_finalize(p);
  }

  if( rc==SQLITE_OK
   && iVersion!=FTS5_CURRENT_VERSION
   && iVersion!=FTS5_CURRENT_VERSION_SECUREDELETE
  ){
    rc = SQLITE_ERROR;
    if( pConfig->pzErrmsg ){
      assert( 0==*pConfig->pzErrmsg );
      *pConfig->pzErrmsg = sqlite3_mprintf("invalid fts5 file format "
          "(found %d, expected %d or %d) - run 'rebuild'",
          iVersion, FTS5_CURRENT_VERSION, FTS5_CURRENT_VERSION_SECUREDELETE
      );
    }
  }else{
    pConfig->iVersion = iVersion;
  }

  if( rc==SQLITE_OK ){
    pConfig->iCookie = iCookie;
  }
  return rc;
}

/*
** 2014 May 31
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
*/



/* #include "fts5Int.h" */
/* #include "fts5parse.h" */

#ifndef SQLITE_FTS5_MAX_EXPR_DEPTH
# define SQLITE_FTS5_MAX_EXPR_DEPTH 256
#endif

/*
** All token types in the generated fts5parse.h file are greater than 0.
*/
#define FTS5_EOF 0

#define FTS5_LARGEST_INT64  (0xffffffff|(((i64)0x7fffffff)<<32))

typedef struct Fts5ExprTerm Fts5ExprTerm;

/*
** Functions generated by lemon from fts5parse.y.
*/
static void *sqlite3Fts5ParserAlloc(void *(*mallocProc)(u64));
static void sqlite3Fts5ParserFree(void*, void (*freeProc)(void*));
static void sqlite3Fts5Parser(void*, int, Fts5Token, Fts5Parse*);
#ifndef NDEBUG
/* #include <stdio.h> */
static void sqlite3Fts5ParserTrace(FILE*, char*);
#endif
static int sqlite3Fts5ParserFallback(int);


struct Fts5Expr {
  Fts5Index *pIndex;
  Fts5Config *pConfig;
  Fts5ExprNode *pRoot;
  int bDesc;                      /* Iterate in descending rowid order */
  int nPhrase;                    /* Number of phrases in expression */
  Fts5ExprPhrase **apExprPhrase;  /* Pointers to phrase objects */
};

/*
** eType:
**   Expression node type. Always one of:
**
**       FTS5_AND                 (nChild, apChild valid)
**       FTS5_OR                  (nChild, apChild valid)
**       FTS5_NOT                 (nChild, apChild valid)
**       FTS5_STRING              (pNear valid)
**       FTS5_TERM                (pNear valid)
**
** iHeight:
**   Distance from this node to furthest leaf. This is always 0 for nodes
**   of type FTS5_STRING and FTS5_TERM. For all other nodes it is one
**   greater than the largest child value.
*/
struct Fts5ExprNode {
  int eType;                      /* Node type */
  int bEof;                       /* True at EOF */
  int bNomatch;                   /* True if entry is not a match */
  int iHeight;                    /* Distance to tree leaf nodes */

  /* Next method for this node. */
  int (*xNext)(Fts5Expr*, Fts5ExprNode*, int, i64);

  i64 iRowid;                     /* Current rowid */
  Fts5ExprNearset *pNear;         /* For FTS5_STRING - cluster of phrases */

  /* Child nodes. For a NOT node, this array always contains 2 entries. For
  ** AND or OR nodes, it contains 2 or more entries.  */
  int nChild;                     /* Number of child nodes */
  Fts5ExprNode *apChild[1];       /* Array of child nodes */
};

#define Fts5NodeIsString(p) ((p)->eType==FTS5_TERM || (p)->eType==FTS5_STRING)

/*
** Invoke the xNext method of an Fts5ExprNode object. This macro should be
** used as if it has the same signature as the xNext() methods themselves.
*/
#define fts5ExprNodeNext(a,b,c,d) (b)->xNext((a), (b), (c), (d))

/*
** An instance of the following structure represents a single search term
** or term prefix.
*/
struct Fts5ExprTerm {
  u8 bPrefix;                     /* True for a prefix term */
  u8 bFirst;                      /* True if token must be first in column */
  char *pTerm;                    /* Term data */
  int nQueryTerm;                 /* Effective size of term in bytes */
  int nFullTerm;                  /* Size of term in bytes incl. tokendata */
  Fts5IndexIter *pIter;           /* Iterator for this term */
  Fts5ExprTerm *pSynonym;         /* Pointer to first in list of synonyms */
};

/*
** A phrase. One or more terms that must appear in a contiguous sequence
** within a document for it to match.
*/
struct Fts5ExprPhrase {
  Fts5ExprNode *pNode;            /* FTS5_STRING node this phrase is part of */
  Fts5Buffer poslist;             /* Current position list */
  int nTerm;                      /* Number of entries in aTerm[] */
  Fts5ExprTerm aTerm[1];          /* Terms that make up this phrase */
};

/*
** One or more phrases that must appear within a certain token distance of
** each other within each matching document.
*/
struct Fts5ExprNearset {
  int nNear;                      /* NEAR parameter */
  Fts5Colset *pColset;            /* Columns to search (NULL -> all columns) */
  int nPhrase;                    /* Number of entries in aPhrase[] array */
  Fts5ExprPhrase *apPhrase[1];    /* Array of phrase pointers */
};


/*
** Parse context.
*/
struct Fts5Parse {
  Fts5Config *pConfig;
  char *zErr;
  int rc;
  int nPhrase;                    /* Size of apPhrase array */
  Fts5ExprPhrase **apPhrase;      /* Array of all phrases */
  Fts5ExprNode *pExpr;            /* Result of a successful parse */
  int bPhraseToAnd;               /* Convert "a+b" to "a AND b" */
};

/*
** Check that the Fts5ExprNode.iHeight variables are set correctly in
** the expression tree passed as the only argument.
*/
#ifndef NDEBUG
static void assert_expr_depth_ok(int rc, Fts5ExprNode *p){
  if( rc==SQLITE_OK ){
    if( p->eType==FTS5_TERM || p->eType==FTS5_STRING || p->eType==0 ){
      assert( p->iHeight==0 );
    }else{
      int ii;
      int iMaxChild = 0;
      for(ii=0; ii<p->nChild; ii++){
        Fts5ExprNode *pChild = p->apChild[ii];
        iMaxChild = MAX(iMaxChild, pChild->iHeight);
        assert_expr_depth_ok(SQLITE_OK, pChild);
      }
      assert( p->iHeight==iMaxChild+1 );
    }
  }
}
#else
# define assert_expr_depth_ok(rc, p)
#endif

static void sqlite3Fts5ParseError(Fts5Parse *pParse, const char *zFmt, ...){
  va_list ap;
  va_start(ap, zFmt);
  if( pParse->rc==SQLITE_OK ){
    assert( pParse->zErr==0 );
    pParse->zErr = sqlite3_vmprintf(zFmt, ap);
    pParse->rc = SQLITE_ERROR;
  }
  va_end(ap);
}
