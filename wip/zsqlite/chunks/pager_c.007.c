

/*
** Read the content for page pPg out of the database file (or out of
** the WAL if that is where the most recent copy if found) into
** pPg->pData. A shared lock or greater must be held on the database
** file before this function is called.
**
** If page 1 is read, then the value of Pager.dbFileVers[] is set to
** the value read from the database file.
**
** If an IO error occurs, then the IO error is returned to the caller.
** Otherwise, SQLITE_OK is returned.
*/
static int readDbPage(PgHdr *pPg){
  Pager *pPager = pPg->pPager; /* Pager object associated with page pPg */
  int rc = SQLITE_OK;          /* Return code */

#ifndef SQLITE_OMIT_WAL
  u32 iFrame = 0;              /* Frame of WAL containing pgno */

  assert( pPager->eState>=PAGER_READER && !MEMDB );
  assert( isOpen(pPager->fd) );

  if( pagerUseWal(pPager) ){
    rc = sqlite3WalFindFrame(pPager->pWal, pPg->pgno, &iFrame);
    if( rc ) return rc;
  }
  if( iFrame ){
    rc = sqlite3WalReadFrame(pPager->pWal, iFrame,pPager->pageSize,pPg->pData);
  }else
#endif
  {
    i64 iOffset = (pPg->pgno-1)*(i64)pPager->pageSize;
    rc = sqlite3OsRead(pPager->fd, pPg->pData, pPager->pageSize, iOffset);
    if( rc==SQLITE_IOERR_SHORT_READ ){
      rc = SQLITE_OK;
    }
  }

  if( pPg->pgno==1 ){
    if( rc ){
      /* If the read is unsuccessful, set the dbFileVers[] to something
      ** that will never be a valid file version.  dbFileVers[] is a copy
      ** of bytes 24..39 of the database.  Bytes 28..31 should always be
      ** zero or the size of the database in page. Bytes 32..35 and 35..39
      ** should be page numbers which are never 0xffffffff.  So filling
      ** pPager->dbFileVers[] with all 0xff bytes should suffice.
      **
      ** For an encrypted database, the situation is more complex:  bytes
      ** 24..39 of the database are white noise.  But the probability of
      ** white noise equaling 16 bytes of 0xff is vanishingly small so
      ** we should still be ok.
      */
      memset(pPager->dbFileVers, 0xff, sizeof(pPager->dbFileVers));
    }else{
      u8 *dbFileVers = &((u8*)pPg->pData)[24];
      memcpy(&pPager->dbFileVers, dbFileVers, sizeof(pPager->dbFileVers));
    }
  }
  PAGER_INCR(sqlite3_pager_readdb_count);
  PAGER_INCR(pPager->nRead);
  IOTRACE(("PGIN %p %d\n", pPager, pPg->pgno));
  PAGERTRACE(("FETCH %d page %d hash(%08x)\n",
               PAGERID(pPager), pPg->pgno, pager_pagehash(pPg)));

  return rc;
}

/*
** Update the value of the change-counter at offsets 24 and 92 in
** the header and the sqlite version number at offset 96.
**
** This is an unconditional update.  See also the pager_incr_changecounter()
** routine which only updates the change-counter if the update is actually
** needed, as determined by the pPager->changeCountDone state variable.
*/
static void pager_write_changecounter(PgHdr *pPg){
  u32 change_counter;
  if( NEVER(pPg==0) ) return;

  /* Increment the value just read and write it back to byte 24. */
  change_counter = sqlite3Get4byte((u8*)pPg->pPager->dbFileVers)+1;
  put32bits(((char*)pPg->pData)+24, change_counter);

  /* Also store the SQLite version number in bytes 96..99 and in
  ** bytes 92..95 store the change counter for which the version number
  ** is valid. */
  put32bits(((char*)pPg->pData)+92, change_counter);
  put32bits(((char*)pPg->pData)+96, SQLITE_VERSION_NUMBER);
}

