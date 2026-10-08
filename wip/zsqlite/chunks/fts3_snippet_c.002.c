

/*
** This function is used to count the entries in a column-list (a
** delta-encoded list of term offsets within a single column of a single
** row). When this function is called, *ppCollist should point to the
** beginning of the first varint in the column-list (the varint that
** contains the position of the first matching term in the column data).
** Before returning, *ppCollist is set to point to the first byte after
** the last varint in the column-list (either the 0x00 signifying the end
** of the position-list, or the 0x01 that precedes the column number of
** the next column in the position-list).
**
** The number of elements in the column-list is returned.
*/
static int fts3ColumnlistCount(char **ppCollist){
  char *pEnd = *ppCollist;
  char c = 0;
  int nEntry = 0;

  /* A column-list is terminated by either a 0x01 or 0x00. */
  while( 0xFE & (*pEnd | c) ){
    c = *pEnd++ & 0x80;
    if( !c ) nEntry++;
  }

  *ppCollist = pEnd;
  return nEntry;
}

/*
** This function gathers 'y' or 'b' data for a single phrase.
*/
static int fts3ExprLHits(
  Fts3Expr *pExpr,                /* Phrase expression node */
  MatchInfo *p                    /* Matchinfo context */
){
  Fts3Table *pTab = (Fts3Table *)p->pCursor->base.pVtab;
  int iStart;
  Fts3Phrase *pPhrase = pExpr->pPhrase;
  char *pIter = pPhrase->doclist.pList;
  int iCol = 0;

  assert( p->flag==FTS3_MATCHINFO_LHITS_BM || p->flag==FTS3_MATCHINFO_LHITS );
  if( p->flag==FTS3_MATCHINFO_LHITS ){
    iStart = pExpr->iPhrase * p->nCol;
  }else{
    iStart = pExpr->iPhrase * ((p->nCol + 31) / 32);
  }

  if( pIter ) while( 1 ){
    int nHit = fts3ColumnlistCount(&pIter);
    if( (pPhrase->iColumn>=pTab->nColumn || pPhrase->iColumn==iCol) ){
      if( p->flag==FTS3_MATCHINFO_LHITS ){
        p->aMatchinfo[iStart + iCol] = (u32)nHit;
      }else if( nHit ){
        p->aMatchinfo[iStart + (iCol+1)/32] |= (1 << (iCol&0x1F));
      }
    }
    assert( *pIter==0x00 || *pIter==0x01 );
    if( *pIter!=0x01 ) break;
    pIter++;
    pIter += fts3GetVarint32(pIter, &iCol);
    if( iCol>=p->nCol ) return FTS_CORRUPT_VTAB;
  }
  return SQLITE_OK;
}

/*
** Gather the results for matchinfo directives 'y' and 'b'.
*/
static int fts3ExprLHitGather(
  Fts3Expr *pExpr,
  MatchInfo *p
){
  int rc = SQLITE_OK;
  assert( (pExpr->pLeft==0)==(pExpr->pRight==0) );
  if( pExpr->bEof==0 && pExpr->iDocid==p->pCursor->iPrevId ){
    if( pExpr->pLeft ){
      rc = fts3ExprLHitGather(pExpr->pLeft, p);
      if( rc==SQLITE_OK ) rc = fts3ExprLHitGather(pExpr->pRight, p);
    }else{
      rc = fts3ExprLHits(pExpr, p);
    }
  }
  return rc;
}

/*
** sqlite3Fts3ExprIterate() callback used to collect the "global" matchinfo
** stats for a single query.
**
** sqlite3Fts3ExprIterate() callback to load the 'global' elements of a
** FTS3_MATCHINFO_HITS matchinfo array. The global stats are those elements
** of the matchinfo array that are constant for all rows returned by the
** current query.
**
** Argument pCtx is actually a pointer to a struct of type MatchInfo. This
** function populates Matchinfo.aMatchinfo[] as follows:
**
**   for(iCol=0; iCol<nCol; iCol++){
**     aMatchinfo[3*iPhrase*nCol + 3*iCol + 1] = X;
**     aMatchinfo[3*iPhrase*nCol + 3*iCol + 2] = Y;
**   }
**
** where X is the number of matches for phrase iPhrase is column iCol of all
** rows of the table. Y is the number of rows for which column iCol contains
** at least one instance of phrase iPhrase.
**
** If the phrase pExpr consists entirely of deferred tokens, then all X and
** Y values are set to nDoc, where nDoc is the number of documents in the
** file system. This is done because the full-text index doclist is required
** to calculate these values properly, and the full-text index doclist is
** not available for deferred tokens.
*/
static int fts3ExprGlobalHitsCb(
  Fts3Expr *pExpr,                /* Phrase expression node */
  int iPhrase,                    /* Phrase number (numbered from zero) */
  void *pCtx                      /* Pointer to MatchInfo structure */
){
  MatchInfo *p = (MatchInfo *)pCtx;
  return sqlite3Fts3EvalPhraseStats(
      p->pCursor, pExpr, &p->aMatchinfo[3*iPhrase*p->nCol]
  );
}

