
/*
** Deserialize and return the structure record currently stored in serialized
** form within buffer pData/nData.
**
** The Fts5Structure.aLevel[] and each Fts5StructureLevel.aSeg[] array
** are over-allocated by one slot. This allows the structure contents
** to be more easily edited.
**
** If an error occurs, *ppOut is set to NULL and an SQLite error code
** returned. Otherwise, *ppOut is set to point to the new object and
** SQLITE_OK returned.
*/
static int fts5StructureDecode(
  const u8 *pData,                /* Buffer containing serialized structure */
  int nData,                      /* Size of buffer pData in bytes */
  int *piCookie,                  /* Configuration cookie value */
  Fts5Structure **ppOut           /* OUT: Deserialized object */
){
  int rc = SQLITE_OK;
  int i = 0;
  int iLvl;
  int nLevel = 0;
  int nSegment = 0;
  sqlite3_int64 nByte;            /* Bytes of space to allocate at pRet */
  Fts5Structure *pRet = 0;        /* Structure object to return */
  int bStructureV2 = 0;           /* True for FTS5_STRUCTURE_V2 */
  u64 nOriginCntr = 0;            /* Largest origin value seen so far */

  /* Grab the cookie value */
  if( piCookie ) *piCookie = sqlite3Fts5Get32(pData);
  i = 4;

  /* Check if this is a V2 structure record. Set bStructureV2 if it is. */
  if( 0==memcmp(&pData[i], FTS5_STRUCTURE_V2, 4) ){
    i += 4;
    bStructureV2 = 1;
  }

  /* Read the total number of levels and segments from the start of the
  ** structure record.  */
  i += fts5GetVarint32(&pData[i], nLevel);
  i += fts5GetVarint32(&pData[i], nSegment);
  if( nLevel>FTS5_MAX_SEGMENT   || nLevel<0
   || nSegment>FTS5_MAX_SEGMENT || nSegment<0
  ){
    return FTS5_CORRUPT;
  }
  nByte = (
      sizeof(Fts5Structure) +                    /* Main structure */
      sizeof(Fts5StructureLevel) * (nLevel-1)    /* aLevel[] array */
  );
  pRet = (Fts5Structure*)sqlite3Fts5MallocZero(&rc, nByte);

  if( pRet ){
    pRet->nRef = 1;
    pRet->nLevel = nLevel;
    pRet->nSegment = nSegment;
    i += sqlite3Fts5GetVarint(&pData[i], &pRet->nWriteCounter);

    for(iLvl=0; rc==SQLITE_OK && iLvl<nLevel; iLvl++){
      Fts5StructureLevel *pLvl = &pRet->aLevel[iLvl];
      int nTotal = 0;
      int iSeg;

      if( i>=nData ){
        rc = FTS5_CORRUPT;
      }else{
        i += fts5GetVarint32(&pData[i], pLvl->nMerge);
        i += fts5GetVarint32(&pData[i], nTotal);
        if( nTotal<pLvl->nMerge ) rc = FTS5_CORRUPT;
        pLvl->aSeg = (Fts5StructureSegment*)sqlite3Fts5MallocZero(&rc,
            nTotal * sizeof(Fts5StructureSegment)
        );
        nSegment -= nTotal;
      }

      if( rc==SQLITE_OK ){
        pLvl->nSeg = nTotal;
        for(iSeg=0; iSeg<nTotal; iSeg++){
          Fts5StructureSegment *pSeg = &pLvl->aSeg[iSeg];
          if( i>=nData ){
            rc = FTS5_CORRUPT;
            break;
          }
          assert( pSeg!=0 );
          i += fts5GetVarint32(&pData[i], pSeg->iSegid);
          i += fts5GetVarint32(&pData[i], pSeg->pgnoFirst);
          i += fts5GetVarint32(&pData[i], pSeg->pgnoLast);
          if( bStructureV2 ){
            i += fts5GetVarint(&pData[i], &pSeg->iOrigin1);
            i += fts5GetVarint(&pData[i], &pSeg->iOrigin2);
            i += fts5GetVarint32(&pData[i], pSeg->nPgTombstone);
            i += fts5GetVarint(&pData[i], &pSeg->nEntryTombstone);
            i += fts5GetVarint(&pData[i], &pSeg->nEntry);
            nOriginCntr = MAX(nOriginCntr, pSeg->iOrigin2);
          }
          if( pSeg->pgnoLast<pSeg->pgnoFirst ){
            rc = FTS5_CORRUPT;
            break;
          }
        }
        if( iLvl>0 && pLvl[-1].nMerge && nTotal==0 ) rc = FTS5_CORRUPT;
        if( iLvl==nLevel-1 && pLvl->nMerge ) rc = FTS5_CORRUPT;
      }
    }
    if( nSegment!=0 && rc==SQLITE_OK ) rc = FTS5_CORRUPT;
    if( bStructureV2 ){
      pRet->nOriginCntr = nOriginCntr+1;
    }

    if( rc!=SQLITE_OK ){
      fts5StructureRelease(pRet);
      pRet = 0;
    }
  }

  *ppOut = pRet;
  return rc;
}

