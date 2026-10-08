
/*
** This is the xRowid method. The SQLite core calls this routine to
** retrieve the rowid for the current row of the result set. The
** rowid should be written to *pRowid.
*/
static int fts5VocabRowidMethod(
  sqlite3_vtab_cursor *pCursor,
  sqlite_int64 *pRowid
){
  Fts5VocabCursor *pCsr = (Fts5VocabCursor*)pCursor;
  *pRowid = pCsr->rowid;
  return SQLITE_OK;
}

static int sqlite3Fts5VocabInit(Fts5Global *pGlobal, sqlite3 *db){
  static const sqlite3_module fts5Vocab = {
    /* iVersion      */ 2,
    /* xCreate       */ fts5VocabCreateMethod,
    /* xConnect      */ fts5VocabConnectMethod,
    /* xBestIndex    */ fts5VocabBestIndexMethod,
    /* xDisconnect   */ fts5VocabDisconnectMethod,
    /* xDestroy      */ fts5VocabDestroyMethod,
    /* xOpen         */ fts5VocabOpenMethod,
    /* xClose        */ fts5VocabCloseMethod,
    /* xFilter       */ fts5VocabFilterMethod,
    /* xNext         */ fts5VocabNextMethod,
    /* xEof          */ fts5VocabEofMethod,
    /* xColumn       */ fts5VocabColumnMethod,
    /* xRowid        */ fts5VocabRowidMethod,
    /* xUpdate       */ 0,
    /* xBegin        */ 0,
    /* xSync         */ 0,
    /* xCommit       */ 0,
    /* xRollback     */ 0,
    /* xFindFunction */ 0,
    /* xRename       */ 0,
    /* xSavepoint    */ 0,
    /* xRelease      */ 0,
    /* xRollbackTo   */ 0,
    /* xShadowName   */ 0,
    /* xIntegrity    */ 0
  };
  void *p = (void*)pGlobal;

  return sqlite3_create_module_v2(db, "fts5vocab", &fts5Vocab, p, 0);
}



#endif /* !defined(SQLITE_CORE) || defined(SQLITE_ENABLE_FTS5) */

