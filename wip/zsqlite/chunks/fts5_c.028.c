
/*
** Argument p points to a buffer containing a varint to be interpreted as a
** position list size field. Read the varint and return the number of bytes
** read. Before returning, set *pnSz to the number of bytes in the position
** list, and *pbDel to true if the delete flag is set, or false otherwise.
*/
static int fts5GetPoslistSize(const u8 *p, int *pnSz, int *pbDel){
  int nSz;
  int n = 0;
  fts5FastGetVarint32(p, n, nSz);
  assert_nc( nSz>=0 );
  *pnSz = nSz/2;
  *pbDel = nSz & 0x0001;
  return n;
}

/*
** Fts5SegIter.iLeafOffset currently points to the first byte of a
** position-list size field. Read the value of the field and store it
** in the following variables:
**
**   Fts5SegIter.nPos
**   Fts5SegIter.bDel
**
** Leave Fts5SegIter.iLeafOffset pointing to the first byte of the
** position list content (if any).
*/
static void fts5SegIterLoadNPos(Fts5Index *p, Fts5SegIter *pIter){
  if( p->rc==SQLITE_OK ){
    int iOff = pIter->iLeafOffset;  /* Offset to read at */
    ASSERT_SZLEAF_OK(pIter->pLeaf);
    if( p->pConfig->eDetail==FTS5_DETAIL_NONE ){
      int iEod = MIN(pIter->iEndofDoclist, pIter->pLeaf->szLeaf);
      pIter->bDel = 0;
      pIter->nPos = 1;
      if( iOff<iEod && pIter->pLeaf->p[iOff]==0 ){
        pIter->bDel = 1;
        iOff++;
        if( iOff<iEod && pIter->pLeaf->p[iOff]==0 ){
          pIter->nPos = 1;
          iOff++;
        }else{
          pIter->nPos = 0;
        }
      }
    }else{
      int nSz;
      fts5FastGetVarint32(pIter->pLeaf->p, iOff, nSz);
      pIter->bDel = (nSz & 0x0001);
      pIter->nPos = nSz>>1;
      assert_nc( pIter->nPos>=0 );
    }
    pIter->iLeafOffset = iOff;
  }
}

static void fts5SegIterLoadRowid(Fts5Index *p, Fts5SegIter *pIter){
  u8 *a = pIter->pLeaf->p;        /* Buffer to read data from */
  i64 iOff = pIter->iLeafOffset;

  ASSERT_SZLEAF_OK(pIter->pLeaf);
  while( iOff>=pIter->pLeaf->szLeaf ){
    fts5SegIterNextPage(p, pIter);
    if( pIter->pLeaf==0 ){
      if( p->rc==SQLITE_OK ) p->rc = FTS5_CORRUPT;
      return;
    }
    iOff = 4;
    a = pIter->pLeaf->p;
  }
  iOff += sqlite3Fts5GetVarint(&a[iOff], (u64*)&pIter->iRowid);
  pIter->iLeafOffset = iOff;
}

/*
** Fts5SegIter.iLeafOffset currently points to the first byte of the
** "nSuffix" field of a term. Function parameter nKeep contains the value
** of the "nPrefix" field (if there was one - it is passed 0 if this is
** the first term in the segment).
**
** This function populates:
**
**   Fts5SegIter.term
**   Fts5SegIter.rowid
**
** accordingly and leaves (Fts5SegIter.iLeafOffset) set to the content of
** the first position list. The position list belonging to document
** (Fts5SegIter.iRowid).
*/
static void fts5SegIterLoadTerm(Fts5Index *p, Fts5SegIter *pIter, int nKeep){
  u8 *a = pIter->pLeaf->p;        /* Buffer to read data from */
  i64 iOff = pIter->iLeafOffset;  /* Offset to read at */
  int nNew;                       /* Bytes of new data */

  iOff += fts5GetVarint32(&a[iOff], nNew);
  if( iOff+nNew>pIter->pLeaf->szLeaf || nKeep>pIter->term.n || nNew==0 ){
    p->rc = FTS5_CORRUPT;
    return;
  }
  pIter->term.n = nKeep;
  fts5BufferAppendBlob(&p->rc, &pIter->term, nNew, &a[iOff]);
  assert( pIter->term.n<=pIter->term.nSpace );
  iOff += nNew;
  pIter->iTermLeafOffset = iOff;
  pIter->iTermLeafPgno = pIter->iLeafPgno;
  pIter->iLeafOffset = iOff;

  if( pIter->iPgidxOff>=pIter->pLeaf->nn ){
    pIter->iEndofDoclist = pIter->pLeaf->nn+1;
  }else{
    int nExtra;
    pIter->iPgidxOff += fts5GetVarint32(&a[pIter->iPgidxOff], nExtra);
    pIter->iEndofDoclist += nExtra;
  }

  fts5SegIterLoadRowid(p, pIter);
}