/*
** Add a level to the Fts5Structure.aLevel[] array of structure object
** (*ppStruct).
*/
static void fts5StructureAddLevel(int *pRc, Fts5Structure **ppStruct){
  fts5StructureMakeWritable(pRc, ppStruct);
  assert( (ppStruct!=0 && (*ppStruct)!=0) || (*pRc)!=SQLITE_OK );
  if( *pRc==SQLITE_OK ){
    Fts5Structure *pStruct = *ppStruct;
    int nLevel = pStruct->nLevel;
    sqlite3_int64 nByte = (
        sizeof(Fts5Structure) +                  /* Main structure */
        sizeof(Fts5StructureLevel) * (nLevel+1)  /* aLevel[] array */
    );

    pStruct = sqlite3_realloc64(pStruct, nByte);
    if( pStruct ){
      memset(&pStruct->aLevel[nLevel], 0, sizeof(Fts5StructureLevel));
      pStruct->nLevel++;
      *ppStruct = pStruct;
    }else{
      *pRc = SQLITE_NOMEM;
    }
  }
}

/*
** Extend level iLvl so that there is room for at least nExtra more
** segments.
*/
static void fts5StructureExtendLevel(
  int *pRc,
  Fts5Structure *pStruct,
  int iLvl,
  int nExtra,
  int bInsert
){
  if( *pRc==SQLITE_OK ){
    Fts5StructureLevel *pLvl = &pStruct->aLevel[iLvl];
    Fts5StructureSegment *aNew;
    sqlite3_int64 nByte;

    nByte = (pLvl->nSeg + nExtra) * sizeof(Fts5StructureSegment);
    aNew = sqlite3_realloc64(pLvl->aSeg, nByte);
    if( aNew ){
      if( bInsert==0 ){
        memset(&aNew[pLvl->nSeg], 0, sizeof(Fts5StructureSegment) * nExtra);
      }else{
        int nMove = pLvl->nSeg * sizeof(Fts5StructureSegment);
        memmove(&aNew[nExtra], aNew, nMove);
        memset(aNew, 0, sizeof(Fts5StructureSegment) * nExtra);
      }
      pLvl->aSeg = aNew;
    }else{
      *pRc = SQLITE_NOMEM;
    }
  }
}

