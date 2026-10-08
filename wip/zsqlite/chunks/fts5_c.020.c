
static Fts5ExprNode *sqlite3Fts5ParseImplicitAnd(
  Fts5Parse *pParse,              /* Parse context */
  Fts5ExprNode *pLeft,            /* Left hand child expression */
  Fts5ExprNode *pRight            /* Right hand child expression */
){
  Fts5ExprNode *pRet = 0;
  Fts5ExprNode *pPrev;

  if( pParse->rc ){
    sqlite3Fts5ParseNodeFree(pLeft);
    sqlite3Fts5ParseNodeFree(pRight);
  }else{

    assert( pLeft->eType==FTS5_STRING
        || pLeft->eType==FTS5_TERM
        || pLeft->eType==FTS5_EOF
        || pLeft->eType==FTS5_AND
    );
    assert( pRight->eType==FTS5_STRING
        || pRight->eType==FTS5_TERM
        || pRight->eType==FTS5_EOF
        || (pRight->eType==FTS5_AND && pParse->bPhraseToAnd)
    );

    if( pLeft->eType==FTS5_AND ){
      pPrev = pLeft->apChild[pLeft->nChild-1];
    }else{
      pPrev = pLeft;
    }
    assert( pPrev->eType==FTS5_STRING
        || pPrev->eType==FTS5_TERM
        || pPrev->eType==FTS5_EOF
        );

    if( pRight->eType==FTS5_EOF ){
      assert( pParse->apPhrase[pParse->nPhrase-1]==pRight->pNear->apPhrase[0] );
      sqlite3Fts5ParseNodeFree(pRight);
      pRet = pLeft;
      pParse->nPhrase--;
    }
    else if( pPrev->eType==FTS5_EOF ){
      Fts5ExprPhrase **ap;

      if( pPrev==pLeft ){
        pRet = pRight;
      }else{
        pLeft->apChild[pLeft->nChild-1] = pRight;
        pRet = pLeft;
      }

      ap = &pParse->apPhrase[pParse->nPhrase-1-pRight->pNear->nPhrase];
      assert( ap[0]==pPrev->pNear->apPhrase[0] );
      memmove(ap, &ap[1], sizeof(Fts5ExprPhrase*)*pRight->pNear->nPhrase);
      pParse->nPhrase--;

      sqlite3Fts5ParseNodeFree(pPrev);
    }
    else{
      pRet = sqlite3Fts5ParseNode(pParse, FTS5_AND, pLeft, pRight, 0);
    }
  }

  return pRet;
}

#if defined(SQLITE_TEST) || defined(SQLITE_FTS5_DEBUG)
static char *fts5ExprTermPrint(Fts5ExprTerm *pTerm){
  sqlite3_int64 nByte = 0;
  Fts5ExprTerm *p;
  char *zQuoted;

  /* Determine the maximum amount of space required. */
  for(p=pTerm; p; p=p->pSynonym){
    nByte += pTerm->nQueryTerm * 2 + 3 + 2;
  }
  zQuoted = sqlite3_malloc64(nByte);

  if( zQuoted ){
    int i = 0;
    for(p=pTerm; p; p=p->pSynonym){
      char *zIn = p->pTerm;
      char *zEnd = &zIn[p->nQueryTerm];
      zQuoted[i++] = '"';
      while( zIn<zEnd ){
        if( *zIn=='"' ) zQuoted[i++] = '"';
        zQuoted[i++] = *zIn++;
      }
      zQuoted[i++] = '"';
      if( p->pSynonym ) zQuoted[i++] = '|';
    }
    if( pTerm->bPrefix ){
      zQuoted[i++] = ' ';
      zQuoted[i++] = '*';
    }
    zQuoted[i++] = '\0';
  }
  return zQuoted;
}

static char *fts5PrintfAppend(char *zApp, const char *zFmt, ...){
  char *zNew;
  va_list ap;
  va_start(ap, zFmt);
  zNew = sqlite3_vmprintf(zFmt, ap);
  va_end(ap);
  if( zApp && zNew ){
    char *zNew2 = sqlite3_mprintf("%s%s", zApp, zNew);
    sqlite3_free(zNew);
    zNew = zNew2;
  }
  sqlite3_free(zApp);
  return zNew;
}

