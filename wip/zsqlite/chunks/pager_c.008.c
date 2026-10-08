#endif

/*
** Playback savepoint pSavepoint. Or, if pSavepoint==NULL, then playback
** the entire super-journal file. The case pSavepoint==NULL occurs when
** a ROLLBACK TO command is invoked on a SAVEPOINT that is a transaction
** savepoint.
**
** When pSavepoint is not NULL (meaning a non-transaction savepoint is
** being rolled back), then the rollback consists of up to three stages,
** performed in the order specified:
**
**   * Pages are played back from the main journal starting at byte
**     offset PagerSavepoint.iOffset and continuing to
**     PagerSavepoint.iHdrOffset, or to the end of the main journal
**     file if PagerSavepoint.iHdrOffset is zero.
**
**   * If PagerSavepoint.iHdrOffset is not zero, then pages are played
**     back starting from the journal header immediately following
**     PagerSavepoint.iHdrOffset to the end of the main journal file.
**
**   * Pages are then played back from the sub-journal file, starting
**     with the PagerSavepoint.iSubRec and continuing to the end of
**     the journal file.
**
** Throughout the rollback process, each time a page is rolled back, the
** corresponding bit is set in a bitvec structure (variable pDone in the
** implementation below). This is used to ensure that a page is only
** rolled back the first time it is encountered in either journal.
**
** If pSavepoint is NULL, then pages are only played back from the main
** journal file. There is no need for a bitvec in this case.
**
** In either case, before playback commences the Pager.dbSize variable
** is reset to the value that it held at the start of the savepoint
** (or transaction). No page with a page-number greater than this value
** is played back. If one is encountered it is simply skipped.
*/
static int pagerPlaybackSavepoint(Pager *pPager, PagerSavepoint *pSavepoint){
  i64 szJ;                 /* Effective size of the main journal */
  i64 iHdrOff;             /* End of first segment of main-journal records */
  int rc = SQLITE_OK;      /* Return code */
  Bitvec *pDone = 0;       /* Bitvec to ensure pages played back only once */

  assert( pPager->eState!=PAGER_ERROR );
  assert( pPager->eState>=PAGER_WRITER_LOCKED );

  /* Allocate a bitvec to use to store the set of pages rolled back */
  if( pSavepoint ){
    pDone = sqlite3BitvecCreate(pSavepoint->nOrig);
    if( !pDone ){
      return SQLITE_NOMEM_BKPT;
    }
  }

  /* Set the database size back to the value it was before the savepoint
  ** being reverted was opened.
  */
  pPager->dbSize = pSavepoint ? pSavepoint->nOrig : pPager->dbOrigSize;
  pPager->changeCountDone = pPager->tempFile;

  if( !pSavepoint && pagerUseWal(pPager) ){
    return pagerRollbackWal(pPager);
  }

  /* Use pPager->journalOff as the effective size of the main rollback
  ** journal.  The actual file might be larger than this in
  ** PAGER_JOURNALMODE_TRUNCATE or PAGER_JOURNALMODE_PERSIST.  But anything
  ** past pPager->journalOff is off-limits to us.
  */
  szJ = pPager->journalOff;
  assert( pagerUseWal(pPager)==0 || szJ==0 );

  /* Begin by rolling back records from the main journal starting at
  ** PagerSavepoint.iOffset and continuing to the next journal header.
  ** There might be records in the main journal that have a page number
  ** greater than the current database size (pPager->dbSize) but those
  ** will be skipped automatically.  Pages are added to pDone as they
  ** are played back.
  */
  if( pSavepoint && !pagerUseWal(pPager) ){
    iHdrOff = pSavepoint->iHdrOffset ? pSavepoint->iHdrOffset : szJ;
    pPager->journalOff = pSavepoint->iOffset;
    while( rc==SQLITE_OK && pPager->journalOff<iHdrOff ){
      rc = pager_playback_one_page(pPager, &pPager->journalOff, pDone, 1, 1);
    }
    assert( rc!=SQLITE_DONE );
  }else{
    pPager->journalOff = 0;
  }

  /* Continue rolling back records out of the main journal starting at
  ** the first journal header seen and continuing until the effective end
  ** of the main journal file.  Continue to skip out-of-range pages and
  ** continue adding pages rolled back to pDone.
  */
  while( rc==SQLITE_OK && pPager->journalOff<szJ ){
    u32 ii;            /* Loop counter */
    u32 nJRec = 0;     /* Number of Journal Records */
    u32 dummy;
    rc = readJournalHdr(pPager, 0, szJ, &nJRec, &dummy);
    assert( rc!=SQLITE_DONE );

    /*
    ** The "pPager->journalHdr+JOURNAL_HDR_SZ(pPager)==pPager->journalOff"
    ** test is related to ticket #2565.  See the discussion in the
    ** pager_playback() function for additional information.
    */
    if( nJRec==0
     && pPager->journalHdr+JOURNAL_HDR_SZ(pPager)==pPager->journalOff
    ){
      nJRec = (u32)((szJ - pPager->journalOff)/JOURNAL_PG_SZ(pPager));
    }
    for(ii=0; rc==SQLITE_OK && ii<nJRec && pPager->journalOff<szJ; ii++){
      rc = pager_playback_one_page(pPager, &pPager->journalOff, pDone, 1, 1);
    }
    assert( rc!=SQLITE_DONE );
  }
  assert( rc!=SQLITE_OK || pPager->journalOff>=szJ );

  /* Finally,  rollback pages from the sub-journal.  Page that were
  ** previously rolled back out of the main journal (and are hence in pDone)
  ** will be skipped.  Out-of-range pages are also skipped.
  */
  if( pSavepoint ){
    u32 ii;            /* Loop counter */
    i64 offset = (i64)pSavepoint->iSubRec*(4+pPager->pageSize);

    if( pagerUseWal(pPager) ){
      rc = sqlite3WalSavepointUndo(pPager->pWal, pSavepoint->aWalData);
    }
    for(ii=pSavepoint->iSubRec; rc==SQLITE_OK && ii<pPager->nSubRec; ii++){
      assert( offset==(i64)ii*(4+pPager->pageSize) );
      rc = pager_playback_one_page(pPager, &offset, pDone, 0, 1);
    }
    assert( rc!=SQLITE_DONE );
  }

  sqlite3BitvecDestroy(pDone);
  if( rc==SQLITE_OK ){
    pPager->journalOff = szJ;
  }

  return rc;
}

