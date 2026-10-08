
/*
** Write a set of frames to the log. The caller must hold the write-lock
** on the log file (obtained using sqlite3WalBeginWriteTransaction()).
*/
static int walFrames(
  Wal *pWal,                      /* Wal handle to write to */
  int szPage,                     /* Database page-size in bytes */
  PgHdr *pList,                   /* List of dirty pages to write */
  Pgno nTruncate,                 /* Database size after this commit */
  int isCommit,                   /* True if this is a commit */
  int sync_flags                  /* Flags to pass to OsSync() (or 0) */
){
  int rc;                         /* Used to catch return codes */
  u32 iFrame;                     /* Next frame address */
  PgHdr *p;                       /* Iterator to run through pList with. */
  PgHdr *pLast = 0;               /* Last frame in list */
  int nExtra = 0;                 /* Number of extra copies of last page */
  int szFrame;                    /* The size of a single frame */
  i64 iOffset;                    /* Next byte to write in WAL file */
  WalWriter w;                    /* The writer */
  u32 iFirst = 0;                 /* First frame that may be overwritten */
  WalIndexHdr *pLive;             /* Pointer to shared header */

  assert( pList );
  assert( pWal->writeLock );

  /* If this frame set completes a transaction, then nTruncate>0.  If
  ** nTruncate==0 then this frame set does not complete the transaction. */
  assert( (isCommit!=0)==(nTruncate!=0) );

#if defined(SQLITE_TEST) && defined(SQLITE_DEBUG)
  { int cnt; for(cnt=0, p=pList; p; p=p->pDirty, cnt++){}
    WALTRACE(("WAL%p: frame write begin. %d frames. mxFrame=%d. %s\n",
              pWal, cnt, pWal->hdr.mxFrame, isCommit ? "Commit" : "Spill"));
  }
#endif

  pLive = (WalIndexHdr*)walIndexHdr(pWal);
  if( memcmp(&pWal->hdr, (void *)pLive, sizeof(WalIndexHdr))!=0 ){
    iFirst = pLive->mxFrame+1;
  }

  /* See if it is possible to write these frames into the start of the
  ** log file, instead of appending to it at pWal->hdr.mxFrame.
  */
  if( SQLITE_OK!=(rc = walRestartLog(pWal)) ){
    return rc;
  }

  /* If this is the first frame written into the log, write the WAL
  ** header to the start of the WAL file. See comments at the top of
  ** this source file for a description of the WAL header format.
  */
  iFrame = pWal->hdr.mxFrame;
  if( iFrame==0 ){
    u8 aWalHdr[WAL_HDRSIZE];      /* Buffer to assemble wal-header in */
    u32 aCksum[2];                /* Checksum for wal-header */

    sqlite3Put4byte(&aWalHdr[0], (WAL_MAGIC | SQLITE_BIGENDIAN));
    sqlite3Put4byte(&aWalHdr[4], WAL_MAX_VERSION);
    sqlite3Put4byte(&aWalHdr[8], szPage);
    sqlite3Put4byte(&aWalHdr[12], pWal->nCkpt);
    if( pWal->nCkpt==0 ) sqlite3_randomness(8, pWal->hdr.aSalt);
    memcpy(&aWalHdr[16], pWal->hdr.aSalt, 8);
    walChecksumBytes(1, aWalHdr, WAL_HDRSIZE-2*4, 0, aCksum);
    sqlite3Put4byte(&aWalHdr[24], aCksum[0]);
    sqlite3Put4byte(&aWalHdr[28], aCksum[1]);

    pWal->szPage = szPage;
    pWal->hdr.bigEndCksum = SQLITE_BIGENDIAN;
    pWal->hdr.aFrameCksum[0] = aCksum[0];
    pWal->hdr.aFrameCksum[1] = aCksum[1];
    pWal->truncateOnCommit = 1;

    rc = sqlite3OsWrite(pWal->pWalFd, aWalHdr, sizeof(aWalHdr), 0);
    WALTRACE(("WAL%p: wal-header write %s\n", pWal, rc ? "failed" : "ok"));
    if( rc!=SQLITE_OK ){
      return rc;
    }

    /* Sync the header (unless SQLITE_IOCAP_SEQUENTIAL is true or unless
    ** all syncing is turned off by PRAGMA synchronous=OFF).  Otherwise
    ** an out-of-order write following a WAL restart could result in
    ** database corruption.  See the ticket:
    **
    **     https://sqlite.org/src/info/ff5be73dee
    */
    if( pWal->syncHeader ){
      rc = sqlite3OsSync(pWal->pWalFd, CKPT_SYNC_FLAGS(sync_flags));
      if( rc ) return rc;
    }
  }
  if( (int)pWal->szPage!=szPage ){
    return SQLITE_CORRUPT_BKPT;  /* TH3 test case: cov1/corrupt155.test */
  }

  /* Setup information needed to write frames into the WAL */
  w.pWal = pWal;
  w.pFd = pWal->pWalFd;
  w.iSyncPoint = 0;
  w.syncFlags = sync_flags;
  w.szPage = szPage;
  iOffset = walFrameOffset(iFrame+1, szPage);
  szFrame = szPage + WAL_FRAME_HDRSIZE;

  /* Write all frames into the log file exactly once */
  for(p=pList; p; p=p->pDirty){
    int nDbSize;   /* 0 normally.  Positive == commit flag */

    /* Check if this page has already been written into the wal file by
    ** the current transaction. If so, overwrite the existing frame and
    ** set Wal.writeLock to WAL_WRITELOCK_RECKSUM - indicating that
    ** checksums must be recomputed when the transaction is committed.  */
    if( iFirst && (p->pDirty || isCommit==0) ){
      u32 iWrite = 0;
      VVA_ONLY(rc =) walFindFrame(pWal, p->pgno, &iWrite);
      assert( rc==SQLITE_OK || iWrite==0 );
      if( iWrite>=iFirst ){
        i64 iOff = walFrameOffset(iWrite, szPage) + WAL_FRAME_HDRSIZE;
        void *pData;
        if( pWal->iReCksum==0 || iWrite<pWal->iReCksum ){
          pWal->iReCksum = iWrite;
        }
        pData = p->pData;
        rc = sqlite3OsWrite(pWal->pWalFd, pData, szPage, iOff);
        if( rc ) return rc;
        p->flags &= ~PGHDR_WAL_APPEND;
        continue;
      }
    }

    iFrame++;
    assert( iOffset==walFrameOffset(iFrame, szPage) );
    nDbSize = (isCommit && p->pDirty==0) ? nTruncate : 0;
    rc = walWriteOneFrame(&w, p, nDbSize, iOffset);
    if( rc ) return rc;
    pLast = p;
    iOffset += szFrame;
    p->flags |= PGHDR_WAL_APPEND;
  }

  /* Recalculate checksums within the wal file if required. */
  if( isCommit && pWal->iReCksum ){
    rc = walRewriteChecksums(pWal, iFrame);
    if( rc ) return rc;
  }

  /* If this is the end of a transaction, then we might need to pad
  ** the transaction and/or sync the WAL file.
  **
  ** Padding and syncing only occur if this set of frames complete a
  ** transaction and if PRAGMA synchronous=FULL.  If synchronous==NORMAL
  ** or synchronous==OFF, then no padding or syncing are needed.
  **
  ** If SQLITE_IOCAP_POWERSAFE_OVERWRITE is defined, then padding is not
  ** needed and only the sync is done.  If padding is needed, then the
  ** final frame is repeated (with its commit mark) until the next sector
  ** boundary is crossed.  Only the part of the WAL prior to the last
  ** sector boundary is synced; the part of the last frame that extends
  ** past the sector boundary is written after the sync.
  */
  if( isCommit && WAL_SYNC_FLAGS(sync_flags)!=0 ){
    int bSync = 1;
    if( pWal->padToSectorBoundary ){
      int sectorSize = sqlite3SectorSize(pWal->pWalFd);
      w.iSyncPoint = ((iOffset+sectorSize-1)/sectorSize)*sectorSize;
      bSync = (w.iSyncPoint==iOffset);
      testcase( bSync );
      while( iOffset<w.iSyncPoint ){
        rc = walWriteOneFrame(&w, pLast, nTruncate, iOffset);
        if( rc ) return rc;
        iOffset += szFrame;
        nExtra++;
        assert( pLast!=0 );
      }
    }
    if( bSync ){
      assert( rc==SQLITE_OK );
      rc = sqlite3OsSync(w.pFd, WAL_SYNC_FLAGS(sync_flags));
    }
  }

  /* If this frame set completes the first transaction in the WAL and
  ** if PRAGMA journal_size_limit is set, then truncate the WAL to the
  ** journal size limit, if possible.
  */
  if( isCommit && pWal->truncateOnCommit && pWal->mxWalSize>=0 ){
    i64 sz = pWal->mxWalSize;
    if( walFrameOffset(iFrame+nExtra+1, szPage)>pWal->mxWalSize ){
      sz = walFrameOffset(iFrame+nExtra+1, szPage);
    }
    walLimitSize(pWal, sz);
    pWal->truncateOnCommit = 0;
  }

  /* Append data to the wal-index. It is not necessary to lock the
  ** wal-index to do this as the SQLITE_SHM_WRITE lock held on the wal-index
  ** guarantees that there are no other writers, and no data that may
  ** be in use by existing readers is being overwritten.
  */
  iFrame = pWal->hdr.mxFrame;
  for(p=pList; p && rc==SQLITE_OK; p=p->pDirty){
    if( (p->flags & PGHDR_WAL_APPEND)==0 ) continue;
    iFrame++;
    rc = walIndexAppend(pWal, iFrame, p->pgno);
  }
  assert( pLast!=0 || nExtra==0 );
  while( rc==SQLITE_OK && nExtra>0 ){
    iFrame++;
    nExtra--;
    rc = walIndexAppend(pWal, iFrame, pLast->pgno);
  }

  if( rc==SQLITE_OK ){
    /* Update the private copy of the header. */
    pWal->hdr.szPage = (u16)((szPage&0xff00) | (szPage>>16));
    testcase( szPage<=32768 );
    testcase( szPage>=65536 );
    pWal->hdr.mxFrame = iFrame;
    if( isCommit ){
      pWal->hdr.iChange++;
      pWal->hdr.nPage = nTruncate;
    }
    /* If this is a commit, update the wal-index header too. */
    if( isCommit ){
      walIndexWriteHdr(pWal);
      pWal->iCallback = iFrame;
    }
  }

  WALTRACE(("WAL%p: frame write %s\n", pWal, rc ? "failed" : "ok"));
  return rc;
}

