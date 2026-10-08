
/*
** 2015 May 08
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
** This is an SQLite virtual table module implementing direct access to an
** existing FTS5 index. The module may create several different types of
** tables:
**
** col:
**     CREATE TABLE vocab(term, col, doc, cnt, PRIMARY KEY(term, col));
**
**   One row for each term/column combination. The value of $doc is set to
**   the number of fts5 rows that contain at least one instance of term
**   $term within column $col. Field $cnt is set to the total number of
**   instances of term $term in column $col (in any row of the fts5 table).
**
** row:
**     CREATE TABLE vocab(term, doc, cnt, PRIMARY KEY(term));
**
**   One row for each term in the database. The value of $doc is set to
**   the number of fts5 rows that contain at least one instance of term
**   $term. Field $cnt is set to the total number of instances of term
**   $term in the database.
**
** instance:
**     CREATE TABLE vocab(term, doc, col, offset, PRIMARY KEY(<all-fields>));
**
**   One row for each term instance in the database.
*/


/* #include "fts5Int.h" */


typedef struct Fts5VocabTable Fts5VocabTable;
typedef struct Fts5VocabCursor Fts5VocabCursor;

struct Fts5VocabTable {
  sqlite3_vtab base;
  char *zFts5Tbl;                 /* Name of fts5 table */
  char *zFts5Db;                  /* Db containing fts5 table */
  sqlite3 *db;                    /* Database handle */
  Fts5Global *pGlobal;            /* FTS5 global object for this database */
  int eType;                      /* FTS5_VOCAB_COL, ROW or INSTANCE */
  unsigned bBusy;                 /* True if busy */
};

struct Fts5VocabCursor {
  sqlite3_vtab_cursor base;
  sqlite3_stmt *pStmt;            /* Statement holding lock on pIndex */
  Fts5Table *pFts5;               /* Associated FTS5 table */

  int bEof;                       /* True if this cursor is at EOF */
  Fts5IndexIter *pIter;           /* Term/rowid iterator object */
  void *pStruct;                  /* From sqlite3Fts5StructureRef() */

  int nLeTerm;                    /* Size of zLeTerm in bytes */
  char *zLeTerm;                  /* (term <= $zLeTerm) paramater, or NULL */

  /* These are used by 'col' tables only */
  int iCol;
  i64 *aCnt;
  i64 *aDoc;

  /* Output values used by all tables. */
  i64 rowid;                      /* This table's current rowid value */
  Fts5Buffer term;                /* Current value of 'term' column */

  /* Output values Used by 'instance' tables only */
  i64 iInstPos;
  int iInstOff;
};

#define FTS5_VOCAB_COL      0
#define FTS5_VOCAB_ROW      1
#define FTS5_VOCAB_INSTANCE 2

#define FTS5_VOCAB_COL_SCHEMA  "term, col, doc, cnt"
#define FTS5_VOCAB_ROW_SCHEMA  "term, doc, cnt"
#define FTS5_VOCAB_INST_SCHEMA "term, doc, col, offset"

/*
** Bits for the mask used as the idxNum value by xBestIndex/xFilter.
*/
#define FTS5_VOCAB_TERM_EQ 0x01
#define FTS5_VOCAB_TERM_GE 0x02
#define FTS5_VOCAB_TERM_LE 0x04


/*
** Translate a string containing an fts5vocab table type to an
** FTS5_VOCAB_XXX constant. If successful, set *peType to the output
** value and return SQLITE_OK. Otherwise, set *pzErr to an error message
** and return SQLITE_ERROR.
*/
static int fts5VocabTableType(const char *zType, char **pzErr, int *peType){
  int rc = SQLITE_OK;
  char *zCopy = sqlite3Fts5Strndup(&rc, zType, -1);
  if( rc==SQLITE_OK ){
    sqlite3Fts5Dequote(zCopy);
    if( sqlite3_stricmp(zCopy, "col")==0 ){
      *peType = FTS5_VOCAB_COL;
    }else

    if( sqlite3_stricmp(zCopy, "row")==0 ){
      *peType = FTS5_VOCAB_ROW;
    }else
    if( sqlite3_stricmp(zCopy, "instance")==0 ){
      *peType = FTS5_VOCAB_INSTANCE;
    }else
    {
      *pzErr = sqlite3_mprintf("fts5vocab: unknown table type: %Q", zCopy);
      rc = SQLITE_ERROR;
    }
    sqlite3_free(zCopy);
  }

  return rc;
}


/*
** The xDisconnect() virtual table method.
*/
static int fts5VocabDisconnectMethod(sqlite3_vtab *pVtab){
  Fts5VocabTable *pTab = (Fts5VocabTable*)pVtab;
  sqlite3_free(pTab);
  return SQLITE_OK;
}

/*
** The xDestroy() virtual table method.
*/
static int fts5VocabDestroyMethod(sqlite3_vtab *pVtab){
  Fts5VocabTable *pTab = (Fts5VocabTable*)pVtab;
  sqlite3_free(pTab);
  return SQLITE_OK;
}

