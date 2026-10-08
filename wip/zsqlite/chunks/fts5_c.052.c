
static void fts5ApiPhraseNext(
  Fts5Context *pUnused,
  Fts5PhraseIter *pIter,
  int *piCol, int *piOff
){
  UNUSED_PARAM(pUnused);
  if( pIter->a>=pIter->b ){
    *piCol = -1;
    *piOff = -1;
  }else{
    int iVal;
    pIter->a += fts5GetVarint32(pIter->a, iVal);
    if( iVal==1 ){
      pIter->a += fts5GetVarint32(pIter->a, iVal);
      *piCol = iVal;
      *piOff = 0;
      pIter->a += fts5GetVarint32(pIter->a, iVal);
    }
    *piOff += (iVal-2);
  }
}

static int fts5ApiPhraseFirst(
  Fts5Context *pCtx,
  int iPhrase,
  Fts5PhraseIter *pIter,
  int *piCol, int *piOff
){
  Fts5Cursor *pCsr = (Fts5Cursor*)pCtx;
  int n;
  int rc = fts5CsrPoslist(pCsr, iPhrase, &pIter->a, &n);
  if( rc==SQLITE_OK ){
    assert( pIter->a || n==0 );
    pIter->b = (pIter->a ? &pIter->a[n] : 0);
    *piCol = 0;
    *piOff = 0;
    fts5ApiPhraseNext(pCtx, pIter, piCol, piOff);
  }
  return rc;
}

static void fts5ApiPhraseNextColumn(
  Fts5Context *pCtx,
  Fts5PhraseIter *pIter,
  int *piCol
){
  Fts5Cursor *pCsr = (Fts5Cursor*)pCtx;
  Fts5Config *pConfig = ((Fts5Table*)(pCsr->base.pVtab))->pConfig;

  if( pConfig->eDetail==FTS5_DETAIL_COLUMNS ){
    if( pIter->a>=pIter->b ){
      *piCol = -1;
    }else{
      int iIncr;
      pIter->a += fts5GetVarint32(&pIter->a[0], iIncr);
      *piCol += (iIncr-2);
    }
  }else{
    while( 1 ){
      int dummy;
      if( pIter->a>=pIter->b ){
        *piCol = -1;
        return;
      }
      if( pIter->a[0]==0x01 ) break;
      pIter->a += fts5GetVarint32(pIter->a, dummy);
    }
    pIter->a += 1 + fts5GetVarint32(&pIter->a[1], *piCol);
  }
}

static int fts5ApiPhraseFirstColumn(
  Fts5Context *pCtx,
  int iPhrase,
  Fts5PhraseIter *pIter,
  int *piCol
){
  int rc = SQLITE_OK;
  Fts5Cursor *pCsr = (Fts5Cursor*)pCtx;
  Fts5Config *pConfig = ((Fts5Table*)(pCsr->base.pVtab))->pConfig;

  if( pConfig->eDetail==FTS5_DETAIL_COLUMNS ){
    Fts5Sorter *pSorter = pCsr->pSorter;
    int n;
    if( pSorter ){
      int i1 = (iPhrase==0 ? 0 : pSorter->aIdx[iPhrase-1]);
      n = pSorter->aIdx[iPhrase] - i1;
      pIter->a = &pSorter->aPoslist[i1];
    }else{
      rc = sqlite3Fts5ExprPhraseCollist(pCsr->pExpr, iPhrase, &pIter->a, &n);
    }
    if( rc==SQLITE_OK ){
      assert( pIter->a || n==0 );
      pIter->b = (pIter->a ? &pIter->a[n] : 0);
      *piCol = 0;
      fts5ApiPhraseNextColumn(pCtx, pIter, piCol);
    }
  }else{
    int n;
    rc = fts5CsrPoslist(pCsr, iPhrase, &pIter->a, &n);
    if( rc==SQLITE_OK ){
      assert( pIter->a || n==0 );
      pIter->b = (pIter->a ? &pIter->a[n] : 0);
      if( n<=0 ){
        *piCol = -1;
      }else if( pIter->a[0]==0x01 ){
        pIter->a += 1 + fts5GetVarint32(&pIter->a[1], *piCol);
      }else{
        *piCol = 0;
      }
    }
  }

  return rc;
}

/*
** xQueryToken() API implemenetation.
*/
static int fts5ApiQueryToken(
  Fts5Context* pCtx,
  int iPhrase,
  int iToken,
  const char **ppOut,
  int *pnOut
){
  Fts5Cursor *pCsr = (Fts5Cursor*)pCtx;
  return sqlite3Fts5ExprQueryToken(pCsr->pExpr, iPhrase, iToken, ppOut, pnOut);
}

