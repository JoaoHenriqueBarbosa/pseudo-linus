
/*
** The xConnect() and xCreate() methods for the virtual table. All the
** work is done in function fts5InitVtab().
*/
static int fts5ConnectMethod(
  sqlite3 *db,                    /* Database connection */
  void *pAux,                     /* Pointer to tokenizer hash table */
  int argc,                       /* Number of elements in argv array */
  const char * const *argv,       /* xCreate/xConnect argument array */
  sqlite3_vtab **ppVtab,          /* OUT: New sqlite3_vtab object */
  char **pzErr                    /* OUT: sqlite3_malloc'd error message */
){
  return fts5InitVtab(0, db, pAux, argc, argv, ppVtab, pzErr);
}
static int fts5CreateMethod(
  sqlite3 *db,                    /* Database connection */
  void *pAux,                     /* Pointer to tokenizer hash table */
  int argc,                       /* Number of elements in argv array */
  const char * const *argv,       /* xCreate/xConnect argument array */
  sqlite3_vtab **ppVtab,          /* OUT: New sqlite3_vtab object */
  char **pzErr                    /* OUT: sqlite3_malloc'd error message */
){
  return fts5InitVtab(1, db, pAux, argc, argv, ppVtab, pzErr);
}

/*
** The different query plans.
*/
#define FTS5_PLAN_MATCH          1       /* (<tbl> MATCH ?) */
#define FTS5_PLAN_SOURCE         2       /* A source cursor for SORTED_MATCH */
#define FTS5_PLAN_SPECIAL        3       /* An internal query */
#define FTS5_PLAN_SORTED_MATCH   4       /* (<tbl> MATCH ? ORDER BY rank) */
#define FTS5_PLAN_SCAN           5       /* No usable constraint */
#define FTS5_PLAN_ROWID          6       /* (rowid = ?) */

/*
** Set the SQLITE_INDEX_SCAN_UNIQUE flag in pIdxInfo->flags. Unless this
** extension is currently being used by a version of SQLite too old to
** support index-info flags. In that case this function is a no-op.
*/
static void fts5SetUniqueFlag(sqlite3_index_info *pIdxInfo){
#if SQLITE_VERSION_NUMBER>=3008012
#ifndef SQLITE_CORE
  if( sqlite3_libversion_number()>=3008012 )
#endif
  {
    pIdxInfo->idxFlags |= SQLITE_INDEX_SCAN_UNIQUE;
  }
#endif
}

static int fts5UsePatternMatch(
  Fts5Config *pConfig,
  struct sqlite3_index_constraint *p
){
  assert( FTS5_PATTERN_GLOB==SQLITE_INDEX_CONSTRAINT_GLOB );
  assert( FTS5_PATTERN_LIKE==SQLITE_INDEX_CONSTRAINT_LIKE );
  if( pConfig->ePattern==FTS5_PATTERN_GLOB && p->op==FTS5_PATTERN_GLOB ){
    return 1;
  }
  if( pConfig->ePattern==FTS5_PATTERN_LIKE
   && (p->op==FTS5_PATTERN_LIKE || p->op==FTS5_PATTERN_GLOB)
  ){
    return 1;
  }
  return 0;
}

