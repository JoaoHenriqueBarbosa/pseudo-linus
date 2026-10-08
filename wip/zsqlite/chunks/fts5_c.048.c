

/*
** Close the cursor.  For additional information see the documentation
** on the xClose method of the virtual table interface.
*/
static int fts5CloseMethod(sqlite3_vtab_cursor *pCursor){
  if( pCursor ){
    Fts5FullTable *pTab = (Fts5FullTable*)(pCursor->pVtab);
    Fts5Cursor *pCsr = (Fts5Cursor*)pCursor;
    Fts5Cursor **pp;

    fts5FreeCursorComponents(pCsr);
    /* Remove the cursor from the Fts5Global.pCsr list */
    for(pp=&pTab->pGlobal->pCsr; (*pp)!=pCsr; pp=&(*pp)->pNext);
    *pp = pCsr->pNext;

    sqlite3_free(pCsr);
  }
  return SQLITE_OK;
}

static int fts5SorterNext(Fts5Cursor *pCsr){
  Fts5Sorter *pSorter = pCsr->pSorter;
  int rc;

  rc = sqlite3_step(pSorter->pStmt);
  if( rc==SQLITE_DONE ){
    rc = SQLITE_OK;
    CsrFlagSet(pCsr, FTS5CSR_EOF|FTS5CSR_REQUIRE_CONTENT);
  }else if( rc==SQLITE_ROW ){
    const u8 *a;
    const u8 *aBlob;
    int nBlob;
    int i;
    int iOff = 0;
    rc = SQLITE_OK;

    pSorter->iRowid = sqlite3_column_int64(pSorter->pStmt, 0);
    nBlob = sqlite3_column_bytes(pSorter->pStmt, 1);
    aBlob = a = sqlite3_column_blob(pSorter->pStmt, 1);

    /* nBlob==0 in detail=none mode. */
    if( nBlob>0 ){
      for(i=0; i<(pSorter->nIdx-1); i++){
        int iVal;
        a += fts5GetVarint32(a, iVal);
        iOff += iVal;
        pSorter->aIdx[i] = iOff;
      }
      pSorter->aIdx[i] = &aBlob[nBlob] - a;
      pSorter->aPoslist = a;
    }

    fts5CsrNewrow(pCsr);
  }

  return rc;
}


/*
** Set the FTS5CSR_REQUIRE_RESEEK flag on all FTS5_PLAN_MATCH cursors
** open on table pTab.
*/
static void fts5TripCursors(Fts5FullTable *pTab){
  Fts5Cursor *pCsr;
  for(pCsr=pTab->pGlobal->pCsr; pCsr; pCsr=pCsr->pNext){
    if( pCsr->ePlan==FTS5_PLAN_MATCH
     && pCsr->base.pVtab==(sqlite3_vtab*)pTab
    ){
      CsrFlagSet(pCsr, FTS5CSR_REQUIRE_RESEEK);
    }
  }
}

/*
** If the REQUIRE_RESEEK flag is set on the cursor passed as the first
** argument, close and reopen all Fts5IndexIter iterators that the cursor
** is using. Then attempt to move the cursor to a rowid equal to or laster
** (in the cursors sort order - ASC or DESC) than the current rowid.
**
** If the new rowid is not equal to the old, set output parameter *pbSkip
** to 1 before returning. Otherwise, leave it unchanged.
**
** Return SQLITE_OK if successful or if no reseek was required, or an
** error code if an error occurred.
*/
static int fts5CursorReseek(Fts5Cursor *pCsr, int *pbSkip){
  int rc = SQLITE_OK;
  assert( *pbSkip==0 );
  if( CsrFlagTest(pCsr, FTS5CSR_REQUIRE_RESEEK) ){
    Fts5FullTable *pTab = (Fts5FullTable*)(pCsr->base.pVtab);
    int bDesc = pCsr->bDesc;
    i64 iRowid = sqlite3Fts5ExprRowid(pCsr->pExpr);

    rc = sqlite3Fts5ExprFirst(pCsr->pExpr, pTab->p.pIndex, iRowid, bDesc);
    if( rc==SQLITE_OK &&  iRowid!=sqlite3Fts5ExprRowid(pCsr->pExpr) ){
      *pbSkip = 1;
    }

    CsrFlagClear(pCsr, FTS5CSR_REQUIRE_RESEEK);
    fts5CsrNewrow(pCsr);
    if( sqlite3Fts5ExprEof(pCsr->pExpr) ){
      CsrFlagSet(pCsr, FTS5CSR_EOF);
      *pbSkip = 1;
    }
  }
  return rc;
}


