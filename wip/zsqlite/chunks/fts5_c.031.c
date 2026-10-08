#else
# define fts5AssertMultiIterSetup(x,y)
#endif

/*
** Do the comparison necessary to populate pIter->aFirst[iOut].
**
** If the returned value is non-zero, then it is the index of an entry
** in the pIter->aSeg[] array that is (a) not at EOF, and (b) pointing
** to a key that is a duplicate of another, higher priority,
** segment-iterator in the pSeg->aSeg[] array.
*/
static int fts5MultiIterDoCompare(Fts5Iter *pIter, int iOut){
  int i1;                         /* Index of left-hand Fts5SegIter */
  int i2;                         /* Index of right-hand Fts5SegIter */
  int iRes;
  Fts5SegIter *p1;                /* Left-hand Fts5SegIter */
  Fts5SegIter *p2;                /* Right-hand Fts5SegIter */
  Fts5CResult *pRes = &pIter->aFirst[iOut];

  assert( iOut<pIter->nSeg && iOut>0 );
  assert( pIter->bRev==0 || pIter->bRev==1 );

  if( iOut>=(pIter->nSeg/2) ){
    i1 = (iOut - pIter->nSeg/2) * 2;
    i2 = i1 + 1;
  }else{
    i1 = pIter->aFirst[iOut*2].iFirst;
    i2 = pIter->aFirst[iOut*2+1].iFirst;
  }
  p1 = &pIter->aSeg[i1];
  p2 = &pIter->aSeg[i2];

  pRes->bTermEq = 0;
  if( p1->pLeaf==0 ){           /* If p1 is at EOF */
    iRes = i2;
  }else if( p2->pLeaf==0 ){     /* If p2 is at EOF */
    iRes = i1;
  }else{
    int res = fts5BufferCompare(&p1->term, &p2->term);
    if( res==0 ){
      assert_nc( i2>i1 );
      assert_nc( i2!=0 );
      pRes->bTermEq = 1;
      if( p1->iRowid==p2->iRowid ){
        return i2;
      }
      res = ((p1->iRowid > p2->iRowid)==pIter->bRev) ? -1 : +1;
    }
    assert( res!=0 );
    if( res<0 ){
      iRes = i1;
    }else{
      iRes = i2;
    }
  }

  pRes->iFirst = (u16)iRes;
  return 0;
}

/*
** Move the seg-iter so that it points to the first rowid on page iLeafPgno.
** It is an error if leaf iLeafPgno does not exist. Unless the db is
** a 'secure-delete' db, if it contains no rowids then this is also an error.
*/
static void fts5SegIterGotoPage(
  Fts5Index *p,                   /* FTS5 backend object */
  Fts5SegIter *pIter,             /* Iterator to advance */
  int iLeafPgno
){
  assert( iLeafPgno>pIter->iLeafPgno );

  if( iLeafPgno>pIter->pSeg->pgnoLast ){
    p->rc = FTS5_CORRUPT;
  }else{
    fts5DataRelease(pIter->pNextLeaf);
    pIter->pNextLeaf = 0;
    pIter->iLeafPgno = iLeafPgno-1;

    while( p->rc==SQLITE_OK ){
      int iOff;
      fts5SegIterNextPage(p, pIter);
      if( pIter->pLeaf==0 ) break;
      iOff = fts5LeafFirstRowidOff(pIter->pLeaf);
      if( iOff>0 ){
        u8 *a = pIter->pLeaf->p;
        int n = pIter->pLeaf->szLeaf;
        if( iOff<4 || iOff>=n ){
          p->rc = FTS5_CORRUPT;
        }else{
          iOff += fts5GetVarint(&a[iOff], (u64*)&pIter->iRowid);
          pIter->iLeafOffset = iOff;
          fts5SegIterLoadNPos(p, pIter);
        }
        break;
      }
    }
  }
}

