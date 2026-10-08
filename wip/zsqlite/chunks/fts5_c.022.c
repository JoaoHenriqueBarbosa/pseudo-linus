
/*
** Does the work of the fts5_api.xQueryToken() API method.
*/
static int sqlite3Fts5ExprQueryToken(
  Fts5Expr *pExpr,
  int iPhrase,
  int iToken,
  const char **ppOut,
  int *pnOut
){
  Fts5ExprPhrase *pPhrase = 0;

  if( iPhrase<0 || iPhrase>=pExpr->nPhrase ){
    return SQLITE_RANGE;
  }
  pPhrase = pExpr->apExprPhrase[iPhrase];
  if( iToken<0 || iToken>=pPhrase->nTerm ){
    return SQLITE_RANGE;
  }

  *ppOut = pPhrase->aTerm[iToken].pTerm;
  *pnOut = pPhrase->aTerm[iToken].nFullTerm;
  return SQLITE_OK;
}

/*
** Does the work of the fts5_api.xInstToken() API method.
*/
static int sqlite3Fts5ExprInstToken(
  Fts5Expr *pExpr,
  i64 iRowid,
  int iPhrase,
  int iCol,
  int iOff,
  int iToken,
  const char **ppOut,
  int *pnOut
){
  Fts5ExprPhrase *pPhrase = 0;
  Fts5ExprTerm *pTerm = 0;
  int rc = SQLITE_OK;

  if( iPhrase<0 || iPhrase>=pExpr->nPhrase ){
    return SQLITE_RANGE;
  }
  pPhrase = pExpr->apExprPhrase[iPhrase];
  if( iToken<0 || iToken>=pPhrase->nTerm ){
    return SQLITE_RANGE;
  }
  pTerm = &pPhrase->aTerm[iToken];
  if( pTerm->bPrefix==0 ){
    if( pExpr->pConfig->bTokendata ){
      rc = sqlite3Fts5IterToken(
          pTerm->pIter, iRowid, iCol, iOff+iToken, ppOut, pnOut
      );
    }else{
      *ppOut = pTerm->pTerm;
      *pnOut = pTerm->nFullTerm;
    }
  }
  return rc;
}

/*
** Clear the token mappings for all Fts5IndexIter objects mannaged by
** the expression passed as the only argument.
*/
static void sqlite3Fts5ExprClearTokens(Fts5Expr *pExpr){
  int ii;
  for(ii=0; ii<pExpr->nPhrase; ii++){
    Fts5ExprTerm *pT;
    for(pT=&pExpr->apExprPhrase[ii]->aTerm[0]; pT; pT=pT->pSynonym){
      sqlite3Fts5IndexIterClearTokendata(pT->pIter);
    }
  }
}

/*
** 2014 August 11
**
** The author disclaims copyright to this source code.  In place of
** a legal notice, here is a blessing:
**
**    May you do good and not evil.
**    May you find forgiveness for yourself and forgive others.
**    May you share freely, never taking more than you give.
**
******************************************************************************
**
*/



/* #include "fts5Int.h" */

typedef struct Fts5HashEntry Fts5HashEntry;

/*
** This file contains the implementation of an in-memory hash table used
** to accumuluate "term -> doclist" content before it is flused to a level-0
** segment.
*/


struct Fts5Hash {
  int eDetail;                    /* Copy of Fts5Config.eDetail */
  int *pnByte;                    /* Pointer to bytes counter */
  int nEntry;                     /* Number of entries currently in hash */
  int nSlot;                      /* Size of aSlot[] array */
  Fts5HashEntry *pScan;           /* Current ordered scan item */
  Fts5HashEntry **aSlot;          /* Array of hash slots */
};

/*
** Each entry in the hash table is represented by an object of the
** following type. Each object, its key, and its current data are stored
** in a single memory allocation. The key immediately follows the object
** in memory. The position list data immediately follows the key data
** in memory.
**
** The key is Fts5HashEntry.nKey bytes in size. It consists of a single
** byte identifying the index (either the main term index or a prefix-index),
** followed by the term data. For example: "0token". There is no
** nul-terminator - in this case nKey=6.
**
** The data that follows the key is in a similar, but not identical format
** to the doclist data stored in the database. It is:
**
**   * Rowid, as a varint
**   * Position list, without 0x00 terminator.
**   * Size of previous position list and rowid, as a 4 byte
**     big-endian integer.
**
** iRowidOff:
**   Offset of last rowid written to data area. Relative to first byte of
**   structure.
**
** nData:
**   Bytes of data written since iRowidOff.
*/
struct Fts5HashEntry {
  Fts5HashEntry *pHashNext;       /* Next hash entry with same hash-key */
  Fts5HashEntry *pScanNext;       /* Next entry in sorted order */

