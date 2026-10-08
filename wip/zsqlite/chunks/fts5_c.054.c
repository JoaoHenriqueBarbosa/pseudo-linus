
/*
** Return true if zName is the extension on one of the shadow tables used
** by this module.
*/
static int fts5ShadowName(const char *zName){
  static const char *azName[] = {
    "config", "content", "data", "docsize", "idx"
  };
  unsigned int i;
  for(i=0; i<sizeof(azName)/sizeof(azName[0]); i++){
    if( sqlite3_stricmp(zName, azName[i])==0 ) return 1;
  }
  return 0;
}

/*
** Run an integrity check on the FTS5 data structures.  Return a string
** if anything is found amiss.  Return a NULL pointer if everything is
** OK.
*/
static int fts5IntegrityMethod(
  sqlite3_vtab *pVtab,    /* the FTS5 virtual table to check */
  const char *zSchema,    /* Name of schema in which this table lives */
  const char *zTabname,   /* Name of the table itself */
  int isQuick,            /* True if this is a quick-check */
  char **pzErr            /* Write error message here */
){
  Fts5FullTable *pTab = (Fts5FullTable*)pVtab;
  int rc;

  assert( pzErr!=0 && *pzErr==0 );
  UNUSED_PARAM(isQuick);
  assert( pTab->p.pConfig->pzErrmsg==0 );
  pTab->p.pConfig->pzErrmsg = pzErr;
  rc = sqlite3Fts5StorageIntegrity(pTab->pStorage, 0);
  if( *pzErr==0 && rc!=SQLITE_OK ){
    if( (rc&0xff)==SQLITE_CORRUPT ){
      *pzErr = sqlite3_mprintf("malformed inverted index for FTS5 table %s.%s",
          zSchema, zTabname);
      rc = (*pzErr) ? SQLITE_OK : SQLITE_NOMEM;
    }else{
      *pzErr = sqlite3_mprintf("unable to validate the inverted index for"
          " FTS5 table %s.%s: %s",
          zSchema, zTabname, sqlite3_errstr(rc));
    }
  }

  sqlite3Fts5IndexCloseReader(pTab->p.pIndex);
  pTab->p.pConfig->pzErrmsg = 0;

  return rc;
}

static int fts5Init(sqlite3 *db){
  static const sqlite3_module fts5Mod = {
    /* iVersion      */ 4,
    /* xCreate       */ fts5CreateMethod,
    /* xConnect      */ fts5ConnectMethod,
    /* xBestIndex    */ fts5BestIndexMethod,
    /* xDisconnect   */ fts5DisconnectMethod,
    /* xDestroy      */ fts5DestroyMethod,
    /* xOpen         */ fts5OpenMethod,
    /* xClose        */ fts5CloseMethod,
    /* xFilter       */ fts5FilterMethod,
    /* xNext         */ fts5NextMethod,
    /* xEof          */ fts5EofMethod,
    /* xColumn       */ fts5ColumnMethod,
    /* xRowid        */ fts5RowidMethod,
    /* xUpdate       */ fts5UpdateMethod,
    /* xBegin        */ fts5BeginMethod,
    /* xSync         */ fts5SyncMethod,
    /* xCommit       */ fts5CommitMethod,
    /* xRollback     */ fts5RollbackMethod,
    /* xFindFunction */ fts5FindFunctionMethod,
    /* xRename       */ fts5RenameMethod,
    /* xSavepoint    */ fts5SavepointMethod,
    /* xRelease      */ fts5ReleaseMethod,
    /* xRollbackTo   */ fts5RollbackToMethod,
    /* xShadowName   */ fts5ShadowName,
    /* xIntegrity    */ fts5IntegrityMethod
  };

  int rc;
  Fts5Global *pGlobal = 0;

  pGlobal = (Fts5Global*)sqlite3_malloc(sizeof(Fts5Global));
  if( pGlobal==0 ){
    rc = SQLITE_NOMEM;
  }else{
    void *p = (void*)pGlobal;
    memset(pGlobal, 0, sizeof(Fts5Global));
    pGlobal->db = db;
    pGlobal->api.iVersion = 2;
    pGlobal->api.xCreateFunction = fts5CreateAux;
    pGlobal->api.xCreateTokenizer = fts5CreateTokenizer;
    pGlobal->api.xFindTokenizer = fts5FindTokenizer;
    rc = sqlite3_create_module_v2(db, "fts5", &fts5Mod, p, fts5ModuleDestroy);
    if( rc==SQLITE_OK ) rc = sqlite3Fts5IndexInit(db);
    if( rc==SQLITE_OK ) rc = sqlite3Fts5ExprInit(pGlobal, db);
    if( rc==SQLITE_OK ) rc = sqlite3Fts5AuxInit(&pGlobal->api);
    if( rc==SQLITE_OK ) rc = sqlite3Fts5TokenizerInit(&pGlobal->api);
    if( rc==SQLITE_OK ) rc = sqlite3Fts5VocabInit(pGlobal, db);
    if( rc==SQLITE_OK ){
      rc = sqlite3_create_function(
          db, "fts5", 1, SQLITE_UTF8, p, fts5Fts5Func, 0, 0
      );
    }
    if( rc==SQLITE_OK ){
      rc = sqlite3_create_function(
          db, "fts5_source_id", 0,
          SQLITE_UTF8|SQLITE_DETERMINISTIC|SQLITE_INNOCUOUS,
          p, fts5SourceIdFunc, 0, 0
      );
    }
  }

  /* If SQLITE_FTS5_ENABLE_TEST_MI is defined, assume that the file
  ** fts5_test_mi.c is compiled and linked into the executable. And call
  ** its entry point to enable the matchinfo() demo.  */
#ifdef SQLITE_FTS5_ENABLE_TEST_MI
  if( rc==SQLITE_OK ){
    extern int sqlite3Fts5TestRegisterMatchinfo(sqlite3*);
    rc = sqlite3Fts5TestRegisterMatchinfo(db);
  }
#endif

  return rc;
}

