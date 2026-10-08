
/*
** Store the current contents of the p->nTotalRow and p->aTotalSize[]
** variables in the "averages" record on disk.
**
** Return SQLITE_OK if successful, or an SQLite error code if an error
** occurs.
*/
static int fts5StorageSaveTotals(Fts5Storage *p){
  int nCol = p->pConfig->nCol;
  int i;
  Fts5Buffer buf;
  int rc = SQLITE_OK;
  memset(&buf, 0, sizeof(buf));

  sqlite3Fts5BufferAppendVarint(&rc, &buf, p->nTotalRow);
  for(i=0; i<nCol; i++){
    sqlite3Fts5BufferAppendVarint(&rc, &buf, p->aTotalSize[i]);
  }
  if( rc==SQLITE_OK ){
    rc = sqlite3Fts5IndexSetAverages(p->pIndex, buf.p, buf.n);
  }
  sqlite3_free(buf.p);

  return rc;
}

/*
** Remove a row from the FTS table.
*/
static int sqlite3Fts5StorageDelete(Fts5Storage *p, i64 iDel, sqlite3_value **apVal){
  Fts5Config *pConfig = p->pConfig;
  int rc;
  sqlite3_stmt *pDel = 0;

  assert( pConfig->eContent!=FTS5_CONTENT_NORMAL || apVal==0 );
  rc = fts5StorageLoadTotals(p, 1);

  /* Delete the index records */
  if( rc==SQLITE_OK ){
    rc = sqlite3Fts5IndexBeginWrite(p->pIndex, 1, iDel);
  }

  if( rc==SQLITE_OK ){
    if( p->pConfig->bContentlessDelete ){
      rc = fts5StorageContentlessDelete(p, iDel);
    }else{
      rc = fts5StorageDeleteFromIndex(p, iDel, apVal);
    }
  }

  /* Delete the %_docsize record */
  if( rc==SQLITE_OK && pConfig->bColumnsize ){
    rc = fts5StorageGetStmt(p, FTS5_STMT_DELETE_DOCSIZE, &pDel, 0);
    if( rc==SQLITE_OK ){
      sqlite3_bind_int64(pDel, 1, iDel);
      sqlite3_step(pDel);
      rc = sqlite3_reset(pDel);
    }
  }

  /* Delete the %_content record */
  if( pConfig->eContent==FTS5_CONTENT_NORMAL ){
    if( rc==SQLITE_OK ){
      rc = fts5StorageGetStmt(p, FTS5_STMT_DELETE_CONTENT, &pDel, 0);
    }
    if( rc==SQLITE_OK ){
      sqlite3_bind_int64(pDel, 1, iDel);
      sqlite3_step(pDel);
      rc = sqlite3_reset(pDel);
    }
  }

  return rc;
}

/*
** Delete all entries in the FTS5 index.
*/
static int sqlite3Fts5StorageDeleteAll(Fts5Storage *p){
  Fts5Config *pConfig = p->pConfig;
  int rc;

  p->bTotalsValid = 0;

  /* Delete the contents of the %_data and %_docsize tables. */
  rc = fts5ExecPrintf(pConfig->db, 0,
      "DELETE FROM %Q.'%q_data';"
      "DELETE FROM %Q.'%q_idx';",
      pConfig->zDb, pConfig->zName,
      pConfig->zDb, pConfig->zName
  );
  if( rc==SQLITE_OK && pConfig->bColumnsize ){
    rc = fts5ExecPrintf(pConfig->db, 0,
        "DELETE FROM %Q.'%q_docsize';",
        pConfig->zDb, pConfig->zName
    );
  }

  /* Reinitialize the %_data table. This call creates the initial structure
  ** and averages records.  */
  if( rc==SQLITE_OK ){
    rc = sqlite3Fts5IndexReinit(p->pIndex);
  }
  if( rc==SQLITE_OK ){
    rc = sqlite3Fts5StorageConfigValue(p, "version", 0, FTS5_CURRENT_VERSION);
  }
  return rc;
}

