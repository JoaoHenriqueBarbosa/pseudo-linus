
/*
** Advance the iterator pDL to the next entry in pDL->aAll/nAll. Set *pbEof
** to true if EOF is reached.
*/
static void fts3EvalDlPhraseNext(
  Fts3Table *pTab,
  Fts3Doclist *pDL,
  u8 *pbEof
){
  char *pIter;                            /* Used to iterate through aAll */
  char *pEnd;                             /* 1 byte past end of aAll */

  if( pDL->pNextDocid ){
    pIter = pDL->pNextDocid;
    assert( pDL->aAll!=0 || pIter==0 );
  }else{
    pIter = pDL->aAll;
  }

  if( pIter==0 || pIter>=(pEnd = pDL->aAll + pDL->nAll) ){
    /* We have already reached the end of this doclist. EOF. */
    *pbEof = 1;
  }else{
    sqlite3_int64 iDelta;
    pIter += sqlite3Fts3GetVarint(pIter, &iDelta);
    if( pTab->bDescIdx==0 || pDL->pNextDocid==0 ){
      pDL->iDocid += iDelta;
    }else{
      pDL->iDocid -= iDelta;
    }
    pDL->pList = pIter;
    fts3PoslistCopy(0, &pIter);
    pDL->nList = (int)(pIter - pDL->pList);

    /* pIter now points just past the 0x00 that terminates the position-
    ** list for document pDL->iDocid. However, if this position-list was
    ** edited in place by fts3EvalNearTrim(), then pIter may not actually
    ** point to the start of the next docid value. The following line deals
    ** with this case by advancing pIter past the zero-padding added by
    ** fts3EvalNearTrim().  */
    while( pIter<pEnd && *pIter==0 ) pIter++;

    pDL->pNextDocid = pIter;
    assert( pIter>=&pDL->aAll[pDL->nAll] || *pIter );
    *pbEof = 0;
  }
}

/*
** Helper type used by fts3EvalIncrPhraseNext() and incrPhraseTokenNext().
*/
typedef struct TokenDoclist TokenDoclist;
struct TokenDoclist {
  int bIgnore;
  sqlite3_int64 iDocid;
  char *pList;
  int nList;
};

/*
** Token pToken is an incrementally loaded token that is part of a
** multi-token phrase. Advance it to the next matching document in the
** database and populate output variable *p with the details of the new
** entry. Or, if the iterator has reached EOF, set *pbEof to true.
**
** If an error occurs, return an SQLite error code. Otherwise, return
** SQLITE_OK.
*/
static int incrPhraseTokenNext(
  Fts3Table *pTab,                /* Virtual table handle */
  Fts3Phrase *pPhrase,            /* Phrase to advance token of */
  int iToken,                     /* Specific token to advance */
  TokenDoclist *p,                /* OUT: Docid and doclist for new entry */
  u8 *pbEof                       /* OUT: True if iterator is at EOF */
){
  int rc = SQLITE_OK;

  if( pPhrase->iDoclistToken==iToken ){
    assert( p->bIgnore==0 );
    assert( pPhrase->aToken[iToken].pSegcsr==0 );
    fts3EvalDlPhraseNext(pTab, &pPhrase->doclist, pbEof);
    p->pList = pPhrase->doclist.pList;
    p->nList = pPhrase->doclist.nList;
    p->iDocid = pPhrase->doclist.iDocid;
  }else{
    Fts3PhraseToken *pToken = &pPhrase->aToken[iToken];
    assert( pToken->pDeferred==0 );
    assert( pToken->pSegcsr || pPhrase->iDoclistToken>=0 );
    if( pToken->pSegcsr ){
      assert( p->bIgnore==0 );
      rc = sqlite3Fts3MsrIncrNext(
          pTab, pToken->pSegcsr, &p->iDocid, &p->pList, &p->nList
      );
      if( p->pList==0 ) *pbEof = 1;
    }else{
      p->bIgnore = 1;
    }
  }

  return rc;
}


