
/*
** This is the xColumn method, called by SQLite to request a value from
** the row that the supplied cursor currently points to.
*/
static int fts5ColumnMethod(
  sqlite3_vtab_cursor *pCursor,   /* Cursor to retrieve value from */
  sqlite3_context *pCtx,          /* Context for sqlite3_result_xxx() calls */
  int iCol                        /* Index of column to read value from */
){
  Fts5FullTable *pTab = (Fts5FullTable*)(pCursor->pVtab);
  Fts5Config *pConfig = pTab->p.pConfig;
  Fts5Cursor *pCsr = (Fts5Cursor*)pCursor;
  int rc = SQLITE_OK;

  assert( CsrFlagTest(pCsr, FTS5CSR_EOF)==0 );

  if( pCsr->ePlan==FTS5_PLAN_SPECIAL ){
    if( iCol==pConfig->nCol ){
      sqlite3_result_int64(pCtx, pCsr->iSpecial);
    }
  }else

  if( iCol==pConfig->nCol ){
    /* User is requesting the value of the special column with the same name
    ** as the table. Return the cursor integer id number. This value is only
    ** useful in that it may be passed as the first argument to an FTS5
    ** auxiliary function.  */
    sqlite3_result_int64(pCtx, pCsr->iCsrId);
  }else if( iCol==pConfig->nCol+1 ){

    /* The value of the "rank" column. */
    if( pCsr->ePlan==FTS5_PLAN_SOURCE ){
      fts5PoslistBlob(pCtx, pCsr);
    }else if(
        pCsr->ePlan==FTS5_PLAN_MATCH
     || pCsr->ePlan==FTS5_PLAN_SORTED_MATCH
    ){
      if( pCsr->pRank || SQLITE_OK==(rc = fts5FindRankFunction(pCsr)) ){
        fts5ApiInvoke(pCsr->pRank, pCsr, pCtx, pCsr->nRankArg, pCsr->apRankArg);
      }
    }
  }else if( !fts5IsContentless(pTab) ){
    pConfig->pzErrmsg = &pTab->p.base.zErrMsg;
    rc = fts5SeekCursor(pCsr, 1);
    if( rc==SQLITE_OK ){
      sqlite3_result_value(pCtx, sqlite3_column_value(pCsr->pStmt, iCol+1));
    }
    pConfig->pzErrmsg = 0;
  }else if( pConfig->bContentlessDelete && sqlite3_vtab_nochange(pCtx) ){
    char *zErr = sqlite3_mprintf("cannot UPDATE a subset of "
        "columns on fts5 contentless-delete table: %s", pConfig->zName
    );
    sqlite3_result_error(pCtx, zErr, -1);
    sqlite3_free(zErr);
  }
  return rc;
}


/*
** This routine implements the xFindFunction method for the FTS3
** virtual table.
*/
static int fts5FindFunctionMethod(
  sqlite3_vtab *pVtab,            /* Virtual table handle */
  int nUnused,                    /* Number of SQL function arguments */
  const char *zName,              /* Name of SQL function */
  void (**pxFunc)(sqlite3_context*,int,sqlite3_value**), /* OUT: Result */
  void **ppArg                    /* OUT: User data for *pxFunc */
){
  Fts5FullTable *pTab = (Fts5FullTable*)pVtab;
  Fts5Auxiliary *pAux;

  UNUSED_PARAM(nUnused);
  pAux = fts5FindAuxiliary(pTab, zName);
  if( pAux ){
    *pxFunc = fts5ApiCallback;
    *ppArg = (void*)pAux;
    return 1;
  }

  /* No function of the specified name was found. Return 0. */
  return 0;
}

/*
** Implementation of FTS5 xRename method. Rename an fts5 table.
*/
static int fts5RenameMethod(
  sqlite3_vtab *pVtab,            /* Virtual table handle */
  const char *zName               /* New name of table */
){
  int rc;
  Fts5FullTable *pTab = (Fts5FullTable*)pVtab;
  rc = sqlite3Fts5StorageRename(pTab->pStorage, zName);
  return rc;
}

static int sqlite3Fts5FlushToDisk(Fts5Table *pTab){
  fts5TripCursors((Fts5FullTable*)pTab);
  return sqlite3Fts5StorageSync(((Fts5FullTable*)pTab)->pStorage);
}

/*
** The xSavepoint() method.
**
** Flush the contents of the pending-terms table to disk.
*/
static int fts5SavepointMethod(sqlite3_vtab *pVtab, int iSavepoint){
  Fts5FullTable *pTab = (Fts5FullTable*)pVtab;
  int rc = SQLITE_OK;

  fts5CheckTransactionState(pTab, FTS5_SAVEPOINT, iSavepoint);
  rc = sqlite3Fts5FlushToDisk((Fts5Table*)pVtab);
  if( rc==SQLITE_OK ){
    pTab->iSavepoint = iSavepoint+1;
  }
  return rc;
}

