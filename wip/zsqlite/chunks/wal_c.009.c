
/*
** Read the contents of frame iRead from the wal file into buffer pOut
** (which is nOut bytes in size). Return SQLITE_OK if successful, or an
** error code otherwise.
*/
SQLITE_PRIVATE int sqlite3WalReadFrame(
  Wal *pWal,                      /* WAL handle */
  u32 iRead,                      /* Frame to read */
  int nOut,                       /* Size of buffer pOut in bytes */
  u8 *pOut                        /* Buffer to write page data to */
){
  int sz;
  i64 iOffset;
  sz = pWal->hdr.szPage;
  sz = (sz&0xfe00) + ((sz&0x0001)<<16);
  testcase( sz<=32768 );
  testcase( sz>=65536 );
  iOffset = walFrameOffset(iRead, sz) + WAL_FRAME_HDRSIZE;
  /* testcase( IS_BIG_INT(iOffset) ); // requires a 4GiB WAL */
  return sqlite3OsRead(pWal->pWalFd, pOut, (nOut>sz ? sz : nOut), iOffset);
}

/*
** Return the size of the database in pages (or zero, if unknown).
*/
SQLITE_PRIVATE Pgno sqlite3WalDbsize(Wal *pWal){
  if( pWal && ALWAYS(pWal->readLock>=0) ){
    return pWal->hdr.nPage;
  }
  return 0;
}


/*
** This function starts a write transaction on the WAL.
**
** A read transaction must have already been started by a prior call
** to sqlite3WalBeginReadTransaction().
**
** If another thread or process has written into the database since
** the read transaction was started, then it is not possible for this
** thread to write as doing so would cause a fork.  So this routine
** returns SQLITE_BUSY in that case and no write transaction is started.
**
** There can only be a single writer active at a time.
*/
SQLITE_PRIVATE int sqlite3WalBeginWriteTransaction(Wal *pWal){
  int rc;

#ifdef SQLITE_ENABLE_SETLK_TIMEOUT
  /* If the write-lock is already held, then it was obtained before the
  ** read-transaction was even opened, making this call a no-op.
  ** Return early. */
  if( pWal->writeLock ){
    assert( !memcmp(&pWal->hdr,(void *)walIndexHdr(pWal),sizeof(WalIndexHdr)) );
    return SQLITE_OK;
  }
#endif

  /* Cannot start a write transaction without first holding a read
  ** transaction. */
  assert( pWal->readLock>=0 );
  assert( pWal->writeLock==0 && pWal->iReCksum==0 );

  if( pWal->readOnly ){
    return SQLITE_READONLY;
  }

  /* Only one writer allowed at a time.  Get the write lock.  Return
  ** SQLITE_BUSY if unable.
  */
  rc = walLockExclusive(pWal, WAL_WRITE_LOCK, 1);
  if( rc ){
    return rc;
  }
  pWal->writeLock = 1;

  /* If another connection has written to the database file since the
  ** time the read transaction on this connection was started, then
  ** the write is disallowed.
  */
  SEH_TRY {
    if( memcmp(&pWal->hdr, (void *)walIndexHdr(pWal), sizeof(WalIndexHdr))!=0 ){
      rc = SQLITE_BUSY_SNAPSHOT;
    }
  }
  SEH_EXCEPT( rc = SQLITE_IOERR_IN_PAGE; )

  if( rc!=SQLITE_OK ){
    walUnlockExclusive(pWal, WAL_WRITE_LOCK, 1);
    pWal->writeLock = 0;
  }
  return rc;
}

/*
** End a write transaction.  The commit has already been done.  This
** routine merely releases the lock.
*/
SQLITE_PRIVATE int sqlite3WalEndWriteTransaction(Wal *pWal){
  if( pWal->writeLock ){
    walUnlockExclusive(pWal, WAL_WRITE_LOCK, 1);
    pWal->writeLock = 0;
    pWal->iReCksum = 0;
    pWal->truncateOnCommit = 0;
  }
  return SQLITE_OK;
}

