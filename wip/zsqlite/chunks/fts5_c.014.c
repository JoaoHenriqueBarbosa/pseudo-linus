
static int fts5ExprIsspace(char t){
  return t==' ' || t=='\t' || t=='\n' || t=='\r';
}

/*
** Read the first token from the nul-terminated string at *pz.
*/
static int fts5ExprGetToken(
  Fts5Parse *pParse,
  const char **pz,                /* IN/OUT: Pointer into buffer */
  Fts5Token *pToken
){
  const char *z = *pz;
  int tok;

  /* Skip past any whitespace */
  while( fts5ExprIsspace(*z) ) z++;

  pToken->p = z;
  pToken->n = 1;
  switch( *z ){
    case '(':  tok = FTS5_LP;    break;
    case ')':  tok = FTS5_RP;    break;
    case '{':  tok = FTS5_LCP;   break;
    case '}':  tok = FTS5_RCP;   break;
    case ':':  tok = FTS5_COLON; break;
    case ',':  tok = FTS5_COMMA; break;
    case '+':  tok = FTS5_PLUS;  break;
    case '*':  tok = FTS5_STAR;  break;
    case '-':  tok = FTS5_MINUS; break;
    case '^':  tok = FTS5_CARET; break;
    case '\0': tok = FTS5_EOF;   break;

    case '"': {
      const char *z2;
      tok = FTS5_STRING;

      for(z2=&z[1]; 1; z2++){
        if( z2[0]=='"' ){
          z2++;
          if( z2[0]!='"' ) break;
        }
        if( z2[0]=='\0' ){
          sqlite3Fts5ParseError(pParse, "unterminated string");
          return FTS5_EOF;
        }
      }
      pToken->n = (z2 - z);
      break;
    }

    default: {
      const char *z2;
      if( sqlite3Fts5IsBareword(z[0])==0 ){
        sqlite3Fts5ParseError(pParse, "fts5: syntax error near \"%.1s\"", z);
        return FTS5_EOF;
      }
      tok = FTS5_STRING;
      for(z2=&z[1]; sqlite3Fts5IsBareword(*z2); z2++);
      pToken->n = (z2 - z);
      if( pToken->n==2 && memcmp(pToken->p, "OR", 2)==0 )  tok = FTS5_OR;
      if( pToken->n==3 && memcmp(pToken->p, "NOT", 3)==0 ) tok = FTS5_NOT;
      if( pToken->n==3 && memcmp(pToken->p, "AND", 3)==0 ) tok = FTS5_AND;
      break;
    }
  }

  *pz = &pToken->p[pToken->n];
  return tok;
}

static void *fts5ParseAlloc(u64 t){ return sqlite3_malloc64((sqlite3_int64)t);}
static void fts5ParseFree(void *p){ sqlite3_free(p); }

static int sqlite3Fts5ExprNew(
  Fts5Config *pConfig,            /* FTS5 Configuration */
  int bPhraseToAnd,
  int iCol,
  const char *zExpr,              /* Expression text */
  Fts5Expr **ppNew,
  char **pzErr
){
  Fts5Parse sParse;
  Fts5Token token;
  const char *z = zExpr;
  int t;                          /* Next token type */
  void *pEngine;
  Fts5Expr *pNew;

  *ppNew = 0;
  *pzErr = 0;
  memset(&sParse, 0, sizeof(sParse));
  sParse.bPhraseToAnd = bPhraseToAnd;
  pEngine = sqlite3Fts5ParserAlloc(fts5ParseAlloc);
  if( pEngine==0 ){ return SQLITE_NOMEM; }
  sParse.pConfig = pConfig;

  do {
    t = fts5ExprGetToken(&sParse, &z, &token);
    sqlite3Fts5Parser(pEngine, t, token, &sParse);
  }while( sParse.rc==SQLITE_OK && t!=FTS5_EOF );
  sqlite3Fts5ParserFree(pEngine, fts5ParseFree);

  assert_expr_depth_ok(sParse.rc, sParse.pExpr);

  /* If the LHS of the MATCH expression was a user column, apply the
  ** implicit column-filter.  */
  if( iCol<pConfig->nCol && sParse.pExpr && sParse.rc==SQLITE_OK ){
    int n = sizeof(Fts5Colset);
    Fts5Colset *pColset = (Fts5Colset*)sqlite3Fts5MallocZero(&sParse.rc, n);
    if( pColset ){
      pColset->nCol = 1;
      pColset->aiCol[0] = iCol;
      sqlite3Fts5ParseSetColset(&sParse, sParse.pExpr, pColset);
    }
  }

  assert( sParse.rc!=SQLITE_OK || sParse.zErr==0 );
  if( sParse.rc==SQLITE_OK ){
    *ppNew = pNew = sqlite3_malloc(sizeof(Fts5Expr));
    if( pNew==0 ){
      sParse.rc = SQLITE_NOMEM;
      sqlite3Fts5ParseNodeFree(sParse.pExpr);
    }else{
      if( !sParse.pExpr ){
        const int nByte = sizeof(Fts5ExprNode);
        pNew->pRoot = (Fts5ExprNode*)sqlite3Fts5MallocZero(&sParse.rc, nByte);
        if( pNew->pRoot ){
          pNew->pRoot->bEof = 1;
        }
      }else{
        pNew->pRoot = sParse.pExpr;
      }
      pNew->pIndex = 0;
      pNew->pConfig = pConfig;
      pNew->apExprPhrase = sParse.apPhrase;
      pNew->nPhrase = sParse.nPhrase;
      pNew->bDesc = 0;
      sParse.apPhrase = 0;
    }
  }else{
    sqlite3Fts5ParseNodeFree(sParse.pExpr);
  }

  sqlite3_free(sParse.apPhrase);
  if( 0==*pzErr ){
    *pzErr = sParse.zErr;
  }else{
    sqlite3_free(sParse.zErr);
  }
  return sParse.rc;
}

