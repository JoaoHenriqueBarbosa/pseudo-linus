
/*
** This is an sqlite3Fts3ExprIterate() callback. If the Fts3Expr.aMI[] array
** has not yet been allocated, allocate and zero it. Otherwise, just zero
** it.
*/
static int fts3AllocateMSI(Fts3Expr *pExpr, int iPhrase, void *pCtx){
  Fts3Table *pTab = (Fts3Table*)pCtx;
  UNUSED_PARAMETER(iPhrase);
  if( pExpr->aMI==0 ){
    pExpr->aMI = (u32 *)sqlite3_malloc64(pTab->nColumn * 3 * sizeof(u32));
    if( pExpr->aMI==0 ) return SQLITE_NOMEM;
  }
  memset(pExpr->aMI, 0, pTab->nColumn * 3 * sizeof(u32));
  return SQLITE_OK;
}

/*
** Expression pExpr must be of type FTSQUERY_PHRASE.
**
** If it is not already allocated and populated, this function allocates and
** populates the Fts3Expr.aMI[] array for expression pExpr. If pExpr is part
** of a NEAR expression, then it also allocates and populates the same array
** for all other phrases that are part of the NEAR expression.
**
** SQLITE_OK is returned if the aMI[] array is successfully allocated and
** populated. Otherwise, if an error occurs, an SQLite error code is returned.
*/
static int fts3EvalGatherStats(
  Fts3Cursor *pCsr,               /* Cursor object */
  Fts3Expr *pExpr                 /* FTSQUERY_PHRASE expression */
){
  int rc = SQLITE_OK;             /* Return code */

  assert( pExpr->eType==FTSQUERY_PHRASE );
  if( pExpr->aMI==0 ){
    Fts3Table *pTab = (Fts3Table *)pCsr->base.pVtab;
    Fts3Expr *pRoot;                /* Root of NEAR expression */

    sqlite3_int64 iPrevId = pCsr->iPrevId;
    sqlite3_int64 iDocid;
    u8 bEof;

    /* Find the root of the NEAR expression */
    pRoot = pExpr;
    while( pRoot->pParent
        && (pRoot->pParent->eType==FTSQUERY_NEAR || pRoot->bDeferred)
    ){
      pRoot = pRoot->pParent;
    }
    iDocid = pRoot->iDocid;
    bEof = pRoot->bEof;
    assert( pRoot->bStart );

    /* Allocate space for the aMSI[] array of each FTSQUERY_PHRASE node */
    rc = sqlite3Fts3ExprIterate(pRoot, fts3AllocateMSI, (void*)pTab);
    if( rc!=SQLITE_OK ) return rc;
    fts3EvalRestart(pCsr, pRoot, &rc);

    while( pCsr->isEof==0 && rc==SQLITE_OK ){

      do {
        /* Ensure the %_content statement is reset. */
        if( pCsr->isRequireSeek==0 ) sqlite3_reset(pCsr->pStmt);
        assert( sqlite3_data_count(pCsr->pStmt)==0 );

        /* Advance to the next document */
        fts3EvalNextRow(pCsr, pRoot, &rc);
        pCsr->isEof = pRoot->bEof;
        pCsr->isRequireSeek = 1;
        pCsr->isMatchinfoNeeded = 1;
        pCsr->iPrevId = pRoot->iDocid;
      }while( pCsr->isEof==0
           && pRoot->eType==FTSQUERY_NEAR
           && sqlite3Fts3EvalTestDeferred(pCsr, &rc)
      );

      if( rc==SQLITE_OK && pCsr->isEof==0 ){
        fts3EvalUpdateCounts(pRoot, pTab->nColumn);
      }
    }

    pCsr->isEof = 0;
    pCsr->iPrevId = iPrevId;

    if( bEof ){
      pRoot->bEof = bEof;
    }else{
      /* Caution: pRoot may iterate through docids in ascending or descending
      ** order. For this reason, even though it seems more defensive, the
      ** do loop can not be written:
      **
      **   do {...} while( pRoot->iDocid<iDocid && rc==SQLITE_OK );
      */
      fts3EvalRestart(pCsr, pRoot, &rc);
      do {
        fts3EvalNextRow(pCsr, pRoot, &rc);
        assert_fts3_nc( pRoot->bEof==0 );
        if( pRoot->bEof ) rc = FTS_CORRUPT_VTAB;
      }while( pRoot->iDocid!=iDocid && rc==SQLITE_OK );
    }
  }
  return rc;
}

