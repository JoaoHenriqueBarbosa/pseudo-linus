
/*
** Determine the average document (row) size in pages. If successful,
** write this value to *pnPage and return SQLITE_OK. Otherwise, return
** an SQLite error code.
**
** The average document size in pages is calculated by first calculating
** determining the average size in bytes, B. If B is less than the amount
** of data that will fit on a single leaf page of an intkey table in
** this database, then the average docsize is 1. Otherwise, it is 1 plus
** the number of overflow pages consumed by a record B bytes in size.
*/
static int fts3EvalAverageDocsize(Fts3Cursor *pCsr, int *pnPage){
  int rc = SQLITE_OK;
  if( pCsr->nRowAvg==0 ){
    /* The average document size, which is required to calculate the cost
    ** of each doclist, has not yet been determined. Read the required
    ** data from the %_stat table to calculate it.
    **
    ** Entry 0 of the %_stat table is a blob containing (nCol+1) FTS3
    ** varints, where nCol is the number of columns in the FTS3 table.
    ** The first varint is the number of documents currently stored in
    ** the table. The following nCol varints contain the total amount of
    ** data stored in all rows of each column of the table, from left
    ** to right.
    */
    Fts3Table *p = (Fts3Table*)pCsr->base.pVtab;
    sqlite3_stmt *pStmt;
    sqlite3_int64 nDoc = 0;
    sqlite3_int64 nByte = 0;
    const char *pEnd;
    const char *a;

    rc = sqlite3Fts3SelectDoctotal(p, &pStmt);
    if( rc!=SQLITE_OK ) return rc;
    a = sqlite3_column_blob(pStmt, 0);
    testcase( a==0 );  /* If %_stat.value set to X'' */
    if( a ){
      pEnd = &a[sqlite3_column_bytes(pStmt, 0)];
      a += sqlite3Fts3GetVarintBounded(a, pEnd, &nDoc);
      while( a<pEnd ){
        a += sqlite3Fts3GetVarintBounded(a, pEnd, &nByte);
      }
    }
    if( nDoc==0 || nByte==0 ){
      sqlite3_reset(pStmt);
      return FTS_CORRUPT_VTAB;
    }

    pCsr->nDoc = nDoc;
    pCsr->nRowAvg = (int)(((nByte / nDoc) + p->nPgsz) / p->nPgsz);
    assert( pCsr->nRowAvg>0 );
    rc = sqlite3_reset(pStmt);
  }

  *pnPage = pCsr->nRowAvg;
  return rc;
}

