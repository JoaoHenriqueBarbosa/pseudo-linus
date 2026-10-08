
static void fts5ExprFunctionHr(
  sqlite3_context *pCtx,          /* Function call context */
  int nArg,                       /* Number of args */
  sqlite3_value **apVal           /* Function arguments */
){
  fts5ExprFunction(pCtx, nArg, apVal, 0);
}
static void fts5ExprFunctionTcl(
  sqlite3_context *pCtx,          /* Function call context */
  int nArg,                       /* Number of args */
  sqlite3_value **apVal           /* Function arguments */
){
  fts5ExprFunction(pCtx, nArg, apVal, 1);
}

/*
** The implementation of an SQLite user-defined-function that accepts a
** single integer as an argument. If the integer is an alpha-numeric
** unicode code point, 1 is returned. Otherwise 0.
*/
static void fts5ExprIsAlnum(
  sqlite3_context *pCtx,          /* Function call context */
  int nArg,                       /* Number of args */
  sqlite3_value **apVal           /* Function arguments */
){
  int iCode;
  u8 aArr[32];
  if( nArg!=1 ){
    sqlite3_result_error(pCtx,
        "wrong number of arguments to function fts5_isalnum", -1
    );
    return;
  }
  memset(aArr, 0, sizeof(aArr));
  sqlite3Fts5UnicodeCatParse("L*", aArr);
  sqlite3Fts5UnicodeCatParse("N*", aArr);
  sqlite3Fts5UnicodeCatParse("Co", aArr);
  iCode = sqlite3_value_int(apVal[0]);
  sqlite3_result_int(pCtx, aArr[sqlite3Fts5UnicodeCategory((u32)iCode)]);
}

static void fts5ExprFold(
  sqlite3_context *pCtx,          /* Function call context */
  int nArg,                       /* Number of args */
  sqlite3_value **apVal           /* Function arguments */
){
  if( nArg!=1 && nArg!=2 ){
    sqlite3_result_error(pCtx,
        "wrong number of arguments to function fts5_fold", -1
    );
  }else{
    int iCode;
    int bRemoveDiacritics = 0;
    iCode = sqlite3_value_int(apVal[0]);
    if( nArg==2 ) bRemoveDiacritics = sqlite3_value_int(apVal[1]);
    sqlite3_result_int(pCtx, sqlite3Fts5UnicodeFold(iCode, bRemoveDiacritics));
  }
}
#endif /* if SQLITE_TEST || SQLITE_FTS5_DEBUG */

/*
** This is called during initialization to register the fts5_expr() scalar
** UDF with the SQLite handle passed as the only argument.
*/
static int sqlite3Fts5ExprInit(Fts5Global *pGlobal, sqlite3 *db){
#if defined(SQLITE_TEST) || defined(SQLITE_FTS5_DEBUG)
  struct Fts5ExprFunc {
    const char *z;
    void (*x)(sqlite3_context*,int,sqlite3_value**);
  } aFunc[] = {
    { "fts5_expr",     fts5ExprFunctionHr },
    { "fts5_expr_tcl", fts5ExprFunctionTcl },
    { "fts5_isalnum",  fts5ExprIsAlnum },
    { "fts5_fold",     fts5ExprFold },
  };
  int i;
  int rc = SQLITE_OK;
  void *pCtx = (void*)pGlobal;

  for(i=0; rc==SQLITE_OK && i<ArraySize(aFunc); i++){
    struct Fts5ExprFunc *p = &aFunc[i];
    rc = sqlite3_create_function(db, p->z, -1, SQLITE_UTF8, pCtx, p->x, 0, 0);
  }
#else
  int rc = SQLITE_OK;
  UNUSED_PARAM2(pGlobal,db);
#endif

  /* Avoid warnings indicating that sqlite3Fts5ParserTrace() and
  ** sqlite3Fts5ParserFallback() are unused */
#ifndef NDEBUG
  (void)sqlite3Fts5ParserTrace;
#endif
  (void)sqlite3Fts5ParserFallback;

  return rc;
}

/*
** Return the number of phrases in expression pExpr.
*/
static int sqlite3Fts5ExprPhraseCount(Fts5Expr *pExpr){
  return (pExpr ? pExpr->nPhrase : 0);
}

/*
** Return the number of terms in the iPhrase'th phrase in pExpr.
*/
static int sqlite3Fts5ExprPhraseSize(Fts5Expr *pExpr, int iPhrase){
  if( iPhrase<0 || iPhrase>=pExpr->nPhrase ) return 0;
  return pExpr->apExprPhrase[iPhrase]->nTerm;
}