/*
** Advance the cursor to the next row in the table that matches the
** search criteria.
**
** Return SQLITE_OK if nothing goes wrong.  SQLITE_OK is returned
** even if we reach end-of-file.  The fts5EofMethod() will be called
** subsequently to determine whether or not an EOF was hit.
*/
static int fts5NextMethod(sqlite3_vtab_cursor *pCursor){
  Fts5Cursor *pCsr = (Fts5Cursor*)pCursor;
  int rc;

  assert( (pCsr->ePlan<3)==
          (pCsr->ePlan==FTS5_PLAN_MATCH || pCsr->ePlan==FTS5_PLAN_SOURCE)
  );
  assert( !CsrFlagTest(pCsr, FTS5CSR_EOF) );

  /* If this cursor uses FTS5_PLAN_MATCH and this is a tokendata=1 table,
  ** clear any token mappings accumulated at the fts5_index.c level. In
  ** other cases, specifically FTS5_PLAN_SOURCE and FTS5_PLAN_SORTED_MATCH,
  ** we need to retain the mappings for the entire query.  */
  if( pCsr->ePlan==FTS5_PLAN_MATCH
   && ((Fts5Table*)pCursor->pVtab)->pConfig->bTokendata
  ){
    sqlite3Fts5ExprClearTokens(pCsr->pExpr);
  }

  if( pCsr->ePlan<3 ){
    int bSkip = 0;
    if( (rc = fts5CursorReseek(pCsr, &bSkip)) || bSkip ) return rc;
    rc = sqlite3Fts5ExprNext(pCsr->pExpr, pCsr->iLastRowid);
    CsrFlagSet(pCsr, sqlite3Fts5ExprEof(pCsr->pExpr));
    fts5CsrNewrow(pCsr);
  }else{
    switch( pCsr->ePlan ){
      case FTS5_PLAN_SPECIAL: {
        CsrFlagSet(pCsr, FTS5CSR_EOF);
        rc = SQLITE_OK;
        break;
      }

      case FTS5_PLAN_SORTED_MATCH: {
        rc = fts5SorterNext(pCsr);
        break;
      }

      default: {
        Fts5Config *pConfig = ((Fts5Table*)pCursor->pVtab)->pConfig;
        pConfig->bLock++;
        rc = sqlite3_step(pCsr->pStmt);
        pConfig->bLock--;
        if( rc!=SQLITE_ROW ){
          CsrFlagSet(pCsr, FTS5CSR_EOF);
          rc = sqlite3_reset(pCsr->pStmt);
          if( rc!=SQLITE_OK ){
            pCursor->pVtab->zErrMsg = sqlite3_mprintf(
                "%s", sqlite3_errmsg(pConfig->db)
            );
          }
        }else{
          rc = SQLITE_OK;
        }
        break;
      }
    }
  }

  return rc;
}


static int fts5PrepareStatement(
  sqlite3_stmt **ppStmt,
  Fts5Config *pConfig,
  const char *zFmt,
  ...
){
  sqlite3_stmt *pRet = 0;
  int rc;
  char *zSql;
  va_list ap;

  va_start(ap, zFmt);
  zSql = sqlite3_vmprintf(zFmt, ap);
  if( zSql==0 ){
    rc = SQLITE_NOMEM;
  }else{
    rc = sqlite3_prepare_v3(pConfig->db, zSql, -1,
                            SQLITE_PREPARE_PERSISTENT, &pRet, 0);
    if( rc!=SQLITE_OK ){
      *pConfig->pzErrmsg = sqlite3_mprintf("%s", sqlite3_errmsg(pConfig->db));
    }
    sqlite3_free(zSql);
  }

  va_end(ap);
  *ppStmt = pRet;
  return rc;
}

static int fts5CursorFirstSorted(
  Fts5FullTable *pTab,
  Fts5Cursor *pCsr,
  int bDesc
){
  Fts5Config *pConfig = pTab->p.pConfig;
  Fts5Sorter *pSorter;
  int nPhrase;
  sqlite3_int64 nByte;
  int rc;
  const char *zRank = pCsr->zRank;
  const char *zRankArgs = pCsr->zRankArgs;

  nPhrase = sqlite3Fts5ExprPhraseCount(pCsr->pExpr);
  nByte = sizeof(Fts5Sorter) + sizeof(int) * (nPhrase-1);
  pSorter = (Fts5Sorter*)sqlite3_malloc64(nByte);
  if( pSorter==0 ) return SQLITE_NOMEM;
  memset(pSorter, 0, (size_t)nByte);
  pSorter->nIdx = nPhrase;

  /* TODO: It would be better to have some system for reusing statement
  ** handles here, rather than preparing a new one for each query. But that
  ** is not possible as SQLite reference counts the virtual table objects.
  ** And since the statement required here reads from this very virtual
  ** table, saving it creates a circular reference.
  **
  ** If SQLite a built-in statement cache, this wouldn't be a problem. */
  rc = fts5PrepareStatement(&pSorter->pStmt, pConfig,
      "SELECT rowid, rank FROM %Q.%Q ORDER BY %s(\"%w\"%s%s) %s",
      pConfig->zDb, pConfig->zName, zRank, pConfig->zName,
      (zRankArgs ? ", " : ""),
      (zRankArgs ? zRankArgs : ""),
      bDesc ? "DESC" : "ASC"
  );

  pCsr->pSorter = pSorter;
  if( rc==SQLITE_OK ){
    assert( pTab->pSortCsr==0 );
    pTab->pSortCsr = pCsr;
    rc = fts5SorterNext(pCsr);
    pTab->pSortCsr = 0;
  }

  if( rc!=SQLITE_OK ){
    sqlite3_finalize(pSorter->pStmt);
    sqlite3_free(pSorter);
    pCsr->pSorter = 0;
  }

  return rc;
}

