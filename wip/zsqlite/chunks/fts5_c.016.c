

/*
** Initialize all term iterators in the pNear object. If any term is found
** to match no documents at all, return immediately without initializing any
** further iterators.
**
** If an error occurs, return an SQLite error code. Otherwise, return
** SQLITE_OK. It is not considered an error if some term matches zero
** documents.
*/
static int fts5ExprNearInitAll(
  Fts5Expr *pExpr,
  Fts5ExprNode *pNode
){
  Fts5ExprNearset *pNear = pNode->pNear;
  int i;

  assert( pNode->bNomatch==0 );
  for(i=0; i<pNear->nPhrase; i++){
    Fts5ExprPhrase *pPhrase = pNear->apPhrase[i];
    if( pPhrase->nTerm==0 ){
      pNode->bEof = 1;
      return SQLITE_OK;
    }else{
      int j;
      for(j=0; j<pPhrase->nTerm; j++){
        Fts5ExprTerm *pTerm = &pPhrase->aTerm[j];
        Fts5ExprTerm *p;
        int bHit = 0;

        for(p=pTerm; p; p=p->pSynonym){
          int rc;
          if( p->pIter ){
            sqlite3Fts5IterClose(p->pIter);
            p->pIter = 0;
          }
          rc = sqlite3Fts5IndexQuery(
              pExpr->pIndex, p->pTerm, p->nQueryTerm,
              (pTerm->bPrefix ? FTS5INDEX_QUERY_PREFIX : 0) |
              (pExpr->bDesc ? FTS5INDEX_QUERY_DESC : 0),
              pNear->pColset,
              &p->pIter
          );
          assert( (rc==SQLITE_OK)==(p->pIter!=0) );
          if( rc!=SQLITE_OK ) return rc;
          if( 0==sqlite3Fts5IterEof(p->pIter) ){
            bHit = 1;
          }
        }

        if( bHit==0 ){
          pNode->bEof = 1;
          return SQLITE_OK;
        }
      }
    }
  }

  pNode->bEof = 0;
  return SQLITE_OK;
}

/*
** If pExpr is an ASC iterator, this function returns a value with the
** same sign as:
**
**   (iLhs - iRhs)
**
** Otherwise, if this is a DESC iterator, the opposite is returned:
**
**   (iRhs - iLhs)
*/
static int fts5RowidCmp(
  Fts5Expr *pExpr,
  i64 iLhs,
  i64 iRhs
){
  assert( pExpr->bDesc==0 || pExpr->bDesc==1 );
  if( pExpr->bDesc==0 ){
    if( iLhs<iRhs ) return -1;
    return (iLhs > iRhs);
  }else{
    if( iLhs>iRhs ) return -1;
    return (iLhs < iRhs);
  }
}

static void fts5ExprSetEof(Fts5ExprNode *pNode){
  int i;
  pNode->bEof = 1;
  pNode->bNomatch = 0;
  for(i=0; i<pNode->nChild; i++){
    fts5ExprSetEof(pNode->apChild[i]);
  }
}

static void fts5ExprNodeZeroPoslist(Fts5ExprNode *pNode){
  if( pNode->eType==FTS5_STRING || pNode->eType==FTS5_TERM ){
    Fts5ExprNearset *pNear = pNode->pNear;
    int i;
    for(i=0; i<pNear->nPhrase; i++){
      Fts5ExprPhrase *pPhrase = pNear->apPhrase[i];
      pPhrase->poslist.n = 0;
    }
  }else{
    int i;
    for(i=0; i<pNode->nChild; i++){
      fts5ExprNodeZeroPoslist(pNode->apChild[i]);
    }
  }
}



/*
** Compare the values currently indicated by the two nodes as follows:
**
**    res = (*p1) - (*p2)
**
** Nodes that point to values that come later in the iteration order are
** considered to be larger. Nodes at EOF are the largest of all.
**
** This means that if the iteration order is ASC, then numerically larger
** rowids are considered larger. Or if it is the default DESC, numerically
** smaller rowids are larger.
*/
static int fts5NodeCompare(
  Fts5Expr *pExpr,
  Fts5ExprNode *p1,
  Fts5ExprNode *p2
){
  if( p2->bEof ) return -1;
  if( p1->bEof ) return +1;
  return fts5RowidCmp(pExpr, p1->iRowid, p2->iRowid);
}

