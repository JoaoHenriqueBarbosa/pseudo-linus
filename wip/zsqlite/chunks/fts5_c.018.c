
typedef struct TokenCtx TokenCtx;
struct TokenCtx {
  Fts5ExprPhrase *pPhrase;
  Fts5Config *pConfig;
  int rc;
};

/*
** Callback for tokenizing terms used by ParseTerm().
*/
static int fts5ParseTokenize(
  void *pContext,                 /* Pointer to Fts5InsertCtx object */
  int tflags,                     /* Mask of FTS5_TOKEN_* flags */
  const char *pToken,             /* Buffer containing token */
  int nToken,                     /* Size of token in bytes */
  int iUnused1,                   /* Start offset of token */
  int iUnused2                    /* End offset of token */
){
  int rc = SQLITE_OK;
  const int SZALLOC = 8;
  TokenCtx *pCtx = (TokenCtx*)pContext;
  Fts5ExprPhrase *pPhrase = pCtx->pPhrase;

  UNUSED_PARAM2(iUnused1, iUnused2);

  /* If an error has already occurred, this is a no-op */
  if( pCtx->rc!=SQLITE_OK ) return pCtx->rc;
  if( nToken>FTS5_MAX_TOKEN_SIZE ) nToken = FTS5_MAX_TOKEN_SIZE;

  if( pPhrase && pPhrase->nTerm>0 && (tflags & FTS5_TOKEN_COLOCATED) ){
    Fts5ExprTerm *pSyn;
    sqlite3_int64 nByte = sizeof(Fts5ExprTerm) + sizeof(Fts5Buffer) + nToken+1;
    pSyn = (Fts5ExprTerm*)sqlite3_malloc64(nByte);
    if( pSyn==0 ){
      rc = SQLITE_NOMEM;
    }else{
      memset(pSyn, 0, (size_t)nByte);
      pSyn->pTerm = ((char*)pSyn) + sizeof(Fts5ExprTerm) + sizeof(Fts5Buffer);
      pSyn->nFullTerm = pSyn->nQueryTerm = nToken;
      if( pCtx->pConfig->bTokendata ){
        pSyn->nQueryTerm = (int)strlen(pSyn->pTerm);
      }
      memcpy(pSyn->pTerm, pToken, nToken);
      pSyn->pSynonym = pPhrase->aTerm[pPhrase->nTerm-1].pSynonym;
      pPhrase->aTerm[pPhrase->nTerm-1].pSynonym = pSyn;
    }
  }else{
    Fts5ExprTerm *pTerm;
    if( pPhrase==0 || (pPhrase->nTerm % SZALLOC)==0 ){
      Fts5ExprPhrase *pNew;
      int nNew = SZALLOC + (pPhrase ? pPhrase->nTerm : 0);

      pNew = (Fts5ExprPhrase*)sqlite3_realloc64(pPhrase,
          sizeof(Fts5ExprPhrase) + sizeof(Fts5ExprTerm) * nNew
      );
      if( pNew==0 ){
        rc = SQLITE_NOMEM;
      }else{
        if( pPhrase==0 ) memset(pNew, 0, sizeof(Fts5ExprPhrase));
        pCtx->pPhrase = pPhrase = pNew;
        pNew->nTerm = nNew - SZALLOC;
      }
    }

    if( rc==SQLITE_OK ){
      pTerm = &pPhrase->aTerm[pPhrase->nTerm++];
      memset(pTerm, 0, sizeof(Fts5ExprTerm));
      pTerm->pTerm = sqlite3Fts5Strndup(&rc, pToken, nToken);
      pTerm->nFullTerm = pTerm->nQueryTerm = nToken;
      if( pCtx->pConfig->bTokendata && rc==SQLITE_OK ){
        pTerm->nQueryTerm = (int)strlen(pTerm->pTerm);
      }
    }
  }

  pCtx->rc = rc;
  return rc;
}


/*
** Free the phrase object passed as the only argument.
*/
static void sqlite3Fts5ParsePhraseFree(Fts5ExprPhrase *pPhrase){
  fts5ExprPhraseFree(pPhrase);
}

/*
** Free the phrase object passed as the second argument.
*/
static void sqlite3Fts5ParseNearsetFree(Fts5ExprNearset *pNear){
  if( pNear ){
    int i;
    for(i=0; i<pNear->nPhrase; i++){
      fts5ExprPhraseFree(pNear->apPhrase[i]);
    }
    sqlite3_free(pNear->pColset);
    sqlite3_free(pNear);
  }
}

static void sqlite3Fts5ParseFinished(Fts5Parse *pParse, Fts5ExprNode *p){
  assert( pParse->pExpr==0 );
  pParse->pExpr = p;
}

static int parseGrowPhraseArray(Fts5Parse *pParse){
  if( (pParse->nPhrase % 8)==0 ){
    sqlite3_int64 nByte = sizeof(Fts5ExprPhrase*) * (pParse->nPhrase + 8);
    Fts5ExprPhrase **apNew;
    apNew = (Fts5ExprPhrase**)sqlite3_realloc64(pParse->apPhrase, nByte);
    if( apNew==0 ){
      pParse->rc = SQLITE_NOMEM;
      return SQLITE_NOMEM;
    }
    pParse->apPhrase = apNew;
  }
  return SQLITE_OK;
}

