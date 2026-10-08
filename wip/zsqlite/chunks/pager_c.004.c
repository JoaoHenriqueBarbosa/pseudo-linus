

/*
** Write the supplied super-journal name into the journal file for pager
** pPager at the current location. The super-journal name must be the last
** thing written to a journal file. If the pager is in full-sync mode, the
** journal file descriptor is advanced to the next sector boundary before
** anything is written. The format is:
**
**   + 4 bytes: PAGER_SJ_PGNO.
**   + N bytes: super-journal filename in utf-8.
**   + 4 bytes: N (length of super-journal name in bytes, no nul-terminator).
**   + 4 bytes: super-journal name checksum.
**   + 8 bytes: aJournalMagic[].
**
** The super-journal page checksum is the sum of the bytes in the super-journal
** name, where each byte is interpreted as a signed 8-bit integer.
**
** If zSuper is a NULL pointer (occurs for a single database transaction),
** this call is a no-op.
*/
static int writeSuperJournal(Pager *pPager, const char *zSuper){
  int rc;                          /* Return code */
  int nSuper;                      /* Length of string zSuper */
  i64 iHdrOff;                     /* Offset of header in journal file */
  i64 jrnlSize;                    /* Size of journal file on disk */
  u32 cksum = 0;                   /* Checksum of string zSuper */

  assert( pPager->setSuper==0 );
  assert( !pagerUseWal(pPager) );

  if( !zSuper
   || pPager->journalMode==PAGER_JOURNALMODE_MEMORY
   || !isOpen(pPager->jfd)
  ){
    return SQLITE_OK;
  }
  pPager->setSuper = 1;
  assert( pPager->journalHdr <= pPager->journalOff );

  /* Calculate the length in bytes and the checksum of zSuper */
  for(nSuper=0; zSuper[nSuper]; nSuper++){
    cksum += zSuper[nSuper];
  }

  /* If in full-sync mode, advance to the next disk sector before writing
  ** the super-journal name. This is in case the previous page written to
  ** the journal has already been synced.
  */
  if( pPager->fullSync ){
    pPager->journalOff = journalHdrOffset(pPager);
  }
  iHdrOff = pPager->journalOff;

  /* Write the super-journal data to the end of the journal file. If
  ** an error occurs, return the error code to the caller.
  */
  if( (0 != (rc = write32bits(pPager->jfd, iHdrOff, PAGER_SJ_PGNO(pPager))))
   || (0 != (rc = sqlite3OsWrite(pPager->jfd, zSuper, nSuper, iHdrOff+4)))
   || (0 != (rc = write32bits(pPager->jfd, iHdrOff+4+nSuper, nSuper)))
   || (0 != (rc = write32bits(pPager->jfd, iHdrOff+4+nSuper+4, cksum)))
   || (0 != (rc = sqlite3OsWrite(pPager->jfd, aJournalMagic, 8,
                                 iHdrOff+4+nSuper+8)))
  ){
    return rc;
  }
  pPager->journalOff += (nSuper+20);

  /* If the pager is in persistent-journal mode, then the physical
  ** journal-file may extend past the end of the super-journal name
  ** and 8 bytes of magic data just written to the file. This is
  ** dangerous because the code to rollback a hot-journal file
  ** will not be able to find the super-journal name to determine
  ** whether or not the journal is hot.
  **
  ** Easiest thing to do in this scenario is to truncate the journal
  ** file to the required size.
  */
  if( SQLITE_OK==(rc = sqlite3OsFileSize(pPager->jfd, &jrnlSize))
   && jrnlSize>pPager->journalOff
  ){
    rc = sqlite3OsTruncate(pPager->jfd, pPager->journalOff);
  }
  return rc;
}

/*
** Discard the entire contents of the in-memory page-cache.
*/
static void pager_reset(Pager *pPager){
  pPager->iDataVersion++;
  sqlite3BackupRestart(pPager->pBackup);
  sqlite3PcacheClear(pPager->pPCache);
}

/*
** Return the pPager->iDataVersion value
*/
SQLITE_PRIVATE u32 sqlite3PagerDataVersion(Pager *pPager){
  return pPager->iDataVersion;
}

