#endif

/*
** Release a reference to page pPg. pPg must have been returned by an
** earlier call to pagerAcquireMapPage().
*/
static void pagerReleaseMapPage(PgHdr *pPg){
  Pager *pPager = pPg->pPager;
  pPager->nMmapOut--;
  pPg->pDirty = pPager->pMmapFreelist;
  pPager->pMmapFreelist = pPg;

  assert( pPager->fd->pMethods->iVersion>=3 );
  sqlite3OsUnfetch(pPager->fd, (i64)(pPg->pgno-1)*pPager->pageSize, pPg->pData);
}

/*
** Free all PgHdr objects stored in the Pager.pMmapFreelist list.
*/
static void pagerFreeMapHdrs(Pager *pPager){
  PgHdr *p;
  PgHdr *pNext;
  for(p=pPager->pMmapFreelist; p; p=pNext){
    pNext = p->pDirty;
    sqlite3_free(p);
  }
}

/* Verify that the database file has not be deleted or renamed out from
** under the pager.  Return SQLITE_OK if the database is still where it ought
** to be on disk.  Return non-zero (SQLITE_READONLY_DBMOVED or some other error
** code from sqlite3OsAccess()) if the database has gone missing.
*/
static int databaseIsUnmoved(Pager *pPager){
  int bHasMoved = 0;
  int rc;

  if( pPager->tempFile ) return SQLITE_OK;
  if( pPager->dbSize==0 ) return SQLITE_OK;
  assert( pPager->zFilename && pPager->zFilename[0] );
  rc = sqlite3OsFileControl(pPager->fd, SQLITE_FCNTL_HAS_MOVED, &bHasMoved);
  if( rc==SQLITE_NOTFOUND ){
    /* If the HAS_MOVED file-control is unimplemented, assume that the file
    ** has not been moved.  That is the historical behavior of SQLite: prior to
    ** version 3.8.3, it never checked */
    rc = SQLITE_OK;
  }else if( rc==SQLITE_OK && bHasMoved ){
    rc = SQLITE_READONLY_DBMOVED;
  }
  return rc;
}


/*
** Shutdown the page cache.  Free all memory and close all files.
**
** If a transaction was in progress when this routine is called, that
** transaction is rolled back.  All outstanding pages are invalidated
** and their memory is freed.  Any attempt to use a page associated
** with this page cache after this function returns will likely
** result in a coredump.
**
** This function always succeeds. If a transaction is active an attempt
** is made to roll it back. If an error occurs during the rollback
** a hot journal may be left in the filesystem but no error is returned
** to the caller.
*/
SQLITE_PRIVATE int sqlite3PagerClose(Pager *pPager, sqlite3 *db){
  u8 *pTmp = (u8*)pPager->pTmpSpace;
  assert( db || pagerUseWal(pPager)==0 );
  assert( assert_pager_state(pPager) );
  disable_simulated_io_errors();
  sqlite3BeginBenignMalloc();
  pagerFreeMapHdrs(pPager);
  /* pPager->errCode = 0; */
  pPager->exclusiveMode = 0;
#ifndef SQLITE_OMIT_WAL
  {
    u8 *a = 0;
    assert( db || pPager->pWal==0 );
    if( db && 0==(db->flags & SQLITE_NoCkptOnClose)
     && SQLITE_OK==databaseIsUnmoved(pPager)
    ){
      a = pTmp;
    }
    sqlite3WalClose(pPager->pWal, db, pPager->walSyncFlags, pPager->pageSize,a);
    pPager->pWal = 0;
  }
#endif
  pager_reset(pPager);
  if( MEMDB ){
    pager_unlock(pPager);
  }else{
    /* If it is open, sync the journal file before calling UnlockAndRollback.
    ** If this is not done, then an unsynced portion of the open journal
    ** file may be played back into the database. If a power failure occurs
    ** while this is happening, the database could become corrupt.
    **
    ** If an error occurs while trying to sync the journal, shift the pager
    ** into the ERROR state. This causes UnlockAndRollback to unlock the
    ** database and close the journal file without attempting to roll it
    ** back or finalize it. The next database user will have to do hot-journal
    ** rollback before accessing the database file.
    */
    if( isOpen(pPager->jfd) ){
      pager_error(pPager, pagerSyncHotJournal(pPager));
    }
    pagerUnlockAndRollback(pPager);
  }
  sqlite3EndBenignMalloc();
  enable_simulated_io_errors();
  PAGERTRACE(("CLOSE %d\n", PAGERID(pPager)));
  IOTRACE(("CLOSE %p\n", pPager))
  sqlite3OsClose(pPager->jfd);
  sqlite3OsClose(pPager->fd);
  sqlite3PageFree(pTmp);
  sqlite3PcacheClose(pPager->pPCache);
  assert( !pPager->aSavepoint && !pPager->pInJournal );
  assert( !isOpen(pPager->jfd) && !isOpen(pPager->sjfd) );

  sqlite3_free(pPager);
  return SQLITE_OK;
}

