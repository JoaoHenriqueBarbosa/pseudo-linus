
/*
** Implementation of FTS3 xRename method. Rename an fts3 table.
*/
static int fts3RenameMethod(
  sqlite3_vtab *pVtab,            /* Virtual table handle */
  const char *zName               /* New name of table */
){
  Fts3Table *p = (Fts3Table *)pVtab;
  sqlite3 *db = p->db;            /* Database connection */
  int rc;                         /* Return Code */

  /* At this point it must be known if the %_stat table exists or not.
  ** So bHasStat may not be 2.  */
  rc = fts3SetHasStat(p);

  /* As it happens, the pending terms table is always empty here. This is
  ** because an "ALTER TABLE RENAME TABLE" statement inside a transaction
  ** always opens a savepoint transaction. And the xSavepoint() method
  ** flushes the pending terms table. But leave the (no-op) call to
  ** PendingTermsFlush() in in case that changes.
  */
  assert( p->nPendingData==0 );
  if( rc==SQLITE_OK ){
    rc = sqlite3Fts3PendingTermsFlush(p);
  }

  p->bIgnoreSavepoint = 1;

  if( p->zContentTbl==0 ){
    fts3DbExec(&rc, db,
      "ALTER TABLE %Q.'%q_content'  RENAME TO '%q_content';",
      p->zDb, p->zName, zName
    );
  }

  if( p->bHasDocsize ){
    fts3DbExec(&rc, db,
      "ALTER TABLE %Q.'%q_docsize'  RENAME TO '%q_docsize';",
      p->zDb, p->zName, zName
    );
  }
  if( p->bHasStat ){
    fts3DbExec(&rc, db,
      "ALTER TABLE %Q.'%q_stat'  RENAME TO '%q_stat';",
      p->zDb, p->zName, zName
    );
  }
  fts3DbExec(&rc, db,
    "ALTER TABLE %Q.'%q_segments' RENAME TO '%q_segments';",
    p->zDb, p->zName, zName
  );
  fts3DbExec(&rc, db,
    "ALTER TABLE %Q.'%q_segdir'   RENAME TO '%q_segdir';",
    p->zDb, p->zName, zName
  );

  p->bIgnoreSavepoint = 0;
  return rc;
}

/*
** The xSavepoint() method.
**
** Flush the contents of the pending-terms table to disk.
*/
static int fts3SavepointMethod(sqlite3_vtab *pVtab, int iSavepoint){
  int rc = SQLITE_OK;
  Fts3Table *pTab = (Fts3Table*)pVtab;
  assert( pTab->inTransaction );
  assert( pTab->mxSavepoint<=iSavepoint );
  TESTONLY( pTab->mxSavepoint = iSavepoint );

  if( pTab->bIgnoreSavepoint==0 ){
    if( fts3HashCount(&pTab->aIndex[0].hPending)>0 ){
      char *zSql = sqlite3_mprintf("INSERT INTO %Q.%Q(%Q) VALUES('flush')",
          pTab->zDb, pTab->zName, pTab->zName
          );
      if( zSql ){
        pTab->bIgnoreSavepoint = 1;
        rc = sqlite3_exec(pTab->db, zSql, 0, 0, 0);
        pTab->bIgnoreSavepoint = 0;
        sqlite3_free(zSql);
      }else{
        rc = SQLITE_NOMEM;
      }
    }
    if( rc==SQLITE_OK ){
      pTab->iSavepoint = iSavepoint+1;
    }
  }
  return rc;
}

/*
** The xRelease() method.
**
** This is a no-op.
*/
static int fts3ReleaseMethod(sqlite3_vtab *pVtab, int iSavepoint){
  Fts3Table *pTab = (Fts3Table*)pVtab;
  assert( pTab->inTransaction );
  assert( pTab->mxSavepoint >= iSavepoint );
  TESTONLY( pTab->mxSavepoint = iSavepoint-1 );
  pTab->iSavepoint = iSavepoint;
  return SQLITE_OK;
}

