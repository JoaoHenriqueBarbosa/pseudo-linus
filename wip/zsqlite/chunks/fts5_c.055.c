
/*
** Drop all shadow tables. Return SQLITE_OK if successful or an SQLite error
** code otherwise.
*/
static int sqlite3Fts5DropAll(Fts5Config *pConfig){
  int rc = fts5ExecPrintf(pConfig->db, 0,
      "DROP TABLE IF EXISTS %Q.'%q_data';"
      "DROP TABLE IF EXISTS %Q.'%q_idx';"
      "DROP TABLE IF EXISTS %Q.'%q_config';",
      pConfig->zDb, pConfig->zName,
      pConfig->zDb, pConfig->zName,
      pConfig->zDb, pConfig->zName
  );
  if( rc==SQLITE_OK && pConfig->bColumnsize ){
    rc = fts5ExecPrintf(pConfig->db, 0,
        "DROP TABLE IF EXISTS %Q.'%q_docsize';",
        pConfig->zDb, pConfig->zName
    );
  }
  if( rc==SQLITE_OK && pConfig->eContent==FTS5_CONTENT_NORMAL ){
    rc = fts5ExecPrintf(pConfig->db, 0,
        "DROP TABLE IF EXISTS %Q.'%q_content';",
        pConfig->zDb, pConfig->zName
    );
  }
  return rc;
}

static void fts5StorageRenameOne(
  Fts5Config *pConfig,            /* Current FTS5 configuration */
  int *pRc,                       /* IN/OUT: Error code */
  const char *zTail,              /* Tail of table name e.g. "data", "config" */
  const char *zName               /* New name of FTS5 table */
){
  if( *pRc==SQLITE_OK ){
    *pRc = fts5ExecPrintf(pConfig->db, 0,
        "ALTER TABLE %Q.'%q_%s' RENAME TO '%q_%s';",
        pConfig->zDb, pConfig->zName, zTail, zName, zTail
    );
  }
}

static int sqlite3Fts5StorageRename(Fts5Storage *pStorage, const char *zName){
  Fts5Config *pConfig = pStorage->pConfig;
  int rc = sqlite3Fts5StorageSync(pStorage);

  fts5StorageRenameOne(pConfig, &rc, "data", zName);
  fts5StorageRenameOne(pConfig, &rc, "idx", zName);
  fts5StorageRenameOne(pConfig, &rc, "config", zName);
  if( pConfig->bColumnsize ){
    fts5StorageRenameOne(pConfig, &rc, "docsize", zName);
  }
  if( pConfig->eContent==FTS5_CONTENT_NORMAL ){
    fts5StorageRenameOne(pConfig, &rc, "content", zName);
  }
  return rc;
}

/*
** Create the shadow table named zPost, with definition zDefn. Return
** SQLITE_OK if successful, or an SQLite error code otherwise.
*/
static int sqlite3Fts5CreateTable(
  Fts5Config *pConfig,            /* FTS5 configuration */
  const char *zPost,              /* Shadow table to create (e.g. "content") */
  const char *zDefn,              /* Columns etc. for shadow table */
  int bWithout,                   /* True for without rowid */
  char **pzErr                    /* OUT: Error message */
){
  int rc;
  char *zErr = 0;

  rc = fts5ExecPrintf(pConfig->db, &zErr, "CREATE TABLE %Q.'%q_%q'(%s)%s",
      pConfig->zDb, pConfig->zName, zPost, zDefn,
#ifndef SQLITE_FTS5_NO_WITHOUT_ROWID
      bWithout?" WITHOUT ROWID":
#endif
      ""
  );
  if( zErr ){
    *pzErr = sqlite3_mprintf(
        "fts5: error creating shadow table %q_%s: %s",
        pConfig->zName, zPost, zErr
    );
    sqlite3_free(zErr);
  }

  return rc;
}

