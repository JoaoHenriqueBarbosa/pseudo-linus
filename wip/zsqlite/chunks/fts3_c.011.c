
/*
** Arguments pList/nList contain the doclist for token iToken of phrase p.
** It is merged into the main doclist stored in p->doclist.aAll/nAll.
**
** This function assumes that pList points to a buffer allocated using
** sqlite3_malloc(). This function takes responsibility for eventually
** freeing the buffer.
**
** SQLITE_OK is returned if successful, or SQLITE_NOMEM if an error occurs.
*/
static int fts3EvalPhraseMergeToken(
  Fts3Table *pTab,                /* FTS Table pointer */
  Fts3Phrase *p,                  /* Phrase to merge pList/nList into */
  int iToken,                     /* Token pList/nList corresponds to */
  char *pList,                    /* Pointer to doclist */
  int nList                       /* Number of bytes in pList */
){
  int rc = SQLITE_OK;
  assert( iToken!=p->iDoclistToken );

  if( pList==0 ){
    sqlite3_free(p->doclist.aAll);
    p->doclist.aAll = 0;
    p->doclist.nAll = 0;
  }

  else if( p->iDoclistToken<0 ){
    p->doclist.aAll = pList;
    p->doclist.nAll = nList;
  }

  else if( p->doclist.aAll==0 ){
    sqlite3_free(pList);
  }

  else {
    char *pLeft;
    char *pRight;
    int nLeft;
    int nRight;
    int nDiff;

    if( p->iDoclistToken<iToken ){
      pLeft = p->doclist.aAll;
      nLeft = p->doclist.nAll;
      pRight = pList;
      nRight = nList;
      nDiff = iToken - p->iDoclistToken;
    }else{
      pRight = p->doclist.aAll;
      nRight = p->doclist.nAll;
      pLeft = pList;
      nLeft = nList;
      nDiff = p->iDoclistToken - iToken;
    }

    rc = fts3DoclistPhraseMerge(
        pTab->bDescIdx, nDiff, pLeft, nLeft, &pRight, &nRight
    );
    sqlite3_free(pLeft);
    p->doclist.aAll = pRight;
    p->doclist.nAll = nRight;
  }

  if( iToken>p->iDoclistToken ) p->iDoclistToken = iToken;
  return rc;
}

/*
** Load the doclist for phrase p into p->doclist.aAll/nAll. The loaded doclist
** does not take deferred tokens into account.
**
** SQLITE_OK is returned if no error occurs, otherwise an SQLite error code.
*/
static int fts3EvalPhraseLoad(
  Fts3Cursor *pCsr,               /* FTS Cursor handle */
  Fts3Phrase *p                   /* Phrase object */
){
  Fts3Table *pTab = (Fts3Table *)pCsr->base.pVtab;
  int iToken;
  int rc = SQLITE_OK;

  for(iToken=0; rc==SQLITE_OK && iToken<p->nToken; iToken++){
    Fts3PhraseToken *pToken = &p->aToken[iToken];
    assert( pToken->pDeferred==0 || pToken->pSegcsr==0 );

    if( pToken->pSegcsr ){
      int nThis = 0;
      char *pThis = 0;
      rc = fts3TermSelect(pTab, pToken, p->iColumn, &nThis, &pThis);
      if( rc==SQLITE_OK ){
        rc = fts3EvalPhraseMergeToken(pTab, p, iToken, pThis, nThis);
      }
    }
    assert( pToken->pSegcsr==0 );
  }

  return rc;
}

