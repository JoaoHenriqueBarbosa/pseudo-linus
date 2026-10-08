
/*
** Buffer pPg contains a page of a tombstone hash table - one of nPg pages
** associated with the same segment. This function adds rowid iRowid to
** the hash table. The caller is required to guarantee that there is at
** least one free slot on the page.
**
** If parameter bForce is false and the hash table is deemed to be full
** (more than half of the slots are occupied), then non-zero is returned
** and iRowid not inserted. Or, if bForce is true or if the hash table page
** is not full, iRowid is inserted and zero returned.
*/
static int fts5IndexTombstoneAddToPage(
  Fts5Data *pPg,
  int bForce,
  int nPg,
  u64 iRowid
){
  const int szKey = TOMBSTONE_KEYSIZE(pPg);
  const int nSlot = TOMBSTONE_NSLOT(pPg);
  const int nElem = fts5GetU32(&pPg->p[4]);
  int iSlot = (iRowid / nPg) % nSlot;
  int nCollide = nSlot;

  if( szKey==4 && iRowid>0xFFFFFFFF ) return 2;
  if( iRowid==0 ){
    pPg->p[1] = 0x01;
    return 0;
  }

  if( bForce==0 && nElem>=(nSlot/2) ){
    return 1;
  }

  fts5PutU32(&pPg->p[4], nElem+1);
  if( szKey==4 ){
    u32 *aSlot = (u32*)&pPg->p[8];
    while( aSlot[iSlot] ){
      iSlot = (iSlot + 1) % nSlot;
      if( nCollide--==0 ) return 0;
    }
    fts5PutU32((u8*)&aSlot[iSlot], (u32)iRowid);
  }else{
    u64 *aSlot = (u64*)&pPg->p[8];
    while( aSlot[iSlot] ){
      iSlot = (iSlot + 1) % nSlot;
      if( nCollide--==0 ) return 0;
    }
    fts5PutU64((u8*)&aSlot[iSlot], iRowid);
  }

  return 0;
}

/*
** This function attempts to build a new hash containing all the keys
** currently in the tombstone hash table for segment pSeg. The new
** hash will be stored in the nOut buffers passed in array apOut[].
** All pages of the new hash use key-size szKey (4 or 8).
**
** Return 0 if the hash is successfully rebuilt into the nOut pages.
** Or non-zero if it is not (because one page became overfull). In this
** case the caller should retry with a larger nOut parameter.
**
** Parameter pData1 is page iPg1 of the hash table being rebuilt.
*/
static int fts5IndexTombstoneRehash(
  Fts5Index *p,
  Fts5StructureSegment *pSeg,     /* Segment to rebuild hash of */
  Fts5Data *pData1,               /* One page of current hash - or NULL */
  int iPg1,                       /* Which page of the current hash is pData1 */
  int szKey,                      /* 4 or 8, the keysize */
  int nOut,                       /* Number of output pages */
  Fts5Data **apOut                /* Array of output hash pages */
){
  int ii;
  int res = 0;

  /* Initialize the headers of all the output pages */
  for(ii=0; ii<nOut; ii++){
    apOut[ii]->p[0] = szKey;
    fts5PutU32(&apOut[ii]->p[4], 0);
  }

  /* Loop through the current pages of the hash table. */
  for(ii=0; res==0 && ii<pSeg->nPgTombstone; ii++){
    Fts5Data *pData = 0;          /* Page ii of the current hash table */
    Fts5Data *pFree = 0;          /* Free this at the end of the loop */

    if( iPg1==ii ){
      pData = pData1;
    }else{
      pFree = pData = fts5DataRead(p, FTS5_TOMBSTONE_ROWID(pSeg->iSegid, ii));
    }

    if( pData ){
      int szKeyIn = TOMBSTONE_KEYSIZE(pData);
      int nSlotIn = (pData->nn - 8) / szKeyIn;
      int iIn;
      for(iIn=0; iIn<nSlotIn; iIn++){
        u64 iVal = 0;

        /* Read the value from slot iIn of the input page into iVal. */
        if( szKeyIn==4 ){
          u32 *aSlot = (u32*)&pData->p[8];
          if( aSlot[iIn] ) iVal = fts5GetU32((u8*)&aSlot[iIn]);
        }else{
          u64 *aSlot = (u64*)&pData->p[8];
          if( aSlot[iIn] ) iVal = fts5GetU64((u8*)&aSlot[iIn]);
        }

        /* If iVal is not 0 at this point, insert it into the new hash table */
        if( iVal ){
          Fts5Data *pPg = apOut[(iVal % nOut)];
          res = fts5IndexTombstoneAddToPage(pPg, 0, nOut, iVal);
          if( res ) break;
        }
      }

      /* If this is page 0 of the old hash, copy the rowid-0-flag from the
      ** old hash to the new.  */
      if( ii==0 ){
        apOut[0]->p[1] = pData->p[1];
      }
    }
    fts5DataRelease(pFree);
  }

  return res;
}

