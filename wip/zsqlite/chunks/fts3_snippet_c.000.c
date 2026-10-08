/*
** 2009 Oct 23
**
** The author disclaims copyright to this source code.  In place of
** a legal notice, here is a blessing:
**
**    May you do good and not evil.
**    May you find forgiveness for yourself and forgive others.
**    May you share freely, never taking more than you give.
**
******************************************************************************
*/

/* #include "fts3Int.h" */
#if !defined(SQLITE_CORE) || defined(SQLITE_ENABLE_FTS3)

/* #include <string.h> */
/* #include <assert.h> */

#ifndef SQLITE_AMALGAMATION
typedef sqlite3_int64 i64;
#endif

/*
** Characters that may appear in the second argument to matchinfo().
*/
#define FTS3_MATCHINFO_NPHRASE   'p'        /* 1 value */
#define FTS3_MATCHINFO_NCOL      'c'        /* 1 value */
#define FTS3_MATCHINFO_NDOC      'n'        /* 1 value */
#define FTS3_MATCHINFO_AVGLENGTH 'a'        /* nCol values */
#define FTS3_MATCHINFO_LENGTH    'l'        /* nCol values */
#define FTS3_MATCHINFO_LCS       's'        /* nCol values */
#define FTS3_MATCHINFO_HITS      'x'        /* 3*nCol*nPhrase values */
#define FTS3_MATCHINFO_LHITS     'y'        /* nCol*nPhrase values */
#define FTS3_MATCHINFO_LHITS_BM  'b'        /* nCol*nPhrase values */

/*
** The default value for the second argument to matchinfo().
*/
#define FTS3_MATCHINFO_DEFAULT   "pcx"


/*
** Used as an sqlite3Fts3ExprIterate() context when loading phrase doclists to
** Fts3Expr.aDoclist[]/nDoclist.
*/
typedef struct LoadDoclistCtx LoadDoclistCtx;
struct LoadDoclistCtx {
  Fts3Cursor *pCsr;               /* FTS3 Cursor */
  int nPhrase;                    /* Number of phrases seen so far */
  int nToken;                     /* Number of tokens seen so far */
};

/*
** The following types are used as part of the implementation of the
** fts3BestSnippet() routine.
*/
typedef struct SnippetIter SnippetIter;
typedef struct SnippetPhrase SnippetPhrase;
typedef struct SnippetFragment SnippetFragment;

struct SnippetIter {
  Fts3Cursor *pCsr;               /* Cursor snippet is being generated from */
  int iCol;                       /* Extract snippet from this column */
  int nSnippet;                   /* Requested snippet length (in tokens) */
  int nPhrase;                    /* Number of phrases in query */
  SnippetPhrase *aPhrase;         /* Array of size nPhrase */
  int iCurrent;                   /* First token of current snippet */
};

struct SnippetPhrase {
  int nToken;                     /* Number of tokens in phrase */
  char *pList;                    /* Pointer to start of phrase position list */
  i64 iHead;                      /* Next value in position list */
  char *pHead;                    /* Position list data following iHead */
  i64 iTail;                      /* Next value in trailing position list */
  char *pTail;                    /* Position list data following iTail */
};

struct SnippetFragment {
  int iCol;                       /* Column snippet is extracted from */
  int iPos;                       /* Index of first token in snippet */
  u64 covered;                    /* Mask of query phrases covered */
  u64 hlmask;                     /* Mask of snippet terms to highlight */
};

/*
** This type is used as an sqlite3Fts3ExprIterate() context object while
** accumulating the data returned by the matchinfo() function.
*/
typedef struct MatchInfo MatchInfo;
struct MatchInfo {
  Fts3Cursor *pCursor;            /* FTS3 Cursor */
  int nCol;                       /* Number of columns in table */
  int nPhrase;                    /* Number of matchable phrases in query */
  sqlite3_int64 nDoc;             /* Number of docs in database */
  char flag;
  u32 *aMatchinfo;                /* Pre-allocated buffer */
};

/*
** An instance of this structure is used to manage a pair of buffers, each
** (nElem * sizeof(u32)) bytes in size. See the MatchinfoBuffer code below
** for details.
*/
struct MatchinfoBuffer {
  u8 aRef[3];
  int nElem;
  int bGlobal;                    /* Set if global data is loaded */
  char *zMatchinfo;
  u32 aMatchinfo[1];
};


/*
** The snippet() and offsets() functions both return text values. An instance
** of the following structure is used to accumulate those values while the
** functions are running. See fts3StringAppend() for details.
*/
typedef struct StrBuffer StrBuffer;
struct StrBuffer {
  char *z;                        /* Pointer to buffer containing string */
  int n;                          /* Length of z in bytes (excl. nul-term) */
  int nAlloc;                     /* Allocated size of buffer z in bytes */
};