/*
** The following functions are used to register the module with SQLite. If
** this module is being built as part of the SQLite core (SQLITE_CORE is
** defined), then sqlite3_open() will call sqlite3Fts5Init() directly.
**
** Or, if this module is being built as a loadable extension,
** sqlite3Fts5Init() is omitted and the two standard entry points
** sqlite3_fts_init() and sqlite3_fts5_init() defined instead.
*/
#ifndef SQLITE_CORE
#ifdef _WIN32
__declspec(dllexport)
#endif
SQLITE_API int sqlite3_fts_init(
  sqlite3 *db,
  char **pzErrMsg,
  const sqlite3_api_routines *pApi
){
  SQLITE_EXTENSION_INIT2(pApi);
  (void)pzErrMsg;  /* Unused parameter */
  return fts5Init(db);
}

#ifdef _WIN32
__declspec(dllexport)
#endif
SQLITE_API int sqlite3_fts5_init(
  sqlite3 *db,
  char **pzErrMsg,
  const sqlite3_api_routines *pApi
){
  SQLITE_EXTENSION_INIT2(pApi);
  (void)pzErrMsg;  /* Unused parameter */
  return fts5Init(db);
}
#else
SQLITE_PRIVATE int sqlite3Fts5Init(sqlite3 *db){
  return fts5Init(db);
}
#endif

/*
** 2014 May 31
**
** The author disclaims copyright to this source code.  In place of
** a legal notice, here is a blessing:
**
**    May you do good and not evil.
**    May you find forgiveness for yourself and forgive others.
**    May you share freely, never taking more than you give.
**
******************************************************************************
**
*/



/* #include "fts5Int.h" */

struct Fts5Storage {
  Fts5Config *pConfig;
  Fts5Index *pIndex;
  int bTotalsValid;               /* True if nTotalRow/aTotalSize[] are valid */
  i64 nTotalRow;                  /* Total number of rows in FTS table */
  i64 *aTotalSize;                /* Total sizes of each column */
  sqlite3_stmt *aStmt[11];
};


#if FTS5_STMT_SCAN_ASC!=0
# error "FTS5_STMT_SCAN_ASC mismatch"
#endif
#if FTS5_STMT_SCAN_DESC!=1
# error "FTS5_STMT_SCAN_DESC mismatch"
#endif
#if FTS5_STMT_LOOKUP!=2
# error "FTS5_STMT_LOOKUP mismatch"
#endif

#define FTS5_STMT_INSERT_CONTENT  3
#define FTS5_STMT_REPLACE_CONTENT 4
#define FTS5_STMT_DELETE_CONTENT  5
#define FTS5_STMT_REPLACE_DOCSIZE  6
#define FTS5_STMT_DELETE_DOCSIZE  7
#define FTS5_STMT_LOOKUP_DOCSIZE  8
#define FTS5_STMT_REPLACE_CONFIG 9
#define FTS5_STMT_SCAN 10