/*
** This function is called to select the tokens (if any) that will be
** deferred. The array aTC[] has already been populated when this is
** called.
**
** This function is called once for each AND/NEAR cluster in the
** expression. Each invocation determines which tokens to defer within
** the cluster with root node pRoot. See comments above the definition
** of struct Fts3TokenAndCost for more details.
**
** If no error occurs, SQLITE_OK is returned and sqlite3Fts3DeferToken()
** called on each token to defer. Otherwise, an SQLite error code is
** returned.
*/
static int fts3EvalSelectDeferred(
  Fts3Cursor *pCsr,               /* FTS Cursor handle */
  Fts3Expr *pRoot,                /* Consider tokens with this root node */
  Fts3TokenAndCost *aTC,          /* Array of expression tokens and costs */
  int nTC                         /* Number of entries in aTC[] */
){
  Fts3Table *pTab = (Fts3Table *)pCsr->base.pVtab;
  int nDocSize = 0;               /* Number of pages per doc loaded */
  int rc = SQLITE_OK;             /* Return code */
  int ii;                         /* Iterator variable for various purposes */
  int nOvfl = 0;                  /* Total overflow pages used by doclists */
  int nToken = 0;                 /* Total number of tokens in cluster */

  int nMinEst = 0;                /* The minimum count for any phrase so far. */
  int nLoad4 = 1;                 /* (Phrases that will be loaded)^4. */

  /* Tokens are never deferred for FTS tables created using the content=xxx
  ** option. The reason being that it is not guaranteed that the content
  ** table actually contains the same data as the index. To prevent this from
  ** causing any problems, the deferred token optimization is completely
  ** disabled for content=xxx tables. */
  if( pTab->zContentTbl ){
    return SQLITE_OK;
  }

  /* Count the tokens in this AND/NEAR cluster. If none of the doclists
  ** associated with the tokens spill onto overflow pages, or if there is
  ** only 1 token, exit early. No tokens to defer in this case. */
  for(ii=0; ii<nTC; ii++){
    if( aTC[ii].pRoot==pRoot ){
      nOvfl += aTC[ii].nOvfl;
      nToken++;
    }
  }
  if( nOvfl==0 || nToken<2 ) return SQLITE_OK;

  /* Obtain the average docsize (in pages). */
  rc = fts3EvalAverageDocsize(pCsr, &nDocSize);
  assert( rc!=SQLITE_OK || nDocSize>0 );


  /* Iterate through all tokens in this AND/NEAR cluster, in ascending order
  ** of the number of overflow pages that will be loaded by the pager layer
  ** to retrieve the entire doclist for the token from the full-text index.
  ** Load the doclists for tokens that are either:
  **
  **   a. The cheapest token in the entire query (i.e. the one visited by the
  **      first iteration of this loop), or
  **
  **   b. Part of a multi-token phrase.
  **
  ** After each token doclist is loaded, merge it with the others from the
  ** same phrase and count the number of documents that the merged doclist
  ** contains. Set variable "nMinEst" to the smallest number of documents in
  ** any phrase doclist for which 1 or more token doclists have been loaded.
  ** Let nOther be the number of other phrases for which it is certain that
  ** one or more tokens will not be deferred.
  **
  ** Then, for each token, defer it if loading the doclist would result in
  ** loading N or more overflow pages into memory, where N is computed as:
  **
  **    (nMinEst + 4^nOther - 1) / (4^nOther)
  */
  for(ii=0; ii<nToken && rc==SQLITE_OK; ii++){
    int iTC;                      /* Used to iterate through aTC[] array. */
    Fts3TokenAndCost *pTC = 0;    /* Set to cheapest remaining token. */

    /* Set pTC to point to the cheapest remaining token. */
    for(iTC=0; iTC<nTC; iTC++){
      if( aTC[iTC].pToken && aTC[iTC].pRoot==pRoot
       && (!pTC || aTC[iTC].nOvfl<pTC->nOvfl)
      ){
        pTC = &aTC[iTC];
      }
    }
    assert( pTC );

    if( ii && pTC->nOvfl>=((nMinEst+(nLoad4/4)-1)/(nLoad4/4))*nDocSize ){
      /* The number of overflow pages to load for this (and therefore all
      ** subsequent) tokens is greater than the estimated number of pages
      ** that will be loaded if all subsequent tokens are deferred.
      */
      Fts3PhraseToken *pToken = pTC->pToken;
      rc = sqlite3Fts3DeferToken(pCsr, pToken, pTC->iCol);
      fts3SegReaderCursorFree(pToken->pSegcsr);
      pToken->pSegcsr = 0;
    }else{
      /* Set nLoad4 to the value of (4^nOther) for the next iteration of the
      ** for-loop. Except, limit the value to 2^24 to prevent it from
      ** overflowing the 32-bit integer it is stored in. */
      if( ii<12 ) nLoad4 = nLoad4*4;

      if( ii==0 || (pTC->pPhrase->nToken>1 && ii!=nToken-1) ){
        /* Either this is the cheapest token in the entire query, or it is
        ** part of a multi-token phrase. Either way, the entire doclist will
        ** (eventually) be loaded into memory. It may as well be now. */
        Fts3PhraseToken *pToken = pTC->pToken;
        int nList = 0;
        char *pList = 0;
        rc = fts3TermSelect(pTab, pToken, pTC->iCol, &nList, &pList);
        assert( rc==SQLITE_OK || pList==0 );
        if( rc==SQLITE_OK ){
          rc = fts3EvalPhraseMergeToken(
              pTab, pTC->pPhrase, pTC->iToken,pList,nList
          );
        }
        if( rc==SQLITE_OK ){
          int nCount;
          nCount = fts3DoclistCountDocids(
              pTC->pPhrase->doclist.aAll, pTC->pPhrase->doclist.nAll
          );
          if( ii==0 || nCount<nMinEst ) nMinEst = nCount;
        }
      }
    }
    pTC->pToken = 0;
  }

  return rc;
}