/*
** Assuming that buffer z is at least nByte bytes in size and contains a
** valid utf-8 string, return the number of characters in the string.
*/
static int fts5ExprCountChar(const char *z, int nByte){
  int nRet = 0;
  int ii;
  for(ii=0; ii<nByte; ii++){
    if( (z[ii] & 0xC0)!=0x80 ) nRet++;
  }
  return nRet;
}

/*
** This function is only called when using the special 'trigram' tokenizer.
** Argument zText contains the text of a LIKE or GLOB pattern matched
** against column iCol. This function creates and compiles an FTS5 MATCH
** expression that will match a superset of the rows matched by the LIKE or
** GLOB. If successful, SQLITE_OK is returned. Otherwise, an SQLite error
** code.
*/
static int sqlite3Fts5ExprPattern(
  Fts5Config *pConfig, int bGlob, int iCol, const char *zText, Fts5Expr **pp
){
  i64 nText = strlen(zText);
  char *zExpr = (char*)sqlite3_malloc64(nText*4 + 1);
  int rc = SQLITE_OK;

  if( zExpr==0 ){
    rc = SQLITE_NOMEM;
  }else{
    char aSpec[3];
    int iOut = 0;
    int i = 0;
    int iFirst = 0;

    if( bGlob==0 ){
      aSpec[0] = '_';
      aSpec[1] = '%';
      aSpec[2] = 0;
    }else{
      aSpec[0] = '*';
      aSpec[1] = '?';
      aSpec[2] = '[';
    }

    while( i<=nText ){
      if( i==nText
       || zText[i]==aSpec[0] || zText[i]==aSpec[1] || zText[i]==aSpec[2]
      ){

        if( fts5ExprCountChar(&zText[iFirst], i-iFirst)>=3 ){
          int jj;
          zExpr[iOut++] = '"';
          for(jj=iFirst; jj<i; jj++){
            zExpr[iOut++] = zText[jj];
            if( zText[jj]=='"' ) zExpr[iOut++] = '"';
          }
          zExpr[iOut++] = '"';
          zExpr[iOut++] = ' ';
        }
        if( zText[i]==aSpec[2] ){
          i += 2;
          if( zText[i-1]=='^' ) i++;
          while( i<nText && zText[i]!=']' ) i++;
        }
        iFirst = i+1;
      }
      i++;
    }
    if( iOut>0 ){
      int bAnd = 0;
      if( pConfig->eDetail!=FTS5_DETAIL_FULL ){
        bAnd = 1;
        if( pConfig->eDetail==FTS5_DETAIL_NONE ){
          iCol = pConfig->nCol;
        }
      }
      zExpr[iOut] = '\0';
      rc = sqlite3Fts5ExprNew(pConfig, bAnd, iCol, zExpr, pp,pConfig->pzErrmsg);
    }else{
      *pp = 0;
    }
    sqlite3_free(zExpr);
  }

  return rc;
}

/*
** Free the expression node object passed as the only argument.
*/
static void sqlite3Fts5ParseNodeFree(Fts5ExprNode *p){
  if( p ){
    int i;
    for(i=0; i<p->nChild; i++){
      sqlite3Fts5ParseNodeFree(p->apChild[i]);
    }
    sqlite3Fts5ParseNearsetFree(p->pNear);
    sqlite3_free(p);
  }
}

/*
** Free the expression object passed as the only argument.
*/
static void sqlite3Fts5ExprFree(Fts5Expr *p){
  if( p ){
    sqlite3Fts5ParseNodeFree(p->pRoot);
    sqlite3_free(p->apExprPhrase);
    sqlite3_free(p);
  }
}