/*
** The xRelease() method.
**
** This is a no-op.
*/
static int fts5ReleaseMethod(sqlite3_vtab *pVtab, int iSavepoint){
  Fts5FullTable *pTab = (Fts5FullTable*)pVtab;
  int rc = SQLITE_OK;
  fts5CheckTransactionState(pTab, FTS5_RELEASE, iSavepoint);
  if( (iSavepoint+1)<pTab->iSavepoint ){
    rc = sqlite3Fts5FlushToDisk(&pTab->p);
    if( rc==SQLITE_OK ){
      pTab->iSavepoint = iSavepoint;
    }
  }
  return rc;
}

/*
** The xRollbackTo() method.
**
** Discard the contents of the pending terms table.
*/
static int fts5RollbackToMethod(sqlite3_vtab *pVtab, int iSavepoint){
  Fts5FullTable *pTab = (Fts5FullTable*)pVtab;
  int rc = SQLITE_OK;
  fts5CheckTransactionState(pTab, FTS5_ROLLBACKTO, iSavepoint);
  fts5TripCursors(pTab);
  if( (iSavepoint+1)<=pTab->iSavepoint ){
    pTab->p.pConfig->pgsz = 0;
    rc = sqlite3Fts5StorageRollback(pTab->pStorage);
  }
  return rc;
}

/*
** Register a new auxiliary function with global context pGlobal.
*/
static int fts5CreateAux(
  fts5_api *pApi,                 /* Global context (one per db handle) */
  const char *zName,              /* Name of new function */
  void *pUserData,                /* User data for aux. function */
  fts5_extension_function xFunc,  /* Aux. function implementation */
  void(*xDestroy)(void*)          /* Destructor for pUserData */
){
  Fts5Global *pGlobal = (Fts5Global*)pApi;
  int rc = sqlite3_overload_function(pGlobal->db, zName, -1);
  if( rc==SQLITE_OK ){
    Fts5Auxiliary *pAux;
    sqlite3_int64 nName;            /* Size of zName in bytes, including \0 */
    sqlite3_int64 nByte;            /* Bytes of space to allocate */

    nName = strlen(zName) + 1;
    nByte = sizeof(Fts5Auxiliary) + nName;
    pAux = (Fts5Auxiliary*)sqlite3_malloc64(nByte);
    if( pAux ){
      memset(pAux, 0, (size_t)nByte);
      pAux->zFunc = (char*)&pAux[1];
      memcpy(pAux->zFunc, zName, nName);
      pAux->pGlobal = pGlobal;
      pAux->pUserData = pUserData;
      pAux->xFunc = xFunc;
      pAux->xDestroy = xDestroy;
      pAux->pNext = pGlobal->pAux;
      pGlobal->pAux = pAux;
    }else{
      rc = SQLITE_NOMEM;
    }
  }

  return rc;
}

/*
** Register a new tokenizer. This is the implementation of the
** fts5_api.xCreateTokenizer() method.
*/
static int fts5CreateTokenizer(
  fts5_api *pApi,                 /* Global context (one per db handle) */
  const char *zName,              /* Name of new function */
  void *pUserData,                /* User data for aux. function */
  fts5_tokenizer *pTokenizer,     /* Tokenizer implementation */
  void(*xDestroy)(void*)          /* Destructor for pUserData */
){
  Fts5Global *pGlobal = (Fts5Global*)pApi;
  Fts5TokenizerModule *pNew;
  sqlite3_int64 nName;            /* Size of zName and its \0 terminator */
  sqlite3_int64 nByte;            /* Bytes of space to allocate */
  int rc = SQLITE_OK;

  nName = strlen(zName) + 1;
  nByte = sizeof(Fts5TokenizerModule) + nName;
  pNew = (Fts5TokenizerModule*)sqlite3_malloc64(nByte);
  if( pNew ){
    memset(pNew, 0, (size_t)nByte);
    pNew->zName = (char*)&pNew[1];
    memcpy(pNew->zName, zName, nName);
    pNew->pUserData = pUserData;
    pNew->x = *pTokenizer;
    pNew->xDestroy = xDestroy;
    pNew->pNext = pGlobal->pTok;
    pGlobal->pTok = pNew;
    if( pNew->pNext==0 ){
      pGlobal->pDfltTok = pNew;
    }
  }else{
    rc = SQLITE_NOMEM;
  }

  return rc;
}