static int sqlite3Fts5StorageRebuild(Fts5Storage *p){
  Fts5Buffer buf = {0,0,0};
  Fts5Config *pConfig = p->pConfig;
  sqlite3_stmt *pScan = 0;
  Fts5InsertCtx ctx;
  int rc, rc2;

  memset(&ctx, 0, sizeof(Fts5InsertCtx));
  ctx.pStorage = p;
  rc = sqlite3Fts5StorageDeleteAll(p);
  if( rc==SQLITE_OK ){
    rc = fts5StorageLoadTotals(p, 1);
  }

  if( rc==SQLITE_OK ){
    rc = fts5StorageGetStmt(p, FTS5_STMT_SCAN, &pScan, pConfig->pzErrmsg);
  }

  while( rc==SQLITE_OK && SQLITE_ROW==sqlite3_step(pScan) ){
    i64 iRowid = sqlite3_column_int64(pScan, 0);

    sqlite3Fts5BufferZero(&buf);
    rc = sqlite3Fts5IndexBeginWrite(p->pIndex, 0, iRowid);
    for(ctx.iCol=0; rc==SQLITE_OK && ctx.iCol<pConfig->nCol; ctx.iCol++){
      ctx.szCol = 0;
      if( pConfig->abUnindexed[ctx.iCol]==0 ){
        const char *zText = (const char*)sqlite3_column_text(pScan, ctx.iCol+1);
        int nText = sqlite3_column_bytes(pScan, ctx.iCol+1);
        rc = sqlite3Fts5Tokenize(pConfig,
            FTS5_TOKENIZE_DOCUMENT,
            zText, nText,
            (void*)&ctx,
            fts5StorageInsertCallback
        );
      }
      sqlite3Fts5BufferAppendVarint(&rc, &buf, ctx.szCol);
      p->aTotalSize[ctx.iCol] += (i64)ctx.szCol;
    }
    p->nTotalRow++;

    if( rc==SQLITE_OK ){
      rc = fts5StorageInsertDocsize(p, iRowid, &buf);
    }
  }
  sqlite3_free(buf.p);
  rc2 = sqlite3_reset(pScan);
  if( rc==SQLITE_OK ) rc = rc2;

  /* Write the averages record */
  if( rc==SQLITE_OK ){
    rc = fts5StorageSaveTotals(p);
  }
  return rc;
}

static int sqlite3Fts5StorageOptimize(Fts5Storage *p){
  return sqlite3Fts5IndexOptimize(p->pIndex);
}

static int sqlite3Fts5StorageMerge(Fts5Storage *p, int nMerge){
  return sqlite3Fts5IndexMerge(p->pIndex, nMerge);
}

static int sqlite3Fts5StorageReset(Fts5Storage *p){
  return sqlite3Fts5IndexReset(p->pIndex);
}

/*
** Allocate a new rowid. This is used for "external content" tables when
** a NULL value is inserted into the rowid column. The new rowid is allocated
** by inserting a dummy row into the %_docsize table. The dummy will be
** overwritten later.
**
** If the %_docsize table does not exist, SQLITE_MISMATCH is returned. In
** this case the user is required to provide a rowid explicitly.
*/
static int fts5StorageNewRowid(Fts5Storage *p, i64 *piRowid){
  int rc = SQLITE_MISMATCH;
  if( p->pConfig->bColumnsize ){
    sqlite3_stmt *pReplace = 0;
    rc = fts5StorageGetStmt(p, FTS5_STMT_REPLACE_DOCSIZE, &pReplace, 0);
    if( rc==SQLITE_OK ){
      sqlite3_bind_null(pReplace, 1);
      sqlite3_bind_null(pReplace, 2);
      sqlite3_step(pReplace);
      rc = sqlite3_reset(pReplace);
    }
    if( rc==SQLITE_OK ){
      *piRowid = sqlite3_last_insert_rowid(p->pConfig->db);
    }
  }
  return rc;
}

/*
** Insert a new row into the FTS content table.
*/
static int sqlite3Fts5StorageContentInsert(
  Fts5Storage *p,
  sqlite3_value **apVal,
  i64 *piRowid
){
  Fts5Config *pConfig = p->pConfig;
  int rc = SQLITE_OK;

  /* Insert the new row into the %_content table. */
  if( pConfig->eContent!=FTS5_CONTENT_NORMAL ){
    if( sqlite3_value_type(apVal[1])==SQLITE_INTEGER ){
      *piRowid = sqlite3_value_int64(apVal[1]);
    }else{
      rc = fts5StorageNewRowid(p, piRowid);
    }
  }else{
    sqlite3_stmt *pInsert = 0;    /* Statement to write %_content table */
    int i;                        /* Counter variable */
    rc = fts5StorageGetStmt(p, FTS5_STMT_INSERT_CONTENT, &pInsert, 0);
    for(i=1; rc==SQLITE_OK && i<=pConfig->nCol+1; i++){
      rc = sqlite3_bind_value(pInsert, i, apVal[i]);
    }
    if( rc==SQLITE_OK ){
      sqlite3_step(pInsert);
      rc = sqlite3_reset(pInsert);
    }
    *piRowid = sqlite3_last_insert_rowid(pConfig->db);
  }

  return rc;
}

