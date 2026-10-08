
/*
** Change the page size used by the Pager object. The new page size
** is passed in *pPageSize.
**
** If the pager is in the error state when this function is called, it
** is a no-op. The value returned is the error state error code (i.e.
** one of SQLITE_IOERR, an SQLITE_IOERR_xxx sub-code or SQLITE_FULL).
**
** Otherwise, if all of the following are true:
**
**   * the new page size (value of *pPageSize) is valid (a power
**     of two between 512 and SQLITE_MAX_PAGE_SIZE, inclusive), and
**
**   * there are no outstanding page references, and
**
**   * the database is either not an in-memory database or it is
**     an in-memory database that currently consists of zero pages.
**
** then the pager object page size is set to *pPageSize.
**
** If the page size is changed, then this function uses sqlite3PagerMalloc()
** to obtain a new Pager.pTmpSpace buffer. If this allocation attempt
** fails, SQLITE_NOMEM is returned and the page size remains unchanged.
** In all other cases, SQLITE_OK is returned.
**
** If the page size is not changed, either because one of the enumerated
** conditions above is not true, the pager was in error state when this
** function was called, or because the memory allocation attempt failed,
** then *pPageSize is set to the old, retained page size before returning.
*/
SQLITE_PRIVATE int sqlite3PagerSetPagesize(Pager *pPager, u32 *pPageSize, int nReserve){
  int rc = SQLITE_OK;

  /* It is not possible to do a full assert_pager_state() here, as this
  ** function may be called from within PagerOpen(), before the state
  ** of the Pager object is internally consistent.
  **
  ** At one point this function returned an error if the pager was in
  ** PAGER_ERROR state. But since PAGER_ERROR state guarantees that
  ** there is at least one outstanding page reference, this function
  ** is a no-op for that case anyhow.
  */

  u32 pageSize = *pPageSize;
  assert( pageSize==0 || (pageSize>=512 && pageSize<=SQLITE_MAX_PAGE_SIZE) );
  if( (pPager->memDb==0 || pPager->dbSize==0)
   && sqlite3PcacheRefCount(pPager->pPCache)==0
   && pageSize && pageSize!=(u32)pPager->pageSize
  ){
    char *pNew = NULL;             /* New temp space */
    i64 nByte = 0;

    if( pPager->eState>PAGER_OPEN && isOpen(pPager->fd) ){
      rc = sqlite3OsFileSize(pPager->fd, &nByte);
    }
    if( rc==SQLITE_OK ){
      /* 8 bytes of zeroed overrun space is sufficient so that the b-tree
      * cell header parser will never run off the end of the allocation */
      pNew = (char *)sqlite3PageMalloc(pageSize+8);
      if( !pNew ){
        rc = SQLITE_NOMEM_BKPT;
      }else{
        memset(pNew+pageSize, 0, 8);
      }
    }

    if( rc==SQLITE_OK ){
      pager_reset(pPager);
      rc = sqlite3PcacheSetPageSize(pPager->pPCache, pageSize);
    }
    if( rc==SQLITE_OK ){
      sqlite3PageFree(pPager->pTmpSpace);
      pPager->pTmpSpace = pNew;
      pPager->dbSize = (Pgno)((nByte+pageSize-1)/pageSize);
      pPager->pageSize = pageSize;
      pPager->lckPgno = (Pgno)(PENDING_BYTE/pageSize) + 1;
    }else{
      sqlite3PageFree(pNew);
    }
  }

  *pPageSize = pPager->pageSize;
  if( rc==SQLITE_OK ){
    if( nReserve<0 ) nReserve = pPager->nReserve;
    assert( nReserve>=0 && nReserve<1000 );
    pPager->nReserve = (i16)nReserve;
    pagerFixMaplimit(pPager);
  }
  return rc;
}

/*
** Return a pointer to the "temporary page" buffer held internally
** by the pager.  This is a buffer that is big enough to hold the
** entire content of a database page.  This buffer is used internally
** during rollback and will be overwritten whenever a rollback
** occurs.  But other modules are free to use it too, as long as
** no rollbacks are happening.
*/
SQLITE_PRIVATE void *sqlite3PagerTempSpace(Pager *pPager){
  return pPager->pTmpSpace;
}

/*
** Attempt to set the maximum database page count if mxPage is positive.
** Make no changes if mxPage is zero or negative.  And never reduce the
** maximum page count below the current size of the database.
**
** Regardless of mxPage, return the current maximum page count.
*/
SQLITE_PRIVATE Pgno sqlite3PagerMaxPageCount(Pager *pPager, Pgno mxPage){
  if( mxPage>0 ){
    pPager->mxPgno = mxPage;
  }
  assert( pPager->eState!=PAGER_OPEN );      /* Called only by OP_MaxPgcnt */
  /* assert( pPager->mxPgno>=pPager->dbSize ); */
  /* OP_MaxPgcnt ensures that the parameter passed to this function is not
  ** less than the total number of valid pages in the database. But this
  ** may be less than Pager.dbSize, and so the assert() above is not valid */
  return pPager->mxPgno;
}

