
static int fts5ExprNodeNext_OR(
  Fts5Expr *pExpr,
  Fts5ExprNode *pNode,
  int bFromValid,
  i64 iFrom
){
  int i;
  i64 iLast = pNode->iRowid;

  for(i=0; i<pNode->nChild; i++){
    Fts5ExprNode *p1 = pNode->apChild[i];
    assert( p1->bEof || fts5RowidCmp(pExpr, p1->iRowid, iLast)>=0 );
    if( p1->bEof==0 ){
      if( (p1->iRowid==iLast)
       || (bFromValid && fts5RowidCmp(pExpr, p1->iRowid, iFrom)<0)
      ){
        int rc = fts5ExprNodeNext(pExpr, p1, bFromValid, iFrom);
        if( rc!=SQLITE_OK ){
          pNode->bNomatch = 0;
          return rc;
        }
      }
    }
  }

  fts5ExprNodeTest_OR(pExpr, pNode);
  return SQLITE_OK;
}

/*
** Argument pNode is an FTS5_AND node.
*/
static int fts5ExprNodeTest_AND(
  Fts5Expr *pExpr,                /* Expression pPhrase belongs to */
  Fts5ExprNode *pAnd              /* FTS5_AND node to advance */
){
  int iChild;
  i64 iLast = pAnd->iRowid;
  int rc = SQLITE_OK;
  int bMatch;

  assert( pAnd->bEof==0 );
  do {
    pAnd->bNomatch = 0;
    bMatch = 1;
    for(iChild=0; iChild<pAnd->nChild; iChild++){
      Fts5ExprNode *pChild = pAnd->apChild[iChild];
      int cmp = fts5RowidCmp(pExpr, iLast, pChild->iRowid);
      if( cmp>0 ){
        /* Advance pChild until it points to iLast or laster */
        rc = fts5ExprNodeNext(pExpr, pChild, 1, iLast);
        if( rc!=SQLITE_OK ){
          pAnd->bNomatch = 0;
          return rc;
        }
      }

      /* If the child node is now at EOF, so is the parent AND node. Otherwise,
      ** the child node is guaranteed to have advanced at least as far as
      ** rowid iLast. So if it is not at exactly iLast, pChild->iRowid is the
      ** new lastest rowid seen so far.  */
      assert( pChild->bEof || fts5RowidCmp(pExpr, iLast, pChild->iRowid)<=0 );
      if( pChild->bEof ){
        fts5ExprSetEof(pAnd);
        bMatch = 1;
        break;
      }else if( iLast!=pChild->iRowid ){
        bMatch = 0;
        iLast = pChild->iRowid;
      }

      if( pChild->bNomatch ){
        pAnd->bNomatch = 1;
      }
    }
  }while( bMatch==0 );

  if( pAnd->bNomatch && pAnd!=pExpr->pRoot ){
    fts5ExprNodeZeroPoslist(pAnd);
  }
  pAnd->iRowid = iLast;
  return SQLITE_OK;
}

static int fts5ExprNodeNext_AND(
  Fts5Expr *pExpr,
  Fts5ExprNode *pNode,
  int bFromValid,
  i64 iFrom
){
  int rc = fts5ExprNodeNext(pExpr, pNode->apChild[0], bFromValid, iFrom);
  if( rc==SQLITE_OK ){
    rc = fts5ExprNodeTest_AND(pExpr, pNode);
  }else{
    pNode->bNomatch = 0;
  }
  return rc;
}

static int fts5ExprNodeTest_NOT(
  Fts5Expr *pExpr,                /* Expression pPhrase belongs to */
  Fts5ExprNode *pNode             /* FTS5_NOT node to advance */
){
  int rc = SQLITE_OK;
  Fts5ExprNode *p1 = pNode->apChild[0];
  Fts5ExprNode *p2 = pNode->apChild[1];
  assert( pNode->nChild==2 );

  while( rc==SQLITE_OK && p1->bEof==0 ){
    int cmp = fts5NodeCompare(pExpr, p1, p2);
    if( cmp>0 ){
      rc = fts5ExprNodeNext(pExpr, p2, 1, p1->iRowid);
      cmp = fts5NodeCompare(pExpr, p1, p2);
    }
    assert( rc!=SQLITE_OK || cmp<=0 );
    if( cmp || p2->bNomatch ) break;
    rc = fts5ExprNodeNext(pExpr, p1, 0, 0);
  }
  pNode->bEof = p1->bEof;
  pNode->bNomatch = p1->bNomatch;
  pNode->iRowid = p1->iRowid;
  if( p1->bEof ){
    fts5ExprNodeZeroPoslist(p2);
  }
  return rc;
}

static int fts5ExprNodeNext_NOT(
  Fts5Expr *pExpr,
  Fts5ExprNode *pNode,
  int bFromValid,
  i64 iFrom
){
  int rc = fts5ExprNodeNext(pExpr, pNode->apChild[0], bFromValid, iFrom);
  if( rc==SQLITE_OK ){
    rc = fts5ExprNodeTest_NOT(pExpr, pNode);
  }
  if( rc!=SQLITE_OK ){
    pNode->bNomatch = 0;
  }
  return rc;
}