static int fts5CursorFirst(Fts5FullTable *pTab, Fts5Cursor *pCsr, int bDesc){
  int rc;
  Fts5Expr *pExpr = pCsr->pExpr;
  rc = sqlite3Fts5ExprFirst(pExpr, pTab->p.pIndex, pCsr->iFirstRowid, bDesc);
  if( sqlite3Fts5ExprEof(pExpr) ){
    CsrFlagSet(pCsr, FTS5CSR_EOF);
  }
  fts5CsrNewrow(pCsr);
  return rc;
}

/*
** Process a "special" query. A special query is identified as one with a
** MATCH expression that begins with a '*' character. The remainder of
** the text passed to the MATCH operator are used as  the special query
** parameters.
*/
static int fts5SpecialMatch(
  Fts5FullTable *pTab,
  Fts5Cursor *pCsr,
  const char *zQuery
){
  int rc = SQLITE_OK;             /* Return code */
  const char *z = zQuery;         /* Special query text */
  int n;                          /* Number of bytes in text at z */

  while( z[0]==' ' ) z++;
  for(n=0; z[n] && z[n]!=' '; n++);

  assert( pTab->p.base.zErrMsg==0 );
  pCsr->ePlan = FTS5_PLAN_SPECIAL;

  if( n==5 && 0==sqlite3_strnicmp("reads", z, n) ){
    pCsr->iSpecial = sqlite3Fts5IndexReads(pTab->p.pIndex);
  }
  else if( n==2 && 0==sqlite3_strnicmp("id", z, n) ){
    pCsr->iSpecial = pCsr->iCsrId;
  }
  else{
    /* An unrecognized directive. Return an error message. */
    pTab->p.base.zErrMsg = sqlite3_mprintf("unknown special query: %.*s", n, z);
    rc = SQLITE_ERROR;
  }

  return rc;
}

/*
** Search for an auxiliary function named zName that can be used with table
** pTab. If one is found, return a pointer to the corresponding Fts5Auxiliary
** structure. Otherwise, if no such function exists, return NULL.
*/
static Fts5Auxiliary *fts5FindAuxiliary(Fts5FullTable *pTab, const char *zName){
  Fts5Auxiliary *pAux;

  for(pAux=pTab->pGlobal->pAux; pAux; pAux=pAux->pNext){
    if( sqlite3_stricmp(zName, pAux->zFunc)==0 ) return pAux;
  }

  /* No function of the specified name was found. Return 0. */
  return 0;
}


static int fts5FindRankFunction(Fts5Cursor *pCsr){
  Fts5FullTable *pTab = (Fts5FullTable*)(pCsr->base.pVtab);
  Fts5Config *pConfig = pTab->p.pConfig;
  int rc = SQLITE_OK;
  Fts5Auxiliary *pAux = 0;
  const char *zRank = pCsr->zRank;
  const char *zRankArgs = pCsr->zRankArgs;

  if( zRankArgs ){
    char *zSql = sqlite3Fts5Mprintf(&rc, "SELECT %s", zRankArgs);
    if( zSql ){
      sqlite3_stmt *pStmt = 0;
      rc = sqlite3_prepare_v3(pConfig->db, zSql, -1,
                              SQLITE_PREPARE_PERSISTENT, &pStmt, 0);
      sqlite3_free(zSql);
      assert( rc==SQLITE_OK || pCsr->pRankArgStmt==0 );
      if( rc==SQLITE_OK ){
        if( SQLITE_ROW==sqlite3_step(pStmt) ){
          sqlite3_int64 nByte;
          pCsr->nRankArg = sqlite3_column_count(pStmt);
          nByte = sizeof(sqlite3_value*)*pCsr->nRankArg;
          pCsr->apRankArg = (sqlite3_value**)sqlite3Fts5MallocZero(&rc, nByte);
          if( rc==SQLITE_OK ){
            int i;
            for(i=0; i<pCsr->nRankArg; i++){
              pCsr->apRankArg[i] = sqlite3_column_value(pStmt, i);
            }
          }
          pCsr->pRankArgStmt = pStmt;
        }else{
          rc = sqlite3_finalize(pStmt);
          assert( rc!=SQLITE_OK );
        }
      }
    }
  }

  if( rc==SQLITE_OK ){
    pAux = fts5FindAuxiliary(pTab, zRank);
    if( pAux==0 ){
      assert( pTab->p.base.zErrMsg==0 );
      pTab->p.base.zErrMsg = sqlite3_mprintf("no such function: %s", zRank);
      rc = SQLITE_ERROR;
    }
  }

  pCsr->pRank = pAux;
  return rc;
}
