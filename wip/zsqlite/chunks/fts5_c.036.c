
/*
** If this is not a contentless_delete=1 table, or if the 'deletemerge'
** configuration option is set to 0, then this function always returns -1.
** Otherwise, it searches the structure object passed as the second argument
** for a level suitable for merging due to having a large number of
** tombstones in the tombstone hash. If one is found, its index is returned.
** Otherwise, if there is no suitable level, -1.
*/
static int fts5IndexFindDeleteMerge(Fts5Index *p, Fts5Structure *pStruct){
  Fts5Config *pConfig = p->pConfig;
  int iRet = -1;
  if( pConfig->bContentlessDelete && pConfig->nDeleteMerge>0 ){
    int ii;
    int nBest = 0;

    for(ii=0; ii<pStruct->nLevel; ii++){
      Fts5StructureLevel *pLvl = &pStruct->aLevel[ii];
      i64 nEntry = 0;
      i64 nTomb = 0;
      int iSeg;
      for(iSeg=0; iSeg<pLvl->nSeg; iSeg++){
        nEntry += pLvl->aSeg[iSeg].nEntry;
        nTomb += pLvl->aSeg[iSeg].nEntryTombstone;
      }
      assert_nc( nEntry>0 || pLvl->nSeg==0 );
      if( nEntry>0 ){
        int nPercent = (nTomb * 100) / nEntry;
        if( nPercent>=pConfig->nDeleteMerge && nPercent>nBest ){
          iRet = ii;
          nBest = nPercent;
        }
      }
    }
  }
  return iRet;
}

/*
** Do up to nPg pages of automerge work on the index.
**
** Return true if any changes were actually made, or false otherwise.
*/
static int fts5IndexMerge(
  Fts5Index *p,                   /* FTS5 backend object */
  Fts5Structure **ppStruct,       /* IN/OUT: Current structure of index */
  int nPg,                        /* Pages of work to do */
  int nMin                        /* Minimum number of segments to merge */
){
  int nRem = nPg;
  int bRet = 0;
  Fts5Structure *pStruct = *ppStruct;
  while( nRem>0 && p->rc==SQLITE_OK ){
    int iLvl;                   /* To iterate through levels */
    int iBestLvl = 0;           /* Level offering the most input segments */
    int nBest = 0;              /* Number of input segments on best level */

    /* Set iBestLvl to the level to read input segments from. Or to -1 if
    ** there is no level suitable to merge segments from.  */
    assert( pStruct->nLevel>0 );
    for(iLvl=0; iLvl<pStruct->nLevel; iLvl++){
      Fts5StructureLevel *pLvl = &pStruct->aLevel[iLvl];
      if( pLvl->nMerge ){
        if( pLvl->nMerge>nBest ){
          iBestLvl = iLvl;
          nBest = nMin;
        }
        break;
      }
      if( pLvl->nSeg>nBest ){
        nBest = pLvl->nSeg;
        iBestLvl = iLvl;
      }
    }
    if( nBest<nMin ){
      iBestLvl = fts5IndexFindDeleteMerge(p, pStruct);
    }

    if( iBestLvl<0 ) break;
    bRet = 1;
    fts5IndexMergeLevel(p, &pStruct, iBestLvl, &nRem);
    if( p->rc==SQLITE_OK && pStruct->aLevel[iBestLvl].nMerge==0 ){
      fts5StructurePromote(p, iBestLvl+1, pStruct);
    }

    if( nMin==1 ) nMin = 2;
  }
  *ppStruct = pStruct;
  return bRet;
}