/*
** This function is the implementation of both the xConnect and xCreate
** methods of the FTS3 virtual table.
**
** The argv[] array contains the following:
**
**   argv[0]   -> module name  ("fts5vocab")
**   argv[1]   -> database name
**   argv[2]   -> table name
**
** then:
**
**   argv[3]   -> name of fts5 table
**   argv[4]   -> type of fts5vocab table
**
** or, for tables in the TEMP schema only.
**
**   argv[3]   -> name of fts5 tables database
**   argv[4]   -> name of fts5 table
**   argv[5]   -> type of fts5vocab table
*/
static int fts5VocabInitVtab(
  sqlite3 *db,                    /* The SQLite database connection */
  void *pAux,                     /* Pointer to Fts5Global object */
  int argc,                       /* Number of elements in argv array */
  const char * const *argv,       /* xCreate/xConnect argument array */
  sqlite3_vtab **ppVTab,          /* Write the resulting vtab structure here */
  char **pzErr                    /* Write any error message here */
){
  const char *azSchema[] = {
    "CREATE TABlE vocab(" FTS5_VOCAB_COL_SCHEMA  ")",
    "CREATE TABlE vocab(" FTS5_VOCAB_ROW_SCHEMA  ")",
    "CREATE TABlE vocab(" FTS5_VOCAB_INST_SCHEMA ")"
  };

  Fts5VocabTable *pRet = 0;
  int rc = SQLITE_OK;             /* Return code */
  int bDb;

  bDb = (argc==6 && strlen(argv[1])==4 && memcmp("temp", argv[1], 4)==0);

  if( argc!=5 && bDb==0 ){
    *pzErr = sqlite3_mprintf("wrong number of vtable arguments");
    rc = SQLITE_ERROR;
  }else{
    int nByte;                      /* Bytes of space to allocate */
    const char *zDb = bDb ? argv[3] : argv[1];
    const char *zTab = bDb ? argv[4] : argv[3];
    const char *zType = bDb ? argv[5] : argv[4];
    int nDb = (int)strlen(zDb)+1;
    int nTab = (int)strlen(zTab)+1;
    int eType = 0;

    rc = fts5VocabTableType(zType, pzErr, &eType);
    if( rc==SQLITE_OK ){
      assert( eType>=0 && eType<ArraySize(azSchema) );
      rc = sqlite3_declare_vtab(db, azSchema[eType]);
    }

    nByte = sizeof(Fts5VocabTable) + nDb + nTab;
    pRet = sqlite3Fts5MallocZero(&rc, nByte);
    if( pRet ){
      pRet->pGlobal = (Fts5Global*)pAux;
      pRet->eType = eType;
      pRet->db = db;
      pRet->zFts5Tbl = (char*)&pRet[1];
      pRet->zFts5Db = &pRet->zFts5Tbl[nTab];
      memcpy(pRet->zFts5Tbl, zTab, nTab);
      memcpy(pRet->zFts5Db, zDb, nDb);
      sqlite3Fts5Dequote(pRet->zFts5Tbl);
      sqlite3Fts5Dequote(pRet->zFts5Db);
    }
  }

  *ppVTab = (sqlite3_vtab*)pRet;
  return rc;
}


/*
** The xConnect() and xCreate() methods for the virtual table. All the
** work is done in function fts5VocabInitVtab().
*/
static int fts5VocabConnectMethod(
  sqlite3 *db,                    /* Database connection */
  void *pAux,                     /* Pointer to tokenizer hash table */
  int argc,                       /* Number of elements in argv array */
  const char * const *argv,       /* xCreate/xConnect argument array */
  sqlite3_vtab **ppVtab,          /* OUT: New sqlite3_vtab object */
  char **pzErr                    /* OUT: sqlite3_malloc'd error message */
){
  return fts5VocabInitVtab(db, pAux, argc, argv, ppVtab, pzErr);
}
static int fts5VocabCreateMethod(
  sqlite3 *db,                    /* Database connection */
  void *pAux,                     /* Pointer to tokenizer hash table */
  int argc,                       /* Number of elements in argv array */
  const char * const *argv,       /* xCreate/xConnect argument array */
  sqlite3_vtab **ppVtab,          /* OUT: New sqlite3_vtab object */
  char **pzErr                    /* OUT: sqlite3_malloc'd error message */
){
  return fts5VocabInitVtab(db, pAux, argc, argv, ppVtab, pzErr);
}

