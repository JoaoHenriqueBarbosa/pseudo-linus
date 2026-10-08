
/*
** If *pRc is not SQLITE_OK, or if pExpr is not the root node of a NEAR
** cluster, then this function returns 1 immediately.
**
** Otherwise, it checks if the current row really does match the NEAR
** expression, using the data currently stored in the position lists
** (Fts3Expr->pPhrase.doclist.pList/nList) for each phrase in the expression.
**
** If the current row is a match, the position list associated with each
** phrase in the NEAR expression is edited in place to contain only those
** phrase instances sufficiently close to their peers to satisfy all NEAR
** constraints. In this case it returns 1. If the NEAR expression does not
** match the current row, 0 is returned. The position lists may or may not
** be edited if 0 is returned.
*/
static int fts3EvalNearTest(Fts3Expr *pExpr, int *pRc){
  int res = 1;

  /* The following block runs if pExpr is the root of a NEAR query.
  ** For example, the query:
  **
  **         "w" NEAR "x" NEAR "y" NEAR "z"
  **
  ** which is represented in tree form as:
  **
  **                               |
  **                          +--NEAR--+      <-- root of NEAR query
  **                          |        |
  **                     +--NEAR--+   "z"
  **                     |        |
  **                +--NEAR--+   "y"
  **                |        |
  **               "w"      "x"
  **
  ** The right-hand child of a NEAR node is always a phrase. The
  ** left-hand child may be either a phrase or a NEAR node. There are
  ** no exceptions to this - it's the way the parser in fts3_expr.c works.
  */
  if( *pRc==SQLITE_OK
   && pExpr->eType==FTSQUERY_NEAR
   && (pExpr->pParent==0 || pExpr->pParent->eType!=FTSQUERY_NEAR)
  ){
    Fts3Expr *p;
    sqlite3_int64 nTmp = 0;       /* Bytes of temp space */
    char *aTmp;                   /* Temp space for PoslistNearMerge() */

    /* Allocate temporary working space. */
    for(p=pExpr; p->pLeft; p=p->pLeft){
      assert( p->pRight->pPhrase->doclist.nList>0 );
      nTmp += p->pRight->pPhrase->doclist.nList;
    }
    nTmp += p->pPhrase->doclist.nList;
    aTmp = sqlite3_malloc64(nTmp*2);
    if( !aTmp ){
      *pRc = SQLITE_NOMEM;
      res = 0;
    }else{
      char *aPoslist = p->pPhrase->doclist.pList;
      int nToken = p->pPhrase->nToken;

      for(p=p->pParent;res && p && p->eType==FTSQUERY_NEAR; p=p->pParent){
        Fts3Phrase *pPhrase = p->pRight->pPhrase;
        int nNear = p->nNear;
        res = fts3EvalNearTrim(nNear, aTmp, &aPoslist, &nToken, pPhrase);
      }

      aPoslist = pExpr->pRight->pPhrase->doclist.pList;
      nToken = pExpr->pRight->pPhrase->nToken;
      for(p=pExpr->pLeft; p && res; p=p->pLeft){
        int nNear;
        Fts3Phrase *pPhrase;
        assert( p->pParent && p->pParent->pLeft==p );
        nNear = p->pParent->nNear;
        pPhrase = (
            p->eType==FTSQUERY_NEAR ? p->pRight->pPhrase : p->pPhrase
        );
        res = fts3EvalNearTrim(nNear, aTmp, &aPoslist, &nToken, pPhrase);
      }
    }

    sqlite3_free(aTmp);
  }

  return res;
}