static void fts5SegIterNext(Fts5Index*, Fts5SegIter*, int*);
static void fts5SegIterNext_Reverse(Fts5Index*, Fts5SegIter*, int*);
static void fts5SegIterNext_None(Fts5Index*, Fts5SegIter*, int*);

static void fts5SegIterSetNext(Fts5Index *p, Fts5SegIter *pIter){
  if( pIter->flags & FTS5_SEGITER_REVERSE ){
    pIter->xNext = fts5SegIterNext_Reverse;
  }else if( p->pConfig->eDetail==FTS5_DETAIL_NONE ){
    pIter->xNext = fts5SegIterNext_None;
  }else{
    pIter->xNext = fts5SegIterNext;
  }
}

/*
** Allocate a tombstone hash page array object (pIter->pTombArray) for
** the iterator passed as the second argument. If an OOM error occurs,
** leave an error in the Fts5Index object.
*/
static void fts5SegIterAllocTombstone(Fts5Index *p, Fts5SegIter *pIter){
  const int nTomb = pIter->pSeg->nPgTombstone;
  if( nTomb>0 ){
    int nByte = nTomb * sizeof(Fts5Data*) + sizeof(Fts5TombstoneArray);
    Fts5TombstoneArray *pNew;
    pNew = (Fts5TombstoneArray*)sqlite3Fts5MallocZero(&p->rc, nByte);
    if( pNew ){
      pNew->nTombstone = nTomb;
      pNew->nRef = 1;
      pIter->pTombArray = pNew;
    }
  }
}

/*
** Initialize the iterator object pIter to iterate through the entries in
** segment pSeg. The iterator is left pointing to the first entry when
** this function returns.
**
** If an error occurs, Fts5Index.rc is set to an appropriate error code. If
** an error has already occurred when this function is called, it is a no-op.
*/
static void fts5SegIterInit(
  Fts5Index *p,                   /* FTS index object */
  Fts5StructureSegment *pSeg,     /* Description of segment */
  Fts5SegIter *pIter              /* Object to populate */
){
  if( pSeg->pgnoFirst==0 ){
    /* This happens if the segment is being used as an input to an incremental
    ** merge and all data has already been "trimmed". See function
    ** fts5TrimSegments() for details. In this case leave the iterator empty.
    ** The caller will see the (pIter->pLeaf==0) and assume the iterator is
    ** at EOF already. */
    assert( pIter->pLeaf==0 );
    return;
  }

  if( p->rc==SQLITE_OK ){
    memset(pIter, 0, sizeof(*pIter));
    fts5SegIterSetNext(p, pIter);
    pIter->pSeg = pSeg;
    pIter->iLeafPgno = pSeg->pgnoFirst-1;
    do {
      fts5SegIterNextPage(p, pIter);
    }while( p->rc==SQLITE_OK && pIter->pLeaf && pIter->pLeaf->nn==4 );
  }

  if( p->rc==SQLITE_OK && pIter->pLeaf ){
    pIter->iLeafOffset = 4;
    assert( pIter->pLeaf!=0 );
    assert_nc( pIter->pLeaf->nn>4 );
    assert_nc( fts5LeafFirstTermOff(pIter->pLeaf)==4 );
    pIter->iPgidxOff = pIter->pLeaf->szLeaf+1;
    fts5SegIterLoadTerm(p, pIter, 0);
    fts5SegIterLoadNPos(p, pIter);
    fts5SegIterAllocTombstone(p, pIter);
  }
}

/*
** This function is only ever called on iterators created by calls to
** Fts5IndexQuery() with the FTS5INDEX_QUERY_DESC flag set.
**
** The iterator is in an unusual state when this function is called: the
** Fts5SegIter.iLeafOffset variable is set to the offset of the start of
** the position-list size field for the first relevant rowid on the page.
** Fts5SegIter.rowid is set, but nPos and bDel are not.
**
** This function advances the iterator so that it points to the last
** relevant rowid on the page and, if necessary, initializes the
** aRowidOffset[] and iRowidOffset variables. At this point the iterator
** is in its regular state - Fts5SegIter.iLeafOffset points to the first
** byte of the position list content associated with said rowid.
*/
static void fts5SegIterReverseInitPage(Fts5Index *p, Fts5SegIter *pIter){
  int eDetail = p->pConfig->eDetail;
  int n = pIter->pLeaf->szLeaf;
  int i = pIter->iLeafOffset;
  u8 *a = pIter->pLeaf->p;
  int iRowidOffset = 0;

  if( n>pIter->iEndofDoclist ){
    n = pIter->iEndofDoclist;
  }

  ASSERT_SZLEAF_OK(pIter->pLeaf);
  while( 1 ){
    u64 iDelta = 0;

    if( eDetail==FTS5_DETAIL_NONE ){
      /* todo */
      if( i<n && a[i]==0 ){
        i++;
        if( i<n && a[i]==0 ) i++;
      }
    }else{
      int nPos;
      int bDummy;
      i += fts5GetPoslistSize(&a[i], &nPos, &bDummy);
      i += nPos;
    }
    if( i>=n ) break;
    i += fts5GetVarint(&a[i], &iDelta);
    pIter->iRowid += iDelta;

    /* If necessary, grow the pIter->aRowidOffset[] array. */
    if( iRowidOffset>=pIter->nRowidOffset ){
      int nNew = pIter->nRowidOffset + 8;
      int *aNew = (int*)sqlite3_realloc64(pIter->aRowidOffset,nNew*sizeof(int));
      if( aNew==0 ){
        p->rc = SQLITE_NOMEM;
        break;
      }
      pIter->aRowidOffset = aNew;
      pIter->nRowidOffset = nNew;
    }

    pIter->aRowidOffset[iRowidOffset++] = pIter->iLeafOffset;
    pIter->iLeafOffset = i;
  }
  pIter->iRowidOffset = iRowidOffset;
  fts5SegIterLoadNPos(p, pIter);
}