/*
** This function is called from within the xFilter method. It initializes
** the full-text query currently stored in pCsr->pExpr. To iterate through
** the results of a query, the caller does:
**
**    fts3EvalStart(pCsr);
**    while( 1 ){
**      fts3EvalNext(pCsr);
**      if( pCsr->bEof ) break;
**      ... return row pCsr->iPrevId to the caller ...
**    }
*/
static int fts3EvalStart(Fts3Cursor *pCsr){
  Fts3Table *pTab = (Fts3Table *)pCsr->base.pVtab;
  int rc = SQLITE_OK;
  int nToken = 0;
  int nOr = 0;

  /* Allocate a MultiSegReader for each token in the expression. */
  fts3EvalAllocateReaders(pCsr, pCsr->pExpr, &nToken, &nOr, &rc);

  /* Determine which, if any, tokens in the expression should be deferred. */
#ifndef SQLITE_DISABLE_FTS4_DEFERRED
  if( rc==SQLITE_OK && nToken>1 && pTab->bFts4 ){
    Fts3TokenAndCost *aTC;
    aTC = (Fts3TokenAndCost *)sqlite3_malloc64(
        sizeof(Fts3TokenAndCost) * nToken
      + sizeof(Fts3Expr *) * nOr * 2
    );

    if( !aTC ){
      rc = SQLITE_NOMEM;
    }else{
      Fts3Expr **apOr = (Fts3Expr **)&aTC[nToken];
      int ii;
      Fts3TokenAndCost *pTC = aTC;
      Fts3Expr **ppOr = apOr;

      fts3EvalTokenCosts(pCsr, 0, pCsr->pExpr, &pTC, &ppOr, &rc);
      nToken = (int)(pTC-aTC);
      nOr = (int)(ppOr-apOr);

      if( rc==SQLITE_OK ){
        rc = fts3EvalSelectDeferred(pCsr, 0, aTC, nToken);
        for(ii=0; rc==SQLITE_OK && ii<nOr; ii++){
          rc = fts3EvalSelectDeferred(pCsr, apOr[ii], aTC, nToken);
        }
      }

      sqlite3_free(aTC);
    }
  }
#endif

  fts3EvalStartReaders(pCsr, pCsr->pExpr, &rc);
  return rc;
}

/*
** Invalidate the current position list for phrase pPhrase.
*/
static void fts3EvalInvalidatePoslist(Fts3Phrase *pPhrase){
  if( pPhrase->doclist.bFreeList ){
    sqlite3_free(pPhrase->doclist.pList);
  }
  pPhrase->doclist.pList = 0;
  pPhrase->doclist.nList = 0;
  pPhrase->doclist.bFreeList = 0;
}

/*
** This function is called to edit the position list associated with
** the phrase object passed as the fifth argument according to a NEAR
** condition. For example:
**
**     abc NEAR/5 "def ghi"
**
** Parameter nNear is passed the NEAR distance of the expression (5 in
** the example above). When this function is called, *paPoslist points to
** the position list, and *pnToken is the number of phrase tokens in the
** phrase on the other side of the NEAR operator to pPhrase. For example,
** if pPhrase refers to the "def ghi" phrase, then *paPoslist points to
** the position list associated with phrase "abc".
**
** All positions in the pPhrase position list that are not sufficiently
** close to a position in the *paPoslist position list are removed. If this
** leaves 0 positions, zero is returned. Otherwise, non-zero.
**
** Before returning, *paPoslist is set to point to the position lsit
** associated with pPhrase. And *pnToken is set to the number of tokens in
** pPhrase.
*/
static int fts3EvalNearTrim(
  int nNear,                      /* NEAR distance. As in "NEAR/nNear". */
  char *aTmp,                     /* Temporary space to use */
  char **paPoslist,               /* IN/OUT: Position list */
  int *pnToken,                   /* IN/OUT: Tokens in phrase of *paPoslist */
  Fts3Phrase *pPhrase             /* The phrase object to trim the doclist of */
){
  int nParam1 = nNear + pPhrase->nToken;
  int nParam2 = nNear + *pnToken;
  int nNew;
  char *p2;
  char *pOut;
  int res;

  assert( pPhrase->doclist.pList );

  p2 = pOut = pPhrase->doclist.pList;
  res = fts3PoslistNearMerge(
    &pOut, aTmp, nParam1, nParam2, paPoslist, &p2
  );
  if( res ){
    nNew = (int)(pOut - pPhrase->doclist.pList) - 1;
    assert_fts3_nc( nNew<=pPhrase->doclist.nList && nNew>0 );
    if( nNew>=0 && nNew<=pPhrase->doclist.nList ){
      assert( pPhrase->doclist.pList[nNew]=='\0' );
      memset(&pPhrase->doclist.pList[nNew], 0, pPhrase->doclist.nList - nNew);
      pPhrase->doclist.nList = nNew;
    }
    *paPoslist = pPhrase->doclist.pList;
    *pnToken = pPhrase->nToken;
  }

  return res;
}