/*
** All individual term iterators in pNear are guaranteed to be valid when
** this function is called. This function checks if all term iterators
** point to the same rowid, and if not, advances them until they do.
** If an EOF is reached before this happens, *pbEof is set to true before
** returning.
**
** SQLITE_OK is returned if an error occurs, or an SQLite error code
** otherwise. It is not considered an error code if an iterator reaches
** EOF.
*/
static int fts5ExprNodeTest_STRING(
  Fts5Expr *pExpr,                /* Expression pPhrase belongs to */
  Fts5ExprNode *pNode
){
  Fts5ExprNearset *pNear = pNode->pNear;
  Fts5ExprPhrase *pLeft = pNear->apPhrase[0];
  int rc = SQLITE_OK;
  i64 iLast;                      /* Lastest rowid any iterator points to */
  int i, j;                       /* Phrase and token index, respectively */
  int bMatch;                     /* True if all terms are at the same rowid */
  const int bDesc = pExpr->bDesc;

  /* Check that this node should not be FTS5_TERM */
  assert( pNear->nPhrase>1
       || pNear->apPhrase[0]->nTerm>1
       || pNear->apPhrase[0]->aTerm[0].pSynonym
       || pNear->apPhrase[0]->aTerm[0].bFirst
  );

  /* Initialize iLast, the "lastest" rowid any iterator points to. If the
  ** iterator skips through rowids in the default ascending order, this means
  ** the maximum rowid. Or, if the iterator is "ORDER BY rowid DESC", then it
  ** means the minimum rowid.  */
  if( pLeft->aTerm[0].pSynonym ){
    iLast = fts5ExprSynonymRowid(&pLeft->aTerm[0], bDesc, 0);
  }else{
    iLast = pLeft->aTerm[0].pIter->iRowid;
  }

  do {
    bMatch = 1;
    for(i=0; i<pNear->nPhrase; i++){
      Fts5ExprPhrase *pPhrase = pNear->apPhrase[i];
      for(j=0; j<pPhrase->nTerm; j++){
        Fts5ExprTerm *pTerm = &pPhrase->aTerm[j];
        if( pTerm->pSynonym ){
          i64 iRowid = fts5ExprSynonymRowid(pTerm, bDesc, 0);
          if( iRowid==iLast ) continue;
          bMatch = 0;
          if( fts5ExprSynonymAdvanceto(pTerm, bDesc, &iLast, &rc) ){
            pNode->bNomatch = 0;
            pNode->bEof = 1;
            return rc;
          }
        }else{
          Fts5IndexIter *pIter = pPhrase->aTerm[j].pIter;
          if( pIter->iRowid==iLast || pIter->bEof ) continue;
          bMatch = 0;
          if( fts5ExprAdvanceto(pIter, bDesc, &iLast, &rc, &pNode->bEof) ){
            return rc;
          }
        }
      }
    }
  }while( bMatch==0 );

  pNode->iRowid = iLast;
  pNode->bNomatch = ((0==fts5ExprNearTest(&rc, pExpr, pNode)) && rc==SQLITE_OK);
  assert( pNode->bEof==0 || pNode->bNomatch==0 );

  return rc;
}