/*
** Change the maximum number of in-memory pages that are allowed
** before attempting to recycle clean and unused pages.
*/
SQLITE_PRIVATE void sqlite3PagerSetCachesize(Pager *pPager, int mxPage){
  sqlite3PcacheSetCachesize(pPager->pPCache, mxPage);
}

/*
** Change the maximum number of in-memory pages that are allowed
** before attempting to spill pages to journal.
*/
SQLITE_PRIVATE int sqlite3PagerSetSpillsize(Pager *pPager, int mxPage){
  return sqlite3PcacheSetSpillsize(pPager->pPCache, mxPage);
}

/*
** Invoke SQLITE_FCNTL_MMAP_SIZE based on the current value of szMmap.
*/
static void pagerFixMaplimit(Pager *pPager){
#if SQLITE_MAX_MMAP_SIZE>0
  sqlite3_file *fd = pPager->fd;
  if( isOpen(fd) && fd->pMethods->iVersion>=3 ){
    sqlite3_int64 sz;
    sz = pPager->szMmap;
    pPager->bUseFetch = (sz>0);
    setGetterMethod(pPager);
    sqlite3OsFileControlHint(pPager->fd, SQLITE_FCNTL_MMAP_SIZE, &sz);
  }
#endif
}

/*
** Change the maximum size of any memory mapping made of the database file.
*/
SQLITE_PRIVATE void sqlite3PagerSetMmapLimit(Pager *pPager, sqlite3_int64 szMmap){
  pPager->szMmap = szMmap;
  pagerFixMaplimit(pPager);
}