/*
** Write a set of frames to the log. The caller must hold the write-lock
** on the log file (obtained using sqlite3WalBeginWriteTransaction()).
**
** The difference between this function and walFrames() is that this
** function wraps walFrames() in an SEH_TRY{...} block.
*/
SQLITE_PRIVATE int sqlite3WalFrames(
  Wal *pWal,                      /* Wal handle to write to */
  int szPage,                     /* Database page-size in bytes */
  PgHdr *pList,                   /* List of dirty pages to write */
  Pgno nTruncate,                 /* Database size after this commit */
  int isCommit,                   /* True if this is a commit */
  int sync_flags                  /* Flags to pass to OsSync() (or 0) */
){
  int rc;
  SEH_TRY {
    rc = walFrames(pWal, szPage, pList, nTruncate, isCommit, sync_flags);
  }
  SEH_EXCEPT( rc = walHandleException(pWal); )
  return rc;
}

/*
** This routine is called to implement sqlite3_wal_checkpoint() and
** related interfaces.
**
** Obtain a CHECKPOINT lock and then backfill as much information as
** we can from WAL into the database.
**
** If parameter xBusy is not NULL, it is a pointer to a busy-handler
** callback. In this case this function runs a blocking checkpoint.
*/
SQLITE_PRIVATE int sqlite3WalCheckpoint(
  Wal *pWal,                      /* Wal connection */
  sqlite3 *db,                    /* Check this handle's interrupt flag */
  int eMode,                      /* PASSIVE, FULL, RESTART, or TRUNCATE */
  int (*xBusy)(void*),            /* Function to call when busy */
  void *pBusyArg,                 /* Context argument for xBusyHandler */
  int sync_flags,                 /* Flags to sync db file with (or 0) */
  int nBuf,                       /* Size of temporary buffer */
  u8 *zBuf,                       /* Temporary buffer to use */
  int *pnLog,                     /* OUT: Number of frames in WAL */
  int *pnCkpt                     /* OUT: Number of backfilled frames in WAL */
){
  int rc;                         /* Return code */
  int isChanged = 0;              /* True if a new wal-index header is loaded */
  int eMode2 = eMode;             /* Mode to pass to walCheckpoint() */
  int (*xBusy2)(void*) = xBusy;   /* Busy handler for eMode2 */

  assert( pWal->ckptLock==0 );
  assert( pWal->writeLock==0 );

  /* EVIDENCE-OF: R-62920-47450 The busy-handler callback is never invoked
  ** in the SQLITE_CHECKPOINT_PASSIVE mode. */
  assert( eMode!=SQLITE_CHECKPOINT_PASSIVE || xBusy==0 );

  if( pWal->readOnly ) return SQLITE_READONLY;
  WALTRACE(("WAL%p: checkpoint begins\n", pWal));

  /* Enable blocking locks, if possible. */
  sqlite3WalDb(pWal, db);
  if( xBusy2 ) (void)walEnableBlocking(pWal);

  /* IMPLEMENTATION-OF: R-62028-47212 All calls obtain an exclusive
  ** "checkpoint" lock on the database file.
  ** EVIDENCE-OF: R-10421-19736 If any other process is running a
  ** checkpoint operation at the same time, the lock cannot be obtained and
  ** SQLITE_BUSY is returned.
  ** EVIDENCE-OF: R-53820-33897 Even if there is a busy-handler configured,
  ** it will not be invoked in this case.
  */
  rc = walLockExclusive(pWal, WAL_CKPT_LOCK, 1);
  testcase( rc==SQLITE_BUSY );
  testcase( rc!=SQLITE_OK && xBusy2!=0 );
  if( rc==SQLITE_OK ){
    pWal->ckptLock = 1;

    /* IMPLEMENTATION-OF: R-59782-36818 The SQLITE_CHECKPOINT_FULL, RESTART and
    ** TRUNCATE modes also obtain the exclusive "writer" lock on the database
    ** file.
    **
    ** EVIDENCE-OF: R-60642-04082 If the writer lock cannot be obtained
    ** immediately, and a busy-handler is configured, it is invoked and the
    ** writer lock retried until either the busy-handler returns 0 or the
    ** lock is successfully obtained.
    */
    if( eMode!=SQLITE_CHECKPOINT_PASSIVE ){
      rc = walBusyLock(pWal, xBusy2, pBusyArg, WAL_WRITE_LOCK, 1);
      if( rc==SQLITE_OK ){
        pWal->writeLock = 1;
      }else if( rc==SQLITE_BUSY ){
        eMode2 = SQLITE_CHECKPOINT_PASSIVE;
        xBusy2 = 0;
        rc = SQLITE_OK;
      }
    }
  }


  /* Read the wal-index header. */
  SEH_TRY {
    if( rc==SQLITE_OK ){
      /* For a passive checkpoint, do not re-enable blocking locks after
      ** reading the wal-index header. A passive checkpoint should not block
      ** or invoke the busy handler. The only lock such a checkpoint may
      ** attempt to obtain is a lock on a read-slot, and it should give up
      ** immediately and do a partial checkpoint if it cannot obtain it. */
      walDisableBlocking(pWal);
      rc = walIndexReadHdr(pWal, &isChanged);
      if( eMode2!=SQLITE_CHECKPOINT_PASSIVE ) (void)walEnableBlocking(pWal);
      if( isChanged && pWal->pDbFd->pMethods->iVersion>=3 ){
        sqlite3OsUnfetch(pWal->pDbFd, 0, 0);
      }
    }

    /* Copy data from the log to the database file. */
    if( rc==SQLITE_OK ){
      if( pWal->hdr.mxFrame && walPagesize(pWal)!=nBuf ){
        rc = SQLITE_CORRUPT_BKPT;
      }else{
        rc = walCheckpoint(pWal, db, eMode2, xBusy2, pBusyArg, sync_flags,zBuf);
      }

      /* If no error occurred, set the output variables. */
      if( rc==SQLITE_OK || rc==SQLITE_BUSY ){
        if( pnLog ) *pnLog = (int)pWal->hdr.mxFrame;
        SEH_INJECT_FAULT;
        if( pnCkpt ) *pnCkpt = (int)(walCkptInfo(pWal)->nBackfill);
      }
    }
  }
  SEH_EXCEPT( rc = walHandleException(pWal); )

  if( isChanged ){
    /* If a new wal-index header was loaded before the checkpoint was
    ** performed, then the pager-cache associated with pWal is now
    ** out of date. So zero the cached wal-index header to ensure that
    ** next time the pager opens a snapshot on this database it knows that
    ** the cache needs to be reset.
    */
    memset(&pWal->hdr, 0, sizeof(WalIndexHdr));
  }

  walDisableBlocking(pWal);
  sqlite3WalDb(pWal, 0);

  /* Release the locks. */
  sqlite3WalEndWriteTransaction(pWal);
  if( pWal->ckptLock ){
    walUnlockExclusive(pWal, WAL_CKPT_LOCK, 1);
    pWal->ckptLock = 0;
  }
  WALTRACE(("WAL%p: checkpoint %s\n", pWal, rc ? "failed" : "ok"));
#ifdef SQLITE_ENABLE_SETLK_TIMEOUT
  if( rc==SQLITE_BUSY_TIMEOUT ) rc = SQLITE_BUSY;
#endif
  return (rc==SQLITE_OK && eMode!=eMode2 ? SQLITE_BUSY : rc);
}
