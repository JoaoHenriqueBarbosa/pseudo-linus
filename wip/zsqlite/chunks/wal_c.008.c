
#ifdef SQLITE_ENABLE_SNAPSHOT
/*
** This function does the work of sqlite3WalSnapshotRecover().
*/
static int walSnapshotRecover(
  Wal *pWal,                      /* WAL handle */
  void *pBuf1,                    /* Temp buffer pWal->szPage bytes in size */
  void *pBuf2                     /* Temp buffer pWal->szPage bytes in size */
){
  int szPage = (int)pWal->szPage;
  int rc;
  i64 szDb;                       /* Size of db file in bytes */

  rc = sqlite3OsFileSize(pWal->pDbFd, &szDb);
  if( rc==SQLITE_OK ){
    volatile WalCkptInfo *pInfo = walCkptInfo(pWal);
    u32 i = pInfo->nBackfillAttempted;
    for(i=pInfo->nBackfillAttempted; i>AtomicLoad(&pInfo->nBackfill); i--){
      WalHashLoc sLoc;          /* Hash table location */
      u32 pgno;                 /* Page number in db file */
      i64 iDbOff;               /* Offset of db file entry */
      i64 iWalOff;              /* Offset of wal file entry */

      rc = walHashGet(pWal, walFramePage(i), &sLoc);
      if( rc!=SQLITE_OK ) break;
      assert( i - sLoc.iZero - 1 >=0 );
      pgno = sLoc.aPgno[i-sLoc.iZero-1];
      iDbOff = (i64)(pgno-1) * szPage;

      if( iDbOff+szPage<=szDb ){
        iWalOff = walFrameOffset(i, szPage) + WAL_FRAME_HDRSIZE;
        rc = sqlite3OsRead(pWal->pWalFd, pBuf1, szPage, iWalOff);

        if( rc==SQLITE_OK ){
          rc = sqlite3OsRead(pWal->pDbFd, pBuf2, szPage, iDbOff);
        }

        if( rc!=SQLITE_OK || 0==memcmp(pBuf1, pBuf2, szPage) ){
          break;
        }
      }

      pInfo->nBackfillAttempted = i-1;
    }
  }

  return rc;
}

/*
** Attempt to reduce the value of the WalCkptInfo.nBackfillAttempted
** variable so that older snapshots can be accessed. To do this, loop
** through all wal frames from nBackfillAttempted to (nBackfill+1),
** comparing their content to the corresponding page with the database
** file, if any. Set nBackfillAttempted to the frame number of the
** first frame for which the wal file content matches the db file.
**
** This is only really safe if the file-system is such that any page
** writes made by earlier checkpointers were atomic operations, which
** is not always true. It is also possible that nBackfillAttempted
** may be left set to a value larger than expected, if a wal frame
** contains content that duplicate of an earlier version of the same
** page.
**
** SQLITE_OK is returned if successful, or an SQLite error code if an
** error occurs. It is not an error if nBackfillAttempted cannot be
** decreased at all.
*/
SQLITE_PRIVATE int sqlite3WalSnapshotRecover(Wal *pWal){
  int rc;

  assert( pWal->readLock>=0 );
  rc = walLockExclusive(pWal, WAL_CKPT_LOCK, 1);
  if( rc==SQLITE_OK ){
    void *pBuf1 = sqlite3_malloc(pWal->szPage);
    void *pBuf2 = sqlite3_malloc(pWal->szPage);
    if( pBuf1==0 || pBuf2==0 ){
      rc = SQLITE_NOMEM;
    }else{
      pWal->ckptLock = 1;
      SEH_TRY {
        rc = walSnapshotRecover(pWal, pBuf1, pBuf2);
      }
      SEH_EXCEPT( rc = SQLITE_IOERR_IN_PAGE; )
      pWal->ckptLock = 0;
    }

    sqlite3_free(pBuf1);
    sqlite3_free(pBuf2);
    walUnlockExclusive(pWal, WAL_CKPT_LOCK, 1);
  }

  return rc;
}
#endif /* SQLITE_ENABLE_SNAPSHOT */