/*
** This function is called by the parser to process a string token. The
** string may or may not be quoted. In any case it is tokenized and a
** phrase object consisting of all tokens returned.
*/
static Fts5ExprPhrase *sqlite3Fts5ParseTerm(
  Fts5Parse *pParse,              /* Parse context */
  Fts5ExprPhrase *pAppend,        /* Phrase to append to */
  Fts5Token *pToken,              /* String to tokenize */
  int bPrefix                     /* True if there is a trailing "*" */
){
  Fts5Config *pConfig = pParse->pConfig;
  TokenCtx sCtx;                  /* Context object passed to callback */
  int rc;                         /* Tokenize return code */
  char *z = 0;

  memset(&sCtx, 0, sizeof(TokenCtx));
  sCtx.pPhrase = pAppend;
  sCtx.pConfig = pConfig;

  rc = fts5ParseStringFromToken(pToken, &z);
  if( rc==SQLITE_OK ){
    int flags = FTS5_TOKENIZE_QUERY | (bPrefix ? FTS5_TOKENIZE_PREFIX : 0);
    int n;
    sqlite3Fts5Dequote(z);
    n = (int)strlen(z);
    rc = sqlite3Fts5Tokenize(pConfig, flags, z, n, &sCtx, fts5ParseTokenize);
  }
  sqlite3_free(z);
  if( rc || (rc = sCtx.rc) ){
    pParse->rc = rc;
    fts5ExprPhraseFree(sCtx.pPhrase);
    sCtx.pPhrase = 0;
  }else{

    if( pAppend==0 ){
      if( parseGrowPhraseArray(pParse) ){
        fts5ExprPhraseFree(sCtx.pPhrase);
        return 0;
      }
      pParse->nPhrase++;
    }

    if( sCtx.pPhrase==0 ){
      /* This happens when parsing a token or quoted phrase that contains
      ** no token characters at all. (e.g ... MATCH '""'). */
      sCtx.pPhrase = sqlite3Fts5MallocZero(&pParse->rc, sizeof(Fts5ExprPhrase));
    }else if( sCtx.pPhrase->nTerm ){
      sCtx.pPhrase->aTerm[sCtx.pPhrase->nTerm-1].bPrefix = (u8)bPrefix;
    }
    pParse->apPhrase[pParse->nPhrase-1] = sCtx.pPhrase;
  }

  return sCtx.pPhrase;
}

/*
** Create a new FTS5 expression by cloning phrase iPhrase of the
** expression passed as the second argument.
*/
static int sqlite3Fts5ExprClonePhrase(
  Fts5Expr *pExpr,
  int iPhrase,
  Fts5Expr **ppNew
){
  int rc = SQLITE_OK;             /* Return code */
  Fts5ExprPhrase *pOrig = 0;      /* The phrase extracted from pExpr */
  Fts5Expr *pNew = 0;             /* Expression to return via *ppNew */
  TokenCtx sCtx = {0,0,0};        /* Context object for fts5ParseTokenize */
  if( iPhrase<0 || iPhrase>=pExpr->nPhrase ){
    rc = SQLITE_RANGE;
  }else{
    pOrig = pExpr->apExprPhrase[iPhrase];
    pNew = (Fts5Expr*)sqlite3Fts5MallocZero(&rc, sizeof(Fts5Expr));
  }
  if( rc==SQLITE_OK ){
    pNew->apExprPhrase = (Fts5ExprPhrase**)sqlite3Fts5MallocZero(&rc,
        sizeof(Fts5ExprPhrase*));
  }
  if( rc==SQLITE_OK ){
    pNew->pRoot = (Fts5ExprNode*)sqlite3Fts5MallocZero(&rc,
        sizeof(Fts5ExprNode));
  }
  if( rc==SQLITE_OK ){
    pNew->pRoot->pNear = (Fts5ExprNearset*)sqlite3Fts5MallocZero(&rc,
        sizeof(Fts5ExprNearset) + sizeof(Fts5ExprPhrase*));
  }
  if( rc==SQLITE_OK && ALWAYS(pOrig!=0) ){
    Fts5Colset *pColsetOrig = pOrig->pNode->pNear->pColset;
    if( pColsetOrig ){
      sqlite3_int64 nByte;
      Fts5Colset *pColset;
      nByte = sizeof(Fts5Colset) + (pColsetOrig->nCol-1) * sizeof(int);
      pColset = (Fts5Colset*)sqlite3Fts5MallocZero(&rc, nByte);
      if( pColset ){
        memcpy(pColset, pColsetOrig, (size_t)nByte);
      }
      pNew->pRoot->pNear->pColset = pColset;
    }
  }

  if( rc==SQLITE_OK ){
    if( pOrig->nTerm ){
      int i;                          /* Used to iterate through phrase terms */
      sCtx.pConfig = pExpr->pConfig;
      for(i=0; rc==SQLITE_OK && i<pOrig->nTerm; i++){
        int tflags = 0;
        Fts5ExprTerm *p;
        for(p=&pOrig->aTerm[i]; p && rc==SQLITE_OK; p=p->pSynonym){
          rc = fts5ParseTokenize((void*)&sCtx,tflags,p->pTerm,p->nFullTerm,0,0);
          tflags = FTS5_TOKEN_COLOCATED;
        }
        if( rc==SQLITE_OK ){
          sCtx.pPhrase->aTerm[i].bPrefix = pOrig->aTerm[i].bPrefix;
          sCtx.pPhrase->aTerm[i].bFirst = pOrig->aTerm[i].bFirst;
        }
      }
    }else{
      /* This happens when parsing a token or quoted phrase that contains
      ** no token characters at all. (e.g ... MATCH '""'). */
      sCtx.pPhrase = sqlite3Fts5MallocZero(&rc, sizeof(Fts5ExprPhrase));
    }
  }

  if( rc==SQLITE_OK && ALWAYS(sCtx.pPhrase) ){
    /* All the allocations succeeded. Put the expression object together. */
    pNew->pIndex = pExpr->pIndex;
    pNew->pConfig = pExpr->pConfig;
    pNew->nPhrase = 1;
    pNew->apExprPhrase[0] = sCtx.pPhrase;
    pNew->pRoot->pNear->apPhrase[0] = sCtx.pPhrase;
    pNew->pRoot->pNear->nPhrase = 1;
    sCtx.pPhrase->pNode = pNew->pRoot;

    if( pOrig->nTerm==1
     && pOrig->aTerm[0].pSynonym==0
     && pOrig->aTerm[0].bFirst==0
    ){
      pNew->pRoot->eType = FTS5_TERM;
      pNew->pRoot->xNext = fts5ExprNodeNext_TERM;
    }else{
      pNew->pRoot->eType = FTS5_STRING;
      pNew->pRoot->xNext = fts5ExprNodeNext_STRING;
    }
  }else{
    sqlite3Fts5ExprFree(pNew);
    fts5ExprPhraseFree(sCtx.pPhrase);
    pNew = 0;
  }

  *ppNew = pNew;
  return rc;
}


