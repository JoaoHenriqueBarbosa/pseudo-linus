
/*
** xEof - Return true if the cursor is at EOF, or false otherwise.
*/
static int fts3tokEofMethod(sqlite3_vtab_cursor *pCursor){
  Fts3tokCursor *pCsr = (Fts3tokCursor *)pCursor;
  return (pCsr->zToken==0);
}

/*
** xColumn - Return a column value.
*/
static int fts3tokColumnMethod(
  sqlite3_vtab_cursor *pCursor,   /* Cursor to retrieve value from */
  sqlite3_context *pCtx,          /* Context for sqlite3_result_xxx() calls */
  int iCol                        /* Index of column to read value from */
){
  Fts3tokCursor *pCsr = (Fts3tokCursor *)pCursor;

  /* CREATE TABLE x(input, token, start, end, position) */
  switch( iCol ){
    case 0:
      sqlite3_result_text(pCtx, pCsr->zInput, -1, SQLITE_TRANSIENT);
      break;
    case 1:
      sqlite3_result_text(pCtx, pCsr->zToken, pCsr->nToken, SQLITE_TRANSIENT);
      break;
    case 2:
      sqlite3_result_int(pCtx, pCsr->iStart);
      break;
    case 3:
      sqlite3_result_int(pCtx, pCsr->iEnd);
      break;
    default:
      assert( iCol==4 );
      sqlite3_result_int(pCtx, pCsr->iPos);
      break;
  }
  return SQLITE_OK;
}

/*
** xRowid - Return the current rowid for the cursor.
*/
static int fts3tokRowidMethod(
  sqlite3_vtab_cursor *pCursor,   /* Cursor to retrieve value from */
  sqlite_int64 *pRowid            /* OUT: Rowid value */
){
  Fts3tokCursor *pCsr = (Fts3tokCursor *)pCursor;
  *pRowid = (sqlite3_int64)pCsr->iRowid;
  return SQLITE_OK;
}

/*
** Register the fts3tok module with database connection db. Return SQLITE_OK
** if successful or an error code if sqlite3_create_module() fails.
*/
SQLITE_PRIVATE int sqlite3Fts3InitTok(sqlite3 *db, Fts3Hash *pHash, void(*xDestroy)(void*)){
  static const sqlite3_module fts3tok_module = {
     0,                           /* iVersion      */
     fts3tokConnectMethod,        /* xCreate       */
     fts3tokConnectMethod,        /* xConnect      */
     fts3tokBestIndexMethod,      /* xBestIndex    */
     fts3tokDisconnectMethod,     /* xDisconnect   */
     fts3tokDisconnectMethod,     /* xDestroy      */
     fts3tokOpenMethod,           /* xOpen         */
     fts3tokCloseMethod,          /* xClose        */
     fts3tokFilterMethod,         /* xFilter       */
     fts3tokNextMethod,           /* xNext         */
     fts3tokEofMethod,            /* xEof          */
     fts3tokColumnMethod,         /* xColumn       */
     fts3tokRowidMethod,          /* xRowid        */
     0,                           /* xUpdate       */
     0,                           /* xBegin        */
     0,                           /* xSync         */
     0,                           /* xCommit       */
     0,                           /* xRollback     */
     0,                           /* xFindFunction */
     0,                           /* xRename       */
     0,                           /* xSavepoint    */
     0,                           /* xRelease      */
     0,                           /* xRollbackTo   */
     0,                           /* xShadowName   */
     0                            /* xIntegrity    */
  };
  int rc;                         /* Return code */

  rc = sqlite3_create_module_v2(
      db, "fts3tokenize", &fts3tok_module, (void*)pHash, xDestroy
  );
  return rc;
}

#endif /* !defined(SQLITE_CORE) || defined(SQLITE_ENABLE_FTS3) */