/*************************************************************************
** Start of MatchinfoBuffer code.
*/

/*
** Allocate a two-slot MatchinfoBuffer object.
*/
static MatchinfoBuffer *fts3MIBufferNew(size_t nElem, const char *zMatchinfo){
  MatchinfoBuffer *pRet;
  sqlite3_int64 nByte = sizeof(u32) * (2*(sqlite3_int64)nElem + 1)
                           + sizeof(MatchinfoBuffer);
  sqlite3_int64 nStr = strlen(zMatchinfo);

  pRet = sqlite3Fts3MallocZero(nByte + nStr+1);
  if( pRet ){
    pRet->aMatchinfo[0] = (u8*)(&pRet->aMatchinfo[1]) - (u8*)pRet;
    pRet->aMatchinfo[1+nElem] = pRet->aMatchinfo[0]
                                      + sizeof(u32)*((int)nElem+1);
    pRet->nElem = (int)nElem;
    pRet->zMatchinfo = ((char*)pRet) + nByte;
    memcpy(pRet->zMatchinfo, zMatchinfo, nStr+1);
    pRet->aRef[0] = 1;
  }

  return pRet;
}

static void fts3MIBufferFree(void *p){
  MatchinfoBuffer *pBuf = (MatchinfoBuffer*)((u8*)p - ((u32*)p)[-1]);

  assert( (u32*)p==&pBuf->aMatchinfo[1]
       || (u32*)p==&pBuf->aMatchinfo[pBuf->nElem+2]
  );
  if( (u32*)p==&pBuf->aMatchinfo[1] ){
    pBuf->aRef[1] = 0;
  }else{
    pBuf->aRef[2] = 0;
  }

  if( pBuf->aRef[0]==0 && pBuf->aRef[1]==0 && pBuf->aRef[2]==0 ){
    sqlite3_free(pBuf);
  }
}

static void (*fts3MIBufferAlloc(MatchinfoBuffer *p, u32 **paOut))(void*){
  void (*xRet)(void*) = 0;
  u32 *aOut = 0;

  if( p->aRef[1]==0 ){
    p->aRef[1] = 1;
    aOut = &p->aMatchinfo[1];
    xRet = fts3MIBufferFree;
  }
  else if( p->aRef[2]==0 ){
    p->aRef[2] = 1;
    aOut = &p->aMatchinfo[p->nElem+2];
    xRet = fts3MIBufferFree;
  }else{
    aOut = (u32*)sqlite3_malloc64(p->nElem * sizeof(u32));
    if( aOut ){
      xRet = sqlite3_free;
      if( p->bGlobal ) memcpy(aOut, &p->aMatchinfo[1], p->nElem*sizeof(u32));
    }
  }

  *paOut = aOut;
  return xRet;
}

static void fts3MIBufferSetGlobal(MatchinfoBuffer *p){
  p->bGlobal = 1;
  memcpy(&p->aMatchinfo[2+p->nElem], &p->aMatchinfo[1], p->nElem*sizeof(u32));
}

/*
** Free a MatchinfoBuffer object allocated using fts3MIBufferNew()
*/
SQLITE_PRIVATE void sqlite3Fts3MIBufferFree(MatchinfoBuffer *p){
  if( p ){
    assert( p->aRef[0]==1 );
    p->aRef[0] = 0;
    if( p->aRef[0]==0 && p->aRef[1]==0 && p->aRef[2]==0 ){
      sqlite3_free(p);
    }
  }
}

/*
** End of MatchinfoBuffer code.
*************************************************************************/


/*
** This function is used to help iterate through a position-list. A position
** list is a list of unique integers, sorted from smallest to largest. Each
** element of the list is represented by an FTS3 varint that takes the value
** of the difference between the current element and the previous one plus
** two. For example, to store the position-list:
**
**     4 9 113
**
** the three varints:
**
**     6 7 106
**
** are encoded.
**
** When this function is called, *pp points to the start of an element of
** the list. *piPos contains the value of the previous entry in the list.
** After it returns, *piPos contains the value of the next element of the
** list and *pp is advanced to the following varint.
*/
static void fts3GetDeltaPosition(char **pp, i64 *piPos){
  int iVal;
  *pp += fts3GetVarint32(*pp, &iVal);
  *piPos += (iVal-2);
}

