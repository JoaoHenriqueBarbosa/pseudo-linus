

/*
** All individual term iterators in pPhrase are guaranteed to be valid and
** pointing to the same rowid when this function is called. This function
** checks if the current rowid really is a match, and if so populates
** the pPhrase->poslist buffer accordingly. Output parameter *pbMatch
** is set to true if this is really a match, or false otherwise.
**
** SQLITE_OK is returned if an error occurs, or an SQLite error code
** otherwise. It is not considered an error code if the current rowid is
** not a match.
*/
static int fts5ExprPhraseIsMatch(
  Fts5ExprNode *pNode,            /* Node pPhrase belongs to */
  Fts5ExprPhrase *pPhrase,        /* Phrase object to initialize */
  int *pbMatch                    /* OUT: Set to true if really a match */
){
  Fts5PoslistWriter writer = {0};
  Fts5PoslistReader aStatic[4];
  Fts5PoslistReader *aIter = aStatic;
  int i;
  int rc = SQLITE_OK;
  int bFirst = pPhrase->aTerm[0].bFirst;

  fts5BufferZero(&pPhrase->poslist);

  /* If the aStatic[] array is not large enough, allocate a large array
  ** using sqlite3_malloc(). This approach could be improved upon. */
  if( pPhrase->nTerm>ArraySize(aStatic) ){
    sqlite3_int64 nByte = sizeof(Fts5PoslistReader) * pPhrase->nTerm;
    aIter = (Fts5PoslistReader*)sqlite3_malloc64(nByte);
    if( !aIter ) return SQLITE_NOMEM;
  }
  memset(aIter, 0, sizeof(Fts5PoslistReader) * pPhrase->nTerm);

  /* Initialize a term iterator for each term in the phrase */
  for(i=0; i<pPhrase->nTerm; i++){
    Fts5ExprTerm *pTerm = &pPhrase->aTerm[i];
    int n = 0;
    int bFlag = 0;
    u8 *a = 0;
    if( pTerm->pSynonym ){
      Fts5Buffer buf = {0, 0, 0};
      rc = fts5ExprSynonymList(pTerm, pNode->iRowid, &buf, &a, &n);
      if( rc ){
        sqlite3_free(a);
        goto ismatch_out;
      }
      if( a==buf.p ) bFlag = 1;
    }else{
      a = (u8*)pTerm->pIter->pData;
      n = pTerm->pIter->nData;
    }
    sqlite3Fts5PoslistReaderInit(a, n, &aIter[i]);
    aIter[i].bFlag = (u8)bFlag;
    if( aIter[i].bEof ) goto ismatch_out;
  }

  while( 1 ){
    int bMatch;
    i64 iPos = aIter[0].iPos;
    do {
      bMatch = 1;
      for(i=0; i<pPhrase->nTerm; i++){
        Fts5PoslistReader *pPos = &aIter[i];
        i64 iAdj = iPos + i;
        if( pPos->iPos!=iAdj ){
          bMatch = 0;
          while( pPos->iPos<iAdj ){
            if( sqlite3Fts5PoslistReaderNext(pPos) ) goto ismatch_out;
          }
          if( pPos->iPos>iAdj ) iPos = pPos->iPos-i;
        }
      }
    }while( bMatch==0 );

    /* Append position iPos to the output */
    if( bFirst==0 || FTS5_POS2OFFSET(iPos)==0 ){
      rc = sqlite3Fts5PoslistWriterAppend(&pPhrase->poslist, &writer, iPos);
      if( rc!=SQLITE_OK ) goto ismatch_out;
    }

    for(i=0; i<pPhrase->nTerm; i++){
      if( sqlite3Fts5PoslistReaderNext(&aIter[i]) ) goto ismatch_out;
    }
  }

 ismatch_out:
  *pbMatch = (pPhrase->poslist.n>0);
  for(i=0; i<pPhrase->nTerm; i++){
    if( aIter[i].bFlag ) sqlite3_free((u8*)aIter[i].a);
  }
  if( aIter!=aStatic ) sqlite3_free(aIter);
  return rc;
}

typedef struct Fts5LookaheadReader Fts5LookaheadReader;
struct Fts5LookaheadReader {
  const u8 *a;                    /* Buffer containing position list */
  int n;                          /* Size of buffer a[] in bytes */
  int i;                          /* Current offset in position list */
  i64 iPos;                       /* Current position */
  i64 iLookahead;                 /* Next position */
};

#define FTS5_LOOKAHEAD_EOF (((i64)1) << 62)

static int fts5LookaheadReaderNext(Fts5LookaheadReader *p){
  p->iPos = p->iLookahead;
  if( sqlite3Fts5PoslistNext64(p->a, p->n, &p->i, &p->iLookahead) ){
    p->iLookahead = FTS5_LOOKAHEAD_EOF;
  }
  return (p->iPos==FTS5_LOOKAHEAD_EOF);
}