/*
** sqlite3Fts3ExprIterate() callback used to collect the "local" part of the
** FTS3_MATCHINFO_HITS array. The local stats are those elements of the
** array that are different for each row returned by the query.
*/
static int fts3ExprLocalHitsCb(
  Fts3Expr *pExpr,                /* Phrase expression node */
  int iPhrase,                    /* Phrase number */
  void *pCtx                      /* Pointer to MatchInfo structure */
){
  int rc = SQLITE_OK;
  MatchInfo *p = (MatchInfo *)pCtx;
  int iStart = iPhrase * p->nCol * 3;
  int i;

  for(i=0; i<p->nCol && rc==SQLITE_OK; i++){
    char *pCsr;
    rc = sqlite3Fts3EvalPhrasePoslist(p->pCursor, pExpr, i, &pCsr);
    if( pCsr ){
      p->aMatchinfo[iStart+i*3] = fts3ColumnlistCount(&pCsr);
    }else{
      p->aMatchinfo[iStart+i*3] = 0;
    }
  }

  return rc;
}

static int fts3MatchinfoCheck(
  Fts3Table *pTab,
  char cArg,
  char **pzErr
){
  if( (cArg==FTS3_MATCHINFO_NPHRASE)
   || (cArg==FTS3_MATCHINFO_NCOL)
   || (cArg==FTS3_MATCHINFO_NDOC && pTab->bFts4)
   || (cArg==FTS3_MATCHINFO_AVGLENGTH && pTab->bFts4)
   || (cArg==FTS3_MATCHINFO_LENGTH && pTab->bHasDocsize)
   || (cArg==FTS3_MATCHINFO_LCS)
   || (cArg==FTS3_MATCHINFO_HITS)
   || (cArg==FTS3_MATCHINFO_LHITS)
   || (cArg==FTS3_MATCHINFO_LHITS_BM)
  ){
    return SQLITE_OK;
  }
  sqlite3Fts3ErrMsg(pzErr, "unrecognized matchinfo request: %c", cArg);
  return SQLITE_ERROR;
}

static size_t fts3MatchinfoSize(MatchInfo *pInfo, char cArg){
  size_t nVal;                      /* Number of integers output by cArg */

  switch( cArg ){
    case FTS3_MATCHINFO_NDOC:
    case FTS3_MATCHINFO_NPHRASE:
    case FTS3_MATCHINFO_NCOL:
      nVal = 1;
      break;

    case FTS3_MATCHINFO_AVGLENGTH:
    case FTS3_MATCHINFO_LENGTH:
    case FTS3_MATCHINFO_LCS:
      nVal = pInfo->nCol;
      break;

    case FTS3_MATCHINFO_LHITS:
      nVal = pInfo->nCol * pInfo->nPhrase;
      break;

    case FTS3_MATCHINFO_LHITS_BM:
      nVal = pInfo->nPhrase * ((pInfo->nCol + 31) / 32);
      break;

    default:
      assert( cArg==FTS3_MATCHINFO_HITS );
      nVal = pInfo->nCol * pInfo->nPhrase * 3;
      break;
  }

  return nVal;
}

static int fts3MatchinfoSelectDoctotal(
  Fts3Table *pTab,
  sqlite3_stmt **ppStmt,
  sqlite3_int64 *pnDoc,
  const char **paLen,
  const char **ppEnd
){
  sqlite3_stmt *pStmt;
  const char *a;
  const char *pEnd;
  sqlite3_int64 nDoc;
  int n;


  if( !*ppStmt ){
    int rc = sqlite3Fts3SelectDoctotal(pTab, ppStmt);
    if( rc!=SQLITE_OK ) return rc;
  }
  pStmt = *ppStmt;
  assert( sqlite3_data_count(pStmt)==1 );

  n = sqlite3_column_bytes(pStmt, 0);
  a = sqlite3_column_blob(pStmt, 0);
  if( a==0 ){
    return FTS_CORRUPT_VTAB;
  }
  pEnd = a + n;
  a += sqlite3Fts3GetVarintBounded(a, pEnd, &nDoc);
  if( nDoc<=0 || a>pEnd ){
    return FTS_CORRUPT_VTAB;
  }
  *pnDoc = nDoc;

  if( paLen ) *paLen = a;
  if( ppEnd ) *ppEnd = pEnd;
  return SQLITE_OK;
}

