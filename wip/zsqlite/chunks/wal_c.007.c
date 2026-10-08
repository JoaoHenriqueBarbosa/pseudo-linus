
/*
** Open a transaction in a connection where the shared-memory is read-only
** and where we cannot verify that there is a separate write-capable connection
** on hand to keep the shared-memory up-to-date with the WAL file.
**
** This can happen, for example, when the shared-memory is implemented by
** memory-mapping a *-shm file, where a prior writer has shut down and
** left the *-shm file on disk, and now the present connection is trying
** to use that database but lacks write permission on the *-shm file.
** Other scenarios are also possible, depending on the VFS implementation.
**
** Precondition:
**
**    The *-wal file has been read and an appropriate wal-index has been
**    constructed in pWal->apWiData[] using heap memory instead of shared
**    memory.
**
** If this function returns SQLITE_OK, then the read transaction has
** been successfully opened. In this case output variable (*pChanged)
** is set to true before returning if the caller should discard the
** contents of the page cache before proceeding. Or, if it returns
** WAL_RETRY, then the heap memory wal-index has been discarded and
** the caller should retry opening the read transaction from the
** beginning (including attempting to map the *-shm file).
**
** If an error occurs, an SQLite error code is returned.
*/
static int walBeginShmUnreliable(Wal *pWal, int *pChanged){
  i64 szWal;                      /* Size of wal file on disk in bytes */
  i64 iOffset;                    /* Current offset when reading wal file */
  u8 aBuf[WAL_HDRSIZE];           /* Buffer to load WAL header into */
  u8 *aFrame = 0;                 /* Malloc'd buffer to load entire frame */
  int szFrame;                    /* Number of bytes in buffer aFrame[] */
  u8 *aData;                      /* Pointer to data part of aFrame buffer */
  volatile void *pDummy;          /* Dummy argument for xShmMap */
  int rc;                         /* Return code */
  u32 aSaveCksum[2];              /* Saved copy of pWal->hdr.aFrameCksum */

  assert( pWal->bShmUnreliable );
  assert( pWal->readOnly & WAL_SHM_RDONLY );
  assert( pWal->nWiData>0 && pWal->apWiData[0] );

  /* Take WAL_READ_LOCK(0). This has the effect of preventing any
  ** writers from running a checkpoint, but does not stop them
  ** from running recovery.  */
  rc = walLockShared(pWal, WAL_READ_LOCK(0));
  if( rc!=SQLITE_OK ){
    if( rc==SQLITE_BUSY ) rc = WAL_RETRY;
    goto begin_unreliable_shm_out;
  }
  pWal->readLock = 0;

  /* Check to see if a separate writer has attached to the shared-memory area,
  ** thus making the shared-memory "reliable" again.  Do this by invoking
  ** the xShmMap() routine of the VFS and looking to see if the return
  ** is SQLITE_READONLY instead of SQLITE_READONLY_CANTINIT.
  **
  ** If the shared-memory is now "reliable" return WAL_RETRY, which will
  ** cause the heap-memory WAL-index to be discarded and the actual
  ** shared memory to be used in its place.
  **
  ** This step is important because, even though this connection is holding
  ** the WAL_READ_LOCK(0) which prevents a checkpoint, a writer might
  ** have already checkpointed the WAL file and, while the current
  ** is active, wrap the WAL and start overwriting frames that this
  ** process wants to use.
  **
  ** Once sqlite3OsShmMap() has been called for an sqlite3_file and has
  ** returned any SQLITE_READONLY value, it must return only SQLITE_READONLY
  ** or SQLITE_READONLY_CANTINIT or some error for all subsequent invocations,
  ** even if some external agent does a "chmod" to make the shared-memory
  ** writable by us, until sqlite3OsShmUnmap() has been called.
  ** This is a requirement on the VFS implementation.
   */
  rc = sqlite3OsShmMap(pWal->pDbFd, 0, WALINDEX_PGSZ, 0, &pDummy);
  assert( rc!=SQLITE_OK ); /* SQLITE_OK not possible for read-only connection */
  if( rc!=SQLITE_READONLY_CANTINIT ){
    rc = (rc==SQLITE_READONLY ? WAL_RETRY : rc);
    goto begin_unreliable_shm_out;
  }

  /* We reach this point only if the real shared-memory is still unreliable.
  ** Assume the in-memory WAL-index substitute is correct and load it
  ** into pWal->hdr.
  */
  memcpy(&pWal->hdr, (void*)walIndexHdr(pWal), sizeof(WalIndexHdr));

  /* Make sure some writer hasn't come in and changed the WAL file out
  ** from under us, then disconnected, while we were not looking.
  */
  rc = sqlite3OsFileSize(pWal->pWalFd, &szWal);
  if( rc!=SQLITE_OK ){
    goto begin_unreliable_shm_out;
  }
  if( szWal<WAL_HDRSIZE ){
    /* If the wal file is too small to contain a wal-header and the
    ** wal-index header has mxFrame==0, then it must be safe to proceed
    ** reading the database file only. However, the page cache cannot
    ** be trusted, as a read/write connection may have connected, written
    ** the db, run a checkpoint, truncated the wal file and disconnected
    ** since this client's last read transaction.  */
    *pChanged = 1;
    rc = (pWal->hdr.mxFrame==0 ? SQLITE_OK : WAL_RETRY);
    goto begin_unreliable_shm_out;
  }

  /* Check the salt keys at the start of the wal file still match. */
  rc = sqlite3OsRead(pWal->pWalFd, aBuf, WAL_HDRSIZE, 0);
  if( rc!=SQLITE_OK ){
    goto begin_unreliable_shm_out;
  }
  if( memcmp(&pWal->hdr.aSalt, &aBuf[16], 8) ){
    /* Some writer has wrapped the WAL file while we were not looking.
    ** Return WAL_RETRY which will cause the in-memory WAL-index to be
    ** rebuilt. */
    rc = WAL_RETRY;
    goto begin_unreliable_shm_out;
  }

  /* Allocate a buffer to read frames into */
  assert( (pWal->szPage & (pWal->szPage-1))==0 );
  assert( pWal->szPage>=512 && pWal->szPage<=65536 );
  szFrame = pWal->szPage + WAL_FRAME_HDRSIZE;
  aFrame = (u8 *)sqlite3_malloc64(szFrame);
  if( aFrame==0 ){
    rc = SQLITE_NOMEM_BKPT;
    goto begin_unreliable_shm_out;
  }
  aData = &aFrame[WAL_FRAME_HDRSIZE];

  /* Check to see if a complete transaction has been appended to the
  ** wal file since the heap-memory wal-index was created. If so, the
  ** heap-memory wal-index is discarded and WAL_RETRY returned to
  ** the caller.  */
  aSaveCksum[0] = pWal->hdr.aFrameCksum[0];
  aSaveCksum[1] = pWal->hdr.aFrameCksum[1];
  for(iOffset=walFrameOffset(pWal->hdr.mxFrame+1, pWal->szPage);
      iOffset+szFrame<=szWal;
      iOffset+=szFrame
  ){
    u32 pgno;                   /* Database page number for frame */
    u32 nTruncate;              /* dbsize field from frame header */

    /* Read and decode the next log frame. */
    rc = sqlite3OsRead(pWal->pWalFd, aFrame, szFrame, iOffset);
    if( rc!=SQLITE_OK ) break;
    if( !walDecodeFrame(pWal, &pgno, &nTruncate, aData, aFrame) ) break;

    /* If nTruncate is non-zero, then a complete transaction has been
    ** appended to this wal file. Set rc to WAL_RETRY and break out of
    ** the loop.  */
    if( nTruncate ){
      rc = WAL_RETRY;
      break;
    }
  }
  pWal->hdr.aFrameCksum[0] = aSaveCksum[0];
  pWal->hdr.aFrameCksum[1] = aSaveCksum[1];

 begin_unreliable_shm_out:
  sqlite3_free(aFrame);
  if( rc!=SQLITE_OK ){
    int i;
    for(i=0; i<pWal->nWiData; i++){
      sqlite3_free((void*)pWal->apWiData[i]);
      pWal->apWiData[i] = 0;
    }
    pWal->bShmUnreliable = 0;
    sqlite3WalEndReadTransaction(pWal);
    *pChanged = 1;
  }
  return rc;
}

