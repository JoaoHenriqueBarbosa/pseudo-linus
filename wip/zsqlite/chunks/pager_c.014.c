
/*
** Begin a write-transaction on the specified pager object. If a
** write-transaction has already been opened, this function is a no-op.
**
** If the exFlag argument is false, then acquire at least a RESERVED
** lock on the database file. If exFlag is true, then acquire at least
** an EXCLUSIVE lock. If such a lock is already held, no locking
** functions need be called.
**
** If the subjInMemory argument is non-zero, then any sub-journal opened
** within this transaction will be opened as an in-memory file. This
** has no effect if the sub-journal is already opened (as it may be when
** running in exclusive mode) or if the transaction does not require a
** sub-journal. If the subjInMemory argument is zero, then any required
** sub-journal is implemented in-memory if pPager is an in-memory database,
** or using a temporary file otherwise.
*/
SQLITE_PRIVATE int sqlite3PagerBegin(Pager *pPager, int exFlag, int subjInMemory){
  int rc = SQLITE_OK;

  if( pPager->errCode ) return pPager->errCode;
  assert( pPager->eState>=PAGER_READER && pPager->eState<PAGER_ERROR );
  pPager->subjInMemory = (u8)subjInMemory;

  if( pPager->eState==PAGER_READER ){
    assert( pPager->pInJournal==0 );

    if( pagerUseWal(pPager) ){
      /* If the pager is configured to use locking_mode=exclusive, and an
      ** exclusive lock on the database is not already held, obtain it now.
      */
      if( pPager->exclusiveMode && sqlite3WalExclusiveMode(pPager->pWal, -1) ){
        rc = pagerLockDb(pPager, EXCLUSIVE_LOCK);
        if( rc!=SQLITE_OK ){
          return rc;
        }
        (void)sqlite3WalExclusiveMode(pPager->pWal, 1);
      }

      /* Grab the write lock on the log file. If successful, upgrade to
      ** PAGER_RESERVED state. Otherwise, return an error code to the caller.
      ** The busy-handler is not invoked if another connection already
      ** holds the write-lock. If possible, the upper layer will call it.
      */
      rc = sqlite3WalBeginWriteTransaction(pPager->pWal);
    }else{
      /* Obtain a RESERVED lock on the database file. If the exFlag parameter
      ** is true, then immediately upgrade this to an EXCLUSIVE lock. The
      ** busy-handler callback can be used when upgrading to the EXCLUSIVE
      ** lock, but not when obtaining the RESERVED lock.
      */
      rc = pagerLockDb(pPager, RESERVED_LOCK);
      if( rc==SQLITE_OK && exFlag ){
        rc = pager_wait_on_lock(pPager, EXCLUSIVE_LOCK);
      }
    }

    if( rc==SQLITE_OK ){
      /* Change to WRITER_LOCKED state.
      **
      ** WAL mode sets Pager.eState to PAGER_WRITER_LOCKED or CACHEMOD
      ** when it has an open transaction, but never to DBMOD or FINISHED.
      ** This is because in those states the code to roll back savepoint
      ** transactions may copy data from the sub-journal into the database
      ** file as well as into the page cache. Which would be incorrect in
      ** WAL mode.
      */
      pPager->eState = PAGER_WRITER_LOCKED;
      pPager->dbHintSize = pPager->dbSize;
      pPager->dbFileSize = pPager->dbSize;
      pPager->dbOrigSize = pPager->dbSize;
      pPager->journalOff = 0;
    }

    assert( rc==SQLITE_OK || pPager->eState==PAGER_READER );
    assert( rc!=SQLITE_OK || pPager->eState==PAGER_WRITER_LOCKED );
    assert( assert_pager_state(pPager) );
  }

  PAGERTRACE(("TRANSACTION %d\n", PAGERID(pPager)));
  return rc;
}