/*
** Compose a tcl-readable representation of expression pExpr. Return a
** pointer to a buffer containing that representation. It is the
** responsibility of the caller to at some point free the buffer using
** sqlite3_free().
*/
static char *fts5ExprPrintTcl(
  Fts5Config *pConfig,
  const char *zNearsetCmd,
  Fts5ExprNode *pExpr
){
  char *zRet = 0;
  if( pExpr->eType==FTS5_STRING || pExpr->eType==FTS5_TERM ){
    Fts5ExprNearset *pNear = pExpr->pNear;
    int i;
    int iTerm;

    zRet = fts5PrintfAppend(zRet, "%s ", zNearsetCmd);
    if( zRet==0 ) return 0;
    if( pNear->pColset ){
      int *aiCol = pNear->pColset->aiCol;
      int nCol = pNear->pColset->nCol;
      if( nCol==1 ){
        zRet = fts5PrintfAppend(zRet, "-col %d ", aiCol[0]);
      }else{
        zRet = fts5PrintfAppend(zRet, "-col {%d", aiCol[0]);
        for(i=1; i<pNear->pColset->nCol; i++){
          zRet = fts5PrintfAppend(zRet, " %d", aiCol[i]);
        }
        zRet = fts5PrintfAppend(zRet, "} ");
      }
      if( zRet==0 ) return 0;
    }

    if( pNear->nPhrase>1 ){
      zRet = fts5PrintfAppend(zRet, "-near %d ", pNear->nNear);
      if( zRet==0 ) return 0;
    }

    zRet = fts5PrintfAppend(zRet, "--");
    if( zRet==0 ) return 0;

    for(i=0; i<pNear->nPhrase; i++){
      Fts5ExprPhrase *pPhrase = pNear->apPhrase[i];

      zRet = fts5PrintfAppend(zRet, " {");
      for(iTerm=0; zRet && iTerm<pPhrase->nTerm; iTerm++){
        Fts5ExprTerm *p = &pPhrase->aTerm[iTerm];
        zRet = fts5PrintfAppend(zRet, "%s%.*s", iTerm==0?"":" ",
            p->nQueryTerm, p->pTerm
        );
        if( pPhrase->aTerm[iTerm].bPrefix ){
          zRet = fts5PrintfAppend(zRet, "*");
        }
      }

      if( zRet ) zRet = fts5PrintfAppend(zRet, "}");
      if( zRet==0 ) return 0;
    }

  }else if( pExpr->eType==0 ){
    zRet = sqlite3_mprintf("{}");
  }else{
    char const *zOp = 0;
    int i;
    switch( pExpr->eType ){
      case FTS5_AND: zOp = "AND"; break;
      case FTS5_NOT: zOp = "NOT"; break;
      default:
        assert( pExpr->eType==FTS5_OR );
        zOp = "OR";
        break;
    }

    zRet = sqlite3_mprintf("%s", zOp);
    for(i=0; zRet && i<pExpr->nChild; i++){
      char *z = fts5ExprPrintTcl(pConfig, zNearsetCmd, pExpr->apChild[i]);
      if( !z ){
        sqlite3_free(zRet);
        zRet = 0;
      }else{
        zRet = fts5PrintfAppend(zRet, " [%z]", z);
      }
    }
  }

  return zRet;
}

static char *fts5ExprPrint(Fts5Config *pConfig, Fts5ExprNode *pExpr){
  char *zRet = 0;
  if( pExpr->eType==0 ){
    return sqlite3_mprintf("\"\"");
  }else
  if( pExpr->eType==FTS5_STRING || pExpr->eType==FTS5_TERM ){
    Fts5ExprNearset *pNear = pExpr->pNear;
    int i;
    int iTerm;

    if( pNear->pColset ){
      int ii;
      Fts5Colset *pColset = pNear->pColset;
      if( pColset->nCol>1 ) zRet = fts5PrintfAppend(zRet, "{");
      for(ii=0; ii<pColset->nCol; ii++){
        zRet = fts5PrintfAppend(zRet, "%s%s",
            pConfig->azCol[pColset->aiCol[ii]], ii==pColset->nCol-1 ? "" : " "
        );
      }
      if( zRet ){
        zRet = fts5PrintfAppend(zRet, "%s : ", pColset->nCol>1 ? "}" : "");
      }
      if( zRet==0 ) return 0;
    }

    if( pNear->nPhrase>1 ){
      zRet = fts5PrintfAppend(zRet, "NEAR(");
      if( zRet==0 ) return 0;
    }

    for(i=0; i<pNear->nPhrase; i++){
      Fts5ExprPhrase *pPhrase = pNear->apPhrase[i];
      if( i!=0 ){
        zRet = fts5PrintfAppend(zRet, " ");
        if( zRet==0 ) return 0;
      }
      for(iTerm=0; iTerm<pPhrase->nTerm; iTerm++){
        char *zTerm = fts5ExprTermPrint(&pPhrase->aTerm[iTerm]);
        if( zTerm ){
          zRet = fts5PrintfAppend(zRet, "%s%s", iTerm==0?"":" + ", zTerm);
          sqlite3_free(zTerm);
        }
        if( zTerm==0 || zRet==0 ){
          sqlite3_free(zRet);
          return 0;
        }
      }
    }

    if( pNear->nPhrase>1 ){
      zRet = fts5PrintfAppend(zRet, ", %d)", pNear->nNear);
      if( zRet==0 ) return 0;
    }

  }else{
    char const *zOp = 0;
    int i;

    switch( pExpr->eType ){
      case FTS5_AND: zOp = " AND "; break;
      case FTS5_NOT: zOp = " NOT "; break;
      default:
        assert( pExpr->eType==FTS5_OR );
        zOp = " OR ";
        break;
    }

    for(i=0; i<pExpr->nChild; i++){
      char *z = fts5ExprPrint(pConfig, pExpr->apChild[i]);
      if( z==0 ){
        sqlite3_free(zRet);
        zRet = 0;
      }else{
        int e = pExpr->apChild[i]->eType;
        int b = (e!=FTS5_STRING && e!=FTS5_TERM && e!=FTS5_EOF);
        zRet = fts5PrintfAppend(zRet, "%s%s%z%s",
            (i==0 ? "" : zOp),
            (b?"(":""), z, (b?")":"")
        );
      }
      if( zRet==0 ) break;
    }
  }

  return zRet;
}

