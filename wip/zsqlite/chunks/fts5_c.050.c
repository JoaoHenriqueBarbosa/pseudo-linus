
static void fts5SetVtabError(Fts5FullTable *p, const char *zFormat, ...){
  va_list ap;                     /* ... printf arguments */
  va_start(ap, zFormat);
  assert( p->p.base.zErrMsg==0 );
  p->p.base.zErrMsg = sqlite3_vmprintf(zFormat, ap);
  va_end(ap);
}

/*
** This function is called to handle an FTS INSERT command. In other words,
** an INSERT statement of the form:
**
**     INSERT INTO fts(fts) VALUES($pCmd)
**     INSERT INTO fts(fts, rank) VALUES($pCmd, $pVal)
**
** Argument pVal is the value assigned to column "fts" by the INSERT
** statement. This function returns SQLITE_OK if successful, or an SQLite
** error code if an error occurs.
**
** The commands implemented by this function are documented in the "Special
** INSERT Directives" section of the documentation. It should be updated if
** more commands are added to this function.
*/
static int fts5SpecialInsert(
  Fts5FullTable *pTab,            /* Fts5 table object */
  const char *zCmd,               /* Text inserted into table-name column */
  sqlite3_value *pVal             /* Value inserted into rank column */
){
  Fts5Config *pConfig = pTab->p.pConfig;
  int rc = SQLITE_OK;
  int bError = 0;
  int bLoadConfig = 0;

  if( 0==sqlite3_stricmp("delete-all", zCmd) ){
    if( pConfig->eContent==FTS5_CONTENT_NORMAL ){
      fts5SetVtabError(pTab,
          "'delete-all' may only be used with a "
          "contentless or external content fts5 table"
      );
      rc = SQLITE_ERROR;
    }else{
      rc = sqlite3Fts5StorageDeleteAll(pTab->pStorage);
    }
    bLoadConfig = 1;
  }else if( 0==sqlite3_stricmp("rebuild", zCmd) ){
    if( pConfig->eContent==FTS5_CONTENT_NONE ){
      fts5SetVtabError(pTab,
          "'rebuild' may not be used with a contentless fts5 table"
      );
      rc = SQLITE_ERROR;
    }else{
      rc = sqlite3Fts5StorageRebuild(pTab->pStorage);
    }
    bLoadConfig = 1;
  }else if( 0==sqlite3_stricmp("optimize", zCmd) ){
    rc = sqlite3Fts5StorageOptimize(pTab->pStorage);
  }else if( 0==sqlite3_stricmp("merge", zCmd) ){
    int nMerge = sqlite3_value_int(pVal);
    rc = sqlite3Fts5StorageMerge(pTab->pStorage, nMerge);
  }else if( 0==sqlite3_stricmp("integrity-check", zCmd) ){
    int iArg = sqlite3_value_int(pVal);
    rc = sqlite3Fts5StorageIntegrity(pTab->pStorage, iArg);
#ifdef SQLITE_DEBUG
  }else if( 0==sqlite3_stricmp("prefix-index", zCmd) ){
    pConfig->bPrefixIndex = sqlite3_value_int(pVal);
#endif
  }else if( 0==sqlite3_stricmp("flush", zCmd) ){
    rc = sqlite3Fts5FlushToDisk(&pTab->p);
  }else{
    rc = sqlite3Fts5FlushToDisk(&pTab->p);
    if( rc==SQLITE_OK ){
      rc = sqlite3Fts5IndexLoadConfig(pTab->p.pIndex);
    }
    if( rc==SQLITE_OK ){
      rc = sqlite3Fts5ConfigSetValue(pTab->p.pConfig, zCmd, pVal, &bError);
    }
    if( rc==SQLITE_OK ){
      if( bError ){
        rc = SQLITE_ERROR;
      }else{
        rc = sqlite3Fts5StorageConfigValue(pTab->pStorage, zCmd, pVal, 0);
      }
    }
  }

  if( rc==SQLITE_OK && bLoadConfig ){
    pTab->p.pConfig->iCookie--;
    rc = sqlite3Fts5IndexLoadConfig(pTab->p.pIndex);
  }

  return rc;
}

static int fts5SpecialDelete(
  Fts5FullTable *pTab,
  sqlite3_value **apVal
){
  int rc = SQLITE_OK;
  int eType1 = sqlite3_value_type(apVal[1]);
  if( eType1==SQLITE_INTEGER ){
    sqlite3_int64 iDel = sqlite3_value_int64(apVal[1]);
    rc = sqlite3Fts5StorageDelete(pTab->pStorage, iDel, &apVal[2]);
  }
  return rc;
}

static void fts5StorageInsert(
  int *pRc,
  Fts5FullTable *pTab,
  sqlite3_value **apVal,
  i64 *piRowid
){
  int rc = *pRc;
  if( rc==SQLITE_OK ){
    rc = sqlite3Fts5StorageContentInsert(pTab->pStorage, apVal, piRowid);
  }
  if( rc==SQLITE_OK ){
    rc = sqlite3Fts5StorageIndexInsert(pTab->pStorage, apVal, *piRowid);
  }
  *pRc = rc;
}