/*
** Free all structures in the Pager.aSavepoint[] array and set both
** Pager.aSavepoint and Pager.nSavepoint to zero. Close the sub-journal
** if it is open and the pager is not in exclusive mode.
*/
static void releaseAllSavepoints(Pager *pPager){
  int ii;               /* Iterator for looping through Pager.aSavepoint */
  for(ii=0; ii<pPager->nSavepoint; ii++){
    sqlite3BitvecDestroy(pPager->aSavepoint[ii].pInSavepoint);
  }
  if( !pPager->exclusiveMode || sqlite3JournalIsInMemory(pPager->sjfd) ){
    sqlite3OsClose(pPager->sjfd);
  }
  sqlite3_free(pPager->aSavepoint);
  pPager->aSavepoint = 0;
  pPager->nSavepoint = 0;
  pPager->nSubRec = 0;
}

/*
** Set the bit number pgno in the PagerSavepoint.pInSavepoint
** bitvecs of all open savepoints. Return SQLITE_OK if successful
** or SQLITE_NOMEM if a malloc failure occurs.
*/
static int addToSavepointBitvecs(Pager *pPager, Pgno pgno){
  int ii;                   /* Loop counter */
  int rc = SQLITE_OK;       /* Result code */

  for(ii=0; ii<pPager->nSavepoint; ii++){
    PagerSavepoint *p = &pPager->aSavepoint[ii];
    if( pgno<=p->nOrig ){
      rc |= sqlite3BitvecSet(p->pInSavepoint, pgno);
      testcase( rc==SQLITE_NOMEM );
      assert( rc==SQLITE_OK || rc==SQLITE_NOMEM );
    }
  }
  return rc;
}

/*
** This function is a no-op if the pager is in exclusive mode and not
** in the ERROR state. Otherwise, it switches the pager to PAGER_OPEN
** state.
**
** If the pager is not in exclusive-access mode, the database file is
** completely unlocked. If the file is unlocked and the file-system does
** not exhibit the UNDELETABLE_WHEN_OPEN property, the journal file is
** closed (if it is open).
**
** If the pager is in ERROR state when this function is called, the
** contents of the pager cache are discarded before switching back to
** the OPEN state. Regardless of whether the pager is in exclusive-mode
** or not, any journal file left in the file-system will be treated
** as a hot-journal and rolled back the next time a read-transaction
** is opened (by this or by any other connection).
*/
static void pager_unlock(Pager *pPager){

  assert( pPager->eState==PAGER_READER
       || pPager->eState==PAGER_OPEN
       || pPager->eState==PAGER_ERROR
  );

  sqlite3BitvecDestroy(pPager->pInJournal);
  pPager->pInJournal = 0;
  releaseAllSavepoints(pPager);

  if( pagerUseWal(pPager) ){
    assert( !isOpen(pPager->jfd) );
    sqlite3WalEndReadTransaction(pPager->pWal);
    pPager->eState = PAGER_OPEN;
  }else if( !pPager->exclusiveMode ){
    int rc;                       /* Error code returned by pagerUnlockDb() */
    int iDc = isOpen(pPager->fd)?sqlite3OsDeviceCharacteristics(pPager->fd):0;

    /* If the operating system support deletion of open files, then
    ** close the journal file when dropping the database lock.  Otherwise
    ** another connection with journal_mode=delete might delete the file
    ** out from under us.
    */
    assert( (PAGER_JOURNALMODE_MEMORY   & 5)!=1 );
    assert( (PAGER_JOURNALMODE_OFF      & 5)!=1 );
    assert( (PAGER_JOURNALMODE_WAL      & 5)!=1 );
    assert( (PAGER_JOURNALMODE_DELETE   & 5)!=1 );
    assert( (PAGER_JOURNALMODE_TRUNCATE & 5)==1 );
    assert( (PAGER_JOURNALMODE_PERSIST  & 5)==1 );
    if( 0==(iDc & SQLITE_IOCAP_UNDELETABLE_WHEN_OPEN)
     || 1!=(pPager->journalMode & 5)
    ){
      sqlite3OsClose(pPager->jfd);
    }

    /* If the pager is in the ERROR state and the call to unlock the database
    ** file fails, set the current lock to UNKNOWN_LOCK. See the comment
    ** above the #define for UNKNOWN_LOCK for an explanation of why this
    ** is necessary.
    */
    rc = pagerUnlockDb(pPager, NO_LOCK);
    if( rc!=SQLITE_OK && pPager->eState==PAGER_ERROR ){
      pPager->eLock = UNKNOWN_LOCK;
    }

    /* The pager state may be changed from PAGER_ERROR to PAGER_OPEN here
    ** without clearing the error code. This is intentional - the error
    ** code is cleared and the cache reset in the block below.
    */
    assert( pPager->errCode || pPager->eState!=PAGER_ERROR );
    pPager->eState = PAGER_OPEN;
  }

  /* If Pager.errCode is set, the contents of the pager cache cannot be
  ** trusted. Now that there are no outstanding references to the pager,
  ** it can safely move back to PAGER_OPEN state. This happens in both
  ** normal and exclusive-locking mode.
  */
  assert( pPager->errCode==SQLITE_OK || !MEMDB );
  if( pPager->errCode ){
    if( pPager->tempFile==0 ){
      pager_reset(pPager);
      pPager->changeCountDone = 0;
      pPager->eState = PAGER_OPEN;
    }else{
      pPager->eState = (isOpen(pPager->jfd) ? PAGER_OPEN : PAGER_READER);
    }
    if( USEFETCH(pPager) ) sqlite3OsUnfetch(pPager->fd, 0, 0);
    pPager->errCode = SQLITE_OK;
    setGetterMethod(pPager);
  }

  pPager->journalOff = 0;
  pPager->journalHdr = 0;
  pPager->setSuper = 0;
}