/*
** Free as much memory as possible from the pager.
*/
SQLITE_PRIVATE void sqlite3PagerShrink(Pager *pPager){
  sqlite3PcacheShrink(pPager->pPCache);
}

/*
** Adjust settings of the pager to those specified in the pgFlags parameter.
**
** The "level" in pgFlags & PAGER_SYNCHRONOUS_MASK sets the robustness
** of the database to damage due to OS crashes or power failures by
** changing the number of syncs()s when writing the journals.
** There are four levels:
**
**    OFF       sqlite3OsSync() is never called.  This is the default
**              for temporary and transient files.
**
**    NORMAL    The journal is synced once before writes begin on the
**              database.  This is normally adequate protection, but
**              it is theoretically possible, though very unlikely,
**              that an inopertune power failure could leave the journal
**              in a state which would cause damage to the database
**              when it is rolled back.
**
**    FULL      The journal is synced twice before writes begin on the
**              database (with some additional information - the nRec field
**              of the journal header - being written in between the two
**              syncs).  If we assume that writing a
**              single disk sector is atomic, then this mode provides
**              assurance that the journal will not be corrupted to the
**              point of causing damage to the database during rollback.
**
**    EXTRA     This is like FULL except that is also syncs the directory
**              that contains the rollback journal after the rollback
**              journal is unlinked.
**
** The above is for a rollback-journal mode.  For WAL mode, OFF continues
** to mean that no syncs ever occur.  NORMAL means that the WAL is synced
** prior to the start of checkpoint and that the database file is synced
** at the conclusion of the checkpoint if the entire content of the WAL
** was written back into the database.  But no sync operations occur for
** an ordinary commit in NORMAL mode with WAL.  FULL means that the WAL
** file is synced following each commit operation, in addition to the
** syncs associated with NORMAL.  There is no difference between FULL
** and EXTRA for WAL mode.
**
** Do not confuse synchronous=FULL with SQLITE_SYNC_FULL.  The
** SQLITE_SYNC_FULL macro means to use the MacOSX-style full-fsync
** using fcntl(F_FULLFSYNC).  SQLITE_SYNC_NORMAL means to do an
** ordinary fsync() call.  There is no difference between SQLITE_SYNC_FULL
** and SQLITE_SYNC_NORMAL on platforms other than MacOSX.  But the
** synchronous=FULL versus synchronous=NORMAL setting determines when
** the xSync primitive is called and is relevant to all platforms.
**
** Numeric values associated with these states are OFF==1, NORMAL=2,
** and FULL=3.
*/
SQLITE_PRIVATE void sqlite3PagerSetFlags(
  Pager *pPager,        /* The pager to set safety level for */
  unsigned pgFlags      /* Various flags */
){
  unsigned level = pgFlags & PAGER_SYNCHRONOUS_MASK;
  if( pPager->tempFile ){
    pPager->noSync = 1;
    pPager->fullSync = 0;
    pPager->extraSync = 0;
  }else{
    pPager->noSync =  level==PAGER_SYNCHRONOUS_OFF ?1:0;
    pPager->fullSync = level>=PAGER_SYNCHRONOUS_FULL ?1:0;
    pPager->extraSync = level==PAGER_SYNCHRONOUS_EXTRA ?1:0;
  }
  if( pPager->noSync ){
    pPager->syncFlags = 0;
  }else if( pgFlags & PAGER_FULLFSYNC ){
    pPager->syncFlags = SQLITE_SYNC_FULL;
  }else{
    pPager->syncFlags = SQLITE_SYNC_NORMAL;
  }
  pPager->walSyncFlags = (pPager->syncFlags<<2);
  if( pPager->fullSync ){
    pPager->walSyncFlags |= pPager->syncFlags;
  }
  if( (pgFlags & PAGER_CKPT_FULLFSYNC) && !pPager->noSync ){
    pPager->walSyncFlags |= (SQLITE_SYNC_FULL<<2);
  }
  if( pgFlags & PAGER_CACHESPILL ){
    pPager->doNotSpill &= ~SPILLFLAG_OFF;
  }else{
    pPager->doNotSpill |= SPILLFLAG_OFF;
  }
}