/*
** Open a new Fts5Index handle. If the bCreate argument is true, create
** and initialize the underlying tables
**
** If successful, set *pp to point to the new object and return SQLITE_OK.
** Otherwise, set *pp to NULL and return an SQLite error code.
*/
static int sqlite3Fts5StorageOpen(
  Fts5Config *pConfig,
  Fts5Index *pIndex,
  int bCreate,
  Fts5Storage **pp,
  char **pzErr                    /* OUT: Error message */
){
  int rc = SQLITE_OK;
  Fts5Storage *p;                 /* New object */
  sqlite3_int64 nByte;            /* Bytes of space to allocate */

  nByte = sizeof(Fts5Storage)               /* Fts5Storage object */
        + pConfig->nCol * sizeof(i64);      /* Fts5Storage.aTotalSize[] */
  *pp = p = (Fts5Storage*)sqlite3_malloc64(nByte);
  if( !p ) return SQLITE_NOMEM;

  memset(p, 0, (size_t)nByte);
  p->aTotalSize = (i64*)&p[1];
  p->pConfig = pConfig;
  p->pIndex = pIndex;

  if( bCreate ){
    if( pConfig->eContent==FTS5_CONTENT_NORMAL ){
      int nDefn = 32 + pConfig->nCol*10;
      char *zDefn = sqlite3_malloc64(32 + (sqlite3_int64)pConfig->nCol * 10);
      if( zDefn==0 ){
        rc = SQLITE_NOMEM;
      }else{
        int i;
        int iOff;
        sqlite3_snprintf(nDefn, zDefn, "id INTEGER PRIMARY KEY");
        iOff = (int)strlen(zDefn);
        for(i=0; i<pConfig->nCol; i++){
          sqlite3_snprintf(nDefn-iOff, &zDefn[iOff], ", c%d", i);
          iOff += (int)strlen(&zDefn[iOff]);
        }
        rc = sqlite3Fts5CreateTable(pConfig, "content", zDefn, 0, pzErr);
      }
      sqlite3_free(zDefn);
    }

    if( rc==SQLITE_OK && pConfig->bColumnsize ){
      const char *zCols = "id INTEGER PRIMARY KEY, sz BLOB";
      if( pConfig->bContentlessDelete ){
        zCols = "id INTEGER PRIMARY KEY, sz BLOB, origin INTEGER";
      }
      rc = sqlite3Fts5CreateTable(pConfig, "docsize", zCols, 0, pzErr);
    }
    if( rc==SQLITE_OK ){
      rc = sqlite3Fts5CreateTable(
          pConfig, "config", "k PRIMARY KEY, v", 1, pzErr
      );
    }
    if( rc==SQLITE_OK ){
      rc = sqlite3Fts5StorageConfigValue(p, "version", 0, FTS5_CURRENT_VERSION);
    }
  }

  if( rc ){
    sqlite3Fts5StorageClose(p);
    *pp = 0;
  }
  return rc;
}

/*
** Close a handle opened by an earlier call to sqlite3Fts5StorageOpen().
*/
static int sqlite3Fts5StorageClose(Fts5Storage *p){
  int rc = SQLITE_OK;
  if( p ){
    int i;

    /* Finalize all SQL statements */
    for(i=0; i<ArraySize(p->aStmt); i++){
      sqlite3_finalize(p->aStmt[i]);
    }

    sqlite3_free(p);
  }
  return rc;
}

typedef struct Fts5InsertCtx Fts5InsertCtx;
struct Fts5InsertCtx {
  Fts5Storage *pStorage;
  int iCol;
  int szCol;                      /* Size of column value in tokens */
};

/*
** Tokenization callback used when inserting tokens into the FTS index.
*/
static int fts5StorageInsertCallback(
  void *pContext,                 /* Pointer to Fts5InsertCtx object */
  int tflags,
  const char *pToken,             /* Buffer containing token */
  int nToken,                     /* Size of token in bytes */
  int iUnused1,                   /* Start offset of token */
  int iUnused2                    /* End offset of token */
){
  Fts5InsertCtx *pCtx = (Fts5InsertCtx*)pContext;
  Fts5Index *pIdx = pCtx->pStorage->pIndex;
  UNUSED_PARAM2(iUnused1, iUnused2);
  if( nToken>FTS5_MAX_TOKEN_SIZE ) nToken = FTS5_MAX_TOKEN_SIZE;
  if( (tflags & FTS5_TOKEN_COLOCATED)==0 || pCtx->szCol==0 ){
    pCtx->szCol++;
  }
  return sqlite3Fts5IndexWrite(pIdx, pCtx->iCol, pCtx->szCol-1, pToken, nToken);
}