/*
** The final argument passed to walTryBeginRead() is of type (int*). The
** caller should invoke walTryBeginRead as follows:
**
**   int cnt = 0;
**   do {
**     rc = walTryBeginRead(..., &cnt);
**   }while( rc==WAL_RETRY );
**
** The final value of "cnt" is of no use to the caller. It is used by
** the implementation of walTryBeginRead() as follows:
**
**   + Each time walTryBeginRead() is called, it is incremented. Once
**     it reaches WAL_RETRY_PROTOCOL_LIMIT - indicating that walTryBeginRead()
**     has many times been invoked and failed with WAL_RETRY - walTryBeginRead()
**     returns SQLITE_PROTOCOL.
**
**   + If SQLITE_ENABLE_SETLK_TIMEOUT is defined and walTryBeginRead() failed
**     because a blocking lock timed out (SQLITE_BUSY_TIMEOUT from the OS
**     layer), the WAL_RETRY_BLOCKED_MASK bit is set in "cnt". In this case
**     the next invocation of walTryBeginRead() may omit an expected call to
**     sqlite3OsSleep(). There has already been a delay when the previous call
**     waited on a lock.
*/
#define WAL_RETRY_PROTOCOL_LIMIT 100
#ifdef SQLITE_ENABLE_SETLK_TIMEOUT
# define WAL_RETRY_BLOCKED_MASK    0x10000000
#else
# define WAL_RETRY_BLOCKED_MASK    0
#endif