#if !defined(NDEBUG) || defined(SQLITE_TEST)
/*
** Return the page number for page pPg.
*/
SQLITE_PRIVATE Pgno sqlite3PagerPagenumber(DbPage *pPg){
  return pPg->pgno;
}
#endif

/*
** Increment the reference count for page pPg.
*/
SQLITE_PRIVATE void sqlite3PagerRef(DbPage *pPg){
  sqlite3PcacheRef(pPg);
}

/*
** Sync the journal. In other words, make sure all the pages that have
** been written to the journal have actually reached the surface of the
** disk and can be restored in the event of a hot-journal rollback.
**
** If the Pager.noSync flag is set, then this function is a no-op.
** Otherwise, the actions required depend on the journal-mode and the
** device characteristics of the file-system, as follows:
**
**   * If the journal file is an in-memory journal file, no action need
**     be taken.
**
**   * Otherwise, if the device does not support the SAFE_APPEND property,
**     then the nRec field of the most recently written journal header
**     is updated to contain the number of journal records that have
**     been written following it. If the pager is operating in full-sync
**     mode, then the journal file is synced before this field is updated.
**
**   * If the device does not support the SEQUENTIAL property, then
**     journal file is synced.
**
** Or, in pseudo-code:
**
**   if( NOT <in-memory journal> ){
**     if( NOT SAFE_APPEND ){
**       if( <full-sync mode> ) xSync(<journal file>);
**       <update nRec field>
**     }
**     if( NOT SEQUENTIAL ) xSync(<journal file>);
**   }
**
** If successful, this routine clears the PGHDR_NEED_SYNC flag of every
** page currently held in memory before returning SQLITE_OK. If an IO
** error is encountered, then the IO error code is returned to the caller.
*/
static int syncJournal(Pager *pPager, int newHdr){
  int rc;                         /* Return code */

  assert( pPager->eState==PAGER_WRITER_CACHEMOD
       || pPager->eState==PAGER_WRITER_DBMOD
  );
  assert( assert_pager_state(pPager) );
  assert( !pagerUseWal(pPager) );

  rc = sqlite3PagerExclusiveLock(pPager);
  if( rc!=SQLITE_OK ) return rc;

  if( !pPager->noSync ){
    assert( !pPager->tempFile );
    if( isOpen(pPager->jfd) && pPager->journalMode!=PAGER_JOURNALMODE_MEMORY ){
      const int iDc = sqlite3OsDeviceCharacteristics(pPager->fd);
      assert( isOpen(pPager->jfd) );

      if( 0==(iDc&SQLITE_IOCAP_SAFE_APPEND) ){
        /* This block deals with an obscure problem. If the last connection
        ** that wrote to this database was operating in persistent-journal
        ** mode, then the journal file may at this point actually be larger
        ** than Pager.journalOff bytes. If the next thing in the journal
        ** file happens to be a journal-header (written as part of the
        ** previous connection's transaction), and a crash or power-failure
        ** occurs after nRec is updated but before this connection writes
        ** anything else to the journal file (or commits/rolls back its
        ** transaction), then SQLite may become confused when doing the
        ** hot-journal rollback following recovery. It may roll back all
        ** of this connections data, then proceed to rolling back the old,
        ** out-of-date data that follows it. Database corruption.
        **
        ** To work around this, if the journal file does appear to contain
        ** a valid header following Pager.journalOff, then write a 0x00
        ** byte to the start of it to prevent it from being recognized.
        **
        ** Variable iNextHdrOffset is set to the offset at which this
        ** problematic header will occur, if it exists. aMagic is used
        ** as a temporary buffer to inspect the first couple of bytes of
        ** the potential journal header.
        */
        i64 iNextHdrOffset;
        u8 aMagic[8];
        u8 zHeader[sizeof(aJournalMagic)+4];

        memcpy(zHeader, aJournalMagic, sizeof(aJournalMagic));
        put32bits(&zHeader[sizeof(aJournalMagic)], pPager->nRec);

        iNextHdrOffset = journalHdrOffset(pPager);
        rc = sqlite3OsRead(pPager->jfd, aMagic, 8, iNextHdrOffset);
        if( rc==SQLITE_OK && 0==memcmp(aMagic, aJournalMagic, 8) ){
          static const u8 zerobyte = 0;
          rc = sqlite3OsWrite(pPager->jfd, &zerobyte, 1, iNextHdrOffset);
        }
        if( rc!=SQLITE_OK && rc!=SQLITE_IOERR_SHORT_READ ){
          return rc;
        }

        /* Write the nRec value into the journal file header. If in
        ** full-synchronous mode, sync the journal first. This ensures that
        ** all data has really hit the disk before nRec is updated to mark
        ** it as a candidate for rollback.
        **
        ** This is not required if the persistent media supports the
        ** SAFE_APPEND property. Because in this case it is not possible
        ** for garbage data to be appended to the file, the nRec field
        ** is populated with 0xFFFFFFFF when the journal header is written
        ** and never needs to be updated.
        */
        if( pPager->fullSync && 0==(iDc&SQLITE_IOCAP_SEQUENTIAL) ){
          PAGERTRACE(("SYNC journal of %d\n", PAGERID(pPager)));
          IOTRACE(("JSYNC %p\n", pPager))
          rc = sqlite3OsSync(pPager->jfd, pPager->syncFlags);
          if( rc!=SQLITE_OK ) return rc;
        }
        IOTRACE(("JHDR %p %lld\n", pPager, pPager->journalHdr));
        rc = sqlite3OsWrite(
            pPager->jfd, zHeader, sizeof(zHeader), pPager->journalHdr
        );
        if( rc!=SQLITE_OK ) return rc;
      }
      if( 0==(iDc&SQLITE_IOCAP_SEQUENTIAL) ){
        PAGERTRACE(("SYNC journal of %d\n", PAGERID(pPager)));
        IOTRACE(("JSYNC %p\n", pPager))
        rc = sqlite3OsSync(pPager->jfd, pPager->syncFlags|
          (pPager->syncFlags==SQLITE_SYNC_FULL?SQLITE_SYNC_DATAONLY:0)
        );
        if( rc!=SQLITE_OK ) return rc;
      }

      pPager->journalHdr = pPager->journalOff;
      if( newHdr && 0==(iDc&SQLITE_IOCAP_SAFE_APPEND) ){
        pPager->nRec = 0;
        rc = writeJournalHdr(pPager);
        if( rc!=SQLITE_OK ) return rc;
      }
    }else{
      pPager->journalHdr = pPager->journalOff;
    }
  }

  /* Unless the pager is in noSync mode, the journal file was just
  ** successfully synced. Either way, clear the PGHDR_NEED_SYNC flag on
  ** all pages.
  */
  sqlite3PcacheClearSyncFlags(pPager->pPCache);
  pPager->eState = PAGER_WRITER_DBMOD;
  assert( assert_pager_state(pPager) );
  return SQLITE_OK;
}