/*
** If a row with rowid iDel is present in the %_content table, add the
** delete-markers to the FTS index necessary to delete it. Do not actually
** remove the %_content row at this time though.
*/
static int fts5StorageDeleteFromIndex(
  Fts5Storage *p,
  i64 iDel,
  sqlite3_value **apVal
){
  Fts5Config *pConfig = p->pConfig;
  sqlite3_stmt *pSeek = 0;        /* SELECT to read row iDel from %_data */
  int rc = SQLITE_OK;             /* Return code */
  int rc2;                        /* sqlite3_reset() return code */
  int iCol;
  Fts5InsertCtx ctx;

  if( apVal==0 ){
    rc = fts5StorageGetStmt(p, FTS5_STMT_LOOKUP, &pSeek, 0);
    if( rc!=SQLITE_OK ) return rc;
    sqlite3_bind_int64(pSeek, 1, iDel);
    if( sqlite3_step(pSeek)!=SQLITE_ROW ){
      return sqlite3_reset(pSeek);
    }
  }

  ctx.pStorage = p;
  ctx.iCol = -1;
  for(iCol=1; rc==SQLITE_OK && iCol<=pConfig->nCol; iCol++){
    if( pConfig->abUnindexed[iCol-1]==0 ){
      const char *zText;
      int nText;
      assert( pSeek==0 || apVal==0 );
      assert( pSeek!=0 || apVal!=0 );
      if( pSeek ){
        zText = (const char*)sqlite3_column_text(pSeek, iCol);
        nText = sqlite3_column_bytes(pSeek, iCol);
      }else if( ALWAYS(apVal) ){
        zText = (const char*)sqlite3_value_text(apVal[iCol-1]);
        nText = sqlite3_value_bytes(apVal[iCol-1]);
      }else{
        continue;
      }
      ctx.szCol = 0;
      rc = sqlite3Fts5Tokenize(pConfig, FTS5_TOKENIZE_DOCUMENT,
          zText, nText, (void*)&ctx, fts5StorageInsertCallback
      );
      p->aTotalSize[iCol-1] -= (i64)ctx.szCol;
      if( p->aTotalSize[iCol-1]<0 ){
        rc = FTS5_CORRUPT;
      }
    }
  }
  if( rc==SQLITE_OK && p->nTotalRow<1 ){
    rc = FTS5_CORRUPT;
  }else{
    p->nTotalRow--;
  }

  rc2 = sqlite3_reset(pSeek);
  if( rc==SQLITE_OK ) rc = rc2;
  return rc;
}

/*
** This function is called to process a DELETE on a contentless_delete=1
** table. It adds the tombstone required to delete the entry with rowid
** iDel. If successful, SQLITE_OK is returned. Or, if an error occurs,
** an SQLite error code.
*/
static int fts5StorageContentlessDelete(Fts5Storage *p, i64 iDel){
  i64 iOrigin = 0;
  sqlite3_stmt *pLookup = 0;
  int rc = SQLITE_OK;

  assert( p->pConfig->bContentlessDelete );
  assert( p->pConfig->eContent==FTS5_CONTENT_NONE );

  /* Look up the origin of the document in the %_docsize table. Store
  ** this in stack variable iOrigin.  */
  rc = fts5StorageGetStmt(p, FTS5_STMT_LOOKUP_DOCSIZE, &pLookup, 0);
  if( rc==SQLITE_OK ){
    sqlite3_bind_int64(pLookup, 1, iDel);
    if( SQLITE_ROW==sqlite3_step(pLookup) ){
      iOrigin = sqlite3_column_int64(pLookup, 1);
    }
    rc = sqlite3_reset(pLookup);
  }

  if( rc==SQLITE_OK && iOrigin!=0 ){
    rc = sqlite3Fts5IndexContentlessDelete(p->pIndex, iOrigin, iDel);
  }

  return rc;
}

/*
** Insert a record into the %_docsize table. Specifically, do:
**
**   INSERT OR REPLACE INTO %_docsize(id, sz) VALUES(iRowid, pBuf);
**
** If there is no %_docsize table (as happens if the columnsize=0 option
** is specified when the FTS5 table is created), this function is a no-op.
*/
static int fts5StorageInsertDocsize(
  Fts5Storage *p,                 /* Storage module to write to */
  i64 iRowid,                     /* id value */
  Fts5Buffer *pBuf                /* sz value */
){
  int rc = SQLITE_OK;
  if( p->pConfig->bColumnsize ){
    sqlite3_stmt *pReplace = 0;
    rc = fts5StorageGetStmt(p, FTS5_STMT_REPLACE_DOCSIZE, &pReplace, 0);
    if( rc==SQLITE_OK ){
      sqlite3_bind_int64(pReplace, 1, iRowid);
      if( p->pConfig->bContentlessDelete ){
        i64 iOrigin = 0;
        rc = sqlite3Fts5IndexGetOrigin(p->pIndex, &iOrigin);
        sqlite3_bind_int64(pReplace, 3, iOrigin);
      }
      if( rc==SQLITE_OK ){
        sqlite3_bind_blob(pReplace, 2, pBuf->p, pBuf->n, SQLITE_STATIC);
        sqlite3_step(pReplace);
        rc = sqlite3_reset(pReplace);
        sqlite3_bind_null(pReplace, 2);
      }
    }
  }
  return rc;
}

/*
** Load the contents of the "averages" record from disk into the
** p->nTotalRow and p->aTotalSize[] variables. If successful, and if
** argument bCache is true, set the p->bTotalsValid flag to indicate
** that the contents of aTotalSize[] and nTotalRow are valid until
** further notice.
**
** Return SQLITE_OK if successful, or an SQLite error code if an error
** occurs.
*/
static int fts5StorageLoadTotals(Fts5Storage *p, int bCache){
  int rc = SQLITE_OK;
  if( p->bTotalsValid==0 ){
    rc = sqlite3Fts5IndexGetAverages(p->pIndex, &p->nTotalRow, p->aTotalSize);
    p->bTotalsValid = bCache;
  }
  return rc;
}