/*
** This function is used to access the current position list for phrase
** iPhrase.
*/
static int sqlite3Fts5ExprPoslist(Fts5Expr *pExpr, int iPhrase, const u8 **pa){
  int nRet;
  Fts5ExprPhrase *pPhrase = pExpr->apExprPhrase[iPhrase];
  Fts5ExprNode *pNode = pPhrase->pNode;
  if( pNode->bEof==0 && pNode->iRowid==pExpr->pRoot->iRowid ){
    *pa = pPhrase->poslist.p;
    nRet = pPhrase->poslist.n;
  }else{
    *pa = 0;
    nRet = 0;
  }
  return nRet;
}

struct Fts5PoslistPopulator {
  Fts5PoslistWriter writer;
  int bOk;                        /* True if ok to populate */
  int bMiss;
};

/*
** Clear the position lists associated with all phrases in the expression
** passed as the first argument. Argument bLive is true if the expression
** might be pointing to a real entry, otherwise it has just been reset.
**
** At present this function is only used for detail=col and detail=none
** fts5 tables. This implies that all phrases must be at most 1 token
** in size, as phrase matches are not supported without detail=full.
*/
static Fts5PoslistPopulator *sqlite3Fts5ExprClearPoslists(Fts5Expr *pExpr, int bLive){
  Fts5PoslistPopulator *pRet;
  pRet = sqlite3_malloc64(sizeof(Fts5PoslistPopulator)*pExpr->nPhrase);
  if( pRet ){
    int i;
    memset(pRet, 0, sizeof(Fts5PoslistPopulator)*pExpr->nPhrase);
    for(i=0; i<pExpr->nPhrase; i++){
      Fts5Buffer *pBuf = &pExpr->apExprPhrase[i]->poslist;
      Fts5ExprNode *pNode = pExpr->apExprPhrase[i]->pNode;
      assert( pExpr->apExprPhrase[i]->nTerm<=1 );
      if( bLive &&
          (pBuf->n==0 || pNode->iRowid!=pExpr->pRoot->iRowid || pNode->bEof)
      ){
        pRet[i].bMiss = 1;
      }else{
        pBuf->n = 0;
      }
    }
  }
  return pRet;
}

struct Fts5ExprCtx {
  Fts5Expr *pExpr;
  Fts5PoslistPopulator *aPopulator;
  i64 iOff;
};
typedef struct Fts5ExprCtx Fts5ExprCtx;

/*
** TODO: Make this more efficient!
*/
static int fts5ExprColsetTest(Fts5Colset *pColset, int iCol){
  int i;
  for(i=0; i<pColset->nCol; i++){
    if( pColset->aiCol[i]==iCol ) return 1;
  }
  return 0;
}

/*
** pToken is a buffer nToken bytes in size that may or may not contain
** an embedded 0x00 byte. If it does, return the number of bytes in
** the buffer before the 0x00. If it does not, return nToken.
*/
static int fts5QueryTerm(const char *pToken, int nToken){
  int ii;
  for(ii=0; ii<nToken && pToken[ii]; ii++){}
  return ii;
}

static int fts5ExprPopulatePoslistsCb(
  void *pCtx,                /* Copy of 2nd argument to xTokenize() */
  int tflags,                /* Mask of FTS5_TOKEN_* flags */
  const char *pToken,        /* Pointer to buffer containing token */
  int nToken,                /* Size of token in bytes */
  int iUnused1,              /* Byte offset of token within input text */
  int iUnused2               /* Byte offset of end of token within input text */
){
  Fts5ExprCtx *p = (Fts5ExprCtx*)pCtx;
  Fts5Expr *pExpr = p->pExpr;
  int i;
  int nQuery = nToken;
  i64 iRowid = pExpr->pRoot->iRowid;

  UNUSED_PARAM2(iUnused1, iUnused2);

  if( nQuery>FTS5_MAX_TOKEN_SIZE ) nQuery = FTS5_MAX_TOKEN_SIZE;
  if( pExpr->pConfig->bTokendata ){
    nQuery = fts5QueryTerm(pToken, nQuery);
  }
  if( (tflags & FTS5_TOKEN_COLOCATED)==0 ) p->iOff++;
  for(i=0; i<pExpr->nPhrase; i++){
    Fts5ExprTerm *pT;
    if( p->aPopulator[i].bOk==0 ) continue;
    for(pT=&pExpr->apExprPhrase[i]->aTerm[0]; pT; pT=pT->pSynonym){
      if( (pT->nQueryTerm==nQuery || (pT->nQueryTerm<nQuery && pT->bPrefix))
       && memcmp(pT->pTerm, pToken, pT->nQueryTerm)==0
      ){
        int rc = sqlite3Fts5PoslistWriterAppend(
            &pExpr->apExprPhrase[i]->poslist, &p->aPopulator[i].writer, p->iOff
        );
        if( rc==SQLITE_OK && pExpr->pConfig->bTokendata && !pT->bPrefix ){
          int iCol = p->iOff>>32;
          int iTokOff = p->iOff & 0x7FFFFFFF;
          rc = sqlite3Fts5IndexIterWriteTokendata(
              pT->pIter, pToken, nToken, iRowid, iCol, iTokOff
          );
        }
        if( rc ) return rc;
        break;
      }
    }
  }
  return SQLITE_OK;
}