/*
** A total of nLeaf leaf pages of data has just been flushed to a level-0
** segment. This function updates the write-counter accordingly and, if
** necessary, performs incremental merge work.
**
** If an error occurs, set the Fts5Index.rc error code. If an error has
** already occurred, this function is a no-op.
*/
static void fts5IndexAutomerge(
  Fts5Index *p,                   /* FTS5 backend object */
  Fts5Structure **ppStruct,       /* IN/OUT: Current structure of index */
  int nLeaf                       /* Number of output leaves just written */
){
  if( p->rc==SQLITE_OK && p->pConfig->nAutomerge>0 && ALWAYS((*ppStruct)!=0) ){
    Fts5Structure *pStruct = *ppStruct;
    u64 nWrite;                   /* Initial value of write-counter */
    int nWork;                    /* Number of work-quanta to perform */
    int nRem;                     /* Number of leaf pages left to write */

    /* Update the write-counter. While doing so, set nWork. */
    nWrite = pStruct->nWriteCounter;
    nWork = (int)(((nWrite + nLeaf) / p->nWorkUnit) - (nWrite / p->nWorkUnit));
    pStruct->nWriteCounter += nLeaf;
    nRem = (int)(p->nWorkUnit * nWork * pStruct->nLevel);

    fts5IndexMerge(p, ppStruct, nRem, p->pConfig->nAutomerge);
  }
}

static void fts5IndexCrisismerge(
  Fts5Index *p,                   /* FTS5 backend object */
  Fts5Structure **ppStruct        /* IN/OUT: Current structure of index */
){
  const int nCrisis = p->pConfig->nCrisisMerge;
  Fts5Structure *pStruct = *ppStruct;
  if( pStruct && pStruct->nLevel>0 ){
    int iLvl = 0;
    while( p->rc==SQLITE_OK && pStruct->aLevel[iLvl].nSeg>=nCrisis ){
      fts5IndexMergeLevel(p, &pStruct, iLvl, 0);
      assert( p->rc!=SQLITE_OK || pStruct->nLevel>(iLvl+1) );
      fts5StructurePromote(p, iLvl+1, pStruct);
      iLvl++;
    }
    *ppStruct = pStruct;
  }
}

static int fts5IndexReturn(Fts5Index *p){
  int rc = p->rc;
  p->rc = SQLITE_OK;
  return rc;
}

typedef struct Fts5FlushCtx Fts5FlushCtx;
struct Fts5FlushCtx {
  Fts5Index *pIdx;
  Fts5SegWriter writer;
};

/*
** Buffer aBuf[] contains a list of varints, all small enough to fit
** in a 32-bit integer. Return the size of the largest prefix of this
** list nMax bytes or less in size.
*/
static int fts5PoslistPrefix(const u8 *aBuf, int nMax){
  int ret;
  u32 dummy;
  ret = fts5GetVarint32(aBuf, dummy);
  if( ret<nMax ){
    while( 1 ){
      int i = fts5GetVarint32(&aBuf[ret], dummy);
      if( (ret + i) > nMax ) break;
      ret += i;
    }
  }
  return ret;
}

/*
** Execute the SQL statement:
**
**    DELETE FROM %_idx WHERE (segid, (pgno/2)) = ($iSegid, $iPgno);
**
** This is used when a secure-delete operation removes the last term
** from a segment leaf page. In that case the %_idx entry is removed
** too. This is done to ensure that if all instances of a token are
** removed from an fts5 database in secure-delete mode, no trace of
** the token itself remains in the database.
*/
static void fts5SecureDeleteIdxEntry(
  Fts5Index *p,                   /* FTS5 backend object */
  int iSegid,                     /* Id of segment to delete entry for */
  int iPgno                       /* Page number within segment */
){
  if( iPgno!=1 ){
    assert( p->pConfig->iVersion==FTS5_CURRENT_VERSION_SECUREDELETE );
    if( p->pDeleteFromIdx==0 ){
      fts5IndexPrepareStmt(p, &p->pDeleteFromIdx, sqlite3_mprintf(
          "DELETE FROM '%q'.'%q_idx' WHERE (segid, (pgno/2)) = (?1, ?2)",
          p->pConfig->zDb, p->pConfig->zName
      ));
    }
    if( p->rc==SQLITE_OK ){
      sqlite3_bind_int(p->pDeleteFromIdx, 1, iSegid);
      sqlite3_bind_int(p->pDeleteFromIdx, 2, iPgno);
      sqlite3_step(p->pDeleteFromIdx);
      p->rc = sqlite3_reset(p->pDeleteFromIdx);
    }
  }
}