#ifndef SQLITE_OMIT_WAL
/*
** This function is invoked once for each page that has already been
** written into the log file when a WAL transaction is rolled back.
** Parameter iPg is the page number of said page. The pCtx argument
** is actually a pointer to the Pager structure.
**
** If page iPg is present in the cache, and has no outstanding references,
** it is discarded. Otherwise, if there are one or more outstanding
** references, the page content is reloaded from the database. If the
** attempt to reload content from the database is required and fails,
** return an SQLite error code. Otherwise, SQLITE_OK.
*/
static int pagerUndoCallback(void *pCtx, Pgno iPg){
  int rc = SQLITE_OK;
  Pager *pPager = (Pager *)pCtx;
  PgHdr *pPg;

  assert( pagerUseWal(pPager) );
  pPg = sqlite3PagerLookup(pPager, iPg);
  if( pPg ){
    if( sqlite3PcachePageRefcount(pPg)==1 ){
      sqlite3PcacheDrop(pPg);
    }else{
      rc = readDbPage(pPg);
      if( rc==SQLITE_OK ){
        pPager->xReiniter(pPg);
      }
      sqlite3PagerUnrefNotNull(pPg);
    }
  }

  /* Normally, if a transaction is rolled back, any backup processes are
  ** updated as data is copied out of the rollback journal and into the
  ** database. This is not generally possible with a WAL database, as
  ** rollback involves simply truncating the log file. Therefore, if one
  ** or more frames have already been written to the log (and therefore
  ** also copied into the backup databases) as part of this transaction,
  ** the backups must be restarted.
  */
  sqlite3BackupRestart(pPager->pBackup);

  return rc;
}

/*
** This function is called to rollback a transaction on a WAL database.
*/
static int pagerRollbackWal(Pager *pPager){
  int rc;                         /* Return Code */
  PgHdr *pList;                   /* List of dirty pages to revert */

  /* For all pages in the cache that are currently dirty or have already
  ** been written (but not committed) to the log file, do one of the
  ** following:
  **
  **   + Discard the cached page (if refcount==0), or
  **   + Reload page content from the database (if refcount>0).
  */
  pPager->dbSize = pPager->dbOrigSize;
  rc = sqlite3WalUndo(pPager->pWal, pagerUndoCallback, (void *)pPager);
  pList = sqlite3PcacheDirtyList(pPager->pPCache);
  while( pList && rc==SQLITE_OK ){
    PgHdr *pNext = pList->pDirty;
    rc = pagerUndoCallback((void *)pPager, pList->pgno);
    pList = pNext;
  }

  return rc;
}

/*
** This function is a wrapper around sqlite3WalFrames(). As well as logging
** the contents of the list of pages headed by pList (connected by pDirty),
** this function notifies any active backup processes that the pages have
** changed.
**
** The list of pages passed into this routine is always sorted by page number.
** Hence, if page 1 appears anywhere on the list, it will be the first page.
*/
static int pagerWalFrames(
  Pager *pPager,                  /* Pager object */
  PgHdr *pList,                   /* List of frames to log */
  Pgno nTruncate,                 /* Database size after this commit */
  int isCommit                    /* True if this is a commit */
){
  int rc;                         /* Return code */
  int nList;                      /* Number of pages in pList */
  PgHdr *p;                       /* For looping over pages */

  assert( pPager->pWal );
  assert( pList );
#ifdef SQLITE_DEBUG
  /* Verify that the page list is in ascending order */
  for(p=pList; p && p->pDirty; p=p->pDirty){
    assert( p->pgno < p->pDirty->pgno );
  }
#endif

  assert( pList->pDirty==0 || isCommit );
  if( isCommit ){
    /* If a WAL transaction is being committed, there is no point in writing
    ** any pages with page numbers greater than nTruncate into the WAL file.
    ** They will never be read by any client. So remove them from the pDirty
    ** list here. */
    PgHdr **ppNext = &pList;
    nList = 0;
    for(p=pList; (*ppNext = p)!=0; p=p->pDirty){
      if( p->pgno<=nTruncate ){
        ppNext = &p->pDirty;
        nList++;
      }
    }
    assert( pList );
  }else{
    nList = 1;
  }
  pPager->aStat[PAGER_STAT_WRITE] += nList;

  if( pList->pgno==1 ) pager_write_changecounter(pList);
  rc = sqlite3WalFrames(pPager->pWal,
      pPager->pageSize, pList, nTruncate, isCommit, pPager->walSyncFlags
  );
  if( rc==SQLITE_OK && pPager->pBackup ){
    for(p=pList; p; p=p->pDirty){
      sqlite3BackupUpdate(pPager->pBackup, p->pgno, (u8 *)p->pData);
    }
  }

#ifdef SQLITE_CHECK_PAGES
  pList = sqlite3PcacheDirtyList(pPager->pPCache);
  for(p=pList; p; p=p->pDirty){
    pager_set_pagehash(p);
  }
#endif

  return rc;
}