/*
** xInstToken() API implemenetation.
*/
static int fts5ApiInstToken(
  Fts5Context *pCtx,
  int iIdx,
  int iToken,
  const char **ppOut, int *pnOut
){
  Fts5Cursor *pCsr = (Fts5Cursor*)pCtx;
  int rc = SQLITE_OK;
  if( CsrFlagTest(pCsr, FTS5CSR_REQUIRE_INST)==0
   || SQLITE_OK==(rc = fts5CacheInstArray(pCsr))
  ){
    if( iIdx<0 || iIdx>=pCsr->nInstCount ){
      rc = SQLITE_RANGE;
    }else{
      int iPhrase = pCsr->aInst[iIdx*3];
      int iCol = pCsr->aInst[iIdx*3 + 1];
      int iOff = pCsr->aInst[iIdx*3 + 2];
      i64 iRowid = fts5CursorRowid(pCsr);
      rc = sqlite3Fts5ExprInstToken(
          pCsr->pExpr, iRowid, iPhrase, iCol, iOff, iToken, ppOut, pnOut
      );
    }
  }
  return rc;
}


static int fts5ApiQueryPhrase(Fts5Context*, int, void*,
    int(*)(const Fts5ExtensionApi*, Fts5Context*, void*)
);

static const Fts5ExtensionApi sFts5Api = {
  3,                            /* iVersion */
  fts5ApiUserData,
  fts5ApiColumnCount,
  fts5ApiRowCount,
  fts5ApiColumnTotalSize,
  fts5ApiTokenize,
  fts5ApiPhraseCount,
  fts5ApiPhraseSize,
  fts5ApiInstCount,
  fts5ApiInst,
  fts5ApiRowid,
  fts5ApiColumnText,
  fts5ApiColumnSize,
  fts5ApiQueryPhrase,
  fts5ApiSetAuxdata,
  fts5ApiGetAuxdata,
  fts5ApiPhraseFirst,
  fts5ApiPhraseNext,
  fts5ApiPhraseFirstColumn,
  fts5ApiPhraseNextColumn,
  fts5ApiQueryToken,
  fts5ApiInstToken
};

/*
** Implementation of API function xQueryPhrase().
*/
static int fts5ApiQueryPhrase(
  Fts5Context *pCtx,
  int iPhrase,
  void *pUserData,
  int(*xCallback)(const Fts5ExtensionApi*, Fts5Context*, void*)
){
  Fts5Cursor *pCsr = (Fts5Cursor*)pCtx;
  Fts5FullTable *pTab = (Fts5FullTable*)(pCsr->base.pVtab);
  int rc;
  Fts5Cursor *pNew = 0;

  rc = fts5OpenMethod(pCsr->base.pVtab, (sqlite3_vtab_cursor**)&pNew);
  if( rc==SQLITE_OK ){
    pNew->ePlan = FTS5_PLAN_MATCH;
    pNew->iFirstRowid = SMALLEST_INT64;
    pNew->iLastRowid = LARGEST_INT64;
    pNew->base.pVtab = (sqlite3_vtab*)pTab;
    rc = sqlite3Fts5ExprClonePhrase(pCsr->pExpr, iPhrase, &pNew->pExpr);
  }

  if( rc==SQLITE_OK ){
    for(rc = fts5CursorFirst(pTab, pNew, 0);
        rc==SQLITE_OK && CsrFlagTest(pNew, FTS5CSR_EOF)==0;
        rc = fts5NextMethod((sqlite3_vtab_cursor*)pNew)
    ){
      rc = xCallback(&sFts5Api, (Fts5Context*)pNew, pUserData);
      if( rc!=SQLITE_OK ){
        if( rc==SQLITE_DONE ) rc = SQLITE_OK;
        break;
      }
    }
  }

  fts5CloseMethod((sqlite3_vtab_cursor*)pNew);
  return rc;
}

static void fts5ApiInvoke(
  Fts5Auxiliary *pAux,
  Fts5Cursor *pCsr,
  sqlite3_context *context,
  int argc,
  sqlite3_value **argv
){
  assert( pCsr->pAux==0 );
  pCsr->pAux = pAux;
  pAux->xFunc(&sFts5Api, (Fts5Context*)pCsr, context, argc, argv);
  pCsr->pAux = 0;
}