/*
** This function is the implementation of the xUpdate callback used by
** FTS3 virtual tables. It is invoked by SQLite each time a row is to be
** inserted, updated or deleted.
**
** A delete specifies a single argument - the rowid of the row to remove.
**
** Update and insert operations pass:
**
**   1. The "old" rowid, or NULL.
**   2. The "new" rowid.
**   3. Values for each of the nCol matchable columns.
**   4. Values for the two hidden columns (<tablename> and "rank").
*/
static int fts5UpdateMethod(
  sqlite3_vtab *pVtab,            /* Virtual table handle */
  int nArg,                       /* Size of argument array */
  sqlite3_value **apVal,          /* Array of arguments */
  sqlite_int64 *pRowid            /* OUT: The affected (or effected) rowid */
){
  Fts5FullTable *pTab = (Fts5FullTable*)pVtab;
  Fts5Config *pConfig = pTab->p.pConfig;
  int eType0;                     /* value_type() of apVal[0] */
  int rc = SQLITE_OK;             /* Return code */
  int bUpdateOrDelete = 0;

  /* A transaction must be open when this is called. */
  assert( pTab->ts.eState==1 || pTab->ts.eState==2 );

  assert( pVtab->zErrMsg==0 );
  assert( nArg==1 || nArg==(2+pConfig->nCol+2) );
  assert( sqlite3_value_type(apVal[0])==SQLITE_INTEGER
       || sqlite3_value_type(apVal[0])==SQLITE_NULL
  );
  assert( pTab->p.pConfig->pzErrmsg==0 );
  if( pConfig->pgsz==0 ){
    rc = sqlite3Fts5IndexLoadConfig(pTab->p.pIndex);
    if( rc!=SQLITE_OK ) return rc;
  }

  pTab->p.pConfig->pzErrmsg = &pTab->p.base.zErrMsg;

  /* Put any active cursors into REQUIRE_SEEK state. */
  fts5TripCursors(pTab);

  eType0 = sqlite3_value_type(apVal[0]);
  if( eType0==SQLITE_NULL
   && sqlite3_value_type(apVal[2+pConfig->nCol])!=SQLITE_NULL
  ){
    /* A "special" INSERT op. These are handled separately. */
    const char *z = (const char*)sqlite3_value_text(apVal[2+pConfig->nCol]);
    if( pConfig->eContent!=FTS5_CONTENT_NORMAL
      && 0==sqlite3_stricmp("delete", z)
    ){
      if( pConfig->bContentlessDelete ){
        fts5SetVtabError(pTab,
            "'delete' may not be used with a contentless_delete=1 table"
        );
        rc = SQLITE_ERROR;
      }else{
        rc = fts5SpecialDelete(pTab, apVal);
        bUpdateOrDelete = 1;
      }
    }else{
      rc = fts5SpecialInsert(pTab, z, apVal[2 + pConfig->nCol + 1]);
    }
  }else{
    /* A regular INSERT, UPDATE or DELETE statement. The trick here is that
    ** any conflict on the rowid value must be detected before any
    ** modifications are made to the database file. There are 4 cases:
    **
    **   1) DELETE
    **   2) UPDATE (rowid not modified)
    **   3) UPDATE (rowid modified)
    **   4) INSERT
    **
    ** Cases 3 and 4 may violate the rowid constraint.
    */
    int eConflict = SQLITE_ABORT;
    if( pConfig->eContent==FTS5_CONTENT_NORMAL || pConfig->bContentlessDelete ){
      eConflict = sqlite3_vtab_on_conflict(pConfig->db);
    }

    assert( eType0==SQLITE_INTEGER || eType0==SQLITE_NULL );
    assert( nArg!=1 || eType0==SQLITE_INTEGER );

    /* Filter out attempts to run UPDATE or DELETE on contentless tables.
    ** This is not suported. Except - they are both supported if the CREATE
    ** VIRTUAL TABLE statement contained "contentless_delete=1". */
    if( eType0==SQLITE_INTEGER
     && pConfig->eContent==FTS5_CONTENT_NONE
     && pConfig->bContentlessDelete==0
    ){
      pTab->p.base.zErrMsg = sqlite3_mprintf(
          "cannot %s contentless fts5 table: %s",
          (nArg>1 ? "UPDATE" : "DELETE from"), pConfig->zName
      );
      rc = SQLITE_ERROR;
    }

    /* DELETE */
    else if( nArg==1 ){
      i64 iDel = sqlite3_value_int64(apVal[0]);  /* Rowid to delete */
      rc = sqlite3Fts5StorageDelete(pTab->pStorage, iDel, 0);
      bUpdateOrDelete = 1;
    }

    /* INSERT or UPDATE */
    else{
      int eType1 = sqlite3_value_numeric_type(apVal[1]);

      if( eType1!=SQLITE_INTEGER && eType1!=SQLITE_NULL ){
        rc = SQLITE_MISMATCH;
      }

      else if( eType0!=SQLITE_INTEGER ){
        /* An INSERT statement. If the conflict-mode is REPLACE, first remove
        ** the current entry (if any). */
        if( eConflict==SQLITE_REPLACE && eType1==SQLITE_INTEGER ){
          i64 iNew = sqlite3_value_int64(apVal[1]);  /* Rowid to delete */
          rc = sqlite3Fts5StorageDelete(pTab->pStorage, iNew, 0);
          bUpdateOrDelete = 1;
        }
        fts5StorageInsert(&rc, pTab, apVal, pRowid);
      }

      /* UPDATE */
      else{
        i64 iOld = sqlite3_value_int64(apVal[0]);  /* Old rowid */
        i64 iNew = sqlite3_value_int64(apVal[1]);  /* New rowid */
        if( eType1==SQLITE_INTEGER && iOld!=iNew ){
          if( eConflict==SQLITE_REPLACE ){
            rc = sqlite3Fts5StorageDelete(pTab->pStorage, iOld, 0);
            if( rc==SQLITE_OK ){
              rc = sqlite3Fts5StorageDelete(pTab->pStorage, iNew, 0);
            }
            fts5StorageInsert(&rc, pTab, apVal, pRowid);
          }else{
            rc = sqlite3Fts5StorageContentInsert(pTab->pStorage, apVal, pRowid);
            if( rc==SQLITE_OK ){
              rc = sqlite3Fts5StorageDelete(pTab->pStorage, iOld, 0);
            }
            if( rc==SQLITE_OK ){
              rc = sqlite3Fts5StorageIndexInsert(pTab->pStorage, apVal,*pRowid);
            }
          }
        }else{
          rc = sqlite3Fts5StorageDelete(pTab->pStorage, iOld, 0);
          fts5StorageInsert(&rc, pTab, apVal, pRowid);
        }
        bUpdateOrDelete = 1;
      }
    }
  }

  if( rc==SQLITE_OK
   && bUpdateOrDelete
   && pConfig->bSecureDelete
   && pConfig->iVersion==FTS5_CURRENT_VERSION
  ){
    rc = sqlite3Fts5StorageConfigValue(
        pTab->pStorage, "version", 0, FTS5_CURRENT_VERSION_SECUREDELETE
    );
    if( rc==SQLITE_OK ){
      pConfig->iVersion = FTS5_CURRENT_VERSION_SECUREDELETE;
    }
  }

  pTab->p.pConfig->pzErrmsg = 0;
  return rc;
}

