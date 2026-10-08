#endif /* SQLITE_TEST || SQLITE_FTS5_DEBUG */

#if defined(SQLITE_TEST) || defined(SQLITE_FTS5_DEBUG)
/*
** The implementation of user-defined scalar function fts5_rowid().
*/
static void fts5RowidFunction(
  sqlite3_context *pCtx,          /* Function call context */
  int nArg,                       /* Number of args (always 2) */
  sqlite3_value **apVal           /* Function arguments */
){
  const char *zArg;
  if( nArg==0 ){
    sqlite3_result_error(pCtx, "should be: fts5_rowid(subject, ....)", -1);
  }else{
    zArg = (const char*)sqlite3_value_text(apVal[0]);
    if( 0==sqlite3_stricmp(zArg, "segment") ){
      i64 iRowid;
      int segid, pgno;
      if( nArg!=3 ){
        sqlite3_result_error(pCtx,
            "should be: fts5_rowid('segment', segid, pgno))", -1
        );
      }else{
        segid = sqlite3_value_int(apVal[1]);
        pgno = sqlite3_value_int(apVal[2]);
        iRowid = FTS5_SEGMENT_ROWID(segid, pgno);
        sqlite3_result_int64(pCtx, iRowid);
      }
    }else{
      sqlite3_result_error(pCtx,
        "first arg to fts5_rowid() must be 'segment'" , -1
      );
    }
  }
}
#endif /* SQLITE_TEST || SQLITE_FTS5_DEBUG */

#if defined(SQLITE_TEST) || defined(SQLITE_FTS5_DEBUG)

typedef struct Fts5StructVtab Fts5StructVtab;
struct Fts5StructVtab {
  sqlite3_vtab base;
};

typedef struct Fts5StructVcsr Fts5StructVcsr;
struct Fts5StructVcsr {
  sqlite3_vtab_cursor base;
  Fts5Structure *pStruct;
  int iLevel;
  int iSeg;
  int iRowid;
};

/*
** Create a new fts5_structure() table-valued function.
*/
static int fts5structConnectMethod(
  sqlite3 *db,
  void *pAux,
  int argc, const char *const*argv,
  sqlite3_vtab **ppVtab,
  char **pzErr
){
  Fts5StructVtab *pNew = 0;
  int rc = SQLITE_OK;

  rc = sqlite3_declare_vtab(db,
      "CREATE TABLE xyz("
          "level, segment, merge, segid, leaf1, leaf2, loc1, loc2, "
          "npgtombstone, nentrytombstone, nentry, struct HIDDEN);"
  );
  if( rc==SQLITE_OK ){
    pNew = sqlite3Fts5MallocZero(&rc, sizeof(*pNew));
  }

  *ppVtab = (sqlite3_vtab*)pNew;
  return rc;
}

/*
** We must have a single struct=? constraint that will be passed through
** into the xFilter method.  If there is no valid stmt=? constraint,
** then return an SQLITE_CONSTRAINT error.
*/
static int fts5structBestIndexMethod(
  sqlite3_vtab *tab,
  sqlite3_index_info *pIdxInfo
){
  int i;
  int rc = SQLITE_CONSTRAINT;
  struct sqlite3_index_constraint *p;
  pIdxInfo->estimatedCost = (double)100;
  pIdxInfo->estimatedRows = 100;
  pIdxInfo->idxNum = 0;
  for(i=0, p=pIdxInfo->aConstraint; i<pIdxInfo->nConstraint; i++, p++){
    if( p->usable==0 ) continue;
    if( p->op==SQLITE_INDEX_CONSTRAINT_EQ && p->iColumn==11 ){
      rc = SQLITE_OK;
      pIdxInfo->aConstraintUsage[i].omit = 1;
      pIdxInfo->aConstraintUsage[i].argvIndex = 1;
      break;
    }
  }
  return rc;
}

/*
** This method is the destructor for bytecodevtab objects.
*/
static int fts5structDisconnectMethod(sqlite3_vtab *pVtab){
  Fts5StructVtab *p = (Fts5StructVtab*)pVtab;
  sqlite3_free(p);
  return SQLITE_OK;
}

/*
** Constructor for a new bytecodevtab_cursor object.
*/
static int fts5structOpenMethod(sqlite3_vtab *p, sqlite3_vtab_cursor **ppCsr){
  int rc = SQLITE_OK;
  Fts5StructVcsr *pNew = 0;

  pNew = sqlite3Fts5MallocZero(&rc, sizeof(*pNew));
  *ppCsr = (sqlite3_vtab_cursor*)pNew;

  return SQLITE_OK;
}