/*
** This function is used by the matchinfo() module to query a phrase
** expression node for the following information:
**
**   1. The total number of occurrences of the phrase in each column of
**      the FTS table (considering all rows), and
**
**   2. For each column, the number of rows in the table for which the
**      column contains at least one instance of the phrase.
**
** If no error occurs, SQLITE_OK is returned and the values for each column
** written into the array aiOut as follows:
**
**   aiOut[iCol*3 + 1] = Number of occurrences
**   aiOut[iCol*3 + 2] = Number of rows containing at least one instance
**
** Caveats:
**
**   * If a phrase consists entirely of deferred tokens, then all output
**     values are set to the number of documents in the table. In other
**     words we assume that very common tokens occur exactly once in each
**     column of each row of the table.
**
**   * If a phrase contains some deferred tokens (and some non-deferred
**     tokens), count the potential occurrence identified by considering
**     the non-deferred tokens instead of actual phrase occurrences.
**
**   * If the phrase is part of a NEAR expression, then only phrase instances
**     that meet the NEAR constraint are included in the counts.
*/
SQLITE_PRIVATE int sqlite3Fts3EvalPhraseStats(
  Fts3Cursor *pCsr,               /* FTS cursor handle */
  Fts3Expr *pExpr,                /* Phrase expression */
  u32 *aiOut                      /* Array to write results into (see above) */
){
  Fts3Table *pTab = (Fts3Table *)pCsr->base.pVtab;
  int rc = SQLITE_OK;
  int iCol;

  if( pExpr->bDeferred && pExpr->pParent->eType!=FTSQUERY_NEAR ){
    assert( pCsr->nDoc>0 );
    for(iCol=0; iCol<pTab->nColumn; iCol++){
      aiOut[iCol*3 + 1] = (u32)pCsr->nDoc;
      aiOut[iCol*3 + 2] = (u32)pCsr->nDoc;
    }
  }else{
    rc = fts3EvalGatherStats(pCsr, pExpr);
    if( rc==SQLITE_OK ){
      assert( pExpr->aMI );
      for(iCol=0; iCol<pTab->nColumn; iCol++){
        aiOut[iCol*3 + 1] = pExpr->aMI[iCol*3 + 1];
        aiOut[iCol*3 + 2] = pExpr->aMI[iCol*3 + 2];
      }
    }
  }

  return rc;
}

