
/************************************************************************/

/*
** The first time the bm25() function is called for a query, an instance
** of the following structure is allocated and populated.
*/
typedef struct Fts5Bm25Data Fts5Bm25Data;
struct Fts5Bm25Data {
  int nPhrase;                    /* Number of phrases in query */
  double avgdl;                   /* Average number of tokens in each row */
  double *aIDF;                   /* IDF for each phrase */
  double *aFreq;                  /* Array used to calculate phrase freq. */
};

/*
** Callback used by fts5Bm25GetData() to count the number of rows in the
** table matched by each individual phrase within the query.
*/
static int fts5CountCb(
  const Fts5ExtensionApi *pApi,
  Fts5Context *pFts,
  void *pUserData                 /* Pointer to sqlite3_int64 variable */
){
  sqlite3_int64 *pn = (sqlite3_int64*)pUserData;
  UNUSED_PARAM2(pApi, pFts);
  (*pn)++;
  return SQLITE_OK;
}

/*
** Set *ppData to point to the Fts5Bm25Data object for the current query.
** If the object has not already been allocated, allocate and populate it
** now.
*/
static int fts5Bm25GetData(
  const Fts5ExtensionApi *pApi,
  Fts5Context *pFts,
  Fts5Bm25Data **ppData           /* OUT: bm25-data object for this query */
){
  int rc = SQLITE_OK;             /* Return code */
  Fts5Bm25Data *p;                /* Object to return */

  p = (Fts5Bm25Data*)pApi->xGetAuxdata(pFts, 0);
  if( p==0 ){
    int nPhrase;                  /* Number of phrases in query */
    sqlite3_int64 nRow = 0;       /* Number of rows in table */
    sqlite3_int64 nToken = 0;     /* Number of tokens in table */
    sqlite3_int64 nByte;          /* Bytes of space to allocate */
    int i;

    /* Allocate the Fts5Bm25Data object */
    nPhrase = pApi->xPhraseCount(pFts);
    nByte = sizeof(Fts5Bm25Data) + nPhrase*2*sizeof(double);
    p = (Fts5Bm25Data*)sqlite3_malloc64(nByte);
    if( p==0 ){
      rc = SQLITE_NOMEM;
    }else{
      memset(p, 0, (size_t)nByte);
      p->nPhrase = nPhrase;
      p->aIDF = (double*)&p[1];
      p->aFreq = &p->aIDF[nPhrase];
    }

    /* Calculate the average document length for this FTS5 table */
    if( rc==SQLITE_OK ) rc = pApi->xRowCount(pFts, &nRow);
    assert( rc!=SQLITE_OK || nRow>0 );
    if( rc==SQLITE_OK ) rc = pApi->xColumnTotalSize(pFts, -1, &nToken);
    if( rc==SQLITE_OK ) p->avgdl = (double)nToken  / (double)nRow;

    /* Calculate an IDF for each phrase in the query */
    for(i=0; rc==SQLITE_OK && i<nPhrase; i++){
      sqlite3_int64 nHit = 0;
      rc = pApi->xQueryPhrase(pFts, i, (void*)&nHit, fts5CountCb);
      if( rc==SQLITE_OK ){
        /* Calculate the IDF (Inverse Document Frequency) for phrase i.
        ** This is done using the standard BM25 formula as found on wikipedia:
        **
        **   IDF = log( (N - nHit + 0.5) / (nHit + 0.5) )
        **
        ** where "N" is the total number of documents in the set and nHit
        ** is the number that contain at least one instance of the phrase
        ** under consideration.
        **
        ** The problem with this is that if (N < 2*nHit), the IDF is
        ** negative. Which is undesirable. So the mimimum allowable IDF is
        ** (1e-6) - roughly the same as a term that appears in just over
        ** half of set of 5,000,000 documents.  */
        double idf = log( (nRow - nHit + 0.5) / (nHit + 0.5) );
        if( idf<=0.0 ) idf = 1e-6;
        p->aIDF[i] = idf;
      }
    }

    if( rc!=SQLITE_OK ){
      sqlite3_free(p);
    }else{
      rc = pApi->xSetAuxdata(pFts, p, sqlite3_free);
    }
    if( rc!=SQLITE_OK ) p = 0;
  }
  *ppData = p;
  return rc;
}

