

static int fts5CursorParseRank(
  Fts5Config *pConfig,
  Fts5Cursor *pCsr,
  sqlite3_value *pRank
){
  int rc = SQLITE_OK;
  if( pRank ){
    const char *z = (const char*)sqlite3_value_text(pRank);
    char *zRank = 0;
    char *zRankArgs = 0;

    if( z==0 ){
      if( sqlite3_value_type(pRank)==SQLITE_NULL ) rc = SQLITE_ERROR;
    }else{
      rc = sqlite3Fts5ConfigParseRank(z, &zRank, &zRankArgs);
    }
    if( rc==SQLITE_OK ){
      pCsr->zRank = zRank;
      pCsr->zRankArgs = zRankArgs;
      CsrFlagSet(pCsr, FTS5CSR_FREE_ZRANK);
    }else if( rc==SQLITE_ERROR ){
      pCsr->base.pVtab->zErrMsg = sqlite3_mprintf(
          "parse error in rank function: %s", z
      );
    }
  }else{
    if( pConfig->zRank ){
      pCsr->zRank = (char*)pConfig->zRank;
      pCsr->zRankArgs = (char*)pConfig->zRankArgs;
    }else{
      pCsr->zRank = (char*)FTS5_DEFAULT_RANK;
      pCsr->zRankArgs = 0;
    }
  }
  return rc;
}

static i64 fts5GetRowidLimit(sqlite3_value *pVal, i64 iDefault){
  if( pVal ){
    int eType = sqlite3_value_numeric_type(pVal);
    if( eType==SQLITE_INTEGER ){
      return sqlite3_value_int64(pVal);
    }
  }
  return iDefault;
}

