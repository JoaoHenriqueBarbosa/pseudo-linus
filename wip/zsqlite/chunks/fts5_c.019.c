
/*
** Allocate and return an Fts5Colset object specifying the inverse of
** the colset passed as the second argument. Free the colset passed
** as the second argument before returning.
*/
static Fts5Colset *sqlite3Fts5ParseColsetInvert(Fts5Parse *pParse, Fts5Colset *p){
  Fts5Colset *pRet;
  int nCol = pParse->pConfig->nCol;

  pRet = (Fts5Colset*)sqlite3Fts5MallocZero(&pParse->rc,
      sizeof(Fts5Colset) + sizeof(int)*nCol
  );
  if( pRet ){
    int i;
    int iOld = 0;
    for(i=0; i<nCol; i++){
      if( iOld>=p->nCol || p->aiCol[iOld]!=i ){
        pRet->aiCol[pRet->nCol++] = i;
      }else{
        iOld++;
      }
    }
  }

  sqlite3_free(p);
  return pRet;
}

static Fts5Colset *sqlite3Fts5ParseColset(
  Fts5Parse *pParse,              /* Store SQLITE_NOMEM here if required */
  Fts5Colset *pColset,            /* Existing colset object */
  Fts5Token *p
){
  Fts5Colset *pRet = 0;
  int iCol;
  char *z;                        /* Dequoted copy of token p */

  z = sqlite3Fts5Strndup(&pParse->rc, p->p, p->n);
  if( pParse->rc==SQLITE_OK ){
    Fts5Config *pConfig = pParse->pConfig;
    sqlite3Fts5Dequote(z);
    for(iCol=0; iCol<pConfig->nCol; iCol++){
      if( 0==sqlite3_stricmp(pConfig->azCol[iCol], z) ) break;
    }
    if( iCol==pConfig->nCol ){
      sqlite3Fts5ParseError(pParse, "no such column: %s", z);
    }else{
      pRet = fts5ParseColset(pParse, pColset, iCol);
    }
    sqlite3_free(z);
  }

  if( pRet==0 ){
    assert( pParse->rc!=SQLITE_OK );
    sqlite3_free(pColset);
  }

  return pRet;
}

/*
** If argument pOrig is NULL, or if (*pRc) is set to anything other than
** SQLITE_OK when this function is called, NULL is returned.
**
** Otherwise, a copy of (*pOrig) is made into memory obtained from
** sqlite3Fts5MallocZero() and a pointer to it returned. If the allocation
** fails, (*pRc) is set to SQLITE_NOMEM and NULL is returned.
*/
static Fts5Colset *fts5CloneColset(int *pRc, Fts5Colset *pOrig){
  Fts5Colset *pRet;
  if( pOrig ){
    sqlite3_int64 nByte = sizeof(Fts5Colset) + (pOrig->nCol-1) * sizeof(int);
    pRet = (Fts5Colset*)sqlite3Fts5MallocZero(pRc, nByte);
    if( pRet ){
      memcpy(pRet, pOrig, (size_t)nByte);
    }
  }else{
    pRet = 0;
  }
  return pRet;
}

/*
** Remove from colset pColset any columns that are not also in colset pMerge.
*/
static void fts5MergeColset(Fts5Colset *pColset, Fts5Colset *pMerge){
  int iIn = 0;          /* Next input in pColset */
  int iMerge = 0;       /* Next input in pMerge */
  int iOut = 0;         /* Next output slot in pColset */

  while( iIn<pColset->nCol && iMerge<pMerge->nCol ){
    int iDiff = pColset->aiCol[iIn] - pMerge->aiCol[iMerge];
    if( iDiff==0 ){
      pColset->aiCol[iOut++] = pMerge->aiCol[iMerge];
      iMerge++;
      iIn++;
    }else if( iDiff>0 ){
      iMerge++;
    }else{
      iIn++;
    }
  }
  pColset->nCol = iOut;
}

/*
** Recursively apply colset pColset to expression node pNode and all of
** its decendents. If (*ppFree) is not NULL, it contains a spare copy
** of pColset. This function may use the spare copy and set (*ppFree) to
** zero, or it may create copies of pColset using fts5CloneColset().
*/
static void fts5ParseSetColset(
  Fts5Parse *pParse,
  Fts5ExprNode *pNode,
  Fts5Colset *pColset,
  Fts5Colset **ppFree
){
  if( pParse->rc==SQLITE_OK ){
    assert( pNode->eType==FTS5_TERM || pNode->eType==FTS5_STRING
         || pNode->eType==FTS5_AND  || pNode->eType==FTS5_OR
         || pNode->eType==FTS5_NOT  || pNode->eType==FTS5_EOF
    );
    if( pNode->eType==FTS5_STRING || pNode->eType==FTS5_TERM ){
      Fts5ExprNearset *pNear = pNode->pNear;
      if( pNear->pColset ){
        fts5MergeColset(pNear->pColset, pColset);
        if( pNear->pColset->nCol==0 ){
          pNode->eType = FTS5_EOF;
          pNode->xNext = 0;
        }
      }else if( *ppFree ){
        pNear->pColset = pColset;
        *ppFree = 0;
      }else{
        pNear->pColset = fts5CloneColset(&pParse->rc, pColset);
      }
    }else{
      int i;
      assert( pNode->eType!=FTS5_EOF || pNode->nChild==0 );
      for(i=0; i<pNode->nChild; i++){
        fts5ParseSetColset(pParse, pNode->apChild[i], pColset, ppFree);
      }
    }
  }
}