/*
** This function does the work of sqlite3WalBeginReadTransaction() (see
** below). That function simply calls this one inside an SEH_TRY{...} block.
*/
static int walBeginReadTransaction(Wal *pWal, int *pChanged){
  int rc;                         /* Return code */
  int cnt = 0;                    /* Number of TryBeginRead attempts */
#ifdef SQLITE_ENABLE_SNAPSHOT
  int ckptLock = 0;
  int bChanged = 0;
  WalIndexHdr *pSnapshot = pWal->pSnapshot;
#endif

  assert( pWal->ckptLock==0 );
  assert( pWal->nSehTry>0 );

#ifdef SQLITE_ENABLE_SNAPSHOT
  if( pSnapshot ){
    if( memcmp(pSnapshot, &pWal->hdr, sizeof(WalIndexHdr))!=0 ){
      bChanged = 1;
    }

    /* It is possible that there is a checkpointer thread running
    ** concurrent with this code. If this is the case, it may be that the
    ** checkpointer has already determined that it will checkpoint
    ** snapshot X, where X is later in the wal file than pSnapshot, but
    ** has not yet set the pInfo->nBackfillAttempted variable to indicate
    ** its intent. To avoid the race condition this leads to, ensure that
    ** there is no checkpointer process by taking a shared CKPT lock
    ** before checking pInfo->nBackfillAttempted.  */
    (void)walEnableBlocking(pWal);
    rc = walLockShared(pWal, WAL_CKPT_LOCK);
    walDisableBlocking(pWal);

    if( rc!=SQLITE_OK ){
      return rc;
    }
    ckptLock = 1;
  }
#endif

  do{
    rc = walTryBeginRead(pWal, pChanged, 0, &cnt);
  }while( rc==WAL_RETRY );
  testcase( (rc&0xff)==SQLITE_BUSY );
  testcase( (rc&0xff)==SQLITE_IOERR );
  testcase( rc==SQLITE_PROTOCOL );
  testcase( rc==SQLITE_OK );

#ifdef SQLITE_ENABLE_SNAPSHOT
  if( rc==SQLITE_OK ){
    if( pSnapshot && memcmp(pSnapshot, &pWal->hdr, sizeof(WalIndexHdr))!=0 ){
      /* At this point the client has a lock on an aReadMark[] slot holding
      ** a value equal to or smaller than pSnapshot->mxFrame, but pWal->hdr
      ** is populated with the wal-index header corresponding to the head
      ** of the wal file. Verify that pSnapshot is still valid before
      ** continuing.  Reasons why pSnapshot might no longer be valid:
      **
      **    (1)  The WAL file has been reset since the snapshot was taken.
      **         In this case, the salt will have changed.
      **
      **    (2)  A checkpoint as been attempted that wrote frames past
      **         pSnapshot->mxFrame into the database file.  Note that the
      **         checkpoint need not have completed for this to cause problems.
      */
      volatile WalCkptInfo *pInfo = walCkptInfo(pWal);

      assert( pWal->readLock>0 || pWal->hdr.mxFrame==0 );
      assert( pInfo->aReadMark[pWal->readLock]<=pSnapshot->mxFrame );

      /* Check that the wal file has not been wrapped. Assuming that it has
      ** not, also check that no checkpointer has attempted to checkpoint any
      ** frames beyond pSnapshot->mxFrame. If either of these conditions are
      ** true, return SQLITE_ERROR_SNAPSHOT. Otherwise, overwrite pWal->hdr
      ** with *pSnapshot and set *pChanged as appropriate for opening the
      ** snapshot.  */
      if( !memcmp(pSnapshot->aSalt, pWal->hdr.aSalt, sizeof(pWal->hdr.aSalt))
       && pSnapshot->mxFrame>=pInfo->nBackfillAttempted
      ){
        assert( pWal->readLock>0 );
        memcpy(&pWal->hdr, pSnapshot, sizeof(WalIndexHdr));
        *pChanged = bChanged;
      }else{
        rc = SQLITE_ERROR_SNAPSHOT;
      }

      /* A client using a non-current snapshot may not ignore any frames
      ** from the start of the wal file. This is because, for a system
      ** where (minFrame < iSnapshot < maxFrame), a checkpointer may
      ** have omitted to checkpoint a frame earlier than minFrame in
      ** the file because there exists a frame after iSnapshot that
      ** is the same database page.  */
      pWal->minFrame = 1;

      if( rc!=SQLITE_OK ){
        sqlite3WalEndReadTransaction(pWal);
      }
    }
  }

  /* Release the shared CKPT lock obtained above. */
  if( ckptLock ){
    assert( pSnapshot );
    walUnlockShared(pWal, WAL_CKPT_LOCK);
  }
#endif
  return rc;
}

/*
** Begin a read transaction on the database.
**
** This routine used to be called sqlite3OpenSnapshot() and with good reason:
** it takes a snapshot of the state of the WAL and wal-index for the current
** instant in time.  The current thread will continue to use this snapshot.
** Other threads might append new content to the WAL and wal-index but
** that extra content is ignored by the current thread.
**
** If the database contents have changes since the previous read
** transaction, then *pChanged is set to 1 before returning.  The
** Pager layer will use this to know that its cache is stale and
** needs to be flushed.
*/
SQLITE_PRIVATE int sqlite3WalBeginReadTransaction(Wal *pWal, int *pChanged){
  int rc;
  SEH_TRY {
    rc = walBeginReadTransaction(pWal, pChanged);
  }
  SEH_EXCEPT( rc = walHandleException(pWal); )
  return rc;
}

/*
** Finish with a read transaction.  All this does is release the
** read-lock.
*/
SQLITE_PRIVATE void sqlite3WalEndReadTransaction(Wal *pWal){
  sqlite3WalEndWriteTransaction(pWal);
  if( pWal->readLock>=0 ){
    walUnlockShared(pWal, WAL_READ_LOCK(pWal->readLock));
    pWal->readLock = -1;
  }
}

