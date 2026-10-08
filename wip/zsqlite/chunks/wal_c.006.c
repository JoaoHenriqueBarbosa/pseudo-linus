
/*
** If the WAL file is currently larger than nMax bytes in size, truncate
** it to exactly nMax bytes. If an error occurs while doing so, ignore it.
*/
static void walLimitSize(Wal *pWal, i64 nMax){
  i64 sz;
  int rx;
  sqlite3BeginBenignMalloc();
  rx = sqlite3OsFileSize(pWal->pWalFd, &sz);
  if( rx==SQLITE_OK && (sz > nMax ) ){
    rx = sqlite3OsTruncate(pWal->pWalFd, nMax);
  }
  sqlite3EndBenignMalloc();
  if( rx ){
    sqlite3_log(rx, "cannot limit WAL size: %s", pWal->zWalName);
  }
}

#ifdef SQLITE_USE_SEH
/*
** This is the "standard" exception handler used in a few places to handle
** an exception thrown by reading from the *-shm mapping after it has become
** invalid in SQLITE_USE_SEH builds. It is used as follows:
**
**   SEH_TRY { ... }
**   SEH_EXCEPT( rc = walHandleException(pWal); )
**
** This function does three things:
**
**   1) Determines the locks that should be held, based on the contents of
**      the Wal.readLock, Wal.writeLock and Wal.ckptLock variables. All other
**      held locks are assumed to be transient locks that would have been
**      released had the exception not been thrown and are dropped.
**
**   2) Frees the pointer at Wal.pFree, if any, using sqlite3_free().
**
**   3) Set pWal->apWiData[pWal->iWiPg] to pWal->pWiValue if not NULL
**
**   4) Returns SQLITE_IOERR.
*/
static int walHandleException(Wal *pWal){
  if( pWal->exclusiveMode==0 ){
    static const int S = 1;
    static const int E = (1<<SQLITE_SHM_NLOCK);
    int ii;
    u32 mUnlock = pWal->lockMask & ~(
        (pWal->readLock<0 ? 0 : (S << WAL_READ_LOCK(pWal->readLock)))
        | (pWal->writeLock ? (E << WAL_WRITE_LOCK) : 0)
        | (pWal->ckptLock ? (E << WAL_CKPT_LOCK) : 0)
        );
    for(ii=0; ii<SQLITE_SHM_NLOCK; ii++){
      if( (S<<ii) & mUnlock ) walUnlockShared(pWal, ii);
      if( (E<<ii) & mUnlock ) walUnlockExclusive(pWal, ii, 1);
    }
  }
  sqlite3_free(pWal->pFree);
  pWal->pFree = 0;
  if( pWal->pWiValue ){
    pWal->apWiData[pWal->iWiPg] = pWal->pWiValue;
    pWal->pWiValue = 0;
  }
  return SQLITE_IOERR_IN_PAGE;
}

/*
** Assert that the Wal.lockMask mask, which indicates the locks held
** by the connenction, is consistent with the Wal.readLock, Wal.writeLock
** and Wal.ckptLock variables. To be used as:
**
**   assert( walAssertLockmask(pWal) );
*/
static int walAssertLockmask(Wal *pWal){
  if( pWal->exclusiveMode==0 ){
    static const int S = 1;
    static const int E = (1<<SQLITE_SHM_NLOCK);
    u32 mExpect = (
        (pWal->readLock<0 ? 0 : (S << WAL_READ_LOCK(pWal->readLock)))
      | (pWal->writeLock ? (E << WAL_WRITE_LOCK) : 0)
      | (pWal->ckptLock ? (E << WAL_CKPT_LOCK) : 0)
#ifdef SQLITE_ENABLE_SNAPSHOT
      | (pWal->pSnapshot ? (pWal->lockMask & (1 << WAL_CKPT_LOCK)) : 0)
#endif
    );
    assert( mExpect==pWal->lockMask );
  }
  return 1;
}