#ifndef SQLITE_DISABLE_FTS4_DEFERRED
/*
** This function is called on each phrase after the position lists for
** any deferred tokens have been loaded into memory. It updates the phrases
** current position list to include only those positions that are really
** instances of the phrase (after considering deferred tokens). If this
** means that the phrase does not appear in the current row, doclist.pList
** and doclist.nList are both zeroed.
**
** SQLITE_OK is returned if no error occurs, otherwise an SQLite error code.
*/
static int fts3EvalDeferredPhrase(Fts3Cursor *pCsr, Fts3Phrase *pPhrase){
  int iToken;                     /* Used to iterate through phrase tokens */
  char *aPoslist = 0;             /* Position list for deferred tokens */
  int nPoslist = 0;               /* Number of bytes in aPoslist */
  int iPrev = -1;                 /* Token number of previous deferred token */
  char *aFree = (pPhrase->doclist.bFreeList ? pPhrase->doclist.pList : 0);

  for(iToken=0; iToken<pPhrase->nToken; iToken++){
    Fts3PhraseToken *pToken = &pPhrase->aToken[iToken];
    Fts3DeferredToken *pDeferred = pToken->pDeferred;

    if( pDeferred ){
      char *pList;
      int nList;
      int rc = sqlite3Fts3DeferredTokenList(pDeferred, &pList, &nList);
      if( rc!=SQLITE_OK ) return rc;

      if( pList==0 ){
        sqlite3_free(aPoslist);
        sqlite3_free(aFree);
        pPhrase->doclist.pList = 0;
        pPhrase->doclist.nList = 0;
        return SQLITE_OK;

      }else if( aPoslist==0 ){
        aPoslist = pList;
        nPoslist = nList;

      }else{
        char *aOut = pList;
        char *p1 = aPoslist;
        char *p2 = aOut;

        assert( iPrev>=0 );
        fts3PoslistPhraseMerge(&aOut, iToken-iPrev, 0, 1, &p1, &p2);
        sqlite3_free(aPoslist);
        aPoslist = pList;
        nPoslist = (int)(aOut - aPoslist);
        if( nPoslist==0 ){
          sqlite3_free(aPoslist);
          sqlite3_free(aFree);
          pPhrase->doclist.pList = 0;
          pPhrase->doclist.nList = 0;
          return SQLITE_OK;
        }
      }
      iPrev = iToken;
    }
  }

  if( iPrev>=0 ){
    int nMaxUndeferred = pPhrase->iDoclistToken;
    if( nMaxUndeferred<0 ){
      pPhrase->doclist.pList = aPoslist;
      pPhrase->doclist.nList = nPoslist;
      pPhrase->doclist.iDocid = pCsr->iPrevId;
      pPhrase->doclist.bFreeList = 1;
    }else{
      int nDistance;
      char *p1;
      char *p2;
      char *aOut;

      if( nMaxUndeferred>iPrev ){
        p1 = aPoslist;
        p2 = pPhrase->doclist.pList;
        nDistance = nMaxUndeferred - iPrev;
      }else{
        p1 = pPhrase->doclist.pList;
        p2 = aPoslist;
        nDistance = iPrev - nMaxUndeferred;
      }

      aOut = (char *)sqlite3Fts3MallocZero(nPoslist+FTS3_BUFFER_PADDING);
      if( !aOut ){
        sqlite3_free(aPoslist);
        return SQLITE_NOMEM;
      }

      pPhrase->doclist.pList = aOut;
      assert( p1 && p2 );
      if( fts3PoslistPhraseMerge(&aOut, nDistance, 0, 1, &p1, &p2) ){
        pPhrase->doclist.bFreeList = 1;
        pPhrase->doclist.nList = (int)(aOut - pPhrase->doclist.pList);
      }else{
        sqlite3_free(aOut);
        pPhrase->doclist.pList = 0;
        pPhrase->doclist.nList = 0;
      }
      sqlite3_free(aPoslist);
    }
  }

  if( pPhrase->doclist.pList!=aFree ) sqlite3_free(aFree);
  return SQLITE_OK;
}
#endif /* SQLITE_DISABLE_FTS4_DEFERRED */

/*
** Maximum number of tokens a phrase may have to be considered for the
** incremental doclists strategy.
*/
#define MAX_INCR_PHRASE_TOKENS 4

/*
** This function is called for each Fts3Phrase in a full-text query
** expression to initialize the mechanism for returning rows. Once this
** function has been called successfully on an Fts3Phrase, it may be
** used with fts3EvalPhraseNext() to iterate through the matching docids.
**
** If parameter bOptOk is true, then the phrase may (or may not) use the
** incremental loading strategy. Otherwise, the entire doclist is loaded into
** memory within this call.
**
** SQLITE_OK is returned if no error occurs, otherwise an SQLite error code.
*/
static int fts3EvalPhraseStart(Fts3Cursor *pCsr, int bOptOk, Fts3Phrase *p){
  Fts3Table *pTab = (Fts3Table *)pCsr->base.pVtab;
  int rc = SQLITE_OK;             /* Error code */
  int i;

  /* Determine if doclists may be loaded from disk incrementally. This is
  ** possible if the bOptOk argument is true, the FTS doclists will be
  ** scanned in forward order, and the phrase consists of
  ** MAX_INCR_PHRASE_TOKENS or fewer tokens, none of which are are "^first"
  ** tokens or prefix tokens that cannot use a prefix-index.  */
  int bHaveIncr = 0;
  int bIncrOk = (bOptOk
   && pCsr->bDesc==pTab->bDescIdx
   && p->nToken<=MAX_INCR_PHRASE_TOKENS && p->nToken>0
#if defined(SQLITE_DEBUG) || defined(SQLITE_TEST)
   && pTab->bNoIncrDoclist==0
#endif
  );
  for(i=0; bIncrOk==1 && i<p->nToken; i++){
    Fts3PhraseToken *pToken = &p->aToken[i];
    if( pToken->bFirst || (pToken->pSegcsr!=0 && !pToken->pSegcsr->bLookup) ){
      bIncrOk = 0;
    }
    if( pToken->pSegcsr ) bHaveIncr = 1;
  }

  if( bIncrOk && bHaveIncr ){
    /* Use the incremental approach. */
    int iCol = (p->iColumn >= pTab->nColumn ? -1 : p->iColumn);
    for(i=0; rc==SQLITE_OK && i<p->nToken; i++){
      Fts3PhraseToken *pToken = &p->aToken[i];
      Fts3MultiSegReader *pSegcsr = pToken->pSegcsr;
      if( pSegcsr ){
        rc = sqlite3Fts3MsrIncrStart(pTab, pSegcsr, iCol, pToken->z, pToken->n);
      }
    }
    p->bIncr = 1;
  }else{
    /* Load the full doclist for the phrase into memory. */
    rc = fts3EvalPhraseLoad(pCsr, p);
    p->bIncr = 0;
  }

  assert( rc!=SQLITE_OK || p->nToken<1 || p->aToken[0].pSegcsr==0 || p->bIncr );
  return rc;
}