/*
** If any data has been written (but not committed) to the log file, this
** function moves the write-pointer back to the start of the transaction.
**
** Additionally, the callback function is invoked for each frame written
** to the WAL since the start of the transaction. If the callback returns
** other than SQLITE_OK, it is not invoked again and the error code is
** returned to the caller.
**
** Otherwise, if the callback function does not return an error, this
** function returns SQLITE_OK.
*/
SQLITE_PRIVATE int sqlite3WalUndo(Wal *pWal, int (*xUndo)(void *, Pgno), void *pUndoCtx){
  int rc = SQLITE_OK;
  if( ALWAYS(pWal->writeLock) ){
    Pgno iMax = pWal->hdr.mxFrame;
    Pgno iFrame;

    SEH_TRY {
      /* Restore the clients cache of the wal-index header to the state it
      ** was in before the client began writing to the database.
      */
      memcpy(&pWal->hdr, (void *)walIndexHdr(pWal), sizeof(WalIndexHdr));

      for(iFrame=pWal->hdr.mxFrame+1;
          ALWAYS(rc==SQLITE_OK) && iFrame<=iMax;
          iFrame++
      ){
        /* This call cannot fail. Unless the page for which the page number
        ** is passed as the second argument is (a) in the cache and
        ** (b) has an outstanding reference, then xUndo is either a no-op
        ** (if (a) is false) or simply expels the page from the cache (if (b)
        ** is false).
        **
        ** If the upper layer is doing a rollback, it is guaranteed that there
        ** are no outstanding references to any page other than page 1. And
        ** page 1 is never written to the log until the transaction is
        ** committed. As a result, the call to xUndo may not fail.
        */
        assert( walFramePgno(pWal, iFrame)!=1 );
        rc = xUndo(pUndoCtx, walFramePgno(pWal, iFrame));
      }
      if( iMax!=pWal->hdr.mxFrame ) walCleanupHash(pWal);
    }
    SEH_EXCEPT( rc = SQLITE_IOERR_IN_PAGE; )
  }
  return rc;
}

/*
** Argument aWalData must point to an array of WAL_SAVEPOINT_NDATA u32
** values. This function populates the array with values required to
** "rollback" the write position of the WAL handle back to the current
** point in the event of a savepoint rollback (via WalSavepointUndo()).
*/
SQLITE_PRIVATE void sqlite3WalSavepoint(Wal *pWal, u32 *aWalData){
  assert( pWal->writeLock );
  aWalData[0] = pWal->hdr.mxFrame;
  aWalData[1] = pWal->hdr.aFrameCksum[0];
  aWalData[2] = pWal->hdr.aFrameCksum[1];
  aWalData[3] = pWal->nCkpt;
}

/*
** Move the write position of the WAL back to the point identified by
** the values in the aWalData[] array. aWalData must point to an array
** of WAL_SAVEPOINT_NDATA u32 values that has been previously populated
** by a call to WalSavepoint().
*/
SQLITE_PRIVATE int sqlite3WalSavepointUndo(Wal *pWal, u32 *aWalData){
  int rc = SQLITE_OK;

  assert( pWal->writeLock );
  assert( aWalData[3]!=pWal->nCkpt || aWalData[0]<=pWal->hdr.mxFrame );

  if( aWalData[3]!=pWal->nCkpt ){
    /* This savepoint was opened immediately after the write-transaction
    ** was started. Right after that, the writer decided to wrap around
    ** to the start of the log. Update the savepoint values to match.
    */
    aWalData[0] = 0;
    aWalData[3] = pWal->nCkpt;
  }

  if( aWalData[0]<pWal->hdr.mxFrame ){
    pWal->hdr.mxFrame = aWalData[0];
    pWal->hdr.aFrameCksum[0] = aWalData[1];
    pWal->hdr.aFrameCksum[1] = aWalData[2];
    SEH_TRY {
      walCleanupHash(pWal);
    }
    SEH_EXCEPT( rc = SQLITE_IOERR_IN_PAGE; )
  }

  return rc;
}