/*
** The xRollbackTo() method.
**
** Discard the contents of the pending terms table.
*/
static int fts3RollbackToMethod(sqlite3_vtab *pVtab, int iSavepoint){
  Fts3Table *pTab = (Fts3Table*)pVtab;
  UNUSED_PARAMETER(iSavepoint);
  assert( pTab->inTransaction );
  TESTONLY( pTab->mxSavepoint = iSavepoint );
  if( (iSavepoint+1)<=pTab->iSavepoint ){
    sqlite3Fts3PendingTermsClear(pTab);
  }
  return SQLITE_OK;
}

/*
** Return true if zName is the extension on one of the shadow tables used
** by this module.
*/
static int fts3ShadowName(const char *zName){
  static const char *azName[] = {
    "content", "docsize", "segdir", "segments", "stat",
  };
  unsigned int i;
  for(i=0; i<sizeof(azName)/sizeof(azName[0]); i++){
    if( sqlite3_stricmp(zName, azName[i])==0 ) return 1;
  }
  return 0;
}

/*
** Implementation of the xIntegrity() method on the FTS3/FTS4 virtual
** table.
*/
static int fts3IntegrityMethod(
  sqlite3_vtab *pVtab,      /* The virtual table to be checked */
  const char *zSchema,      /* Name of schema in which pVtab lives */
  const char *zTabname,     /* Name of the pVTab table */
  int isQuick,              /* True if this is a quick_check */
  char **pzErr              /* Write error message here */
){
  Fts3Table *p = (Fts3Table*)pVtab;
  int rc = SQLITE_OK;
  int bOk = 0;

  UNUSED_PARAMETER(isQuick);
  rc = sqlite3Fts3IntegrityCheck(p, &bOk);
  assert( rc!=SQLITE_CORRUPT_VTAB );
  if( rc==SQLITE_ERROR || (rc&0xFF)==SQLITE_CORRUPT ){
    *pzErr = sqlite3_mprintf("unable to validate the inverted index for"
                             " FTS%d table %s.%s: %s",
                p->bFts4 ? 4 : 3, zSchema, zTabname, sqlite3_errstr(rc));
    if( *pzErr ) rc = SQLITE_OK;
  }else if( rc==SQLITE_OK && bOk==0 ){
    *pzErr = sqlite3_mprintf("malformed inverted index for FTS%d table %s.%s",
                p->bFts4 ? 4 : 3, zSchema, zTabname);
    if( *pzErr==0 ) rc = SQLITE_NOMEM;
  }
  sqlite3Fts3SegmentsClose(p);
  return rc;
}



static const sqlite3_module fts3Module = {
  /* iVersion      */ 4,
  /* xCreate       */ fts3CreateMethod,
  /* xConnect      */ fts3ConnectMethod,
  /* xBestIndex    */ fts3BestIndexMethod,
  /* xDisconnect   */ fts3DisconnectMethod,
  /* xDestroy      */ fts3DestroyMethod,
  /* xOpen         */ fts3OpenMethod,
  /* xClose        */ fts3CloseMethod,
  /* xFilter       */ fts3FilterMethod,
  /* xNext         */ fts3NextMethod,
  /* xEof          */ fts3EofMethod,
  /* xColumn       */ fts3ColumnMethod,
  /* xRowid        */ fts3RowidMethod,
  /* xUpdate       */ fts3UpdateMethod,
  /* xBegin        */ fts3BeginMethod,
  /* xSync         */ fts3SyncMethod,
  /* xCommit       */ fts3CommitMethod,
  /* xRollback     */ fts3RollbackMethod,
  /* xFindFunction */ fts3FindFunctionMethod,
  /* xRename */       fts3RenameMethod,
  /* xSavepoint    */ fts3SavepointMethod,
  /* xRelease      */ fts3ReleaseMethod,
  /* xRollbackTo   */ fts3RollbackToMethod,
  /* xShadowName   */ fts3ShadowName,
  /* xIntegrity    */ fts3IntegrityMethod,
};