/*
** The following global variable is incremented whenever the library
** attempts to open a temporary file.  This information is used for
** testing and analysis only.
*/
#ifdef SQLITE_TEST
SQLITE_API int sqlite3_opentemp_count = 0;
#endif

/*
** Open a temporary file.
**
** Write the file descriptor into *pFile. Return SQLITE_OK on success
** or some other error code if we fail. The OS will automatically
** delete the temporary file when it is closed.
**
** The flags passed to the VFS layer xOpen() call are those specified
** by parameter vfsFlags ORed with the following:
**
**     SQLITE_OPEN_READWRITE
**     SQLITE_OPEN_CREATE
**     SQLITE_OPEN_EXCLUSIVE
**     SQLITE_OPEN_DELETEONCLOSE
*/
static int pagerOpentemp(
  Pager *pPager,        /* The pager object */
  sqlite3_file *pFile,  /* Write the file descriptor here */
  int vfsFlags          /* Flags passed through to the VFS */
){
  int rc;               /* Return code */

#ifdef SQLITE_TEST
  sqlite3_opentemp_count++;  /* Used for testing and analysis only */
#endif

  vfsFlags |=  SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE |
            SQLITE_OPEN_EXCLUSIVE | SQLITE_OPEN_DELETEONCLOSE;
  rc = sqlite3OsOpen(pPager->pVfs, 0, pFile, vfsFlags, 0);
  assert( rc!=SQLITE_OK || isOpen(pFile) );
  return rc;
}

/*
** Set the busy handler function.
**
** The pager invokes the busy-handler if sqlite3OsLock() returns
** SQLITE_BUSY when trying to upgrade from no-lock to a SHARED lock,
** or when trying to upgrade from a RESERVED lock to an EXCLUSIVE
** lock. It does *not* invoke the busy handler when upgrading from
** SHARED to RESERVED, or when upgrading from SHARED to EXCLUSIVE
** (which occurs during hot-journal rollback). Summary:
**
**   Transition                        | Invokes xBusyHandler
**   --------------------------------------------------------
**   NO_LOCK       -> SHARED_LOCK      | Yes
**   SHARED_LOCK   -> RESERVED_LOCK    | No
**   SHARED_LOCK   -> EXCLUSIVE_LOCK   | No
**   RESERVED_LOCK -> EXCLUSIVE_LOCK   | Yes
**
** If the busy-handler callback returns non-zero, the lock is
** retried. If it returns zero, then the SQLITE_BUSY error is
** returned to the caller of the pager API function.
*/
SQLITE_PRIVATE void sqlite3PagerSetBusyHandler(
  Pager *pPager,                       /* Pager object */
  int (*xBusyHandler)(void *),         /* Pointer to busy-handler function */
  void *pBusyHandlerArg                /* Argument to pass to xBusyHandler */
){
  void **ap;
  pPager->xBusyHandler = xBusyHandler;
  pPager->pBusyHandlerArg = pBusyHandlerArg;
  ap = (void **)&pPager->xBusyHandler;
  assert( ((int(*)(void *))(ap[0]))==xBusyHandler );
  assert( ap[1]==pBusyHandlerArg );
  sqlite3OsFileControlHint(pPager->fd, SQLITE_FCNTL_BUSYHANDLER, (void *)ap);
}