/*
** The phrase iterator passed as the second argument:
**
**   * features at least one token that uses an incremental doclist, and
**
**   * does not contain any deferred tokens.
**
** Advance it to the next matching documnent in the database and populate
** the Fts3Doclist.pList and nList fields.
**
** If there is no "next" entry and no error occurs, then *pbEof is set to
** 1 before returning. Otherwise, if no error occurs and the iterator is
** successfully advanced, *pbEof is set to 0.
**
** If an error occurs, return an SQLite error code. Otherwise, return
** SQLITE_OK.
*/
static int fts3EvalIncrPhraseNext(
  Fts3Cursor *pCsr,               /* FTS Cursor handle */
  Fts3Phrase *p,                  /* Phrase object to advance to next docid */
  u8 *pbEof                       /* OUT: Set to 1 if EOF */
){
  int rc = SQLITE_OK;
  Fts3Doclist *pDL = &p->doclist;
  Fts3Table *pTab = (Fts3Table *)pCsr->base.pVtab;
  u8 bEof = 0;

  /* This is only called if it is guaranteed that the phrase has at least
  ** one incremental token. In which case the bIncr flag is set. */
  assert( p->bIncr==1 );

  if( p->nToken==1 ){
    rc = sqlite3Fts3MsrIncrNext(pTab, p->aToken[0].pSegcsr,
        &pDL->iDocid, &pDL->pList, &pDL->nList
    );
    if( pDL->pList==0 ) bEof = 1;
  }else{
    int bDescDoclist = pCsr->bDesc;
    struct TokenDoclist a[MAX_INCR_PHRASE_TOKENS];

    memset(a, 0, sizeof(a));
    assert( p->nToken<=MAX_INCR_PHRASE_TOKENS );
    assert( p->iDoclistToken<MAX_INCR_PHRASE_TOKENS );

    while( bEof==0 ){
      int bMaxSet = 0;
      sqlite3_int64 iMax = 0;     /* Largest docid for all iterators */
      int i;                      /* Used to iterate through tokens */

      /* Advance the iterator for each token in the phrase once. */
      for(i=0; rc==SQLITE_OK && i<p->nToken && bEof==0; i++){
        rc = incrPhraseTokenNext(pTab, p, i, &a[i], &bEof);
        if( a[i].bIgnore==0 && (bMaxSet==0 || DOCID_CMP(iMax, a[i].iDocid)<0) ){
          iMax = a[i].iDocid;
          bMaxSet = 1;
        }
      }
      assert( rc!=SQLITE_OK || (p->nToken>=1 && a[p->nToken-1].bIgnore==0) );
      assert( rc!=SQLITE_OK || bMaxSet );

      /* Keep advancing iterators until they all point to the same document */
      for(i=0; i<p->nToken; i++){
        while( rc==SQLITE_OK && bEof==0
            && a[i].bIgnore==0 && DOCID_CMP(a[i].iDocid, iMax)<0
        ){
          rc = incrPhraseTokenNext(pTab, p, i, &a[i], &bEof);
          if( DOCID_CMP(a[i].iDocid, iMax)>0 ){
            iMax = a[i].iDocid;
            i = 0;
          }
        }
      }

      /* Check if the current entries really are a phrase match */
      if( bEof==0 ){
        int nList = 0;
        int nByte = a[p->nToken-1].nList;
        char *aDoclist = sqlite3_malloc64((i64)nByte+FTS3_BUFFER_PADDING);
        if( !aDoclist ) return SQLITE_NOMEM;
        memcpy(aDoclist, a[p->nToken-1].pList, nByte+1);
        memset(&aDoclist[nByte], 0, FTS3_BUFFER_PADDING);

        for(i=0; i<(p->nToken-1); i++){
          if( a[i].bIgnore==0 ){
            char *pL = a[i].pList;
            char *pR = aDoclist;
            char *pOut = aDoclist;
            int nDist = p->nToken-1-i;
            int res = fts3PoslistPhraseMerge(&pOut, nDist, 0, 1, &pL, &pR);
            if( res==0 ) break;
            nList = (int)(pOut - aDoclist);
          }
        }
        if( i==(p->nToken-1) ){
          pDL->iDocid = iMax;
          pDL->pList = aDoclist;
          pDL->nList = nList;
          pDL->bFreeList = 1;
          break;
        }
        sqlite3_free(aDoclist);
      }
    }
  }

  *pbEof = bEof;
  return rc;
}

/*
** Attempt to move the phrase iterator to point to the next matching docid.
** If an error occurs, return an SQLite error code. Otherwise, return
** SQLITE_OK.
**
** If there is no "next" entry and no error occurs, then *pbEof is set to
** 1 before returning. Otherwise, if no error occurs and the iterator is
** successfully advanced, *pbEof is set to 0.
*/
static int fts3EvalPhraseNext(
  Fts3Cursor *pCsr,               /* FTS Cursor handle */
  Fts3Phrase *p,                  /* Phrase object to advance to next docid */
  u8 *pbEof                       /* OUT: Set to 1 if EOF */
){
  int rc = SQLITE_OK;
  Fts3Doclist *pDL = &p->doclist;
  Fts3Table *pTab = (Fts3Table *)pCsr->base.pVtab;

  if( p->bIncr ){
    rc = fts3EvalIncrPhraseNext(pCsr, p, pbEof);
  }else if( pCsr->bDesc!=pTab->bDescIdx && pDL->nAll ){
    sqlite3Fts3DoclistPrev(pTab->bDescIdx, pDL->aAll, pDL->nAll,
        &pDL->pNextDocid, &pDL->iDocid, &pDL->nList, pbEof
    );
    pDL->pList = pDL->pNextDocid;
  }else{
    fts3EvalDlPhraseNext(pTab, pDL, pbEof);
  }

  return rc;
}

