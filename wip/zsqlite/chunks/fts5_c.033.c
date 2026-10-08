
/*
** xSetOutputs callback used when the Fts5Colset object has nCol==0 (match
** against no columns at all).
*/
static void fts5IterSetOutputs_ZeroColset(Fts5Iter *pIter, Fts5SegIter *pSeg){
  UNUSED_PARAM(pSeg);
  pIter->base.nData = 0;
}

/*
** xSetOutputs callback used by detail=col when there is a column filter
** and there are 100 or more columns. Also called as a fallback from
** fts5IterSetOutputs_Col100 if the column-list spans more than one page.
*/
static void fts5IterSetOutputs_Col(Fts5Iter *pIter, Fts5SegIter *pSeg){
  fts5BufferZero(&pIter->poslist);
  fts5SegiterPoslist(pIter->pIndex, pSeg, pIter->pColset, &pIter->poslist);
  pIter->base.iRowid = pSeg->iRowid;
  pIter->base.pData = pIter->poslist.p;
  pIter->base.nData = pIter->poslist.n;
}

/*
** xSetOutputs callback used when:
**
**   * detail=col,
**   * there is a column filter, and
**   * the table contains 100 or fewer columns.
**
** The last point is to ensure all column numbers are stored as
** single-byte varints.
*/
static void fts5IterSetOutputs_Col100(Fts5Iter *pIter, Fts5SegIter *pSeg){

  assert( pIter->pIndex->pConfig->eDetail==FTS5_DETAIL_COLUMNS );
  assert( pIter->pColset );

  if( pSeg->iLeafOffset+pSeg->nPos>pSeg->pLeaf->szLeaf ){
    fts5IterSetOutputs_Col(pIter, pSeg);
  }else{
    u8 *a = (u8*)&pSeg->pLeaf->p[pSeg->iLeafOffset];
    u8 *pEnd = (u8*)&a[pSeg->nPos];
    int iPrev = 0;
    int *aiCol = pIter->pColset->aiCol;
    int *aiColEnd = &aiCol[pIter->pColset->nCol];

    u8 *aOut = pIter->poslist.p;
    int iPrevOut = 0;

    pIter->base.iRowid = pSeg->iRowid;

    while( a<pEnd ){
      iPrev += (int)a++[0] - 2;
      while( *aiCol<iPrev ){
        aiCol++;
        if( aiCol==aiColEnd ) goto setoutputs_col_out;
      }
      if( *aiCol==iPrev ){
        *aOut++ = (u8)((iPrev - iPrevOut) + 2);
        iPrevOut = iPrev;
      }
    }

setoutputs_col_out:
    pIter->base.pData = pIter->poslist.p;
    pIter->base.nData = aOut - pIter->poslist.p;
  }
}

/*
** xSetOutputs callback used by detail=full when there is a column filter.
*/
static void fts5IterSetOutputs_Full(Fts5Iter *pIter, Fts5SegIter *pSeg){
  Fts5Colset *pColset = pIter->pColset;
  pIter->base.iRowid = pSeg->iRowid;

  assert( pIter->pIndex->pConfig->eDetail==FTS5_DETAIL_FULL );
  assert( pColset );

  if( pSeg->iLeafOffset+pSeg->nPos<=pSeg->pLeaf->szLeaf ){
    /* All data is stored on the current page. Populate the output
    ** variables to point into the body of the page object. */
    const u8 *a = &pSeg->pLeaf->p[pSeg->iLeafOffset];
    int *pRc = &pIter->pIndex->rc;
    fts5BufferZero(&pIter->poslist);
    fts5IndexExtractColset(pRc, pColset, a, pSeg->nPos, pIter);
  }else{
    /* The data is distributed over two or more pages. Copy it into the
    ** Fts5Iter.poslist buffer and then set the output pointer to point
    ** to this buffer.  */
    fts5BufferZero(&pIter->poslist);
    fts5SegiterPoslist(pIter->pIndex, pSeg, pColset, &pIter->poslist);
    pIter->base.pData = pIter->poslist.p;
    pIter->base.nData = pIter->poslist.n;
  }
}