/*
** The following set of routines are used to disable the simulated
** I/O error mechanism.  These routines are used to avoid simulated
** errors in places where we do not care about errors.
**
** Unless -DSQLITE_TEST=1 is used, these routines are all no-ops
** and generate no code.
*/
#ifdef SQLITE_TEST
SQLITE_API extern int sqlite3_io_error_pending;
SQLITE_API extern int sqlite3_io_error_hit;
static int saved_cnt;
void disable_simulated_io_errors(void){
  saved_cnt = sqlite3_io_error_pending;
  sqlite3_io_error_pending = -1;
}
void enable_simulated_io_errors(void){
  sqlite3_io_error_pending = saved_cnt;
}
#else
# define disable_simulated_io_errors()
# define enable_simulated_io_errors()
#endif

/*
** Read the first N bytes from the beginning of the file into memory
** that pDest points to.
**
** If the pager was opened on a transient file (zFilename==""), or
** opened on a file less than N bytes in size, the output buffer is
** zeroed and SQLITE_OK returned. The rationale for this is that this
** function is used to read database headers, and a new transient or
** zero sized database has a header than consists entirely of zeroes.
**
** If any IO error apart from SQLITE_IOERR_SHORT_READ is encountered,
** the error code is returned to the caller and the contents of the
** output buffer undefined.
*/
SQLITE_PRIVATE int sqlite3PagerReadFileheader(Pager *pPager, int N, unsigned char *pDest){
  int rc = SQLITE_OK;
  memset(pDest, 0, N);
  assert( isOpen(pPager->fd) || pPager->tempFile );

  /* This routine is only called by btree immediately after creating
  ** the Pager object.  There has not been an opportunity to transition
  ** to WAL mode yet.
  */
  assert( !pagerUseWal(pPager) );

  if( isOpen(pPager->fd) ){
    IOTRACE(("DBHDR %p 0 %d\n", pPager, N))
    rc = sqlite3OsRead(pPager->fd, pDest, N, 0);
    if( rc==SQLITE_IOERR_SHORT_READ ){
      rc = SQLITE_OK;
    }
  }
  return rc;
}

/*
** This function may only be called when a read-transaction is open on
** the pager. It returns the total number of pages in the database.
**
** However, if the file is between 1 and <page-size> bytes in size, then
** this is considered a 1 page file.
*/
SQLITE_PRIVATE void sqlite3PagerPagecount(Pager *pPager, int *pnPage){
  assert( pPager->eState>=PAGER_READER );
  assert( pPager->eState!=PAGER_WRITER_FINISHED );
  *pnPage = (int)pPager->dbSize;
}


/*
** Try to obtain a lock of type locktype on the database file. If
** a similar or greater lock is already held, this function is a no-op
** (returning SQLITE_OK immediately).
**
** Otherwise, attempt to obtain the lock using sqlite3OsLock(). Invoke
** the busy callback if the lock is currently not available. Repeat
** until the busy callback returns false or until the attempt to
** obtain the lock succeeds.
**
** Return SQLITE_OK on success and an error code if we cannot obtain
** the lock. If the lock is obtained successfully, set the Pager.state
** variable to locktype before returning.
*/
static int pager_wait_on_lock(Pager *pPager, int locktype){
  int rc;                              /* Return code */

  /* Check that this is either a no-op (because the requested lock is
  ** already held), or one of the transitions that the busy-handler
  ** may be invoked during, according to the comment above
  ** sqlite3PagerSetBusyhandler().
  */
  assert( (pPager->eLock>=locktype)
       || (pPager->eLock==NO_LOCK && locktype==SHARED_LOCK)
       || (pPager->eLock==RESERVED_LOCK && locktype==EXCLUSIVE_LOCK)
  );

  do {
    rc = pagerLockDb(pPager, locktype);
  }while( rc==SQLITE_BUSY && pPager->xBusyHandler(pPager->pBusyHandlerArg) );
  return rc;
}