/*
** Implementation of bm25() function.
*/
static void fts5Bm25Function(
  const Fts5ExtensionApi *pApi,   /* API offered by current FTS version */
  Fts5Context *pFts,              /* First arg to pass to pApi functions */
  sqlite3_context *pCtx,          /* Context for returning result/error */
  int nVal,                       /* Number of values in apVal[] array */
  sqlite3_value **apVal           /* Array of trailing arguments */
){
  const double k1 = 1.2;          /* Constant "k1" from BM25 formula */
  const double b = 0.75;          /* Constant "b" from BM25 formula */
  int rc;                         /* Error code */
  double score = 0.0;             /* SQL function return value */
  Fts5Bm25Data *pData;            /* Values allocated/calculated once only */
  int i;                          /* Iterator variable */
  int nInst = 0;                  /* Value returned by xInstCount() */
  double D = 0.0;                 /* Total number of tokens in row */
  double *aFreq = 0;              /* Array of phrase freq. for current row */

  /* Calculate the phrase frequency (symbol "f(qi,D)" in the documentation)
  ** for each phrase in the query for the current row. */
  rc = fts5Bm25GetData(pApi, pFts, &pData);
  if( rc==SQLITE_OK ){
    aFreq = pData->aFreq;
    memset(aFreq, 0, sizeof(double) * pData->nPhrase);
    rc = pApi->xInstCount(pFts, &nInst);
  }
  for(i=0; rc==SQLITE_OK && i<nInst; i++){
    int ip; int ic; int io;
    rc = pApi->xInst(pFts, i, &ip, &ic, &io);
    if( rc==SQLITE_OK ){
      double w = (nVal > ic) ? sqlite3_value_double(apVal[ic]) : 1.0;
      aFreq[ip] += w;
    }
  }

  /* Figure out the total size of the current row in tokens. */
  if( rc==SQLITE_OK ){
    int nTok;
    rc = pApi->xColumnSize(pFts, -1, &nTok);
    D = (double)nTok;
  }

  /* Determine and return the BM25 score for the current row. Or, if an
  ** error has occurred, throw an exception. */
  if( rc==SQLITE_OK ){
    for(i=0; i<pData->nPhrase; i++){
      score += pData->aIDF[i] * (
          ( aFreq[i] * (k1 + 1.0) ) /
          ( aFreq[i] + k1 * (1 - b + b * D / pData->avgdl) )
      );
    }
    sqlite3_result_double(pCtx, -1.0 * score);
  }else{
    sqlite3_result_error_code(pCtx, rc);
  }
}

