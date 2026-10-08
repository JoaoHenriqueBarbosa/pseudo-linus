
/*
** Implementation of offsets() function.
*/
SQLITE_PRIVATE void sqlite3Fts3Offsets(
  sqlite3_context *pCtx,          /* SQLite function call context */
  Fts3Cursor *pCsr                /* Cursor object */
){
  Fts3Table *pTab = (Fts3Table *)pCsr->base.pVtab;
  sqlite3_tokenizer_module const *pMod = pTab->pTokenizer->pModule;
  int rc;                         /* Return Code */
  int nToken;                     /* Number of tokens in query */
  int iCol;                       /* Column currently being processed */
  StrBuffer res = {0, 0, 0};      /* Result string */
  TermOffsetCtx sCtx;             /* Context for fts3ExprTermOffsetInit() */

  if( !pCsr->pExpr ){
    sqlite3_result_text(pCtx, "", 0, SQLITE_STATIC);
    return;
  }

  memset(&sCtx, 0, sizeof(sCtx));
  assert( pCsr->isRequireSeek==0 );

  /* Count the number of terms in the query */
  rc = fts3ExprLoadDoclists(pCsr, 0, &nToken);
  if( rc!=SQLITE_OK ) goto offsets_out;

  /* Allocate the array of TermOffset iterators. */
  sCtx.aTerm = (TermOffset *)sqlite3Fts3MallocZero(sizeof(TermOffset)*nToken);
  if( 0==sCtx.aTerm ){
    rc = SQLITE_NOMEM;
    goto offsets_out;
  }
  sCtx.iDocid = pCsr->iPrevId;
  sCtx.pCsr = pCsr;

  /* Loop through the table columns, appending offset information to
  ** string-buffer res for each column.
  */
  for(iCol=0; iCol<pTab->nColumn; iCol++){
    sqlite3_tokenizer_cursor *pC; /* Tokenizer cursor */
    const char *ZDUMMY;           /* Dummy argument used with xNext() */
    int NDUMMY = 0;               /* Dummy argument used with xNext() */
    int iStart = 0;
    int iEnd = 0;
    int iCurrent = 0;
    const char *zDoc;
    int nDoc;

    /* Initialize the contents of sCtx.aTerm[] for column iCol. This
    ** operation may fail if the database contains corrupt records.
    */
    sCtx.iCol = iCol;
    sCtx.iTerm = 0;
    rc = sqlite3Fts3ExprIterate(
        pCsr->pExpr, fts3ExprTermOffsetInit, (void*)&sCtx
    );
    if( rc!=SQLITE_OK ) goto offsets_out;

    /* Retreive the text stored in column iCol. If an SQL NULL is stored
    ** in column iCol, jump immediately to the next iteration of the loop.
    ** If an OOM occurs while retrieving the data (this can happen if SQLite
    ** needs to transform the data from utf-16 to utf-8), return SQLITE_NOMEM
    ** to the caller.
    */
    zDoc = (const char *)sqlite3_column_text(pCsr->pStmt, iCol+1);
    nDoc = sqlite3_column_bytes(pCsr->pStmt, iCol+1);
    if( zDoc==0 ){
      if( sqlite3_column_type(pCsr->pStmt, iCol+1)==SQLITE_NULL ){
        continue;
      }
      rc = SQLITE_NOMEM;
      goto offsets_out;
    }

    /* Initialize a tokenizer iterator to iterate through column iCol. */
    rc = sqlite3Fts3OpenTokenizer(pTab->pTokenizer, pCsr->iLangid,
        zDoc, nDoc, &pC
    );
    if( rc!=SQLITE_OK ) goto offsets_out;

    rc = pMod->xNext(pC, &ZDUMMY, &NDUMMY, &iStart, &iEnd, &iCurrent);
    while( rc==SQLITE_OK ){
      int i;                      /* Used to loop through terms */
      int iMinPos = 0x7FFFFFFF;   /* Position of next token */
      TermOffset *pTerm = 0;      /* TermOffset associated with next token */

      for(i=0; i<nToken; i++){
        TermOffset *pT = &sCtx.aTerm[i];
        if( pT->pList && (pT->iPos-pT->iOff)<iMinPos ){
          iMinPos = pT->iPos-pT->iOff;
          pTerm = pT;
        }
      }

      if( !pTerm ){
        /* All offsets for this column have been gathered. */
        rc = SQLITE_DONE;
      }else{
        assert_fts3_nc( iCurrent<=iMinPos );
        if( 0==(0xFE&*pTerm->pList) ){
          pTerm->pList = 0;
        }else{
          fts3GetDeltaPosition(&pTerm->pList, &pTerm->iPos);
        }
        while( rc==SQLITE_OK && iCurrent<iMinPos ){
          rc = pMod->xNext(pC, &ZDUMMY, &NDUMMY, &iStart, &iEnd, &iCurrent);
        }
        if( rc==SQLITE_OK ){
          char aBuffer[64];
          sqlite3_snprintf(sizeof(aBuffer), aBuffer,
              "%d %d %d %d ", iCol, pTerm-sCtx.aTerm, iStart, iEnd-iStart
          );
          rc = fts3StringAppend(&res, aBuffer, -1);
        }else if( rc==SQLITE_DONE && pTab->zContentTbl==0 ){
          rc = FTS_CORRUPT_VTAB;
        }
      }
    }
    if( rc==SQLITE_DONE ){
      rc = SQLITE_OK;
    }

    pMod->xClose(pC);
    if( rc!=SQLITE_OK ) goto offsets_out;
  }

 offsets_out:
  sqlite3_free(sCtx.aTerm);
  assert( rc!=SQLITE_DONE );
  sqlite3Fts3SegmentsClose(pTab);
  if( rc!=SQLITE_OK ){
    sqlite3_result_error_code(pCtx,  rc);
    sqlite3_free(res.z);
  }else{
    sqlite3_result_text(pCtx, res.z, res.n-1, sqlite3_free);
  }
  return;
}

/*
** Implementation of matchinfo() function.
*/
SQLITE_PRIVATE void sqlite3Fts3Matchinfo(
  sqlite3_context *pContext,      /* Function call context */
  Fts3Cursor *pCsr,               /* FTS3 table cursor */
  const char *zArg                /* Second arg to matchinfo() function */
){
  Fts3Table *pTab = (Fts3Table *)pCsr->base.pVtab;
  const char *zFormat;

  if( zArg ){
    zFormat = zArg;
  }else{
    zFormat = FTS3_MATCHINFO_DEFAULT;
  }

  if( !pCsr->pExpr ){
    sqlite3_result_blob(pContext, "", 0, SQLITE_STATIC);
    return;
  }else{
    /* Retrieve matchinfo() data. */
    fts3GetMatchinfo(pContext, pCsr, zFormat);
    sqlite3Fts3SegmentsClose(pTab);
  }
}

#endif

