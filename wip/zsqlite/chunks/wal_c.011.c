
/* Return the value to pass to a sqlite3_wal_hook callback, the
** number of frames in the WAL at the point of the last commit since
** sqlite3WalCallback() was called.  If no commits have occurred since
** the last call, then return 0.
*/
SQLITE_PRIVATE int sqlite3WalCallback(Wal *pWal){
  u32 ret = 0;
  if( pWal ){
    ret = pWal->iCallback;
    pWal->iCallback = 0;
  }
  return (int)ret;
}

/*
** This function is called to change the WAL subsystem into or out
** of locking_mode=EXCLUSIVE.
**
** If op is zero, then attempt to change from locking_mode=EXCLUSIVE
** into locking_mode=NORMAL.  This means that we must acquire a lock
** on the pWal->readLock byte.  If the WAL is already in locking_mode=NORMAL
** or if the acquisition of the lock fails, then return 0.  If the
** transition out of exclusive-mode is successful, return 1.  This
** operation must occur while the pager is still holding the exclusive
** lock on the main database file.
**
** If op is one, then change from locking_mode=NORMAL into
** locking_mode=EXCLUSIVE.  This means that the pWal->readLock must
** be released.  Return 1 if the transition is made and 0 if the
** WAL is already in exclusive-locking mode - meaning that this
** routine is a no-op.  The pager must already hold the exclusive lock
** on the main database file before invoking this operation.
**
** If op is negative, then do a dry-run of the op==1 case but do
** not actually change anything. The pager uses this to see if it
** should acquire the database exclusive lock prior to invoking
** the op==1 case.
*/
SQLITE_PRIVATE int sqlite3WalExclusiveMode(Wal *pWal, int op){
  int rc;
  assert( pWal->writeLock==0 );
  assert( pWal->exclusiveMode!=WAL_HEAPMEMORY_MODE || op==-1 );

  /* pWal->readLock is usually set, but might be -1 if there was a
  ** prior error while attempting to acquire are read-lock. This cannot
  ** happen if the connection is actually in exclusive mode (as no xShmLock
  ** locks are taken in this case). Nor should the pager attempt to
  ** upgrade to exclusive-mode following such an error.
  */
#ifndef SQLITE_USE_SEH
  assert( pWal->readLock>=0 || pWal->lockError );
#endif
  assert( pWal->readLock>=0 || (op<=0 && pWal->exclusiveMode==0) );

  if( op==0 ){
    if( pWal->exclusiveMode!=WAL_NORMAL_MODE ){
      pWal->exclusiveMode = WAL_NORMAL_MODE;
      if( walLockShared(pWal, WAL_READ_LOCK(pWal->readLock))!=SQLITE_OK ){
        pWal->exclusiveMode = WAL_EXCLUSIVE_MODE;
      }
      rc = pWal->exclusiveMode==WAL_NORMAL_MODE;
    }else{
      /* Already in locking_mode=NORMAL */
      rc = 0;
    }
  }else if( op>0 ){
    assert( pWal->exclusiveMode==WAL_NORMAL_MODE );
    assert( pWal->readLock>=0 );
    walUnlockShared(pWal, WAL_READ_LOCK(pWal->readLock));
    pWal->exclusiveMode = WAL_EXCLUSIVE_MODE;
    rc = 1;
  }else{
    rc = pWal->exclusiveMode==WAL_NORMAL_MODE;
  }
  return rc;
}

/*
** Return true if the argument is non-NULL and the WAL module is using
** heap-memory for the wal-index. Otherwise, if the argument is NULL or the
** WAL module is using shared-memory, return false.
*/
SQLITE_PRIVATE int sqlite3WalHeapMemory(Wal *pWal){
  return (pWal && pWal->exclusiveMode==WAL_HEAPMEMORY_MODE );
}