/*
** The argument is the first in a linked list of dirty pages connected
** by the PgHdr.pDirty pointer. This function writes each one of the
** in-memory pages in the list to the database file. The argument may
** be NULL, representing an empty list. In this case this function is
** a no-op.
**
** The pager must hold at least a RESERVED lock when this function
** is called. Before writing anything to the database file, this lock
** is upgraded to an EXCLUSIVE lock. If the lock cannot be obtained,
** SQLITE_BUSY is returned and no data is written to the database file.
**
** If the pager is a temp-file pager and the actual file-system file
** is not yet open, it is created and opened before any data is
** written out.
**
** Once the lock has been upgraded and, if necessary, the file opened,
** the pages are written out to the database file in list order. Writing
** a page is skipped if it meets either of the following criteria:
**
**   * The page number is greater than Pager.dbSize, or
**   * The PGHDR_DONT_WRITE flag is set on the page.
**
** If writing out a page causes the database file to grow, Pager.dbFileSize
** is updated accordingly. If page 1 is written out, then the value cached
** in Pager.dbFileVers[] is updated to match the new value stored in
** the database file.
**
** If everything is successful, SQLITE_OK is returned. If an IO error
** occurs, an IO error code is returned. Or, if the EXCLUSIVE lock cannot
** be obtained, SQLITE_BUSY is returned.
*/
static int pager_write_pagelist(Pager *pPager, PgHdr *pList){
  int rc = SQLITE_OK;                  /* Return code */

  /* This function is only called for rollback pagers in WRITER_DBMOD state. */
  assert( !pagerUseWal(pPager) );
  assert( pPager->tempFile || pPager->eState==PAGER_WRITER_DBMOD );
  assert( pPager->eLock==EXCLUSIVE_LOCK );
  assert( isOpen(pPager->fd) || pList->pDirty==0 );

  /* If the file is a temp-file has not yet been opened, open it now. It
  ** is not possible for rc to be other than SQLITE_OK if this branch
  ** is taken, as pager_wait_on_lock() is a no-op for temp-files.
  */
  if( !isOpen(pPager->fd) ){
    assert( pPager->tempFile && rc==SQLITE_OK );
    rc = pagerOpentemp(pPager, pPager->fd, pPager->vfsFlags);
  }

  /* Before the first write, give the VFS a hint of what the final
  ** file size will be.
  */
  assert( rc!=SQLITE_OK || isOpen(pPager->fd) );
  if( rc==SQLITE_OK
   && pPager->dbHintSize<pPager->dbSize
   && (pList->pDirty || pList->pgno>pPager->dbHintSize)
  ){
    sqlite3_int64 szFile = pPager->pageSize * (sqlite3_int64)pPager->dbSize;
    sqlite3OsFileControlHint(pPager->fd, SQLITE_FCNTL_SIZE_HINT, &szFile);
    pPager->dbHintSize = pPager->dbSize;
  }

  while( rc==SQLITE_OK && pList ){
    Pgno pgno = pList->pgno;

    /* If there are dirty pages in the page cache with page numbers greater
    ** than Pager.dbSize, this means sqlite3PagerTruncateImage() was called to
    ** make the file smaller (presumably by auto-vacuum code). Do not write
    ** any such pages to the file.
    **
    ** Also, do not write out any page that has the PGHDR_DONT_WRITE flag
    ** set (set by sqlite3PagerDontWrite()).
    */
    if( pgno<=pPager->dbSize && 0==(pList->flags&PGHDR_DONT_WRITE) ){
      i64 offset = (pgno-1)*(i64)pPager->pageSize;   /* Offset to write */
      char *pData;                                   /* Data to write */

      assert( (pList->flags&PGHDR_NEED_SYNC)==0 );
      if( pList->pgno==1 ) pager_write_changecounter(pList);

      pData = pList->pData;

      /* Write out the page data. */
      rc = sqlite3OsWrite(pPager->fd, pData, pPager->pageSize, offset);

      /* If page 1 was just written, update Pager.dbFileVers to match
      ** the value now stored in the database file. If writing this
      ** page caused the database file to grow, update dbFileSize.
      */
      if( pgno==1 ){
        memcpy(&pPager->dbFileVers, &pData[24], sizeof(pPager->dbFileVers));
      }
      if( pgno>pPager->dbFileSize ){
        pPager->dbFileSize = pgno;
      }
      pPager->aStat[PAGER_STAT_WRITE]++;

      /* Update any backup objects copying the contents of this pager. */
      sqlite3BackupUpdate(pPager->pBackup, pgno, (u8*)pList->pData);

      PAGERTRACE(("STORE %d page %d hash(%08x)\n",
                   PAGERID(pPager), pgno, pager_pagehash(pList)));
      IOTRACE(("PGOUT %p %d\n", pPager, pgno));
      PAGER_INCR(sqlite3_pager_writedb_count);
    }else{
      PAGERTRACE(("NOSTORE %d page %d\n", PAGERID(pPager), pgno));
    }
    pager_set_pagehash(pList);
    pList = pList->pDirty;
  }

  return rc;
}