/*
** Apply colset pColset to expression node pExpr and all of its descendents.
*/
static void sqlite3Fts5ParseSetColset(
  Fts5Parse *pParse,
  Fts5ExprNode *pExpr,
  Fts5Colset *pColset
){
  Fts5Colset *pFree = pColset;
  if( pParse->pConfig->eDetail==FTS5_DETAIL_NONE ){
    sqlite3Fts5ParseError(pParse,
        "fts5: column queries are not supported (detail=none)"
    );
  }else{
    fts5ParseSetColset(pParse, pExpr, pColset, &pFree);
  }
  sqlite3_free(pFree);
}

static void fts5ExprAssignXNext(Fts5ExprNode *pNode){
  switch( pNode->eType ){
    case FTS5_STRING: {
      Fts5ExprNearset *pNear = pNode->pNear;
      if( pNear->nPhrase==1 && pNear->apPhrase[0]->nTerm==1
       && pNear->apPhrase[0]->aTerm[0].pSynonym==0
       && pNear->apPhrase[0]->aTerm[0].bFirst==0
      ){
        pNode->eType = FTS5_TERM;
        pNode->xNext = fts5ExprNodeNext_TERM;
      }else{
        pNode->xNext = fts5ExprNodeNext_STRING;
      }
      break;
    };

    case FTS5_OR: {
      pNode->xNext = fts5ExprNodeNext_OR;
      break;
    };

    case FTS5_AND: {
      pNode->xNext = fts5ExprNodeNext_AND;
      break;
    };

    default: assert( pNode->eType==FTS5_NOT ); {
      pNode->xNext = fts5ExprNodeNext_NOT;
      break;
    };
  }
}

static void fts5ExprAddChildren(Fts5ExprNode *p, Fts5ExprNode *pSub){
  int ii = p->nChild;
  if( p->eType!=FTS5_NOT && pSub->eType==p->eType ){
    int nByte = sizeof(Fts5ExprNode*) * pSub->nChild;
    memcpy(&p->apChild[p->nChild], pSub->apChild, nByte);
    p->nChild += pSub->nChild;
    sqlite3_free(pSub);
  }else{
    p->apChild[p->nChild++] = pSub;
  }
  for( ; ii<p->nChild; ii++){
    p->iHeight = MAX(p->iHeight, p->apChild[ii]->iHeight + 1);
  }
}

/*
** This function is used when parsing LIKE or GLOB patterns against
** trigram indexes that specify either detail=column or detail=none.
** It converts a phrase:
**
**     abc + def + ghi
**
** into an AND tree:
**
**     abc AND def AND ghi
*/
static Fts5ExprNode *fts5ParsePhraseToAnd(
  Fts5Parse *pParse,
  Fts5ExprNearset *pNear
){
  int nTerm = pNear->apPhrase[0]->nTerm;
  int ii;
  int nByte;
  Fts5ExprNode *pRet;

  assert( pNear->nPhrase==1 );
  assert( pParse->bPhraseToAnd );

  nByte = sizeof(Fts5ExprNode) + nTerm*sizeof(Fts5ExprNode*);
  pRet = (Fts5ExprNode*)sqlite3Fts5MallocZero(&pParse->rc, nByte);
  if( pRet ){
    pRet->eType = FTS5_AND;
    pRet->nChild = nTerm;
    pRet->iHeight = 1;
    fts5ExprAssignXNext(pRet);
    pParse->nPhrase--;
    for(ii=0; ii<nTerm; ii++){
      Fts5ExprPhrase *pPhrase = (Fts5ExprPhrase*)sqlite3Fts5MallocZero(
          &pParse->rc, sizeof(Fts5ExprPhrase)
      );
      if( pPhrase ){
        if( parseGrowPhraseArray(pParse) ){
          fts5ExprPhraseFree(pPhrase);
        }else{
          Fts5ExprTerm *p = &pNear->apPhrase[0]->aTerm[ii];
          Fts5ExprTerm *pTo = &pPhrase->aTerm[0];
          pParse->apPhrase[pParse->nPhrase++] = pPhrase;
          pPhrase->nTerm = 1;
          pTo->pTerm = sqlite3Fts5Strndup(&pParse->rc, p->pTerm, p->nFullTerm);
          pTo->nQueryTerm = p->nQueryTerm;
          pTo->nFullTerm = p->nFullTerm;
          pRet->apChild[ii] = sqlite3Fts5ParseNode(pParse, FTS5_STRING,
              0, 0, sqlite3Fts5ParseNearset(pParse, 0, pPhrase)
          );
        }
      }
    }

    if( pParse->rc ){
      sqlite3Fts5ParseNodeFree(pRet);
      pRet = 0;
    }else{
      sqlite3Fts5ParseNearsetFree(pNear);
    }
  }

  return pRet;
}