static int sqlite3Fts5ExprPopulatePoslists(
  Fts5Config *pConfig,
  Fts5Expr *pExpr,
  Fts5PoslistPopulator *aPopulator,
  int iCol,
  const char *z, int n
){
  int i;
  Fts5ExprCtx sCtx;
  sCtx.pExpr = pExpr;
  sCtx.aPopulator = aPopulator;
  sCtx.iOff = (((i64)iCol) << 32) - 1;

  for(i=0; i<pExpr->nPhrase; i++){
    Fts5ExprNode *pNode = pExpr->apExprPhrase[i]->pNode;
    Fts5Colset *pColset = pNode->pNear->pColset;
    if( (pColset && 0==fts5ExprColsetTest(pColset, iCol))
     || aPopulator[i].bMiss
    ){
      aPopulator[i].bOk = 0;
    }else{
      aPopulator[i].bOk = 1;
    }
  }

  return sqlite3Fts5Tokenize(pConfig,
      FTS5_TOKENIZE_DOCUMENT, z, n, (void*)&sCtx, fts5ExprPopulatePoslistsCb
  );
}

static void fts5ExprClearPoslists(Fts5ExprNode *pNode){
  if( pNode->eType==FTS5_TERM || pNode->eType==FTS5_STRING ){
    pNode->pNear->apPhrase[0]->poslist.n = 0;
  }else{
    int i;
    for(i=0; i<pNode->nChild; i++){
      fts5ExprClearPoslists(pNode->apChild[i]);
    }
  }
}

static int fts5ExprCheckPoslists(Fts5ExprNode *pNode, i64 iRowid){
  pNode->iRowid = iRowid;
  pNode->bEof = 0;
  switch( pNode->eType ){
    case FTS5_TERM:
    case FTS5_STRING:
      return (pNode->pNear->apPhrase[0]->poslist.n>0);

    case FTS5_AND: {
      int i;
      for(i=0; i<pNode->nChild; i++){
        if( fts5ExprCheckPoslists(pNode->apChild[i], iRowid)==0 ){
          fts5ExprClearPoslists(pNode);
          return 0;
        }
      }
      break;
    }

    case FTS5_OR: {
      int i;
      int bRet = 0;
      for(i=0; i<pNode->nChild; i++){
        if( fts5ExprCheckPoslists(pNode->apChild[i], iRowid) ){
          bRet = 1;
        }
      }
      return bRet;
    }

    default: {
      assert( pNode->eType==FTS5_NOT );
      if( 0==fts5ExprCheckPoslists(pNode->apChild[0], iRowid)
          || 0!=fts5ExprCheckPoslists(pNode->apChild[1], iRowid)
        ){
        fts5ExprClearPoslists(pNode);
        return 0;
      }
      break;
    }
  }
  return 1;
}

static void sqlite3Fts5ExprCheckPoslists(Fts5Expr *pExpr, i64 iRowid){
  fts5ExprCheckPoslists(pExpr->pRoot, iRowid);
}

/*
** This function is only called for detail=columns tables.
*/
static int sqlite3Fts5ExprPhraseCollist(
  Fts5Expr *pExpr,
  int iPhrase,
  const u8 **ppCollist,
  int *pnCollist
){
  Fts5ExprPhrase *pPhrase = pExpr->apExprPhrase[iPhrase];
  Fts5ExprNode *pNode = pPhrase->pNode;
  int rc = SQLITE_OK;

  assert( iPhrase>=0 && iPhrase<pExpr->nPhrase );
  assert( pExpr->pConfig->eDetail==FTS5_DETAIL_COLUMNS );

  if( pNode->bEof==0
   && pNode->iRowid==pExpr->pRoot->iRowid
   && pPhrase->poslist.n>0
  ){
    Fts5ExprTerm *pTerm = &pPhrase->aTerm[0];
    if( pTerm->pSynonym ){
      Fts5Buffer *pBuf = (Fts5Buffer*)&pTerm->pSynonym[1];
      rc = fts5ExprSynonymList(
          pTerm, pNode->iRowid, pBuf, (u8**)ppCollist, pnCollist
      );
    }else{
      *ppCollist = pPhrase->aTerm[0].pIter->pData;
      *pnCollist = pPhrase->aTerm[0].pIter->nData;
    }
  }else{
    *ppCollist = 0;
    *pnCollist = 0;
  }

  return rc;
}