/*
** This is the xFilter interface for the virtual table.  See
** the virtual table xFilter method documentation for additional
** information.
**
** There are three possible query strategies:
**
**   1. Full-text search using a MATCH operator.
**   2. A by-rowid lookup.
**   3. A full-table scan.
*/
static int fts5FilterMethod(
  sqlite3_vtab_cursor *pCursor,   /* The cursor used for this query */
  int idxNum,                     /* Strategy index */
  const char *idxStr,             /* Unused */
  int nVal,                       /* Number of elements in apVal */
  sqlite3_value **apVal           /* Arguments for the indexing scheme */
){
  Fts5FullTable *pTab = (Fts5FullTable*)(pCursor->pVtab);
  Fts5Config *pConfig = pTab->p.pConfig;
  Fts5Cursor *pCsr = (Fts5Cursor*)pCursor;
  int rc = SQLITE_OK;             /* Error code */
  int bDesc;                      /* True if ORDER BY [rank|rowid] DESC */
  int bOrderByRank;               /* True if ORDER BY rank */
  sqlite3_value *pRank = 0;       /* rank MATCH ? expression (or NULL) */
  sqlite3_value *pRowidEq = 0;    /* rowid = ? expression (or NULL) */
  sqlite3_value *pRowidLe = 0;    /* rowid <= ? expression (or NULL) */
  sqlite3_value *pRowidGe = 0;    /* rowid >= ? expression (or NULL) */
  int iCol;                       /* Column on LHS of MATCH operator */
  char **pzErrmsg = pConfig->pzErrmsg;
  int i;
  int iIdxStr = 0;
  Fts5Expr *pExpr = 0;

  if( pConfig->bLock ){
    pTab->p.base.zErrMsg = sqlite3_mprintf(
        "recursively defined fts5 content table"
    );
    return SQLITE_ERROR;
  }

  if( pCsr->ePlan ){
    fts5FreeCursorComponents(pCsr);
    memset(&pCsr->ePlan, 0, sizeof(Fts5Cursor) - ((u8*)&pCsr->ePlan-(u8*)pCsr));
  }

  assert( pCsr->pStmt==0 );
  assert( pCsr->pExpr==0 );
  assert( pCsr->csrflags==0 );
  assert( pCsr->pRank==0 );
  assert( pCsr->zRank==0 );
  assert( pCsr->zRankArgs==0 );
  assert( pTab->pSortCsr==0 || nVal==0 );

  assert( pzErrmsg==0 || pzErrmsg==&pTab->p.base.zErrMsg );
  pConfig->pzErrmsg = &pTab->p.base.zErrMsg;

  /* Decode the arguments passed through to this function. */
  for(i=0; i<nVal; i++){
    switch( idxStr[iIdxStr++] ){
      case 'r':
        pRank = apVal[i];
        break;
      case 'M': {
        const char *zText = (const char*)sqlite3_value_text(apVal[i]);
        if( zText==0 ) zText = "";
        iCol = 0;
        do{
          iCol = iCol*10 + (idxStr[iIdxStr]-'0');
          iIdxStr++;
        }while( idxStr[iIdxStr]>='0' && idxStr[iIdxStr]<='9' );

        if( zText[0]=='*' ){
          /* The user has issued a query of the form "MATCH '*...'". This
          ** indicates that the MATCH expression is not a full text query,
          ** but a request for an internal parameter.  */
          rc = fts5SpecialMatch(pTab, pCsr, &zText[1]);
          goto filter_out;
        }else{
          char **pzErr = &pTab->p.base.zErrMsg;
          rc = sqlite3Fts5ExprNew(pConfig, 0, iCol, zText, &pExpr, pzErr);
          if( rc==SQLITE_OK ){
            rc = sqlite3Fts5ExprAnd(&pCsr->pExpr, pExpr);
            pExpr = 0;
          }
          if( rc!=SQLITE_OK ) goto filter_out;
        }

        break;
      }
      case 'L':
      case 'G': {
        int bGlob = (idxStr[iIdxStr-1]=='G');
        const char *zText = (const char*)sqlite3_value_text(apVal[i]);
        iCol = 0;
        do{
          iCol = iCol*10 + (idxStr[iIdxStr]-'0');
          iIdxStr++;
        }while( idxStr[iIdxStr]>='0' && idxStr[iIdxStr]<='9' );
        if( zText ){
          rc = sqlite3Fts5ExprPattern(pConfig, bGlob, iCol, zText, &pExpr);
        }
        if( rc==SQLITE_OK ){
          rc = sqlite3Fts5ExprAnd(&pCsr->pExpr, pExpr);
          pExpr = 0;
        }
        if( rc!=SQLITE_OK ) goto filter_out;
        break;
      }
      case '=':
        pRowidEq = apVal[i];
        break;
      case '<':
        pRowidLe = apVal[i];
        break;
      default: assert( idxStr[iIdxStr-1]=='>' );
        pRowidGe = apVal[i];
        break;
    }
  }
  bOrderByRank = ((idxNum & FTS5_BI_ORDER_RANK) ? 1 : 0);
  pCsr->bDesc = bDesc = ((idxNum & FTS5_BI_ORDER_DESC) ? 1 : 0);

  /* Set the cursor upper and lower rowid limits. Only some strategies
  ** actually use them. This is ok, as the xBestIndex() method leaves the
  ** sqlite3_index_constraint.omit flag clear for range constraints
  ** on the rowid field.  */
  if( pRowidEq ){
    pRowidLe = pRowidGe = pRowidEq;
  }
  if( bDesc ){
    pCsr->iFirstRowid = fts5GetRowidLimit(pRowidLe, LARGEST_INT64);
    pCsr->iLastRowid = fts5GetRowidLimit(pRowidGe, SMALLEST_INT64);
  }else{
    pCsr->iLastRowid = fts5GetRowidLimit(pRowidLe, LARGEST_INT64);
    pCsr->iFirstRowid = fts5GetRowidLimit(pRowidGe, SMALLEST_INT64);
  }

  rc = sqlite3Fts5IndexLoadConfig(pTab->p.pIndex);
  if( rc!=SQLITE_OK ) goto filter_out;

  if( pTab->pSortCsr ){
    /* If pSortCsr is non-NULL, then this call is being made as part of
    ** processing for a "... MATCH <expr> ORDER BY rank" query (ePlan is
    ** set to FTS5_PLAN_SORTED_MATCH). pSortCsr is the cursor that will
    ** return results to the user for this query. The current cursor
    ** (pCursor) is used to execute the query issued by function
    ** fts5CursorFirstSorted() above.  */
    assert( pRowidEq==0 && pRowidLe==0 && pRowidGe==0 && pRank==0 );
    assert( nVal==0 && bOrderByRank==0 && bDesc==0 );
    assert( pCsr->iLastRowid==LARGEST_INT64 );
    assert( pCsr->iFirstRowid==SMALLEST_INT64 );
    if( pTab->pSortCsr->bDesc ){
      pCsr->iLastRowid = pTab->pSortCsr->iFirstRowid;
      pCsr->iFirstRowid = pTab->pSortCsr->iLastRowid;
    }else{
      pCsr->iLastRowid = pTab->pSortCsr->iLastRowid;
      pCsr->iFirstRowid = pTab->pSortCsr->iFirstRowid;
    }
    pCsr->ePlan = FTS5_PLAN_SOURCE;
    pCsr->pExpr = pTab->pSortCsr->pExpr;
    rc = fts5CursorFirst(pTab, pCsr, bDesc);
  }else if( pCsr->pExpr ){
    assert( rc==SQLITE_OK );
    rc = fts5CursorParseRank(pConfig, pCsr, pRank);
    if( rc==SQLITE_OK ){
      if( bOrderByRank ){
        pCsr->ePlan = FTS5_PLAN_SORTED_MATCH;
        rc = fts5CursorFirstSorted(pTab, pCsr, bDesc);
      }else{
        pCsr->ePlan = FTS5_PLAN_MATCH;
        rc = fts5CursorFirst(pTab, pCsr, bDesc);
      }
    }
  }else if( pConfig->zContent==0 ){
    *pConfig->pzErrmsg = sqlite3_mprintf(
        "%s: table does not support scanning", pConfig->zName
    );
    rc = SQLITE_ERROR;
  }else{
    /* This is either a full-table scan (ePlan==FTS5_PLAN_SCAN) or a lookup
    ** by rowid (ePlan==FTS5_PLAN_ROWID).  */
    pCsr->ePlan = (pRowidEq ? FTS5_PLAN_ROWID : FTS5_PLAN_SCAN);
    rc = sqlite3Fts5StorageStmt(
        pTab->pStorage, fts5StmtType(pCsr), &pCsr->pStmt, &pTab->p.base.zErrMsg
    );
    if( rc==SQLITE_OK ){
      if( pRowidEq!=0 ){
        assert( pCsr->ePlan==FTS5_PLAN_ROWID );
        sqlite3_bind_value(pCsr->pStmt, 1, pRowidEq);
      }else{
        sqlite3_bind_int64(pCsr->pStmt, 1, pCsr->iFirstRowid);
        sqlite3_bind_int64(pCsr->pStmt, 2, pCsr->iLastRowid);
      }
      rc = fts5NextMethod(pCursor);
    }
  }

 filter_out:
  sqlite3Fts5ExprFree(pExpr);
  pConfig->pzErrmsg = pzErrmsg;
  return rc;
}

