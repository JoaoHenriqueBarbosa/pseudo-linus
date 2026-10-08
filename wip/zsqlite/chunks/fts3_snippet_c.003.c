
/*
** Populate the buffer pInfo->aMatchinfo[] with an array of integers to
** be returned by the matchinfo() function. Argument zArg contains the
** format string passed as the second argument to matchinfo (or the
** default value "pcx" if no second argument was specified). The format
** string has already been validated and the pInfo->aMatchinfo[] array
** is guaranteed to be large enough for the output.
**
** If bGlobal is true, then populate all fields of the matchinfo() output.
** If it is false, then assume that those fields that do not change between
** rows (i.e. FTS3_MATCHINFO_NPHRASE, NCOL, NDOC, AVGLENGTH and part of HITS)
** have already been populated.
**
** Return SQLITE_OK if successful, or an SQLite error code if an error
** occurs. If a value other than SQLITE_OK is returned, the state the
** pInfo->aMatchinfo[] buffer is left in is undefined.
*/
static int fts3MatchinfoValues(
  Fts3Cursor *pCsr,               /* FTS3 cursor object */
  int bGlobal,                    /* True to grab the global stats */
  MatchInfo *pInfo,               /* Matchinfo context object */
  const char *zArg                /* Matchinfo format string */
){
  int rc = SQLITE_OK;
  int i;
  Fts3Table *pTab = (Fts3Table *)pCsr->base.pVtab;
  sqlite3_stmt *pSelect = 0;

  for(i=0; rc==SQLITE_OK && zArg[i]; i++){
    pInfo->flag = zArg[i];
    switch( zArg[i] ){
      case FTS3_MATCHINFO_NPHRASE:
        if( bGlobal ) pInfo->aMatchinfo[0] = pInfo->nPhrase;
        break;

      case FTS3_MATCHINFO_NCOL:
        if( bGlobal ) pInfo->aMatchinfo[0] = pInfo->nCol;
        break;

      case FTS3_MATCHINFO_NDOC:
        if( bGlobal ){
          sqlite3_int64 nDoc = 0;
          rc = fts3MatchinfoSelectDoctotal(pTab, &pSelect, &nDoc, 0, 0);
          pInfo->aMatchinfo[0] = (u32)nDoc;
        }
        break;

      case FTS3_MATCHINFO_AVGLENGTH:
        if( bGlobal ){
          sqlite3_int64 nDoc;     /* Number of rows in table */
          const char *a;          /* Aggregate column length array */
          const char *pEnd;       /* First byte past end of length array */

          rc = fts3MatchinfoSelectDoctotal(pTab, &pSelect, &nDoc, &a, &pEnd);
          if( rc==SQLITE_OK ){
            int iCol;
            for(iCol=0; iCol<pInfo->nCol; iCol++){
              u32 iVal;
              sqlite3_int64 nToken;
              a += sqlite3Fts3GetVarint(a, &nToken);
              if( a>pEnd ){
                rc = SQLITE_CORRUPT_VTAB;
                break;
              }
              iVal = (u32)(((u32)(nToken&0xffffffff)+nDoc/2)/nDoc);
              pInfo->aMatchinfo[iCol] = iVal;
            }
          }
        }
        break;

      case FTS3_MATCHINFO_LENGTH: {
        sqlite3_stmt *pSelectDocsize = 0;
        rc = sqlite3Fts3SelectDocsize(pTab, pCsr->iPrevId, &pSelectDocsize);
        if( rc==SQLITE_OK ){
          int iCol;
          const char *a = sqlite3_column_blob(pSelectDocsize, 0);
          const char *pEnd = a + sqlite3_column_bytes(pSelectDocsize, 0);
          for(iCol=0; iCol<pInfo->nCol; iCol++){
            sqlite3_int64 nToken;
            a += sqlite3Fts3GetVarintBounded(a, pEnd, &nToken);
            if( a>pEnd ){
              rc = SQLITE_CORRUPT_VTAB;
              break;
            }
            pInfo->aMatchinfo[iCol] = (u32)nToken;
          }
        }
        sqlite3_reset(pSelectDocsize);
        break;
      }

      case FTS3_MATCHINFO_LCS:
        rc = fts3ExprLoadDoclists(pCsr, 0, 0);
        if( rc==SQLITE_OK ){
          rc = fts3MatchinfoLcs(pCsr, pInfo);
        }
        break;

      case FTS3_MATCHINFO_LHITS_BM:
      case FTS3_MATCHINFO_LHITS: {
        size_t nZero = fts3MatchinfoSize(pInfo, zArg[i]) * sizeof(u32);
        memset(pInfo->aMatchinfo, 0, nZero);
        rc = fts3ExprLHitGather(pCsr->pExpr, pInfo);
        break;
      }

      default: {
        Fts3Expr *pExpr;
        assert( zArg[i]==FTS3_MATCHINFO_HITS );
        pExpr = pCsr->pExpr;
        rc = fts3ExprLoadDoclists(pCsr, 0, 0);
        if( rc!=SQLITE_OK ) break;
        if( bGlobal ){
          if( pCsr->pDeferred ){
            rc = fts3MatchinfoSelectDoctotal(pTab, &pSelect, &pInfo->nDoc,0,0);
            if( rc!=SQLITE_OK ) break;
          }
          rc = sqlite3Fts3ExprIterate(pExpr, fts3ExprGlobalHitsCb,(void*)pInfo);
          sqlite3Fts3EvalTestDeferred(pCsr, &rc);
          if( rc!=SQLITE_OK ) break;
        }
        (void)sqlite3Fts3ExprIterate(pExpr, fts3ExprLocalHitsCb,(void*)pInfo);
        break;
      }
    }

    pInfo->aMatchinfo += fts3MatchinfoSize(pInfo, zArg[i]);
  }

  sqlite3_reset(pSelect);
  return rc;
}