/*
** This is called when a secure-delete operation removes a position-list
** that overflows onto segment page iPgno of segment pSeg. This function
** rewrites node iPgno, and possibly one or more of its right-hand peers,
** to remove this portion of the position list.
**
** Output variable (*pbLastInDoclist) is set to true if the position-list
** removed is followed by a new term or the end-of-segment, or false if
** it is followed by another rowid/position list.
*/
static void fts5SecureDeleteOverflow(
  Fts5Index *p,
  Fts5StructureSegment *pSeg,
  int iPgno,
  int *pbLastInDoclist
){
  const int bDetailNone = (p->pConfig->eDetail==FTS5_DETAIL_NONE);
  int pgno;
  Fts5Data *pLeaf = 0;
  assert( iPgno!=1 );

  *pbLastInDoclist = 1;
  for(pgno=iPgno; p->rc==SQLITE_OK && pgno<=pSeg->pgnoLast; pgno++){
    i64 iRowid = FTS5_SEGMENT_ROWID(pSeg->iSegid, pgno);
    int iNext = 0;
    u8 *aPg = 0;

    pLeaf = fts5DataRead(p, iRowid);
    if( pLeaf==0 ) break;
    aPg = pLeaf->p;

    iNext = fts5GetU16(&aPg[0]);
    if( iNext!=0 ){
      *pbLastInDoclist = 0;
    }
    if( iNext==0 && pLeaf->szLeaf!=pLeaf->nn ){
      fts5GetVarint32(&aPg[pLeaf->szLeaf], iNext);
    }

    if( iNext==0 ){
      /* The page contains no terms or rowids. Replace it with an empty
      ** page and move on to the right-hand peer.  */
      const u8 aEmpty[] = {0x00, 0x00, 0x00, 0x04};
      assert_nc( bDetailNone==0 || pLeaf->nn==4 );
      if( bDetailNone==0 ) fts5DataWrite(p, iRowid, aEmpty, sizeof(aEmpty));
      fts5DataRelease(pLeaf);
      pLeaf = 0;
    }else if( bDetailNone ){
      break;
    }else if( iNext>=pLeaf->szLeaf || pLeaf->nn<pLeaf->szLeaf || iNext<4 ){
      p->rc = FTS5_CORRUPT;
      break;
    }else{
      int nShift = iNext - 4;
      int nPg;

      int nIdx = 0;
      u8 *aIdx = 0;

      /* Unless the current page footer is 0 bytes in size (in which case
      ** the new page footer will be as well), allocate and populate a
      ** buffer containing the new page footer. Set stack variables aIdx
      ** and nIdx accordingly.  */
      if( pLeaf->nn>pLeaf->szLeaf ){
        int iFirst = 0;
        int i1 = pLeaf->szLeaf;
        int i2 = 0;

        i1 += fts5GetVarint32(&aPg[i1], iFirst);
        if( iFirst<iNext ){
          p->rc = FTS5_CORRUPT;
          break;
        }
        aIdx = sqlite3Fts5MallocZero(&p->rc, (pLeaf->nn-pLeaf->szLeaf)+2);
        if( aIdx==0 ) break;
        i2 = sqlite3Fts5PutVarint(aIdx, iFirst-nShift);
        if( i1<pLeaf->nn ){
          memcpy(&aIdx[i2], &aPg[i1], pLeaf->nn-i1);
          i2 += (pLeaf->nn-i1);
        }
        nIdx = i2;
      }

      /* Modify the contents of buffer aPg[]. Set nPg to the new size
      ** in bytes. The new page is always smaller than the old.  */
      nPg = pLeaf->szLeaf - nShift;
      memmove(&aPg[4], &aPg[4+nShift], nPg-4);
      fts5PutU16(&aPg[2], nPg);
      if( fts5GetU16(&aPg[0]) ) fts5PutU16(&aPg[0], 4);
      if( nIdx>0 ){
        memcpy(&aPg[nPg], aIdx, nIdx);
        nPg += nIdx;
      }
      sqlite3_free(aIdx);

      /* Write the new page to disk and exit the loop */
      assert( nPg>4 || fts5GetU16(aPg)==0 );
      fts5DataWrite(p, iRowid, aPg, nPg);
      break;
    }
  }
  fts5DataRelease(pLeaf);
}