static Fts5TokenizerModule *fts5LocateTokenizer(
  Fts5Global *pGlobal,
  const char *zName
){
  Fts5TokenizerModule *pMod = 0;

  if( zName==0 ){
    pMod = pGlobal->pDfltTok;
  }else{
    for(pMod=pGlobal->pTok; pMod; pMod=pMod->pNext){
      if( sqlite3_stricmp(zName, pMod->zName)==0 ) break;
    }
  }

  return pMod;
}

/*
** Find a tokenizer. This is the implementation of the
** fts5_api.xFindTokenizer() method.
*/
static int fts5FindTokenizer(
  fts5_api *pApi,                 /* Global context (one per db handle) */
  const char *zName,              /* Name of new function */
  void **ppUserData,
  fts5_tokenizer *pTokenizer      /* Populate this object */
){
  int rc = SQLITE_OK;
  Fts5TokenizerModule *pMod;

  pMod = fts5LocateTokenizer((Fts5Global*)pApi, zName);
  if( pMod ){
    *pTokenizer = pMod->x;
    *ppUserData = pMod->pUserData;
  }else{
    memset(pTokenizer, 0, sizeof(fts5_tokenizer));
    rc = SQLITE_ERROR;
  }

  return rc;
}

static int sqlite3Fts5GetTokenizer(
  Fts5Global *pGlobal,
  const char **azArg,
  int nArg,
  Fts5Config *pConfig,
  char **pzErr
){
  Fts5TokenizerModule *pMod;
  int rc = SQLITE_OK;

  pMod = fts5LocateTokenizer(pGlobal, nArg==0 ? 0 : azArg[0]);
  if( pMod==0 ){
    assert( nArg>0 );
    rc = SQLITE_ERROR;
    if( pzErr ) *pzErr = sqlite3_mprintf("no such tokenizer: %s", azArg[0]);
  }else{
    rc = pMod->x.xCreate(
        pMod->pUserData, (azArg?&azArg[1]:0), (nArg?nArg-1:0), &pConfig->pTok
    );
    pConfig->pTokApi = &pMod->x;
    if( rc!=SQLITE_OK ){
      if( pzErr && rc!=SQLITE_NOMEM ){
        *pzErr = sqlite3_mprintf("error in tokenizer constructor");
      }
    }else{
      pConfig->ePattern = sqlite3Fts5TokenizerPattern(
          pMod->x.xCreate, pConfig->pTok
      );
    }
  }

  if( rc!=SQLITE_OK ){
    pConfig->pTokApi = 0;
    pConfig->pTok = 0;
  }

  return rc;
}

static void fts5ModuleDestroy(void *pCtx){
  Fts5TokenizerModule *pTok, *pNextTok;
  Fts5Auxiliary *pAux, *pNextAux;
  Fts5Global *pGlobal = (Fts5Global*)pCtx;

  for(pAux=pGlobal->pAux; pAux; pAux=pNextAux){
    pNextAux = pAux->pNext;
    if( pAux->xDestroy ) pAux->xDestroy(pAux->pUserData);
    sqlite3_free(pAux);
  }

  for(pTok=pGlobal->pTok; pTok; pTok=pNextTok){
    pNextTok = pTok->pNext;
    if( pTok->xDestroy ) pTok->xDestroy(pTok->pUserData);
    sqlite3_free(pTok);
  }

  sqlite3_free(pGlobal);
}

static void fts5Fts5Func(
  sqlite3_context *pCtx,          /* Function call context */
  int nArg,                       /* Number of args */
  sqlite3_value **apArg           /* Function arguments */
){
  Fts5Global *pGlobal = (Fts5Global*)sqlite3_user_data(pCtx);
  fts5_api **ppApi;
  UNUSED_PARAM(nArg);
  assert( nArg==1 );
  ppApi = (fts5_api**)sqlite3_value_pointer(apArg[0], "fts5_api_ptr");
  if( ppApi ) *ppApi = &pGlobal->api;
}

/*
** Implementation of fts5_source_id() function.
*/
static void fts5SourceIdFunc(
  sqlite3_context *pCtx,          /* Function call context */
  int nArg,                       /* Number of args */
  sqlite3_value **apUnused        /* Function arguments */
){
  assert( nArg==0 );
  UNUSED_PARAM2(nArg, apUnused);
  sqlite3_result_text(pCtx, "fts5: 2024-08-13 09:16:08 c9c2ab54ba1f5f46360f1b4f35d849cd3f080e6fc2b6c60e91b16c63f69a1e33", -1, SQLITE_TRANSIENT);
}