#ifdef SQLITE_ENABLE_SNAPSHOT
/* Create a snapshot object.  The content of a snapshot is opaque to
** every other subsystem, so the WAL module can put whatever it needs
** in the object.
*/
SQLITE_PRIVATE int sqlite3WalSnapshotGet(Wal *pWal, sqlite3_snapshot **ppSnapshot){
  int rc = SQLITE_OK;
  WalIndexHdr *pRet;
  static const u32 aZero[4] = { 0, 0, 0, 0 };

  assert( pWal->readLock>=0 && pWal->writeLock==0 );

  if( memcmp(&pWal->hdr.aFrameCksum[0],aZero,16)==0 ){
    *ppSnapshot = 0;
    return SQLITE_ERROR;
  }
  pRet = (WalIndexHdr*)sqlite3_malloc(sizeof(WalIndexHdr));
  if( pRet==0 ){
    rc = SQLITE_NOMEM_BKPT;
  }else{
    memcpy(pRet, &pWal->hdr, sizeof(WalIndexHdr));
    *ppSnapshot = (sqlite3_snapshot*)pRet;
  }

  return rc;
}

/* Try to open on pSnapshot when the next read-transaction starts
*/
SQLITE_PRIVATE void sqlite3WalSnapshotOpen(
  Wal *pWal,
  sqlite3_snapshot *pSnapshot
){
  pWal->pSnapshot = (WalIndexHdr*)pSnapshot;
}

/*
** Return a +ve value if snapshot p1 is newer than p2. A -ve value if
** p1 is older than p2 and zero if p1 and p2 are the same snapshot.
*/
SQLITE_API int sqlite3_snapshot_cmp(sqlite3_snapshot *p1, sqlite3_snapshot *p2){
  WalIndexHdr *pHdr1 = (WalIndexHdr*)p1;
  WalIndexHdr *pHdr2 = (WalIndexHdr*)p2;

  /* aSalt[0] is a copy of the value stored in the wal file header. It
  ** is incremented each time the wal file is restarted.  */
  if( pHdr1->aSalt[0]<pHdr2->aSalt[0] ) return -1;
  if( pHdr1->aSalt[0]>pHdr2->aSalt[0] ) return +1;
  if( pHdr1->mxFrame<pHdr2->mxFrame ) return -1;
  if( pHdr1->mxFrame>pHdr2->mxFrame ) return +1;
  return 0;
}

/*
** The caller currently has a read transaction open on the database.
** This function takes a SHARED lock on the CHECKPOINTER slot and then
** checks if the snapshot passed as the second argument is still
** available. If so, SQLITE_OK is returned.
**
** If the snapshot is not available, SQLITE_ERROR is returned. Or, if
** the CHECKPOINTER lock cannot be obtained, SQLITE_BUSY. If any error
** occurs (any value other than SQLITE_OK is returned), the CHECKPOINTER
** lock is released before returning.
*/
SQLITE_PRIVATE int sqlite3WalSnapshotCheck(Wal *pWal, sqlite3_snapshot *pSnapshot){
  int rc;
  SEH_TRY {
    rc = walLockShared(pWal, WAL_CKPT_LOCK);
    if( rc==SQLITE_OK ){
      WalIndexHdr *pNew = (WalIndexHdr*)pSnapshot;
      if( memcmp(pNew->aSalt, pWal->hdr.aSalt, sizeof(pWal->hdr.aSalt))
       || pNew->mxFrame<walCkptInfo(pWal)->nBackfillAttempted
      ){
        rc = SQLITE_ERROR_SNAPSHOT;
        walUnlockShared(pWal, WAL_CKPT_LOCK);
      }
    }
  }
  SEH_EXCEPT( rc = walHandleException(pWal); )
  return rc;
}

/*
** Release a lock obtained by an earlier successful call to
** sqlite3WalSnapshotCheck().
*/
SQLITE_PRIVATE void sqlite3WalSnapshotUnlock(Wal *pWal){
  assert( pWal );
  walUnlockShared(pWal, WAL_CKPT_LOCK);
}


#endif /* SQLITE_ENABLE_SNAPSHOT */

#ifdef SQLITE_ENABLE_ZIPVFS
/*
** If the argument is not NULL, it points to a Wal object that holds a
** read-lock. This function returns the database page-size if it is known,
** or zero if it is not (or if pWal is NULL).
*/
SQLITE_PRIVATE int sqlite3WalFramesize(Wal *pWal){
  assert( pWal==0 || pWal->readLock>=0 );
  return (pWal ? pWal->szPage : 0);
}
#endif

/* Return the sqlite3_file object for the WAL file
*/
SQLITE_PRIVATE sqlite3_file *sqlite3WalFile(Wal *pWal){
  return pWal->pWalFd;
}

#endif /* #ifndef SQLITE_OMIT_WAL */