static int fts5LookaheadReaderInit(
  const u8 *a, int n,             /* Buffer to read position list from */
  Fts5LookaheadReader *p          /* Iterator object to initialize */
){
  memset(p, 0, sizeof(Fts5LookaheadReader));
  p->a = a;
  p->n = n;
  fts5LookaheadReaderNext(p);
  return fts5LookaheadReaderNext(p);
}

typedef struct Fts5NearTrimmer Fts5NearTrimmer;
struct Fts5NearTrimmer {
  Fts5LookaheadReader reader;     /* Input iterator */
  Fts5PoslistWriter writer;       /* Writer context */
  Fts5Buffer *pOut;               /* Output poslist */
};

/*
** The near-set object passed as the first argument contains more than
** one phrase. All phrases currently point to the same row. The
** Fts5ExprPhrase.poslist buffers are populated accordingly. This function
** tests if the current row contains instances of each phrase sufficiently
** close together to meet the NEAR constraint. Non-zero is returned if it
** does, or zero otherwise.
**
** If in/out parameter (*pRc) is set to other than SQLITE_OK when this
** function is called, it is a no-op. Or, if an error (e.g. SQLITE_NOMEM)
** occurs within this function (*pRc) is set accordingly before returning.
** The return value is undefined in both these cases.
**
** If no error occurs and non-zero (a match) is returned, the position-list
** of each phrase object is edited to contain only those entries that
** meet the constraint before returning.
*/
static int fts5ExprNearIsMatch(int *pRc, Fts5ExprNearset *pNear){
  Fts5NearTrimmer aStatic[4];
  Fts5NearTrimmer *a = aStatic;
  Fts5ExprPhrase **apPhrase = pNear->apPhrase;

  int i;
  int rc = *pRc;
  int bMatch;

  assert( pNear->nPhrase>1 );

  /* If the aStatic[] array is not large enough, allocate a large array
  ** using sqlite3_malloc(). This approach could be improved upon. */
  if( pNear->nPhrase>ArraySize(aStatic) ){
    sqlite3_int64 nByte = sizeof(Fts5NearTrimmer) * pNear->nPhrase;
    a = (Fts5NearTrimmer*)sqlite3Fts5MallocZero(&rc, nByte);
  }else{
    memset(aStatic, 0, sizeof(aStatic));
  }
  if( rc!=SQLITE_OK ){
    *pRc = rc;
    return 0;
  }

  /* Initialize a lookahead iterator for each phrase. After passing the
  ** buffer and buffer size to the lookaside-reader init function, zero
  ** the phrase poslist buffer. The new poslist for the phrase (containing
  ** the same entries as the original with some entries removed on account
  ** of the NEAR constraint) is written over the original even as it is
  ** being read. This is safe as the entries for the new poslist are a
  ** subset of the old, so it is not possible for data yet to be read to
  ** be overwritten.  */
  for(i=0; i<pNear->nPhrase; i++){
    Fts5Buffer *pPoslist = &apPhrase[i]->poslist;
    fts5LookaheadReaderInit(pPoslist->p, pPoslist->n, &a[i].reader);
    pPoslist->n = 0;
    a[i].pOut = pPoslist;
  }

  while( 1 ){
    int iAdv;
    i64 iMin;
    i64 iMax;

    /* This block advances the phrase iterators until they point to a set of
    ** entries that together comprise a match.  */
    iMax = a[0].reader.iPos;
    do {
      bMatch = 1;
      for(i=0; i<pNear->nPhrase; i++){
        Fts5LookaheadReader *pPos = &a[i].reader;
        iMin = iMax - pNear->apPhrase[i]->nTerm - pNear->nNear;
        if( pPos->iPos<iMin || pPos->iPos>iMax ){
          bMatch = 0;
          while( pPos->iPos<iMin ){
            if( fts5LookaheadReaderNext(pPos) ) goto ismatch_out;
          }
          if( pPos->iPos>iMax ) iMax = pPos->iPos;
        }
      }
    }while( bMatch==0 );

    /* Add an entry to each output position list */
    for(i=0; i<pNear->nPhrase; i++){
      i64 iPos = a[i].reader.iPos;
      Fts5PoslistWriter *pWriter = &a[i].writer;
      if( a[i].pOut->n==0 || iPos!=pWriter->iPrev ){
        sqlite3Fts5PoslistWriterAppend(a[i].pOut, pWriter, iPos);
      }
    }

    iAdv = 0;
    iMin = a[0].reader.iLookahead;
    for(i=0; i<pNear->nPhrase; i++){
      if( a[i].reader.iLookahead < iMin ){
        iMin = a[i].reader.iLookahead;
        iAdv = i;
      }
    }
    if( fts5LookaheadReaderNext(&a[iAdv].reader) ) goto ismatch_out;
  }

  ismatch_out: {
    int bRet = a[0].pOut->n>0;
    *pRc = rc;
    if( a!=aStatic ) sqlite3_free(a);
    return bRet;
  }
}