/*
** Begin a read transaction on the WAL.
**
** This routine used to be called "pagerOpenSnapshot()" because it essentially
** makes a snapshot of the database at the current point in time and preserves
** that snapshot for use by the reader in spite of concurrently changes by
** other writers or checkpointers.
*/
static int pagerBeginReadTransaction(Pager *pPager){
  int rc;                         /* Return code */
  int changed = 0;                /* True if cache must be reset */

  assert( pagerUseWal(pPager) );
  assert( pPager->eState==PAGER_OPEN || pPager->eState==PAGER_READER );

  /* sqlite3WalEndReadTransaction() was not called for the previous
  ** transaction in locking_mode=EXCLUSIVE.  So call it now.  If we
  ** are in locking_mode=NORMAL and EndRead() was previously called,
  ** the duplicate call is harmless.
  */
  sqlite3WalEndReadTransaction(pPager->pWal);

  rc = sqlite3WalBeginReadTransaction(pPager->pWal, &changed);
  if( rc!=SQLITE_OK || changed ){
    pager_reset(pPager);
    if( USEFETCH(pPager) ) sqlite3OsUnfetch(pPager->fd, 0, 0);
  }

  return rc;
}
#endif

/*
** This function is called as part of the transition from PAGER_OPEN
** to PAGER_READER state to determine the size of the database file
** in pages (assuming the page size currently stored in Pager.pageSize).
**
** If no error occurs, SQLITE_OK is returned and the size of the database
** in pages is stored in *pnPage. Otherwise, an error code (perhaps
** SQLITE_IOERR_FSTAT) is returned and *pnPage is left unmodified.
*/
static int pagerPagecount(Pager *pPager, Pgno *pnPage){
  Pgno nPage;                     /* Value to return via *pnPage */

  /* Query the WAL sub-system for the database size. The WalDbsize()
  ** function returns zero if the WAL is not open (i.e. Pager.pWal==0), or
  ** if the database size is not available. The database size is not
  ** available from the WAL sub-system if the log file is empty or
  ** contains no valid committed transactions.
  */
  assert( pPager->eState==PAGER_OPEN );
  assert( pPager->eLock>=SHARED_LOCK );
  assert( isOpen(pPager->fd) );
  assert( pPager->tempFile==0 );
  nPage = sqlite3WalDbsize(pPager->pWal);

  /* If the number of pages in the database is not available from the
  ** WAL sub-system, determine the page count based on the size of
  ** the database file.  If the size of the database file is not an
  ** integer multiple of the page-size, round up the result.
  */
  if( nPage==0 && ALWAYS(isOpen(pPager->fd)) ){
    i64 n = 0;                    /* Size of db file in bytes */
    int rc = sqlite3OsFileSize(pPager->fd, &n);
    if( rc!=SQLITE_OK ){
      return rc;
    }
    nPage = (Pgno)((n+pPager->pageSize-1) / pPager->pageSize);
  }

  /* If the current number of pages in the file is greater than the
  ** configured maximum pager number, increase the allowed limit so
  ** that the file can be read.
  */
  if( nPage>pPager->mxPgno ){
    pPager->mxPgno = (Pgno)nPage;
  }

  *pnPage = nPage;
  return SQLITE_OK;
}

#ifndef SQLITE_OMIT_WAL
/*
** Check if the *-wal file that corresponds to the database opened by pPager
** exists if the database is not empty, or verify that the *-wal file does
** not exist (by deleting it) if the database file is empty.
**
** If the database is not empty and the *-wal file exists, open the pager
** in WAL mode.  If the database is empty or if no *-wal file exists and
** if no error occurs, make sure Pager.journalMode is not set to
** PAGER_JOURNALMODE_WAL.
**
** Return SQLITE_OK or an error code.
**
** The caller must hold a SHARED lock on the database file to call this
** function. Because an EXCLUSIVE lock on the db file is required to delete
** a WAL on a none-empty database, this ensures there is no race condition
** between the xAccess() below and an xDelete() being executed by some
** other connection.
*/
static int pagerOpenWalIfPresent(Pager *pPager){
  int rc = SQLITE_OK;
  assert( pPager->eState==PAGER_OPEN );
  assert( pPager->eLock>=SHARED_LOCK );

  if( !pPager->tempFile ){
    int isWal;                    /* True if WAL file exists */
    rc = sqlite3OsAccess(
        pPager->pVfs, pPager->zWal, SQLITE_ACCESS_EXISTS, &isWal
    );
    if( rc==SQLITE_OK ){
      if( isWal ){
        Pgno nPage;                   /* Size of the database file */

        rc = pagerPagecount(pPager, &nPage);
        if( rc ) return rc;
        if( nPage==0 ){
          rc = sqlite3OsDelete(pPager->pVfs, pPager->zWal, 0);
        }else{
          testcase( sqlite3PcachePagecount(pPager->pPCache)==0 );
          rc = sqlite3PagerOpenWal(pPager, 0);
        }
      }else if( pPager->journalMode==PAGER_JOURNALMODE_WAL ){
        pPager->journalMode = PAGER_JOURNALMODE_DELETE;
      }
    }
  }
  return rc;
}
