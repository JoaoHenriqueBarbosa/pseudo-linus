#endif

/*
** A call to this routine tells the pager that it is not necessary to
** write the information on page pPg back to the disk, even though
** that page might be marked as dirty.  This happens, for example, when
** the page has been added as a leaf of the freelist and so its
** content no longer matters.
**
** The overlying software layer calls this routine when all of the data
** on the given page is unused. The pager marks the page as clean so
** that it does not get written to disk.
**
** Tests show that this optimization can quadruple the speed of large
** DELETE operations.
**
** This optimization cannot be used with a temp-file, as the page may
** have been dirty at the start of the transaction. In that case, if
** memory pressure forces page pPg out of the cache, the data does need
** to be written out to disk so that it may be read back in if the
** current transaction is rolled back.
*/
SQLITE_PRIVATE void sqlite3PagerDontWrite(PgHdr *pPg){
  Pager *pPager = pPg->pPager;
  if( !pPager->tempFile && (pPg->flags&PGHDR_DIRTY) && pPager->nSavepoint==0 ){
    PAGERTRACE(("DONT_WRITE page %d of %d\n", pPg->pgno, PAGERID(pPager)));
    IOTRACE(("CLEAN %p %d\n", pPager, pPg->pgno))
    pPg->flags |= PGHDR_DONT_WRITE;
    pPg->flags &= ~PGHDR_WRITEABLE;
    testcase( pPg->flags & PGHDR_NEED_SYNC );
    pager_set_pagehash(pPg);
  }
}

/*
** This routine is called to increment the value of the database file
** change-counter, stored as a 4-byte big-endian integer starting at
** byte offset 24 of the pager file.  The secondary change counter at
** 92 is also updated, as is the SQLite version number at offset 96.
**
** But this only happens if the pPager->changeCountDone flag is false.
** To avoid excess churning of page 1, the update only happens once.
** See also the pager_write_changecounter() routine that does an
** unconditional update of the change counters.
**
** If the isDirectMode flag is zero, then this is done by calling
** sqlite3PagerWrite() on page 1, then modifying the contents of the
** page data. In this case the file will be updated when the current
** transaction is committed.
**
** The isDirectMode flag may only be non-zero if the library was compiled
** with the SQLITE_ENABLE_ATOMIC_WRITE macro defined. In this case,
** if isDirect is non-zero, then the database file is updated directly
** by writing an updated version of page 1 using a call to the
** sqlite3OsWrite() function.
*/
static int pager_incr_changecounter(Pager *pPager, int isDirectMode){
  int rc = SQLITE_OK;

  assert( pPager->eState==PAGER_WRITER_CACHEMOD
       || pPager->eState==PAGER_WRITER_DBMOD
  );
  assert( assert_pager_state(pPager) );

  /* Declare and initialize constant integer 'isDirect'. If the
  ** atomic-write optimization is enabled in this build, then isDirect
  ** is initialized to the value passed as the isDirectMode parameter
  ** to this function. Otherwise, it is always set to zero.
  **
  ** The idea is that if the atomic-write optimization is not
  ** enabled at compile time, the compiler can omit the tests of
  ** 'isDirect' below, as well as the block enclosed in the
  ** "if( isDirect )" condition.
  */
#ifndef SQLITE_ENABLE_ATOMIC_WRITE
# define DIRECT_MODE 0
  assert( isDirectMode==0 );
  UNUSED_PARAMETER(isDirectMode);
#else
# define DIRECT_MODE isDirectMode
#endif

  if( !pPager->changeCountDone && pPager->dbSize>0 ){
    PgHdr *pPgHdr;                /* Reference to page 1 */

    assert( !pPager->tempFile && isOpen(pPager->fd) );

    /* Open page 1 of the file for writing. */
    rc = sqlite3PagerGet(pPager, 1, &pPgHdr, 0);
    assert( pPgHdr==0 || rc==SQLITE_OK );

    /* If page one was fetched successfully, and this function is not
    ** operating in direct-mode, make page 1 writable.  When not in
    ** direct mode, page 1 is always held in cache and hence the PagerGet()
    ** above is always successful - hence the ALWAYS on rc==SQLITE_OK.
    */
    if( !DIRECT_MODE && ALWAYS(rc==SQLITE_OK) ){
      rc = sqlite3PagerWrite(pPgHdr);
    }

    if( rc==SQLITE_OK ){
      /* Actually do the update of the change counter */
      pager_write_changecounter(pPgHdr);

      /* If running in direct mode, write the contents of page 1 to the file. */
      if( DIRECT_MODE ){
        const void *zBuf;
        assert( pPager->dbFileSize>0 );
        zBuf = pPgHdr->pData;
        if( rc==SQLITE_OK ){
          rc = sqlite3OsWrite(pPager->fd, zBuf, pPager->pageSize, 0);
          pPager->aStat[PAGER_STAT_WRITE]++;
        }
        if( rc==SQLITE_OK ){
          /* Update the pager's copy of the change-counter. Otherwise, the
          ** next time a read transaction is opened the cache will be
          ** flushed (as the change-counter values will not match).  */
          const void *pCopy = (const void *)&((const char *)zBuf)[24];
          memcpy(&pPager->dbFileVers, pCopy, sizeof(pPager->dbFileVers));
          pPager->changeCountDone = 1;
        }
      }else{
        pPager->changeCountDone = 1;
      }
    }

    /* Release the page reference. */
    sqlite3PagerUnref(pPgHdr);
  }
  return rc;
}