/*
** This function is called whenever an IOERR or FULL error that requires
** the pager to transition into the ERROR state may have occurred.
** The first argument is a pointer to the pager structure, the second
** the error-code about to be returned by a pager API function. The
** value returned is a copy of the second argument to this function.
**
** If the second argument is SQLITE_FULL, SQLITE_IOERR or one of the
** IOERR sub-codes, the pager enters the ERROR state and the error code
** is stored in Pager.errCode. While the pager remains in the ERROR state,
** all major API calls on the Pager will immediately return Pager.errCode.
**
** The ERROR state indicates that the contents of the pager-cache
** cannot be trusted. This state can be cleared by completely discarding
** the contents of the pager-cache. If a transaction was active when
** the persistent error occurred, then the rollback journal may need
** to be replayed to restore the contents of the database file (as if
** it were a hot-journal).
*/
static int pager_error(Pager *pPager, int rc){
  int rc2 = rc & 0xff;
  assert( rc==SQLITE_OK || !MEMDB );
  assert(
       pPager->errCode==SQLITE_FULL ||
       pPager->errCode==SQLITE_OK ||
       (pPager->errCode & 0xff)==SQLITE_IOERR
  );
  if( rc2==SQLITE_FULL || rc2==SQLITE_IOERR ){
    pPager->errCode = rc;
    pPager->eState = PAGER_ERROR;
    setGetterMethod(pPager);
  }
  return rc;
}

static int pager_truncate(Pager *pPager, Pgno nPage);

/*
** The write transaction open on pPager is being committed (bCommit==1)
** or rolled back (bCommit==0).
**
** Return TRUE if and only if all dirty pages should be flushed to disk.
**
** Rules:
**
**   *  For non-TEMP databases, always sync to disk.  This is necessary
**      for transactions to be durable.
**
**   *  Sync TEMP database only on a COMMIT (not a ROLLBACK) when the backing
**      file has been created already (via a spill on pagerStress()) and
**      when the number of dirty pages in memory exceeds 25% of the total
**      cache size.
*/
static int pagerFlushOnCommit(Pager *pPager, int bCommit){
  if( pPager->tempFile==0 ) return 1;
  if( !bCommit ) return 0;
  if( !isOpen(pPager->fd) ) return 0;
  return (sqlite3PCachePercentDirty(pPager->pPCache)>=25);
}