/*
** This function is called just before writing a set of frames to the log
** file (see sqlite3WalFrames()). It checks to see if, instead of appending
** to the current log file, it is possible to overwrite the start of the
** existing log file with the new frames (i.e. "reset" the log). If so,
** it sets pWal->hdr.mxFrame to 0. Otherwise, pWal->hdr.mxFrame is left
** unchanged.
**
** SQLITE_OK is returned if no error is encountered (regardless of whether
** or not pWal->hdr.mxFrame is modified). An SQLite error code is returned
** if an error occurs.
*/
static int walRestartLog(Wal *pWal){
  int rc = SQLITE_OK;
  int cnt;

  if( pWal->readLock==0 ){
    volatile WalCkptInfo *pInfo = walCkptInfo(pWal);
    assert( pInfo->nBackfill==pWal->hdr.mxFrame );
    if( pInfo->nBackfill>0 ){
      u32 salt1;
      sqlite3_randomness(4, &salt1);
      rc = walLockExclusive(pWal, WAL_READ_LOCK(1), WAL_NREADER-1);
      if( rc==SQLITE_OK ){
        /* If all readers are using WAL_READ_LOCK(0) (in other words if no
        ** readers are currently using the WAL), then the transactions
        ** frames will overwrite the start of the existing log. Update the
        ** wal-index header to reflect this.
        **
        ** In theory it would be Ok to update the cache of the header only
        ** at this point. But updating the actual wal-index header is also
        ** safe and means there is no special case for sqlite3WalUndo()
        ** to handle if this transaction is rolled back.  */
        walRestartHdr(pWal, salt1);
        walUnlockExclusive(pWal, WAL_READ_LOCK(1), WAL_NREADER-1);
      }else if( rc!=SQLITE_BUSY ){
        return rc;
      }
    }
    walUnlockShared(pWal, WAL_READ_LOCK(0));
    pWal->readLock = -1;
    cnt = 0;
    do{
      int notUsed;
      rc = walTryBeginRead(pWal, &notUsed, 1, &cnt);
    }while( rc==WAL_RETRY );
    assert( (rc&0xff)!=SQLITE_BUSY ); /* BUSY not possible when useWal==1 */
    testcase( (rc&0xff)==SQLITE_IOERR );
    testcase( rc==SQLITE_PROTOCOL );
    testcase( rc==SQLITE_OK );
  }
  return rc;
}

/*
** Information about the current state of the WAL file and where
** the next fsync should occur - passed from sqlite3WalFrames() into
** walWriteToLog().
*/
typedef struct WalWriter {
  Wal *pWal;                   /* The complete WAL information */
  sqlite3_file *pFd;           /* The WAL file to which we write */
  sqlite3_int64 iSyncPoint;    /* Fsync at this offset */
  int syncFlags;               /* Flags for the fsync */
  int szPage;                  /* Size of one page */
} WalWriter;

/*
** Write iAmt bytes of content into the WAL file beginning at iOffset.
** Do a sync when crossing the p->iSyncPoint boundary.
**
** In other words, if iSyncPoint is in between iOffset and iOffset+iAmt,
** first write the part before iSyncPoint, then sync, then write the
** rest.
*/
static int walWriteToLog(
  WalWriter *p,              /* WAL to write to */
  void *pContent,            /* Content to be written */
  int iAmt,                  /* Number of bytes to write */
  sqlite3_int64 iOffset      /* Start writing at this offset */
){
  int rc;
  if( iOffset<p->iSyncPoint && iOffset+iAmt>=p->iSyncPoint ){
    int iFirstAmt = (int)(p->iSyncPoint - iOffset);
    rc = sqlite3OsWrite(p->pFd, pContent, iFirstAmt, iOffset);
    if( rc ) return rc;
    iOffset += iFirstAmt;
    iAmt -= iFirstAmt;
    pContent = (void*)(iFirstAmt + (char*)pContent);
    assert( WAL_SYNC_FLAGS(p->syncFlags)!=0 );
    rc = sqlite3OsSync(p->pFd, WAL_SYNC_FLAGS(p->syncFlags));
    if( iAmt==0 || rc ) return rc;
  }
  rc = sqlite3OsWrite(p->pFd, pContent, iAmt, iOffset);
  return rc;
}