/*
** Populate pCsr->aMatchinfo[] with data for the current row. The
** 'matchinfo' data is an array of 32-bit unsigned integers (C type u32).
*/
static void fts3GetMatchinfo(
  sqlite3_context *pCtx,        /* Return results here */
  Fts3Cursor *pCsr,               /* FTS3 Cursor object */
  const char *zArg                /* Second argument to matchinfo() function */
){
  MatchInfo sInfo;
  Fts3Table *pTab = (Fts3Table *)pCsr->base.pVtab;
  int rc = SQLITE_OK;
  int bGlobal = 0;                /* Collect 'global' stats as well as local */

  u32 *aOut = 0;
  void (*xDestroyOut)(void*) = 0;

  memset(&sInfo, 0, sizeof(MatchInfo));
  sInfo.pCursor = pCsr;
  sInfo.nCol = pTab->nColumn;

  /* If there is cached matchinfo() data, but the format string for the
  ** cache does not match the format string for this request, discard
  ** the cached data. */
  if( pCsr->pMIBuffer && strcmp(pCsr->pMIBuffer->zMatchinfo, zArg) ){
    sqlite3Fts3MIBufferFree(pCsr->pMIBuffer);
    pCsr->pMIBuffer = 0;
  }

  /* If Fts3Cursor.pMIBuffer is NULL, then this is the first time the
  ** matchinfo function has been called for this query. In this case
  ** allocate the array used to accumulate the matchinfo data and
  ** initialize those elements that are constant for every row.
  */
  if( pCsr->pMIBuffer==0 ){
    size_t nMatchinfo = 0;        /* Number of u32 elements in match-info */
    int i;                        /* Used to iterate through zArg */

    /* Determine the number of phrases in the query */
    pCsr->nPhrase = fts3ExprPhraseCount(pCsr->pExpr);
    sInfo.nPhrase = pCsr->nPhrase;

    /* Determine the number of integers in the buffer returned by this call. */
    for(i=0; zArg[i]; i++){
      char *zErr = 0;
      if( fts3MatchinfoCheck(pTab, zArg[i], &zErr) ){
        sqlite3_result_error(pCtx, zErr, -1);
        sqlite3_free(zErr);
        return;
      }
      nMatchinfo += fts3MatchinfoSize(&sInfo, zArg[i]);
    }

    /* Allocate space for Fts3Cursor.aMatchinfo[] and Fts3Cursor.zMatchinfo. */
    pCsr->pMIBuffer = fts3MIBufferNew(nMatchinfo, zArg);
    if( !pCsr->pMIBuffer ) rc = SQLITE_NOMEM;

    pCsr->isMatchinfoNeeded = 1;
    bGlobal = 1;
  }

  if( rc==SQLITE_OK ){
    xDestroyOut = fts3MIBufferAlloc(pCsr->pMIBuffer, &aOut);
    if( xDestroyOut==0 ){
      rc = SQLITE_NOMEM;
    }
  }

  if( rc==SQLITE_OK ){
    sInfo.aMatchinfo = aOut;
    sInfo.nPhrase = pCsr->nPhrase;
    rc = fts3MatchinfoValues(pCsr, bGlobal, &sInfo, zArg);
    if( bGlobal ){
      fts3MIBufferSetGlobal(pCsr->pMIBuffer);
    }
  }

  if( rc!=SQLITE_OK ){
    sqlite3_result_error_code(pCtx, rc);
    if( xDestroyOut ) xDestroyOut(aOut);
  }else{
    int n = pCsr->pMIBuffer->nElem * sizeof(u32);
    sqlite3_result_blob(pCtx, aOut, n, xDestroyOut);
  }
}

