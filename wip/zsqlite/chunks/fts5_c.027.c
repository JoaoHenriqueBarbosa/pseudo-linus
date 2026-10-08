
#if 0
static void fts5DebugStructure(int*,Fts5Buffer*,Fts5Structure*);
static void fts5PrintStructure(const char *zCaption, Fts5Structure *pStruct){
  int rc = SQLITE_OK;
  Fts5Buffer buf;
  memset(&buf, 0, sizeof(buf));
  fts5DebugStructure(&rc, &buf, pStruct);
  fprintf(stdout, "%s: %s\n", zCaption, buf.p);
  fflush(stdout);
  fts5BufferFree(&buf);
}
#else
# define fts5PrintStructure(x,y)
#endif

static int fts5SegmentSize(Fts5StructureSegment *pSeg){
  return 1 + pSeg->pgnoLast - pSeg->pgnoFirst;
}

/*
** Return a copy of index structure pStruct. Except, promote as many
** segments as possible to level iPromote. If an OOM occurs, NULL is
** returned.
*/
static void fts5StructurePromoteTo(
  Fts5Index *p,
  int iPromote,
  int szPromote,
  Fts5Structure *pStruct
){
  int il, is;
  Fts5StructureLevel *pOut = &pStruct->aLevel[iPromote];

  if( pOut->nMerge==0 ){
    for(il=iPromote+1; il<pStruct->nLevel; il++){
      Fts5StructureLevel *pLvl = &pStruct->aLevel[il];
      if( pLvl->nMerge ) return;
      for(is=pLvl->nSeg-1; is>=0; is--){
        int sz = fts5SegmentSize(&pLvl->aSeg[is]);
        if( sz>szPromote ) return;
        fts5StructureExtendLevel(&p->rc, pStruct, iPromote, 1, 1);
        if( p->rc ) return;
        memcpy(pOut->aSeg, &pLvl->aSeg[is], sizeof(Fts5StructureSegment));
        pOut->nSeg++;
        pLvl->nSeg--;
      }
    }
  }
}

/*
** A new segment has just been written to level iLvl of index structure
** pStruct. This function determines if any segments should be promoted
** as a result. Segments are promoted in two scenarios:
**
**   a) If the segment just written is smaller than one or more segments
**      within the previous populated level, it is promoted to the previous
**      populated level.
**
**   b) If the segment just written is larger than the newest segment on
**      the next populated level, then that segment, and any other adjacent
**      segments that are also smaller than the one just written, are
**      promoted.
**
** If one or more segments are promoted, the structure object is updated
** to reflect this.
*/
static void fts5StructurePromote(
  Fts5Index *p,                   /* FTS5 backend object */
  int iLvl,                       /* Index level just updated */
  Fts5Structure *pStruct          /* Index structure */
){
  if( p->rc==SQLITE_OK ){
    int iTst;
    int iPromote = -1;
    int szPromote = 0;            /* Promote anything this size or smaller */
    Fts5StructureSegment *pSeg;   /* Segment just written */
    int szSeg;                    /* Size of segment just written */
    int nSeg = pStruct->aLevel[iLvl].nSeg;

    if( nSeg==0 ) return;
    pSeg = &pStruct->aLevel[iLvl].aSeg[pStruct->aLevel[iLvl].nSeg-1];
    szSeg = (1 + pSeg->pgnoLast - pSeg->pgnoFirst);

    /* Check for condition (a) */
    for(iTst=iLvl-1; iTst>=0 && pStruct->aLevel[iTst].nSeg==0; iTst--);
    if( iTst>=0 ){
      int i;
      int szMax = 0;
      Fts5StructureLevel *pTst = &pStruct->aLevel[iTst];
      assert( pTst->nMerge==0 );
      for(i=0; i<pTst->nSeg; i++){
        int sz = pTst->aSeg[i].pgnoLast - pTst->aSeg[i].pgnoFirst + 1;
        if( sz>szMax ) szMax = sz;
      }
      if( szMax>=szSeg ){
        /* Condition (a) is true. Promote the newest segment on level
        ** iLvl to level iTst.  */
        iPromote = iTst;
        szPromote = szMax;
      }
    }

    /* If condition (a) is not met, assume (b) is true. StructurePromoteTo()
    ** is a no-op if it is not.  */
    if( iPromote<0 ){
      iPromote = iLvl;
      szPromote = szSeg;
    }
    fts5StructurePromoteTo(p, iPromote, szPromote, pStruct);
  }
}