/*
** Write out a single frame of the WAL
*/
static int walWriteOneFrame(
  WalWriter *p,               /* Where to write the frame */
  PgHdr *pPage,               /* The page of the frame to be written */
  int nTruncate,              /* The commit flag.  Usually 0.  >0 for commit */
  sqlite3_int64 iOffset       /* Byte offset at which to write */
){
  int rc;                         /* Result code from subfunctions */
  void *pData;                    /* Data actually written */
  u8 aFrame[WAL_FRAME_HDRSIZE];   /* Buffer to assemble frame-header in */
  pData = pPage->pData;
  walEncodeFrame(p->pWal, pPage->pgno, nTruncate, pData, aFrame);
  rc = walWriteToLog(p, aFrame, sizeof(aFrame), iOffset);
  if( rc ) return rc;
  /* Write the page data */
  rc = walWriteToLog(p, pData, p->szPage, iOffset+sizeof(aFrame));
  return rc;
}

/*
** This function is called as part of committing a transaction within which
** one or more frames have been overwritten. It updates the checksums for
** all frames written to the wal file by the current transaction starting
** with the earliest to have been overwritten.
**
** SQLITE_OK is returned if successful, or an SQLite error code otherwise.
*/
static int walRewriteChecksums(Wal *pWal, u32 iLast){
  const int szPage = pWal->szPage;/* Database page size */
  int rc = SQLITE_OK;             /* Return code */
  u8 *aBuf;                       /* Buffer to load data from wal file into */
  u8 aFrame[WAL_FRAME_HDRSIZE];   /* Buffer to assemble frame-headers in */
  u32 iRead;                      /* Next frame to read from wal file */
  i64 iCksumOff;

  aBuf = sqlite3_malloc(szPage + WAL_FRAME_HDRSIZE);
  if( aBuf==0 ) return SQLITE_NOMEM_BKPT;

  /* Find the checksum values to use as input for the recalculating the
  ** first checksum. If the first frame is frame 1 (implying that the current
  ** transaction restarted the wal file), these values must be read from the
  ** wal-file header. Otherwise, read them from the frame header of the
  ** previous frame.  */
  assert( pWal->iReCksum>0 );
  if( pWal->iReCksum==1 ){
    iCksumOff = 24;
  }else{
    iCksumOff = walFrameOffset(pWal->iReCksum-1, szPage) + 16;
  }
  rc = sqlite3OsRead(pWal->pWalFd, aBuf, sizeof(u32)*2, iCksumOff);
  pWal->hdr.aFrameCksum[0] = sqlite3Get4byte(aBuf);
  pWal->hdr.aFrameCksum[1] = sqlite3Get4byte(&aBuf[sizeof(u32)]);

  iRead = pWal->iReCksum;
  pWal->iReCksum = 0;
  for(; rc==SQLITE_OK && iRead<=iLast; iRead++){
    i64 iOff = walFrameOffset(iRead, szPage);
    rc = sqlite3OsRead(pWal->pWalFd, aBuf, szPage+WAL_FRAME_HDRSIZE, iOff);
    if( rc==SQLITE_OK ){
      u32 iPgno, nDbSize;
      iPgno = sqlite3Get4byte(aBuf);
      nDbSize = sqlite3Get4byte(&aBuf[4]);

      walEncodeFrame(pWal, iPgno, nDbSize, &aBuf[WAL_FRAME_HDRSIZE], aFrame);
      rc = sqlite3OsWrite(pWal->pWalFd, aFrame, sizeof(aFrame), iOff);
    }
  }

  sqlite3_free(aBuf);
  return rc;
}