/*
** This function is registered as the module destructor (called when an
** FTS3 enabled database connection is closed). It frees the memory
** allocated for the tokenizer hash table.
*/
static void hashDestroy(void *p){
  Fts3HashWrapper *pHash = (Fts3HashWrapper *)p;
  pHash->nRef--;
  if( pHash->nRef<=0 ){
    sqlite3Fts3HashClear(&pHash->hash);
    sqlite3_free(pHash);
  }
}

/*
** The fts3 built-in tokenizers - "simple", "porter" and "icu"- are
** implemented in files fts3_tokenizer1.c, fts3_porter.c and fts3_icu.c
** respectively. The following three forward declarations are for functions
** declared in these files used to retrieve the respective implementations.
**
** Calling sqlite3Fts3SimpleTokenizerModule() sets the value pointed
** to by the argument to point to the "simple" tokenizer implementation.
** And so on.
*/
SQLITE_PRIVATE void sqlite3Fts3SimpleTokenizerModule(sqlite3_tokenizer_module const**ppModule);
SQLITE_PRIVATE void sqlite3Fts3PorterTokenizerModule(sqlite3_tokenizer_module const**ppModule);
#ifndef SQLITE_DISABLE_FTS3_UNICODE
SQLITE_PRIVATE void sqlite3Fts3UnicodeTokenizer(sqlite3_tokenizer_module const**ppModule);
#endif
#ifdef SQLITE_ENABLE_ICU
SQLITE_PRIVATE void sqlite3Fts3IcuTokenizerModule(sqlite3_tokenizer_module const**ppModule);
#endif

/*
** Initialize the fts3 extension. If this extension is built as part
** of the sqlite library, then this function is called directly by
** SQLite. If fts3 is built as a dynamically loadable extension, this
** function is called by the sqlite3_extension_init() entry point.
*/
SQLITE_PRIVATE int sqlite3Fts3Init(sqlite3 *db){
  int rc = SQLITE_OK;
  Fts3HashWrapper *pHash = 0;
  const sqlite3_tokenizer_module *pSimple = 0;
  const sqlite3_tokenizer_module *pPorter = 0;
#ifndef SQLITE_DISABLE_FTS3_UNICODE
  const sqlite3_tokenizer_module *pUnicode = 0;
#endif

#ifdef SQLITE_ENABLE_ICU
  const sqlite3_tokenizer_module *pIcu = 0;
  sqlite3Fts3IcuTokenizerModule(&pIcu);
#endif

#ifndef SQLITE_DISABLE_FTS3_UNICODE
  sqlite3Fts3UnicodeTokenizer(&pUnicode);
#endif

#ifdef SQLITE_TEST
  rc = sqlite3Fts3InitTerm(db);
  if( rc!=SQLITE_OK ) return rc;
#endif

  rc = sqlite3Fts3InitAux(db);
  if( rc!=SQLITE_OK ) return rc;

  sqlite3Fts3SimpleTokenizerModule(&pSimple);
  sqlite3Fts3PorterTokenizerModule(&pPorter);

  /* Allocate and initialize the hash-table used to store tokenizers. */
  pHash = sqlite3_malloc(sizeof(Fts3HashWrapper));
  if( !pHash ){
    rc = SQLITE_NOMEM;
  }else{
    sqlite3Fts3HashInit(&pHash->hash, FTS3_HASH_STRING, 1);
    pHash->nRef = 0;
  }

  /* Load the built-in tokenizers into the hash table */
  if( rc==SQLITE_OK ){
    if( sqlite3Fts3HashInsert(&pHash->hash, "simple", 7, (void *)pSimple)
     || sqlite3Fts3HashInsert(&pHash->hash, "porter", 7, (void *)pPorter)

#ifndef SQLITE_DISABLE_FTS3_UNICODE
     || sqlite3Fts3HashInsert(&pHash->hash, "unicode61", 10, (void *)pUnicode)
#endif
#ifdef SQLITE_ENABLE_ICU
     || (pIcu && sqlite3Fts3HashInsert(&pHash->hash, "icu", 4, (void *)pIcu))
#endif
    ){
      rc = SQLITE_NOMEM;
    }
  }

#ifdef SQLITE_TEST
  if( rc==SQLITE_OK ){
    rc = sqlite3Fts3ExprInitTestInterface(db, &pHash->hash);
  }
#endif

  /* Create the virtual table wrapper around the hash-table and overload
  ** the four scalar functions. If this is successful, register the
  ** module with sqlite.
  */
  if( SQLITE_OK==rc
   && SQLITE_OK==(rc=sqlite3Fts3InitHashTable(db,&pHash->hash,"fts3_tokenizer"))
   && SQLITE_OK==(rc = sqlite3_overload_function(db, "snippet", -1))
   && SQLITE_OK==(rc = sqlite3_overload_function(db, "offsets", 1))
   && SQLITE_OK==(rc = sqlite3_overload_function(db, "matchinfo", 1))
   && SQLITE_OK==(rc = sqlite3_overload_function(db, "matchinfo", 2))
   && SQLITE_OK==(rc = sqlite3_overload_function(db, "optimize", 1))
  ){
    pHash->nRef++;
    rc = sqlite3_create_module_v2(
        db, "fts3", &fts3Module, (void *)pHash, hashDestroy
    );
    if( rc==SQLITE_OK ){
      pHash->nRef++;
      rc = sqlite3_create_module_v2(
          db, "fts4", &fts3Module, (void *)pHash, hashDestroy
      );
    }
    if( rc==SQLITE_OK ){
      pHash->nRef++;
      rc = sqlite3Fts3InitTok(db, (void *)pHash, hashDestroy);
    }
    return rc;
  }


  /* An error has occurred. Delete the hash table and return the error code. */
  assert( rc!=SQLITE_OK );
  if( pHash ){
    sqlite3Fts3HashClear(&pHash->hash);
    sqlite3_free(pHash);
  }
  return rc;
}

