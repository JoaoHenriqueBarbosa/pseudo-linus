
static sqlite3_stmt *fts5IdxSelectStmt(Fts5Index *p){
  if( p->pIdxSelect==0 ){
    Fts5Config *pConfig = p->pConfig;
    fts5IndexPrepareStmt(p, &p->pIdxSelect, sqlite3_mprintf(
          "SELECT pgno FROM '%q'.'%q_idx' WHERE "
          "segid=? AND term<=? ORDER BY term DESC LIMIT 1",
          pConfig->zDb, pConfig->zName
    ));
  }
  return p->pIdxSelect;
}

/*
** Initialize the object pIter to point to term pTerm/nTerm within segment
** pSeg. If there is no such term in the index, the iterator is set to EOF.
**
** If an error occurs, Fts5Index.rc is set to an appropriate error code. If
** an error has already occurred when this function is called, it is a no-op.
*/
static void fts5SegIterSeekInit(
  Fts5Index *p,                   /* FTS5 backend */
  const u8 *pTerm, int nTerm,     /* Term to seek to */
  int flags,                      /* Mask of FTS5INDEX_XXX flags */
  Fts5StructureSegment *pSeg,     /* Description of segment */
  Fts5SegIter *pIter              /* Object to populate */
){
  int iPg = 1;
  int bGe = (flags & FTS5INDEX_QUERY_SCAN);
  int bDlidx = 0;                 /* True if there is a doclist-index */
  sqlite3_stmt *pIdxSelect = 0;

  assert( bGe==0 || (flags & FTS5INDEX_QUERY_DESC)==0 );
  assert( pTerm && nTerm );
  memset(pIter, 0, sizeof(*pIter));
  pIter->pSeg = pSeg;

  /* This block sets stack variable iPg to the leaf page number that may
  ** contain term (pTerm/nTerm), if it is present in the segment. */
  pIdxSelect = fts5IdxSelectStmt(p);
  if( p->rc ) return;
  sqlite3_bind_int(pIdxSelect, 1, pSeg->iSegid);
  sqlite3_bind_blob(pIdxSelect, 2, pTerm, nTerm, SQLITE_STATIC);
  if( SQLITE_ROW==sqlite3_step(pIdxSelect) ){
    i64 val = sqlite3_column_int(pIdxSelect, 0);
    iPg = (int)(val>>1);
    bDlidx = (val & 0x0001);
  }
  p->rc = sqlite3_reset(pIdxSelect);
  sqlite3_bind_null(pIdxSelect, 2);

  if( iPg<pSeg->pgnoFirst ){
    iPg = pSeg->pgnoFirst;
    bDlidx = 0;
  }

  pIter->iLeafPgno = iPg - 1;
  fts5SegIterNextPage(p, pIter);

  if( pIter->pLeaf ){
    fts5LeafSeek(p, bGe, pIter, pTerm, nTerm);
  }

  if( p->rc==SQLITE_OK && (bGe==0 || (flags & FTS5INDEX_QUERY_SCANONETERM)) ){
    pIter->flags |= FTS5_SEGITER_ONETERM;
    if( pIter->pLeaf ){
      if( flags & FTS5INDEX_QUERY_DESC ){
        pIter->flags |= FTS5_SEGITER_REVERSE;
      }
      if( bDlidx ){
        fts5SegIterLoadDlidx(p, pIter);
      }
      if( flags & FTS5INDEX_QUERY_DESC ){
        fts5SegIterReverse(p, pIter);
      }
    }
  }

  fts5SegIterSetNext(p, pIter);
  if( 0==(flags & FTS5INDEX_QUERY_SCANONETERM) ){
    fts5SegIterAllocTombstone(p, pIter);
  }

  /* Either:
  **
  **   1) an error has occurred, or
  **   2) the iterator points to EOF, or
  **   3) the iterator points to an entry with term (pTerm/nTerm), or
  **   4) the FTS5INDEX_QUERY_SCAN flag was set and the iterator points
  **      to an entry with a term greater than or equal to (pTerm/nTerm).
  */
  assert_nc( p->rc!=SQLITE_OK                                       /* 1 */
   || pIter->pLeaf==0                                               /* 2 */
   || fts5BufferCompareBlob(&pIter->term, pTerm, nTerm)==0          /* 3 */
   || (bGe && fts5BufferCompareBlob(&pIter->term, pTerm, nTerm)>0)  /* 4 */
  );
}


/*
** SQL used by fts5SegIterNextInit() to find the page to open.
*/
static sqlite3_stmt *fts5IdxNextStmt(Fts5Index *p){
  if( p->pIdxNextSelect==0 ){
    Fts5Config *pConfig = p->pConfig;
    fts5IndexPrepareStmt(p, &p->pIdxNextSelect, sqlite3_mprintf(
          "SELECT pgno FROM '%q'.'%q_idx' WHERE "
          "segid=? AND term>? ORDER BY term ASC LIMIT 1",
          pConfig->zDb, pConfig->zName
    ));

  }
  return p->pIdxNextSelect;
}