/*
** Insert new entries into the FTS index and %_docsize table.
*/
static int sqlite3Fts5StorageIndexInsert(
  Fts5Storage *p,
  sqlite3_value **apVal,
  i64 iRowid
){
  Fts5Config *pConfig = p->pConfig;
  int rc = SQLITE_OK;             /* Return code */
  Fts5InsertCtx ctx;              /* Tokenization callback context object */
  Fts5Buffer buf;                 /* Buffer used to build up %_docsize blob */

  memset(&buf, 0, sizeof(Fts5Buffer));
  ctx.pStorage = p;
  rc = fts5StorageLoadTotals(p, 1);

  if( rc==SQLITE_OK ){
    rc = sqlite3Fts5IndexBeginWrite(p->pIndex, 0, iRowid);
  }
  for(ctx.iCol=0; rc==SQLITE_OK && ctx.iCol<pConfig->nCol; ctx.iCol++){
    ctx.szCol = 0;
    if( pConfig->abUnindexed[ctx.iCol]==0 ){
      const char *zText = (const char*)sqlite3_value_text(apVal[ctx.iCol+2]);
      int nText = sqlite3_value_bytes(apVal[ctx.iCol+2]);
      rc = sqlite3Fts5Tokenize(pConfig,
          FTS5_TOKENIZE_DOCUMENT,
          zText, nText,
          (void*)&ctx,
          fts5StorageInsertCallback
      );
    }
    sqlite3Fts5BufferAppendVarint(&rc, &buf, ctx.szCol);
    p->aTotalSize[ctx.iCol] += (i64)ctx.szCol;
  }
  p->nTotalRow++;

  /* Write the %_docsize record */
  if( rc==SQLITE_OK ){
    rc = fts5StorageInsertDocsize(p, iRowid, &buf);
  }
  sqlite3_free(buf.p);

  return rc;
}

static int fts5StorageCount(Fts5Storage *p, const char *zSuffix, i64 *pnRow){
  Fts5Config *pConfig = p->pConfig;
  char *zSql;
  int rc;

  zSql = sqlite3_mprintf("SELECT count(*) FROM %Q.'%q_%s'",
      pConfig->zDb, pConfig->zName, zSuffix
  );
  if( zSql==0 ){
    rc = SQLITE_NOMEM;
  }else{
    sqlite3_stmt *pCnt = 0;
    rc = sqlite3_prepare_v2(pConfig->db, zSql, -1, &pCnt, 0);
    if( rc==SQLITE_OK ){
      if( SQLITE_ROW==sqlite3_step(pCnt) ){
        *pnRow = sqlite3_column_int64(pCnt, 0);
      }
      rc = sqlite3_finalize(pCnt);
    }
  }

  sqlite3_free(zSql);
  return rc;
}

/*
** Context object used by sqlite3Fts5StorageIntegrity().
*/
typedef struct Fts5IntegrityCtx Fts5IntegrityCtx;
struct Fts5IntegrityCtx {
  i64 iRowid;
  int iCol;
  int szCol;
  u64 cksum;
  Fts5Termset *pTermset;
  Fts5Config *pConfig;
};


/*
** Tokenization callback used by integrity check.
*/
static int fts5StorageIntegrityCallback(
  void *pContext,                 /* Pointer to Fts5IntegrityCtx object */
  int tflags,
  const char *pToken,             /* Buffer containing token */
  int nToken,                     /* Size of token in bytes */
  int iUnused1,                   /* Start offset of token */
  int iUnused2                    /* End offset of token */
){
  Fts5IntegrityCtx *pCtx = (Fts5IntegrityCtx*)pContext;
  Fts5Termset *pTermset = pCtx->pTermset;
  int bPresent;
  int ii;
  int rc = SQLITE_OK;
  int iPos;
  int iCol;

  UNUSED_PARAM2(iUnused1, iUnused2);
  if( nToken>FTS5_MAX_TOKEN_SIZE ) nToken = FTS5_MAX_TOKEN_SIZE;

  if( (tflags & FTS5_TOKEN_COLOCATED)==0 || pCtx->szCol==0 ){
    pCtx->szCol++;
  }

  switch( pCtx->pConfig->eDetail ){
    case FTS5_DETAIL_FULL:
      iPos = pCtx->szCol-1;
      iCol = pCtx->iCol;
      break;

    case FTS5_DETAIL_COLUMNS:
      iPos = pCtx->iCol;
      iCol = 0;
      break;

    default:
      assert( pCtx->pConfig->eDetail==FTS5_DETAIL_NONE );
      iPos = 0;
      iCol = 0;
      break;
  }

  rc = sqlite3Fts5TermsetAdd(pTermset, 0, pToken, nToken, &bPresent);
  if( rc==SQLITE_OK && bPresent==0 ){
    pCtx->cksum ^= sqlite3Fts5IndexEntryCksum(
        pCtx->iRowid, iCol, iPos, 0, pToken, nToken
    );
  }

  for(ii=0; rc==SQLITE_OK && ii<pCtx->pConfig->nPrefix; ii++){
    const int nChar = pCtx->pConfig->aPrefix[ii];
    int nByte = sqlite3Fts5IndexCharlenToBytelen(pToken, nToken, nChar);
    if( nByte ){
      rc = sqlite3Fts5TermsetAdd(pTermset, ii+1, pToken, nByte, &bPresent);
      if( bPresent==0 ){
        pCtx->cksum ^= sqlite3Fts5IndexEntryCksum(
            pCtx->iRowid, iCol, iPos, ii+1, pToken, nByte
        );
      }
    }
  }

  return rc;
}
