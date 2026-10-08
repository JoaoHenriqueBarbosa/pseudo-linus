
static int fts5ApiColumnTotalSize(
  Fts5Context *pCtx,
  int iCol,
  sqlite3_int64 *pnToken
){
  Fts5Cursor *pCsr = (Fts5Cursor*)pCtx;
  Fts5FullTable *pTab = (Fts5FullTable*)(pCsr->base.pVtab);
  return sqlite3Fts5StorageSize(pTab->pStorage, iCol, pnToken);
}

static int fts5ApiRowCount(Fts5Context *pCtx, i64 *pnRow){
  Fts5Cursor *pCsr = (Fts5Cursor*)pCtx;
  Fts5FullTable *pTab = (Fts5FullTable*)(pCsr->base.pVtab);
  return sqlite3Fts5StorageRowCount(pTab->pStorage, pnRow);
}

static int fts5ApiTokenize(
  Fts5Context *pCtx,
  const char *pText, int nText,
  void *pUserData,
  int (*xToken)(void*, int, const char*, int, int, int)
){
  Fts5Cursor *pCsr = (Fts5Cursor*)pCtx;
  Fts5Table *pTab = (Fts5Table*)(pCsr->base.pVtab);
  return sqlite3Fts5Tokenize(
      pTab->pConfig, FTS5_TOKENIZE_AUX, pText, nText, pUserData, xToken
  );
}

static int fts5ApiPhraseCount(Fts5Context *pCtx){
  Fts5Cursor *pCsr = (Fts5Cursor*)pCtx;
  return sqlite3Fts5ExprPhraseCount(pCsr->pExpr);
}

static int fts5ApiPhraseSize(Fts5Context *pCtx, int iPhrase){
  Fts5Cursor *pCsr = (Fts5Cursor*)pCtx;
  return sqlite3Fts5ExprPhraseSize(pCsr->pExpr, iPhrase);
}

static int fts5ApiColumnText(
  Fts5Context *pCtx,
  int iCol,
  const char **pz,
  int *pn
){
  int rc = SQLITE_OK;
  Fts5Cursor *pCsr = (Fts5Cursor*)pCtx;
  Fts5Table *pTab = (Fts5Table*)(pCsr->base.pVtab);
  if( iCol<0 || iCol>=pTab->pConfig->nCol ){
    rc = SQLITE_RANGE;
  }else if( fts5IsContentless((Fts5FullTable*)(pCsr->base.pVtab))
   || pCsr->ePlan==FTS5_PLAN_SPECIAL
  ){
    *pz = 0;
    *pn = 0;
  }else{
    rc = fts5SeekCursor(pCsr, 0);
    if( rc==SQLITE_OK ){
      *pz = (const char*)sqlite3_column_text(pCsr->pStmt, iCol+1);
      *pn = sqlite3_column_bytes(pCsr->pStmt, iCol+1);
    }
  }
  return rc;
}

static int fts5CsrPoslist(
  Fts5Cursor *pCsr,
  int iPhrase,
  const u8 **pa,
  int *pn
){
  Fts5Config *pConfig = ((Fts5Table*)(pCsr->base.pVtab))->pConfig;
  int rc = SQLITE_OK;
  int bLive = (pCsr->pSorter==0);

  if( iPhrase<0 || iPhrase>=sqlite3Fts5ExprPhraseCount(pCsr->pExpr) ){
    rc = SQLITE_RANGE;
  }else if( CsrFlagTest(pCsr, FTS5CSR_REQUIRE_POSLIST) ){
    if( pConfig->eDetail!=FTS5_DETAIL_FULL ){
      Fts5PoslistPopulator *aPopulator;
      int i;
      aPopulator = sqlite3Fts5ExprClearPoslists(pCsr->pExpr, bLive);
      if( aPopulator==0 ) rc = SQLITE_NOMEM;
      for(i=0; i<pConfig->nCol && rc==SQLITE_OK; i++){
        int n; const char *z;
        rc = fts5ApiColumnText((Fts5Context*)pCsr, i, &z, &n);
        if( rc==SQLITE_OK ){
          rc = sqlite3Fts5ExprPopulatePoslists(
              pConfig, pCsr->pExpr, aPopulator, i, z, n
          );
        }
      }
      sqlite3_free(aPopulator);

      if( pCsr->pSorter ){
        sqlite3Fts5ExprCheckPoslists(pCsr->pExpr, pCsr->pSorter->iRowid);
      }
    }
    CsrFlagClear(pCsr, FTS5CSR_REQUIRE_POSLIST);
  }

  if( rc==SQLITE_OK ){
    if( pCsr->pSorter && pConfig->eDetail==FTS5_DETAIL_FULL ){
      Fts5Sorter *pSorter = pCsr->pSorter;
      int i1 = (iPhrase==0 ? 0 : pSorter->aIdx[iPhrase-1]);
      *pn = pSorter->aIdx[iPhrase] - i1;
      *pa = &pSorter->aPoslist[i1];
    }else{
      *pn = sqlite3Fts5ExprPoslist(pCsr->pExpr, iPhrase, pa);
    }
  }else{
    *pa = 0;
    *pn = 0;
  }


  return rc;
}