/*
** Search the wal file for page pgno. If found, set *piRead to the frame that
** contains the page. Otherwise, if pgno is not in the wal file, set *piRead
** to zero.
**
** Return SQLITE_OK if successful, or an error code if an error occurs. If an
** error does occur, the final value of *piRead is undefined.
*/
static int walFindFrame(
  Wal *pWal,                      /* WAL handle */
  Pgno pgno,                      /* Database page number to read data for */
  u32 *piRead                     /* OUT: Frame number (or zero) */
){
  u32 iRead = 0;                  /* If !=0, WAL frame to return data from */
  u32 iLast = pWal->hdr.mxFrame;  /* Last page in WAL for this reader */
  int iHash;                      /* Used to loop through N hash tables */
  int iMinHash;

  /* This routine is only be called from within a read transaction. */
  assert( pWal->readLock>=0 || pWal->lockError );

  /* If the "last page" field of the wal-index header snapshot is 0, then
  ** no data will be read from the wal under any circumstances. Return early
  ** in this case as an optimization.  Likewise, if pWal->readLock==0,
  ** then the WAL is ignored by the reader so return early, as if the
  ** WAL were empty.
  */
  if( iLast==0 || (pWal->readLock==0 && pWal->bShmUnreliable==0) ){
    *piRead = 0;
    return SQLITE_OK;
  }

  /* Search the hash table or tables for an entry matching page number
  ** pgno. Each iteration of the following for() loop searches one
  ** hash table (each hash table indexes up to HASHTABLE_NPAGE frames).
  **
  ** This code might run concurrently to the code in walIndexAppend()
  ** that adds entries to the wal-index (and possibly to this hash
  ** table). This means the value just read from the hash
  ** slot (aHash[iKey]) may have been added before or after the
  ** current read transaction was opened. Values added after the
  ** read transaction was opened may have been written incorrectly -
  ** i.e. these slots may contain garbage data. However, we assume
  ** that any slots written before the current read transaction was
  ** opened remain unmodified.
  **
  ** For the reasons above, the if(...) condition featured in the inner
  ** loop of the following block is more stringent that would be required
  ** if we had exclusive access to the hash-table:
  **
  **   (aPgno[iFrame]==pgno):
  **     This condition filters out normal hash-table collisions.
  **
  **   (iFrame<=iLast):
  **     This condition filters out entries that were added to the hash
  **     table after the current read-transaction had started.
  */
  iMinHash = walFramePage(pWal->minFrame);
  for(iHash=walFramePage(iLast); iHash>=iMinHash; iHash--){
    WalHashLoc sLoc;              /* Hash table location */
    int iKey;                     /* Hash slot index */
    int nCollide;                 /* Number of hash collisions remaining */
    int rc;                       /* Error code */
    u32 iH;

    rc = walHashGet(pWal, iHash, &sLoc);
    if( rc!=SQLITE_OK ){
      return rc;
    }
    nCollide = HASHTABLE_NSLOT;
    iKey = walHash(pgno);
    SEH_INJECT_FAULT;
    while( (iH = AtomicLoad(&sLoc.aHash[iKey]))!=0 ){
      u32 iFrame = iH + sLoc.iZero;
      if( iFrame<=iLast && iFrame>=pWal->minFrame && sLoc.aPgno[iH-1]==pgno ){
        assert( iFrame>iRead || CORRUPT_DB );
        iRead = iFrame;
      }
      if( (nCollide--)==0 ){
        *piRead = 0;
        return SQLITE_CORRUPT_BKPT;
      }
      iKey = walNextHash(iKey);
    }
    if( iRead ) break;
  }

#ifdef SQLITE_ENABLE_EXPENSIVE_ASSERT
  /* If expensive assert() statements are available, do a linear search
  ** of the wal-index file content. Make sure the results agree with the
  ** result obtained using the hash indexes above.  */
  {
    u32 iRead2 = 0;
    u32 iTest;
    assert( pWal->bShmUnreliable || pWal->minFrame>0 );
    for(iTest=iLast; iTest>=pWal->minFrame && iTest>0; iTest--){
      if( walFramePgno(pWal, iTest)==pgno ){
        iRead2 = iTest;
        break;
      }
    }
    assert( iRead==iRead2 );
  }
#endif

  *piRead = iRead;
  return SQLITE_OK;
}

/*
** Search the wal file for page pgno. If found, set *piRead to the frame that
** contains the page. Otherwise, if pgno is not in the wal file, set *piRead
** to zero.
**
** Return SQLITE_OK if successful, or an error code if an error occurs. If an
** error does occur, the final value of *piRead is undefined.
**
** The difference between this function and walFindFrame() is that this
** function wraps walFindFrame() in an SEH_TRY{...} block.
*/
SQLITE_PRIVATE int sqlite3WalFindFrame(
  Wal *pWal,                      /* WAL handle */
  Pgno pgno,                      /* Database page number to read data for */
  u32 *piRead                     /* OUT: Frame number (or zero) */
){
  int rc;
  SEH_TRY {
    rc = walFindFrame(pWal, pgno, piRead);
  }
  SEH_EXCEPT( rc = SQLITE_IOERR_IN_PAGE; )
  return rc;
}