  int nAlloc;                     /* Total size of allocation */
  int iSzPoslist;                 /* Offset of space for 4-byte poslist size */
  int nData;                      /* Total bytes of data (incl. structure) */
  int nKey;                       /* Length of key in bytes */
  u8 bDel;                        /* Set delete-flag @ iSzPoslist */
  u8 bContent;                    /* Set content-flag (detail=none mode) */
  i16 iCol;                       /* Column of last value written */
  int iPos;                       /* Position of last value written */
  i64 iRowid;                     /* Rowid of last value written */
};

/*
** Eqivalent to:
**
**   char *fts5EntryKey(Fts5HashEntry *pEntry){ return zKey; }
*/
#define fts5EntryKey(p) ( ((char *)(&(p)[1])) )


/*
** Allocate a new hash table.
*/
static int sqlite3Fts5HashNew(Fts5Config *pConfig, Fts5Hash **ppNew, int *pnByte){
  int rc = SQLITE_OK;
  Fts5Hash *pNew;

  *ppNew = pNew = (Fts5Hash*)sqlite3_malloc(sizeof(Fts5Hash));
  if( pNew==0 ){
    rc = SQLITE_NOMEM;
  }else{
    sqlite3_int64 nByte;
    memset(pNew, 0, sizeof(Fts5Hash));
    pNew->pnByte = pnByte;
    pNew->eDetail = pConfig->eDetail;

    pNew->nSlot = 1024;
    nByte = sizeof(Fts5HashEntry*) * pNew->nSlot;
    pNew->aSlot = (Fts5HashEntry**)sqlite3_malloc64(nByte);
    if( pNew->aSlot==0 ){
      sqlite3_free(pNew);
      *ppNew = 0;
      rc = SQLITE_NOMEM;
    }else{
      memset(pNew->aSlot, 0, (size_t)nByte);
    }
  }
  return rc;
}

/*
** Free a hash table object.
*/
static void sqlite3Fts5HashFree(Fts5Hash *pHash){
  if( pHash ){
    sqlite3Fts5HashClear(pHash);
    sqlite3_free(pHash->aSlot);
    sqlite3_free(pHash);
  }
}

/*
** Empty (but do not delete) a hash table.
*/
static void sqlite3Fts5HashClear(Fts5Hash *pHash){
  int i;
  for(i=0; i<pHash->nSlot; i++){
    Fts5HashEntry *pNext;
    Fts5HashEntry *pSlot;
    for(pSlot=pHash->aSlot[i]; pSlot; pSlot=pNext){
      pNext = pSlot->pHashNext;
      sqlite3_free(pSlot);
    }
  }
  memset(pHash->aSlot, 0, pHash->nSlot * sizeof(Fts5HashEntry*));
  pHash->nEntry = 0;
}

static unsigned int fts5HashKey(int nSlot, const u8 *p, int n){
  int i;
  unsigned int h = 13;
  for(i=n-1; i>=0; i--){
    h = (h << 3) ^ h ^ p[i];
  }
  return (h % nSlot);
}

static unsigned int fts5HashKey2(int nSlot, u8 b, const u8 *p, int n){
  int i;
  unsigned int h = 13;
  for(i=n-1; i>=0; i--){
    h = (h << 3) ^ h ^ p[i];
  }
  h = (h << 3) ^ h ^ b;
  return (h % nSlot);
}

