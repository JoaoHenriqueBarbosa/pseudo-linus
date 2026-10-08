

/*
** This function is used to change the actual size of the database
** file in the file-system. This only happens when committing a transaction,
** or rolling back a transaction (including rolling back a hot-journal).
**
** If the main database file is not open, or the pager is not in either
** DBMOD or OPEN state, this function is a no-op. Otherwise, the size
** of the file is changed to nPage pages (nPage*pPager->pageSize bytes).
** If the file on disk is currently larger than nPage pages, then use the VFS
** xTruncate() method to truncate it.
**
** Or, it might be the case that the file on disk is smaller than
** nPage pages. Some operating system implementations can get confused if
** you try to truncate a file to some size that is larger than it
** currently is, so detect this case and write a single zero byte to
** the end of the new file instead.
**
** If successful, return SQLITE_OK. If an IO error occurs while modifying
** the database file, return the error code to the caller.
*/
static int pager_truncate(Pager *pPager, Pgno nPage){
  int rc = SQLITE_OK;
  assert( pPager->eState!=PAGER_ERROR );
  assert( pPager->eState!=PAGER_READER );
  PAGERTRACE(("Truncate %d npage %u\n", PAGERID(pPager), nPage));


  if( isOpen(pPager->fd)
   && (pPager->eState>=PAGER_WRITER_DBMOD || pPager->eState==PAGER_OPEN)
  ){
    i64 currentSize, newSize;
    int szPage = pPager->pageSize;
    assert( pPager->eLock==EXCLUSIVE_LOCK );
    /* TODO: Is it safe to use Pager.dbFileSize here? */
    rc = sqlite3OsFileSize(pPager->fd, &currentSize);
    newSize = szPage*(i64)nPage;
    if( rc==SQLITE_OK && currentSize!=newSize ){
      if( currentSize>newSize ){
        rc = sqlite3OsTruncate(pPager->fd, newSize);
      }else if( (currentSize+szPage)<=newSize ){
        char *pTmp = pPager->pTmpSpace;
        memset(pTmp, 0, szPage);
        testcase( (newSize-szPage) == currentSize );
        testcase( (newSize-szPage) >  currentSize );
        sqlite3OsFileControlHint(pPager->fd, SQLITE_FCNTL_SIZE_HINT, &newSize);
        rc = sqlite3OsWrite(pPager->fd, pTmp, szPage, newSize-szPage);
      }
      if( rc==SQLITE_OK ){
        pPager->dbFileSize = nPage;
      }
    }
  }
  return rc;
}

/*
** Return a sanitized version of the sector-size of OS file pFile. The
** return value is guaranteed to lie between 32 and MAX_SECTOR_SIZE.
*/
SQLITE_PRIVATE int sqlite3SectorSize(sqlite3_file *pFile){
  int iRet = sqlite3OsSectorSize(pFile);
  if( iRet<32 ){
    iRet = 512;
  }else if( iRet>MAX_SECTOR_SIZE ){
    assert( MAX_SECTOR_SIZE>=512 );
    iRet = MAX_SECTOR_SIZE;
  }
  return iRet;
}

/*
** Set the value of the Pager.sectorSize variable for the given
** pager based on the value returned by the xSectorSize method
** of the open database file. The sector size will be used
** to determine the size and alignment of journal header and
** super-journal pointers within created journal files.
**
** For temporary files the effective sector size is always 512 bytes.
**
** Otherwise, for non-temporary files, the effective sector size is
** the value returned by the xSectorSize() method rounded up to 32 if
** it is less than 32, or rounded down to MAX_SECTOR_SIZE if it
** is greater than MAX_SECTOR_SIZE.
**
** If the file has the SQLITE_IOCAP_POWERSAFE_OVERWRITE property, then set
** the effective sector size to its minimum value (512).  The purpose of
** pPager->sectorSize is to define the "blast radius" of bytes that
** might change if a crash occurs while writing to a single byte in
** that range.  But with POWERSAFE_OVERWRITE, the blast radius is zero
** (that is what POWERSAFE_OVERWRITE means), so we minimize the sector
** size.  For backwards compatibility of the rollback journal file format,
** we cannot reduce the effective sector size below 512.
*/
static void setSectorSize(Pager *pPager){
  assert( isOpen(pPager->fd) || pPager->tempFile );

  if( pPager->tempFile
   || (sqlite3OsDeviceCharacteristics(pPager->fd) &
              SQLITE_IOCAP_POWERSAFE_OVERWRITE)!=0
  ){
    /* Sector size doesn't matter for temporary files. Also, the file
    ** may not have been opened yet, in which case the OsSectorSize()
    ** call will segfault. */
    pPager->sectorSize = 512;
  }else{
    pPager->sectorSize = sqlite3SectorSize(pPager->fd);
  }
}