/*
** This routine ends a transaction. A transaction is usually ended by
** either a COMMIT or a ROLLBACK operation. This routine may be called
** after rollback of a hot-journal, or if an error occurs while opening
** the journal file or writing the very first journal-header of a
** database transaction.
**
** This routine is never called in PAGER_ERROR state. If it is called
** in PAGER_NONE or PAGER_SHARED state and the lock held is less
** exclusive than a RESERVED lock, it is a no-op.
**
** Otherwise, any active savepoints are released.
**
** If the journal file is open, then it is "finalized". Once a journal
** file has been finalized it is not possible to use it to roll back a
** transaction. Nor will it be considered to be a hot-journal by this
** or any other database connection. Exactly how a journal is finalized
** depends on whether or not the pager is running in exclusive mode and
** the current journal-mode (Pager.journalMode value), as follows:
**
**   journalMode==MEMORY
**     Journal file descriptor is simply closed. This destroys an
**     in-memory journal.
**
**   journalMode==TRUNCATE
**     Journal file is truncated to zero bytes in size.
**
**   journalMode==PERSIST
**     The first 28 bytes of the journal file are zeroed. This invalidates
**     the first journal header in the file, and hence the entire journal
**     file. An invalid journal file cannot be rolled back.
**
**   journalMode==DELETE
**     The journal file is closed and deleted using sqlite3OsDelete().
**
**     If the pager is running in exclusive mode, this method of finalizing
**     the journal file is never used. Instead, if the journalMode is
**     DELETE and the pager is in exclusive mode, the method described under
**     journalMode==PERSIST is used instead.
**
** After the journal is finalized, the pager moves to PAGER_READER state.
** If running in non-exclusive rollback mode, the lock on the file is
** downgraded to a SHARED_LOCK.
**
** SQLITE_OK is returned if no error occurs. If an error occurs during
** any of the IO operations to finalize the journal file or unlock the
** database then the IO error code is returned to the user. If the
** operation to finalize the journal file fails, then the code still
** tries to unlock the database file if not in exclusive mode. If the
** unlock operation fails as well, then the first error code related
** to the first error encountered (the journal finalization one) is
** returned.
*/
static int pager_end_transaction(Pager *pPager, int hasSuper, int bCommit){
  int rc = SQLITE_OK;      /* Error code from journal finalization operation */
  int rc2 = SQLITE_OK;     /* Error code from db file unlock operation */

  /* Do nothing if the pager does not have an open write transaction
  ** or at least a RESERVED lock. This function may be called when there
  ** is no write-transaction active but a RESERVED or greater lock is
  ** held under two circumstances:
  **
  **   1. After a successful hot-journal rollback, it is called with
  **      eState==PAGER_NONE and eLock==EXCLUSIVE_LOCK.
  **
  **   2. If a connection with locking_mode=exclusive holding an EXCLUSIVE
  **      lock switches back to locking_mode=normal and then executes a
  **      read-transaction, this function is called with eState==PAGER_READER
  **      and eLock==EXCLUSIVE_LOCK when the read-transaction is closed.
  */
  assert( assert_pager_state(pPager) );
  assert( pPager->eState!=PAGER_ERROR );
  if( pPager->eState<PAGER_WRITER_LOCKED && pPager->eLock<RESERVED_LOCK ){
    return SQLITE_OK;
  }

  releaseAllSavepoints(pPager);
  assert( isOpen(pPager->jfd) || pPager->pInJournal==0
      || (sqlite3OsDeviceCharacteristics(pPager->fd)&SQLITE_IOCAP_BATCH_ATOMIC)
  );
  if( isOpen(pPager->jfd) ){
    assert( !pagerUseWal(pPager) );

    /* Finalize the journal file. */
    if( sqlite3JournalIsInMemory(pPager->jfd) ){
      /* assert( pPager->journalMode==PAGER_JOURNALMODE_MEMORY ); */
      sqlite3OsClose(pPager->jfd);
    }else if( pPager->journalMode==PAGER_JOURNALMODE_TRUNCATE ){
      if( pPager->journalOff==0 ){
        rc = SQLITE_OK;
      }else{
        rc = sqlite3OsTruncate(pPager->jfd, 0);
        if( rc==SQLITE_OK && pPager->fullSync ){
          /* Make sure the new file size is written into the inode right away.
          ** Otherwise the journal might resurrect following a power loss and
          ** cause the last transaction to roll back.  See
          ** https://bugzilla.mozilla.org/show_bug.cgi?id=1072773
          */
          rc = sqlite3OsSync(pPager->jfd, pPager->syncFlags);
        }
      }
      pPager->journalOff = 0;
    }else if( pPager->journalMode==PAGER_JOURNALMODE_PERSIST
      || (pPager->exclusiveMode && pPager->journalMode!=PAGER_JOURNALMODE_WAL)
    ){
      rc = zeroJournalHdr(pPager, hasSuper||pPager->tempFile);
      pPager->journalOff = 0;
    }else{
      /* This branch may be executed with Pager.journalMode==MEMORY if
      ** a hot-journal was just rolled back. In this case the journal
      ** file should be closed and deleted. If this connection writes to
      ** the database file, it will do so using an in-memory journal.
      */
      int bDelete = !pPager->tempFile;
      assert( sqlite3JournalIsInMemory(pPager->jfd)==0 );
      assert( pPager->journalMode==PAGER_JOURNALMODE_DELETE
           || pPager->journalMode==PAGER_JOURNALMODE_MEMORY
           || pPager->journalMode==PAGER_JOURNALMODE_WAL
      );
      sqlite3OsClose(pPager->jfd);
      if( bDelete ){
        rc = sqlite3OsDelete(pPager->pVfs, pPager->zJournal, pPager->extraSync);
      }
    }
  }

#ifdef SQLITE_CHECK_PAGES
  sqlite3PcacheIterateDirty(pPager->pPCache, pager_set_pagehash);
  if( pPager->dbSize==0 && sqlite3PcacheRefCount(pPager->pPCache)>0 ){
    PgHdr *p = sqlite3PagerLookup(pPager, 1);
    if( p ){
      p->pageHash = 0;
      sqlite3PagerUnrefNotNull(p);
    }
  }
#endif

  sqlite3BitvecDestroy(pPager->pInJournal);
  pPager->pInJournal = 0;
  pPager->nRec = 0;
  if( rc==SQLITE_OK ){
    if( MEMDB || pagerFlushOnCommit(pPager, bCommit) ){
      sqlite3PcacheCleanAll(pPager->pPCache);
    }else{
      sqlite3PcacheClearWritable(pPager->pPCache);
    }
    sqlite3PcacheTruncate(pPager->pPCache, pPager->dbSize);
  }

  if( pagerUseWal(pPager) ){
    /* Drop the WAL write-lock, if any. Also, if the connection was in
    ** locking_mode=exclusive mode but is no longer, drop the EXCLUSIVE
    ** lock held on the database file.
    */
    rc2 = sqlite3WalEndWriteTransaction(pPager->pWal);
    assert( rc2==SQLITE_OK );
  }else if( rc==SQLITE_OK && bCommit && pPager->dbFileSize>pPager->dbSize ){
    /* This branch is taken when committing a transaction in rollback-journal
    ** mode if the database file on disk is larger than the database image.
    ** At this point the journal has been finalized and the transaction
    ** successfully committed, but the EXCLUSIVE lock is still held on the
    ** file. So it is safe to truncate the database file to its minimum
    ** required size.  */
    assert( pPager->eLock==EXCLUSIVE_LOCK );
    rc = pager_truncate(pPager, pPager->dbSize);
  }

  if( rc==SQLITE_OK && bCommit ){
    rc = sqlite3OsFileControl(pPager->fd, SQLITE_FCNTL_COMMIT_PHASETWO, 0);
    if( rc==SQLITE_NOTFOUND ) rc = SQLITE_OK;
  }

  if( !pPager->exclusiveMode
   && (!pagerUseWal(pPager) || sqlite3WalExclusiveMode(pPager->pWal, 0))
  ){
    rc2 = pagerUnlockDb(pPager, SHARED_LOCK);
  }
  pPager->eState = PAGER_READER;
  pPager->setSuper = 0;

  return (rc==SQLITE_OK?rc2:rc);
}