/*
** This is called to rebuild the hash table belonging to segment pSeg.
** If parameter pData1 is not NULL, then one page of the existing hash table
** has already been loaded - pData1, which is page iPg1. The key-size for
** the new hash table is szKey (4 or 8).
**
** If successful, the new hash table is not written to disk. Instead,
** output parameter (*pnOut) is set to the number of pages in the new
** hash table, and (*papOut) to point to an array of buffers containing
** the new page data.
**
** If an error occurs, an error code is left in the Fts5Index object and
** both output parameters set to 0 before returning.
*/
static void fts5IndexTombstoneRebuild(
  Fts5Index *p,
  Fts5StructureSegment *pSeg,     /* Segment to rebuild hash of */
  Fts5Data *pData1,               /* One page of current hash - or NULL */
  int iPg1,                       /* Which page of the current hash is pData1 */
  int szKey,                      /* 4 or 8, the keysize */
  int *pnOut,                     /* OUT: Number of output pages */
  Fts5Data ***papOut              /* OUT: Output hash pages */
){
  const int MINSLOT = 32;
  int nSlotPerPage = MAX(MINSLOT, (p->pConfig->pgsz - 8) / szKey);
  int nSlot = 0;                  /* Number of slots in each output page */
  int nOut = 0;

  /* Figure out how many output pages (nOut) and how many slots per
  ** page (nSlot).  There are three possibilities:
  **
  **   1. The hash table does not yet exist. In this case the new hash
  **      table will consist of a single page with MINSLOT slots.
  **
  **   2. The hash table exists but is currently a single page. In this
  **      case an attempt is made to grow the page to accommodate the new
  **      entry. The page is allowed to grow up to nSlotPerPage (see above)
  **      slots.
  **
  **   3. The hash table already consists of more than one page, or of
  **      a single page already so large that it cannot be grown. In this
  **      case the new hash consists of (nPg*2+1) pages of nSlotPerPage
  **      slots each, where nPg is the current number of pages in the
  **      hash table.
  */
  if( pSeg->nPgTombstone==0 ){
    /* Case 1. */
    nOut = 1;
    nSlot = MINSLOT;
  }else if( pSeg->nPgTombstone==1 ){
    /* Case 2. */
    int nElem = (int)fts5GetU32(&pData1->p[4]);
    assert( pData1 && iPg1==0 );
    nOut = 1;
    nSlot = MAX(nElem*4, MINSLOT);
    if( nSlot>nSlotPerPage ) nOut = 0;
  }
  if( nOut==0 ){
    /* Case 3. */
    nOut = (pSeg->nPgTombstone * 2 + 1);
    nSlot = nSlotPerPage;
  }

  /* Allocate the required array and output pages */
  while( 1 ){
    int res = 0;
    int ii = 0;
    int szPage = 0;
    Fts5Data **apOut = 0;

    /* Allocate space for the new hash table */
    assert( nSlot>=MINSLOT );
    apOut = (Fts5Data**)sqlite3Fts5MallocZero(&p->rc, sizeof(Fts5Data*) * nOut);
    szPage = 8 + nSlot*szKey;
    for(ii=0; ii<nOut; ii++){
      Fts5Data *pNew = (Fts5Data*)sqlite3Fts5MallocZero(&p->rc,
          sizeof(Fts5Data)+szPage
      );
      if( pNew ){
        pNew->nn = szPage;
        pNew->p = (u8*)&pNew[1];
        apOut[ii] = pNew;
      }
    }

    /* Rebuild the hash table. */
    if( p->rc==SQLITE_OK ){
      res = fts5IndexTombstoneRehash(p, pSeg, pData1, iPg1, szKey, nOut, apOut);
    }
    if( res==0 ){
      if( p->rc ){
        fts5IndexFreeArray(apOut, nOut);
        apOut = 0;
        nOut = 0;
      }
      *pnOut = nOut;
      *papOut = apOut;
      break;
    }

    /* If control flows to here, it was not possible to rebuild the hash
    ** table. Free all buffers and then try again with more pages. */
    assert( p->rc==SQLITE_OK );
    fts5IndexFreeArray(apOut, nOut);
    nSlot = nSlotPerPage;
    nOut = nOut*2 + 1;
  }
}