/*
** Function assertTruncateConstraint(pPager) checks that one of the
** following is true for all dirty pages currently in the page-cache:
**
**   a) The page number is less than or equal to the size of the
**      current database image, in pages, OR
**
**   b) if the page content were written at this time, it would not
**      be necessary to write the current content out to the sub-journal.
**
** If the condition asserted by this function were not true, and the
** dirty page were to be discarded from the cache via the pagerStress()
** routine, pagerStress() would not write the current page content to
** the database file. If a savepoint transaction were rolled back after
** this happened, the correct behavior would be to restore the current
** content of the page. However, since this content is not present in either
** the database file or the portion of the rollback journal and
** sub-journal rolled back the content could not be restored and the
** database image would become corrupt. It is therefore fortunate that
** this circumstance cannot arise.
*/
#if defined(SQLITE_DEBUG)
static void assertTruncateConstraintCb(PgHdr *pPg){
  Pager *pPager = pPg->pPager;
  assert( pPg->flags&PGHDR_DIRTY );
  if( pPg->pgno>pPager->dbSize ){      /* if (a) is false */
    Pgno pgno = pPg->pgno;
    int i;
    for(i=0; i<pPg->pPager->nSavepoint; i++){
      PagerSavepoint *p = &pPager->aSavepoint[i];
      assert( p->nOrig<pgno || sqlite3BitvecTestNotNull(p->pInSavepoint,pgno) );
    }
  }
}
static void assertTruncateConstraint(Pager *pPager){
  sqlite3PcacheIterateDirty(pPager->pPCache, assertTruncateConstraintCb);
}
#else
# define assertTruncateConstraint(pPager)
#endif

/*
** Truncate the in-memory database file image to nPage pages. This
** function does not actually modify the database file on disk. It
** just sets the internal state of the pager object so that the
** truncation will be done when the current transaction is committed.
**
** This function is only called right before committing a transaction.
** Once this function has been called, the transaction must either be
** rolled back or committed. It is not safe to call this function and
** then continue writing to the database.
*/
SQLITE_PRIVATE void sqlite3PagerTruncateImage(Pager *pPager, Pgno nPage){
  assert( pPager->dbSize>=nPage || CORRUPT_DB );
  assert( pPager->eState>=PAGER_WRITER_CACHEMOD );
  pPager->dbSize = nPage;

  /* At one point the code here called assertTruncateConstraint() to
  ** ensure that all pages being truncated away by this operation are,
  ** if one or more savepoints are open, present in the savepoint
  ** journal so that they can be restored if the savepoint is rolled
  ** back. This is no longer necessary as this function is now only
  ** called right before committing a transaction. So although the
  ** Pager object may still have open savepoints (Pager.nSavepoint!=0),
  ** they cannot be rolled back. So the assertTruncateConstraint() call
  ** is no longer correct. */
}


/*
** This function is called before attempting a hot-journal rollback. It
** syncs the journal file to disk, then sets pPager->journalHdr to the
** size of the journal file so that the pager_playback() routine knows
** that the entire journal file has been synced.
**
** Syncing a hot-journal to disk before attempting to roll it back ensures
** that if a power-failure occurs during the rollback, the process that
** attempts rollback following system recovery sees the same journal
** content as this process.
**
** If everything goes as planned, SQLITE_OK is returned. Otherwise,
** an SQLite error code.
*/
static int pagerSyncHotJournal(Pager *pPager){
  int rc = SQLITE_OK;
  if( !pPager->noSync ){
    rc = sqlite3OsSync(pPager->jfd, SQLITE_SYNC_NORMAL);
  }
  if( rc==SQLITE_OK ){
    rc = sqlite3OsFileSize(pPager->jfd, &pPager->journalHdr);
  }
  return rc;
}

#if SQLITE_MAX_MMAP_SIZE>0
/*
** Obtain a reference to a memory mapped page object for page number pgno.
** The new object will use the pointer pData, obtained from xFetch().
** If successful, set *ppPage to point to the new page reference
** and return SQLITE_OK. Otherwise, return an SQLite error code and set
** *ppPage to zero.
**
** Page references obtained by calling this function should be released
** by calling pagerReleaseMapPage().
*/
static int pagerAcquireMapPage(
  Pager *pPager,                  /* Pager object */
  Pgno pgno,                      /* Page number */
  void *pData,                    /* xFetch()'d data for this page */
  PgHdr **ppPage                  /* OUT: Acquired page object */
){
  PgHdr *p;                       /* Memory mapped page to return */

  if( pPager->pMmapFreelist ){
    *ppPage = p = pPager->pMmapFreelist;
    pPager->pMmapFreelist = p->pDirty;
    p->pDirty = 0;
    assert( pPager->nExtra>=8 );
    memset(p->pExtra, 0, 8);
  }else{
    *ppPage = p = (PgHdr *)sqlite3MallocZero(sizeof(PgHdr) + pPager->nExtra);
    if( p==0 ){
      sqlite3OsUnfetch(pPager->fd, (i64)(pgno-1) * pPager->pageSize, pData);
      return SQLITE_NOMEM_BKPT;
    }
    p->pExtra = (void *)&p[1];
    p->flags = PGHDR_MMAP;
    p->nRef = 1;
    p->pPager = pPager;
  }

  assert( p->pExtra==(void *)&p[1] );
  assert( p->pPage==0 );
  assert( p->flags==PGHDR_MMAP );
  assert( p->pPager==pPager );
  assert( p->nRef==1 );

  p->pgno = pgno;
  p->pData = pData;
  pPager->nMmapOut++;

  return SQLITE_OK;
}