/*
** If pNode currently points to a match, this function returns SQLITE_OK
** without modifying it. Otherwise, pNode is advanced until it does point
** to a match or EOF is reached.
*/
static int fts5ExprNodeTest(
  Fts5Expr *pExpr,                /* Expression of which pNode is a part */
  Fts5ExprNode *pNode             /* Expression node to test */
){
  int rc = SQLITE_OK;
  if( pNode->bEof==0 ){
    switch( pNode->eType ){

      case FTS5_STRING: {
        rc = fts5ExprNodeTest_STRING(pExpr, pNode);
        break;
      }

      case FTS5_TERM: {
        rc = fts5ExprNodeTest_TERM(pExpr, pNode);
        break;
      }

      case FTS5_AND: {
        rc = fts5ExprNodeTest_AND(pExpr, pNode);
        break;
      }

      case FTS5_OR: {
        fts5ExprNodeTest_OR(pExpr, pNode);
        break;
      }

      default: assert( pNode->eType==FTS5_NOT ); {
        rc = fts5ExprNodeTest_NOT(pExpr, pNode);
        break;
      }
    }
  }
  return rc;
}


/*
** Set node pNode, which is part of expression pExpr, to point to the first
** match. If there are no matches, set the Node.bEof flag to indicate EOF.
**
** Return an SQLite error code if an error occurs, or SQLITE_OK otherwise.
** It is not an error if there are no matches.
*/
static int fts5ExprNodeFirst(Fts5Expr *pExpr, Fts5ExprNode *pNode){
  int rc = SQLITE_OK;
  pNode->bEof = 0;
  pNode->bNomatch = 0;

  if( Fts5NodeIsString(pNode) ){
    /* Initialize all term iterators in the NEAR object. */
    rc = fts5ExprNearInitAll(pExpr, pNode);
  }else if( pNode->xNext==0 ){
    pNode->bEof = 1;
  }else{
    int i;
    int nEof = 0;
    for(i=0; i<pNode->nChild && rc==SQLITE_OK; i++){
      Fts5ExprNode *pChild = pNode->apChild[i];
      rc = fts5ExprNodeFirst(pExpr, pNode->apChild[i]);
      assert( pChild->bEof==0 || pChild->bEof==1 );
      nEof += pChild->bEof;
    }
    pNode->iRowid = pNode->apChild[0]->iRowid;

    switch( pNode->eType ){
      case FTS5_AND:
        if( nEof>0 ) fts5ExprSetEof(pNode);
        break;

      case FTS5_OR:
        if( pNode->nChild==nEof ) fts5ExprSetEof(pNode);
        break;

      default:
        assert( pNode->eType==FTS5_NOT );
        pNode->bEof = pNode->apChild[0]->bEof;
        break;
    }
  }

  if( rc==SQLITE_OK ){
    rc = fts5ExprNodeTest(pExpr, pNode);
  }
  return rc;
}


/*
** Begin iterating through the set of documents in index pIdx matched by
** the MATCH expression passed as the first argument. If the "bDesc"
** parameter is passed a non-zero value, iteration is in descending rowid
** order. Or, if it is zero, in ascending order.
**
** If iterating in ascending rowid order (bDesc==0), the first document
** visited is that with the smallest rowid that is larger than or equal
** to parameter iFirst. Or, if iterating in ascending order (bDesc==1),
** then the first document visited must have a rowid smaller than or
** equal to iFirst.
**
** Return SQLITE_OK if successful, or an SQLite error code otherwise. It
** is not considered an error if the query does not match any documents.
*/
static int sqlite3Fts5ExprFirst(Fts5Expr *p, Fts5Index *pIdx, i64 iFirst, int bDesc){
  Fts5ExprNode *pRoot = p->pRoot;
  int rc;                         /* Return code */

  p->pIndex = pIdx;
  p->bDesc = bDesc;
  rc = fts5ExprNodeFirst(p, pRoot);

  /* If not at EOF but the current rowid occurs earlier than iFirst in
  ** the iteration order, move to document iFirst or later. */
  if( rc==SQLITE_OK
   && 0==pRoot->bEof
   && fts5RowidCmp(p, pRoot->iRowid, iFirst)<0
  ){
    rc = fts5ExprNodeNext(p, pRoot, 1, iFirst);
  }

  /* If the iterator is not at a real match, skip forward until it is. */
  while( pRoot->bNomatch && rc==SQLITE_OK ){
    assert( pRoot->bEof==0 );
    rc = fts5ExprNodeNext(p, pRoot, 0, 0);
  }
  return rc;
}