/*
** Implementation of snippet() function.
*/
SQLITE_PRIVATE void sqlite3Fts3Snippet(
  sqlite3_context *pCtx,          /* SQLite function call context */
  Fts3Cursor *pCsr,               /* Cursor object */
  const char *zStart,             /* Snippet start text - "<b>" */
  const char *zEnd,               /* Snippet end text - "</b>" */
  const char *zEllipsis,          /* Snippet ellipsis text - "<b>...</b>" */
  int iCol,                       /* Extract snippet from this column */
  int nToken                      /* Approximate number of tokens in snippet */
){
  Fts3Table *pTab = (Fts3Table *)pCsr->base.pVtab;
  int rc = SQLITE_OK;
  int i;
  StrBuffer res = {0, 0, 0};

  /* The returned text includes up to four fragments of text extracted from
  ** the data in the current row. The first iteration of the for(...) loop
  ** below attempts to locate a single fragment of text nToken tokens in
  ** size that contains at least one instance of all phrases in the query
  ** expression that appear in the current row. If such a fragment of text
  ** cannot be found, the second iteration of the loop attempts to locate
  ** a pair of fragments, and so on.
  */
  int nSnippet = 0;               /* Number of fragments in this snippet */
  SnippetFragment aSnippet[4];    /* Maximum of 4 fragments per snippet */
  int nFToken = -1;               /* Number of tokens in each fragment */

  if( !pCsr->pExpr ){
    sqlite3_result_text(pCtx, "", 0, SQLITE_STATIC);
    return;
  }

  /* Limit the snippet length to 64 tokens. */
  if( nToken<-64 ) nToken = -64;
  if( nToken>+64 ) nToken = +64;

  for(nSnippet=1; 1; nSnippet++){

    int iSnip;                    /* Loop counter 0..nSnippet-1 */
    u64 mCovered = 0;             /* Bitmask of phrases covered by snippet */
    u64 mSeen = 0;                /* Bitmask of phrases seen by BestSnippet() */

    if( nToken>=0 ){
      nFToken = (nToken+nSnippet-1) / nSnippet;
    }else{
      nFToken = -1 * nToken;
    }

    for(iSnip=0; iSnip<nSnippet; iSnip++){
      int iBestScore = -1;        /* Best score of columns checked so far */
      int iRead;                  /* Used to iterate through columns */
      SnippetFragment *pFragment = &aSnippet[iSnip];

      memset(pFragment, 0, sizeof(*pFragment));

      /* Loop through all columns of the table being considered for snippets.
      ** If the iCol argument to this function was negative, this means all
      ** columns of the FTS3 table. Otherwise, only column iCol is considered.
      */
      for(iRead=0; iRead<pTab->nColumn; iRead++){
        SnippetFragment sF = {0, 0, 0, 0};
        int iS = 0;
        if( iCol>=0 && iRead!=iCol ) continue;

        /* Find the best snippet of nFToken tokens in column iRead. */
        rc = fts3BestSnippet(nFToken, pCsr, iRead, mCovered, &mSeen, &sF, &iS);
        if( rc!=SQLITE_OK ){
          goto snippet_out;
        }
        if( iS>iBestScore ){
          *pFragment = sF;
          iBestScore = iS;
        }
      }

      mCovered |= pFragment->covered;
    }

    /* If all query phrases seen by fts3BestSnippet() are present in at least
    ** one of the nSnippet snippet fragments, break out of the loop.
    */
    assert( (mCovered&mSeen)==mCovered );
    if( mSeen==mCovered || nSnippet==SizeofArray(aSnippet) ) break;
  }

  assert( nFToken>0 );

  for(i=0; i<nSnippet && rc==SQLITE_OK; i++){
    rc = fts3SnippetText(pCsr, &aSnippet[i],
        i, (i==nSnippet-1), nFToken, zStart, zEnd, zEllipsis, &res
    );
  }

 snippet_out:
  sqlite3Fts3SegmentsClose(pTab);
  if( rc!=SQLITE_OK ){
    sqlite3_result_error_code(pCtx, rc);
    sqlite3_free(res.z);
  }else{
    sqlite3_result_text(pCtx, res.z, -1, sqlite3_free);
  }
}


typedef struct TermOffset TermOffset;
typedef struct TermOffsetCtx TermOffsetCtx;

struct TermOffset {
  char *pList;                    /* Position-list */
  i64 iPos;                       /* Position just read from pList */
  i64 iOff;                       /* Offset of this term from read positions */
};

struct TermOffsetCtx {
  Fts3Cursor *pCsr;
  int iCol;                       /* Column of table to populate aTerm for */
  int iTerm;
  sqlite3_int64 iDocid;
  TermOffset *aTerm;
};

/*
** This function is an sqlite3Fts3ExprIterate() callback used by sqlite3Fts3Offsets().
*/
static int fts3ExprTermOffsetInit(Fts3Expr *pExpr, int iPhrase, void *ctx){
  TermOffsetCtx *p = (TermOffsetCtx *)ctx;
  int nTerm;                      /* Number of tokens in phrase */
  int iTerm;                      /* For looping through nTerm phrase terms */
  char *pList;                    /* Pointer to position list for phrase */
  i64 iPos = 0;                   /* First position in position-list */
  int rc;

  UNUSED_PARAMETER(iPhrase);
  rc = sqlite3Fts3EvalPhrasePoslist(p->pCsr, pExpr, p->iCol, &pList);
  nTerm = pExpr->pPhrase->nToken;
  if( pList ){
    fts3GetDeltaPosition(&pList, &iPos);
    assert_fts3_nc( iPos>=0 );
  }

  for(iTerm=0; iTerm<nTerm; iTerm++){
    TermOffset *pT = &p->aTerm[p->iTerm++];
    pT->iOff = nTerm-iTerm-1;
    pT->pList = pList;
    pT->iPos = iPos;
  }

  return rc;
}