/*
** This function is a no-op if *pRc is other than SQLITE_OK when it is called.
** Otherwise, it advances the expression passed as the second argument to
** point to the next matching row in the database. Expressions iterate through
** matching rows in docid order. Ascending order if Fts3Cursor.bDesc is zero,
** or descending if it is non-zero.
**
** If an error occurs, *pRc is set to an SQLite error code. Otherwise, if
** successful, the following variables in pExpr are set:
**
**   Fts3Expr.bEof                (non-zero if EOF - there is no next row)
**   Fts3Expr.iDocid              (valid if bEof==0. The docid of the next row)
**
** If the expression is of type FTSQUERY_PHRASE, and the expression is not
** at EOF, then the following variables are populated with the position list
** for the phrase for the visited row:
**
**   FTs3Expr.pPhrase->doclist.nList        (length of pList in bytes)
**   FTs3Expr.pPhrase->doclist.pList        (pointer to position list)
**
** It says above that this function advances the expression to the next
** matching row. This is usually true, but there are the following exceptions:
**
**   1. Deferred tokens are not taken into account. If a phrase consists
**      entirely of deferred tokens, it is assumed to match every row in
**      the db. In this case the position-list is not populated at all.
**
**      Or, if a phrase contains one or more deferred tokens and one or
**      more non-deferred tokens, then the expression is advanced to the
**      next possible match, considering only non-deferred tokens. In other
**      words, if the phrase is "A B C", and "B" is deferred, the expression
**      is advanced to the next row that contains an instance of "A * C",
**      where "*" may match any single token. The position list in this case
**      is populated as for "A * C" before returning.
**
**   2. NEAR is treated as AND. If the expression is "x NEAR y", it is
**      advanced to point to the next row that matches "x AND y".
**
** See sqlite3Fts3EvalTestDeferred() for details on testing if a row is
** really a match, taking into account deferred tokens and NEAR operators.
*/
static void fts3EvalNextRow(
  Fts3Cursor *pCsr,               /* FTS Cursor handle */
  Fts3Expr *pExpr,                /* Expr. to advance to next matching row */
  int *pRc                        /* IN/OUT: Error code */
){
  if( *pRc==SQLITE_OK && pExpr->bEof==0 ){
    int bDescDoclist = pCsr->bDesc;         /* Used by DOCID_CMP() macro */
    pExpr->bStart = 1;

    switch( pExpr->eType ){
      case FTSQUERY_NEAR:
      case FTSQUERY_AND: {
        Fts3Expr *pLeft = pExpr->pLeft;
        Fts3Expr *pRight = pExpr->pRight;
        assert( !pLeft->bDeferred || !pRight->bDeferred );

        if( pLeft->bDeferred ){
          /* LHS is entirely deferred. So we assume it matches every row.
          ** Advance the RHS iterator to find the next row visited. */
          fts3EvalNextRow(pCsr, pRight, pRc);
          pExpr->iDocid = pRight->iDocid;
          pExpr->bEof = pRight->bEof;
        }else if( pRight->bDeferred ){
          /* RHS is entirely deferred. So we assume it matches every row.
          ** Advance the LHS iterator to find the next row visited. */
          fts3EvalNextRow(pCsr, pLeft, pRc);
          pExpr->iDocid = pLeft->iDocid;
          pExpr->bEof = pLeft->bEof;
        }else{
          /* Neither the RHS or LHS are deferred. */
          fts3EvalNextRow(pCsr, pLeft, pRc);
          fts3EvalNextRow(pCsr, pRight, pRc);
          while( !pLeft->bEof && !pRight->bEof && *pRc==SQLITE_OK ){
            sqlite3_int64 iDiff = DOCID_CMP(pLeft->iDocid, pRight->iDocid);
            if( iDiff==0 ) break;
            if( iDiff<0 ){
              fts3EvalNextRow(pCsr, pLeft, pRc);
            }else{
              fts3EvalNextRow(pCsr, pRight, pRc);
            }
          }
          pExpr->iDocid = pLeft->iDocid;
          pExpr->bEof = (pLeft->bEof || pRight->bEof);
          if( pExpr->eType==FTSQUERY_NEAR && pExpr->bEof ){
            assert( pRight->eType==FTSQUERY_PHRASE );
            if( pRight->pPhrase->doclist.aAll ){
              Fts3Doclist *pDl = &pRight->pPhrase->doclist;
              while( *pRc==SQLITE_OK && pRight->bEof==0 ){
                memset(pDl->pList, 0, pDl->nList);
                fts3EvalNextRow(pCsr, pRight, pRc);
              }
            }
            if( pLeft->pPhrase && pLeft->pPhrase->doclist.aAll ){
              Fts3Doclist *pDl = &pLeft->pPhrase->doclist;
              while( *pRc==SQLITE_OK && pLeft->bEof==0 ){
                memset(pDl->pList, 0, pDl->nList);
                fts3EvalNextRow(pCsr, pLeft, pRc);
              }
            }
            pRight->bEof = pLeft->bEof = 1;
          }
        }
        break;
      }

      case FTSQUERY_OR: {
        Fts3Expr *pLeft = pExpr->pLeft;
        Fts3Expr *pRight = pExpr->pRight;
        sqlite3_int64 iCmp = DOCID_CMP(pLeft->iDocid, pRight->iDocid);

        assert_fts3_nc( pLeft->bStart || pLeft->iDocid==pRight->iDocid );
        assert_fts3_nc( pRight->bStart || pLeft->iDocid==pRight->iDocid );

        if( pRight->bEof || (pLeft->bEof==0 && iCmp<0) ){
          fts3EvalNextRow(pCsr, pLeft, pRc);
        }else if( pLeft->bEof || iCmp>0 ){
          fts3EvalNextRow(pCsr, pRight, pRc);
        }else{
          fts3EvalNextRow(pCsr, pLeft, pRc);
          fts3EvalNextRow(pCsr, pRight, pRc);
        }

        pExpr->bEof = (pLeft->bEof && pRight->bEof);
        iCmp = DOCID_CMP(pLeft->iDocid, pRight->iDocid);
        if( pRight->bEof || (pLeft->bEof==0 &&  iCmp<0) ){
          pExpr->iDocid = pLeft->iDocid;
        }else{
          pExpr->iDocid = pRight->iDocid;
        }

        break;
      }

      case FTSQUERY_NOT: {
        Fts3Expr *pLeft = pExpr->pLeft;
        Fts3Expr *pRight = pExpr->pRight;

        if( pRight->bStart==0 ){
          fts3EvalNextRow(pCsr, pRight, pRc);
          assert( *pRc!=SQLITE_OK || pRight->bStart );
        }

        fts3EvalNextRow(pCsr, pLeft, pRc);
        if( pLeft->bEof==0 ){
          while( !*pRc
              && !pRight->bEof
              && DOCID_CMP(pLeft->iDocid, pRight->iDocid)>0
          ){
            fts3EvalNextRow(pCsr, pRight, pRc);
          }
        }
        pExpr->iDocid = pLeft->iDocid;
        pExpr->bEof = pLeft->bEof;
        break;
      }

      default: {
        Fts3Phrase *pPhrase = pExpr->pPhrase;
        fts3EvalInvalidatePoslist(pPhrase);
        *pRc = fts3EvalPhraseNext(pCsr, pPhrase, &pExpr->bEof);
        pExpr->iDocid = pPhrase->doclist.iDocid;
        break;
      }
    }
  }
}