/*
** This function is used to iterate backwards (from the end to start)
** through doclists. It is used by this module to iterate through phrase
** doclists in reverse and by the fts3_write.c module to iterate through
** pending-terms lists when writing to databases with "order=desc".
**
** The doclist may be sorted in ascending (parameter bDescIdx==0) or
** descending (parameter bDescIdx==1) order of docid. Regardless, this
** function iterates from the end of the doclist to the beginning.
*/
SQLITE_PRIVATE void sqlite3Fts3DoclistPrev(
  int bDescIdx,                   /* True if the doclist is desc */
  char *aDoclist,                 /* Pointer to entire doclist */
  int nDoclist,                   /* Length of aDoclist in bytes */
  char **ppIter,                  /* IN/OUT: Iterator pointer */
  sqlite3_int64 *piDocid,         /* IN/OUT: Docid pointer */
  int *pnList,                    /* OUT: List length pointer */
  u8 *pbEof                       /* OUT: End-of-file flag */
){
  char *p = *ppIter;

  assert( nDoclist>0 );
  assert( *pbEof==0 );
  assert_fts3_nc( p || *piDocid==0 );
  assert( !p || (p>aDoclist && p<&aDoclist[nDoclist]) );

  if( p==0 ){
    sqlite3_int64 iDocid = 0;
    char *pNext = 0;
    char *pDocid = aDoclist;
    char *pEnd = &aDoclist[nDoclist];
    int iMul = 1;

    while( pDocid<pEnd ){
      sqlite3_int64 iDelta;
      pDocid += sqlite3Fts3GetVarint(pDocid, &iDelta);
      iDocid += (iMul * iDelta);
      pNext = pDocid;
      fts3PoslistCopy(0, &pDocid);
      while( pDocid<pEnd && *pDocid==0 ) pDocid++;
      iMul = (bDescIdx ? -1 : 1);
    }

    *pnList = (int)(pEnd - pNext);
    *ppIter = pNext;
    *piDocid = iDocid;
  }else{
    int iMul = (bDescIdx ? -1 : 1);
    sqlite3_int64 iDelta;
    fts3GetReverseVarint(&p, aDoclist, &iDelta);
    *piDocid -= (iMul * iDelta);

    if( p==aDoclist ){
      *pbEof = 1;
    }else{
      char *pSave = p;
      fts3ReversePoslist(aDoclist, &p);
      *pnList = (int)(pSave - p);
    }
    *ppIter = p;
  }
}

/*
** Iterate forwards through a doclist.
*/
SQLITE_PRIVATE void sqlite3Fts3DoclistNext(
  int bDescIdx,                   /* True if the doclist is desc */
  char *aDoclist,                 /* Pointer to entire doclist */
  int nDoclist,                   /* Length of aDoclist in bytes */
  char **ppIter,                  /* IN/OUT: Iterator pointer */
  sqlite3_int64 *piDocid,         /* IN/OUT: Docid pointer */
  u8 *pbEof                       /* OUT: End-of-file flag */
){
  char *p = *ppIter;

  assert( nDoclist>0 );
  assert( *pbEof==0 );
  assert_fts3_nc( p || *piDocid==0 );
  assert( !p || (p>=aDoclist && p<=&aDoclist[nDoclist]) );

  if( p==0 ){
    p = aDoclist;
    p += sqlite3Fts3GetVarint(p, piDocid);
  }else{
    fts3PoslistCopy(0, &p);
    while( p<&aDoclist[nDoclist] && *p==0 ) p++;
    if( p>=&aDoclist[nDoclist] ){
      *pbEof = 1;
    }else{
      sqlite3_int64 iVar;
      p += sqlite3Fts3GetVarint(p, &iVar);
      *piDocid += ((bDescIdx ? -1 : 1) * iVar);
    }
  }

  *ppIter = p;
}