/*
** Prepare the two insert statements - Fts5Storage.pInsertContent and
** Fts5Storage.pInsertDocsize - if they have not already been prepared.
** Return SQLITE_OK if successful, or an SQLite error code if an error
** occurs.
*/
static int fts5StorageGetStmt(
  Fts5Storage *p,                 /* Storage handle */
  int eStmt,                      /* FTS5_STMT_XXX constant */
  sqlite3_stmt **ppStmt,          /* OUT: Prepared statement handle */
  char **pzErrMsg                 /* OUT: Error message (if any) */
){
  int rc = SQLITE_OK;

  /* If there is no %_docsize table, there should be no requests for
  ** statements to operate on it.  */
  assert( p->pConfig->bColumnsize || (
        eStmt!=FTS5_STMT_REPLACE_DOCSIZE
     && eStmt!=FTS5_STMT_DELETE_DOCSIZE
     && eStmt!=FTS5_STMT_LOOKUP_DOCSIZE
  ));

  assert( eStmt>=0 && eStmt<ArraySize(p->aStmt) );
  if( p->aStmt[eStmt]==0 ){
    const char *azStmt[] = {
      "SELECT %s FROM %s T WHERE T.%Q >= ? AND T.%Q <= ? ORDER BY T.%Q ASC",
      "SELECT %s FROM %s T WHERE T.%Q <= ? AND T.%Q >= ? ORDER BY T.%Q DESC",
      "SELECT %s FROM %s T WHERE T.%Q=?",               /* LOOKUP  */

      "INSERT INTO %Q.'%q_content' VALUES(%s)",         /* INSERT_CONTENT  */
      "REPLACE INTO %Q.'%q_content' VALUES(%s)",        /* REPLACE_CONTENT */
      "DELETE FROM %Q.'%q_content' WHERE id=?",         /* DELETE_CONTENT  */
      "REPLACE INTO %Q.'%q_docsize' VALUES(?,?%s)",     /* REPLACE_DOCSIZE  */
      "DELETE FROM %Q.'%q_docsize' WHERE id=?",         /* DELETE_DOCSIZE  */

      "SELECT sz%s FROM %Q.'%q_docsize' WHERE id=?",    /* LOOKUP_DOCSIZE  */

      "REPLACE INTO %Q.'%q_config' VALUES(?,?)",        /* REPLACE_CONFIG */
      "SELECT %s FROM %s AS T",                         /* SCAN */
    };
    Fts5Config *pC = p->pConfig;
    char *zSql = 0;

    switch( eStmt ){
      case FTS5_STMT_SCAN:
        zSql = sqlite3_mprintf(azStmt[eStmt],
            pC->zContentExprlist, pC->zContent
        );
        break;

      case FTS5_STMT_SCAN_ASC:
      case FTS5_STMT_SCAN_DESC:
        zSql = sqlite3_mprintf(azStmt[eStmt], pC->zContentExprlist,
            pC->zContent, pC->zContentRowid, pC->zContentRowid,
            pC->zContentRowid
        );
        break;

      case FTS5_STMT_LOOKUP:
        zSql = sqlite3_mprintf(azStmt[eStmt],
            pC->zContentExprlist, pC->zContent, pC->zContentRowid
        );
        break;

      case FTS5_STMT_INSERT_CONTENT:
      case FTS5_STMT_REPLACE_CONTENT: {
        int nCol = pC->nCol + 1;
        char *zBind;
        int i;

        zBind = sqlite3_malloc64(1 + nCol*2);
        if( zBind ){
          for(i=0; i<nCol; i++){
            zBind[i*2] = '?';
            zBind[i*2 + 1] = ',';
          }
          zBind[i*2-1] = '\0';
          zSql = sqlite3_mprintf(azStmt[eStmt], pC->zDb, pC->zName, zBind);
          sqlite3_free(zBind);
        }
        break;
      }

      case FTS5_STMT_REPLACE_DOCSIZE:
        zSql = sqlite3_mprintf(azStmt[eStmt], pC->zDb, pC->zName,
          (pC->bContentlessDelete ? ",?" : "")
        );
        break;

      case FTS5_STMT_LOOKUP_DOCSIZE:
        zSql = sqlite3_mprintf(azStmt[eStmt],
            (pC->bContentlessDelete ? ",origin" : ""),
            pC->zDb, pC->zName
        );
        break;

      default:
        zSql = sqlite3_mprintf(azStmt[eStmt], pC->zDb, pC->zName);
        break;
    }

    if( zSql==0 ){
      rc = SQLITE_NOMEM;
    }else{
      int f = SQLITE_PREPARE_PERSISTENT;
      if( eStmt>FTS5_STMT_LOOKUP ) f |= SQLITE_PREPARE_NO_VTAB;
      p->pConfig->bLock++;
      rc = sqlite3_prepare_v3(pC->db, zSql, -1, f, &p->aStmt[eStmt], 0);
      p->pConfig->bLock--;
      sqlite3_free(zSql);
      if( rc!=SQLITE_OK && pzErrMsg ){
        *pzErrMsg = sqlite3_mprintf("%s", sqlite3_errmsg(pC->db));
      }
    }
  }

  *ppStmt = p->aStmt[eStmt];
  sqlite3_reset(*ppStmt);
  return rc;
}


static int fts5ExecPrintf(
  sqlite3 *db,
  char **pzErr,
  const char *zFormat,
  ...
){
  int rc;
  va_list ap;                     /* ... printf arguments */
  char *zSql;

  va_start(ap, zFormat);
  zSql = sqlite3_vmprintf(zFormat, ap);

  if( zSql==0 ){
    rc = SQLITE_NOMEM;
  }else{
    rc = sqlite3_exec(db, zSql, 0, 0, pzErr);
    sqlite3_free(zSql);
  }

  va_end(ap);
  return rc;
}