/*
** Sync the database file to disk. This is a no-op for in-memory databases
** or pages with the Pager.noSync flag set.
**
** If successful, or if called on a pager for which it is a no-op, this
** function returns SQLITE_OK. Otherwise, an IO error code is returned.
*/
SQLITE_PRIVATE int sqlite3PagerSync(Pager *pPager, const char *zSuper){
  int rc = SQLITE_OK;
  void *pArg = (void*)zSuper;
  rc = sqlite3OsFileControl(pPager->fd, SQLITE_FCNTL_SYNC, pArg);
  if( rc==SQLITE_NOTFOUND ) rc = SQLITE_OK;
  if( rc==SQLITE_OK && !pPager->noSync ){
    assert( !MEMDB );
    rc = sqlite3OsSync(pPager->fd, pPager->syncFlags);
  }
  return rc;
}

/*
** This function may only be called while a write-transaction is active in
** rollback. If the connection is in WAL mode, this call is a no-op.
** Otherwise, if the connection does not already have an EXCLUSIVE lock on
** the database file, an attempt is made to obtain one.
**
** If the EXCLUSIVE lock is already held or the attempt to obtain it is
** successful, or the connection is in WAL mode, SQLITE_OK is returned.
** Otherwise, either SQLITE_BUSY or an SQLITE_IOERR_XXX error code is
** returned.
*/
SQLITE_PRIVATE int sqlite3PagerExclusiveLock(Pager *pPager){
  int rc = pPager->errCode;
  assert( assert_pager_state(pPager) );
  if( rc==SQLITE_OK ){
    assert( pPager->eState==PAGER_WRITER_CACHEMOD
         || pPager->eState==PAGER_WRITER_DBMOD
         || pPager->eState==PAGER_WRITER_LOCKED
    );
    assert( assert_pager_state(pPager) );
    if( 0==pagerUseWal(pPager) ){
      rc = pager_wait_on_lock(pPager, EXCLUSIVE_LOCK);
    }
  }
  return rc;
}