/*
** Playback the journal and thus restore the database file to
** the state it was in before we started making changes.
**
** The journal file format is as follows:
**
**  (1)  8 byte prefix.  A copy of aJournalMagic[].
**  (2)  4 byte big-endian integer which is the number of valid page records
**       in the journal.  If this value is 0xffffffff, then compute the
**       number of page records from the journal size.
**  (3)  4 byte big-endian integer which is the initial value for the
**       sanity checksum.
**  (4)  4 byte integer which is the number of pages to truncate the
**       database to during a rollback.
**  (5)  4 byte big-endian integer which is the sector size.  The header
**       is this many bytes in size.
**  (6)  4 byte big-endian integer which is the page size.
**  (7)  zero padding out to the next sector size.
**  (8)  Zero or more pages instances, each as follows:
**        +  4 byte page number.
**        +  pPager->pageSize bytes of data.
**        +  4 byte checksum
**
** When we speak of the journal header, we mean the first 7 items above.
** Each entry in the journal is an instance of the 8th item.
**
** Call the value from the second bullet "nRec".  nRec is the number of
** valid page entries in the journal.  In most cases, you can compute the
** value of nRec from the size of the journal file.  But if a power
** failure occurred while the journal was being written, it could be the
** case that the size of the journal file had already been increased but
** the extra entries had not yet made it safely to disk.  In such a case,
** the value of nRec computed from the file size would be too large.  For
** that reason, we always use the nRec value in the header.
**
** If the nRec value is 0xffffffff it means that nRec should be computed
** from the file size.  This value is used when the user selects the
** no-sync option for the journal.  A power failure could lead to corruption
** in this case.  But for things like temporary table (which will be
** deleted when the power is restored) we don't care.
**
** If the file opened as the journal file is not a well-formed
** journal file then all pages up to the first corrupted page are rolled
** back (or no pages if the journal header is corrupted). The journal file
** is then deleted and SQLITE_OK returned, just as if no corruption had
** been encountered.
**
** If an I/O or malloc() error occurs, the journal-file is not deleted
** and an error code is returned.
**
** The isHot parameter indicates that we are trying to rollback a journal
** that might be a hot journal.  Or, it could be that the journal is
** preserved because of JOURNALMODE_PERSIST or JOURNALMODE_TRUNCATE.
** If the journal really is hot, reset the pager cache prior rolling
** back any content.  If the journal is merely persistent, no reset is
** needed.
*/
static int pager_playback(Pager *pPager, int isHot){
  sqlite3_vfs *pVfs = pPager->pVfs;
  i64 szJ;                 /* Size of the journal file in bytes */
  u32 nRec;                /* Number of Records in the journal */
  u32 u;                   /* Unsigned loop counter */
  Pgno mxPg = 0;           /* Size of the original file in pages */
  int rc;                  /* Result code of a subroutine */
  int res = 1;             /* Value returned by sqlite3OsAccess() */
  char *zSuper = 0;        /* Name of super-journal file if any */
  int needPagerReset;      /* True to reset page prior to first page rollback */
  int nPlayback = 0;       /* Total number of pages restored from journal */
  u32 savedPageSize = pPager->pageSize;

  /* Figure out how many records are in the journal.  Abort early if
  ** the journal is empty.
  */
  assert( isOpen(pPager->jfd) );
  rc = sqlite3OsFileSize(pPager->jfd, &szJ);
  if( rc!=SQLITE_OK ){
    goto end_playback;
  }

  /* Read the super-journal name from the journal, if it is present.
  ** If a super-journal file name is specified, but the file is not
  ** present on disk, then the journal is not hot and does not need to be
  ** played back.
  **
  ** TODO: Technically the following is an error because it assumes that
  ** buffer Pager.pTmpSpace is (mxPathname+1) bytes or larger. i.e. that
  ** (pPager->pageSize >= pPager->pVfs->mxPathname+1). Using os_unix.c,
  ** mxPathname is 512, which is the same as the minimum allowable value
  ** for pageSize.
  */
  zSuper = pPager->pTmpSpace;
  rc = readSuperJournal(pPager->jfd, zSuper, pPager->pVfs->mxPathname+1);
  if( rc==SQLITE_OK && zSuper[0] ){
    rc = sqlite3OsAccess(pVfs, zSuper, SQLITE_ACCESS_EXISTS, &res);
  }
  zSuper = 0;
  if( rc!=SQLITE_OK || !res ){
    goto end_playback;
  }
  pPager->journalOff = 0;
  needPagerReset = isHot;

  /* This loop terminates either when a readJournalHdr() or
  ** pager_playback_one_page() call returns SQLITE_DONE or an IO error
  ** occurs.
  */
  while( 1 ){
    /* Read the next journal header from the journal file.  If there are
    ** not enough bytes left in the journal file for a complete header, or
    ** it is corrupted, then a process must have failed while writing it.
    ** This indicates nothing more needs to be rolled back.
    */
    rc = readJournalHdr(pPager, isHot, szJ, &nRec, &mxPg);
    if( rc!=SQLITE_OK ){
      if( rc==SQLITE_DONE ){
        rc = SQLITE_OK;
      }
      goto end_playback;
    }

    /* If nRec is 0xffffffff, then this journal was created by a process
    ** working in no-sync mode. This means that the rest of the journal
    ** file consists of pages, there are no more journal headers. Compute
    ** the value of nRec based on this assumption.
    */
    if( nRec==0xffffffff ){
      assert( pPager->journalOff==JOURNAL_HDR_SZ(pPager) );
      nRec = (int)((szJ - JOURNAL_HDR_SZ(pPager))/JOURNAL_PG_SZ(pPager));
    }

    /* If nRec is 0 and this rollback is of a transaction created by this
    ** process and if this is the final header in the journal, then it means
    ** that this part of the journal was being filled but has not yet been
    ** synced to disk.  Compute the number of pages based on the remaining
    ** size of the file.
    **
    ** The third term of the test was added to fix ticket #2565.
    ** When rolling back a hot journal, nRec==0 always means that the next
    ** chunk of the journal contains zero pages to be rolled back.  But
    ** when doing a ROLLBACK and the nRec==0 chunk is the last chunk in
    ** the journal, it means that the journal might contain additional
    ** pages that need to be rolled back and that the number of pages
    ** should be computed based on the journal file size.
    */
    if( nRec==0 && !isHot &&
        pPager->journalHdr+JOURNAL_HDR_SZ(pPager)==pPager->journalOff ){
      nRec = (int)((szJ - pPager->journalOff) / JOURNAL_PG_SZ(pPager));
    }

    /* If this is the first header read from the journal, truncate the
    ** database file back to its original size.
    */
    if( pPager->journalOff==JOURNAL_HDR_SZ(pPager) ){
      rc = pager_truncate(pPager, mxPg);
      if( rc!=SQLITE_OK ){
        goto end_playback;
      }
      pPager->dbSize = mxPg;
      if( pPager->mxPgno<mxPg ){
        pPager->mxPgno = mxPg;
      }
    }

    /* Copy original pages out of the journal and back into the
    ** database file and/or page cache.
    */
    for(u=0; u<nRec; u++){
      if( needPagerReset ){
        pager_reset(pPager);
        needPagerReset = 0;
      }
      rc = pager_playback_one_page(pPager,&pPager->journalOff,0,1,0);
      if( rc==SQLITE_OK ){
        nPlayback++;
      }else{
        if( rc==SQLITE_DONE ){
          pPager->journalOff = szJ;
          break;
        }else if( rc==SQLITE_IOERR_SHORT_READ ){
          /* If the journal has been truncated, simply stop reading and
          ** processing the journal. This might happen if the journal was
          ** not completely written and synced prior to a crash.  In that
          ** case, the database should have never been written in the
          ** first place so it is OK to simply abandon the rollback. */
          rc = SQLITE_OK;
          goto end_playback;
        }else{
          /* If we are unable to rollback, quit and return the error
          ** code.  This will cause the pager to enter the error state
          ** so that no further harm will be done.  Perhaps the next
          ** process to come along will be able to rollback the database.
          */
          goto end_playback;
        }
      }
    }
  }
  /*NOTREACHED*/
  assert( 0 );

end_playback:
  if( rc==SQLITE_OK ){
    rc = sqlite3PagerSetPagesize(pPager, &savedPageSize, -1);
  }
  /* Following a rollback, the database file should be back in its original
  ** state prior to the start of the transaction, so invoke the
  ** SQLITE_FCNTL_DB_UNCHANGED file-control method to disable the
  ** assertion that the transaction counter was modified.
  */
#ifdef SQLITE_DEBUG
  sqlite3OsFileControlHint(pPager->fd,SQLITE_FCNTL_DB_UNCHANGED,0);
#endif

  /* If this playback is happening automatically as a result of an IO or
  ** malloc error that occurred after the change-counter was updated but
  ** before the transaction was committed, then the change-counter
  ** modification may just have been reverted. If this happens in exclusive
  ** mode, then subsequent transactions performed by the connection will not
  ** update the change-counter at all. This may lead to cache inconsistency
  ** problems for other processes at some point in the future. So, just
  ** in case this has happened, clear the changeCountDone flag now.
  */
  pPager->changeCountDone = pPager->tempFile;

  if( rc==SQLITE_OK ){
    /* Leave 4 bytes of space before the super-journal filename in memory.
    ** This is because it may end up being passed to sqlite3OsOpen(), in
    ** which case it requires 4 0x00 bytes in memory immediately before
    ** the filename. */
    zSuper = &pPager->pTmpSpace[4];
    rc = readSuperJournal(pPager->jfd, zSuper, pPager->pVfs->mxPathname+1);
    testcase( rc!=SQLITE_OK );
  }
  if( rc==SQLITE_OK
   && (pPager->eState>=PAGER_WRITER_DBMOD || pPager->eState==PAGER_OPEN)
  ){
    rc = sqlite3PagerSync(pPager, 0);
  }
  if( rc==SQLITE_OK ){
    rc = pager_end_transaction(pPager, zSuper[0]!='\0', 0);
    testcase( rc!=SQLITE_OK );
  }
  if( rc==SQLITE_OK && zSuper[0] && res ){
    /* If there was a super-journal and this routine will return success,
    ** see if it is possible to delete the super-journal.
    */
    assert( zSuper==&pPager->pTmpSpace[4] );
    memset(pPager->pTmpSpace, 0, 4);
    rc = pager_delsuper(pPager, zSuper);
    testcase( rc!=SQLITE_OK );
  }
  if( isHot && nPlayback ){
    sqlite3_log(SQLITE_NOTICE_RECOVER_ROLLBACK, "recovered %d pages from %s",
                nPlayback, pPager->zJournal);
  }

  /* The Pager.sectorSize variable may have been updated while rolling
  ** back a journal created by a process with a different sector size
  ** value. Reset it to the correct value for this process.
  */
  setSectorSize(pPager);
  return rc;
}