/*
** Allocate and return a new expression object. If anything goes wrong (i.e.
** OOM error), leave an error code in pParse and return NULL.
*/
static Fts5ExprNode *sqlite3Fts5ParseNode(
  Fts5Parse *pParse,              /* Parse context */
  int eType,                      /* FTS5_STRING, AND, OR or NOT */
  Fts5ExprNode *pLeft,            /* Left hand child expression */
  Fts5ExprNode *pRight,           /* Right hand child expression */
  Fts5ExprNearset *pNear          /* For STRING expressions, the near cluster */
){
  Fts5ExprNode *pRet = 0;

  if( pParse->rc==SQLITE_OK ){
    int nChild = 0;               /* Number of children of returned node */
    sqlite3_int64 nByte;          /* Bytes of space to allocate for this node */

    assert( (eType!=FTS5_STRING && !pNear)
         || (eType==FTS5_STRING && !pLeft && !pRight)
    );
    if( eType==FTS5_STRING && pNear==0 ) return 0;
    if( eType!=FTS5_STRING && pLeft==0 ) return pRight;
    if( eType!=FTS5_STRING && pRight==0 ) return pLeft;

    if( eType==FTS5_STRING
     && pParse->bPhraseToAnd
     && pNear->apPhrase[0]->nTerm>1
    ){
      pRet = fts5ParsePhraseToAnd(pParse, pNear);
    }else{
      if( eType==FTS5_NOT ){
        nChild = 2;
      }else if( eType==FTS5_AND || eType==FTS5_OR ){
        nChild = 2;
        if( pLeft->eType==eType ) nChild += pLeft->nChild-1;
        if( pRight->eType==eType ) nChild += pRight->nChild-1;
      }

      nByte = sizeof(Fts5ExprNode) + sizeof(Fts5ExprNode*)*(nChild-1);
      pRet = (Fts5ExprNode*)sqlite3Fts5MallocZero(&pParse->rc, nByte);

      if( pRet ){
        pRet->eType = eType;
        pRet->pNear = pNear;
        fts5ExprAssignXNext(pRet);
        if( eType==FTS5_STRING ){
          int iPhrase;
          for(iPhrase=0; iPhrase<pNear->nPhrase; iPhrase++){
            pNear->apPhrase[iPhrase]->pNode = pRet;
            if( pNear->apPhrase[iPhrase]->nTerm==0 ){
              pRet->xNext = 0;
              pRet->eType = FTS5_EOF;
            }
          }

          if( pParse->pConfig->eDetail!=FTS5_DETAIL_FULL ){
            Fts5ExprPhrase *pPhrase = pNear->apPhrase[0];
            if( pNear->nPhrase!=1
                || pPhrase->nTerm>1
                || (pPhrase->nTerm>0 && pPhrase->aTerm[0].bFirst)
              ){
              sqlite3Fts5ParseError(pParse,
                  "fts5: %s queries are not supported (detail!=full)",
                  pNear->nPhrase==1 ? "phrase": "NEAR"
              );
              sqlite3_free(pRet);
              pRet = 0;
            }
          }
        }else{
          fts5ExprAddChildren(pRet, pLeft);
          fts5ExprAddChildren(pRet, pRight);
          if( pRet->iHeight>SQLITE_FTS5_MAX_EXPR_DEPTH ){
            sqlite3Fts5ParseError(pParse,
                "fts5 expression tree is too large (maximum depth %d)",
                SQLITE_FTS5_MAX_EXPR_DEPTH
            );
            sqlite3_free(pRet);
            pRet = 0;
          }
        }
      }
    }
  }

  if( pRet==0 ){
    assert( pParse->rc!=SQLITE_OK );
    sqlite3Fts5ParseNodeFree(pLeft);
    sqlite3Fts5ParseNodeFree(pRight);
    sqlite3Fts5ParseNearsetFree(pNear);
  }
  return pRet;
}
