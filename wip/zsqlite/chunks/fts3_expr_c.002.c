
/*
** Return SQLITE_ERROR if the maximum depth of the expression tree passed
** as the only argument is more than nMaxDepth.
*/
static int fts3ExprCheckDepth(Fts3Expr *p, int nMaxDepth){
  int rc = SQLITE_OK;
  if( p ){
    if( nMaxDepth<0 ){
      rc = SQLITE_TOOBIG;
    }else{
      rc = fts3ExprCheckDepth(p->pLeft, nMaxDepth-1);
      if( rc==SQLITE_OK ){
        rc = fts3ExprCheckDepth(p->pRight, nMaxDepth-1);
      }
    }
  }
  return rc;
}

/*
** This function attempts to transform the expression tree at (*pp) to
** an equivalent but more balanced form. The tree is modified in place.
** If successful, SQLITE_OK is returned and (*pp) set to point to the
** new root expression node.
**
** nMaxDepth is the maximum allowable depth of the balanced sub-tree.
**
** Otherwise, if an error occurs, an SQLite error code is returned and
** expression (*pp) freed.
*/
static int fts3ExprBalance(Fts3Expr **pp, int nMaxDepth){
  int rc = SQLITE_OK;             /* Return code */
  Fts3Expr *pRoot = *pp;          /* Initial root node */
  Fts3Expr *pFree = 0;            /* List of free nodes. Linked by pParent. */
  int eType = pRoot->eType;       /* Type of node in this tree */

  if( nMaxDepth==0 ){
    rc = SQLITE_ERROR;
  }

  if( rc==SQLITE_OK ){
    if( (eType==FTSQUERY_AND || eType==FTSQUERY_OR) ){
      Fts3Expr **apLeaf;
      apLeaf = (Fts3Expr **)sqlite3_malloc64(sizeof(Fts3Expr *) * nMaxDepth);
      if( 0==apLeaf ){
        rc = SQLITE_NOMEM;
      }else{
        memset(apLeaf, 0, sizeof(Fts3Expr *) * nMaxDepth);
      }

      if( rc==SQLITE_OK ){
        int i;
        Fts3Expr *p;

        /* Set $p to point to the left-most leaf in the tree of eType nodes. */
        for(p=pRoot; p->eType==eType; p=p->pLeft){
          assert( p->pParent==0 || p->pParent->pLeft==p );
          assert( p->pLeft && p->pRight );
        }

        /* This loop runs once for each leaf in the tree of eType nodes. */
        while( 1 ){
          int iLvl;
          Fts3Expr *pParent = p->pParent;     /* Current parent of p */

          assert( pParent==0 || pParent->pLeft==p );
          p->pParent = 0;
          if( pParent ){
            pParent->pLeft = 0;
          }else{
            pRoot = 0;
          }
          rc = fts3ExprBalance(&p, nMaxDepth-1);
          if( rc!=SQLITE_OK ) break;

          for(iLvl=0; p && iLvl<nMaxDepth; iLvl++){
            if( apLeaf[iLvl]==0 ){
              apLeaf[iLvl] = p;
              p = 0;
            }else{
              assert( pFree );
              pFree->pLeft = apLeaf[iLvl];
              pFree->pRight = p;
              pFree->pLeft->pParent = pFree;
              pFree->pRight->pParent = pFree;

              p = pFree;
              pFree = pFree->pParent;
              p->pParent = 0;
              apLeaf[iLvl] = 0;
            }
          }
          if( p ){
            sqlite3Fts3ExprFree(p);
            rc = SQLITE_TOOBIG;
            break;
          }

          /* If that was the last leaf node, break out of the loop */
          if( pParent==0 ) break;

          /* Set $p to point to the next leaf in the tree of eType nodes */
          for(p=pParent->pRight; p->eType==eType; p=p->pLeft);

          /* Remove pParent from the original tree. */
          assert( pParent->pParent==0 || pParent->pParent->pLeft==pParent );
          pParent->pRight->pParent = pParent->pParent;
          if( pParent->pParent ){
            pParent->pParent->pLeft = pParent->pRight;
          }else{
            assert( pParent==pRoot );
            pRoot = pParent->pRight;
          }

          /* Link pParent into the free node list. It will be used as an
          ** internal node of the new tree.  */
          pParent->pParent = pFree;
          pFree = pParent;
        }

        if( rc==SQLITE_OK ){
          p = 0;
          for(i=0; i<nMaxDepth; i++){
            if( apLeaf[i] ){
              if( p==0 ){
                p = apLeaf[i];
                p->pParent = 0;
              }else{
                assert( pFree!=0 );
                pFree->pRight = p;
                pFree->pLeft = apLeaf[i];
                pFree->pLeft->pParent = pFree;
                pFree->pRight->pParent = pFree;

                p = pFree;
                pFree = pFree->pParent;
                p->pParent = 0;
              }
            }
          }
          pRoot = p;
        }else{
          /* An error occurred. Delete the contents of the apLeaf[] array
          ** and pFree list. Everything else is cleaned up by the call to
          ** sqlite3Fts3ExprFree(pRoot) below.  */
          Fts3Expr *pDel;
          for(i=0; i<nMaxDepth; i++){
            sqlite3Fts3ExprFree(apLeaf[i]);
          }
          while( (pDel=pFree)!=0 ){
            pFree = pDel->pParent;
            sqlite3_free(pDel);
          }
        }

        assert( pFree==0 );
        sqlite3_free( apLeaf );
      }
    }else if( eType==FTSQUERY_NOT ){
      Fts3Expr *pLeft = pRoot->pLeft;
      Fts3Expr *pRight = pRoot->pRight;

      pRoot->pLeft = 0;
      pRoot->pRight = 0;
      pLeft->pParent = 0;
      pRight->pParent = 0;

      rc = fts3ExprBalance(&pLeft, nMaxDepth-1);
      if( rc==SQLITE_OK ){
        rc = fts3ExprBalance(&pRight, nMaxDepth-1);
      }

      if( rc!=SQLITE_OK ){
        sqlite3Fts3ExprFree(pRight);
        sqlite3Fts3ExprFree(pLeft);
      }else{
        assert( pLeft && pRight );
        pRoot->pLeft = pLeft;
        pLeft->pParent = pRoot;
        pRoot->pRight = pRight;
        pRight->pParent = pRoot;
      }
    }
  }

  if( rc!=SQLITE_OK ){
    sqlite3Fts3ExprFree(pRoot);
    pRoot = 0;
  }
  *pp = pRoot;
  return rc;
}