/*
** Destructor for a bytecodevtab_cursor.
*/
static int fts5structCloseMethod(sqlite3_vtab_cursor *cur){
  Fts5StructVcsr *pCsr = (Fts5StructVcsr*)cur;
  fts5StructureRelease(pCsr->pStruct);
  sqlite3_free(pCsr);
  return SQLITE_OK;
}


/*
** Advance a bytecodevtab_cursor to its next row of output.
*/
static int fts5structNextMethod(sqlite3_vtab_cursor *cur){
  Fts5StructVcsr *pCsr = (Fts5StructVcsr*)cur;
  Fts5Structure *p = pCsr->pStruct;

  assert( pCsr->pStruct );
  pCsr->iSeg++;
  pCsr->iRowid++;
  while( pCsr->iLevel<p->nLevel && pCsr->iSeg>=p->aLevel[pCsr->iLevel].nSeg ){
    pCsr->iLevel++;
    pCsr->iSeg = 0;
  }
  if( pCsr->iLevel>=p->nLevel ){
    fts5StructureRelease(pCsr->pStruct);
    pCsr->pStruct = 0;
  }
  return SQLITE_OK;
}

/*
** Return TRUE if the cursor has been moved off of the last
** row of output.
*/
static int fts5structEofMethod(sqlite3_vtab_cursor *cur){
  Fts5StructVcsr *pCsr = (Fts5StructVcsr*)cur;
  return pCsr->pStruct==0;
}

static int fts5structRowidMethod(
  sqlite3_vtab_cursor *cur,
  sqlite_int64 *piRowid
){
  Fts5StructVcsr *pCsr = (Fts5StructVcsr*)cur;
  *piRowid = pCsr->iRowid;
  return SQLITE_OK;
}

/*
** Return values of columns for the row at which the bytecodevtab_cursor
** is currently pointing.
*/
static int fts5structColumnMethod(
  sqlite3_vtab_cursor *cur,   /* The cursor */
  sqlite3_context *ctx,       /* First argument to sqlite3_result_...() */
  int i                       /* Which column to return */
){
  Fts5StructVcsr *pCsr = (Fts5StructVcsr*)cur;
  Fts5Structure *p = pCsr->pStruct;
  Fts5StructureSegment *pSeg = &p->aLevel[pCsr->iLevel].aSeg[pCsr->iSeg];

  switch( i ){
    case 0: /* level */
      sqlite3_result_int(ctx, pCsr->iLevel);
      break;
    case 1: /* segment */
      sqlite3_result_int(ctx, pCsr->iSeg);
      break;
    case 2: /* merge */
      sqlite3_result_int(ctx, pCsr->iSeg < p->aLevel[pCsr->iLevel].nMerge);
      break;
    case 3: /* segid */
      sqlite3_result_int(ctx, pSeg->iSegid);
      break;
    case 4: /* leaf1 */
      sqlite3_result_int(ctx, pSeg->pgnoFirst);
      break;
    case 5: /* leaf2 */
      sqlite3_result_int(ctx, pSeg->pgnoLast);
      break;
    case 6: /* origin1 */
      sqlite3_result_int64(ctx, pSeg->iOrigin1);
      break;
    case 7: /* origin2 */
      sqlite3_result_int64(ctx, pSeg->iOrigin2);
      break;
    case 8: /* npgtombstone */
      sqlite3_result_int(ctx, pSeg->nPgTombstone);
      break;
    case 9: /* nentrytombstone */
      sqlite3_result_int64(ctx, pSeg->nEntryTombstone);
      break;
    case 10: /* nentry */
      sqlite3_result_int64(ctx, pSeg->nEntry);
      break;
  }
  return SQLITE_OK;
}

/*
** Initialize a cursor.
**
**    idxNum==0     means show all subprograms
**    idxNum==1     means show only the main bytecode and omit subprograms.
*/
static int fts5structFilterMethod(
  sqlite3_vtab_cursor *pVtabCursor,
  int idxNum, const char *idxStr,
  int argc, sqlite3_value **argv
){
  Fts5StructVcsr *pCsr = (Fts5StructVcsr *)pVtabCursor;
  int rc = SQLITE_OK;

  const u8 *aBlob = 0;
  int nBlob = 0;

  assert( argc==1 );
  fts5StructureRelease(pCsr->pStruct);
  pCsr->pStruct = 0;

  nBlob = sqlite3_value_bytes(argv[0]);
  aBlob = (const u8*)sqlite3_value_blob(argv[0]);
  rc = fts5StructureDecode(aBlob, nBlob, 0, &pCsr->pStruct);
  if( rc==SQLITE_OK ){
    pCsr->iLevel = 0;
    pCsr->iRowid = 0;
    pCsr->iSeg = -1;
    rc = fts5structNextMethod(pVtabCursor);
  }

  return rc;
}

