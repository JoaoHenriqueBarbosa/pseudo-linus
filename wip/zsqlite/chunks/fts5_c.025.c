
/*
** The only argument points to a buffer at least 4 bytes in size. This
** function interprets the first 4 bytes of the buffer as a 32-bit big-endian
** unsigned integer and returns the result.
*/
static u32 fts5GetU32(const u8 *a){
  return ((u32)a[0] << 24)
       + ((u32)a[1] << 16)
       + ((u32)a[2] << 8)
       + ((u32)a[3] << 0);
}

/*
** Write iVal, formated as a 64-bit big-endian unsigned integer, to the
** buffer indicated by the first argument.
*/
static void fts5PutU64(u8 *a, u64 iVal){
  a[0] = ((iVal >> 56) & 0xFF);
  a[1] = ((iVal >> 48) & 0xFF);
  a[2] = ((iVal >> 40) & 0xFF);
  a[3] = ((iVal >> 32) & 0xFF);
  a[4] = ((iVal >> 24) & 0xFF);
  a[5] = ((iVal >> 16) & 0xFF);
  a[6] = ((iVal >>  8) & 0xFF);
  a[7] = ((iVal >>  0) & 0xFF);
}

/*
** Write iVal, formated as a 32-bit big-endian unsigned integer, to the
** buffer indicated by the first argument.
*/
static void fts5PutU32(u8 *a, u32 iVal){
  a[0] = ((iVal >> 24) & 0xFF);
  a[1] = ((iVal >> 16) & 0xFF);
  a[2] = ((iVal >>  8) & 0xFF);
  a[3] = ((iVal >>  0) & 0xFF);
}

/*
** Allocate and return a buffer at least nByte bytes in size.
**
** If an OOM error is encountered, return NULL and set the error code in
** the Fts5Index handle passed as the first argument.
*/
static void *fts5IdxMalloc(Fts5Index *p, sqlite3_int64 nByte){
  return sqlite3Fts5MallocZero(&p->rc, nByte);
}

/*
** Compare the contents of the pLeft buffer with the pRight/nRight blob.
**
** Return -ve if pLeft is smaller than pRight, 0 if they are equal or
** +ve if pRight is smaller than pLeft. In other words:
**
**     res = *pLeft - *pRight
*/
#ifdef SQLITE_DEBUG
static int fts5BufferCompareBlob(
  Fts5Buffer *pLeft,              /* Left hand side of comparison */
  const u8 *pRight, int nRight    /* Right hand side of comparison */
){
  int nCmp = MIN(pLeft->n, nRight);
  int res = memcmp(pLeft->p, pRight, nCmp);
  return (res==0 ? (pLeft->n - nRight) : res);
}
#endif

/*
** Compare the contents of the two buffers using memcmp(). If one buffer
** is a prefix of the other, it is considered the lesser.
**
** Return -ve if pLeft is smaller than pRight, 0 if they are equal or
** +ve if pRight is smaller than pLeft. In other words:
**
**     res = *pLeft - *pRight
*/
static int fts5BufferCompare(Fts5Buffer *pLeft, Fts5Buffer *pRight){
  int nCmp, res;
  nCmp = MIN(pLeft->n, pRight->n);
  assert( nCmp<=0 || pLeft->p!=0 );
  assert( nCmp<=0 || pRight->p!=0 );
  res = fts5Memcmp(pLeft->p, pRight->p, nCmp);
  return (res==0 ? (pLeft->n - pRight->n) : res);
}

static int fts5LeafFirstTermOff(Fts5Data *pLeaf){
  int ret;
  fts5GetVarint32(&pLeaf->p[pLeaf->szLeaf], ret);
  return ret;
}

/*
** Close the read-only blob handle, if it is open.
*/
static void sqlite3Fts5IndexCloseReader(Fts5Index *p){
  if( p->pReader ){
    sqlite3_blob *pReader = p->pReader;
    p->pReader = 0;
    sqlite3_blob_close(pReader);
  }
}

