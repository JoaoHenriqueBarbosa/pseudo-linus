/*
** 2011 Jan 27
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
/* #include "fts3Int.h" */
#if !defined(SQLITE_CORE) || defined(SQLITE_ENABLE_FTS3)

/* #include <string.h> */
/* #include <assert.h> */

typedef struct Fts3auxTable Fts3auxTable;
typedef struct Fts3auxCursor Fts3auxCursor;

struct Fts3auxTable {
  sqlite3_vtab base;              /* Base class used by SQLite core */
  Fts3Table *pFts3Tab;
};

struct Fts3auxCursor {
  sqlite3_vtab_cursor base;       /* Base class used by SQLite core */
  Fts3MultiSegReader csr;        /* Must be right after "base" */
  Fts3SegFilter filter;
  char *zStop;
  int nStop;                      /* Byte-length of string zStop */
  int iLangid;                    /* Language id to query */
  int isEof;                      /* True if cursor is at EOF */
  sqlite3_int64 iRowid;           /* Current rowid */

  int iCol;                       /* Current value of 'col' column */
  int nStat;                      /* Size of aStat[] array */
  struct Fts3auxColstats {
    sqlite3_int64 nDoc;           /* 'documents' values for current csr row */
    sqlite3_int64 nOcc;           /* 'occurrences' values for current csr row */
  } *aStat;
};

/*
** Schema of the terms table.
*/
#define FTS3_AUX_SCHEMA \
  "CREATE TABLE x(term, col, documents, occurrences, languageid HIDDEN)"

/*
** This function does all the work for both the xConnect and xCreate methods.
** These tables have no persistent representation of their own, so xConnect
** and xCreate are identical operations.
*/
static int fts3auxConnectMethod(
  sqlite3 *db,                    /* Database connection */
  void *pUnused,                  /* Unused */
  int argc,                       /* Number of elements in argv array */
  const char * const *argv,       /* xCreate/xConnect argument array */
  sqlite3_vtab **ppVtab,          /* OUT: New sqlite3_vtab object */
  char **pzErr                    /* OUT: sqlite3_malloc'd error message */
){
  char const *zDb;                /* Name of database (e.g. "main") */
  char const *zFts3;              /* Name of fts3 table */
  int nDb;                        /* Result of strlen(zDb) */
  int nFts3;                      /* Result of strlen(zFts3) */
  sqlite3_int64 nByte;            /* Bytes of space to allocate here */
  int rc;                         /* value returned by declare_vtab() */
  Fts3auxTable *p;                /* Virtual table object to return */

  UNUSED_PARAMETER(pUnused);

  /* The user should invoke this in one of two forms:
  **
  **     CREATE VIRTUAL TABLE xxx USING fts4aux(fts4-table);
  **     CREATE VIRTUAL TABLE xxx USING fts4aux(fts4-table-db, fts4-table);
  */
  if( argc!=4 && argc!=5 ) goto bad_args;

  zDb = argv[1];
  nDb = (int)strlen(zDb);
  if( argc==5 ){
    if( nDb==4 && 0==sqlite3_strnicmp("temp", zDb, 4) ){
      zDb = argv[3];
      nDb = (int)strlen(zDb);
      zFts3 = argv[4];
    }else{
      goto bad_args;
    }
  }else{
    zFts3 = argv[3];
  }
  nFts3 = (int)strlen(zFts3);

  rc = sqlite3_declare_vtab(db, FTS3_AUX_SCHEMA);
  if( rc!=SQLITE_OK ) return rc;

  nByte = sizeof(Fts3auxTable) + sizeof(Fts3Table) + nDb + nFts3 + 2;
  p = (Fts3auxTable *)sqlite3_malloc64(nByte);
  if( !p ) return SQLITE_NOMEM;
  memset(p, 0, nByte);

  p->pFts3Tab = (Fts3Table *)&p[1];
  p->pFts3Tab->zDb = (char *)&p->pFts3Tab[1];
  p->pFts3Tab->zName = &p->pFts3Tab->zDb[nDb+1];
  p->pFts3Tab->db = db;
  p->pFts3Tab->nIndex = 1;

  memcpy((char *)p->pFts3Tab->zDb, zDb, nDb);
  memcpy((char *)p->pFts3Tab->zName, zFts3, nFts3);
  sqlite3Fts3Dequote((char *)p->pFts3Tab->zName);

  *ppVtab = (sqlite3_vtab *)p;
  return SQLITE_OK;

 bad_args:
  sqlite3Fts3ErrMsg(pzErr, "invalid arguments to fts4aux constructor");
  return SQLITE_ERROR;
}