/*
**
** If *pRc is not SQLITE_OK when this function is called, it is a no-op.
** Otherwise, fts3EvalPhraseStart() is called on all phrases within the
** expression. Also the Fts3Expr.bDeferred variable is set to true for any
** expressions for which all descendent tokens are deferred.
**
** If parameter bOptOk is zero, then it is guaranteed that the
** Fts3Phrase.doclist.aAll/nAll variables contain the entire doclist for
** each phrase in the expression (subject to deferred token processing).
** Or, if bOptOk is non-zero, then one or more tokens within the expression
** may be loaded incrementally, meaning doclist.aAll/nAll is not available.
**
** If an error occurs within this function, *pRc is set to an SQLite error
** code before returning.
*/
static void fts3EvalStartReaders(
  Fts3Cursor *pCsr,               /* FTS Cursor handle */
  Fts3Expr *pExpr,                /* Expression to initialize phrases in */
  int *pRc                        /* IN/OUT: Error code */
){
  if( pExpr && SQLITE_OK==*pRc ){
    if( pExpr->eType==FTSQUERY_PHRASE ){
      int nToken = pExpr->pPhrase->nToken;
      if( nToken ){
        int i;
        for(i=0; i<nToken; i++){
          if( pExpr->pPhrase->aToken[i].pDeferred==0 ) break;
        }
        pExpr->bDeferred = (i==nToken);
      }
      *pRc = fts3EvalPhraseStart(pCsr, 1, pExpr->pPhrase);
    }else{
      fts3EvalStartReaders(pCsr, pExpr->pLeft, pRc);
      fts3EvalStartReaders(pCsr, pExpr->pRight, pRc);
      pExpr->bDeferred = (pExpr->pLeft->bDeferred && pExpr->pRight->bDeferred);
    }
  }
}

/*
** An array of the following structures is assembled as part of the process
** of selecting tokens to defer before the query starts executing (as part
** of the xFilter() method). There is one element in the array for each
** token in the FTS expression.
**
** Tokens are divided into AND/NEAR clusters. All tokens in a cluster belong
** to phrases that are connected only by AND and NEAR operators (not OR or
** NOT). When determining tokens to defer, each AND/NEAR cluster is considered
** separately. The root of a tokens AND/NEAR cluster is stored in
** Fts3TokenAndCost.pRoot.
*/
typedef struct Fts3TokenAndCost Fts3TokenAndCost;
struct Fts3TokenAndCost {
  Fts3Phrase *pPhrase;            /* The phrase the token belongs to */
  int iToken;                     /* Position of token in phrase */
  Fts3PhraseToken *pToken;        /* The token itself */
  Fts3Expr *pRoot;                /* Root of NEAR/AND cluster */
  int nOvfl;                      /* Number of overflow pages to load doclist */
  int iCol;                       /* The column the token must match */
};

/*
** This function is used to populate an allocated Fts3TokenAndCost array.
**
** If *pRc is not SQLITE_OK when this function is called, it is a no-op.
** Otherwise, if an error occurs during execution, *pRc is set to an
** SQLite error code.
*/
static void fts3EvalTokenCosts(
  Fts3Cursor *pCsr,               /* FTS Cursor handle */
  Fts3Expr *pRoot,                /* Root of current AND/NEAR cluster */
  Fts3Expr *pExpr,                /* Expression to consider */
  Fts3TokenAndCost **ppTC,        /* Write new entries to *(*ppTC)++ */
  Fts3Expr ***ppOr,               /* Write new OR root to *(*ppOr)++ */
  int *pRc                        /* IN/OUT: Error code */
){
  if( *pRc==SQLITE_OK ){
    if( pExpr->eType==FTSQUERY_PHRASE ){
      Fts3Phrase *pPhrase = pExpr->pPhrase;
      int i;
      for(i=0; *pRc==SQLITE_OK && i<pPhrase->nToken; i++){
        Fts3TokenAndCost *pTC = (*ppTC)++;
        pTC->pPhrase = pPhrase;
        pTC->iToken = i;
        pTC->pRoot = pRoot;
        pTC->pToken = &pPhrase->aToken[i];
        pTC->iCol = pPhrase->iColumn;
        *pRc = sqlite3Fts3MsrOvfl(pCsr, pTC->pToken->pSegcsr, &pTC->nOvfl);
      }
    }else if( pExpr->eType!=FTSQUERY_NOT ){
      assert( pExpr->eType==FTSQUERY_OR
           || pExpr->eType==FTSQUERY_AND
           || pExpr->eType==FTSQUERY_NEAR
      );
      assert( pExpr->pLeft && pExpr->pRight );
      if( pExpr->eType==FTSQUERY_OR ){
        pRoot = pExpr->pLeft;
        **ppOr = pRoot;
        (*ppOr)++;
      }
      fts3EvalTokenCosts(pCsr, pRoot, pExpr->pLeft, ppTC, ppOr, pRc);
      if( pExpr->eType==FTSQUERY_OR ){
        pRoot = pExpr->pRight;
        **ppOr = pRoot;
        (*ppOr)++;
      }
      fts3EvalTokenCosts(pCsr, pRoot, pExpr->pRight, ppTC, ppOr, pRc);
    }
  }
}