/*
** Write page pPg onto the end of the rollback journal.
*/
static SQLITE_NOINLINE int pagerAddPageToRollbackJournal(PgHdr *pPg){
  Pager *pPager = pPg->pPager;
  int rc;
  u32 cksum;
  char *pData2;
  i64 iOff = pPager->journalOff;

  /* We should never write to the journal file the page that
  ** contains the database locks.  The following assert verifies
  ** that we do not. */
  assert( pPg->pgno!=PAGER_SJ_PGNO(pPager) );

  assert( pPager->journalHdr<=pPager->journalOff );
  pData2 = pPg->pData;
  cksum = pager_cksum(pPager, (u8*)pData2);

  /* Even if an IO or diskfull error occurs while journalling the
  ** page in the block above, set the need-sync flag for the page.
  ** Otherwise, when the transaction is rolled back, the logic in
  ** playback_one_page() will think that the page needs to be restored
  ** in the database file. And if an IO error occurs while doing so,
  ** then corruption may follow.
  */
  pPg->flags |= PGHDR_NEED_SYNC;

  rc = write32bits(pPager->jfd, iOff, pPg->pgno);
  if( rc!=SQLITE_OK ) return rc;
  rc = sqlite3OsWrite(pPager->jfd, pData2, pPager->pageSize, iOff+4);
  if( rc!=SQLITE_OK ) return rc;
  rc = write32bits(pPager->jfd, iOff+pPager->pageSize+4, cksum);
  if( rc!=SQLITE_OK ) return rc;

  IOTRACE(("JOUT %p %d %lld %d\n", pPager, pPg->pgno,
           pPager->journalOff, pPager->pageSize));
  PAGER_INCR(sqlite3_pager_writej_count);
  PAGERTRACE(("JOURNAL %d page %d needSync=%d hash(%08x)\n",
       PAGERID(pPager), pPg->pgno,
       ((pPg->flags&PGHDR_NEED_SYNC)?1:0), pager_pagehash(pPg)));

  pPager->journalOff += 8 + pPager->pageSize;
  pPager->nRec++;
  assert( pPager->pInJournal!=0 );
  rc = sqlite3BitvecSet(pPager->pInJournal, pPg->pgno);
  testcase( rc==SQLITE_NOMEM );
  assert( rc==SQLITE_OK || rc==SQLITE_NOMEM );
  rc |= addToSavepointBitvecs(pPager, pPg->pgno);
  assert( rc==SQLITE_OK || rc==SQLITE_NOMEM );
  return rc;
}