/*
** Implementation of the xBestIndex method for FTS5 tables. Within the
** WHERE constraint, it searches for the following:
**
**   1. A MATCH constraint against the table column.
**   2. A MATCH constraint against the "rank" column.
**   3. A MATCH constraint against some other column.
**   4. An == constraint against the rowid column.
**   5. A < or <= constraint against the rowid column.
**   6. A > or >= constraint against the rowid column.
**
** Within the ORDER BY, the following are supported:
**
**   5. ORDER BY rank [ASC|DESC]
**   6. ORDER BY rowid [ASC|DESC]
**
** Information for the xFilter call is passed via both the idxNum and
** idxStr variables. Specifically, idxNum is a bitmask of the following
** flags used to encode the ORDER BY clause:
**
**     FTS5_BI_ORDER_RANK
**     FTS5_BI_ORDER_ROWID
**     FTS5_BI_ORDER_DESC
**
** idxStr is used to encode data from the WHERE clause. For each argument
** passed to the xFilter method, the following is appended to idxStr:
**
**   Match against table column:            "m"
**   Match against rank column:             "r"
**   Match against other column:            "M<column-number>"
**   LIKE  against other column:            "L<column-number>"
**   GLOB  against other column:            "G<column-number>"
**   Equality constraint against the rowid: "="
**   A < or <= against the rowid:           "<"
**   A > or >= against the rowid:           ">"
**
** This function ensures that there is at most one "r" or "=". And that if
** there exists an "=" then there is no "<" or ">".
**
** Costs are assigned as follows:
**
**  a) If an unusable MATCH operator is present in the WHERE clause, the
**     cost is unconditionally set to 1e50 (a really big number).
**
**  a) If a MATCH operator is present, the cost depends on the other
**     constraints also present. As follows:
**
**       * No other constraints:         cost=1000.0
**       * One rowid range constraint:   cost=750.0
**       * Both rowid range constraints: cost=500.0
**       * An == rowid constraint:       cost=100.0
**
**  b) Otherwise, if there is no MATCH:
**
**       * No other constraints:         cost=1000000.0
**       * One rowid range constraint:   cost=750000.0
**       * Both rowid range constraints: cost=250000.0
**       * An == rowid constraint:       cost=10.0
**
** Costs are not modified by the ORDER BY clause.
*/
static int fts5BestIndexMethod(sqlite3_vtab *pVTab, sqlite3_index_info *pInfo){
  Fts5Table *pTab = (Fts5Table*)pVTab;
  Fts5Config *pConfig = pTab->pConfig;
  const int nCol = pConfig->nCol;
  int idxFlags = 0;               /* Parameter passed through to xFilter() */
  int i;

  char *idxStr;
  int iIdxStr = 0;
  int iCons = 0;

  int bSeenEq = 0;
  int bSeenGt = 0;
  int bSeenLt = 0;
  int bSeenMatch = 0;
  int bSeenRank = 0;


  assert( SQLITE_INDEX_CONSTRAINT_EQ<SQLITE_INDEX_CONSTRAINT_MATCH );
  assert( SQLITE_INDEX_CONSTRAINT_GT<SQLITE_INDEX_CONSTRAINT_MATCH );
  assert( SQLITE_INDEX_CONSTRAINT_LE<SQLITE_INDEX_CONSTRAINT_MATCH );
  assert( SQLITE_INDEX_CONSTRAINT_GE<SQLITE_INDEX_CONSTRAINT_MATCH );
  assert( SQLITE_INDEX_CONSTRAINT_LE<SQLITE_INDEX_CONSTRAINT_MATCH );

  if( pConfig->bLock ){
    pTab->base.zErrMsg = sqlite3_mprintf(
        "recursively defined fts5 content table"
    );
    return SQLITE_ERROR;
  }

  idxStr = (char*)sqlite3_malloc(pInfo->nConstraint * 8 + 1);
  if( idxStr==0 ) return SQLITE_NOMEM;
  pInfo->idxStr = idxStr;
  pInfo->needToFreeIdxStr = 1;

  for(i=0; i<pInfo->nConstraint; i++){
    struct sqlite3_index_constraint *p = &pInfo->aConstraint[i];
    int iCol = p->iColumn;
    if( p->op==SQLITE_INDEX_CONSTRAINT_MATCH
     || (p->op==SQLITE_INDEX_CONSTRAINT_EQ && iCol>=nCol)
    ){
      /* A MATCH operator or equivalent */
      if( p->usable==0 || iCol<0 ){
        /* As there exists an unusable MATCH constraint this is an
        ** unusable plan. Set a prohibitively high cost. */
        pInfo->estimatedCost = 1e50;
        assert( iIdxStr < pInfo->nConstraint*6 + 1 );
        idxStr[iIdxStr] = 0;
        return SQLITE_OK;
      }else{
        if( iCol==nCol+1 ){
          if( bSeenRank ) continue;
          idxStr[iIdxStr++] = 'r';
          bSeenRank = 1;
        }else if( iCol>=0 ){
          bSeenMatch = 1;
          idxStr[iIdxStr++] = 'M';
          sqlite3_snprintf(6, &idxStr[iIdxStr], "%d", iCol);
          idxStr += strlen(&idxStr[iIdxStr]);
          assert( idxStr[iIdxStr]=='\0' );
        }
        pInfo->aConstraintUsage[i].argvIndex = ++iCons;
        pInfo->aConstraintUsage[i].omit = 1;
      }
    }else if( p->usable ){
      if( iCol>=0 && iCol<nCol && fts5UsePatternMatch(pConfig, p) ){
        assert( p->op==FTS5_PATTERN_LIKE || p->op==FTS5_PATTERN_GLOB );
        idxStr[iIdxStr++] = p->op==FTS5_PATTERN_LIKE ? 'L' : 'G';
        sqlite3_snprintf(6, &idxStr[iIdxStr], "%d", iCol);
        idxStr += strlen(&idxStr[iIdxStr]);
        pInfo->aConstraintUsage[i].argvIndex = ++iCons;
        assert( idxStr[iIdxStr]=='\0' );
      }else if( bSeenEq==0 && p->op==SQLITE_INDEX_CONSTRAINT_EQ && iCol<0 ){
        idxStr[iIdxStr++] = '=';
        bSeenEq = 1;
        pInfo->aConstraintUsage[i].argvIndex = ++iCons;
      }
    }
  }

  if( bSeenEq==0 ){
    for(i=0; i<pInfo->nConstraint; i++){
      struct sqlite3_index_constraint *p = &pInfo->aConstraint[i];
      if( p->iColumn<0 && p->usable ){
        int op = p->op;
        if( op==SQLITE_INDEX_CONSTRAINT_LT || op==SQLITE_INDEX_CONSTRAINT_LE ){
          if( bSeenLt ) continue;
          idxStr[iIdxStr++] = '<';
          pInfo->aConstraintUsage[i].argvIndex = ++iCons;
          bSeenLt = 1;
        }else
        if( op==SQLITE_INDEX_CONSTRAINT_GT || op==SQLITE_INDEX_CONSTRAINT_GE ){
          if( bSeenGt ) continue;
          idxStr[iIdxStr++] = '>';
          pInfo->aConstraintUsage[i].argvIndex = ++iCons;
          bSeenGt = 1;
        }
      }
    }
  }
  idxStr[iIdxStr] = '\0';

  /* Set idxFlags flags for the ORDER BY clause
  **
  ** Note that tokendata=1 tables cannot currently handle "ORDER BY rowid DESC".
  */
  if( pInfo->nOrderBy==1 ){
    int iSort = pInfo->aOrderBy[0].iColumn;
    if( iSort==(pConfig->nCol+1) && bSeenMatch ){
      idxFlags |= FTS5_BI_ORDER_RANK;
    }else if( iSort==-1 && (!pInfo->aOrderBy[0].desc || !pConfig->bTokendata) ){
      idxFlags |= FTS5_BI_ORDER_ROWID;
    }
    if( BitFlagTest(idxFlags, FTS5_BI_ORDER_RANK|FTS5_BI_ORDER_ROWID) ){
      pInfo->orderByConsumed = 1;
      if( pInfo->aOrderBy[0].desc ){
        idxFlags |= FTS5_BI_ORDER_DESC;
      }
    }
  }

  /* Calculate the estimated cost based on the flags set in idxFlags. */
  if( bSeenEq ){
    pInfo->estimatedCost = bSeenMatch ? 100.0 : 10.0;
    if( bSeenMatch==0 ) fts5SetUniqueFlag(pInfo);
  }else if( bSeenLt && bSeenGt ){
    pInfo->estimatedCost = bSeenMatch ? 500.0 : 250000.0;
  }else if( bSeenLt || bSeenGt ){
    pInfo->estimatedCost = bSeenMatch ? 750.0 : 750000.0;
  }else{
    pInfo->estimatedCost = bSeenMatch ? 1000.0 : 1000000.0;
  }

  pInfo->idxNum = idxFlags;
  return SQLITE_OK;
}