static Fts5Structure *fts5StructureReadUncached(Fts5Index *p){
  Fts5Structure *pRet = 0;
  Fts5Config *pConfig = p->pConfig;
  int iCookie;                    /* Configuration cookie */
  Fts5Data *pData;

  pData = fts5DataRead(p, FTS5_STRUCTURE_ROWID);
  if( p->rc==SQLITE_OK ){
    /* TODO: Do we need this if the leaf-index is appended? Probably... */
    memset(&pData->p[pData->nn], 0, FTS5_DATA_PADDING);
    p->rc = fts5StructureDecode(pData->p, pData->nn, &iCookie, &pRet);
    if( p->rc==SQLITE_OK && (pConfig->pgsz==0 || pConfig->iCookie!=iCookie) ){
      p->rc = sqlite3Fts5ConfigLoad(pConfig, iCookie);
    }
    fts5DataRelease(pData);
    if( p->rc!=SQLITE_OK ){
      fts5StructureRelease(pRet);
      pRet = 0;
    }
  }

  return pRet;
}

static i64 fts5IndexDataVersion(Fts5Index *p){
  i64 iVersion = 0;

  if( p->rc==SQLITE_OK ){
    if( p->pDataVersion==0 ){
      p->rc = fts5IndexPrepareStmt(p, &p->pDataVersion,
          sqlite3_mprintf("PRAGMA %Q.data_version", p->pConfig->zDb)
          );
      if( p->rc ) return 0;
    }

    if( SQLITE_ROW==sqlite3_step(p->pDataVersion) ){
      iVersion = sqlite3_column_int64(p->pDataVersion, 0);
    }
    p->rc = sqlite3_reset(p->pDataVersion);
  }

  return iVersion;
}

/*
** Read, deserialize and return the structure record.
**
** The Fts5Structure.aLevel[] and each Fts5StructureLevel.aSeg[] array
** are over-allocated as described for function fts5StructureDecode()
** above.
**
** If an error occurs, NULL is returned and an error code left in the
** Fts5Index handle. If an error has already occurred when this function
** is called, it is a no-op.
*/
static Fts5Structure *fts5StructureRead(Fts5Index *p){

  if( p->pStruct==0 ){
    p->iStructVersion = fts5IndexDataVersion(p);
    if( p->rc==SQLITE_OK ){
      p->pStruct = fts5StructureReadUncached(p);
    }
  }

#if 0
  else{
    Fts5Structure *pTest = fts5StructureReadUncached(p);
    if( pTest ){
      int i, j;
      assert_nc( p->pStruct->nSegment==pTest->nSegment );
      assert_nc( p->pStruct->nLevel==pTest->nLevel );
      for(i=0; i<pTest->nLevel; i++){
        assert_nc( p->pStruct->aLevel[i].nMerge==pTest->aLevel[i].nMerge );
        assert_nc( p->pStruct->aLevel[i].nSeg==pTest->aLevel[i].nSeg );
        for(j=0; j<pTest->aLevel[i].nSeg; j++){
          Fts5StructureSegment *p1 = &pTest->aLevel[i].aSeg[j];
          Fts5StructureSegment *p2 = &p->pStruct->aLevel[i].aSeg[j];
          assert_nc( p1->iSegid==p2->iSegid );
          assert_nc( p1->pgnoFirst==p2->pgnoFirst );
          assert_nc( p1->pgnoLast==p2->pgnoLast );
        }
      }
      fts5StructureRelease(pTest);
    }
  }
#endif

  if( p->rc!=SQLITE_OK ) return 0;
  assert( p->iStructVersion!=0 );
  assert( p->pStruct!=0 );
  fts5StructureRef(p->pStruct);
  return p->pStruct;
}

static void fts5StructureInvalidate(Fts5Index *p){
  if( p->pStruct ){
    fts5StructureRelease(p->pStruct);
    p->pStruct = 0;
  }
}

/*
** Return the total number of segments in index structure pStruct. This
** function is only ever used as part of assert() conditions.
*/
#ifdef SQLITE_DEBUG
static int fts5StructureCountSegments(Fts5Structure *pStruct){
  int nSegment = 0;               /* Total number of segments */
  if( pStruct ){
    int iLvl;                     /* Used to iterate through levels */
    for(iLvl=0; iLvl<pStruct->nLevel; iLvl++){
      nSegment += pStruct->aLevel[iLvl].nSeg;
    }
  }

  return nSegment;
}
#endif