/*
** Mark a single data page as writeable. The page is written into the
** main journal or sub-journal as required. If the page is written into
** one of the journals, the corresponding bit is set in the
** Pager.pInJournal bitvec and the PagerSavepoint.pInSavepoint bitvecs
** of any open savepoints as appropriate.
*/
static int pager_write(PgHdr *pPg){
  Pager *pPager = pPg->pPager;
  int rc = SQLITE_OK;

  /* This routine is not called unless a write-transaction has already
  ** been started. The journal file may or may not be open at this point.
  ** It is never called in the ERROR state.
  */
  assert( pPager->eState==PAGER_WRITER_LOCKED
       || pPager->eState==PAGER_WRITER_CACHEMOD
       || pPager->eState==PAGER_WRITER_DBMOD
  );
  assert( assert_pager_state(pPager) );
  assert( pPager->errCode==0 );
  assert( pPager->readOnly==0 );
  CHECK_PAGE(pPg);

  /* The journal file needs to be opened. Higher level routines have already
  ** obtained the necessary locks to begin the write-transaction, but the
  ** rollback journal might not yet be open. Open it now if this is the case.
  **
  ** This is done before calling sqlite3PcacheMakeDirty() on the page.
  ** Otherwise, if it were done after calling sqlite3PcacheMakeDirty(), then
  ** an error might occur and the pager would end up in WRITER_LOCKED state
  ** with pages marked as dirty in the cache.
  */
  if( pPager->eState==PAGER_WRITER_LOCKED ){
    rc = pager_open_journal(pPager);
    if( rc!=SQLITE_OK ) return rc;
  }
  assert( pPager->eState>=PAGER_WRITER_CACHEMOD );
  assert( assert_pager_state(pPager) );

  /* Mark the page that is about to be modified as dirty. */
  sqlite3PcacheMakeDirty(pPg);

  /* If a rollback journal is in use, them make sure the page that is about
  ** to change is in the rollback journal, or if the page is a new page off
  ** then end of the file, make sure it is marked as PGHDR_NEED_SYNC.
  */
  assert( (pPager->pInJournal!=0) == isOpen(pPager->jfd) );
  if( pPager->pInJournal!=0
   && sqlite3BitvecTestNotNull(pPager->pInJournal, pPg->pgno)==0
  ){
    assert( pagerUseWal(pPager)==0 );
    if( pPg->pgno<=pPager->dbOrigSize ){
      rc = pagerAddPageToRollbackJournal(pPg);
      if( rc!=SQLITE_OK ){
        return rc;
      }
    }else{
      if( pPager->eState!=PAGER_WRITER_DBMOD ){
        pPg->flags |= PGHDR_NEED_SYNC;
      }
      PAGERTRACE(("APPEND %d page %d needSync=%d\n",
              PAGERID(pPager), pPg->pgno,
             ((pPg->flags&PGHDR_NEED_SYNC)?1:0)));
    }
  }

  /* The PGHDR_DIRTY bit is set above when the page was added to the dirty-list
  ** and before writing the page into the rollback journal.  Wait until now,
  ** after the page has been successfully journalled, before setting the
  ** PGHDR_WRITEABLE bit that indicates that the page can be safely modified.
  */
  pPg->flags |= PGHDR_WRITEABLE;

  /* If the statement journal is open and the page is not in it,
  ** then write the page into the statement journal.
  */
  if( pPager->nSavepoint>0 ){
    rc = subjournalPageIfRequired(pPg);
  }

  /* Update the database size and return. */
  if( pPager->dbSize<pPg->pgno ){
    pPager->dbSize = pPg->pgno;
  }
  return rc;
}