#endif /* SQLITE_TEST || SQLITE_FTS5_DEBUG */

/*
** This is called as part of registering the FTS5 module with database
** connection db. It registers several user-defined scalar functions useful
** with FTS5.
**
** If successful, SQLITE_OK is returned. If an error occurs, some other
** SQLite error code is returned instead.
*/
static int sqlite3Fts5IndexInit(sqlite3 *db){
#if defined(SQLITE_TEST) || defined(SQLITE_FTS5_DEBUG)
  int rc = sqlite3_create_function(
      db, "fts5_decode", 2, SQLITE_UTF8, 0, fts5DecodeFunction, 0, 0
  );

  if( rc==SQLITE_OK ){
    rc = sqlite3_create_function(
        db, "fts5_decode_none", 2,
        SQLITE_UTF8, (void*)db, fts5DecodeFunction, 0, 0
    );
  }

  if( rc==SQLITE_OK ){
    rc = sqlite3_create_function(
        db, "fts5_rowid", -1, SQLITE_UTF8, 0, fts5RowidFunction, 0, 0
    );
  }

  if( rc==SQLITE_OK ){
    static const sqlite3_module fts5structure_module = {
      0,                           /* iVersion      */
      0,                           /* xCreate       */
      fts5structConnectMethod,     /* xConnect      */
      fts5structBestIndexMethod,   /* xBestIndex    */
      fts5structDisconnectMethod,  /* xDisconnect   */
      0,                           /* xDestroy      */
      fts5structOpenMethod,        /* xOpen         */
      fts5structCloseMethod,       /* xClose        */
      fts5structFilterMethod,      /* xFilter       */
      fts5structNextMethod,        /* xNext         */
      fts5structEofMethod,         /* xEof          */
      fts5structColumnMethod,      /* xColumn       */
      fts5structRowidMethod,       /* xRowid        */
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
    rc = sqlite3_create_module(db, "fts5_structure", &fts5structure_module, 0);
  }
  return rc;
#else
  return SQLITE_OK;
  UNUSED_PARAM(db);
#endif
}


static int sqlite3Fts5IndexReset(Fts5Index *p){
  assert( p->pStruct==0 || p->iStructVersion!=0 );
  if( fts5IndexDataVersion(p)!=p->iStructVersion ){
    fts5StructureInvalidate(p);
  }
  return fts5IndexReturn(p);
}

/*
** 2014 Jun 09
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
** This is an SQLite module implementing full-text search.
*/


/* #include "fts5Int.h" */

/*
** This variable is set to false when running tests for which the on disk
** structures should not be corrupt. Otherwise, true. If it is false, extra
** assert() conditions in the fts5 code are activated - conditions that are
** only true if it is guaranteed that the fts5 database is not corrupt.
*/
#ifdef SQLITE_DEBUG
SQLITE_API int sqlite3_fts5_may_be_corrupt = 1;
#endif


typedef struct Fts5Auxdata Fts5Auxdata;
typedef struct Fts5Auxiliary Fts5Auxiliary;
typedef struct Fts5Cursor Fts5Cursor;
typedef struct Fts5FullTable Fts5FullTable;
typedef struct Fts5Sorter Fts5Sorter;
typedef struct Fts5TokenizerModule Fts5TokenizerModule;

/*
** NOTES ON TRANSACTIONS:
**
** SQLite invokes the following virtual table methods as transactions are
** opened and closed by the user:
**
**     xBegin():    Start of a new transaction.
**     xSync():     Initial part of two-phase commit.
**     xCommit():   Final part of two-phase commit.
**     xRollback(): Rollback the transaction.
**
** Anything that is required as part of a commit that may fail is performed
** in the xSync() callback. Current versions of SQLite ignore any errors
** returned by xCommit().
**
** And as sub-transactions are opened/closed:
**
**     xSavepoint(int S):  Open savepoint S.
**     xRelease(int S):    Commit and close savepoint S.
**     xRollbackTo(int S): Rollback to start of savepoint S.
**
** During a write-transaction the fts5_index.c module may cache some data
** in-memory. It is flushed to disk whenever xSync(), xRelease() or
** xSavepoint() is called. And discarded whenever xRollback() or xRollbackTo()
** is called.
**
** Additionally, if SQLITE_DEBUG is defined, an instance of the following
** structure is used to record the current transaction state. This information
** is not required, but it is used in the assert() statements executed by
** function fts5CheckTransactionState() (see below).
*/
struct Fts5TransactionState {
  int eState;                     /* 0==closed, 1==open, 2==synced */
  int iSavepoint;                 /* Number of open savepoints (0 -> none) */
};