static void fts5IterSetOutputCb(int *pRc, Fts5Iter *pIter){
  assert( pIter!=0 || (*pRc)!=SQLITE_OK );
  if( *pRc==SQLITE_OK ){
    Fts5Config *pConfig = pIter->pIndex->pConfig;
    if( pConfig->eDetail==FTS5_DETAIL_NONE ){
      pIter->xSetOutputs = fts5IterSetOutputs_None;
    }

    else if( pIter->pColset==0 ){
      pIter->xSetOutputs = fts5IterSetOutputs_Nocolset;
    }

    else if( pIter->pColset->nCol==0 ){
      pIter->xSetOutputs = fts5IterSetOutputs_ZeroColset;
    }

    else if( pConfig->eDetail==FTS5_DETAIL_FULL ){
      pIter->xSetOutputs = fts5IterSetOutputs_Full;
    }

    else{
      assert( pConfig->eDetail==FTS5_DETAIL_COLUMNS );
      if( pConfig->nCol<=100 ){
        pIter->xSetOutputs = fts5IterSetOutputs_Col100;
        sqlite3Fts5BufferSize(pRc, &pIter->poslist, pConfig->nCol);
      }else{
        pIter->xSetOutputs = fts5IterSetOutputs_Col;
      }
    }
  }
}

/*
** All the component segment-iterators of pIter have been set up. This
** functions finishes setup for iterator pIter itself.
*/
static void fts5MultiIterFinishSetup(Fts5Index *p, Fts5Iter *pIter){
  int iIter;
  for(iIter=pIter->nSeg-1; iIter>0; iIter--){
    int iEq;
    if( (iEq = fts5MultiIterDoCompare(pIter, iIter)) ){
      Fts5SegIter *pSeg = &pIter->aSeg[iEq];
      if( p->rc==SQLITE_OK ) pSeg->xNext(p, pSeg, 0);
      fts5MultiIterAdvanced(p, pIter, iEq, iIter);
    }
  }
  fts5MultiIterSetEof(pIter);
  fts5AssertMultiIterSetup(p, pIter);

  if( (pIter->bSkipEmpty && fts5MultiIterIsEmpty(p, pIter))
   || fts5MultiIterIsDeleted(pIter)
  ){
    fts5MultiIterNext(p, pIter, 0, 0);
  }else if( pIter->base.bEof==0 ){
    Fts5SegIter *pSeg = &pIter->aSeg[pIter->aFirst[1].iFirst];
    pIter->xSetOutputs(pIter, pSeg);
  }
}