/*
** Ensure that the Fts5Cursor.nInstCount and aInst[] variables are populated
** correctly for the current view. Return SQLITE_OK if successful, or an
** SQLite error code otherwise.
*/
static int fts5CacheInstArray(Fts5Cursor *pCsr){
  int rc = SQLITE_OK;
  Fts5PoslistReader *aIter;       /* One iterator for each phrase */
  int nIter;                      /* Number of iterators/phrases */
  int nCol = ((Fts5Table*)pCsr->base.pVtab)->pConfig->nCol;

  nIter = sqlite3Fts5ExprPhraseCount(pCsr->pExpr);
  if( pCsr->aInstIter==0 ){
    sqlite3_int64 nByte = sizeof(Fts5PoslistReader) * nIter;
    pCsr->aInstIter = (Fts5PoslistReader*)sqlite3Fts5MallocZero(&rc, nByte);
  }
  aIter = pCsr->aInstIter;

  if( aIter ){
    int nInst = 0;                /* Number instances seen so far */
    int i;

    /* Initialize all iterators */
    for(i=0; i<nIter && rc==SQLITE_OK; i++){
      const u8 *a;
      int n;
      rc = fts5CsrPoslist(pCsr, i, &a, &n);
      if( rc==SQLITE_OK ){
        sqlite3Fts5PoslistReaderInit(a, n, &aIter[i]);
      }
    }

    if( rc==SQLITE_OK ){
      while( 1 ){
        int *aInst;
        int iBest = -1;
        for(i=0; i<nIter; i++){
          if( (aIter[i].bEof==0)
              && (iBest<0 || aIter[i].iPos<aIter[iBest].iPos)
            ){
            iBest = i;
          }
        }
        if( iBest<0 ) break;

        nInst++;
        if( nInst>=pCsr->nInstAlloc ){
          int nNewSize = pCsr->nInstAlloc ? pCsr->nInstAlloc*2 : 32;
          aInst = (int*)sqlite3_realloc64(
              pCsr->aInst, nNewSize*sizeof(int)*3
              );
          if( aInst ){
            pCsr->aInst = aInst;
            pCsr->nInstAlloc = nNewSize;
          }else{
            nInst--;
            rc = SQLITE_NOMEM;
            break;
          }
        }

        aInst = &pCsr->aInst[3 * (nInst-1)];
        aInst[0] = iBest;
        aInst[1] = FTS5_POS2COLUMN(aIter[iBest].iPos);
        aInst[2] = FTS5_POS2OFFSET(aIter[iBest].iPos);
        if( aInst[1]<0 || aInst[1]>=nCol ){
          rc = FTS5_CORRUPT;
          break;
        }
        sqlite3Fts5PoslistReaderNext(&aIter[iBest]);
      }
    }

    pCsr->nInstCount = nInst;
    CsrFlagClear(pCsr, FTS5CSR_REQUIRE_INST);
  }
  return rc;
}

static int fts5ApiInstCount(Fts5Context *pCtx, int *pnInst){
  Fts5Cursor *pCsr = (Fts5Cursor*)pCtx;
  int rc = SQLITE_OK;
  if( CsrFlagTest(pCsr, FTS5CSR_REQUIRE_INST)==0
   || SQLITE_OK==(rc = fts5CacheInstArray(pCsr)) ){
    *pnInst = pCsr->nInstCount;
  }
  return rc;
}

static int fts5ApiInst(
  Fts5Context *pCtx,
  int iIdx,
  int *piPhrase,
  int *piCol,
  int *piOff
){
  Fts5Cursor *pCsr = (Fts5Cursor*)pCtx;
  int rc = SQLITE_OK;
  if( CsrFlagTest(pCsr, FTS5CSR_REQUIRE_INST)==0
   || SQLITE_OK==(rc = fts5CacheInstArray(pCsr))
  ){
    if( iIdx<0 || iIdx>=pCsr->nInstCount ){
      rc = SQLITE_RANGE;
    }else{
      *piPhrase = pCsr->aInst[iIdx*3];
      *piCol = pCsr->aInst[iIdx*3 + 1];
      *piOff = pCsr->aInst[iIdx*3 + 2];
    }
  }
  return rc;
}