/*
** This function is similar to sqlite3Fts3ExprParse(), with the following
** differences:
**
**   1. It does not do expression rebalancing.
**   2. It does not check that the expression does not exceed the
**      maximum allowable depth.
**   3. Even if it fails, *ppExpr may still be set to point to an
**      expression tree. It should be deleted using sqlite3Fts3ExprFree()
**      in this case.
*/
static int fts3ExprParseUnbalanced(
  sqlite3_tokenizer *pTokenizer,      /* Tokenizer module */
  int iLangid,                        /* Language id for tokenizer */
  char **azCol,                       /* Array of column names for fts3 table */
  int bFts4,                          /* True to allow FTS4-only syntax */
  int nCol,                           /* Number of entries in azCol[] */
  int iDefaultCol,                    /* Default column to query */
  const char *z, int n,               /* Text of MATCH query */
  Fts3Expr **ppExpr                   /* OUT: Parsed query structure */
){
  int nParsed;
  int rc;
  ParseContext sParse;

  memset(&sParse, 0, sizeof(ParseContext));
  sParse.pTokenizer = pTokenizer;
  sParse.iLangid = iLangid;
  sParse.azCol = (const char **)azCol;
  sParse.nCol = nCol;
  sParse.iDefaultCol = iDefaultCol;
  sParse.bFts4 = bFts4;
  if( z==0 ){
    *ppExpr = 0;
    return SQLITE_OK;
  }
  if( n<0 ){
    n = (int)strlen(z);
  }
  rc = fts3ExprParse(&sParse, z, n, ppExpr, &nParsed);
  assert( rc==SQLITE_OK || *ppExpr==0 );

  /* Check for mismatched parenthesis */
  if( rc==SQLITE_OK && sParse.nNest ){
    rc = SQLITE_ERROR;
  }

  return rc;
}

/*
** Parameters z and n contain a pointer to and length of a buffer containing
** an fts3 query expression, respectively. This function attempts to parse the
** query expression and create a tree of Fts3Expr structures representing the
** parsed expression. If successful, *ppExpr is set to point to the head
** of the parsed expression tree and SQLITE_OK is returned. If an error
** occurs, either SQLITE_NOMEM (out-of-memory error) or SQLITE_ERROR (parse
** error) is returned and *ppExpr is set to 0.
**
** If parameter n is a negative number, then z is assumed to point to a
** nul-terminated string and the length is determined using strlen().
**
** The first parameter, pTokenizer, is passed the fts3 tokenizer module to
** use to normalize query tokens while parsing the expression. The azCol[]
** array, which is assumed to contain nCol entries, should contain the names
** of each column in the target fts3 table, in order from left to right.
** Column names must be nul-terminated strings.
**
** The iDefaultCol parameter should be passed the index of the table column
** that appears on the left-hand-side of the MATCH operator (the default
** column to match against for tokens for which a column name is not explicitly
** specified as part of the query string), or -1 if tokens may by default
** match any table column.
*/
SQLITE_PRIVATE int sqlite3Fts3ExprParse(
  sqlite3_tokenizer *pTokenizer,      /* Tokenizer module */
  int iLangid,                        /* Language id for tokenizer */
  char **azCol,                       /* Array of column names for fts3 table */
  int bFts4,                          /* True to allow FTS4-only syntax */
  int nCol,                           /* Number of entries in azCol[] */
  int iDefaultCol,                    /* Default column to query */
  const char *z, int n,               /* Text of MATCH query */
  Fts3Expr **ppExpr,                  /* OUT: Parsed query structure */
  char **pzErr                        /* OUT: Error message (sqlite3_malloc) */
){
  int rc = fts3ExprParseUnbalanced(
      pTokenizer, iLangid, azCol, bFts4, nCol, iDefaultCol, z, n, ppExpr
  );

  /* Rebalance the expression. And check that its depth does not exceed
  ** SQLITE_FTS3_MAX_EXPR_DEPTH.  */
  if( rc==SQLITE_OK && *ppExpr ){
    rc = fts3ExprBalance(ppExpr, SQLITE_FTS3_MAX_EXPR_DEPTH);
    if( rc==SQLITE_OK ){
      rc = fts3ExprCheckDepth(*ppExpr, SQLITE_FTS3_MAX_EXPR_DEPTH);
    }
  }

  if( rc!=SQLITE_OK ){
    sqlite3Fts3ExprFree(*ppExpr);
    *ppExpr = 0;
    if( rc==SQLITE_TOOBIG ){
      sqlite3Fts3ErrMsg(pzErr,
          "FTS expression tree is too large (maximum depth %d)",
          SQLITE_FTS3_MAX_EXPR_DEPTH
      );
      rc = SQLITE_ERROR;
    }else if( rc==SQLITE_ERROR ){
      sqlite3Fts3ErrMsg(pzErr, "malformed MATCH expression: [%s]", z);
    }
  }

  return rc;
}