/*
** An instance of the following structure is used to store state while
** iterating through a multi-column position-list corresponding to the
** hits for a single phrase on a single row in order to calculate the
** values for a matchinfo() FTS3_MATCHINFO_LCS request.
*/
typedef struct LcsIterator LcsIterator;
struct LcsIterator {
  Fts3Expr *pExpr;                /* Pointer to phrase expression */
  int iPosOffset;                 /* Tokens count up to end of this phrase */
  char *pRead;                    /* Cursor used to iterate through aDoclist */
  int iPos;                       /* Current position */
};

/*
** If LcsIterator.iCol is set to the following value, the iterator has
** finished iterating through all offsets for all columns.
*/
#define LCS_ITERATOR_FINISHED 0x7FFFFFFF;

static int fts3MatchinfoLcsCb(
  Fts3Expr *pExpr,                /* Phrase expression node */
  int iPhrase,                    /* Phrase number (numbered from zero) */
  void *pCtx                      /* Pointer to MatchInfo structure */
){
  LcsIterator *aIter = (LcsIterator *)pCtx;
  aIter[iPhrase].pExpr = pExpr;
  return SQLITE_OK;
}

/*
** Advance the iterator passed as an argument to the next position. Return
** 1 if the iterator is at EOF or if it now points to the start of the
** position list for the next column.
*/
static int fts3LcsIteratorAdvance(LcsIterator *pIter){
  char *pRead;
  sqlite3_int64 iRead;
  int rc = 0;

  if( NEVER(pIter==0) ) return 1;
  pRead = pIter->pRead;
  pRead += sqlite3Fts3GetVarint(pRead, &iRead);
  if( iRead==0 || iRead==1 ){
    pRead = 0;
    rc = 1;
  }else{
    pIter->iPos += (int)(iRead-2);
  }

  pIter->pRead = pRead;
  return rc;
}

/*
** This function implements the FTS3_MATCHINFO_LCS matchinfo() flag.
**
** If the call is successful, the longest-common-substring lengths for each
** column are written into the first nCol elements of the pInfo->aMatchinfo[]
** array before returning. SQLITE_OK is returned in this case.
**
** Otherwise, if an error occurs, an SQLite error code is returned and the
** data written to the first nCol elements of pInfo->aMatchinfo[] is
** undefined.
*/
static int fts3MatchinfoLcs(Fts3Cursor *pCsr, MatchInfo *pInfo){
  LcsIterator *aIter;
  int i;
  int iCol;
  int nToken = 0;
  int rc = SQLITE_OK;

  /* Allocate and populate the array of LcsIterator objects. The array
  ** contains one element for each matchable phrase in the query.
  **/
  aIter = sqlite3Fts3MallocZero(sizeof(LcsIterator) * pCsr->nPhrase);
  if( !aIter ) return SQLITE_NOMEM;
  (void)sqlite3Fts3ExprIterate(pCsr->pExpr, fts3MatchinfoLcsCb, (void*)aIter);

  for(i=0; i<pInfo->nPhrase; i++){
    LcsIterator *pIter = &aIter[i];
    nToken -= pIter->pExpr->pPhrase->nToken;
    pIter->iPosOffset = nToken;
  }

  for(iCol=0; iCol<pInfo->nCol; iCol++){
    int nLcs = 0;                 /* LCS value for this column */
    int nLive = 0;                /* Number of iterators in aIter not at EOF */

    for(i=0; i<pInfo->nPhrase; i++){
      LcsIterator *pIt = &aIter[i];
      rc = sqlite3Fts3EvalPhrasePoslist(pCsr, pIt->pExpr, iCol, &pIt->pRead);
      if( rc!=SQLITE_OK ) goto matchinfo_lcs_out;
      if( pIt->pRead ){
        pIt->iPos = pIt->iPosOffset;
        fts3LcsIteratorAdvance(pIt);
        if( pIt->pRead==0 ){
          rc = FTS_CORRUPT_VTAB;
          goto matchinfo_lcs_out;
        }
        nLive++;
      }
    }

    while( nLive>0 ){
      LcsIterator *pAdv = 0;      /* The iterator to advance by one position */
      int nThisLcs = 0;           /* LCS for the current iterator positions */

      for(i=0; i<pInfo->nPhrase; i++){
        LcsIterator *pIter = &aIter[i];
        if( pIter->pRead==0 ){
          /* This iterator is already at EOF for this column. */
          nThisLcs = 0;
        }else{
          if( pAdv==0 || pIter->iPos<pAdv->iPos ){
            pAdv = pIter;
          }
          if( nThisLcs==0 || pIter->iPos==pIter[-1].iPos ){
            nThisLcs++;
          }else{
            nThisLcs = 1;
          }
          if( nThisLcs>nLcs ) nLcs = nThisLcs;
        }
      }
      if( fts3LcsIteratorAdvance(pAdv) ) nLive--;
    }

    pInfo->aMatchinfo[iCol] = nLcs;
  }

 matchinfo_lcs_out:
  sqlite3_free(aIter);
  return rc;
}