/*
** This function does the work for both the xDisconnect and xDestroy methods.
** These tables have no persistent representation of their own, so xDisconnect
** and xDestroy are identical operations.
*/
static int fts3auxDisconnectMethod(sqlite3_vtab *pVtab){
  Fts3auxTable *p = (Fts3auxTable *)pVtab;
  Fts3Table *pFts3 = p->pFts3Tab;
  int i;

  /* Free any prepared statements held */
  for(i=0; i<SizeofArray(pFts3->aStmt); i++){
    sqlite3_finalize(pFts3->aStmt[i]);
  }
  sqlite3_free(pFts3->zSegmentsTbl);
  sqlite3_free(p);
  return SQLITE_OK;
}

#define FTS4AUX_EQ_CONSTRAINT 1
#define FTS4AUX_GE_CONSTRAINT 2
#define FTS4AUX_LE_CONSTRAINT 4

/*
** xBestIndex - Analyze a WHERE and ORDER BY clause.
*/
static int fts3auxBestIndexMethod(
  sqlite3_vtab *pVTab,
  sqlite3_index_info *pInfo
){
  int i;
  int iEq = -1;
  int iGe = -1;
  int iLe = -1;
  int iLangid = -1;
  int iNext = 1;                  /* Next free argvIndex value */

  UNUSED_PARAMETER(pVTab);

  /* This vtab delivers always results in "ORDER BY term ASC" order. */
  if( pInfo->nOrderBy==1
   && pInfo->aOrderBy[0].iColumn==0
   && pInfo->aOrderBy[0].desc==0
  ){
    pInfo->orderByConsumed = 1;
  }

  /* Search for equality and range constraints on the "term" column.
  ** And equality constraints on the hidden "languageid" column. */
  for(i=0; i<pInfo->nConstraint; i++){
    if( pInfo->aConstraint[i].usable ){
      int op = pInfo->aConstraint[i].op;
      int iCol = pInfo->aConstraint[i].iColumn;

      if( iCol==0 ){
        if( op==SQLITE_INDEX_CONSTRAINT_EQ ) iEq = i;
        if( op==SQLITE_INDEX_CONSTRAINT_LT ) iLe = i;
        if( op==SQLITE_INDEX_CONSTRAINT_LE ) iLe = i;
        if( op==SQLITE_INDEX_CONSTRAINT_GT ) iGe = i;
        if( op==SQLITE_INDEX_CONSTRAINT_GE ) iGe = i;
      }
      if( iCol==4 ){
        if( op==SQLITE_INDEX_CONSTRAINT_EQ ) iLangid = i;
      }
    }
  }

  if( iEq>=0 ){
    pInfo->idxNum = FTS4AUX_EQ_CONSTRAINT;
    pInfo->aConstraintUsage[iEq].argvIndex = iNext++;
    pInfo->estimatedCost = 5;
  }else{
    pInfo->idxNum = 0;
    pInfo->estimatedCost = 20000;
    if( iGe>=0 ){
      pInfo->idxNum += FTS4AUX_GE_CONSTRAINT;
      pInfo->aConstraintUsage[iGe].argvIndex = iNext++;
      pInfo->estimatedCost /= 2;
    }
    if( iLe>=0 ){
      pInfo->idxNum += FTS4AUX_LE_CONSTRAINT;
      pInfo->aConstraintUsage[iLe].argvIndex = iNext++;
      pInfo->estimatedCost /= 2;
    }
  }
  if( iLangid>=0 ){
    pInfo->aConstraintUsage[iLangid].argvIndex = iNext++;
    pInfo->estimatedCost--;
  }

  return SQLITE_OK;
}

/*
** xOpen - Open a cursor.
*/
static int fts3auxOpenMethod(sqlite3_vtab *pVTab, sqlite3_vtab_cursor **ppCsr){
  Fts3auxCursor *pCsr;            /* Pointer to cursor object to return */

  UNUSED_PARAMETER(pVTab);

  pCsr = (Fts3auxCursor *)sqlite3_malloc(sizeof(Fts3auxCursor));
  if( !pCsr ) return SQLITE_NOMEM;
  memset(pCsr, 0, sizeof(Fts3auxCursor));

  *ppCsr = (sqlite3_vtab_cursor *)pCsr;
  return SQLITE_OK;
}