static int fts5NewTransaction(Fts5FullTable *pTab){
  Fts5Cursor *pCsr;
  for(pCsr=pTab->pGlobal->pCsr; pCsr; pCsr=pCsr->pNext){
    if( pCsr->base.pVtab==(sqlite3_vtab*)pTab ) return SQLITE_OK;
  }
  return sqlite3Fts5StorageReset(pTab->pStorage);
}

/*
** Implementation of xOpen method.
*/
static int fts5OpenMethod(sqlite3_vtab *pVTab, sqlite3_vtab_cursor **ppCsr){
  Fts5FullTable *pTab = (Fts5FullTable*)pVTab;
  Fts5Config *pConfig = pTab->p.pConfig;
  Fts5Cursor *pCsr = 0;           /* New cursor object */
  sqlite3_int64 nByte;            /* Bytes of space to allocate */
  int rc;                         /* Return code */

  rc = fts5NewTransaction(pTab);
  if( rc==SQLITE_OK ){
    nByte = sizeof(Fts5Cursor) + pConfig->nCol * sizeof(int);
    pCsr = (Fts5Cursor*)sqlite3_malloc64(nByte);
    if( pCsr ){
      Fts5Global *pGlobal = pTab->pGlobal;
      memset(pCsr, 0, (size_t)nByte);
      pCsr->aColumnSize = (int*)&pCsr[1];
      pCsr->pNext = pGlobal->pCsr;
      pGlobal->pCsr = pCsr;
      pCsr->iCsrId = ++pGlobal->iNextId;
    }else{
      rc = SQLITE_NOMEM;
    }
  }
  *ppCsr = (sqlite3_vtab_cursor*)pCsr;
  return rc;
}