/*
** This function is a helper function for sqlite3Fts3EvalTestDeferred().
** Assuming no error occurs or has occurred, It returns non-zero if the
** expression passed as the second argument matches the row that pCsr
** currently points to, or zero if it does not.
**
** If *pRc is not SQLITE_OK when this function is called, it is a no-op.
** If an error occurs during execution of this function, *pRc is set to
** the appropriate SQLite error code. In this case the returned value is
** undefined.
*/
static int fts3EvalTestExpr(
  Fts3Cursor *pCsr,               /* FTS cursor handle */
  Fts3Expr *pExpr,                /* Expr to test. May or may not be root. */
  int *pRc                        /* IN/OUT: Error code */
){
  int bHit = 1;                   /* Return value */
  if( *pRc==SQLITE_OK ){
    switch( pExpr->eType ){
      case FTSQUERY_NEAR:
      case FTSQUERY_AND:
        bHit = (
            fts3EvalTestExpr(pCsr, pExpr->pLeft, pRc)
         && fts3EvalTestExpr(pCsr, pExpr->pRight, pRc)
         && fts3EvalNearTest(pExpr, pRc)
        );

        /* If the NEAR expression does not match any rows, zero the doclist for
        ** all phrases involved in the NEAR. This is because the snippet(),
        ** offsets() and matchinfo() functions are not supposed to recognize
        ** any instances of phrases that are part of unmatched NEAR queries.
        ** For example if this expression:
        **
        **    ... MATCH 'a OR (b NEAR c)'
        **
        ** is matched against a row containing:
        **
        **        'a b d e'
        **
        ** then any snippet() should ony highlight the "a" term, not the "b"
        ** (as "b" is part of a non-matching NEAR clause).
        */
        if( bHit==0
         && pExpr->eType==FTSQUERY_NEAR
         && (pExpr->pParent==0 || pExpr->pParent->eType!=FTSQUERY_NEAR)
        ){
          Fts3Expr *p;
          for(p=pExpr; p->pPhrase==0; p=p->pLeft){
            if( p->pRight->iDocid==pCsr->iPrevId ){
              fts3EvalInvalidatePoslist(p->pRight->pPhrase);
            }
          }
          if( p->iDocid==pCsr->iPrevId ){
            fts3EvalInvalidatePoslist(p->pPhrase);
          }
        }

        break;

      case FTSQUERY_OR: {
        int bHit1 = fts3EvalTestExpr(pCsr, pExpr->pLeft, pRc);
        int bHit2 = fts3EvalTestExpr(pCsr, pExpr->pRight, pRc);
        bHit = bHit1 || bHit2;
        break;
      }

      case FTSQUERY_NOT:
        bHit = (
            fts3EvalTestExpr(pCsr, pExpr->pLeft, pRc)
         && !fts3EvalTestExpr(pCsr, pExpr->pRight, pRc)
        );
        break;

      default: {
#ifndef SQLITE_DISABLE_FTS4_DEFERRED
        if( pCsr->pDeferred && (pExpr->bDeferred || (
            pExpr->iDocid==pCsr->iPrevId && pExpr->pPhrase->doclist.pList
        ))){
          Fts3Phrase *pPhrase = pExpr->pPhrase;
          if( pExpr->bDeferred ){
            fts3EvalInvalidatePoslist(pPhrase);
          }
          *pRc = fts3EvalDeferredPhrase(pCsr, pPhrase);
          bHit = (pPhrase->doclist.pList!=0);
          pExpr->iDocid = pCsr->iPrevId;
        }else
#endif
        {
          bHit = (
              pExpr->bEof==0 && pExpr->iDocid==pCsr->iPrevId
           && pExpr->pPhrase->doclist.nList>0
          );
        }
        break;
      }
    }
  }
  return bHit;
}

/*
** This function is called as the second part of each xNext operation when
** iterating through the results of a full-text query. At this point the
** cursor points to a row that matches the query expression, with the
** following caveats:
**
**   * Up until this point, "NEAR" operators in the expression have been
**     treated as "AND".
**
**   * Deferred tokens have not yet been considered.
**
** If *pRc is not SQLITE_OK when this function is called, it immediately
** returns 0. Otherwise, it tests whether or not after considering NEAR
** operators and deferred tokens the current row is still a match for the
** expression. It returns 1 if both of the following are true:
**
**   1. *pRc is SQLITE_OK when this function returns, and
**
**   2. After scanning the current FTS table row for the deferred tokens,
**      it is determined that the row does *not* match the query.
**
** Or, if no error occurs and it seems the current row does match the FTS
** query, return 0.
*/
SQLITE_PRIVATE int sqlite3Fts3EvalTestDeferred(Fts3Cursor *pCsr, int *pRc){
  int rc = *pRc;
  int bMiss = 0;
  if( rc==SQLITE_OK ){

    /* If there are one or more deferred tokens, load the current row into
    ** memory and scan it to determine the position list for each deferred
    ** token. Then, see if this row is really a match, considering deferred
    ** tokens and NEAR operators (neither of which were taken into account
    ** earlier, by fts3EvalNextRow()).
    */
    if( pCsr->pDeferred ){
      rc = fts3CursorSeek(0, pCsr);
      if( rc==SQLITE_OK ){
        rc = sqlite3Fts3CacheDeferredDoclists(pCsr);
      }
    }
    bMiss = (0==fts3EvalTestExpr(pCsr, pCsr->pExpr, &rc));

    /* Free the position-lists accumulated for each deferred token above. */
    sqlite3Fts3FreeDeferredDoclists(pCsr);
    *pRc = rc;
  }
  return (rc==SQLITE_OK && bMiss);
}