/*
** Return and zero the "system error" field set when an
** EXCEPTION_IN_PAGE_ERROR exception is caught.
*/
SQLITE_PRIVATE int sqlite3WalSystemErrno(Wal *pWal){
  int iRet = 0;
  if( pWal ){
    iRet = pWal->iSysErrno;
    pWal->iSysErrno = 0;
  }
  return iRet;
}

#else
# define walAssertLockmask(x) 1
#endif /* ifdef SQLITE_USE_SEH */

/*
** Close a connection to a log file.
*/
SQLITE_PRIVATE int sqlite3WalClose(
  Wal *pWal,                      /* Wal to close */
  sqlite3 *db,                    /* For interrupt flag */
  int sync_flags,                 /* Flags to pass to OsSync() (or 0) */
  int nBuf,
  u8 *zBuf                        /* Buffer of at least nBuf bytes */
){
  int rc = SQLITE_OK;
  if( pWal ){
    int isDelete = 0;             /* True to unlink wal and wal-index files */

    assert( walAssertLockmask(pWal) );

    /* If an EXCLUSIVE lock can be obtained on the database file (using the
    ** ordinary, rollback-mode locking methods, this guarantees that the
    ** connection associated with this log file is the only connection to
    ** the database. In this case checkpoint the database and unlink both
    ** the wal and wal-index files.
    **
    ** The EXCLUSIVE lock is not released before returning.
    */
    if( zBuf!=0
     && SQLITE_OK==(rc = sqlite3OsLock(pWal->pDbFd, SQLITE_LOCK_EXCLUSIVE))
    ){
      if( pWal->exclusiveMode==WAL_NORMAL_MODE ){
        pWal->exclusiveMode = WAL_EXCLUSIVE_MODE;
      }
      rc = sqlite3WalCheckpoint(pWal, db,
          SQLITE_CHECKPOINT_PASSIVE, 0, 0, sync_flags, nBuf, zBuf, 0, 0
      );
      if( rc==SQLITE_OK ){
        int bPersist = -1;
        sqlite3OsFileControlHint(
            pWal->pDbFd, SQLITE_FCNTL_PERSIST_WAL, &bPersist
        );
        if( bPersist!=1 ){
          /* Try to delete the WAL file if the checkpoint completed and
          ** fsynced (rc==SQLITE_OK) and if we are not in persistent-wal
          ** mode (!bPersist) */
          isDelete = 1;
        }else if( pWal->mxWalSize>=0 ){
          /* Try to truncate the WAL file to zero bytes if the checkpoint
          ** completed and fsynced (rc==SQLITE_OK) and we are in persistent
          ** WAL mode (bPersist) and if the PRAGMA journal_size_limit is a
          ** non-negative value (pWal->mxWalSize>=0).  Note that we truncate
          ** to zero bytes as truncating to the journal_size_limit might
          ** leave a corrupt WAL file on disk. */
          walLimitSize(pWal, 0);
        }
      }
    }

    walIndexClose(pWal, isDelete);
    sqlite3OsClose(pWal->pWalFd);
    if( isDelete ){
      sqlite3BeginBenignMalloc();
      sqlite3OsDelete(pWal->pVfs, pWal->zWalName, 0);
      sqlite3EndBenignMalloc();
    }
    WALTRACE(("WAL%p: closed\n", pWal));
    sqlite3_free((void *)pWal->apWiData);
    sqlite3_free(pWal);
  }
  return rc;
}