/*
** This is the xEof method of the virtual table. SQLite calls this
** routine to find out if it has reached the end of a result set.
*/
static int fts5EofMethod(sqlite3_vtab_cursor *pCursor){
  Fts5Cursor *pCsr = (Fts5Cursor*)pCursor;
  return (CsrFlagTest(pCsr, FTS5CSR_EOF) ? 1 : 0);
}

/*
** Return the rowid that the cursor currently points to.
*/
static i64 fts5CursorRowid(Fts5Cursor *pCsr){
  assert( pCsr->ePlan==FTS5_PLAN_MATCH
       || pCsr->ePlan==FTS5_PLAN_SORTED_MATCH
       || pCsr->ePlan==FTS5_PLAN_SOURCE
  );
  if( pCsr->pSorter ){
    return pCsr->pSorter->iRowid;
  }else{
    return sqlite3Fts5ExprRowid(pCsr->pExpr);
  }
}

/*
** This is the xRowid method. The SQLite core calls this routine to
** retrieve the rowid for the current row of the result set. fts5
** exposes %_content.rowid as the rowid for the virtual table. The
** rowid should be written to *pRowid.
*/
static int fts5RowidMethod(sqlite3_vtab_cursor *pCursor, sqlite_int64 *pRowid){
  Fts5Cursor *pCsr = (Fts5Cursor*)pCursor;
  int ePlan = pCsr->ePlan;

  assert( CsrFlagTest(pCsr, FTS5CSR_EOF)==0 );
  switch( ePlan ){
    case FTS5_PLAN_SPECIAL:
      *pRowid = 0;
      break;

    case FTS5_PLAN_SOURCE:
    case FTS5_PLAN_MATCH:
    case FTS5_PLAN_SORTED_MATCH:
      *pRowid = fts5CursorRowid(pCsr);
      break;

    default:
      *pRowid = sqlite3_column_int64(pCsr->pStmt, 0);
      break;
  }

  return SQLITE_OK;
}

/*
** If the cursor requires seeking (bSeekRequired flag is set), seek it.
** Return SQLITE_OK if no error occurs, or an SQLite error code otherwise.
**
** If argument bErrormsg is true and an error occurs, an error message may
** be left in sqlite3_vtab.zErrMsg.
*/
static int fts5SeekCursor(Fts5Cursor *pCsr, int bErrormsg){
  int rc = SQLITE_OK;

  /* If the cursor does not yet have a statement handle, obtain one now. */
  if( pCsr->pStmt==0 ){
    Fts5FullTable *pTab = (Fts5FullTable*)(pCsr->base.pVtab);
    int eStmt = fts5StmtType(pCsr);
    rc = sqlite3Fts5StorageStmt(
        pTab->pStorage, eStmt, &pCsr->pStmt, (bErrormsg?&pTab->p.base.zErrMsg:0)
    );
    assert( rc!=SQLITE_OK || pTab->p.base.zErrMsg==0 );
    assert( CsrFlagTest(pCsr, FTS5CSR_REQUIRE_CONTENT) );
  }

  if( rc==SQLITE_OK && CsrFlagTest(pCsr, FTS5CSR_REQUIRE_CONTENT) ){
    Fts5Table *pTab = (Fts5Table*)(pCsr->base.pVtab);
    assert( pCsr->pExpr );
    sqlite3_reset(pCsr->pStmt);
    sqlite3_bind_int64(pCsr->pStmt, 1, fts5CursorRowid(pCsr));
    pTab->pConfig->bLock++;
    rc = sqlite3_step(pCsr->pStmt);
    pTab->pConfig->bLock--;
    if( rc==SQLITE_ROW ){
      rc = SQLITE_OK;
      CsrFlagClear(pCsr, FTS5CSR_REQUIRE_CONTENT);
    }else{
      rc = sqlite3_reset(pCsr->pStmt);
      if( rc==SQLITE_OK ){
        rc = FTS5_CORRUPT;
      }else if( pTab->pConfig->pzErrmsg ){
        *pTab->pConfig->pzErrmsg = sqlite3_mprintf(
            "%s", sqlite3_errmsg(pTab->pConfig->db)
        );
      }
    }
  }
  return rc;
}
