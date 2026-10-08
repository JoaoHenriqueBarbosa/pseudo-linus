
/*
** Return the current journal mode.
*/
SQLITE_PRIVATE int sqlite3PagerGetJournalMode(Pager *pPager){
  return (int)pPager->journalMode;
}

/*
** Return TRUE if the pager is in a state where it is OK to change the
** journalmode.  Journalmode changes can only happen when the database
** is unmodified.
*/
SQLITE_PRIVATE int sqlite3PagerOkToChangeJournalMode(Pager *pPager){
  assert( assert_pager_state(pPager) );
  if( pPager->eState>=PAGER_WRITER_CACHEMOD ) return 0;
  if( NEVER(isOpen(pPager->jfd) && pPager->journalOff>0) ) return 0;
  return 1;
}

/*
** Get/set the size-limit used for persistent journal files.
**
** Setting the size limit to -1 means no limit is enforced.
** An attempt to set a limit smaller than -1 is a no-op.
*/
SQLITE_PRIVATE i64 sqlite3PagerJournalSizeLimit(Pager *pPager, i64 iLimit){
  if( iLimit>=-1 ){
    pPager->journalSizeLimit = iLimit;
    sqlite3WalLimit(pPager->pWal, iLimit);
  }
  return pPager->journalSizeLimit;
}

/*
** Return a pointer to the pPager->pBackup variable. The backup module
** in backup.c maintains the content of this variable. This module
** uses it opaquely as an argument to sqlite3BackupRestart() and
** sqlite3BackupUpdate() only.
*/
SQLITE_PRIVATE sqlite3_backup **sqlite3PagerBackupPtr(Pager *pPager){
  return &pPager->pBackup;
}

#ifndef SQLITE_OMIT_VACUUM
/*
** Unless this is an in-memory or temporary database, clear the pager cache.
*/
SQLITE_PRIVATE void sqlite3PagerClearCache(Pager *pPager){
  assert( MEMDB==0 || pPager->tempFile );
  if( pPager->tempFile==0 ) pager_reset(pPager);
}
#endif


#ifndef SQLITE_OMIT_WAL
/*
** This function is called when the user invokes "PRAGMA wal_checkpoint",
** "PRAGMA wal_blocking_checkpoint" or calls the sqlite3_wal_checkpoint()
** or wal_blocking_checkpoint() API functions.
**
** Parameter eMode is one of SQLITE_CHECKPOINT_PASSIVE, FULL or RESTART.
*/
SQLITE_PRIVATE int sqlite3PagerCheckpoint(
  Pager *pPager,                  /* Checkpoint on this pager */
  sqlite3 *db,                    /* Db handle used to check for interrupts */
  int eMode,                      /* Type of checkpoint */
  int *pnLog,                     /* OUT: Final number of frames in log */
  int *pnCkpt                     /* OUT: Final number of checkpointed frames */
){
  int rc = SQLITE_OK;
  if( pPager->pWal==0 && pPager->journalMode==PAGER_JOURNALMODE_WAL ){
    /* This only happens when a database file is zero bytes in size opened and
    ** then "PRAGMA journal_mode=WAL" is run and then sqlite3_wal_checkpoint()
    ** is invoked without any intervening transactions.  We need to start
    ** a transaction to initialize pWal.  The PRAGMA table_list statement is
    ** used for this since it starts transactions on every database file,
    ** including all ATTACHed databases.  This seems expensive for a single
    ** sqlite3_wal_checkpoint() call, but it happens very rarely.
    ** https://sqlite.org/forum/forumpost/fd0f19d229156939
    */
    sqlite3_exec(db, "PRAGMA table_list",0,0,0);
  }
  if( pPager->pWal ){
    rc = sqlite3WalCheckpoint(pPager->pWal, db, eMode,
        (eMode==SQLITE_CHECKPOINT_PASSIVE ? 0 : pPager->xBusyHandler),
        pPager->pBusyHandlerArg,
        pPager->walSyncFlags, pPager->pageSize, (u8 *)pPager->pTmpSpace,
        pnLog, pnCkpt
    );
  }
  return rc;
}

SQLITE_PRIVATE int sqlite3PagerWalCallback(Pager *pPager){
  return sqlite3WalCallback(pPager->pWal);
}

/*
** Return true if the underlying VFS for the given pager supports the
** primitives necessary for write-ahead logging.
*/
SQLITE_PRIVATE int sqlite3PagerWalSupported(Pager *pPager){
  const sqlite3_io_methods *pMethods = pPager->fd->pMethods;
  if( pPager->noLock ) return 0;
  return pPager->exclusiveMode || (pMethods->iVersion>=2 && pMethods->xShmMap);
}