/*
** Allocate an Fts3MultiSegReader for each token in the expression headed
** by pExpr.
**
** An Fts3SegReader object is a cursor that can seek or scan a range of
** entries within a single segment b-tree. An Fts3MultiSegReader uses multiple
** Fts3SegReader objects internally to provide an interface to seek or scan
** within the union of all segments of a b-tree. Hence the name.
**
** If the allocated Fts3MultiSegReader just seeks to a single entry in a
** segment b-tree (if the term is not a prefix or it is a prefix for which
** there exists prefix b-tree of the right length) then it may be traversed
** and merged incrementally. Otherwise, it has to be merged into an in-memory
** doclist and then traversed.
*/
static void fts3EvalAllocateReaders(
  Fts3Cursor *pCsr,               /* FTS cursor handle */
  Fts3Expr *pExpr,                /* Allocate readers for this expression */
  int *pnToken,                   /* OUT: Total number of tokens in phrase. */
  int *pnOr,                      /* OUT: Total number of OR nodes in expr. */
  int *pRc                        /* IN/OUT: Error code */
){
  if( pExpr && SQLITE_OK==*pRc ){
    if( pExpr->eType==FTSQUERY_PHRASE ){
      int i;
      int nToken = pExpr->pPhrase->nToken;
      *pnToken += nToken;
      for(i=0; i<nToken; i++){
        Fts3PhraseToken *pToken = &pExpr->pPhrase->aToken[i];
        int rc = fts3TermSegReaderCursor(pCsr,
            pToken->z, pToken->n, pToken->isPrefix, &pToken->pSegcsr
        );
        if( rc!=SQLITE_OK ){
          *pRc = rc;
          return;
        }
      }
      assert( pExpr->pPhrase->iDoclistToken==0 );
      pExpr->pPhrase->iDoclistToken = -1;
    }else{
      *pnOr += (pExpr->eType==FTSQUERY_OR);
      fts3EvalAllocateReaders(pCsr, pExpr->pLeft, pnToken, pnOr, pRc);
      fts3EvalAllocateReaders(pCsr, pExpr->pRight, pnToken, pnOr, pRc);
    }
  }
}