/*
** Try to read the wal-index header.  Return 0 on success and 1 if
** there is a problem.
**
** The wal-index is in shared memory.  Another thread or process might
** be writing the header at the same time this procedure is trying to
** read it, which might result in inconsistency.  A dirty read is detected
** by verifying that both copies of the header are the same and also by
** a checksum on the header.
**
** If and only if the read is consistent and the header is different from
** pWal->hdr, then pWal->hdr is updated to the content of the new header
** and *pChanged is set to 1.
**
** If the checksum cannot be verified return non-zero. If the header
** is read successfully and the checksum verified, return zero.
*/
static SQLITE_NO_TSAN int walIndexTryHdr(Wal *pWal, int *pChanged){
  u32 aCksum[2];                  /* Checksum on the header content */
  WalIndexHdr h1, h2;             /* Two copies of the header content */
  WalIndexHdr volatile *aHdr;     /* Header in shared memory */

  /* The first page of the wal-index must be mapped at this point. */
  assert( pWal->nWiData>0 && pWal->apWiData[0] );

  /* Read the header. This might happen concurrently with a write to the
  ** same area of shared memory on a different CPU in a SMP,
  ** meaning it is possible that an inconsistent snapshot is read
  ** from the file. If this happens, return non-zero.
  **
  ** tag-20200519-1:
  ** There are two copies of the header at the beginning of the wal-index.
  ** When reading, read [0] first then [1].  Writes are in the reverse order.
  ** Memory barriers are used to prevent the compiler or the hardware from
  ** reordering the reads and writes.  TSAN and similar tools can sometimes
  ** give false-positive warnings about these accesses because the tools do not
  ** account for the double-read and the memory barrier. The use of mutexes
  ** here would be problematic as the memory being accessed is potentially
  ** shared among multiple processes and not all mutex implementations work
  ** reliably in that environment.
  */
  aHdr = walIndexHdr(pWal);
  memcpy(&h1, (void *)&aHdr[0], sizeof(h1)); /* Possible TSAN false-positive */
  walShmBarrier(pWal);
  memcpy(&h2, (void *)&aHdr[1], sizeof(h2));

  if( memcmp(&h1, &h2, sizeof(h1))!=0 ){
    return 1;   /* Dirty read */
  }
  if( h1.isInit==0 ){
    return 1;   /* Malformed header - probably all zeros */
  }
  walChecksumBytes(1, (u8*)&h1, sizeof(h1)-sizeof(h1.aCksum), 0, aCksum);
  if( aCksum[0]!=h1.aCksum[0] || aCksum[1]!=h1.aCksum[1] ){
    return 1;   /* Checksum does not match */
  }

  if( memcmp(&pWal->hdr, &h1, sizeof(WalIndexHdr)) ){
    *pChanged = 1;
    memcpy(&pWal->hdr, &h1, sizeof(WalIndexHdr));
    pWal->szPage = (pWal->hdr.szPage&0xfe00) + ((pWal->hdr.szPage&0x0001)<<16);
    testcase( pWal->szPage<=32768 );
    testcase( pWal->szPage>=65536 );
  }

  /* The header was successfully read. Return zero. */
  return 0;
}

/*
** This is the value that walTryBeginRead returns when it needs to
** be retried.
*/
#define WAL_RETRY  (-1)