/*
** Token pTok has appeared in a MATCH expression where the NEAR operator
** is expected. If token pTok does not contain "NEAR", store an error
** in the pParse object.
*/
static void sqlite3Fts5ParseNear(Fts5Parse *pParse, Fts5Token *pTok){
  if( pTok->n!=4 || memcmp("NEAR", pTok->p, 4) ){
    sqlite3Fts5ParseError(
        pParse, "fts5: syntax error near \"%.*s\"", pTok->n, pTok->p
    );
  }
}

static void sqlite3Fts5ParseSetDistance(
  Fts5Parse *pParse,
  Fts5ExprNearset *pNear,
  Fts5Token *p
){
  if( pNear ){
    int nNear = 0;
    int i;
    if( p->n ){
      for(i=0; i<p->n; i++){
        char c = (char)p->p[i];
        if( c<'0' || c>'9' ){
          sqlite3Fts5ParseError(
              pParse, "expected integer, got \"%.*s\"", p->n, p->p
              );
          return;
        }
        nNear = nNear * 10 + (p->p[i] - '0');
      }
    }else{
      nNear = FTS5_DEFAULT_NEARDIST;
    }
    pNear->nNear = nNear;
  }
}

/*
** The second argument passed to this function may be NULL, or it may be
** an existing Fts5Colset object. This function returns a pointer to
** a new colset object containing the contents of (p) with new value column
** number iCol appended.
**
** If an OOM error occurs, store an error code in pParse and return NULL.
** The old colset object (if any) is not freed in this case.
*/
static Fts5Colset *fts5ParseColset(
  Fts5Parse *pParse,              /* Store SQLITE_NOMEM here if required */
  Fts5Colset *p,                  /* Existing colset object */
  int iCol                        /* New column to add to colset object */
){
  int nCol = p ? p->nCol : 0;     /* Num. columns already in colset object */
  Fts5Colset *pNew;               /* New colset object to return */

  assert( pParse->rc==SQLITE_OK );
  assert( iCol>=0 && iCol<pParse->pConfig->nCol );

  pNew = sqlite3_realloc64(p, sizeof(Fts5Colset) + sizeof(int)*nCol);
  if( pNew==0 ){
    pParse->rc = SQLITE_NOMEM;
  }else{
    int *aiCol = pNew->aiCol;
    int i, j;
    for(i=0; i<nCol; i++){
      if( aiCol[i]==iCol ) return pNew;
      if( aiCol[i]>iCol ) break;
    }
    for(j=nCol; j>i; j--){
      aiCol[j] = aiCol[j-1];
    }
    aiCol[i] = iCol;
    pNew->nCol = nCol+1;

#ifndef NDEBUG
    /* Check that the array is in order and contains no duplicate entries. */
    for(i=1; i<pNew->nCol; i++) assert( pNew->aiCol[i]>pNew->aiCol[i-1] );
#endif
  }

  return pNew;
}