/*
** Free a single node of an expression tree.
*/
static void fts3FreeExprNode(Fts3Expr *p){
  assert( p->eType==FTSQUERY_PHRASE || p->pPhrase==0 );
  sqlite3Fts3EvalPhraseCleanup(p->pPhrase);
  sqlite3_free(p->aMI);
  sqlite3_free(p);
}

/*
** Free a parsed fts3 query expression allocated by sqlite3Fts3ExprParse().
**
** This function would be simpler if it recursively called itself. But
** that would mean passing a sufficiently large expression to ExprParse()
** could cause a stack overflow.
*/
SQLITE_PRIVATE void sqlite3Fts3ExprFree(Fts3Expr *pDel){
  Fts3Expr *p;
  assert( pDel==0 || pDel->pParent==0 );
  for(p=pDel; p && (p->pLeft||p->pRight); p=(p->pLeft ? p->pLeft : p->pRight)){
    assert( p->pParent==0 || p==p->pParent->pRight || p==p->pParent->pLeft );
  }
  while( p ){
    Fts3Expr *pParent = p->pParent;
    fts3FreeExprNode(p);
    if( pParent && p==pParent->pLeft && pParent->pRight ){
      p = pParent->pRight;
      while( p && (p->pLeft || p->pRight) ){
        assert( p==p->pParent->pRight || p==p->pParent->pLeft );
        p = (p->pLeft ? p->pLeft : p->pRight);
      }
    }else{
      p = pParent;
    }
  }
}

/****************************************************************************
*****************************************************************************
** Everything after this point is just test code.
*/

#ifdef SQLITE_TEST

/* #include <stdio.h> */

/*
** Return a pointer to a buffer containing a text representation of the
** expression passed as the first argument. The buffer is obtained from
** sqlite3_malloc(). It is the responsibility of the caller to use
** sqlite3_free() to release the memory. If an OOM condition is encountered,
** NULL is returned.
**
** If the second argument is not NULL, then its contents are prepended to
** the returned expression text and then freed using sqlite3_free().
*/
static char *exprToString(Fts3Expr *pExpr, char *zBuf){
  if( pExpr==0 ){
    return sqlite3_mprintf("");
  }
  switch( pExpr->eType ){
    case FTSQUERY_PHRASE: {
      Fts3Phrase *pPhrase = pExpr->pPhrase;
      int i;
      zBuf = sqlite3_mprintf(
          "%zPHRASE %d 0", zBuf, pPhrase->iColumn);
      for(i=0; zBuf && i<pPhrase->nToken; i++){
        zBuf = sqlite3_mprintf("%z %.*s%s", zBuf,
            pPhrase->aToken[i].n, pPhrase->aToken[i].z,
            (pPhrase->aToken[i].isPrefix?"+":"")
        );
      }
      return zBuf;
    }

    case FTSQUERY_NEAR:
      zBuf = sqlite3_mprintf("%zNEAR/%d ", zBuf, pExpr->nNear);
      break;
    case FTSQUERY_NOT:
      zBuf = sqlite3_mprintf("%zNOT ", zBuf);
      break;
    case FTSQUERY_AND:
      zBuf = sqlite3_mprintf("%zAND ", zBuf);
      break;
    case FTSQUERY_OR:
      zBuf = sqlite3_mprintf("%zOR ", zBuf);
      break;
  }

  if( zBuf ) zBuf = sqlite3_mprintf("%z{", zBuf);
  if( zBuf ) zBuf = exprToString(pExpr->pLeft, zBuf);
  if( zBuf ) zBuf = sqlite3_mprintf("%z} {", zBuf);

  if( zBuf ) zBuf = exprToString(pExpr->pRight, zBuf);
  if( zBuf ) zBuf = sqlite3_mprintf("%z}", zBuf);

  return zBuf;
}