/*
** The expression pExpr passed as the second argument to this function
** must be of type FTSQUERY_PHRASE.
**
** The returned value is either NULL or a pointer to a buffer containing
** a position-list indicating the occurrences of the phrase in column iCol
** of the current row.
**
** More specifically, the returned buffer contains 1 varint for each
** occurrence of the phrase in the column, stored using the normal (delta+2)
** compression and is terminated by either an 0x01 or 0x00 byte. For example,
** if the requested column contains "a b X c d X X" and the position-list
** for 'X' is requested, the buffer returned may contain:
**
**     0x04 0x05 0x03 0x01   or   0x04 0x05 0x03 0x00
**
** This function works regardless of whether or not the phrase is deferred,
** incremental, or neither.
*/
SQLITE_PRIVATE int sqlite3Fts3EvalPhrasePoslist(
  Fts3Cursor *pCsr,               /* FTS3 cursor object */
  Fts3Expr *pExpr,                /* Phrase to return doclist for */
  int iCol,                       /* Column to return position list for */
  char **ppOut                    /* OUT: Pointer to position list */
){
  Fts3Phrase *pPhrase = pExpr->pPhrase;
  Fts3Table *pTab = (Fts3Table *)pCsr->base.pVtab;
  char *pIter;
  int iThis;
  sqlite3_int64 iDocid;

  /* If this phrase is applies specifically to some column other than
  ** column iCol, return a NULL pointer.  */
  *ppOut = 0;
  assert( iCol>=0 && iCol<pTab->nColumn );
  if( (pPhrase->iColumn<pTab->nColumn && pPhrase->iColumn!=iCol) ){
    return SQLITE_OK;
  }

  iDocid = pExpr->iDocid;
  pIter = pPhrase->doclist.pList;
  if( iDocid!=pCsr->iPrevId || pExpr->bEof ){
    int rc = SQLITE_OK;
    int bDescDoclist = pTab->bDescIdx;      /* For DOCID_CMP macro */
    int bOr = 0;
    u8 bTreeEof = 0;
    Fts3Expr *p;                  /* Used to iterate from pExpr to root */
    Fts3Expr *pNear;              /* Most senior NEAR ancestor (or pExpr) */
    Fts3Expr *pRun;               /* Closest non-deferred ancestor of pNear */
    int bMatch;

    /* Check if this phrase descends from an OR expression node. If not,
    ** return NULL. Otherwise, the entry that corresponds to docid
    ** pCsr->iPrevId may lie earlier in the doclist buffer. Or, if the
    ** tree that the node is part of has been marked as EOF, but the node
    ** itself is not EOF, then it may point to an earlier entry. */
    pNear = pExpr;
    for(p=pExpr->pParent; p; p=p->pParent){
      if( p->eType==FTSQUERY_OR ) bOr = 1;
      if( p->eType==FTSQUERY_NEAR ) pNear = p;
      if( p->bEof ) bTreeEof = 1;
    }
    if( bOr==0 ) return SQLITE_OK;
    pRun = pNear;
    while( pRun->bDeferred ){
      assert( pRun->pParent );
      pRun = pRun->pParent;
    }

    /* This is the descendent of an OR node. In this case we cannot use
    ** an incremental phrase. Load the entire doclist for the phrase
    ** into memory in this case.  */
    if( pPhrase->bIncr ){
      int bEofSave = pRun->bEof;
      fts3EvalRestart(pCsr, pRun, &rc);
      while( rc==SQLITE_OK && !pRun->bEof ){
        fts3EvalNextRow(pCsr, pRun, &rc);
        if( bEofSave==0 && pRun->iDocid==iDocid ) break;
      }
      assert( rc!=SQLITE_OK || pPhrase->bIncr==0 );
      if( rc==SQLITE_OK && pRun->bEof!=bEofSave ){
        rc = FTS_CORRUPT_VTAB;
      }
    }
    if( bTreeEof ){
      while( rc==SQLITE_OK && !pRun->bEof ){
        fts3EvalNextRow(pCsr, pRun, &rc);
      }
    }
    if( rc!=SQLITE_OK ) return rc;

    bMatch = 1;
    for(p=pNear; p; p=p->pLeft){
      u8 bEof = 0;
      Fts3Expr *pTest = p;
      Fts3Phrase *pPh;
      assert( pTest->eType==FTSQUERY_NEAR || pTest->eType==FTSQUERY_PHRASE );
      if( pTest->eType==FTSQUERY_NEAR ) pTest = pTest->pRight;
      assert( pTest->eType==FTSQUERY_PHRASE );
      pPh = pTest->pPhrase;

      pIter = pPh->pOrPoslist;
      iDocid = pPh->iOrDocid;
      if( pCsr->bDesc==bDescDoclist ){
        bEof = !pPh->doclist.nAll ||
          (pIter >= (pPh->doclist.aAll + pPh->doclist.nAll));
        while( (pIter==0 || DOCID_CMP(iDocid, pCsr->iPrevId)<0 ) && bEof==0 ){
          sqlite3Fts3DoclistNext(
              bDescDoclist, pPh->doclist.aAll, pPh->doclist.nAll,
              &pIter, &iDocid, &bEof
          );
        }
      }else{
        bEof = !pPh->doclist.nAll || (pIter && pIter<=pPh->doclist.aAll);
        while( (pIter==0 || DOCID_CMP(iDocid, pCsr->iPrevId)>0 ) && bEof==0 ){
          int dummy;
          sqlite3Fts3DoclistPrev(
              bDescDoclist, pPh->doclist.aAll, pPh->doclist.nAll,
              &pIter, &iDocid, &dummy, &bEof
              );
        }
      }
      pPh->pOrPoslist = pIter;
      pPh->iOrDocid = iDocid;
      if( bEof || iDocid!=pCsr->iPrevId ) bMatch = 0;
    }

    if( bMatch ){
      pIter = pPhrase->pOrPoslist;
    }else{
      pIter = 0;
    }
  }
  if( pIter==0 ) return SQLITE_OK;

  if( *pIter==0x01 ){
    pIter++;
    pIter += fts3GetVarint32(pIter, &iThis);
  }else{
    iThis = 0;
  }
  while( iThis<iCol ){
    fts3ColumnlistCopy(0, &pIter);
    if( *pIter==0x00 ) return SQLITE_OK;
    pIter++;
    pIter += fts3GetVarint32(pIter, &iThis);
  }
  if( *pIter==0x00 ){
    pIter = 0;
  }

  *ppOut = ((iCol==iThis)?pIter:0);
  return SQLITE_OK;
}

/*
** Free all components of the Fts3Phrase structure that were allocated by
** the eval module. Specifically, this means to free:
**
**   * the contents of pPhrase->doclist, and
**   * any Fts3MultiSegReader objects held by phrase tokens.
*/
SQLITE_PRIVATE void sqlite3Fts3EvalPhraseCleanup(Fts3Phrase *pPhrase){
  if( pPhrase ){
    int i;
    sqlite3_free(pPhrase->doclist.aAll);
    fts3EvalInvalidatePoslist(pPhrase);
    memset(&pPhrase->doclist, 0, sizeof(Fts3Doclist));
    for(i=0; i<pPhrase->nToken; i++){
      fts3SegReaderCursorFree(pPhrase->aToken[i].pSegcsr);
      pPhrase->aToken[i].pSegcsr = 0;
    }
  }
}


/*
** Return SQLITE_CORRUPT_VTAB.
*/
#ifdef SQLITE_DEBUG
SQLITE_PRIVATE int sqlite3Fts3Corrupt(){
  return SQLITE_CORRUPT_VTAB;
}
#endif

#if !SQLITE_CORE
/*
** Initialize API pointer table, if required.
*/
#ifdef _WIN32
__declspec(dllexport)
#endif
SQLITE_API int sqlite3_fts3_init(
  sqlite3 *db,
  char **pzErrMsg,
  const sqlite3_api_routines *pApi
){
  SQLITE_EXTENSION_INIT2(pApi)
  return sqlite3Fts3Init(db);
}