static Fts5Cursor *fts5CursorFromCsrid(Fts5Global *pGlobal, i64 iCsrId){
  Fts5Cursor *pCsr;
  for(pCsr=pGlobal->pCsr; pCsr; pCsr=pCsr->pNext){
    if( pCsr->iCsrId==iCsrId ) break;
  }
  return pCsr;
}

static void fts5ApiCallback(
  sqlite3_context *context,
  int argc,
  sqlite3_value **argv
){

  Fts5Auxiliary *pAux;
  Fts5Cursor *pCsr;
  i64 iCsrId;

  assert( argc>=1 );
  pAux = (Fts5Auxiliary*)sqlite3_user_data(context);
  iCsrId = sqlite3_value_int64(argv[0]);

  pCsr = fts5CursorFromCsrid(pAux->pGlobal, iCsrId);
  if( pCsr==0 || pCsr->ePlan==0 ){
    char *zErr = sqlite3_mprintf("no such cursor: %lld", iCsrId);
    sqlite3_result_error(context, zErr, -1);
    sqlite3_free(zErr);
  }else{
    fts5ApiInvoke(pAux, pCsr, context, argc-1, &argv[1]);
  }
}


/*
** Given cursor id iId, return a pointer to the corresponding Fts5Table
** object. Or NULL If the cursor id does not exist.
*/
static Fts5Table *sqlite3Fts5TableFromCsrid(
  Fts5Global *pGlobal,            /* FTS5 global context for db handle */
  i64 iCsrId                      /* Id of cursor to find */
){
  Fts5Cursor *pCsr;
  pCsr = fts5CursorFromCsrid(pGlobal, iCsrId);
  if( pCsr ){
    return (Fts5Table*)pCsr->base.pVtab;
  }
  return 0;
}

/*
** Return a "position-list blob" corresponding to the current position of
** cursor pCsr via sqlite3_result_blob(). A position-list blob contains
** the current position-list for each phrase in the query associated with
** cursor pCsr.
**
** A position-list blob begins with (nPhrase-1) varints, where nPhrase is
** the number of phrases in the query. Following the varints are the
** concatenated position lists for each phrase, in order.
**
** The first varint (if it exists) contains the size of the position list
** for phrase 0. The second (same disclaimer) contains the size of position
** list 1. And so on. There is no size field for the final position list,
** as it can be derived from the total size of the blob.
*/
static int fts5PoslistBlob(sqlite3_context *pCtx, Fts5Cursor *pCsr){
  int i;
  int rc = SQLITE_OK;
  int nPhrase = sqlite3Fts5ExprPhraseCount(pCsr->pExpr);
  Fts5Buffer val;

  memset(&val, 0, sizeof(Fts5Buffer));
  switch( ((Fts5Table*)(pCsr->base.pVtab))->pConfig->eDetail ){
    case FTS5_DETAIL_FULL:

      /* Append the varints */
      for(i=0; i<(nPhrase-1); i++){
        const u8 *dummy;
        int nByte = sqlite3Fts5ExprPoslist(pCsr->pExpr, i, &dummy);
        sqlite3Fts5BufferAppendVarint(&rc, &val, nByte);
      }

      /* Append the position lists */
      for(i=0; i<nPhrase; i++){
        const u8 *pPoslist;
        int nPoslist;
        nPoslist = sqlite3Fts5ExprPoslist(pCsr->pExpr, i, &pPoslist);
        sqlite3Fts5BufferAppendBlob(&rc, &val, nPoslist, pPoslist);
      }
      break;

    case FTS5_DETAIL_COLUMNS:

      /* Append the varints */
      for(i=0; rc==SQLITE_OK && i<(nPhrase-1); i++){
        const u8 *dummy;
        int nByte;
        rc = sqlite3Fts5ExprPhraseCollist(pCsr->pExpr, i, &dummy, &nByte);
        sqlite3Fts5BufferAppendVarint(&rc, &val, nByte);
      }

      /* Append the position lists */
      for(i=0; rc==SQLITE_OK && i<nPhrase; i++){
        const u8 *pPoslist;
        int nPoslist;
        rc = sqlite3Fts5ExprPhraseCollist(pCsr->pExpr, i, &pPoslist, &nPoslist);
        sqlite3Fts5BufferAppendBlob(&rc, &val, nPoslist, pPoslist);
      }
      break;

    default:
      break;
  }

  sqlite3_result_blob(pCtx, val.p, val.n, sqlite3_free);
  return rc;
}