static int sqlite3Fts5ExprAnd(Fts5Expr **pp1, Fts5Expr *p2){
  Fts5Parse sParse;
  memset(&sParse, 0, sizeof(sParse));

  if( *pp1 && p2 ){
    Fts5Expr *p1 = *pp1;
    int nPhrase = p1->nPhrase + p2->nPhrase;

    p1->pRoot = sqlite3Fts5ParseNode(&sParse, FTS5_AND, p1->pRoot, p2->pRoot,0);
    p2->pRoot = 0;

    if( sParse.rc==SQLITE_OK ){
      Fts5ExprPhrase **ap = (Fts5ExprPhrase**)sqlite3_realloc(
          p1->apExprPhrase, nPhrase * sizeof(Fts5ExprPhrase*)
      );
      if( ap==0 ){
        sParse.rc = SQLITE_NOMEM;
      }else{
        int i;
        memmove(&ap[p2->nPhrase], ap, p1->nPhrase*sizeof(Fts5ExprPhrase*));
        for(i=0; i<p2->nPhrase; i++){
          ap[i] = p2->apExprPhrase[i];
        }
        p1->nPhrase = nPhrase;
        p1->apExprPhrase = ap;
      }
    }
    sqlite3_free(p2->apExprPhrase);
    sqlite3_free(p2);
  }else if( p2 ){
    *pp1 = p2;
  }

  return sParse.rc;
}

/*
** Argument pTerm must be a synonym iterator. Return the current rowid
** that it points to.
*/
static i64 fts5ExprSynonymRowid(Fts5ExprTerm *pTerm, int bDesc, int *pbEof){
  i64 iRet = 0;
  int bRetValid = 0;
  Fts5ExprTerm *p;

  assert( pTerm );
  assert( pTerm->pSynonym );
  assert( bDesc==0 || bDesc==1 );
  for(p=pTerm; p; p=p->pSynonym){
    if( 0==sqlite3Fts5IterEof(p->pIter) ){
      i64 iRowid = p->pIter->iRowid;
      if( bRetValid==0 || (bDesc!=(iRowid<iRet)) ){
        iRet = iRowid;
        bRetValid = 1;
      }
    }
  }

  if( pbEof && bRetValid==0 ) *pbEof = 1;
  return iRet;
}

/*
** Argument pTerm must be a synonym iterator.
*/
static int fts5ExprSynonymList(
  Fts5ExprTerm *pTerm,
  i64 iRowid,
  Fts5Buffer *pBuf,               /* Use this buffer for space if required */
  u8 **pa, int *pn
){
  Fts5PoslistReader aStatic[4];
  Fts5PoslistReader *aIter = aStatic;
  int nIter = 0;
  int nAlloc = 4;
  int rc = SQLITE_OK;
  Fts5ExprTerm *p;

  assert( pTerm->pSynonym );
  for(p=pTerm; p; p=p->pSynonym){
    Fts5IndexIter *pIter = p->pIter;
    if( sqlite3Fts5IterEof(pIter)==0 && pIter->iRowid==iRowid ){
      if( pIter->nData==0 ) continue;
      if( nIter==nAlloc ){
        sqlite3_int64 nByte = sizeof(Fts5PoslistReader) * nAlloc * 2;
        Fts5PoslistReader *aNew = (Fts5PoslistReader*)sqlite3_malloc64(nByte);
        if( aNew==0 ){
          rc = SQLITE_NOMEM;
          goto synonym_poslist_out;
        }
        memcpy(aNew, aIter, sizeof(Fts5PoslistReader) * nIter);
        nAlloc = nAlloc*2;
        if( aIter!=aStatic ) sqlite3_free(aIter);
        aIter = aNew;
      }
      sqlite3Fts5PoslistReaderInit(pIter->pData, pIter->nData, &aIter[nIter]);
      assert( aIter[nIter].bEof==0 );
      nIter++;
    }
  }

  if( nIter==1 ){
    *pa = (u8*)aIter[0].a;
    *pn = aIter[0].n;
  }else{
    Fts5PoslistWriter writer = {0};
    i64 iPrev = -1;
    fts5BufferZero(pBuf);
    while( 1 ){
      int i;
      i64 iMin = FTS5_LARGEST_INT64;
      for(i=0; i<nIter; i++){
        if( aIter[i].bEof==0 ){
          if( aIter[i].iPos==iPrev ){
            if( sqlite3Fts5PoslistReaderNext(&aIter[i]) ) continue;
          }
          if( aIter[i].iPos<iMin ){
            iMin = aIter[i].iPos;
          }
        }
      }
      if( iMin==FTS5_LARGEST_INT64 || rc!=SQLITE_OK ) break;
      rc = sqlite3Fts5PoslistWriterAppend(pBuf, &writer, iMin);
      iPrev = iMin;
    }
    if( rc==SQLITE_OK ){
      *pa = pBuf->p;
      *pn = pBuf->n;
    }
  }

 synonym_poslist_out:
  if( aIter!=aStatic ) sqlite3_free(aIter);
  return rc;
}