/*
** The implementation of user-defined scalar functions fts5_expr() (bTcl==0)
** and fts5_expr_tcl() (bTcl!=0).
*/
static void fts5ExprFunction(
  sqlite3_context *pCtx,          /* Function call context */
  int nArg,                       /* Number of args */
  sqlite3_value **apVal,          /* Function arguments */
  int bTcl
){
  Fts5Global *pGlobal = (Fts5Global*)sqlite3_user_data(pCtx);
  sqlite3 *db = sqlite3_context_db_handle(pCtx);
  const char *zExpr = 0;
  char *zErr = 0;
  Fts5Expr *pExpr = 0;
  int rc;
  int i;

  const char **azConfig;          /* Array of arguments for Fts5Config */
  const char *zNearsetCmd = "nearset";
  int nConfig;                    /* Size of azConfig[] */
  Fts5Config *pConfig = 0;
  int iArg = 1;

  if( nArg<1 ){
    zErr = sqlite3_mprintf("wrong number of arguments to function %s",
        bTcl ? "fts5_expr_tcl" : "fts5_expr"
    );
    sqlite3_result_error(pCtx, zErr, -1);
    sqlite3_free(zErr);
    return;
  }

  if( bTcl && nArg>1 ){
    zNearsetCmd = (const char*)sqlite3_value_text(apVal[1]);
    iArg = 2;
  }

  nConfig = 3 + (nArg-iArg);
  azConfig = (const char**)sqlite3_malloc64(sizeof(char*) * nConfig);
  if( azConfig==0 ){
    sqlite3_result_error_nomem(pCtx);
    return;
  }
  azConfig[0] = 0;
  azConfig[1] = "main";
  azConfig[2] = "tbl";
  for(i=3; iArg<nArg; iArg++){
    const char *z = (const char*)sqlite3_value_text(apVal[iArg]);
    azConfig[i++] = (z ? z : "");
  }

  zExpr = (const char*)sqlite3_value_text(apVal[0]);
  if( zExpr==0 ) zExpr = "";

  rc = sqlite3Fts5ConfigParse(pGlobal, db, nConfig, azConfig, &pConfig, &zErr);
  if( rc==SQLITE_OK ){
    rc = sqlite3Fts5ExprNew(pConfig, 0, pConfig->nCol, zExpr, &pExpr, &zErr);
  }
  if( rc==SQLITE_OK ){
    char *zText;
    if( pExpr->pRoot->xNext==0 ){
      zText = sqlite3_mprintf("");
    }else if( bTcl ){
      zText = fts5ExprPrintTcl(pConfig, zNearsetCmd, pExpr->pRoot);
    }else{
      zText = fts5ExprPrint(pConfig, pExpr->pRoot);
    }
    if( zText==0 ){
      rc = SQLITE_NOMEM;
    }else{
      sqlite3_result_text(pCtx, zText, -1, SQLITE_TRANSIENT);
      sqlite3_free(zText);
    }
  }

  if( rc!=SQLITE_OK ){
    if( zErr ){
      sqlite3_result_error(pCtx, zErr, -1);
      sqlite3_free(zErr);
    }else{
      sqlite3_result_error_code(pCtx, rc);
    }
  }
  sqlite3_free((void *)azConfig);
  sqlite3Fts5ConfigFree(pConfig);
  sqlite3Fts5ExprFree(pExpr);
}