/*
** Move to the next document
**
** Return SQLITE_OK if successful, or an SQLite error code otherwise. It
** is not considered an error if the query does not match any documents.
*/
static int sqlite3Fts5ExprNext(Fts5Expr *p, i64 iLast){
  int rc;
  Fts5ExprNode *pRoot = p->pRoot;
  assert( pRoot->bEof==0 && pRoot->bNomatch==0 );
  do {
    rc = fts5ExprNodeNext(p, pRoot, 0, 0);
    assert( pRoot->bNomatch==0 || (rc==SQLITE_OK && pRoot->bEof==0) );
  }while( pRoot->bNomatch );
  if( fts5RowidCmp(p, pRoot->iRowid, iLast)>0 ){
    pRoot->bEof = 1;
  }
  return rc;
}

static int sqlite3Fts5ExprEof(Fts5Expr *p){
  return p->pRoot->bEof;
}

static i64 sqlite3Fts5ExprRowid(Fts5Expr *p){
  return p->pRoot->iRowid;
}

static int fts5ParseStringFromToken(Fts5Token *pToken, char **pz){
  int rc = SQLITE_OK;
  *pz = sqlite3Fts5Strndup(&rc, pToken->p, pToken->n);
  return rc;
}

/*
** Free the phrase object passed as the only argument.
*/
static void fts5ExprPhraseFree(Fts5ExprPhrase *pPhrase){
  if( pPhrase ){
    int i;
    for(i=0; i<pPhrase->nTerm; i++){
      Fts5ExprTerm *pSyn;
      Fts5ExprTerm *pNext;
      Fts5ExprTerm *pTerm = &pPhrase->aTerm[i];
      sqlite3_free(pTerm->pTerm);
      sqlite3Fts5IterClose(pTerm->pIter);
      for(pSyn=pTerm->pSynonym; pSyn; pSyn=pNext){
        pNext = pSyn->pSynonym;
        sqlite3Fts5IterClose(pSyn->pIter);
        fts5BufferFree((Fts5Buffer*)&pSyn[1]);
        sqlite3_free(pSyn);
      }
    }
    if( pPhrase->poslist.nSpace>0 ) fts5BufferFree(&pPhrase->poslist);
    sqlite3_free(pPhrase);
  }
}

/*
** Set the "bFirst" flag on the first token of the phrase passed as the
** only argument.
*/
static void sqlite3Fts5ParseSetCaret(Fts5ExprPhrase *pPhrase){
  if( pPhrase && pPhrase->nTerm ){
    pPhrase->aTerm[0].bFirst = 1;
  }
}

/*
** If argument pNear is NULL, then a new Fts5ExprNearset object is allocated
** and populated with pPhrase. Or, if pNear is not NULL, phrase pPhrase is
** appended to it and the results returned.
**
** If an OOM error occurs, both the pNear and pPhrase objects are freed and
** NULL returned.
*/
static Fts5ExprNearset *sqlite3Fts5ParseNearset(
  Fts5Parse *pParse,              /* Parse context */
  Fts5ExprNearset *pNear,         /* Existing nearset, or NULL */
  Fts5ExprPhrase *pPhrase         /* Recently parsed phrase */
){
  const int SZALLOC = 8;
  Fts5ExprNearset *pRet = 0;

  if( pParse->rc==SQLITE_OK ){
    if( pPhrase==0 ){
      return pNear;
    }
    if( pNear==0 ){
      sqlite3_int64 nByte;
      nByte = sizeof(Fts5ExprNearset) + SZALLOC * sizeof(Fts5ExprPhrase*);
      pRet = sqlite3_malloc64(nByte);
      if( pRet==0 ){
        pParse->rc = SQLITE_NOMEM;
      }else{
        memset(pRet, 0, (size_t)nByte);
      }
    }else if( (pNear->nPhrase % SZALLOC)==0 ){
      int nNew = pNear->nPhrase + SZALLOC;
      sqlite3_int64 nByte;

      nByte = sizeof(Fts5ExprNearset) + nNew * sizeof(Fts5ExprPhrase*);
      pRet = (Fts5ExprNearset*)sqlite3_realloc64(pNear, nByte);
      if( pRet==0 ){
        pParse->rc = SQLITE_NOMEM;
      }
    }else{
      pRet = pNear;
    }
  }

  if( pRet==0 ){
    assert( pParse->rc!=SQLITE_OK );
    sqlite3Fts5ParseNearsetFree(pNear);
    sqlite3Fts5ParsePhraseFree(pPhrase);
  }else{
    if( pRet->nPhrase>0 ){
      Fts5ExprPhrase *pLast = pRet->apPhrase[pRet->nPhrase-1];
      assert( pParse!=0 );
      assert( pParse->apPhrase!=0 );
      assert( pParse->nPhrase>=2 );
      assert( pLast==pParse->apPhrase[pParse->nPhrase-2] );
      if( pPhrase->nTerm==0 ){
        fts5ExprPhraseFree(pPhrase);
        pRet->nPhrase--;
        pParse->nPhrase--;
        pPhrase = pLast;
      }else if( pLast->nTerm==0 ){
        fts5ExprPhraseFree(pLast);
        pParse->apPhrase[pParse->nPhrase-2] = pPhrase;
        pParse->nPhrase--;
        pRet->nPhrase--;
      }
    }
    pRet->apPhrase[pRet->nPhrase++] = pPhrase;
  }
  return pRet;
}