/*
** Advance iterator pIter until it points to a value equal to or laster
** than the initial value of *piLast. If this means the iterator points
** to a value laster than *piLast, update *piLast to the new lastest value.
**
** If the iterator reaches EOF, set *pbEof to true before returning. If
** an error occurs, set *pRc to an error code. If either *pbEof or *pRc
** are set, return a non-zero value. Otherwise, return zero.
*/
static int fts5ExprAdvanceto(
  Fts5IndexIter *pIter,           /* Iterator to advance */
  int bDesc,                      /* True if iterator is "rowid DESC" */
  i64 *piLast,                    /* IN/OUT: Lastest rowid seen so far */
  int *pRc,                       /* OUT: Error code */
  int *pbEof                      /* OUT: Set to true if EOF */
){
  i64 iLast = *piLast;
  i64 iRowid;

  iRowid = pIter->iRowid;
  if( (bDesc==0 && iLast>iRowid) || (bDesc && iLast<iRowid) ){
    int rc = sqlite3Fts5IterNextFrom(pIter, iLast);
    if( rc || sqlite3Fts5IterEof(pIter) ){
      *pRc = rc;
      *pbEof = 1;
      return 1;
    }
    iRowid = pIter->iRowid;
    assert( (bDesc==0 && iRowid>=iLast) || (bDesc==1 && iRowid<=iLast) );
  }
  *piLast = iRowid;

  return 0;
}

static int fts5ExprSynonymAdvanceto(
  Fts5ExprTerm *pTerm,            /* Term iterator to advance */
  int bDesc,                      /* True if iterator is "rowid DESC" */
  i64 *piLast,                    /* IN/OUT: Lastest rowid seen so far */
  int *pRc                        /* OUT: Error code */
){
  int rc = SQLITE_OK;
  i64 iLast = *piLast;
  Fts5ExprTerm *p;
  int bEof = 0;

  for(p=pTerm; rc==SQLITE_OK && p; p=p->pSynonym){
    if( sqlite3Fts5IterEof(p->pIter)==0 ){
      i64 iRowid = p->pIter->iRowid;
      if( (bDesc==0 && iLast>iRowid) || (bDesc && iLast<iRowid) ){
        rc = sqlite3Fts5IterNextFrom(p->pIter, iLast);
      }
    }
  }

  if( rc!=SQLITE_OK ){
    *pRc = rc;
    bEof = 1;
  }else{
    *piLast = fts5ExprSynonymRowid(pTerm, bDesc, &bEof);
  }
  return bEof;
}


static int fts5ExprNearTest(
  int *pRc,
  Fts5Expr *pExpr,                /* Expression that pNear is a part of */
  Fts5ExprNode *pNode             /* The "NEAR" node (FTS5_STRING) */
){
  Fts5ExprNearset *pNear = pNode->pNear;
  int rc = *pRc;

  if( pExpr->pConfig->eDetail!=FTS5_DETAIL_FULL ){
    Fts5ExprTerm *pTerm;
    Fts5ExprPhrase *pPhrase = pNear->apPhrase[0];
    pPhrase->poslist.n = 0;
    for(pTerm=&pPhrase->aTerm[0]; pTerm; pTerm=pTerm->pSynonym){
      Fts5IndexIter *pIter = pTerm->pIter;
      if( sqlite3Fts5IterEof(pIter)==0 ){
        if( pIter->iRowid==pNode->iRowid && pIter->nData>0 ){
          pPhrase->poslist.n = 1;
        }
      }
    }
    return pPhrase->poslist.n;
  }else{
    int i;

    /* Check that each phrase in the nearset matches the current row.
    ** Populate the pPhrase->poslist buffers at the same time. If any
    ** phrase is not a match, break out of the loop early.  */
    for(i=0; rc==SQLITE_OK && i<pNear->nPhrase; i++){
      Fts5ExprPhrase *pPhrase = pNear->apPhrase[i];
      if( pPhrase->nTerm>1 || pPhrase->aTerm[0].pSynonym
       || pNear->pColset || pPhrase->aTerm[0].bFirst
      ){
        int bMatch = 0;
        rc = fts5ExprPhraseIsMatch(pNode, pPhrase, &bMatch);
        if( bMatch==0 ) break;
      }else{
        Fts5IndexIter *pIter = pPhrase->aTerm[0].pIter;
        fts5BufferSet(&rc, &pPhrase->poslist, pIter->nData, pIter->pData);
      }
    }

    *pRc = rc;
    if( i==pNear->nPhrase && (i==1 || fts5ExprNearIsMatch(pRc, pNear)) ){
      return 1;
    }
    return 0;
  }
}