/*
** Advance the iterator passed as the only argument. If the end of the
** doclist-index page is reached, return non-zero.
*/
static int fts5DlidxLvlNext(Fts5DlidxLvl *pLvl){
  Fts5Data *pData = pLvl->pData;

  if( pLvl->iOff==0 ){
    assert( pLvl->bEof==0 );
    pLvl->iOff = 1;
    pLvl->iOff += fts5GetVarint32(&pData->p[1], pLvl->iLeafPgno);
    pLvl->iOff += fts5GetVarint(&pData->p[pLvl->iOff], (u64*)&pLvl->iRowid);
    pLvl->iFirstOff = pLvl->iOff;
  }else{
    int iOff;
    for(iOff=pLvl->iOff; iOff<pData->nn; iOff++){
      if( pData->p[iOff] ) break;
    }

    if( iOff<pData->nn ){
      u64 iVal;
      pLvl->iLeafPgno += (iOff - pLvl->iOff) + 1;
      iOff += fts5GetVarint(&pData->p[iOff], &iVal);
      pLvl->iRowid += iVal;
      pLvl->iOff = iOff;
    }else{
      pLvl->bEof = 1;
    }
  }

  return pLvl->bEof;
}

/*
** Advance the iterator passed as the only argument.
*/
static int fts5DlidxIterNextR(Fts5Index *p, Fts5DlidxIter *pIter, int iLvl){
  Fts5DlidxLvl *pLvl = &pIter->aLvl[iLvl];

  assert( iLvl<pIter->nLvl );
  if( fts5DlidxLvlNext(pLvl) ){
    if( (iLvl+1) < pIter->nLvl ){
      fts5DlidxIterNextR(p, pIter, iLvl+1);
      if( pLvl[1].bEof==0 ){
        fts5DataRelease(pLvl->pData);
        memset(pLvl, 0, sizeof(Fts5DlidxLvl));
        pLvl->pData = fts5DataRead(p,
            FTS5_DLIDX_ROWID(pIter->iSegid, iLvl, pLvl[1].iLeafPgno)
        );
        if( pLvl->pData ) fts5DlidxLvlNext(pLvl);
      }
    }
  }

  return pIter->aLvl[0].bEof;
}
static int fts5DlidxIterNext(Fts5Index *p, Fts5DlidxIter *pIter){
  return fts5DlidxIterNextR(p, pIter, 0);
}

/*
** The iterator passed as the first argument has the following fields set
** as follows. This function sets up the rest of the iterator so that it
** points to the first rowid in the doclist-index.
**
**   pData:
**     pointer to doclist-index record,
**
** When this function is called pIter->iLeafPgno is the page number the
** doclist is associated with (the one featuring the term).
*/
static int fts5DlidxIterFirst(Fts5DlidxIter *pIter){
  int i;
  for(i=0; i<pIter->nLvl; i++){
    fts5DlidxLvlNext(&pIter->aLvl[i]);
  }
  return pIter->aLvl[0].bEof;
}


static int fts5DlidxIterEof(Fts5Index *p, Fts5DlidxIter *pIter){
  return p->rc!=SQLITE_OK || pIter->aLvl[0].bEof;
}

static void fts5DlidxIterLast(Fts5Index *p, Fts5DlidxIter *pIter){
  int i;

  /* Advance each level to the last entry on the last page */
  for(i=pIter->nLvl-1; p->rc==SQLITE_OK && i>=0; i--){
    Fts5DlidxLvl *pLvl = &pIter->aLvl[i];
    while( fts5DlidxLvlNext(pLvl)==0 );
    pLvl->bEof = 0;

    if( i>0 ){
      Fts5DlidxLvl *pChild = &pLvl[-1];
      fts5DataRelease(pChild->pData);
      memset(pChild, 0, sizeof(Fts5DlidxLvl));
      pChild->pData = fts5DataRead(p,
          FTS5_DLIDX_ROWID(pIter->iSegid, i-1, pLvl->iLeafPgno)
      );
    }
  }
}

/*
** Move the iterator passed as the only argument to the previous entry.
*/
static int fts5DlidxLvlPrev(Fts5DlidxLvl *pLvl){
  int iOff = pLvl->iOff;

  assert( pLvl->bEof==0 );
  if( iOff<=pLvl->iFirstOff ){
    pLvl->bEof = 1;
  }else{
    u8 *a = pLvl->pData->p;

    pLvl->iOff = 0;
    fts5DlidxLvlNext(pLvl);
    while( 1 ){
      int nZero = 0;
      int ii = pLvl->iOff;
      u64 delta = 0;

      while( a[ii]==0 ){
        nZero++;
        ii++;
      }
      ii += sqlite3Fts5GetVarint(&a[ii], &delta);

      if( ii>=iOff ) break;
      pLvl->iLeafPgno += nZero+1;
      pLvl->iRowid += delta;
      pLvl->iOff = ii;
    }
  }

  return pLvl->bEof;
}