/*
**
*/
static void fts5SegIterReverseNewPage(Fts5Index *p, Fts5SegIter *pIter){
  assert( pIter->flags & FTS5_SEGITER_REVERSE );
  assert( pIter->flags & FTS5_SEGITER_ONETERM );

  fts5DataRelease(pIter->pLeaf);
  pIter->pLeaf = 0;
  while( p->rc==SQLITE_OK && pIter->iLeafPgno>pIter->iTermLeafPgno ){
    Fts5Data *pNew;
    pIter->iLeafPgno--;
    pNew = fts5DataRead(p, FTS5_SEGMENT_ROWID(
          pIter->pSeg->iSegid, pIter->iLeafPgno
    ));
    if( pNew ){
      /* iTermLeafOffset may be equal to szLeaf if the term is the last
      ** thing on the page - i.e. the first rowid is on the following page.
      ** In this case leave pIter->pLeaf==0, this iterator is at EOF. */
      if( pIter->iLeafPgno==pIter->iTermLeafPgno ){
        assert( pIter->pLeaf==0 );
        if( pIter->iTermLeafOffset<pNew->szLeaf ){
          pIter->pLeaf = pNew;
          pIter->iLeafOffset = pIter->iTermLeafOffset;
        }
      }else{
        int iRowidOff;
        iRowidOff = fts5LeafFirstRowidOff(pNew);
        if( iRowidOff ){
          if( iRowidOff>=pNew->szLeaf ){
            p->rc = FTS5_CORRUPT;
          }else{
            pIter->pLeaf = pNew;
            pIter->iLeafOffset = iRowidOff;
          }
        }
      }

      if( pIter->pLeaf ){
        u8 *a = &pIter->pLeaf->p[pIter->iLeafOffset];
        pIter->iLeafOffset += fts5GetVarint(a, (u64*)&pIter->iRowid);
        break;
      }else{
        fts5DataRelease(pNew);
      }
    }
  }

  if( pIter->pLeaf ){
    pIter->iEndofDoclist = pIter->pLeaf->nn+1;
    fts5SegIterReverseInitPage(p, pIter);
  }
}

/*
** Return true if the iterator passed as the second argument currently
** points to a delete marker. A delete marker is an entry with a 0 byte
** position-list.
*/
static int fts5MultiIterIsEmpty(Fts5Index *p, Fts5Iter *pIter){
  Fts5SegIter *pSeg = &pIter->aSeg[pIter->aFirst[1].iFirst];
  return (p->rc==SQLITE_OK && pSeg->pLeaf && pSeg->nPos==0);
}

/*
** Advance iterator pIter to the next entry.
**
** This version of fts5SegIterNext() is only used by reverse iterators.
*/
static void fts5SegIterNext_Reverse(
  Fts5Index *p,                   /* FTS5 backend object */
  Fts5SegIter *pIter,             /* Iterator to advance */
  int *pbUnused                   /* Unused */
){
  assert( pIter->flags & FTS5_SEGITER_REVERSE );
  assert( pIter->pNextLeaf==0 );
  UNUSED_PARAM(pbUnused);

  if( pIter->iRowidOffset>0 ){
    u8 *a = pIter->pLeaf->p;
    int iOff;
    u64 iDelta;

    pIter->iRowidOffset--;
    pIter->iLeafOffset = pIter->aRowidOffset[pIter->iRowidOffset];
    fts5SegIterLoadNPos(p, pIter);
    iOff = pIter->iLeafOffset;
    if( p->pConfig->eDetail!=FTS5_DETAIL_NONE ){
      iOff += pIter->nPos;
    }
    fts5GetVarint(&a[iOff], &iDelta);
    pIter->iRowid -= iDelta;
  }else{
    fts5SegIterReverseNewPage(p, pIter);
  }
}