/*
** Implementation of xSync() method.
*/
static int fts5SyncMethod(sqlite3_vtab *pVtab){
  int rc;
  Fts5FullTable *pTab = (Fts5FullTable*)pVtab;
  fts5CheckTransactionState(pTab, FTS5_SYNC, 0);
  pTab->p.pConfig->pzErrmsg = &pTab->p.base.zErrMsg;
  rc = sqlite3Fts5FlushToDisk(&pTab->p);
  pTab->p.pConfig->pzErrmsg = 0;
  return rc;
}

/*
** Implementation of xBegin() method.
*/
static int fts5BeginMethod(sqlite3_vtab *pVtab){
  fts5CheckTransactionState((Fts5FullTable*)pVtab, FTS5_BEGIN, 0);
  fts5NewTransaction((Fts5FullTable*)pVtab);
  return SQLITE_OK;
}

/*
** Implementation of xCommit() method. This is a no-op. The contents of
** the pending-terms hash-table have already been flushed into the database
** by fts5SyncMethod().
*/
static int fts5CommitMethod(sqlite3_vtab *pVtab){
  UNUSED_PARAM(pVtab);  /* Call below is a no-op for NDEBUG builds */
  fts5CheckTransactionState((Fts5FullTable*)pVtab, FTS5_COMMIT, 0);
  return SQLITE_OK;
}

/*
** Implementation of xRollback(). Discard the contents of the pending-terms
** hash-table. Any changes made to the database are reverted by SQLite.
*/
static int fts5RollbackMethod(sqlite3_vtab *pVtab){
  int rc;
  Fts5FullTable *pTab = (Fts5FullTable*)pVtab;
  fts5CheckTransactionState(pTab, FTS5_ROLLBACK, 0);
  rc = sqlite3Fts5StorageRollback(pTab->pStorage);
  return rc;
}

static int fts5CsrPoslist(Fts5Cursor*, int, const u8**, int*);

static void *fts5ApiUserData(Fts5Context *pCtx){
  Fts5Cursor *pCsr = (Fts5Cursor*)pCtx;
  return pCsr->pAux->pUserData;
}

static int fts5ApiColumnCount(Fts5Context *pCtx){
  Fts5Cursor *pCsr = (Fts5Cursor*)pCtx;
  return ((Fts5Table*)(pCsr->base.pVtab))->pConfig->nCol;
}