static int fts5StmtType(Fts5Cursor *pCsr){
  if( pCsr->ePlan==FTS5_PLAN_SCAN ){
    return (pCsr->bDesc) ? FTS5_STMT_SCAN_DESC : FTS5_STMT_SCAN_ASC;
  }
  return FTS5_STMT_LOOKUP;
}

/*
** This function is called after the cursor passed as the only argument
** is moved to point at a different row. It clears all cached data
** specific to the previous row stored by the cursor object.
*/
static void fts5CsrNewrow(Fts5Cursor *pCsr){
  CsrFlagSet(pCsr,
      FTS5CSR_REQUIRE_CONTENT
    | FTS5CSR_REQUIRE_DOCSIZE
    | FTS5CSR_REQUIRE_INST
    | FTS5CSR_REQUIRE_POSLIST
  );
}

static void fts5FreeCursorComponents(Fts5Cursor *pCsr){
  Fts5FullTable *pTab = (Fts5FullTable*)(pCsr->base.pVtab);
  Fts5Auxdata *pData;
  Fts5Auxdata *pNext;

  sqlite3_free(pCsr->aInstIter);
  sqlite3_free(pCsr->aInst);
  if( pCsr->pStmt ){
    int eStmt = fts5StmtType(pCsr);
    sqlite3Fts5StorageStmtRelease(pTab->pStorage, eStmt, pCsr->pStmt);
  }
  if( pCsr->pSorter ){
    Fts5Sorter *pSorter = pCsr->pSorter;
    sqlite3_finalize(pSorter->pStmt);
    sqlite3_free(pSorter);
  }

  if( pCsr->ePlan!=FTS5_PLAN_SOURCE ){
    sqlite3Fts5ExprFree(pCsr->pExpr);
  }

  for(pData=pCsr->pAuxdata; pData; pData=pNext){
    pNext = pData->pNext;
    if( pData->xDelete ) pData->xDelete(pData->pPtr);
    sqlite3_free(pData);
  }

  sqlite3_finalize(pCsr->pRankArgStmt);
  sqlite3_free(pCsr->apRankArg);

  if( CsrFlagTest(pCsr, FTS5CSR_FREE_ZRANK) ){
    sqlite3_free(pCsr->zRank);
    sqlite3_free(pCsr->zRankArgs);
  }

  sqlite3Fts5IndexCloseReader(pTab->p.pIndex);
  memset(&pCsr->ePlan, 0, sizeof(Fts5Cursor) - ((u8*)&pCsr->ePlan - (u8*)pCsr));
}