/*
** Sync the database file for the pager pPager. zSuper points to the name
** of a super-journal file that should be written into the individual
** journal file. zSuper may be NULL, which is interpreted as no
** super-journal (a single database transaction).
**
** This routine ensures that:
**
**   * The database file change-counter is updated,
**   * the journal is synced (unless the atomic-write optimization is used),
**   * all dirty pages are written to the database file,
**   * the database file is truncated (if required), and
**   * the database file synced.
**
** The only thing that remains to commit the transaction is to finalize
** (delete, truncate or zero the first part of) the journal file (or
** delete the super-journal file if specified).
**
** Note that if zSuper==NULL, this does not overwrite a previous value
** passed to an sqlite3PagerCommitPhaseOne() call.
**
** If the final parameter - noSync - is true, then the database file itself
** is not synced. The caller must call sqlite3PagerSync() directly to
** sync the database file before calling CommitPhaseTwo() to delete the
** journal file in this case.
*/
SQLITE_PRIVATE int sqlite3PagerCommitPhaseOne(
  Pager *pPager,                  /* Pager object */
  const char *zSuper,            /* If not NULL, the super-journal name */
  int noSync                      /* True to omit the xSync on the db file */
){
  int rc = SQLITE_OK;             /* Return code */

  assert( pPager->eState==PAGER_WRITER_LOCKED
       || pPager->eState==PAGER_WRITER_CACHEMOD
       || pPager->eState==PAGER_WRITER_DBMOD
       || pPager->eState==PAGER_ERROR
  );
  assert( assert_pager_state(pPager) );

  /* If a prior error occurred, report that error again. */
  if( NEVER(pPager->errCode) ) return pPager->errCode;

  /* Provide the ability to easily simulate an I/O error during testing */
  if( sqlite3FaultSim(400) ) return SQLITE_IOERR;

  PAGERTRACE(("DATABASE SYNC: File=%s zSuper=%s nSize=%d\n",
      pPager->zFilename, zSuper, pPager->dbSize));

  /* If no database changes have been made, return early. */
  if( pPager->eState<PAGER_WRITER_CACHEMOD ) return SQLITE_OK;

  assert( MEMDB==0 || pPager->tempFile );
  assert( isOpen(pPager->fd) || pPager->tempFile );
  if( 0==pagerFlushOnCommit(pPager, 1) ){
    /* If this is an in-memory db, or no pages have been written to, or this
    ** function has already been called, it is mostly a no-op.  However, any
    ** backup in progress needs to be restarted.  */
    sqlite3BackupRestart(pPager->pBackup);
  }else{
    PgHdr *pList;
    if( pagerUseWal(pPager) ){
      PgHdr *pPageOne = 0;
      pList = sqlite3PcacheDirtyList(pPager->pPCache);
      if( pList==0 ){
        /* Must have at least one page for the WAL commit flag.
        ** Ticket [2d1a5c67dfc2363e44f29d9bbd57f] 2011-05-18 */
        rc = sqlite3PagerGet(pPager, 1, &pPageOne, 0);
        pList = pPageOne;
        pList->pDirty = 0;
      }
      assert( rc==SQLITE_OK );
      if( ALWAYS(pList) ){
        rc = pagerWalFrames(pPager, pList, pPager->dbSize, 1);
      }
      sqlite3PagerUnref(pPageOne);
      if( rc==SQLITE_OK ){
        sqlite3PcacheCleanAll(pPager->pPCache);
      }
    }else{
      /* The bBatch boolean is true if the batch-atomic-write commit method
      ** should be used.  No rollback journal is created if batch-atomic-write
      ** is enabled.
      */
#ifdef SQLITE_ENABLE_BATCH_ATOMIC_WRITE
      sqlite3_file *fd = pPager->fd;
      int bBatch = zSuper==0    /* An SQLITE_IOCAP_BATCH_ATOMIC commit */
        && (sqlite3OsDeviceCharacteristics(fd) & SQLITE_IOCAP_BATCH_ATOMIC)
        && !pPager->noSync
        && sqlite3JournalIsInMemory(pPager->jfd);
#else
#     define bBatch 0
#endif

#ifdef SQLITE_ENABLE_ATOMIC_WRITE
      /* The following block updates the change-counter. Exactly how it
      ** does this depends on whether or not the atomic-update optimization
      ** was enabled at compile time, and if this transaction meets the
      ** runtime criteria to use the operation:
      **
      **    * The file-system supports the atomic-write property for
      **      blocks of size page-size, and
      **    * This commit is not part of a multi-file transaction, and
      **    * Exactly one page has been modified and store in the journal file.
      **
      ** If the optimization was not enabled at compile time, then the
      ** pager_incr_changecounter() function is called to update the change
      ** counter in 'indirect-mode'. If the optimization is compiled in but
      ** is not applicable to this transaction, call sqlite3JournalCreate()
      ** to make sure the journal file has actually been created, then call
      ** pager_incr_changecounter() to update the change-counter in indirect
      ** mode.
      **
      ** Otherwise, if the optimization is both enabled and applicable,
      ** then call pager_incr_changecounter() to update the change-counter
      ** in 'direct' mode. In this case the journal file will never be
      ** created for this transaction.
      */
      if( bBatch==0 ){
        PgHdr *pPg;
        assert( isOpen(pPager->jfd)
            || pPager->journalMode==PAGER_JOURNALMODE_OFF
            || pPager->journalMode==PAGER_JOURNALMODE_WAL
            );
        if( !zSuper && isOpen(pPager->jfd)
         && pPager->journalOff==jrnlBufferSize(pPager)
         && pPager->dbSize>=pPager->dbOrigSize
         && (!(pPg = sqlite3PcacheDirtyList(pPager->pPCache)) || 0==pPg->pDirty)
        ){
          /* Update the db file change counter via the direct-write method. The
          ** following call will modify the in-memory representation of page 1
          ** to include the updated change counter and then write page 1
          ** directly to the database file. Because of the atomic-write
          ** property of the host file-system, this is safe.
          */
          rc = pager_incr_changecounter(pPager, 1);
        }else{
          rc = sqlite3JournalCreate(pPager->jfd);
          if( rc==SQLITE_OK ){
            rc = pager_incr_changecounter(pPager, 0);
          }
        }
      }
#else  /* SQLITE_ENABLE_ATOMIC_WRITE */
#ifdef SQLITE_ENABLE_BATCH_ATOMIC_WRITE
      if( zSuper ){
        rc = sqlite3JournalCreate(pPager->jfd);
        if( rc!=SQLITE_OK ) goto commit_phase_one_exit;
        assert( bBatch==0 );
      }
#endif
      rc = pager_incr_changecounter(pPager, 0);
#endif /* !SQLITE_ENABLE_ATOMIC_WRITE */
      if( rc!=SQLITE_OK ) goto commit_phase_one_exit;

      /* Write the super-journal name into the journal file. If a
      ** super-journal file name has already been written to the journal file,
      ** or if zSuper is NULL (no super-journal), then this call is a no-op.
      */
      rc = writeSuperJournal(pPager, zSuper);
      if( rc!=SQLITE_OK ) goto commit_phase_one_exit;

      /* Sync the journal file and write all dirty pages to the database.
      ** If the atomic-update optimization is being used, this sync will not
      ** create the journal file or perform any real IO.
      **
      ** Because the change-counter page was just modified, unless the
      ** atomic-update optimization is used it is almost certain that the
      ** journal requires a sync here. However, in locking_mode=exclusive
      ** on a system under memory pressure it is just possible that this is
      ** not the case. In this case it is likely enough that the redundant
      ** xSync() call will be changed to a no-op by the OS anyhow.
      */
      rc = syncJournal(pPager, 0);
      if( rc!=SQLITE_OK ) goto commit_phase_one_exit;

      pList = sqlite3PcacheDirtyList(pPager->pPCache);
#ifdef SQLITE_ENABLE_BATCH_ATOMIC_WRITE
      if( bBatch ){
        rc = sqlite3OsFileControl(fd, SQLITE_FCNTL_BEGIN_ATOMIC_WRITE, 0);
        if( rc==SQLITE_OK ){
          rc = pager_write_pagelist(pPager, pList);
          if( rc==SQLITE_OK && pPager->dbSize>pPager->dbFileSize ){
            char *pTmp = pPager->pTmpSpace;
            int szPage = (int)pPager->pageSize;
            memset(pTmp, 0, szPage);
            rc = sqlite3OsWrite(pPager->fd, pTmp, szPage,
                      ((i64)pPager->dbSize*pPager->pageSize)-szPage);
          }
          if( rc==SQLITE_OK ){
            rc = sqlite3OsFileControl(fd, SQLITE_FCNTL_COMMIT_ATOMIC_WRITE, 0);
          }
          if( rc!=SQLITE_OK ){
            sqlite3OsFileControlHint(fd, SQLITE_FCNTL_ROLLBACK_ATOMIC_WRITE, 0);
          }
        }

        if( (rc&0xFF)==SQLITE_IOERR && rc!=SQLITE_IOERR_NOMEM ){
          rc = sqlite3JournalCreate(pPager->jfd);
          if( rc!=SQLITE_OK ){
            sqlite3OsClose(pPager->jfd);
            goto commit_phase_one_exit;
          }
          bBatch = 0;
        }else{
          sqlite3OsClose(pPager->jfd);
        }
      }
#endif /* SQLITE_ENABLE_BATCH_ATOMIC_WRITE */

      if( bBatch==0 ){
        rc = pager_write_pagelist(pPager, pList);
      }
      if( rc!=SQLITE_OK ){
        assert( rc!=SQLITE_IOERR_BLOCKED );
        goto commit_phase_one_exit;
      }
      sqlite3PcacheCleanAll(pPager->pPCache);

      /* If the file on disk is smaller than the database image, use
      ** pager_truncate to grow the file here. This can happen if the database
      ** image was extended as part of the current transaction and then the
      ** last page in the db image moved to the free-list. In this case the
      ** last page is never written out to disk, leaving the database file
      ** undersized. Fix this now if it is the case.  */
      if( pPager->dbSize>pPager->dbFileSize ){
        Pgno nNew = pPager->dbSize - (pPager->dbSize==PAGER_SJ_PGNO(pPager));
        assert( pPager->eState==PAGER_WRITER_DBMOD );
        rc = pager_truncate(pPager, nNew);
        if( rc!=SQLITE_OK ) goto commit_phase_one_exit;
      }

      /* Finally, sync the database file. */
      if( !noSync ){
        rc = sqlite3PagerSync(pPager, zSuper);
      }
      IOTRACE(("DBSYNC %p\n", pPager))
    }
  }

commit_phase_one_exit:
  if( rc==SQLITE_OK && !pagerUseWal(pPager) ){
    pPager->eState = PAGER_WRITER_FINISHED;
  }
  return rc;
}