/*
** This is similar to fts5SegIterSeekInit(), except that it initializes
** the segment iterator to point to the first term following the page
** with pToken/nToken on it.
*/
static void fts5SegIterNextInit(
  Fts5Index *p,
  const char *pTerm, int nTerm,
  Fts5StructureSegment *pSeg,     /* Description of segment */
  Fts5SegIter *pIter              /* Object to populate */
){
  int iPg = -1;                   /* Page of segment to open */
  int bDlidx = 0;
  sqlite3_stmt *pSel = 0;         /* SELECT to find iPg */

  pSel = fts5IdxNextStmt(p);
  if( pSel ){
    assert( p->rc==SQLITE_OK );
    sqlite3_bind_int(pSel, 1, pSeg->iSegid);
    sqlite3_bind_blob(pSel, 2, pTerm, nTerm, SQLITE_STATIC);

    if( sqlite3_step(pSel)==SQLITE_ROW ){
      i64 val = sqlite3_column_int64(pSel, 0);
      iPg = (int)(val>>1);
      bDlidx = (val & 0x0001);
    }
    p->rc = sqlite3_reset(pSel);
    sqlite3_bind_null(pSel, 2);
    if( p->rc ) return;
  }

  memset(pIter, 0, sizeof(*pIter));
  pIter->pSeg = pSeg;
  pIter->flags |= FTS5_SEGITER_ONETERM;
  if( iPg>=0 ){
    pIter->iLeafPgno = iPg - 1;
    fts5SegIterNextPage(p, pIter);
    fts5SegIterSetNext(p, pIter);
  }
  if( pIter->pLeaf ){
    const u8 *a = pIter->pLeaf->p;
    int iTermOff = 0;

    pIter->iPgidxOff = pIter->pLeaf->szLeaf;
    pIter->iPgidxOff += fts5GetVarint32(&a[pIter->iPgidxOff], iTermOff);
    pIter->iLeafOffset = iTermOff;
    fts5SegIterLoadTerm(p, pIter, 0);
    fts5SegIterLoadNPos(p, pIter);
    if( bDlidx ) fts5SegIterLoadDlidx(p, pIter);

    assert( p->rc!=SQLITE_OK ||
        fts5BufferCompareBlob(&pIter->term, (const u8*)pTerm, nTerm)>0
    );
  }
}

/*
** Initialize the object pIter to point to term pTerm/nTerm within the
** in-memory hash table. If there is no such term in the hash-table, the
** iterator is set to EOF.
**
** If an error occurs, Fts5Index.rc is set to an appropriate error code. If
** an error has already occurred when this function is called, it is a no-op.
*/
static void fts5SegIterHashInit(
  Fts5Index *p,                   /* FTS5 backend */
  const u8 *pTerm, int nTerm,     /* Term to seek to */
  int flags,                      /* Mask of FTS5INDEX_XXX flags */
  Fts5SegIter *pIter              /* Object to populate */
){
  int nList = 0;
  const u8 *z = 0;
  int n = 0;
  Fts5Data *pLeaf = 0;

  assert( p->pHash );
  assert( p->rc==SQLITE_OK );

  if( pTerm==0 || (flags & FTS5INDEX_QUERY_SCAN) ){
    const u8 *pList = 0;

    p->rc = sqlite3Fts5HashScanInit(p->pHash, (const char*)pTerm, nTerm);
    sqlite3Fts5HashScanEntry(p->pHash, (const char**)&z, &n, &pList, &nList);
    if( pList ){
      pLeaf = fts5IdxMalloc(p, sizeof(Fts5Data));
      if( pLeaf ){
        pLeaf->p = (u8*)pList;
      }
    }

    /* The call to sqlite3Fts5HashScanInit() causes the hash table to
    ** fill the size field of all existing position lists. This means they
    ** can no longer be appended to. Since the only scenario in which they
    ** can be appended to is if the previous operation on this table was
    ** a DELETE, by clearing the Fts5Index.bDelete flag we can avoid this
    ** possibility altogether.  */
    p->bDelete = 0;
  }else{
    p->rc = sqlite3Fts5HashQuery(p->pHash, sizeof(Fts5Data),
        (const char*)pTerm, nTerm, (void**)&pLeaf, &nList
    );
    if( pLeaf ){
      pLeaf->p = (u8*)&pLeaf[1];
    }
    z = pTerm;
    n = nTerm;
    pIter->flags |= FTS5_SEGITER_ONETERM;
  }

  if( pLeaf ){
    sqlite3Fts5BufferSet(&p->rc, &pIter->term, n, z);
    pLeaf->nn = pLeaf->szLeaf = nList;
    pIter->pLeaf = pLeaf;
    pIter->iLeafOffset = fts5GetVarint(pLeaf->p, (u64*)&pIter->iRowid);
    pIter->iEndofDoclist = pLeaf->nn;

    if( flags & FTS5INDEX_QUERY_DESC ){
      pIter->flags |= FTS5_SEGITER_REVERSE;
      fts5SegIterReverseInitPage(p, pIter);
    }else{
      fts5SegIterLoadNPos(p, pIter);
    }
  }

  fts5SegIterSetNext(p, pIter);
}