#define fts5BufferSafeAppendBlob(pBuf, pBlob, nBlob) {     \
  assert( (pBuf)->nSpace>=((pBuf)->n+nBlob) );             \
  memcpy(&(pBuf)->p[(pBuf)->n], pBlob, nBlob);             \
  (pBuf)->n += nBlob;                                      \
}

#define fts5BufferSafeAppendVarint(pBuf, iVal) {                \
  (pBuf)->n += sqlite3Fts5PutVarint(&(pBuf)->p[(pBuf)->n], (iVal));  \
  assert( (pBuf)->nSpace>=(pBuf)->n );                          \
}


/*
** Serialize and store the "structure" record.
**
** If an error occurs, leave an error code in the Fts5Index object. If an
** error has already occurred, this function is a no-op.
*/
static void fts5StructureWrite(Fts5Index *p, Fts5Structure *pStruct){
  if( p->rc==SQLITE_OK ){
    Fts5Buffer buf;               /* Buffer to serialize record into */
    int iLvl;                     /* Used to iterate through levels */
    int iCookie;                  /* Cookie value to store */
    int nHdr = (pStruct->nOriginCntr>0 ? (4+4+9+9+9) : (4+9+9));

    assert( pStruct->nSegment==fts5StructureCountSegments(pStruct) );
    memset(&buf, 0, sizeof(Fts5Buffer));

    /* Append the current configuration cookie */
    iCookie = p->pConfig->iCookie;
    if( iCookie<0 ) iCookie = 0;

    if( 0==sqlite3Fts5BufferSize(&p->rc, &buf, nHdr) ){
      sqlite3Fts5Put32(buf.p, iCookie);
      buf.n = 4;
      if( pStruct->nOriginCntr>0 ){
        fts5BufferSafeAppendBlob(&buf, FTS5_STRUCTURE_V2, 4);
      }
      fts5BufferSafeAppendVarint(&buf, pStruct->nLevel);
      fts5BufferSafeAppendVarint(&buf, pStruct->nSegment);
      fts5BufferSafeAppendVarint(&buf, (i64)pStruct->nWriteCounter);
    }

    for(iLvl=0; iLvl<pStruct->nLevel; iLvl++){
      int iSeg;                     /* Used to iterate through segments */
      Fts5StructureLevel *pLvl = &pStruct->aLevel[iLvl];
      fts5BufferAppendVarint(&p->rc, &buf, pLvl->nMerge);
      fts5BufferAppendVarint(&p->rc, &buf, pLvl->nSeg);
      assert( pLvl->nMerge<=pLvl->nSeg );

      for(iSeg=0; iSeg<pLvl->nSeg; iSeg++){
        Fts5StructureSegment *pSeg = &pLvl->aSeg[iSeg];
        fts5BufferAppendVarint(&p->rc, &buf, pSeg->iSegid);
        fts5BufferAppendVarint(&p->rc, &buf, pSeg->pgnoFirst);
        fts5BufferAppendVarint(&p->rc, &buf, pSeg->pgnoLast);
        if( pStruct->nOriginCntr>0 ){
          fts5BufferAppendVarint(&p->rc, &buf, pSeg->iOrigin1);
          fts5BufferAppendVarint(&p->rc, &buf, pSeg->iOrigin2);
          fts5BufferAppendVarint(&p->rc, &buf, pSeg->nPgTombstone);
          fts5BufferAppendVarint(&p->rc, &buf, pSeg->nEntryTombstone);
          fts5BufferAppendVarint(&p->rc, &buf, pSeg->nEntry);
        }
      }
    }

    fts5DataWrite(p, FTS5_STRUCTURE_ROWID, buf.p, buf.n);
    fts5BufferFree(&buf);
  }
}