/*
** Advance the iterator passed as the second argument until it is at or
** past rowid iFrom. Regardless of the value of iFrom, the iterator is
** always advanced at least once.
*/
static void fts5SegIterNextFrom(
  Fts5Index *p,                   /* FTS5 backend object */
  Fts5SegIter *pIter,             /* Iterator to advance */
  i64 iMatch                      /* Advance iterator at least this far */
){
  int bRev = (pIter->flags & FTS5_SEGITER_REVERSE);
  Fts5DlidxIter *pDlidx = pIter->pDlidx;
  int iLeafPgno = pIter->iLeafPgno;
  int bMove = 1;

  assert( pIter->flags & FTS5_SEGITER_ONETERM );
  assert( pIter->pDlidx );
  assert( pIter->pLeaf );

  if( bRev==0 ){
    while( !fts5DlidxIterEof(p, pDlidx) && iMatch>fts5DlidxIterRowid(pDlidx) ){
      iLeafPgno = fts5DlidxIterPgno(pDlidx);
      fts5DlidxIterNext(p, pDlidx);
    }
    assert_nc( iLeafPgno>=pIter->iLeafPgno || p->rc );
    if( iLeafPgno>pIter->iLeafPgno ){
      fts5SegIterGotoPage(p, pIter, iLeafPgno);
      bMove = 0;
    }
  }else{
    assert( pIter->pNextLeaf==0 );
    assert( iMatch<pIter->iRowid );
    while( !fts5DlidxIterEof(p, pDlidx) && iMatch<fts5DlidxIterRowid(pDlidx) ){
      fts5DlidxIterPrev(p, pDlidx);
    }
    iLeafPgno = fts5DlidxIterPgno(pDlidx);

    assert( fts5DlidxIterEof(p, pDlidx) || iLeafPgno<=pIter->iLeafPgno );

    if( iLeafPgno<pIter->iLeafPgno ){
      pIter->iLeafPgno = iLeafPgno+1;
      fts5SegIterReverseNewPage(p, pIter);
      bMove = 0;
    }
  }

  do{
    if( bMove && p->rc==SQLITE_OK ) pIter->xNext(p, pIter, 0);
    if( pIter->pLeaf==0 ) break;
    if( bRev==0 && pIter->iRowid>=iMatch ) break;
    if( bRev!=0 && pIter->iRowid<=iMatch ) break;
    bMove = 1;
  }while( p->rc==SQLITE_OK );
}

/*
** Free the iterator object passed as the second argument.
*/
static void fts5MultiIterFree(Fts5Iter *pIter){
  if( pIter ){
    int i;
    for(i=0; i<pIter->nSeg; i++){
      fts5SegIterClear(&pIter->aSeg[i]);
    }
    fts5BufferFree(&pIter->poslist);
    sqlite3_free(pIter);
  }
}

static void fts5MultiIterAdvanced(
  Fts5Index *p,                   /* FTS5 backend to iterate within */
  Fts5Iter *pIter,                /* Iterator to update aFirst[] array for */
  int iChanged,                   /* Index of sub-iterator just advanced */
  int iMinset                     /* Minimum entry in aFirst[] to set */
){
  int i;
  for(i=(pIter->nSeg+iChanged)/2; i>=iMinset && p->rc==SQLITE_OK; i=i/2){
    int iEq;
    if( (iEq = fts5MultiIterDoCompare(pIter, i)) ){
      Fts5SegIter *pSeg = &pIter->aSeg[iEq];
      assert( p->rc==SQLITE_OK );
      pSeg->xNext(p, pSeg, 0);
      i = pIter->nSeg + iEq;
    }
  }
}

/*
** Sub-iterator iChanged of iterator pIter has just been advanced. It still
** points to the same term though - just a different rowid. This function
** attempts to update the contents of the pIter->aFirst[] accordingly.
** If it does so successfully, 0 is returned. Otherwise 1.
**
** If non-zero is returned, the caller should call fts5MultiIterAdvanced()
** on the iterator instead. That function does the same as this one, except
** that it deals with more complicated cases as well.
*/
static int fts5MultiIterAdvanceRowid(
  Fts5Iter *pIter,                /* Iterator to update aFirst[] array for */
  int iChanged,                   /* Index of sub-iterator just advanced */
  Fts5SegIter **ppFirst
){
  Fts5SegIter *pNew = &pIter->aSeg[iChanged];

  if( pNew->iRowid==pIter->iSwitchRowid
   || (pNew->iRowid<pIter->iSwitchRowid)==pIter->bRev
  ){
    int i;
    Fts5SegIter *pOther = &pIter->aSeg[iChanged ^ 0x0001];
    pIter->iSwitchRowid = pIter->bRev ? SMALLEST_INT64 : LARGEST_INT64;
    for(i=(pIter->nSeg+iChanged)/2; 1; i=i/2){
      Fts5CResult *pRes = &pIter->aFirst[i];

      assert( pNew->pLeaf );
      assert( pRes->bTermEq==0 || pOther->pLeaf );

      if( pRes->bTermEq ){
        if( pNew->iRowid==pOther->iRowid ){
          return 1;
        }else if( (pOther->iRowid>pNew->iRowid)==pIter->bRev ){
          pIter->iSwitchRowid = pOther->iRowid;
          pNew = pOther;
        }else if( (pOther->iRowid>pIter->iSwitchRowid)==pIter->bRev ){
          pIter->iSwitchRowid = pOther->iRowid;
        }
      }
      pRes->iFirst = (u16)(pNew - pIter->aSeg);
      if( i==1 ) break;

      pOther = &pIter->aSeg[ pIter->aFirst[i ^ 0x0001].iFirst ];
    }
  }

  *ppFirst = pNew;
  return 0;
}

/*
** Set the pIter->bEof variable based on the state of the sub-iterators.
*/
static void fts5MultiIterSetEof(Fts5Iter *pIter){
  Fts5SegIter *pSeg = &pIter->aSeg[ pIter->aFirst[1].iFirst ];
  pIter->base.bEof = pSeg->pLeaf==0;
  pIter->iSwitchRowid = pSeg->iRowid;
}