/*
** Attempt to start a read transaction.  This might fail due to a race or
** other transient condition.  When that happens, it returns WAL_RETRY to
** indicate to the caller that it is safe to retry immediately.
**
** On success return SQLITE_OK.  On a permanent failure (such an
** I/O error or an SQLITE_BUSY because another process is running
** recovery) return a positive error code.
**
** The useWal parameter is true to force the use of the WAL and disable
** the case where the WAL is bypassed because it has been completely
** checkpointed.  If useWal==0 then this routine calls walIndexReadHdr()
** to make a copy of the wal-index header into pWal->hdr.  If the
** wal-index header has changed, *pChanged is set to 1 (as an indication
** to the caller that the local page cache is obsolete and needs to be
** flushed.)  When useWal==1, the wal-index header is assumed to already
** be loaded and the pChanged parameter is unused.
**
** The caller must set the cnt parameter to the number of prior calls to
** this routine during the current read attempt that returned WAL_RETRY.
** This routine will start taking more aggressive measures to clear the
** race conditions after multiple WAL_RETRY returns, and after an excessive
** number of errors will ultimately return SQLITE_PROTOCOL.  The
** SQLITE_PROTOCOL return indicates that some other process has gone rogue
** and is not honoring the locking protocol.  There is a vanishingly small
** chance that SQLITE_PROTOCOL could be returned because of a run of really
** bad luck when there is lots of contention for the wal-index, but that
** possibility is so small that it can be safely neglected, we believe.
**
** On success, this routine obtains a read lock on
** WAL_READ_LOCK(pWal->readLock).  The pWal->readLock integer is
** in the range 0 <= pWal->readLock < WAL_NREADER.  If pWal->readLock==(-1)
** that means the Wal does not hold any read lock.  The reader must not
** access any database page that is modified by a WAL frame up to and
** including frame number aReadMark[pWal->readLock].  The reader will
** use WAL frames up to and including pWal->hdr.mxFrame if pWal->readLock>0
** Or if pWal->readLock==0, then the reader will ignore the WAL
** completely and get all content directly from the database file.
** If the useWal parameter is 1 then the WAL will never be ignored and
** this routine will always set pWal->readLock>0 on success.
** When the read transaction is completed, the caller must release the
** lock on WAL_READ_LOCK(pWal->readLock) and set pWal->readLock to -1.
**
** This routine uses the nBackfill and aReadMark[] fields of the header
** to select a particular WAL_READ_LOCK() that strives to let the
** checkpoint process do as much work as possible.  This routine might
** update values of the aReadMark[] array in the header, but if it does
** so it takes care to hold an exclusive lock on the corresponding
** WAL_READ_LOCK() while changing values.
*/
static int walTryBeginRead(Wal *pWal, int *pChanged, int useWal, int *pCnt){
  volatile WalCkptInfo *pInfo;    /* Checkpoint information in wal-index */
  u32 mxReadMark;                 /* Largest aReadMark[] value */
  int mxI;                        /* Index of largest aReadMark[] value */
  int i;                          /* Loop counter */
  int rc = SQLITE_OK;             /* Return code  */
  u32 mxFrame;                    /* Wal frame to lock to */
#ifdef SQLITE_ENABLE_SETLK_TIMEOUT
  int nBlockTmout = 0;
#endif

  assert( pWal->readLock<0 );     /* Not currently locked */

  /* useWal may only be set for read/write connections */
  assert( (pWal->readOnly & WAL_SHM_RDONLY)==0 || useWal==0 );

  /* Take steps to avoid spinning forever if there is a protocol error.
  **
  ** Circumstances that cause a RETRY should only last for the briefest
  ** instances of time.  No I/O or other system calls are done while the
  ** locks are held, so the locks should not be held for very long. But
  ** if we are unlucky, another process that is holding a lock might get
  ** paged out or take a page-fault that is time-consuming to resolve,
  ** during the few nanoseconds that it is holding the lock.  In that case,
  ** it might take longer than normal for the lock to free.
  **
  ** After 5 RETRYs, we begin calling sqlite3OsSleep().  The first few
  ** calls to sqlite3OsSleep() have a delay of 1 microsecond.  Really this
  ** is more of a scheduler yield than an actual delay.  But on the 10th
  ** an subsequent retries, the delays start becoming longer and longer,
  ** so that on the 100th (and last) RETRY we delay for 323 milliseconds.
  ** The total delay time before giving up is less than 10 seconds.
  */
  (*pCnt)++;
  if( *pCnt>5 ){
    int nDelay = 1;                      /* Pause time in microseconds */
    int cnt = (*pCnt & ~WAL_RETRY_BLOCKED_MASK);
    if( cnt>WAL_RETRY_PROTOCOL_LIMIT ){
      VVA_ONLY( pWal->lockError = 1; )
      return SQLITE_PROTOCOL;
    }
    if( *pCnt>=10 ) nDelay = (cnt-9)*(cnt-9)*39;
#ifdef SQLITE_ENABLE_SETLK_TIMEOUT
    /* In SQLITE_ENABLE_SETLK_TIMEOUT builds, configure the file-descriptor
    ** to block for locks for approximately nDelay us. This affects three
    ** locks: (a) the shared lock taken on the DMS slot in os_unix.c (if
    ** using os_unix.c), (b) the WRITER lock taken in walIndexReadHdr() if the
    ** first attempted read fails, and (c) the shared lock taken on the
    ** read-mark.
    **
    ** If the previous call failed due to an SQLITE_BUSY_TIMEOUT error,
    ** then sleep for the minimum of 1us. The previous call already provided
    ** an extra delay while it was blocking on the lock.
    */
    nBlockTmout = (nDelay+998) / 1000;
    if( !useWal && walEnableBlockingMs(pWal, nBlockTmout) ){
      if( *pCnt & WAL_RETRY_BLOCKED_MASK ) nDelay = 1;
    }
#endif
    sqlite3OsSleep(pWal->pVfs, nDelay);
    *pCnt &= ~WAL_RETRY_BLOCKED_MASK;
  }

  if( !useWal ){
    assert( rc==SQLITE_OK );
    if( pWal->bShmUnreliable==0 ){
      rc = walIndexReadHdr(pWal, pChanged);
    }
#ifdef SQLITE_ENABLE_SETLK_TIMEOUT
    walDisableBlocking(pWal);
    if( rc==SQLITE_BUSY_TIMEOUT ){
      rc = SQLITE_BUSY;
      *pCnt |= WAL_RETRY_BLOCKED_MASK;
    }
#endif
    if( rc==SQLITE_BUSY ){
      /* If there is not a recovery running in another thread or process
      ** then convert BUSY errors to WAL_RETRY.  If recovery is known to
      ** be running, convert BUSY to BUSY_RECOVERY.  There is a race here
      ** which might cause WAL_RETRY to be returned even if BUSY_RECOVERY
      ** would be technically correct.  But the race is benign since with
      ** WAL_RETRY this routine will be called again and will probably be
      ** right on the second iteration.
      */
      if( pWal->apWiData[0]==0 ){
        /* This branch is taken when the xShmMap() method returns SQLITE_BUSY.
        ** We assume this is a transient condition, so return WAL_RETRY. The
        ** xShmMap() implementation used by the default unix and win32 VFS
        ** modules may return SQLITE_BUSY due to a race condition in the
        ** code that determines whether or not the shared-memory region
        ** must be zeroed before the requested page is returned.
        */
        rc = WAL_RETRY;
      }else if( SQLITE_OK==(rc = walLockShared(pWal, WAL_RECOVER_LOCK)) ){
        walUnlockShared(pWal, WAL_RECOVER_LOCK);
        rc = WAL_RETRY;
      }else if( rc==SQLITE_BUSY ){
        rc = SQLITE_BUSY_RECOVERY;
      }
    }
    if( rc!=SQLITE_OK ){
      return rc;
    }
    else if( pWal->bShmUnreliable ){
      return walBeginShmUnreliable(pWal, pChanged);
    }
  }

  assert( pWal->nWiData>0 );
  assert( pWal->apWiData[0]!=0 );
  pInfo = walCkptInfo(pWal);
  SEH_INJECT_FAULT;
  if( !useWal && AtomicLoad(&pInfo->nBackfill)==pWal->hdr.mxFrame
#ifdef SQLITE_ENABLE_SNAPSHOT
   && (pWal->pSnapshot==0 || pWal->hdr.mxFrame==0)
#endif
  ){
    /* The WAL has been completely backfilled (or it is empty).
    ** and can be safely ignored.
    */
    rc = walLockShared(pWal, WAL_READ_LOCK(0));
    walShmBarrier(pWal);
    if( rc==SQLITE_OK ){
      if( memcmp((void *)walIndexHdr(pWal), &pWal->hdr, sizeof(WalIndexHdr)) ){
        /* It is not safe to allow the reader to continue here if frames
        ** may have been appended to the log before READ_LOCK(0) was obtained.
        ** When holding READ_LOCK(0), the reader ignores the entire log file,
        ** which implies that the database file contains a trustworthy
        ** snapshot. Since holding READ_LOCK(0) prevents a checkpoint from
        ** happening, this is usually correct.
        **
        ** However, if frames have been appended to the log (or if the log
        ** is wrapped and written for that matter) before the READ_LOCK(0)
        ** is obtained, that is not necessarily true. A checkpointer may
        ** have started to backfill the appended frames but crashed before
        ** it finished. Leaving a corrupt image in the database file.
        */
        walUnlockShared(pWal, WAL_READ_LOCK(0));
        return WAL_RETRY;
      }
      pWal->readLock = 0;
      return SQLITE_OK;
    }else if( rc!=SQLITE_BUSY ){
      return rc;
    }
  }

  /* If we get this far, it means that the reader will want to use
  ** the WAL to get at content from recent commits.  The job now is
  ** to select one of the aReadMark[] entries that is closest to
  ** but not exceeding pWal->hdr.mxFrame and lock that entry.
  */
  mxReadMark = 0;
  mxI = 0;
  mxFrame = pWal->hdr.mxFrame;
#ifdef SQLITE_ENABLE_SNAPSHOT
  if( pWal->pSnapshot && pWal->pSnapshot->mxFrame<mxFrame ){
    mxFrame = pWal->pSnapshot->mxFrame;
  }
#endif
  for(i=1; i<WAL_NREADER; i++){
    u32 thisMark = AtomicLoad(pInfo->aReadMark+i); SEH_INJECT_FAULT;
    if( mxReadMark<=thisMark && thisMark<=mxFrame ){
      assert( thisMark!=READMARK_NOT_USED );
      mxReadMark = thisMark;
      mxI = i;
    }
  }
  if( (pWal->readOnly & WAL_SHM_RDONLY)==0
   && (mxReadMark<mxFrame || mxI==0)
  ){
    for(i=1; i<WAL_NREADER; i++){
      rc = walLockExclusive(pWal, WAL_READ_LOCK(i), 1);
      if( rc==SQLITE_OK ){
        AtomicStore(pInfo->aReadMark+i,mxFrame);
        mxReadMark = mxFrame;
        mxI = i;
        walUnlockExclusive(pWal, WAL_READ_LOCK(i), 1);
        break;
      }else if( rc!=SQLITE_BUSY ){
        return rc;
      }
    }
  }
  if( mxI==0 ){
    assert( rc==SQLITE_BUSY || (pWal->readOnly & WAL_SHM_RDONLY)!=0 );
    return rc==SQLITE_BUSY ? WAL_RETRY : SQLITE_READONLY_CANTINIT;
  }

  (void)walEnableBlockingMs(pWal, nBlockTmout);
  rc = walLockShared(pWal, WAL_READ_LOCK(mxI));
  walDisableBlocking(pWal);
  if( rc ){
#ifdef SQLITE_ENABLE_SETLK_TIMEOUT
    if( rc==SQLITE_BUSY_TIMEOUT ){
      *pCnt |= WAL_RETRY_BLOCKED_MASK;
    }
#else
    assert( rc!=SQLITE_BUSY_TIMEOUT );
#endif
    assert( (rc&0xFF)!=SQLITE_BUSY||rc==SQLITE_BUSY||rc==SQLITE_BUSY_TIMEOUT );
    return (rc&0xFF)==SQLITE_BUSY ? WAL_RETRY : rc;
  }
  /* Now that the read-lock has been obtained, check that neither the
  ** value in the aReadMark[] array or the contents of the wal-index
  ** header have changed.
  **
  ** It is necessary to check that the wal-index header did not change
  ** between the time it was read and when the shared-lock was obtained
  ** on WAL_READ_LOCK(mxI) was obtained to account for the possibility
  ** that the log file may have been wrapped by a writer, or that frames
  ** that occur later in the log than pWal->hdr.mxFrame may have been
  ** copied into the database by a checkpointer. If either of these things
  ** happened, then reading the database with the current value of
  ** pWal->hdr.mxFrame risks reading a corrupted snapshot. So, retry
  ** instead.
  **
  ** Before checking that the live wal-index header has not changed
  ** since it was read, set Wal.minFrame to the first frame in the wal
  ** file that has not yet been checkpointed. This client will not need
  ** to read any frames earlier than minFrame from the wal file - they
  ** can be safely read directly from the database file.
  **
  ** Because a ShmBarrier() call is made between taking the copy of
  ** nBackfill and checking that the wal-header in shared-memory still
  ** matches the one cached in pWal->hdr, it is guaranteed that the
  ** checkpointer that set nBackfill was not working with a wal-index
  ** header newer than that cached in pWal->hdr. If it were, that could
  ** cause a problem. The checkpointer could omit to checkpoint
  ** a version of page X that lies before pWal->minFrame (call that version
  ** A) on the basis that there is a newer version (version B) of the same
  ** page later in the wal file. But if version B happens to like past
  ** frame pWal->hdr.mxFrame - then the client would incorrectly assume
  ** that it can read version A from the database file. However, since
  ** we can guarantee that the checkpointer that set nBackfill could not
  ** see any pages past pWal->hdr.mxFrame, this problem does not come up.
  */
  pWal->minFrame = AtomicLoad(&pInfo->nBackfill)+1; SEH_INJECT_FAULT;
  walShmBarrier(pWal);
  if( AtomicLoad(pInfo->aReadMark+mxI)!=mxReadMark
   || memcmp((void *)walIndexHdr(pWal), &pWal->hdr, sizeof(WalIndexHdr))
  ){
    walUnlockShared(pWal, WAL_READ_LOCK(mxI));
    return WAL_RETRY;
  }else{
    assert( mxReadMark<=pWal->hdr.mxFrame );
    pWal->readLock = (i16)mxI;
  }
  return rc;
}