/*
** Completely remove the entry that pSeg currently points to from
** the database.
*/
static void fts5DoSecureDelete(
  Fts5Index *p,
  Fts5SegIter *pSeg
){
  const int bDetailNone = (p->pConfig->eDetail==FTS5_DETAIL_NONE);
  int iSegid = pSeg->pSeg->iSegid;
  u8 *aPg = pSeg->pLeaf->p;
  int nPg = pSeg->pLeaf->nn;
  int iPgIdx = pSeg->pLeaf->szLeaf;

  u64 iDelta = 0;
  int iNextOff = 0;
  int iOff = 0;
  int nIdx = 0;
  u8 *aIdx = 0;
  int bLastInDoclist = 0;
  int iIdx = 0;
  int iStart = 0;
  int iDelKeyOff = 0;       /* Offset of deleted key, if any */

  nIdx = nPg-iPgIdx;
  aIdx = sqlite3Fts5MallocZero(&p->rc, nIdx+16);
  if( p->rc ) return;
  memcpy(aIdx, &aPg[iPgIdx], nIdx);

  /* At this point segment iterator pSeg points to the entry
  ** this function should remove from the b-tree segment.
  **
  ** In detail=full or detail=column mode, pSeg->iLeafOffset is the
  ** offset of the first byte in the position-list for the entry to
  ** remove. Immediately before this comes two varints that will also
  ** need to be removed:
  **
  **     + the rowid or delta rowid value for the entry, and
  **     + the size of the position list in bytes.
  **
  ** Or, in detail=none mode, there is a single varint prior to
  ** pSeg->iLeafOffset - the rowid or delta rowid value.
  **
  ** This block sets the following variables:
  **
  **   iStart:
  **     The offset of the first byte of the rowid or delta-rowid
  **     value for the doclist entry being removed.
  **
  **   iDelta:
  **     The value of the rowid or delta-rowid value for the doclist
  **     entry being removed.
  **
  **   iNextOff:
  **     The offset of the next entry following the position list
  **     for the one being removed. If the position list for this
  **     entry overflows onto the next leaf page, this value will be
  **     greater than pLeaf->szLeaf.
  */
  {
    int iSOP;                     /* Start-Of-Position-list */
    if( pSeg->iLeafPgno==pSeg->iTermLeafPgno ){
      iStart = pSeg->iTermLeafOffset;
    }else{
      iStart = fts5GetU16(&aPg[0]);
    }

    iSOP = iStart + fts5GetVarint(&aPg[iStart], &iDelta);
    assert_nc( iSOP<=pSeg->iLeafOffset );

    if( bDetailNone ){
      while( iSOP<pSeg->iLeafOffset ){
        if( aPg[iSOP]==0x00 ) iSOP++;
        if( aPg[iSOP]==0x00 ) iSOP++;
        iStart = iSOP;
        iSOP = iStart + fts5GetVarint(&aPg[iStart], &iDelta);
      }

      iNextOff = iSOP;
      if( iNextOff<pSeg->iEndofDoclist && aPg[iNextOff]==0x00 ) iNextOff++;
      if( iNextOff<pSeg->iEndofDoclist && aPg[iNextOff]==0x00 ) iNextOff++;

    }else{
      int nPos = 0;
      iSOP += fts5GetVarint32(&aPg[iSOP], nPos);
      while( iSOP<pSeg->iLeafOffset ){
        iStart = iSOP + (nPos/2);
        iSOP = iStart + fts5GetVarint(&aPg[iStart], &iDelta);
        iSOP += fts5GetVarint32(&aPg[iSOP], nPos);
      }
      assert_nc( iSOP==pSeg->iLeafOffset );
      iNextOff = pSeg->iLeafOffset + pSeg->nPos;
    }
  }

  iOff = iStart;

  /* If the position-list for the entry being removed flows over past
  ** the end of this page, delete the portion of the position-list on the
  ** next page and beyond.
  **
  ** Set variable bLastInDoclist to true if this entry happens
  ** to be the last rowid in the doclist for its term.  */
  if( iNextOff>=iPgIdx ){
    int pgno = pSeg->iLeafPgno+1;
    fts5SecureDeleteOverflow(p, pSeg->pSeg, pgno, &bLastInDoclist);
    iNextOff = iPgIdx;
  }

  if( pSeg->bDel==0 ){
    if( iNextOff!=iPgIdx ){
      /* Loop through the page-footer. If iNextOff (offset of the
      ** entry following the one we are removing) is equal to the
      ** offset of a key on this page, then the entry is the last
      ** in its doclist. */
      int iKeyOff = 0;
      for(iIdx=0; iIdx<nIdx; /* no-op */){
        u32 iVal = 0;
        iIdx += fts5GetVarint32(&aIdx[iIdx], iVal);
        iKeyOff += iVal;
        if( iKeyOff==iNextOff ){
          bLastInDoclist = 1;
        }
      }
    }

    /* If this is (a) the first rowid on a page and (b) is not followed by
    ** another position list on the same page, set the "first-rowid" field
    ** of the header to 0.  */
    if( fts5GetU16(&aPg[0])==iStart && (bLastInDoclist || iNextOff==iPgIdx) ){
      fts5PutU16(&aPg[0], 0);
    }
  }

  if( pSeg->bDel ){
    iOff += sqlite3Fts5PutVarint(&aPg[iOff], iDelta);
    aPg[iOff++] = 0x01;
  }else if( bLastInDoclist==0 ){
    if( iNextOff!=iPgIdx ){
      u64 iNextDelta = 0;
      iNextOff += fts5GetVarint(&aPg[iNextOff], &iNextDelta);
      iOff += sqlite3Fts5PutVarint(&aPg[iOff], iDelta + iNextDelta);
    }
  }else if(
      pSeg->iLeafPgno==pSeg->iTermLeafPgno
   && iStart==pSeg->iTermLeafOffset
  ){
    /* The entry being removed was the only position list in its
    ** doclist. Therefore the term needs to be removed as well. */
    int iKey = 0;
    int iKeyOff = 0;

    /* Set iKeyOff to the offset of the term that will be removed - the
    ** last offset in the footer that is not greater than iStart. */
    for(iIdx=0; iIdx<nIdx; iKey++){
      u32 iVal = 0;
      iIdx += fts5GetVarint32(&aIdx[iIdx], iVal);
      if( (iKeyOff+iVal)>(u32)iStart ) break;
      iKeyOff += iVal;
    }
    assert_nc( iKey>=1 );

    /* Set iDelKeyOff to the value of the footer entry to remove from
    ** the page. */
    iDelKeyOff = iOff = iKeyOff;

    if( iNextOff!=iPgIdx ){
      /* This is the only position-list associated with the term, and there
      ** is another term following it on this page. So the subsequent term
      ** needs to be moved to replace the term associated with the entry
      ** being removed. */
      int nPrefix = 0;
      int nSuffix = 0;
      int nPrefix2 = 0;
      int nSuffix2 = 0;

      iDelKeyOff = iNextOff;
      iNextOff += fts5GetVarint32(&aPg[iNextOff], nPrefix2);
      iNextOff += fts5GetVarint32(&aPg[iNextOff], nSuffix2);

      if( iKey!=1 ){
        iKeyOff += fts5GetVarint32(&aPg[iKeyOff], nPrefix);
      }
      iKeyOff += fts5GetVarint32(&aPg[iKeyOff], nSuffix);

      nPrefix = MIN(nPrefix, nPrefix2);
      nSuffix = (nPrefix2 + nSuffix2) - nPrefix;

      if( (iKeyOff+nSuffix)>iPgIdx || (iNextOff+nSuffix2)>iPgIdx ){
        p->rc = FTS5_CORRUPT;
      }else{
        if( iKey!=1 ){
          iOff += sqlite3Fts5PutVarint(&aPg[iOff], nPrefix);
        }
        iOff += sqlite3Fts5PutVarint(&aPg[iOff], nSuffix);
        if( nPrefix2>pSeg->term.n ){
          p->rc = FTS5_CORRUPT;
        }else if( nPrefix2>nPrefix ){
          memcpy(&aPg[iOff], &pSeg->term.p[nPrefix], nPrefix2-nPrefix);
          iOff += (nPrefix2-nPrefix);
        }
        memmove(&aPg[iOff], &aPg[iNextOff], nSuffix2);
        iOff += nSuffix2;
        iNextOff += nSuffix2;
      }
    }
  }else if( iStart==4 ){
    int iPgno;

    assert_nc( pSeg->iLeafPgno>pSeg->iTermLeafPgno );
    /* The entry being removed may be the only position list in
    ** its doclist. */
    for(iPgno=pSeg->iLeafPgno-1; iPgno>pSeg->iTermLeafPgno; iPgno-- ){
      Fts5Data *pPg = fts5DataRead(p, FTS5_SEGMENT_ROWID(iSegid, iPgno));
      int bEmpty = (pPg && pPg->nn==4);
      fts5DataRelease(pPg);
      if( bEmpty==0 ) break;
    }

    if( iPgno==pSeg->iTermLeafPgno ){
      i64 iId = FTS5_SEGMENT_ROWID(iSegid, pSeg->iTermLeafPgno);
      Fts5Data *pTerm = fts5DataRead(p, iId);
      if( pTerm && pTerm->szLeaf==pSeg->iTermLeafOffset ){
        u8 *aTermIdx = &pTerm->p[pTerm->szLeaf];
        int nTermIdx = pTerm->nn - pTerm->szLeaf;
        int iTermIdx = 0;
        int iTermOff = 0;

        while( 1 ){
          u32 iVal = 0;
          int nByte = fts5GetVarint32(&aTermIdx[iTermIdx], iVal);
          iTermOff += iVal;
          if( (iTermIdx+nByte)>=nTermIdx ) break;
          iTermIdx += nByte;
        }
        nTermIdx = iTermIdx;

        memmove(&pTerm->p[iTermOff], &pTerm->p[pTerm->szLeaf], nTermIdx);
        fts5PutU16(&pTerm->p[2], iTermOff);

        fts5DataWrite(p, iId, pTerm->p, iTermOff+nTermIdx);
        if( nTermIdx==0 ){
          fts5SecureDeleteIdxEntry(p, iSegid, pSeg->iTermLeafPgno);
        }
      }
      fts5DataRelease(pTerm);
    }
  }

  /* Assuming no error has occurred, this block does final edits to the
  ** leaf page before writing it back to disk. Input variables are:
  **
  **   nPg: Total initial size of leaf page.
  **   iPgIdx: Initial offset of page footer.
  **
  **   iOff: Offset to move data to
  **   iNextOff: Offset to move data from
  */
  if( p->rc==SQLITE_OK ){
    const int nMove = nPg - iNextOff;     /* Number of bytes to move */
    int nShift = iNextOff - iOff;         /* Distance to move them */

    int iPrevKeyOut = 0;
    int iKeyIn = 0;

    memmove(&aPg[iOff], &aPg[iNextOff], nMove);
    iPgIdx -= nShift;
    nPg = iPgIdx;
    fts5PutU16(&aPg[2], iPgIdx);

    for(iIdx=0; iIdx<nIdx; /* no-op */){
      u32 iVal = 0;
      iIdx += fts5GetVarint32(&aIdx[iIdx], iVal);
      iKeyIn += iVal;
      if( iKeyIn!=iDelKeyOff ){
        int iKeyOut = (iKeyIn - (iKeyIn>iOff ? nShift : 0));
        nPg += sqlite3Fts5PutVarint(&aPg[nPg], iKeyOut - iPrevKeyOut);
        iPrevKeyOut = iKeyOut;
      }
    }

    if( iPgIdx==nPg && nIdx>0 && pSeg->iLeafPgno!=1 ){
      fts5SecureDeleteIdxEntry(p, iSegid, pSeg->iLeafPgno);
    }

    assert_nc( nPg>4 || fts5GetU16(aPg)==0 );
    fts5DataWrite(p, FTS5_SEGMENT_ROWID(iSegid,pSeg->iLeafPgno), aPg, nPg);
  }
  sqlite3_free(aIdx);
}