/*
** Add a tombstone for rowid iRowid to segment pSeg.
*/
static void fts5IndexTombstoneAdd(
  Fts5Index *p,
  Fts5StructureSegment *pSeg,
  u64 iRowid
){
  Fts5Data *pPg = 0;
  int iPg = -1;
  int szKey = 0;
  int nHash = 0;
  Fts5Data **apHash = 0;

  p->nContentlessDelete++;

  if( pSeg->nPgTombstone>0 ){
    iPg = iRowid % pSeg->nPgTombstone;
    pPg = fts5DataRead(p, FTS5_TOMBSTONE_ROWID(pSeg->iSegid,iPg));
    if( pPg==0 ){
      assert( p->rc!=SQLITE_OK );
      return;
    }

    if( 0==fts5IndexTombstoneAddToPage(pPg, 0, pSeg->nPgTombstone, iRowid) ){
      fts5DataWrite(p, FTS5_TOMBSTONE_ROWID(pSeg->iSegid,iPg), pPg->p, pPg->nn);
      fts5DataRelease(pPg);
      return;
    }
  }

  /* Have to rebuild the hash table. First figure out the key-size (4 or 8). */
  szKey = pPg ? TOMBSTONE_KEYSIZE(pPg) : 4;
  if( iRowid>0xFFFFFFFF ) szKey = 8;

  /* Rebuild the hash table */
  fts5IndexTombstoneRebuild(p, pSeg, pPg, iPg, szKey, &nHash, &apHash);
  assert( p->rc==SQLITE_OK || (nHash==0 && apHash==0) );

  /* If all has succeeded, write the new rowid into one of the new hash
  ** table pages, then write them all out to disk. */
  if( nHash ){
    int ii = 0;
    fts5IndexTombstoneAddToPage(apHash[iRowid % nHash], 1, nHash, iRowid);
    for(ii=0; ii<nHash; ii++){
      i64 iTombstoneRowid = FTS5_TOMBSTONE_ROWID(pSeg->iSegid, ii);
      fts5DataWrite(p, iTombstoneRowid, apHash[ii]->p, apHash[ii]->nn);
    }
    pSeg->nPgTombstone = nHash;
    fts5StructureWrite(p, p->pStruct);
  }

  fts5DataRelease(pPg);
  fts5IndexFreeArray(apHash, nHash);
}

/*
** Add iRowid to the tombstone list of the segment or segments that contain
** rows from origin iOrigin. Return SQLITE_OK if successful, or an SQLite
** error code otherwise.
*/
static int sqlite3Fts5IndexContentlessDelete(Fts5Index *p, i64 iOrigin, i64 iRowid){
  Fts5Structure *pStruct;
  pStruct = fts5StructureRead(p);
  if( pStruct ){
    int bFound = 0;               /* True after pSeg->nEntryTombstone incr. */
    int iLvl;
    for(iLvl=pStruct->nLevel-1; iLvl>=0; iLvl--){
      int iSeg;
      for(iSeg=pStruct->aLevel[iLvl].nSeg-1; iSeg>=0; iSeg--){
        Fts5StructureSegment *pSeg = &pStruct->aLevel[iLvl].aSeg[iSeg];
        if( pSeg->iOrigin1<=(u64)iOrigin && pSeg->iOrigin2>=(u64)iOrigin ){
          if( bFound==0 ){
            pSeg->nEntryTombstone++;
            bFound = 1;
          }
          fts5IndexTombstoneAdd(p, pSeg, iRowid);
        }
      }
    }
    fts5StructureRelease(pStruct);
  }
  return fts5IndexReturn(p);
}

/*************************************************************************
**************************************************************************
** Below this point is the implementation of the integrity-check
** functionality.
*/

/*
** Return a simple checksum value based on the arguments.
*/
static u64 sqlite3Fts5IndexEntryCksum(
  i64 iRowid,
  int iCol,
  int iPos,
  int iIdx,
  const char *pTerm,
  int nTerm
){
  int i;
  u64 ret = iRowid;
  ret += (ret<<3) + iCol;
  ret += (ret<<3) + iPos;
  if( iIdx>=0 ) ret += (ret<<3) + (FTS5_MAIN_PREFIX + iIdx);
  for(i=0; i<nTerm; i++) ret += (ret<<3) + pTerm[i];
  return ret;
}