/*
** The argument to this macro must be an Fts5Data structure containing a
** tombstone hash page. This macro returns the key-size of the hash-page.
*/
#define TOMBSTONE_KEYSIZE(pPg) (pPg->p[0]==4 ? 4 : 8)

#define TOMBSTONE_NSLOT(pPg)   \
  ((pPg->nn > 16) ? ((pPg->nn-8) / TOMBSTONE_KEYSIZE(pPg)) : 1)

/*
** Query a single tombstone hash table for rowid iRowid. Return true if
** it is found or false otherwise. The tombstone hash table is one of
** nHashTable tables.
*/
static int fts5IndexTombstoneQuery(
  Fts5Data *pHash,                /* Hash table page to query */
  int nHashTable,                 /* Number of pages attached to segment */
  u64 iRowid                      /* Rowid to query hash for */
){
  const int szKey = TOMBSTONE_KEYSIZE(pHash);
  const int nSlot = TOMBSTONE_NSLOT(pHash);
  int iSlot = (iRowid / nHashTable) % nSlot;
  int nCollide = nSlot;

  if( iRowid==0 ){
    return pHash->p[1];
  }else if( szKey==4 ){
    u32 *aSlot = (u32*)&pHash->p[8];
    while( aSlot[iSlot] ){
      if( fts5GetU32((u8*)&aSlot[iSlot])==iRowid ) return 1;
      if( nCollide--==0 ) break;
      iSlot = (iSlot+1)%nSlot;
    }
  }else{
    u64 *aSlot = (u64*)&pHash->p[8];
    while( aSlot[iSlot] ){
      if( fts5GetU64((u8*)&aSlot[iSlot])==iRowid ) return 1;
      if( nCollide--==0 ) break;
      iSlot = (iSlot+1)%nSlot;
    }
  }

  return 0;
}

/*
** Return true if the iterator passed as the only argument points
** to an segment entry for which there is a tombstone. Return false
** if there is no tombstone or if the iterator is already at EOF.
*/
static int fts5MultiIterIsDeleted(Fts5Iter *pIter){
  int iFirst = pIter->aFirst[1].iFirst;
  Fts5SegIter *pSeg = &pIter->aSeg[iFirst];
  Fts5TombstoneArray *pArray = pSeg->pTombArray;

  if( pSeg->pLeaf && pArray ){
    /* Figure out which page the rowid might be present on. */
    int iPg = ((u64)pSeg->iRowid) % pArray->nTombstone;
    assert( iPg>=0 );

    /* If tombstone hash page iPg has not yet been loaded from the
    ** database, load it now. */
    if( pArray->apTombstone[iPg]==0 ){
      pArray->apTombstone[iPg] = fts5DataRead(pIter->pIndex,
          FTS5_TOMBSTONE_ROWID(pSeg->pSeg->iSegid, iPg)
      );
      if( pArray->apTombstone[iPg]==0 ) return 0;
    }

    return fts5IndexTombstoneQuery(
        pArray->apTombstone[iPg],
        pArray->nTombstone,
        pSeg->iRowid
    );
  }

  return 0;
}

/*
** Move the iterator to the next entry.
**
** If an error occurs, an error code is left in Fts5Index.rc. It is not
** considered an error if the iterator reaches EOF, or if it is already at
** EOF when this function is called.
*/
static void fts5MultiIterNext(
  Fts5Index *p,
  Fts5Iter *pIter,
  int bFrom,                      /* True if argument iFrom is valid */
  i64 iFrom                       /* Advance at least as far as this */
){
  int bUseFrom = bFrom;
  assert( pIter->base.bEof==0 );
  while( p->rc==SQLITE_OK ){
    int iFirst = pIter->aFirst[1].iFirst;
    int bNewTerm = 0;
    Fts5SegIter *pSeg = &pIter->aSeg[iFirst];
    assert( p->rc==SQLITE_OK );
    if( bUseFrom && pSeg->pDlidx ){
      fts5SegIterNextFrom(p, pSeg, iFrom);
    }else{
      pSeg->xNext(p, pSeg, &bNewTerm);
    }

    if( pSeg->pLeaf==0 || bNewTerm
     || fts5MultiIterAdvanceRowid(pIter, iFirst, &pSeg)
    ){
      fts5MultiIterAdvanced(p, pIter, iFirst, 1);
      fts5MultiIterSetEof(pIter);
      pSeg = &pIter->aSeg[pIter->aFirst[1].iFirst];
      if( pSeg->pLeaf==0 ) return;
    }

    fts5AssertMultiIterSetup(p, pIter);
    assert( pSeg==&pIter->aSeg[pIter->aFirst[1].iFirst] && pSeg->pLeaf );
    if( (pIter->bSkipEmpty==0 || pSeg->nPos)
      && 0==fts5MultiIterIsDeleted(pIter)
    ){
      pIter->xSetOutputs(pIter, pSeg);
      return;
    }
    bUseFrom = 0;
  }
}