/*
** Helper function for sqlite3Fts3ExprIterate() (see below).
*/
static int fts3ExprIterate2(
  Fts3Expr *pExpr,                /* Expression to iterate phrases of */
  int *piPhrase,                  /* Pointer to phrase counter */
  int (*x)(Fts3Expr*,int,void*),  /* Callback function to invoke for phrases */
  void *pCtx                      /* Second argument to pass to callback */
){
  int rc;                         /* Return code */
  int eType = pExpr->eType;     /* Type of expression node pExpr */

  if( eType!=FTSQUERY_PHRASE ){
    assert( pExpr->pLeft && pExpr->pRight );
    rc = fts3ExprIterate2(pExpr->pLeft, piPhrase, x, pCtx);
    if( rc==SQLITE_OK && eType!=FTSQUERY_NOT ){
      rc = fts3ExprIterate2(pExpr->pRight, piPhrase, x, pCtx);
    }
  }else{
    rc = x(pExpr, *piPhrase, pCtx);
    (*piPhrase)++;
  }
  return rc;
}

/*
** Iterate through all phrase nodes in an FTS3 query, except those that
** are part of a sub-tree that is the right-hand-side of a NOT operator.
** For each phrase node found, the supplied callback function is invoked.
**
** If the callback function returns anything other than SQLITE_OK,
** the iteration is abandoned and the error code returned immediately.
** Otherwise, SQLITE_OK is returned after a callback has been made for
** all eligible phrase nodes.
*/
SQLITE_PRIVATE int sqlite3Fts3ExprIterate(
  Fts3Expr *pExpr,                /* Expression to iterate phrases of */
  int (*x)(Fts3Expr*,int,void*),  /* Callback function to invoke for phrases */
  void *pCtx                      /* Second argument to pass to callback */
){
  int iPhrase = 0;                /* Variable used as the phrase counter */
  return fts3ExprIterate2(pExpr, &iPhrase, x, pCtx);
}

/*
** This is an sqlite3Fts3ExprIterate() callback used while loading the
** doclists for each phrase into Fts3Expr.aDoclist[]/nDoclist. See also
** fts3ExprLoadDoclists().
*/
static int fts3ExprLoadDoclistsCb(Fts3Expr *pExpr, int iPhrase, void *ctx){
  int rc = SQLITE_OK;
  Fts3Phrase *pPhrase = pExpr->pPhrase;
  LoadDoclistCtx *p = (LoadDoclistCtx *)ctx;

  UNUSED_PARAMETER(iPhrase);

  p->nPhrase++;
  p->nToken += pPhrase->nToken;

  return rc;
}

/*
** Load the doclists for each phrase in the query associated with FTS3 cursor
** pCsr.
**
** If pnPhrase is not NULL, then *pnPhrase is set to the number of matchable
** phrases in the expression (all phrases except those directly or
** indirectly descended from the right-hand-side of a NOT operator). If
** pnToken is not NULL, then it is set to the number of tokens in all
** matchable phrases of the expression.
*/
static int fts3ExprLoadDoclists(
  Fts3Cursor *pCsr,               /* Fts3 cursor for current query */
  int *pnPhrase,                  /* OUT: Number of phrases in query */
  int *pnToken                    /* OUT: Number of tokens in query */
){
  int rc;                         /* Return Code */
  LoadDoclistCtx sCtx = {0,0,0};  /* Context for sqlite3Fts3ExprIterate() */
  sCtx.pCsr = pCsr;
  rc = sqlite3Fts3ExprIterate(pCsr->pExpr,fts3ExprLoadDoclistsCb,(void*)&sCtx);
  if( pnPhrase ) *pnPhrase = sCtx.nPhrase;
  if( pnToken ) *pnToken = sCtx.nToken;
  return rc;
}

static int fts3ExprPhraseCountCb(Fts3Expr *pExpr, int iPhrase, void *ctx){
  (*(int *)ctx)++;
  pExpr->iPhrase = iPhrase;
  return SQLITE_OK;
}
static int fts3ExprPhraseCount(Fts3Expr *pExpr){
  int nPhrase = 0;
  (void)sqlite3Fts3ExprIterate(pExpr, fts3ExprPhraseCountCb, (void *)&nPhrase);
  return nPhrase;
}

/*
** Advance the position list iterator specified by the first two
** arguments so that it points to the first element with a value greater
** than or equal to parameter iNext.
*/
static void fts3SnippetAdvance(char **ppIter, i64 *piIter, int iNext){
  char *pIter = *ppIter;
  if( pIter ){
    i64 iIter = *piIter;

    while( iIter<iNext ){
      if( 0==(*pIter & 0xFE) ){
        iIter = -1;
        pIter = 0;
        break;
      }
      fts3GetDeltaPosition(&pIter, &iIter);
    }

    *piIter = iIter;
    *ppIter = pIter;
  }
}