/*
** Allocate a new Fts5Iter object.
**
** The new object will be used to iterate through data in structure pStruct.
** If iLevel is -ve, then all data in all segments is merged. Or, if iLevel
** is zero or greater, data from the first nSegment segments on level iLevel
** is merged.
**
** The iterator initially points to the first term/rowid entry in the
** iterated data.
*/
static void fts5MultiIterNew(
  Fts5Index *p,                   /* FTS5 backend to iterate within */
  Fts5Structure *pStruct,         /* Structure of specific index */
  int flags,                      /* FTS5INDEX_QUERY_XXX flags */
  Fts5Colset *pColset,            /* Colset to filter on (or NULL) */
  const u8 *pTerm, int nTerm,     /* Term to seek to (or NULL/0) */
  int iLevel,                     /* Level to iterate (-1 for all) */
  int nSegment,                   /* Number of segments to merge (iLevel>=0) */
  Fts5Iter **ppOut                /* New object */
){
  int nSeg = 0;                   /* Number of segment-iters in use */
  int iIter = 0;                  /* */
  int iSeg;                       /* Used to iterate through segments */
  Fts5StructureLevel *pLvl;
  Fts5Iter *pNew;

  assert( (pTerm==0 && nTerm==0) || iLevel<0 );

  /* Allocate space for the new multi-seg-iterator. */
  if( p->rc==SQLITE_OK ){
    if( iLevel<0 ){
      assert( pStruct->nSegment==fts5StructureCountSegments(pStruct) );
      nSeg = pStruct->nSegment;
      nSeg += (p->pHash && 0==(flags & FTS5INDEX_QUERY_SKIPHASH));
    }else{
      nSeg = MIN(pStruct->aLevel[iLevel].nSeg, nSegment);
    }
  }
  *ppOut = pNew = fts5MultiIterAlloc(p, nSeg);
  if( pNew==0 ){
    assert( p->rc!=SQLITE_OK );
    goto fts5MultiIterNew_post_check;
  }
  pNew->bRev = (0!=(flags & FTS5INDEX_QUERY_DESC));
  pNew->bSkipEmpty = (0!=(flags & FTS5INDEX_QUERY_SKIPEMPTY));
  pNew->pColset = pColset;
  if( (flags & FTS5INDEX_QUERY_NOOUTPUT)==0 ){
    fts5IterSetOutputCb(&p->rc, pNew);
  }

  /* Initialize each of the component segment iterators. */
  if( p->rc==SQLITE_OK ){
    if( iLevel<0 ){
      Fts5StructureLevel *pEnd = &pStruct->aLevel[pStruct->nLevel];
      if( p->pHash && 0==(flags & FTS5INDEX_QUERY_SKIPHASH) ){
        /* Add a segment iterator for the current contents of the hash table. */
        Fts5SegIter *pIter = &pNew->aSeg[iIter++];
        fts5SegIterHashInit(p, pTerm, nTerm, flags, pIter);
      }
      for(pLvl=&pStruct->aLevel[0]; pLvl<pEnd; pLvl++){
        for(iSeg=pLvl->nSeg-1; iSeg>=0; iSeg--){
          Fts5StructureSegment *pSeg = &pLvl->aSeg[iSeg];
          Fts5SegIter *pIter = &pNew->aSeg[iIter++];
          if( pTerm==0 ){
            fts5SegIterInit(p, pSeg, pIter);
          }else{
            fts5SegIterSeekInit(p, pTerm, nTerm, flags, pSeg, pIter);
          }
        }
      }
    }else{
      pLvl = &pStruct->aLevel[iLevel];
      for(iSeg=nSeg-1; iSeg>=0; iSeg--){
        fts5SegIterInit(p, &pLvl->aSeg[iSeg], &pNew->aSeg[iIter++]);
      }
    }
    assert( iIter==nSeg );
  }

  /* If the above was successful, each component iterator now points
  ** to the first entry in its segment. In this case initialize the
  ** aFirst[] array. Or, if an error has occurred, free the iterator
  ** object and set the output variable to NULL.  */
  if( p->rc==SQLITE_OK ){
    fts5MultiIterFinishSetup(p, pNew);
  }else{
    fts5MultiIterFree(pNew);
    *ppOut = 0;
  }

fts5MultiIterNew_post_check:
  assert( (*ppOut)!=0 || p->rc!=SQLITE_OK );
  return;
}

/*
** Create an Fts5Iter that iterates through the doclist provided
** as the second argument.
*/
static void fts5MultiIterNew2(
  Fts5Index *p,                   /* FTS5 backend to iterate within */
  Fts5Data *pData,                /* Doclist to iterate through */
  int bDesc,                      /* True for descending rowid order */
  Fts5Iter **ppOut                /* New object */
){
  Fts5Iter *pNew;
  pNew = fts5MultiIterAlloc(p, 2);
  if( pNew ){
    Fts5SegIter *pIter = &pNew->aSeg[1];
    pIter->flags = FTS5_SEGITER_ONETERM;
    if( pData->szLeaf>0 ){
      pIter->pLeaf = pData;
      pIter->iLeafOffset = fts5GetVarint(pData->p, (u64*)&pIter->iRowid);
      pIter->iEndofDoclist = pData->nn;
      pNew->aFirst[1].iFirst = 1;
      if( bDesc ){
        pNew->bRev = 1;
        pIter->flags |= FTS5_SEGITER_REVERSE;
        fts5SegIterReverseInitPage(p, pIter);
      }else{
        fts5SegIterLoadNPos(p, pIter);
      }
      pData = 0;
    }else{
      pNew->base.bEof = 1;
    }
    fts5SegIterSetNext(p, pIter);

    *ppOut = pNew;
  }

  fts5DataRelease(pData);
}

/*
** Return true if the iterator is at EOF or if an error has occurred.
** False otherwise.
*/
static int fts5MultiIterEof(Fts5Index *p, Fts5Iter *pIter){
  assert( pIter!=0 || p->rc!=SQLITE_OK );
  assert( p->rc!=SQLITE_OK
      || (pIter->aSeg[ pIter->aFirst[1].iFirst ].pLeaf==0)==pIter->base.bEof
  );
  return (p->rc || pIter->base.bEof);
}