/*
** Advance the first term iterator in the first phrase of pNear. Set output
** variable *pbEof to true if it reaches EOF or if an error occurs.
**
** Return SQLITE_OK if successful, or an SQLite error code if an error
** occurs.
*/
static int fts5ExprNodeNext_STRING(
  Fts5Expr *pExpr,                /* Expression pPhrase belongs to */
  Fts5ExprNode *pNode,            /* FTS5_STRING or FTS5_TERM node */
  int bFromValid,
  i64 iFrom
){
  Fts5ExprTerm *pTerm = &pNode->pNear->apPhrase[0]->aTerm[0];
  int rc = SQLITE_OK;

  pNode->bNomatch = 0;
  if( pTerm->pSynonym ){
    int bEof = 1;
    Fts5ExprTerm *p;

    /* Find the firstest rowid any synonym points to. */
    i64 iRowid = fts5ExprSynonymRowid(pTerm, pExpr->bDesc, 0);

    /* Advance each iterator that currently points to iRowid. Or, if iFrom
    ** is valid - each iterator that points to a rowid before iFrom.  */
    for(p=pTerm; p; p=p->pSynonym){
      if( sqlite3Fts5IterEof(p->pIter)==0 ){
        i64 ii = p->pIter->iRowid;
        if( ii==iRowid
         || (bFromValid && ii!=iFrom && (ii>iFrom)==pExpr->bDesc)
        ){
          if( bFromValid ){
            rc = sqlite3Fts5IterNextFrom(p->pIter, iFrom);
          }else{
            rc = sqlite3Fts5IterNext(p->pIter);
          }
          if( rc!=SQLITE_OK ) break;
          if( sqlite3Fts5IterEof(p->pIter)==0 ){
            bEof = 0;
          }
        }else{
          bEof = 0;
        }
      }
    }

    /* Set the EOF flag if either all synonym iterators are at EOF or an
    ** error has occurred.  */
    pNode->bEof = (rc || bEof);
  }else{
    Fts5IndexIter *pIter = pTerm->pIter;

    assert( Fts5NodeIsString(pNode) );
    if( bFromValid ){
      rc = sqlite3Fts5IterNextFrom(pIter, iFrom);
    }else{
      rc = sqlite3Fts5IterNext(pIter);
    }

    pNode->bEof = (rc || sqlite3Fts5IterEof(pIter));
  }

  if( pNode->bEof==0 ){
    assert( rc==SQLITE_OK );
    rc = fts5ExprNodeTest_STRING(pExpr, pNode);
  }

  return rc;
}


static int fts5ExprNodeTest_TERM(
  Fts5Expr *pExpr,                /* Expression that pNear is a part of */
  Fts5ExprNode *pNode             /* The "NEAR" node (FTS5_TERM) */
){
  /* As this "NEAR" object is actually a single phrase that consists
  ** of a single term only, grab pointers into the poslist managed by the
  ** fts5_index.c iterator object. This is much faster than synthesizing
  ** a new poslist the way we have to for more complicated phrase or NEAR
  ** expressions.  */
  Fts5ExprPhrase *pPhrase = pNode->pNear->apPhrase[0];
  Fts5IndexIter *pIter = pPhrase->aTerm[0].pIter;

  assert( pNode->eType==FTS5_TERM );
  assert( pNode->pNear->nPhrase==1 && pPhrase->nTerm==1 );
  assert( pPhrase->aTerm[0].pSynonym==0 );

  pPhrase->poslist.n = pIter->nData;
  if( pExpr->pConfig->eDetail==FTS5_DETAIL_FULL ){
    pPhrase->poslist.p = (u8*)pIter->pData;
  }
  pNode->iRowid = pIter->iRowid;
  pNode->bNomatch = (pPhrase->poslist.n==0);
  return SQLITE_OK;
}

/*
** xNext() method for a node of type FTS5_TERM.
*/
static int fts5ExprNodeNext_TERM(
  Fts5Expr *pExpr,
  Fts5ExprNode *pNode,
  int bFromValid,
  i64 iFrom
){
  int rc;
  Fts5IndexIter *pIter = pNode->pNear->apPhrase[0]->aTerm[0].pIter;

  assert( pNode->bEof==0 );
  if( bFromValid ){
    rc = sqlite3Fts5IterNextFrom(pIter, iFrom);
  }else{
    rc = sqlite3Fts5IterNext(pIter);
  }
  if( rc==SQLITE_OK && sqlite3Fts5IterEof(pIter)==0 ){
    rc = fts5ExprNodeTest_TERM(pExpr, pNode);
  }else{
    pNode->bEof = 1;
    pNode->bNomatch = 0;
  }
  return rc;
}

static void fts5ExprNodeTest_OR(
  Fts5Expr *pExpr,                /* Expression of which pNode is a part */
  Fts5ExprNode *pNode             /* Expression node to test */
){
  Fts5ExprNode *pNext = pNode->apChild[0];
  int i;

  for(i=1; i<pNode->nChild; i++){
    Fts5ExprNode *pChild = pNode->apChild[i];
    int cmp = fts5NodeCompare(pExpr, pNext, pChild);
    if( cmp>0 || (cmp==0 && pChild->bNomatch==0) ){
      pNext = pChild;
    }
  }
  pNode->iRowid = pNext->iRowid;
  pNode->bEof = pNext->bEof;
  pNode->bNomatch = pNext->bNomatch;
}