/*
** Array ap[] contains n elements. Release each of these elements using
** fts5DataRelease(). Then free the array itself using sqlite3_free().
*/
static void fts5IndexFreeArray(Fts5Data **ap, int n){
  if( ap ){
    int ii;
    for(ii=0; ii<n; ii++){
      fts5DataRelease(ap[ii]);
    }
    sqlite3_free(ap);
  }
}

/*
** Decrement the ref-count of the object passed as the only argument. If it
** reaches 0, free it and its contents.
*/
static void fts5TombstoneArrayDelete(Fts5TombstoneArray *p){
  if( p ){
    p->nRef--;
    if( p->nRef<=0 ){
      int ii;
      for(ii=0; ii<p->nTombstone; ii++){
        fts5DataRelease(p->apTombstone[ii]);
      }
      sqlite3_free(p);
    }
  }
}

/*
** Zero the iterator passed as the only argument.
*/
static void fts5SegIterClear(Fts5SegIter *pIter){
  fts5BufferFree(&pIter->term);
  fts5DataRelease(pIter->pLeaf);
  fts5DataRelease(pIter->pNextLeaf);
  fts5TombstoneArrayDelete(pIter->pTombArray);
  fts5DlidxIterFree(pIter->pDlidx);
  sqlite3_free(pIter->aRowidOffset);
  memset(pIter, 0, sizeof(Fts5SegIter));
}

#ifdef SQLITE_DEBUG

/*
** This function is used as part of the big assert() procedure implemented by
** fts5AssertMultiIterSetup(). It ensures that the result currently stored
** in *pRes is the correct result of comparing the current positions of the
** two iterators.
*/
static void fts5AssertComparisonResult(
  Fts5Iter *pIter,
  Fts5SegIter *p1,
  Fts5SegIter *p2,
  Fts5CResult *pRes
){
  int i1 = p1 - pIter->aSeg;
  int i2 = p2 - pIter->aSeg;

  if( p1->pLeaf || p2->pLeaf ){
    if( p1->pLeaf==0 ){
      assert( pRes->iFirst==i2 );
    }else if( p2->pLeaf==0 ){
      assert( pRes->iFirst==i1 );
    }else{
      int nMin = MIN(p1->term.n, p2->term.n);
      int res = fts5Memcmp(p1->term.p, p2->term.p, nMin);
      if( res==0 ) res = p1->term.n - p2->term.n;

      if( res==0 ){
        assert( pRes->bTermEq==1 );
        assert( p1->iRowid!=p2->iRowid );
        res = ((p1->iRowid > p2->iRowid)==pIter->bRev) ? -1 : 1;
      }else{
        assert( pRes->bTermEq==0 );
      }

      if( res<0 ){
        assert( pRes->iFirst==i1 );
      }else{
        assert( pRes->iFirst==i2 );
      }
    }
  }
}

/*
** This function is a no-op unless SQLITE_DEBUG is defined when this module
** is compiled. In that case, this function is essentially an assert()
** statement used to verify that the contents of the pIter->aFirst[] array
** are correct.
*/
static void fts5AssertMultiIterSetup(Fts5Index *p, Fts5Iter *pIter){
  if( p->rc==SQLITE_OK ){
    Fts5SegIter *pFirst = &pIter->aSeg[ pIter->aFirst[1].iFirst ];
    int i;

    assert( (pFirst->pLeaf==0)==pIter->base.bEof );

    /* Check that pIter->iSwitchRowid is set correctly. */
    for(i=0; i<pIter->nSeg; i++){
      Fts5SegIter *p1 = &pIter->aSeg[i];
      assert( p1==pFirst
           || p1->pLeaf==0
           || fts5BufferCompare(&pFirst->term, &p1->term)
           || p1->iRowid==pIter->iSwitchRowid
           || (p1->iRowid<pIter->iSwitchRowid)==pIter->bRev
      );
    }

    for(i=0; i<pIter->nSeg; i+=2){
      Fts5SegIter *p1 = &pIter->aSeg[i];
      Fts5SegIter *p2 = &pIter->aSeg[i+1];
      Fts5CResult *pRes = &pIter->aFirst[(pIter->nSeg + i) / 2];
      fts5AssertComparisonResult(pIter, p1, p2, pRes);
    }

    for(i=1; i<(pIter->nSeg / 2); i+=2){
      Fts5SegIter *p1 = &pIter->aSeg[ pIter->aFirst[i*2].iFirst ];
      Fts5SegIter *p2 = &pIter->aSeg[ pIter->aFirst[i*2+1].iFirst ];
      Fts5CResult *pRes = &pIter->aFirst[i];
      fts5AssertComparisonResult(pIter, p1, p2, pRes);
    }
  }
}