/*
** Resize the hash table by doubling the number of slots.
*/
static int fts5HashResize(Fts5Hash *pHash){
  int nNew = pHash->nSlot*2;
  int i;
  Fts5HashEntry **apNew;
  Fts5HashEntry **apOld = pHash->aSlot;

  apNew = (Fts5HashEntry**)sqlite3_malloc64(nNew*sizeof(Fts5HashEntry*));
  if( !apNew ) return SQLITE_NOMEM;
  memset(apNew, 0, nNew*sizeof(Fts5HashEntry*));

  for(i=0; i<pHash->nSlot; i++){
    while( apOld[i] ){
      unsigned int iHash;
      Fts5HashEntry *p = apOld[i];
      apOld[i] = p->pHashNext;
      iHash = fts5HashKey(nNew, (u8*)fts5EntryKey(p), p->nKey);
      p->pHashNext = apNew[iHash];
      apNew[iHash] = p;
    }
  }

  sqlite3_free(apOld);
  pHash->nSlot = nNew;
  pHash->aSlot = apNew;
  return SQLITE_OK;
}

static int fts5HashAddPoslistSize(
  Fts5Hash *pHash,
  Fts5HashEntry *p,
  Fts5HashEntry *p2
){
  int nRet = 0;
  if( p->iSzPoslist ){
    u8 *pPtr = p2 ? (u8*)p2 : (u8*)p;
    int nData = p->nData;
    if( pHash->eDetail==FTS5_DETAIL_NONE ){
      assert( nData==p->iSzPoslist );
      if( p->bDel ){
        pPtr[nData++] = 0x00;
        if( p->bContent ){
          pPtr[nData++] = 0x00;
        }
      }
    }else{
      int nSz = (nData - p->iSzPoslist - 1);       /* Size in bytes */
      int nPos = nSz*2 + p->bDel;                     /* Value of nPos field */

      assert( p->bDel==0 || p->bDel==1 );
      if( nPos<=127 ){
        pPtr[p->iSzPoslist] = (u8)nPos;
      }else{
        int nByte = sqlite3Fts5GetVarintLen((u32)nPos);
        memmove(&pPtr[p->iSzPoslist + nByte], &pPtr[p->iSzPoslist + 1], nSz);
        sqlite3Fts5PutVarint(&pPtr[p->iSzPoslist], nPos);
        nData += (nByte-1);
      }
    }

    nRet = nData - p->nData;
    if( p2==0 ){
      p->iSzPoslist = 0;
      p->bDel = 0;
      p->bContent = 0;
      p->nData = nData;
    }
  }
  return nRet;
}