/*
** xClose - Close a cursor.
*/
static int fts3auxCloseMethod(sqlite3_vtab_cursor *pCursor){
  Fts3Table *pFts3 = ((Fts3auxTable *)pCursor->pVtab)->pFts3Tab;
  Fts3auxCursor *pCsr = (Fts3auxCursor *)pCursor;

  sqlite3Fts3SegmentsClose(pFts3);
  sqlite3Fts3SegReaderFinish(&pCsr->csr);
  sqlite3_free((void *)pCsr->filter.zTerm);
  sqlite3_free(pCsr->zStop);
  sqlite3_free(pCsr->aStat);
  sqlite3_free(pCsr);
  return SQLITE_OK;
}

static int fts3auxGrowStatArray(Fts3auxCursor *pCsr, int nSize){
  if( nSize>pCsr->nStat ){
    struct Fts3auxColstats *aNew;
    aNew = (struct Fts3auxColstats *)sqlite3_realloc64(pCsr->aStat,
        sizeof(struct Fts3auxColstats) * nSize
    );
    if( aNew==0 ) return SQLITE_NOMEM;
    memset(&aNew[pCsr->nStat], 0,
        sizeof(struct Fts3auxColstats) * (nSize - pCsr->nStat)
    );
    pCsr->aStat = aNew;
    pCsr->nStat = nSize;
  }
  return SQLITE_OK;
}

/*
** xNext - Advance the cursor to the next row, if any.
*/
static int fts3auxNextMethod(sqlite3_vtab_cursor *pCursor){
  Fts3auxCursor *pCsr = (Fts3auxCursor *)pCursor;
  Fts3Table *pFts3 = ((Fts3auxTable *)pCursor->pVtab)->pFts3Tab;
  int rc;

  /* Increment our pretend rowid value. */
  pCsr->iRowid++;

  for(pCsr->iCol++; pCsr->iCol<pCsr->nStat; pCsr->iCol++){
    if( pCsr->aStat[pCsr->iCol].nDoc>0 ) return SQLITE_OK;
  }

  rc = sqlite3Fts3SegReaderStep(pFts3, &pCsr->csr);
  if( rc==SQLITE_ROW ){
    int i = 0;
    int nDoclist = pCsr->csr.nDoclist;
    char *aDoclist = pCsr->csr.aDoclist;
    int iCol;

    int eState = 0;

    if( pCsr->zStop ){
      int n = (pCsr->nStop<pCsr->csr.nTerm) ? pCsr->nStop : pCsr->csr.nTerm;
      int mc = memcmp(pCsr->zStop, pCsr->csr.zTerm, n);
      if( mc<0 || (mc==0 && pCsr->csr.nTerm>pCsr->nStop) ){
        pCsr->isEof = 1;
        return SQLITE_OK;
      }
    }

    if( fts3auxGrowStatArray(pCsr, 2) ) return SQLITE_NOMEM;
    memset(pCsr->aStat, 0, sizeof(struct Fts3auxColstats) * pCsr->nStat);
    iCol = 0;
    rc = SQLITE_OK;

    while( i<nDoclist ){
      sqlite3_int64 v = 0;

      i += sqlite3Fts3GetVarint(&aDoclist[i], &v);
      switch( eState ){
        /* State 0. In this state the integer just read was a docid. */
        case 0:
          pCsr->aStat[0].nDoc++;
          eState = 1;
          iCol = 0;
          break;

        /* State 1. In this state we are expecting either a 1, indicating
        ** that the following integer will be a column number, or the
        ** start of a position list for column 0.
        **
        ** The only difference between state 1 and state 2 is that if the
        ** integer encountered in state 1 is not 0 or 1, then we need to
        ** increment the column 0 "nDoc" count for this term.
        */
        case 1:
          assert( iCol==0 );
          if( v>1 ){
            pCsr->aStat[1].nDoc++;
          }
          eState = 2;
          /* fall through */

        case 2:
          if( v==0 ){       /* 0x00. Next integer will be a docid. */
            eState = 0;
          }else if( v==1 ){ /* 0x01. Next integer will be a column number. */
            eState = 3;
          }else{            /* 2 or greater. A position. */
            pCsr->aStat[iCol+1].nOcc++;
            pCsr->aStat[0].nOcc++;
          }
          break;

        /* State 3. The integer just read is a column number. */
        default: assert( eState==3 );
          iCol = (int)v;
          if( iCol<1 ){
            rc = SQLITE_CORRUPT_VTAB;
            break;
          }
          if( fts3auxGrowStatArray(pCsr, iCol+2) ) return SQLITE_NOMEM;
          pCsr->aStat[iCol+1].nDoc++;
          eState = 2;
          break;
      }
    }

    pCsr->iCol = 0;
  }else{
    pCsr->isEof = 1;
  }
  return rc;
}