/*
** Implementation of the xBestIndex method.
**
** Only constraints of the form:
**
**     term <= ?
**     term == ?
**     term >= ?
**
** are interpreted. Less-than and less-than-or-equal are treated
** identically, as are greater-than and greater-than-or-equal.
*/
static int fts5VocabBestIndexMethod(
  sqlite3_vtab *pUnused,
  sqlite3_index_info *pInfo
){
  int i;
  int iTermEq = -1;
  int iTermGe = -1;
  int iTermLe = -1;
  int idxNum = 0;
  int nArg = 0;

  UNUSED_PARAM(pUnused);

  for(i=0; i<pInfo->nConstraint; i++){
    struct sqlite3_index_constraint *p = &pInfo->aConstraint[i];
    if( p->usable==0 ) continue;
    if( p->iColumn==0 ){          /* term column */
      if( p->op==SQLITE_INDEX_CONSTRAINT_EQ ) iTermEq = i;
      if( p->op==SQLITE_INDEX_CONSTRAINT_LE ) iTermLe = i;
      if( p->op==SQLITE_INDEX_CONSTRAINT_LT ) iTermLe = i;
      if( p->op==SQLITE_INDEX_CONSTRAINT_GE ) iTermGe = i;
      if( p->op==SQLITE_INDEX_CONSTRAINT_GT ) iTermGe = i;
    }
  }

  if( iTermEq>=0 ){
    idxNum |= FTS5_VOCAB_TERM_EQ;
    pInfo->aConstraintUsage[iTermEq].argvIndex = ++nArg;
    pInfo->estimatedCost = 100;
  }else{
    pInfo->estimatedCost = 1000000;
    if( iTermGe>=0 ){
      idxNum |= FTS5_VOCAB_TERM_GE;
      pInfo->aConstraintUsage[iTermGe].argvIndex = ++nArg;
      pInfo->estimatedCost = pInfo->estimatedCost / 2;
    }
    if( iTermLe>=0 ){
      idxNum |= FTS5_VOCAB_TERM_LE;
      pInfo->aConstraintUsage[iTermLe].argvIndex = ++nArg;
      pInfo->estimatedCost = pInfo->estimatedCost / 2;
    }
  }

  /* This virtual table always delivers results in ascending order of
  ** the "term" column (column 0). So if the user has requested this
  ** specifically - "ORDER BY term" or "ORDER BY term ASC" - set the
  ** sqlite3_index_info.orderByConsumed flag to tell the core the results
  ** are already in sorted order.  */
  if( pInfo->nOrderBy==1
   && pInfo->aOrderBy[0].iColumn==0
   && pInfo->aOrderBy[0].desc==0
  ){
    pInfo->orderByConsumed = 1;
  }

  pInfo->idxNum = idxNum;
  return SQLITE_OK;
}

/*
** Implementation of xOpen method.
*/
static int fts5VocabOpenMethod(
  sqlite3_vtab *pVTab,
  sqlite3_vtab_cursor **ppCsr
){
  Fts5VocabTable *pTab = (Fts5VocabTable*)pVTab;
  Fts5Table *pFts5 = 0;
  Fts5VocabCursor *pCsr = 0;
  int rc = SQLITE_OK;
  sqlite3_stmt *pStmt = 0;
  char *zSql = 0;

  if( pTab->bBusy ){
    pVTab->zErrMsg = sqlite3_mprintf(
       "recursive definition for %s.%s", pTab->zFts5Db, pTab->zFts5Tbl
    );
    return SQLITE_ERROR;
  }
  zSql = sqlite3Fts5Mprintf(&rc,
      "SELECT t.%Q FROM %Q.%Q AS t WHERE t.%Q MATCH '*id'",
      pTab->zFts5Tbl, pTab->zFts5Db, pTab->zFts5Tbl, pTab->zFts5Tbl
  );
  if( zSql ){
    rc = sqlite3_prepare_v2(pTab->db, zSql, -1, &pStmt, 0);
  }
  sqlite3_free(zSql);
  assert( rc==SQLITE_OK || pStmt==0 );
  if( rc==SQLITE_ERROR ) rc = SQLITE_OK;

  pTab->bBusy = 1;
  if( pStmt && sqlite3_step(pStmt)==SQLITE_ROW ){
    i64 iId = sqlite3_column_int64(pStmt, 0);
    pFts5 = sqlite3Fts5TableFromCsrid(pTab->pGlobal, iId);
  }
  pTab->bBusy = 0;

  if( rc==SQLITE_OK ){
    if( pFts5==0 ){
      rc = sqlite3_finalize(pStmt);
      pStmt = 0;
      if( rc==SQLITE_OK ){
        pVTab->zErrMsg = sqlite3_mprintf(
            "no such fts5 table: %s.%s", pTab->zFts5Db, pTab->zFts5Tbl
            );
        rc = SQLITE_ERROR;
      }
    }else{
      rc = sqlite3Fts5FlushToDisk(pFts5);
    }
  }

  if( rc==SQLITE_OK ){
    i64 nByte = pFts5->pConfig->nCol * sizeof(i64)*2 + sizeof(Fts5VocabCursor);
    pCsr = (Fts5VocabCursor*)sqlite3Fts5MallocZero(&rc, nByte);
  }

  if( pCsr ){
    pCsr->pFts5 = pFts5;
    pCsr->pStmt = pStmt;
    pCsr->aCnt = (i64*)&pCsr[1];
    pCsr->aDoc = &pCsr->aCnt[pFts5->pConfig->nCol];
  }else{
    sqlite3_finalize(pStmt);
  }

  *ppCsr = (sqlite3_vtab_cursor*)pCsr;
  return rc;
}