/*
** Add an entry to the in-memory hash table. The key is the concatenation
** of bByte and (pToken/nToken). The value is (iRowid/iCol/iPos).
**
**     (bByte || pToken) -> (iRowid,iCol,iPos)
**
** Or, if iCol is negative, then the value is a delete marker.
*/
static int sqlite3Fts5HashWrite(
  Fts5Hash *pHash,
  i64 iRowid,                     /* Rowid for this entry */
  int iCol,                       /* Column token appears in (-ve -> delete) */
  int iPos,                       /* Position of token within column */
  char bByte,                     /* First byte of token */
  const char *pToken, int nToken  /* Token to add or remove to or from index */
){
  unsigned int iHash;
  Fts5HashEntry *p;
  u8 *pPtr;
  int nIncr = 0;                  /* Amount to increment (*pHash->pnByte) by */
  int bNew;                       /* If non-delete entry should be written */

  bNew = (pHash->eDetail==FTS5_DETAIL_FULL);

  /* Attempt to locate an existing hash entry */
  iHash = fts5HashKey2(pHash->nSlot, (u8)bByte, (const u8*)pToken, nToken);
  for(p=pHash->aSlot[iHash]; p; p=p->pHashNext){
    char *zKey = fts5EntryKey(p);
    if( zKey[0]==bByte
     && p->nKey==nToken+1
     && memcmp(&zKey[1], pToken, nToken)==0
    ){
      break;
    }
  }

  /* If an existing hash entry cannot be found, create a new one. */
  if( p==0 ){
    /* Figure out how much space to allocate */
    char *zKey;
    sqlite3_int64 nByte = sizeof(Fts5HashEntry) + (nToken+1) + 1 + 64;
    if( nByte<128 ) nByte = 128;

    /* Grow the Fts5Hash.aSlot[] array if necessary. */
    if( (pHash->nEntry*2)>=pHash->nSlot ){
      int rc = fts5HashResize(pHash);
      if( rc!=SQLITE_OK ) return rc;
      iHash = fts5HashKey2(pHash->nSlot, (u8)bByte, (const u8*)pToken, nToken);
    }

    /* Allocate new Fts5HashEntry and add it to the hash table. */
    p = (Fts5HashEntry*)sqlite3_malloc64(nByte);
    if( !p ) return SQLITE_NOMEM;
    memset(p, 0, sizeof(Fts5HashEntry));
    p->nAlloc = (int)nByte;
    zKey = fts5EntryKey(p);
    zKey[0] = bByte;
    memcpy(&zKey[1], pToken, nToken);
    assert( iHash==fts5HashKey(pHash->nSlot, (u8*)zKey, nToken+1) );
    p->nKey = nToken+1;
    zKey[nToken+1] = '\0';
    p->nData = nToken+1 + sizeof(Fts5HashEntry);
    p->pHashNext = pHash->aSlot[iHash];
    pHash->aSlot[iHash] = p;
    pHash->nEntry++;

    /* Add the first rowid field to the hash-entry */
    p->nData += sqlite3Fts5PutVarint(&((u8*)p)[p->nData], iRowid);
    p->iRowid = iRowid;

    p->iSzPoslist = p->nData;
    if( pHash->eDetail!=FTS5_DETAIL_NONE ){
      p->nData += 1;
      p->iCol = (pHash->eDetail==FTS5_DETAIL_FULL ? 0 : -1);
    }

  }else{

    /* Appending to an existing hash-entry. Check that there is enough
    ** space to append the largest possible new entry. Worst case scenario
    ** is:
    **
    **     + 9 bytes for a new rowid,
    **     + 4 byte reserved for the "poslist size" varint.
    **     + 1 byte for a "new column" byte,
    **     + 3 bytes for a new column number (16-bit max) as a varint,
    **     + 5 bytes for the new position offset (32-bit max).
    */
    if( (p->nAlloc - p->nData) < (9 + 4 + 1 + 3 + 5) ){
      sqlite3_int64 nNew = p->nAlloc * 2;
      Fts5HashEntry *pNew;
      Fts5HashEntry **pp;
      pNew = (Fts5HashEntry*)sqlite3_realloc64(p, nNew);
      if( pNew==0 ) return SQLITE_NOMEM;
      pNew->nAlloc = (int)nNew;
      for(pp=&pHash->aSlot[iHash]; *pp!=p; pp=&(*pp)->pHashNext);
      *pp = pNew;
      p = pNew;
    }
    nIncr -= p->nData;
  }
  assert( (p->nAlloc - p->nData) >= (9 + 4 + 1 + 3 + 5) );

  pPtr = (u8*)p;

  /* If this is a new rowid, append the 4-byte size field for the previous
  ** entry, and the new rowid for this entry.  */
  if( iRowid!=p->iRowid ){
    u64 iDiff = (u64)iRowid - (u64)p->iRowid;
    fts5HashAddPoslistSize(pHash, p, 0);
    p->nData += sqlite3Fts5PutVarint(&pPtr[p->nData], iDiff);
    p->iRowid = iRowid;
    bNew = 1;
    p->iSzPoslist = p->nData;
    if( pHash->eDetail!=FTS5_DETAIL_NONE ){
      p->nData += 1;
      p->iCol = (pHash->eDetail==FTS5_DETAIL_FULL ? 0 : -1);
      p->iPos = 0;
    }
  }

  if( iCol>=0 ){
    if( pHash->eDetail==FTS5_DETAIL_NONE ){
      p->bContent = 1;
    }else{
      /* Append a new column value, if necessary */
      assert_nc( iCol>=p->iCol );
      if( iCol!=p->iCol ){
        if( pHash->eDetail==FTS5_DETAIL_FULL ){
          pPtr[p->nData++] = 0x01;
          p->nData += sqlite3Fts5PutVarint(&pPtr[p->nData], iCol);
          p->iCol = (i16)iCol;
          p->iPos = 0;
        }else{
          bNew = 1;
          p->iCol = (i16)(iPos = iCol);
        }
      }

      /* Append the new position offset, if necessary */
      if( bNew ){
        p->nData += sqlite3Fts5PutVarint(&pPtr[p->nData], iPos - p->iPos + 2);
        p->iPos = iPos;
      }
    }
  }else{
    /* This is a delete. Set the delete flag. */
    p->bDel = 1;
  }

  nIncr += p->nData;
  *pHash->pnByte += nIncr;
  return SQLITE_OK;
}