/*
** This is a variant of sqlite3PagerWrite() that runs when the sector size
** is larger than the page size.  SQLite makes the (reasonable) assumption that
** all bytes of a sector are written together by hardware.  Hence, all bytes of
** a sector need to be journalled in case of a power loss in the middle of
** a write.
**
** Usually, the sector size is less than or equal to the page size, in which
** case pages can be individually written.  This routine only runs in the
** exceptional case where the page size is smaller than the sector size.
*/
static SQLITE_NOINLINE int pagerWriteLargeSector(PgHdr *pPg){
  int rc = SQLITE_OK;          /* Return code */
  Pgno nPageCount;             /* Total number of pages in database file */
  Pgno pg1;                    /* First page of the sector pPg is located on. */
  int nPage = 0;               /* Number of pages starting at pg1 to journal */
  int ii;                      /* Loop counter */
  int needSync = 0;            /* True if any page has PGHDR_NEED_SYNC */
  Pager *pPager = pPg->pPager; /* The pager that owns pPg */
  Pgno nPagePerSector = (pPager->sectorSize/pPager->pageSize);

  /* Set the doNotSpill NOSYNC bit to 1. This is because we cannot allow
  ** a journal header to be written between the pages journaled by
  ** this function.
  */
  assert( !MEMDB );
  assert( (pPager->doNotSpill & SPILLFLAG_NOSYNC)==0 );
  pPager->doNotSpill |= SPILLFLAG_NOSYNC;

  /* This trick assumes that both the page-size and sector-size are
  ** an integer power of 2. It sets variable pg1 to the identifier
  ** of the first page of the sector pPg is located on.
  */
  pg1 = ((pPg->pgno-1) & ~(nPagePerSector-1)) + 1;

  nPageCount = pPager->dbSize;
  if( pPg->pgno>nPageCount ){
    nPage = (pPg->pgno - pg1)+1;
  }else if( (pg1+nPagePerSector-1)>nPageCount ){
    nPage = nPageCount+1-pg1;
  }else{
    nPage = nPagePerSector;
  }
  assert(nPage>0);
  assert(pg1<=pPg->pgno);
  assert((pg1+nPage)>pPg->pgno);

  for(ii=0; ii<nPage && rc==SQLITE_OK; ii++){
    Pgno pg = pg1+ii;
    PgHdr *pPage;
    if( pg==pPg->pgno || !sqlite3BitvecTest(pPager->pInJournal, pg) ){
      if( pg!=PAGER_SJ_PGNO(pPager) ){
        rc = sqlite3PagerGet(pPager, pg, &pPage, 0);
        if( rc==SQLITE_OK ){
          rc = pager_write(pPage);
          if( pPage->flags&PGHDR_NEED_SYNC ){
            needSync = 1;
          }
          sqlite3PagerUnrefNotNull(pPage);
        }
      }
    }else if( (pPage = sqlite3PagerLookup(pPager, pg))!=0 ){
      if( pPage->flags&PGHDR_NEED_SYNC ){
        needSync = 1;
      }
      sqlite3PagerUnrefNotNull(pPage);
    }
  }

  /* If the PGHDR_NEED_SYNC flag is set for any of the nPage pages
  ** starting at pg1, then it needs to be set for all of them. Because
  ** writing to any of these nPage pages may damage the others, the
  ** journal file must contain sync()ed copies of all of them
  ** before any of them can be written out to the database file.
  */
  if( rc==SQLITE_OK && needSync ){
    assert( !MEMDB );
    for(ii=0; ii<nPage; ii++){
      PgHdr *pPage = sqlite3PagerLookup(pPager, pg1+ii);
      if( pPage ){
        pPage->flags |= PGHDR_NEED_SYNC;
        sqlite3PagerUnrefNotNull(pPage);
      }
    }
  }

  assert( (pPager->doNotSpill & SPILLFLAG_NOSYNC)!=0 );
  pPager->doNotSpill &= ~SPILLFLAG_NOSYNC;
  return rc;
}

/*
** Mark a data page as writeable. This routine must be called before
** making changes to a page. The caller must check the return value
** of this function and be careful not to change any page data unless
** this routine returns SQLITE_OK.
**
** The difference between this function and pager_write() is that this
** function also deals with the special case where 2 or more pages
** fit on a single disk sector. In this case all co-resident pages
** must have been written to the journal file before returning.
**
** If an error occurs, SQLITE_NOMEM or an IO error code is returned
** as appropriate. Otherwise, SQLITE_OK.
*/
SQLITE_PRIVATE int sqlite3PagerWrite(PgHdr *pPg){
  Pager *pPager = pPg->pPager;
  assert( (pPg->flags & PGHDR_MMAP)==0 );
  assert( pPager->eState>=PAGER_WRITER_LOCKED );
  assert( assert_pager_state(pPager) );
  if( (pPg->flags & PGHDR_WRITEABLE)!=0 && pPager->dbSize>=pPg->pgno ){
    if( pPager->nSavepoint ) return subjournalPageIfRequired(pPg);
    return SQLITE_OK;
  }else if( pPager->errCode ){
    return pPager->errCode;
  }else if( pPager->sectorSize > (u32)pPager->pageSize ){
    assert( pPager->tempFile==0 );
    return pagerWriteLargeSector(pPg);
  }else{
    return pager_write(pPg);
  }
}

/*
** Return TRUE if the page given in the argument was previously passed
** to sqlite3PagerWrite().  In other words, return TRUE if it is ok
** to change the content of the page.
*/
#ifndef NDEBUG
SQLITE_PRIVATE int sqlite3PagerIswriteable(DbPage *pPg){
  return pPg->flags & PGHDR_WRITEABLE;
}