static sqlite3_int64 fts5ApiRowid(Fts5Context *pCtx){
  return fts5CursorRowid((Fts5Cursor*)pCtx);
}

static int fts5ColumnSizeCb(
  void *pContext,                 /* Pointer to int */
  int tflags,
  const char *pUnused,            /* Buffer containing token */
  int nUnused,                    /* Size of token in bytes */
  int iUnused1,                   /* Start offset of token */
  int iUnused2                    /* End offset of token */
){
  int *pCnt = (int*)pContext;
  UNUSED_PARAM2(pUnused, nUnused);
  UNUSED_PARAM2(iUnused1, iUnused2);
  if( (tflags & FTS5_TOKEN_COLOCATED)==0 ){
    (*pCnt)++;
  }
  return SQLITE_OK;
}

static int fts5ApiColumnSize(Fts5Context *pCtx, int iCol, int *pnToken){
  Fts5Cursor *pCsr = (Fts5Cursor*)pCtx;
  Fts5FullTable *pTab = (Fts5FullTable*)(pCsr->base.pVtab);
  Fts5Config *pConfig = pTab->p.pConfig;
  int rc = SQLITE_OK;

  if( CsrFlagTest(pCsr, FTS5CSR_REQUIRE_DOCSIZE) ){
    if( pConfig->bColumnsize ){
      i64 iRowid = fts5CursorRowid(pCsr);
      rc = sqlite3Fts5StorageDocsize(pTab->pStorage, iRowid, pCsr->aColumnSize);
    }else if( pConfig->zContent==0 ){
      int i;
      for(i=0; i<pConfig->nCol; i++){
        if( pConfig->abUnindexed[i]==0 ){
          pCsr->aColumnSize[i] = -1;
        }
      }
    }else{
      int i;
      for(i=0; rc==SQLITE_OK && i<pConfig->nCol; i++){
        if( pConfig->abUnindexed[i]==0 ){
          const char *z; int n;
          void *p = (void*)(&pCsr->aColumnSize[i]);
          pCsr->aColumnSize[i] = 0;
          rc = fts5ApiColumnText(pCtx, i, &z, &n);
          if( rc==SQLITE_OK ){
            rc = sqlite3Fts5Tokenize(
                pConfig, FTS5_TOKENIZE_AUX, z, n, p, fts5ColumnSizeCb
            );
          }
        }
      }
    }
    CsrFlagClear(pCsr, FTS5CSR_REQUIRE_DOCSIZE);
  }
  if( iCol<0 ){
    int i;
    *pnToken = 0;
    for(i=0; i<pConfig->nCol; i++){
      *pnToken += pCsr->aColumnSize[i];
    }
  }else if( iCol<pConfig->nCol ){
    *pnToken = pCsr->aColumnSize[iCol];
  }else{
    *pnToken = 0;
    rc = SQLITE_RANGE;
  }
  return rc;
}

/*
** Implementation of the xSetAuxdata() method.
*/
static int fts5ApiSetAuxdata(
  Fts5Context *pCtx,              /* Fts5 context */
  void *pPtr,                     /* Pointer to save as auxdata */
  void(*xDelete)(void*)           /* Destructor for pPtr (or NULL) */
){
  Fts5Cursor *pCsr = (Fts5Cursor*)pCtx;
  Fts5Auxdata *pData;

  /* Search through the cursors list of Fts5Auxdata objects for one that
  ** corresponds to the currently executing auxiliary function.  */
  for(pData=pCsr->pAuxdata; pData; pData=pData->pNext){
    if( pData->pAux==pCsr->pAux ) break;
  }

  if( pData ){
    if( pData->xDelete ){
      pData->xDelete(pData->pPtr);
    }
  }else{
    int rc = SQLITE_OK;
    pData = (Fts5Auxdata*)sqlite3Fts5MallocZero(&rc, sizeof(Fts5Auxdata));
    if( pData==0 ){
      if( xDelete ) xDelete(pPtr);
      return rc;
    }
    pData->pAux = pCsr->pAux;
    pData->pNext = pCsr->pAuxdata;
    pCsr->pAuxdata = pData;
  }

  pData->xDelete = xDelete;
  pData->pPtr = pPtr;
  return SQLITE_OK;
}

static void *fts5ApiGetAuxdata(Fts5Context *pCtx, int bClear){
  Fts5Cursor *pCsr = (Fts5Cursor*)pCtx;
  Fts5Auxdata *pData;
  void *pRet = 0;

  for(pData=pCsr->pAuxdata; pData; pData=pData->pNext){
    if( pData->pAux==pCsr->pAux ) break;
  }

  if( pData ){
    pRet = pData->pPtr;
    if( bClear ){
      pData->pPtr = 0;
      pData->xDelete = 0;
    }
  }

  return pRet;
}