/*
** Advance to the next document that matches the FTS expression in
** Fts3Cursor.pExpr.
*/
static int fts3EvalNext(Fts3Cursor *pCsr){
  int rc = SQLITE_OK;             /* Return Code */
  Fts3Expr *pExpr = pCsr->pExpr;
  assert( pCsr->isEof==0 );
  if( pExpr==0 ){
    pCsr->isEof = 1;
  }else{
    do {
      if( pCsr->isRequireSeek==0 ){
        sqlite3_reset(pCsr->pStmt);
      }
      assert( sqlite3_data_count(pCsr->pStmt)==0 );
      fts3EvalNextRow(pCsr, pExpr, &rc);
      pCsr->isEof = pExpr->bEof;
      pCsr->isRequireSeek = 1;
      pCsr->isMatchinfoNeeded = 1;
      pCsr->iPrevId = pExpr->iDocid;
    }while( pCsr->isEof==0 && sqlite3Fts3EvalTestDeferred(pCsr, &rc) );
  }

  /* Check if the cursor is past the end of the docid range specified
  ** by Fts3Cursor.iMinDocid/iMaxDocid. If so, set the EOF flag.  */
  if( rc==SQLITE_OK && (
        (pCsr->bDesc==0 && pCsr->iPrevId>pCsr->iMaxDocid)
     || (pCsr->bDesc!=0 && pCsr->iPrevId<pCsr->iMinDocid)
  )){
    pCsr->isEof = 1;
  }

  return rc;
}

/*
** Restart interation for expression pExpr so that the next call to
** fts3EvalNext() visits the first row. Do not allow incremental
** loading or merging of phrase doclists for this iteration.
**
** If *pRc is other than SQLITE_OK when this function is called, it is
** a no-op. If an error occurs within this function, *pRc is set to an
** SQLite error code before returning.
*/
static void fts3EvalRestart(
  Fts3Cursor *pCsr,
  Fts3Expr *pExpr,
  int *pRc
){
  if( pExpr && *pRc==SQLITE_OK ){
    Fts3Phrase *pPhrase = pExpr->pPhrase;

    if( pPhrase ){
      fts3EvalInvalidatePoslist(pPhrase);
      if( pPhrase->bIncr ){
        int i;
        for(i=0; i<pPhrase->nToken; i++){
          Fts3PhraseToken *pToken = &pPhrase->aToken[i];
          assert( pToken->pDeferred==0 );
          if( pToken->pSegcsr ){
            sqlite3Fts3MsrIncrRestart(pToken->pSegcsr);
          }
        }
        *pRc = fts3EvalPhraseStart(pCsr, 0, pPhrase);
      }
      pPhrase->doclist.pNextDocid = 0;
      pPhrase->doclist.iDocid = 0;
      pPhrase->pOrPoslist = 0;
    }

    pExpr->iDocid = 0;
    pExpr->bEof = 0;
    pExpr->bStart = 0;

    fts3EvalRestart(pCsr, pExpr->pLeft, pRc);
    fts3EvalRestart(pCsr, pExpr->pRight, pRc);
  }
}

/*
** After allocating the Fts3Expr.aMI[] array for each phrase in the
** expression rooted at pExpr, the cursor iterates through all rows matched
** by pExpr, calling this function for each row. This function increments
** the values in Fts3Expr.aMI[] according to the position-list currently
** found in Fts3Expr.pPhrase->doclist.pList for each of the phrase
** expression nodes.
*/
static void fts3EvalUpdateCounts(Fts3Expr *pExpr, int nCol){
  if( pExpr ){
    Fts3Phrase *pPhrase = pExpr->pPhrase;
    if( pPhrase && pPhrase->doclist.pList ){
      int iCol = 0;
      char *p = pPhrase->doclist.pList;

      do{
        u8 c = 0;
        int iCnt = 0;
        while( 0xFE & (*p | c) ){
          if( (c&0x80)==0 ) iCnt++;
          c = *p++ & 0x80;
        }

        /* aMI[iCol*3 + 1] = Number of occurrences
        ** aMI[iCol*3 + 2] = Number of rows containing at least one instance
        */
        pExpr->aMI[iCol*3 + 1] += iCnt;
        pExpr->aMI[iCol*3 + 2] += (iCnt>0);
        if( *p==0x00 ) break;
        p++;
        p += fts3GetVarint32(p, &iCol);
      }while( iCol<nCol );
    }

    fts3EvalUpdateCounts(pExpr->pLeft, nCol);
    fts3EvalUpdateCounts(pExpr->pRight, nCol);
  }
}