static int sqlite3Fts5AuxInit(fts5_api *pApi){
  struct Builtin {
    const char *zFunc;            /* Function name (nul-terminated) */
    void *pUserData;              /* User-data pointer */
    fts5_extension_function xFunc;/* Callback function */
    void (*xDestroy)(void*);      /* Destructor function */
  } aBuiltin [] = {
    { "snippet",   0, fts5SnippetFunction, 0 },
    { "highlight", 0, fts5HighlightFunction, 0 },
    { "bm25",      0, fts5Bm25Function,    0 },
  };
  int rc = SQLITE_OK;             /* Return code */
  int i;                          /* To iterate through builtin functions */

  for(i=0; rc==SQLITE_OK && i<ArraySize(aBuiltin); i++){
    rc = pApi->xCreateFunction(pApi,
        aBuiltin[i].zFunc,
        aBuiltin[i].pUserData,
        aBuiltin[i].xFunc,
        aBuiltin[i].xDestroy
    );
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
*/



/* #include "fts5Int.h" */

static int sqlite3Fts5BufferSize(int *pRc, Fts5Buffer *pBuf, u32 nByte){
  if( (u32)pBuf->nSpace<nByte ){
    u64 nNew = pBuf->nSpace ? pBuf->nSpace : 64;
    u8 *pNew;
    while( nNew<nByte ){
      nNew = nNew * 2;
    }
    pNew = sqlite3_realloc64(pBuf->p, nNew);
    if( pNew==0 ){
      *pRc = SQLITE_NOMEM;
      return 1;
    }else{
      pBuf->nSpace = (int)nNew;
      pBuf->p = pNew;
    }
  }
  return 0;
}


/*
** Encode value iVal as an SQLite varint and append it to the buffer object
** pBuf. If an OOM error occurs, set the error code in p.
*/
static void sqlite3Fts5BufferAppendVarint(int *pRc, Fts5Buffer *pBuf, i64 iVal){
  if( fts5BufferGrow(pRc, pBuf, 9) ) return;
  pBuf->n += sqlite3Fts5PutVarint(&pBuf->p[pBuf->n], iVal);
}

static void sqlite3Fts5Put32(u8 *aBuf, int iVal){
  aBuf[0] = (iVal>>24) & 0x00FF;
  aBuf[1] = (iVal>>16) & 0x00FF;
  aBuf[2] = (iVal>> 8) & 0x00FF;
  aBuf[3] = (iVal>> 0) & 0x00FF;
}

static int sqlite3Fts5Get32(const u8 *aBuf){
  return (int)((((u32)aBuf[0])<<24) + (aBuf[1]<<16) + (aBuf[2]<<8) + aBuf[3]);
}

/*
** Append buffer nData/pData to buffer pBuf. If an OOM error occurs, set
** the error code in p. If an error has already occurred when this function
** is called, it is a no-op.
*/
static void sqlite3Fts5BufferAppendBlob(
  int *pRc,
  Fts5Buffer *pBuf,
  u32 nData,
  const u8 *pData
){
  if( nData ){
    if( fts5BufferGrow(pRc, pBuf, nData) ) return;
    assert( pBuf->p!=0 );
    memcpy(&pBuf->p[pBuf->n], pData, nData);
    pBuf->n += nData;
  }
}

/*
** Append the nul-terminated string zStr to the buffer pBuf. This function
** ensures that the byte following the buffer data is set to 0x00, even
** though this byte is not included in the pBuf->n count.
*/
static void sqlite3Fts5BufferAppendString(
  int *pRc,
  Fts5Buffer *pBuf,
  const char *zStr
){
  int nStr = (int)strlen(zStr);
  sqlite3Fts5BufferAppendBlob(pRc, pBuf, nStr+1, (const u8*)zStr);
  pBuf->n--;
}

/*
** Argument zFmt is a printf() style format string. This function performs
** the printf() style processing, then appends the results to buffer pBuf.
**
** Like sqlite3Fts5BufferAppendString(), this function ensures that the byte
** following the buffer data is set to 0x00, even though this byte is not
** included in the pBuf->n count.
*/
static void sqlite3Fts5BufferAppendPrintf(
  int *pRc,
  Fts5Buffer *pBuf,
  char *zFmt, ...
){
  if( *pRc==SQLITE_OK ){
    char *zTmp;
    va_list ap;
    va_start(ap, zFmt);
    zTmp = sqlite3_vmprintf(zFmt, ap);
    va_end(ap);

    if( zTmp==0 ){
      *pRc = SQLITE_NOMEM;
    }else{
      sqlite3Fts5BufferAppendString(pRc, pBuf, zTmp);
      sqlite3_free(zTmp);
    }
  }
}

static char *sqlite3Fts5Mprintf(int *pRc, const char *zFmt, ...){
  char *zRet = 0;
  if( *pRc==SQLITE_OK ){
    va_list ap;
    va_start(ap, zFmt);
    zRet = sqlite3_vmprintf(zFmt, ap);
    va_end(ap);
    if( zRet==0 ){
      *pRc = SQLITE_NOMEM;
    }
  }
  return zRet;
}


/*
** Free any buffer allocated by pBuf. Zero the structure before returning.
*/
static void sqlite3Fts5BufferFree(Fts5Buffer *pBuf){
  sqlite3_free(pBuf->p);
  memset(pBuf, 0, sizeof(Fts5Buffer));
}

/*
** Zero the contents of the buffer object. But do not free the associated
** memory allocation.
*/
static void sqlite3Fts5BufferZero(Fts5Buffer *pBuf){
  pBuf->n = 0;
}

/*
** Set the buffer to contain nData/pData. If an OOM error occurs, leave an
** the error code in p. If an error has already occurred when this function
** is called, it is a no-op.
*/
static void sqlite3Fts5BufferSet(
  int *pRc,
  Fts5Buffer *pBuf,
  int nData,
  const u8 *pData
){
  pBuf->n = 0;
  sqlite3Fts5BufferAppendBlob(pRc, pBuf, nData, pData);
}