/*
** Return the rowid of the entry that the iterator currently points
** to. If the iterator points to EOF when this function is called the
** results are undefined.
*/
static i64 fts5MultiIterRowid(Fts5Iter *pIter){
  assert( pIter->aSeg[ pIter->aFirst[1].iFirst ].pLeaf );
  return pIter->aSeg[ pIter->aFirst[1].iFirst ].iRowid;
}

/*
** Move the iterator to the next entry at or following iMatch.
*/
static void fts5MultiIterNextFrom(
  Fts5Index *p,
  Fts5Iter *pIter,
  i64 iMatch
){
  while( 1 ){
    i64 iRowid;
    fts5MultiIterNext(p, pIter, 1, iMatch);
    if( fts5MultiIterEof(p, pIter) ) break;
    iRowid = fts5MultiIterRowid(pIter);
    if( pIter->bRev==0 && iRowid>=iMatch ) break;
    if( pIter->bRev!=0 && iRowid<=iMatch ) break;
  }
}

/*
** Return a pointer to a buffer containing the term associated with the
** entry that the iterator currently points to.
*/
static const u8 *fts5MultiIterTerm(Fts5Iter *pIter, int *pn){
  Fts5SegIter *p = &pIter->aSeg[ pIter->aFirst[1].iFirst ];
  *pn = p->term.n;
  return p->term.p;
}

/*
** Allocate a new segment-id for the structure pStruct. The new segment
** id must be between 1 and 65335 inclusive, and must not be used by
** any currently existing segment. If a free segment id cannot be found,
** SQLITE_FULL is returned.
**
** If an error has already occurred, this function is a no-op. 0 is
** returned in this case.
*/
static int fts5AllocateSegid(Fts5Index *p, Fts5Structure *pStruct){
  int iSegid = 0;

  if( p->rc==SQLITE_OK ){
    if( pStruct->nSegment>=FTS5_MAX_SEGMENT ){
      p->rc = SQLITE_FULL;
    }else{
      /* FTS5_MAX_SEGMENT is currently defined as 2000. So the following
      ** array is 63 elements, or 252 bytes, in size.  */
      u32 aUsed[(FTS5_MAX_SEGMENT+31) / 32];
      int iLvl, iSeg;
      int i;
      u32 mask;
      memset(aUsed, 0, sizeof(aUsed));
      for(iLvl=0; iLvl<pStruct->nLevel; iLvl++){
        for(iSeg=0; iSeg<pStruct->aLevel[iLvl].nSeg; iSeg++){
          int iId = pStruct->aLevel[iLvl].aSeg[iSeg].iSegid;
          if( iId<=FTS5_MAX_SEGMENT && iId>0 ){
            aUsed[(iId-1) / 32] |= (u32)1 << ((iId-1) % 32);
          }
        }
      }

      for(i=0; aUsed[i]==0xFFFFFFFF; i++);
      mask = aUsed[i];
      for(iSegid=0; mask & ((u32)1 << iSegid); iSegid++);
      iSegid += 1 + i*32;

#ifdef SQLITE_DEBUG
      for(iLvl=0; iLvl<pStruct->nLevel; iLvl++){
        for(iSeg=0; iSeg<pStruct->aLevel[iLvl].nSeg; iSeg++){
          assert_nc( iSegid!=pStruct->aLevel[iLvl].aSeg[iSeg].iSegid );
        }
      }
      assert_nc( iSegid>0 && iSegid<=FTS5_MAX_SEGMENT );

      {
        sqlite3_stmt *pIdxSelect = fts5IdxSelectStmt(p);
        if( p->rc==SQLITE_OK ){
          u8 aBlob[2] = {0xff, 0xff};
          sqlite3_bind_int(pIdxSelect, 1, iSegid);
          sqlite3_bind_blob(pIdxSelect, 2, aBlob, 2, SQLITE_STATIC);
          assert_nc( sqlite3_step(pIdxSelect)!=SQLITE_ROW );
          p->rc = sqlite3_reset(pIdxSelect);
          sqlite3_bind_null(pIdxSelect, 2);
        }
      }
#endif
    }
  }

  return iSegid;
}