/*
** Read the wal-index header from the wal-index and into pWal->hdr.
** If the wal-header appears to be corrupt, try to reconstruct the
** wal-index from the WAL before returning.
**
** Set *pChanged to 1 if the wal-index header value in pWal->hdr is
** changed by this operation.  If pWal->hdr is unchanged, set *pChanged
** to 0.
**
** If the wal-index header is successfully read, return SQLITE_OK.
** Otherwise an SQLite error code.
*/
static int walIndexReadHdr(Wal *pWal, int *pChanged){
  int rc;                         /* Return code */
  int badHdr;                     /* True if a header read failed */
  volatile u32 *page0;            /* Chunk of wal-index containing header */

  /* Ensure that page 0 of the wal-index (the page that contains the
  ** wal-index header) is mapped. Return early if an error occurs here.
  */
  assert( pChanged );
  rc = walIndexPage(pWal, 0, &page0);
  if( rc!=SQLITE_OK ){
    assert( rc!=SQLITE_READONLY ); /* READONLY changed to OK in walIndexPage */
    if( rc==SQLITE_READONLY_CANTINIT ){
      /* The SQLITE_READONLY_CANTINIT return means that the shared-memory
      ** was openable but is not writable, and this thread is unable to
      ** confirm that another write-capable connection has the shared-memory
      ** open, and hence the content of the shared-memory is unreliable,
      ** since the shared-memory might be inconsistent with the WAL file
      ** and there is no writer on hand to fix it. */
      assert( page0==0 );
      assert( pWal->writeLock==0 );
      assert( pWal->readOnly & WAL_SHM_RDONLY );
      pWal->bShmUnreliable = 1;
      pWal->exclusiveMode = WAL_HEAPMEMORY_MODE;
      *pChanged = 1;
    }else{
      return rc; /* Any other non-OK return is just an error */
    }
  }else{
    /* page0 can be NULL if the SHM is zero bytes in size and pWal->writeLock
    ** is zero, which prevents the SHM from growing */
    testcase( page0!=0 );
  }
  assert( page0!=0 || pWal->writeLock==0 );

  /* If the first page of the wal-index has been mapped, try to read the
  ** wal-index header immediately, without holding any lock. This usually
  ** works, but may fail if the wal-index header is corrupt or currently
  ** being modified by another thread or process.
  */
  badHdr = (page0 ? walIndexTryHdr(pWal, pChanged) : 1);

  /* If the first attempt failed, it might have been due to a race
  ** with a writer.  So get a WRITE lock and try again.
  */
  if( badHdr ){
    if( pWal->bShmUnreliable==0 && (pWal->readOnly & WAL_SHM_RDONLY) ){
      if( SQLITE_OK==(rc = walLockShared(pWal, WAL_WRITE_LOCK)) ){
        walUnlockShared(pWal, WAL_WRITE_LOCK);
        rc = SQLITE_READONLY_RECOVERY;
      }
    }else{
      int bWriteLock = pWal->writeLock;
      if( bWriteLock
       || SQLITE_OK==(rc = walLockExclusive(pWal, WAL_WRITE_LOCK, 1))
      ){
        pWal->writeLock = 1;
        if( SQLITE_OK==(rc = walIndexPage(pWal, 0, &page0)) ){
          badHdr = walIndexTryHdr(pWal, pChanged);
          if( badHdr ){
            /* If the wal-index header is still malformed even while holding
            ** a WRITE lock, it can only mean that the header is corrupted and
            ** needs to be reconstructed.  So run recovery to do exactly that.
            ** Disable blocking locks first.  */
            walDisableBlocking(pWal);
            rc = walIndexRecover(pWal);
            *pChanged = 1;
          }
        }
        if( bWriteLock==0 ){
          pWal->writeLock = 0;
          walUnlockExclusive(pWal, WAL_WRITE_LOCK, 1);
        }
      }
    }
  }

  /* If the header is read successfully, check the version number to make
  ** sure the wal-index was not constructed with some future format that
  ** this version of SQLite cannot understand.
  */
  if( badHdr==0 && pWal->hdr.iVersion!=WALINDEX_MAX_VERSION ){
    rc = SQLITE_CANTOPEN_BKPT;
  }
  if( pWal->bShmUnreliable ){
    if( rc!=SQLITE_OK ){
      walIndexClose(pWal, 0);
      pWal->bShmUnreliable = 0;
      assert( pWal->nWiData>0 && pWal->apWiData[0]==0 );
      /* walIndexRecover() might have returned SHORT_READ if a concurrent
      ** writer truncated the WAL out from under it.  If that happens, it
      ** indicates that a writer has fixed the SHM file for us, so retry */
      if( rc==SQLITE_IOERR_SHORT_READ ) rc = WAL_RETRY;
    }
    pWal->exclusiveMode = WAL_NORMAL_MODE;
  }

  return rc;
}