static int fts5DlidxIterPrevR(Fts5Index *p, Fts5DlidxIter *pIter, int iLvl){
  Fts5DlidxLvl *pLvl = &pIter->aLvl[iLvl];

  assert( iLvl<pIter->nLvl );
  if( fts5DlidxLvlPrev(pLvl) ){
    if( (iLvl+1) < pIter->nLvl ){
      fts5DlidxIterPrevR(p, pIter, iLvl+1);
      if( pLvl[1].bEof==0 ){
        fts5DataRelease(pLvl->pData);
        memset(pLvl, 0, sizeof(Fts5DlidxLvl));
        pLvl->pData = fts5DataRead(p,
            FTS5_DLIDX_ROWID(pIter->iSegid, iLvl, pLvl[1].iLeafPgno)
        );
        if( pLvl->pData ){
          while( fts5DlidxLvlNext(pLvl)==0 );
          pLvl->bEof = 0;
        }
      }
    }
  }

  return pIter->aLvl[0].bEof;
}
static int fts5DlidxIterPrev(Fts5Index *p, Fts5DlidxIter *pIter){
  return fts5DlidxIterPrevR(p, pIter, 0);
}

/*
** Free a doclist-index iterator object allocated by fts5DlidxIterInit().
*/
static void fts5DlidxIterFree(Fts5DlidxIter *pIter){
  if( pIter ){
    int i;
    for(i=0; i<pIter->nLvl; i++){
      fts5DataRelease(pIter->aLvl[i].pData);
    }
    sqlite3_free(pIter);
  }
}

static Fts5DlidxIter *fts5DlidxIterInit(
  Fts5Index *p,                   /* Fts5 Backend to iterate within */
  int bRev,                       /* True for ORDER BY ASC */
  int iSegid,                     /* Segment id */
  int iLeafPg                     /* Leaf page number to load dlidx for */
){
  Fts5DlidxIter *pIter = 0;
  int i;
  int bDone = 0;

  for(i=0; p->rc==SQLITE_OK && bDone==0; i++){
    sqlite3_int64 nByte = sizeof(Fts5DlidxIter) + i * sizeof(Fts5DlidxLvl);
    Fts5DlidxIter *pNew;

    pNew = (Fts5DlidxIter*)sqlite3_realloc64(pIter, nByte);
    if( pNew==0 ){
      p->rc = SQLITE_NOMEM;
    }else{
      i64 iRowid = FTS5_DLIDX_ROWID(iSegid, i, iLeafPg);
      Fts5DlidxLvl *pLvl = &pNew->aLvl[i];
      pIter = pNew;
      memset(pLvl, 0, sizeof(Fts5DlidxLvl));
      pLvl->pData = fts5DataRead(p, iRowid);
      if( pLvl->pData && (pLvl->pData->p[0] & 0x0001)==0 ){
        bDone = 1;
      }
      pIter->nLvl = i+1;
    }
  }

  if( p->rc==SQLITE_OK ){
    pIter->iSegid = iSegid;
    if( bRev==0 ){
      fts5DlidxIterFirst(pIter);
    }else{
      fts5DlidxIterLast(p, pIter);
    }
  }

  if( p->rc!=SQLITE_OK ){
    fts5DlidxIterFree(pIter);
    pIter = 0;
  }

  return pIter;
}

static i64 fts5DlidxIterRowid(Fts5DlidxIter *pIter){
  return pIter->aLvl[0].iRowid;
}
static int fts5DlidxIterPgno(Fts5DlidxIter *pIter){
  return pIter->aLvl[0].iLeafPgno;
}

/*
** Load the next leaf page into the segment iterator.
*/
static void fts5SegIterNextPage(
  Fts5Index *p,                   /* FTS5 backend object */
  Fts5SegIter *pIter              /* Iterator to advance to next page */
){
  Fts5Data *pLeaf;
  Fts5StructureSegment *pSeg = pIter->pSeg;
  fts5DataRelease(pIter->pLeaf);
  pIter->iLeafPgno++;
  if( pIter->pNextLeaf ){
    pIter->pLeaf = pIter->pNextLeaf;
    pIter->pNextLeaf = 0;
  }else if( pIter->iLeafPgno<=pSeg->pgnoLast ){
    pIter->pLeaf = fts5LeafRead(p,
        FTS5_SEGMENT_ROWID(pSeg->iSegid, pIter->iLeafPgno)
    );
  }else{
    pIter->pLeaf = 0;
  }
  pLeaf = pIter->pLeaf;

  if( pLeaf ){
    pIter->iPgidxOff = pLeaf->szLeaf;
    if( fts5LeafIsTermless(pLeaf) ){
      pIter->iEndofDoclist = pLeaf->nn+1;
    }else{
      pIter->iPgidxOff += fts5GetVarint32(&pLeaf->p[pIter->iPgidxOff],
          pIter->iEndofDoclist
      );
    }
  }
}