/*
** Retrieve a record from the %_data table.
**
** If an error occurs, NULL is returned and an error left in the
** Fts5Index object.
*/
static Fts5Data *fts5DataRead(Fts5Index *p, i64 iRowid){
  Fts5Data *pRet = 0;
  if( p->rc==SQLITE_OK ){
    int rc = SQLITE_OK;

    if( p->pReader ){
      /* This call may return SQLITE_ABORT if there has been a savepoint
      ** rollback since it was last used. In this case a new blob handle
      ** is required.  */
      sqlite3_blob *pBlob = p->pReader;
      p->pReader = 0;
      rc = sqlite3_blob_reopen(pBlob, iRowid);
      assert( p->pReader==0 );
      p->pReader = pBlob;
      if( rc!=SQLITE_OK ){
        sqlite3Fts5IndexCloseReader(p);
      }
      if( rc==SQLITE_ABORT ) rc = SQLITE_OK;
    }

    /* If the blob handle is not open at this point, open it and seek
    ** to the requested entry.  */
    if( p->pReader==0 && rc==SQLITE_OK ){
      Fts5Config *pConfig = p->pConfig;
      rc = sqlite3_blob_open(pConfig->db,
          pConfig->zDb, p->zDataTbl, "block", iRowid, 0, &p->pReader
      );
    }

    /* If either of the sqlite3_blob_open() or sqlite3_blob_reopen() calls
    ** above returned SQLITE_ERROR, return SQLITE_CORRUPT_VTAB instead.
    ** All the reasons those functions might return SQLITE_ERROR - missing
    ** table, missing row, non-blob/text in block column - indicate
    ** backing store corruption.  */
    if( rc==SQLITE_ERROR ) rc = FTS5_CORRUPT;

    if( rc==SQLITE_OK ){
      u8 *aOut = 0;               /* Read blob data into this buffer */
      int nByte = sqlite3_blob_bytes(p->pReader);
      sqlite3_int64 nAlloc = sizeof(Fts5Data) + nByte + FTS5_DATA_PADDING;
      pRet = (Fts5Data*)sqlite3_malloc64(nAlloc);
      if( pRet ){
        pRet->nn = nByte;
        aOut = pRet->p = (u8*)&pRet[1];
      }else{
        rc = SQLITE_NOMEM;
      }

      if( rc==SQLITE_OK ){
        rc = sqlite3_blob_read(p->pReader, aOut, nByte, 0);
      }
      if( rc!=SQLITE_OK ){
        sqlite3_free(pRet);
        pRet = 0;
      }else{
        /* TODO1: Fix this */
        pRet->p[nByte] = 0x00;
        pRet->p[nByte+1] = 0x00;
        pRet->szLeaf = fts5GetU16(&pRet->p[2]);
      }
    }
    p->rc = rc;
    p->nRead++;
  }

  assert( (pRet==0)==(p->rc!=SQLITE_OK) );
  return pRet;
}


/*
** Release a reference to data record returned by an earlier call to
** fts5DataRead().
*/
static void fts5DataRelease(Fts5Data *pData){
  sqlite3_free(pData);
}

static Fts5Data *fts5LeafRead(Fts5Index *p, i64 iRowid){
  Fts5Data *pRet = fts5DataRead(p, iRowid);
  if( pRet ){
    if( pRet->nn<4 || pRet->szLeaf>pRet->nn ){
      p->rc = FTS5_CORRUPT;
      fts5DataRelease(pRet);
      pRet = 0;
    }
  }
  return pRet;
}

static int fts5IndexPrepareStmt(
  Fts5Index *p,
  sqlite3_stmt **ppStmt,
  char *zSql
){
  if( p->rc==SQLITE_OK ){
    if( zSql ){
      p->rc = sqlite3_prepare_v3(p->pConfig->db, zSql, -1,
          SQLITE_PREPARE_PERSISTENT|SQLITE_PREPARE_NO_VTAB,
          ppStmt, 0);
    }else{
      p->rc = SQLITE_NOMEM;
    }
  }
  sqlite3_free(zSql);
  return p->rc;
}


/*
** INSERT OR REPLACE a record into the %_data table.
*/
static void fts5DataWrite(Fts5Index *p, i64 iRowid, const u8 *pData, int nData){
  if( p->rc!=SQLITE_OK ) return;

  if( p->pWriter==0 ){
    Fts5Config *pConfig = p->pConfig;
    fts5IndexPrepareStmt(p, &p->pWriter, sqlite3_mprintf(
          "REPLACE INTO '%q'.'%q_data'(id, block) VALUES(?,?)",
          pConfig->zDb, pConfig->zName
    ));
    if( p->rc ) return;
  }

  sqlite3_bind_int64(p->pWriter, 1, iRowid);
  sqlite3_bind_blob(p->pWriter, 2, pData, nData, SQLITE_STATIC);
  sqlite3_step(p->pWriter);
  p->rc = sqlite3_reset(p->pWriter);
  sqlite3_bind_null(p->pWriter, 2);
}