/*
** Attempt to take an exclusive lock on the database file. If a PENDING lock
** is obtained instead, immediately release it.
*/
static int pagerExclusiveLock(Pager *pPager){
  int rc;                         /* Return code */
  u8 eOrigLock;                   /* Original lock */

  assert( pPager->eLock>=SHARED_LOCK );
  eOrigLock = pPager->eLock;
  rc = pagerLockDb(pPager, EXCLUSIVE_LOCK);
  if( rc!=SQLITE_OK ){
    /* If the attempt to grab the exclusive lock failed, release the
    ** pending lock that may have been obtained instead.  */
    pagerUnlockDb(pPager, eOrigLock);
  }

  return rc;
}

/*
** Call sqlite3WalOpen() to open the WAL handle. If the pager is in
** exclusive-locking mode when this function is called, take an EXCLUSIVE
** lock on the database file and use heap-memory to store the wal-index
** in. Otherwise, use the normal shared-memory.
*/
static int pagerOpenWal(Pager *pPager){
  int rc = SQLITE_OK;

  assert( pPager->pWal==0 && pPager->tempFile==0 );
  assert( pPager->eLock==SHARED_LOCK || pPager->eLock==EXCLUSIVE_LOCK );

  /* If the pager is already in exclusive-mode, the WAL module will use
  ** heap-memory for the wal-index instead of the VFS shared-memory
  ** implementation. Take the exclusive lock now, before opening the WAL
  ** file, to make sure this is safe.
  */
  if( pPager->exclusiveMode ){
    rc = pagerExclusiveLock(pPager);
  }

  /* Open the connection to the log file. If this operation fails,
  ** (e.g. due to malloc() failure), return an error code.
  */
  if( rc==SQLITE_OK ){
    rc = sqlite3WalOpen(pPager->pVfs,
        pPager->fd, pPager->zWal, pPager->exclusiveMode,
        pPager->journalSizeLimit, &pPager->pWal
    );
  }
  pagerFixMaplimit(pPager);

  return rc;
}


/*
** The caller must be holding a SHARED lock on the database file to call
** this function.
**
** If the pager passed as the first argument is open on a real database
** file (not a temp file or an in-memory database), and the WAL file
** is not already open, make an attempt to open it now. If successful,
** return SQLITE_OK. If an error occurs or the VFS used by the pager does
** not support the xShmXXX() methods, return an error code. *pbOpen is
** not modified in either case.
**
** If the pager is open on a temp-file (or in-memory database), or if
** the WAL file is already open, set *pbOpen to 1 and return SQLITE_OK
** without doing anything.
*/
SQLITE_PRIVATE int sqlite3PagerOpenWal(
  Pager *pPager,                  /* Pager object */
  int *pbOpen                     /* OUT: Set to true if call is a no-op */
){
  int rc = SQLITE_OK;             /* Return code */

  assert( assert_pager_state(pPager) );
  assert( pPager->eState==PAGER_OPEN   || pbOpen );
  assert( pPager->eState==PAGER_READER || !pbOpen );
  assert( pbOpen==0 || *pbOpen==0 );
  assert( pbOpen!=0 || (!pPager->tempFile && !pPager->pWal) );

  if( !pPager->tempFile && !pPager->pWal ){
    if( !sqlite3PagerWalSupported(pPager) ) return SQLITE_CANTOPEN;

    /* Close any rollback journal previously open */
    sqlite3OsClose(pPager->jfd);

    rc = pagerOpenWal(pPager);
    if( rc==SQLITE_OK ){
      pPager->journalMode = PAGER_JOURNALMODE_WAL;
      pPager->eState = PAGER_OPEN;
    }
  }else{
    *pbOpen = 1;
  }

  return rc;
}

/*
** This function is called to close the connection to the log file prior
** to switching from WAL to rollback mode.
**
** Before closing the log file, this function attempts to take an
** EXCLUSIVE lock on the database file. If this cannot be obtained, an
** error (SQLITE_BUSY) is returned and the log connection is not closed.
** If successful, the EXCLUSIVE lock is not released before returning.
*/
SQLITE_PRIVATE int sqlite3PagerCloseWal(Pager *pPager, sqlite3 *db){
  int rc = SQLITE_OK;

  assert( pPager->journalMode==PAGER_JOURNALMODE_WAL );

  /* If the log file is not already open, but does exist in the file-system,
  ** it may need to be checkpointed before the connection can switch to
  ** rollback mode. Open it now so this can happen.
  */
  if( !pPager->pWal ){
    int logexists = 0;
    rc = pagerLockDb(pPager, SHARED_LOCK);
    if( rc==SQLITE_OK ){
      rc = sqlite3OsAccess(
          pPager->pVfs, pPager->zWal, SQLITE_ACCESS_EXISTS, &logexists
      );
    }
    if( rc==SQLITE_OK && logexists ){
      rc = pagerOpenWal(pPager);
    }
  }

  /* Checkpoint and close the log. Because an EXCLUSIVE lock is held on
  ** the database file, the log and log-summary files will be deleted.
  */
  if( rc==SQLITE_OK && pPager->pWal ){
    rc = pagerExclusiveLock(pPager);
    if( rc==SQLITE_OK ){
      rc = sqlite3WalClose(pPager->pWal, db, pPager->walSyncFlags,
                           pPager->pageSize, (u8*)pPager->pTmpSpace);
      pPager->pWal = 0;
      pagerFixMaplimit(pPager);
      if( rc && !pPager->exclusiveMode ) pagerUnlockDb(pPager, SHARED_LOCK);
    }
  }
  return rc;
}

#ifdef SQLITE_ENABLE_SETLK_TIMEOUT
/*
** If pager pPager is a wal-mode database not in exclusive locking mode,
** invoke the sqlite3WalWriteLock() function on the associated Wal object
** with the same db and bLock parameters as were passed to this function.
** Return an SQLite error code if an error occurs, or SQLITE_OK otherwise.
*/
SQLITE_PRIVATE int sqlite3PagerWalWriteLock(Pager *pPager, int bLock){
  int rc = SQLITE_OK;
  if( pagerUseWal(pPager) && pPager->exclusiveMode==0 ){
    rc = sqlite3WalWriteLock(pPager->pWal, bLock);
  }
  return rc;
}

/*
** Set the database handle used by the wal layer to determine if
** blocking locks are required.
*/
SQLITE_PRIVATE void sqlite3PagerWalDb(Pager *pPager, sqlite3 *db){
  if( pagerUseWal(pPager) ){
    sqlite3WalDb(pPager->pWal, db);
  }
}
#endif

#ifdef SQLITE_ENABLE_SNAPSHOT
/*
** If this is a WAL database, obtain a snapshot handle for the snapshot
** currently open. Otherwise, return an error.
*/
SQLITE_PRIVATE int sqlite3PagerSnapshotGet(Pager *pPager, sqlite3_snapshot **ppSnapshot){
  int rc = SQLITE_ERROR;
  if( pPager->pWal ){
    rc = sqlite3WalSnapshotGet(pPager->pWal, ppSnapshot);
  }
  return rc;
}

/*
** If this is a WAL database, store a pointer to pSnapshot. Next time a
** read transaction is opened, attempt to read from the snapshot it
** identifies. If this is not a WAL database, return an error.
*/
SQLITE_PRIVATE int sqlite3PagerSnapshotOpen(
  Pager *pPager,
  sqlite3_snapshot *pSnapshot
){
  int rc = SQLITE_OK;
  if( pPager->pWal ){
    sqlite3WalSnapshotOpen(pPager->pWal, pSnapshot);
  }else{
    rc = SQLITE_ERROR;
  }
  return rc;
}

/*
** If this is a WAL database, call sqlite3WalSnapshotRecover(). If this
** is not a WAL database, return an error.
*/
SQLITE_PRIVATE int sqlite3PagerSnapshotRecover(Pager *pPager){
  int rc;
  if( pPager->pWal ){
    rc = sqlite3WalSnapshotRecover(pPager->pWal);
  }else{
    rc = SQLITE_ERROR;
  }
  return rc;
}

/*
** The caller currently has a read transaction open on the database.
** If this is not a WAL database, SQLITE_ERROR is returned. Otherwise,
** this function takes a SHARED lock on the CHECKPOINTER slot and then
** checks if the snapshot passed as the second argument is still
** available. If so, SQLITE_OK is returned.
**
** If the snapshot is not available, SQLITE_ERROR is returned. Or, if
** the CHECKPOINTER lock cannot be obtained, SQLITE_BUSY. If any error
** occurs (any value other than SQLITE_OK is returned), the CHECKPOINTER
** lock is released before returning.
*/
SQLITE_PRIVATE int sqlite3PagerSnapshotCheck(Pager *pPager, sqlite3_snapshot *pSnapshot){
  int rc;
  if( pPager->pWal ){
    rc = sqlite3WalSnapshotCheck(pPager->pWal, pSnapshot);
  }else{
    rc = SQLITE_ERROR;
  }
  return rc;
}

/*
** Release a lock obtained by an earlier successful call to
** sqlite3PagerSnapshotCheck().
*/
SQLITE_PRIVATE void sqlite3PagerSnapshotUnlock(Pager *pPager){
  assert( pPager->pWal );
  sqlite3WalSnapshotUnlock(pPager->pWal);
}