/*
** Execute the following SQL:
**
**     DELETE FROM %_data WHERE id BETWEEN $iFirst AND $iLast
*/
static void fts5DataDelete(Fts5Index *p, i64 iFirst, i64 iLast){
  if( p->rc!=SQLITE_OK ) return;

  if( p->pDeleter==0 ){
    Fts5Config *pConfig = p->pConfig;
    char *zSql = sqlite3_mprintf(
        "DELETE FROM '%q'.'%q_data' WHERE id>=? AND id<=?",
          pConfig->zDb, pConfig->zName
    );
    if( fts5IndexPrepareStmt(p, &p->pDeleter, zSql) ) return;
  }

  sqlite3_bind_int64(p->pDeleter, 1, iFirst);
  sqlite3_bind_int64(p->pDeleter, 2, iLast);
  sqlite3_step(p->pDeleter);
  p->rc = sqlite3_reset(p->pDeleter);
}

/*
** Remove all records associated with segment iSegid.
*/
static void fts5DataRemoveSegment(Fts5Index *p, Fts5StructureSegment *pSeg){
  int iSegid = pSeg->iSegid;
  i64 iFirst = FTS5_SEGMENT_ROWID(iSegid, 0);
  i64 iLast = FTS5_SEGMENT_ROWID(iSegid+1, 0)-1;
  fts5DataDelete(p, iFirst, iLast);

  if( pSeg->nPgTombstone ){
    i64 iTomb1 = FTS5_TOMBSTONE_ROWID(iSegid, 0);
    i64 iTomb2 = FTS5_TOMBSTONE_ROWID(iSegid, pSeg->nPgTombstone-1);
    fts5DataDelete(p, iTomb1, iTomb2);
  }
  if( p->pIdxDeleter==0 ){
    Fts5Config *pConfig = p->pConfig;
    fts5IndexPrepareStmt(p, &p->pIdxDeleter, sqlite3_mprintf(
          "DELETE FROM '%q'.'%q_idx' WHERE segid=?",
          pConfig->zDb, pConfig->zName
    ));
  }
  if( p->rc==SQLITE_OK ){
    sqlite3_bind_int(p->pIdxDeleter, 1, iSegid);
    sqlite3_step(p->pIdxDeleter);
    p->rc = sqlite3_reset(p->pIdxDeleter);
  }
}

/*
** Release a reference to an Fts5Structure object returned by an earlier
** call to fts5StructureRead() or fts5StructureDecode().
*/
static void fts5StructureRelease(Fts5Structure *pStruct){
  if( pStruct && 0>=(--pStruct->nRef) ){
    int i;
    assert( pStruct->nRef==0 );
    for(i=0; i<pStruct->nLevel; i++){
      sqlite3_free(pStruct->aLevel[i].aSeg);
    }
    sqlite3_free(pStruct);
  }
}

static void fts5StructureRef(Fts5Structure *pStruct){
  pStruct->nRef++;
}

static void *sqlite3Fts5StructureRef(Fts5Index *p){
  fts5StructureRef(p->pStruct);
  return (void*)p->pStruct;
}
static void sqlite3Fts5StructureRelease(void *p){
  if( p ){
    fts5StructureRelease((Fts5Structure*)p);
  }
}
static int sqlite3Fts5StructureTest(Fts5Index *p, void *pStruct){
  if( p->pStruct!=(Fts5Structure*)pStruct ){
    return SQLITE_ABORT;
  }
  return SQLITE_OK;
}

/*
** Ensure that structure object (*pp) is writable.
**
** This function is a no-op if (*pRc) is not SQLITE_OK when it is called. If
** an error occurs, (*pRc) is set to an SQLite error code before returning.
*/
static void fts5StructureMakeWritable(int *pRc, Fts5Structure **pp){
  Fts5Structure *p = *pp;
  if( *pRc==SQLITE_OK && p->nRef>1 ){
    i64 nByte = sizeof(Fts5Structure)+(p->nLevel-1)*sizeof(Fts5StructureLevel);
    Fts5Structure *pNew;
    pNew = (Fts5Structure*)sqlite3Fts5MallocZero(pRc, nByte);
    if( pNew ){
      int i;
      memcpy(pNew, p, nByte);
      for(i=0; i<p->nLevel; i++) pNew->aLevel[i].aSeg = 0;
      for(i=0; i<p->nLevel; i++){
        Fts5StructureLevel *pLvl = &pNew->aLevel[i];
        nByte = sizeof(Fts5StructureSegment) * pNew->aLevel[i].nSeg;
        pLvl->aSeg = (Fts5StructureSegment*)sqlite3Fts5MallocZero(pRc, nByte);
        if( pLvl->aSeg==0 ){
          for(i=0; i<p->nLevel; i++){
            sqlite3_free(pNew->aLevel[i].aSeg);
          }
          sqlite3_free(pNew);
          return;
        }
        memcpy(pLvl->aSeg, p->aLevel[i].aSeg, nByte);
      }
      p->nRef--;
      pNew->nRef = 1;
    }
    *pp = pNew;
  }
}
