
/* Forward reference */
static int pager_playback(Pager *pPager, int isHot);

/*
** Execute a rollback if a transaction is active and unlock the
** database file.
**
** If the pager has already entered the ERROR state, do not attempt
** the rollback at this time. Instead, pager_unlock() is called. The
** call to pager_unlock() will discard all in-memory pages, unlock
** the database file and move the pager back to OPEN state. If this
** means that there is a hot-journal left in the file-system, the next
** connection to obtain a shared lock on the pager (which may be this one)
** will roll it back.
**
** If the pager has not already entered the ERROR state, but an IO or
** malloc error occurs during a rollback, then this will itself cause
** the pager to enter the ERROR state. Which will be cleared by the
** call to pager_unlock(), as described above.
*/
static void pagerUnlockAndRollback(Pager *pPager){
  if( pPager->eState!=PAGER_ERROR && pPager->eState!=PAGER_OPEN ){
    assert( assert_pager_state(pPager) );
    if( pPager->eState>=PAGER_WRITER_LOCKED ){
      sqlite3BeginBenignMalloc();
      sqlite3PagerRollback(pPager);
      sqlite3EndBenignMalloc();
    }else if( !pPager->exclusiveMode ){
      assert( pPager->eState==PAGER_READER );
      pager_end_transaction(pPager, 0, 0);
    }
  }else if( pPager->eState==PAGER_ERROR
         && pPager->journalMode==PAGER_JOURNALMODE_MEMORY
         && isOpen(pPager->jfd)
  ){
    /* Special case for a ROLLBACK due to I/O error with an in-memory
    ** journal:  We have to rollback immediately, before the journal is
    ** closed, because once it is closed, all content is forgotten. */
    int errCode = pPager->errCode;
    u8 eLock = pPager->eLock;
    pPager->eState = PAGER_OPEN;
    pPager->errCode = SQLITE_OK;
    pPager->eLock = EXCLUSIVE_LOCK;
    pager_playback(pPager, 1);
    pPager->errCode = errCode;
    pPager->eLock = eLock;
  }
  pager_unlock(pPager);
}

/*
** Parameter aData must point to a buffer of pPager->pageSize bytes
** of data. Compute and return a checksum based on the contents of the
** page of data and the current value of pPager->cksumInit.
**
** This is not a real checksum. It is really just the sum of the
** random initial value (pPager->cksumInit) and every 200th byte
** of the page data, starting with byte offset (pPager->pageSize%200).
** Each byte is interpreted as an 8-bit unsigned integer.
**
** Changing the formula used to compute this checksum results in an
** incompatible journal file format.
**
** If journal corruption occurs due to a power failure, the most likely
** scenario is that one end or the other of the record will be changed.
** It is much less likely that the two ends of the journal record will be
** correct and the middle be corrupt.  Thus, this "checksum" scheme,
** though fast and simple, catches the mostly likely kind of corruption.
*/
static u32 pager_cksum(Pager *pPager, const u8 *aData){
  u32 cksum = pPager->cksumInit;         /* Checksum value to return */
  int i = pPager->pageSize-200;          /* Loop counter */
  while( i>0 ){
    cksum += aData[i];
    i -= 200;
  }
  return cksum;
}

/*
** Read a single page from either the journal file (if isMainJrnl==1) or
** from the sub-journal (if isMainJrnl==0) and playback that page.
** The page begins at offset *pOffset into the file. The *pOffset
** value is increased to the start of the next page in the journal.
**
** The main rollback journal uses checksums - the statement journal does
** not.
**
** If the page number of the page record read from the (sub-)journal file
** is greater than the current value of Pager.dbSize, then playback is
** skipped and SQLITE_OK is returned.
**
** If pDone is not NULL, then it is a record of pages that have already
** been played back.  If the page at *pOffset has already been played back
** (if the corresponding pDone bit is set) then skip the playback.
** Make sure the pDone bit corresponding to the *pOffset page is set
** prior to returning.
**
** If the page record is successfully read from the (sub-)journal file
** and played back, then SQLITE_OK is returned. If an IO error occurs
** while reading the record from the (sub-)journal file or while writing
** to the database file, then the IO error code is returned. If data
** is successfully read from the (sub-)journal file but appears to be
** corrupted, SQLITE_DONE is returned. Data is considered corrupted in
** two circumstances:
**
**   * If the record page-number is illegal (0 or PAGER_SJ_PGNO), or
**   * If the record is being rolled back from the main journal file
**     and the checksum field does not match the record content.
**
** Neither of these two scenarios are possible during a savepoint rollback.
**
** If this is a savepoint rollback, then memory may have to be dynamically
** allocated by this function. If this is the case and an allocation fails,
** SQLITE_NOMEM is returned.
*/
static int pager_playback_one_page(
  Pager *pPager,                /* The pager being played back */
  i64 *pOffset,                 /* Offset of record to playback */
  Bitvec *pDone,                /* Bitvec of pages already played back */
  int isMainJrnl,               /* 1 -> main journal. 0 -> sub-journal. */
  int isSavepnt                 /* True for a savepoint rollback */
){
  int rc;
  PgHdr *pPg;                   /* An existing page in the cache */
  Pgno pgno;                    /* The page number of a page in journal */
  u32 cksum;                    /* Checksum used for sanity checking */
  char *aData;                  /* Temporary storage for the page */
  sqlite3_file *jfd;            /* The file descriptor for the journal file */
  int isSynced;                 /* True if journal page is synced */

  assert( (isMainJrnl&~1)==0 );      /* isMainJrnl is 0 or 1 */
  assert( (isSavepnt&~1)==0 );       /* isSavepnt is 0 or 1 */
  assert( isMainJrnl || pDone );     /* pDone always used on sub-journals */
  assert( isSavepnt || pDone==0 );   /* pDone never used on non-savepoint */

  aData = pPager->pTmpSpace;
  assert( aData );         /* Temp storage must have already been allocated */
  assert( pagerUseWal(pPager)==0 || (!isMainJrnl && isSavepnt) );

  /* Either the state is greater than PAGER_WRITER_CACHEMOD (a transaction
  ** or savepoint rollback done at the request of the caller) or this is
  ** a hot-journal rollback. If it is a hot-journal rollback, the pager
  ** is in state OPEN and holds an EXCLUSIVE lock. Hot-journal rollback
  ** only reads from the main journal, not the sub-journal.
  */
  assert( pPager->eState>=PAGER_WRITER_CACHEMOD
       || (pPager->eState==PAGER_OPEN && pPager->eLock==EXCLUSIVE_LOCK)
  );
  assert( pPager->eState>=PAGER_WRITER_CACHEMOD || isMainJrnl );

  /* Read the page number and page data from the journal or sub-journal
  ** file. Return an error code to the caller if an IO error occurs.
  */
  jfd = isMainJrnl ? pPager->jfd : pPager->sjfd;
  rc = read32bits(jfd, *pOffset, &pgno);
  if( rc!=SQLITE_OK ) return rc;
  rc = sqlite3OsRead(jfd, (u8*)aData, pPager->pageSize, (*pOffset)+4);
  if( rc!=SQLITE_OK ) return rc;
  *pOffset += pPager->pageSize + 4 + isMainJrnl*4;

  /* Sanity checking on the page.  This is more important that I originally
  ** thought.  If a power failure occurs while the journal is being written,
  ** it could cause invalid data to be written into the journal.  We need to
  ** detect this invalid data (with high probability) and ignore it.
  */
  if( pgno==0 || pgno==PAGER_SJ_PGNO(pPager) ){
    assert( !isSavepnt );
    return SQLITE_DONE;
  }
  if( pgno>(Pgno)pPager->dbSize || sqlite3BitvecTest(pDone, pgno) ){
    return SQLITE_OK;
  }
  if( isMainJrnl ){
    rc = read32bits(jfd, (*pOffset)-4, &cksum);
    if( rc ) return rc;
    if( !isSavepnt && pager_cksum(pPager, (u8*)aData)!=cksum ){
      return SQLITE_DONE;
    }
  }

  /* If this page has already been played back before during the current
  ** rollback, then don't bother to play it back again.
  */
  if( pDone && (rc = sqlite3BitvecSet(pDone, pgno))!=SQLITE_OK ){
    return rc;
  }

  /* When playing back page 1, restore the nReserve setting
  */
  if( pgno==1 && pPager->nReserve!=((u8*)aData)[20] ){
    pPager->nReserve = ((u8*)aData)[20];
  }

  /* If the pager is in CACHEMOD state, then there must be a copy of this
  ** page in the pager cache. In this case just update the pager cache,
  ** not the database file. The page is left marked dirty in this case.
  **
  ** An exception to the above rule: If the database is in no-sync mode
  ** and a page is moved during an incremental vacuum then the page may
  ** not be in the pager cache. Later: if a malloc() or IO error occurs
  ** during a Movepage() call, then the page may not be in the cache
  ** either. So the condition described in the above paragraph is not
  ** assert()able.
  **
  ** If in WRITER_DBMOD, WRITER_FINISHED or OPEN state, then we update the
  ** pager cache if it exists and the main file. The page is then marked
  ** not dirty. Since this code is only executed in PAGER_OPEN state for
  ** a hot-journal rollback, it is guaranteed that the page-cache is empty
  ** if the pager is in OPEN state.
  **
  ** Ticket #1171:  The statement journal might contain page content that is
  ** different from the page content at the start of the transaction.
  ** This occurs when a page is changed prior to the start of a statement
  ** then changed again within the statement.  When rolling back such a
  ** statement we must not write to the original database unless we know
  ** for certain that original page contents are synced into the main rollback
  ** journal.  Otherwise, a power loss might leave modified data in the
  ** database file without an entry in the rollback journal that can
  ** restore the database to its original form.  Two conditions must be
  ** met before writing to the database files. (1) the database must be
  ** locked.  (2) we know that the original page content is fully synced
  ** in the main journal either because the page is not in cache or else
  ** the page is marked as needSync==0.
  **
  ** 2008-04-14:  When attempting to vacuum a corrupt database file, it
  ** is possible to fail a statement on a database that does not yet exist.
  ** Do not attempt to write if database file has never been opened.
  */
  if( pagerUseWal(pPager) ){
    pPg = 0;
  }else{
    pPg = sqlite3PagerLookup(pPager, pgno);
  }
  assert( pPg || !MEMDB );
  assert( pPager->eState!=PAGER_OPEN || pPg==0 || pPager->tempFile );
  PAGERTRACE(("PLAYBACK %d page %d hash(%08x) %s\n",
           PAGERID(pPager), pgno, pager_datahash(pPager->pageSize, (u8*)aData),
           (isMainJrnl?"main-journal":"sub-journal")
  ));
  if( isMainJrnl ){
    isSynced = pPager->noSync || (*pOffset <= pPager->journalHdr);
  }else{
    isSynced = (pPg==0 || 0==(pPg->flags & PGHDR_NEED_SYNC));
  }
  if( isOpen(pPager->fd)
   && (pPager->eState>=PAGER_WRITER_DBMOD || pPager->eState==PAGER_OPEN)
   && isSynced
  ){
    i64 ofst = (pgno-1)*(i64)pPager->pageSize;
    testcase( !isSavepnt && pPg!=0 && (pPg->flags&PGHDR_NEED_SYNC)!=0 );
    assert( !pagerUseWal(pPager) );

    /* Write the data read from the journal back into the database file.
    ** This is usually safe even for an encrypted database - as the data
    ** was encrypted before it was written to the journal file. The exception
    ** is if the data was just read from an in-memory sub-journal. In that
    ** case it must be encrypted here before it is copied into the database
    ** file.  */
    rc = sqlite3OsWrite(pPager->fd, (u8 *)aData, pPager->pageSize, ofst);

    if( pgno>pPager->dbFileSize ){
      pPager->dbFileSize = pgno;
    }
    if( pPager->pBackup ){
      sqlite3BackupUpdate(pPager->pBackup, pgno, (u8*)aData);
    }
  }else if( !isMainJrnl && pPg==0 ){
    /* If this is a rollback of a savepoint and data was not written to
    ** the database and the page is not in-memory, there is a potential
    ** problem. When the page is next fetched by the b-tree layer, it
    ** will be read from the database file, which may or may not be
    ** current.
    **
    ** There are a couple of different ways this can happen. All are quite
    ** obscure. When running in synchronous mode, this can only happen
    ** if the page is on the free-list at the start of the transaction, then
    ** populated, then moved using sqlite3PagerMovepage().
    **
    ** The solution is to add an in-memory page to the cache containing
    ** the data just read from the sub-journal. Mark the page as dirty
    ** and if the pager requires a journal-sync, then mark the page as
    ** requiring a journal-sync before it is written.
    */
    assert( isSavepnt );
    assert( (pPager->doNotSpill & SPILLFLAG_ROLLBACK)==0 );
    pPager->doNotSpill |= SPILLFLAG_ROLLBACK;
    rc = sqlite3PagerGet(pPager, pgno, &pPg, 1);
    assert( (pPager->doNotSpill & SPILLFLAG_ROLLBACK)!=0 );
    pPager->doNotSpill &= ~SPILLFLAG_ROLLBACK;
    if( rc!=SQLITE_OK ) return rc;
    sqlite3PcacheMakeDirty(pPg);
  }
  if( pPg ){
    /* No page should ever be explicitly rolled back that is in use, except
    ** for page 1 which is held in use in order to keep the lock on the
    ** database active. However such a page may be rolled back as a result
    ** of an internal error resulting in an automatic call to
    ** sqlite3PagerRollback().
    */
    void *pData;
    pData = pPg->pData;
    memcpy(pData, (u8*)aData, pPager->pageSize);
    pPager->xReiniter(pPg);
    /* It used to be that sqlite3PcacheMakeClean(pPg) was called here.  But
    ** that call was dangerous and had no detectable benefit since the cache
    ** is normally cleaned by sqlite3PcacheCleanAll() after rollback and so
    ** has been removed. */
    pager_set_pagehash(pPg);

    /* If this was page 1, then restore the value of Pager.dbFileVers.
    ** Do this before any decoding. */
    if( pgno==1 ){
      memcpy(&pPager->dbFileVers, &((u8*)pData)[24],sizeof(pPager->dbFileVers));
    }
    sqlite3PcacheRelease(pPg);
  }
  return rc;
}

/*
** Parameter zSuper is the name of a super-journal file. A single journal
** file that referred to the super-journal file has just been rolled back.
** This routine checks if it is possible to delete the super-journal file,
** and does so if it is.
**
** Argument zSuper may point to Pager.pTmpSpace. So that buffer is not
** available for use within this function.
**
** When a super-journal file is created, it is populated with the names
** of all of its child journals, one after another, formatted as utf-8
** encoded text. The end of each child journal file is marked with a
** nul-terminator byte (0x00). i.e. the entire contents of a super-journal
** file for a transaction involving two databases might be:
**
**   "/home/bill/a.db-journal\x00/home/bill/b.db-journal\x00"
**
** A super-journal file may only be deleted once all of its child
** journals have been rolled back.
**
** This function reads the contents of the super-journal file into
** memory and loops through each of the child journal names. For
** each child journal, it checks if:
**
**   * if the child journal exists, and if so
**   * if the child journal contains a reference to super-journal
**     file zSuper
**
** If a child journal can be found that matches both of the criteria
** above, this function returns without doing anything. Otherwise, if
** no such child journal can be found, file zSuper is deleted from
** the file-system using sqlite3OsDelete().
**
** If an IO error within this function, an error code is returned. This
** function allocates memory by calling sqlite3Malloc(). If an allocation
** fails, SQLITE_NOMEM is returned. Otherwise, if no IO or malloc errors
** occur, SQLITE_OK is returned.
**
** TODO: This function allocates a single block of memory to load
** the entire contents of the super-journal file. This could be
** a couple of kilobytes or so - potentially larger than the page
** size.
*/
static int pager_delsuper(Pager *pPager, const char *zSuper){
  sqlite3_vfs *pVfs = pPager->pVfs;
  int rc;                   /* Return code */
  sqlite3_file *pSuper;     /* Malloc'd super-journal file descriptor */
  sqlite3_file *pJournal;   /* Malloc'd child-journal file descriptor */
  char *zSuperJournal = 0;  /* Contents of super-journal file */
  i64 nSuperJournal;        /* Size of super-journal file */
  char *zJournal;           /* Pointer to one journal within MJ file */
  char *zSuperPtr;          /* Space to hold super-journal filename */
  char *zFree = 0;          /* Free this buffer */
  int nSuperPtr;            /* Amount of space allocated to zSuperPtr[] */

  /* Allocate space for both the pJournal and pSuper file descriptors.
  ** If successful, open the super-journal file for reading.
  */
  pSuper = (sqlite3_file *)sqlite3MallocZero(pVfs->szOsFile * 2);
  if( !pSuper ){
    rc = SQLITE_NOMEM_BKPT;
    pJournal = 0;
  }else{
    const int flags = (SQLITE_OPEN_READONLY|SQLITE_OPEN_SUPER_JOURNAL);
    rc = sqlite3OsOpen(pVfs, zSuper, pSuper, flags, 0);
    pJournal = (sqlite3_file *)(((u8 *)pSuper) + pVfs->szOsFile);
  }
  if( rc!=SQLITE_OK ) goto delsuper_out;

  /* Load the entire super-journal file into space obtained from
  ** sqlite3_malloc() and pointed to by zSuperJournal.   Also obtain
  ** sufficient space (in zSuperPtr) to hold the names of super-journal
  ** files extracted from regular rollback-journals.
  */
  rc = sqlite3OsFileSize(pSuper, &nSuperJournal);
  if( rc!=SQLITE_OK ) goto delsuper_out;
  nSuperPtr = pVfs->mxPathname+1;
  zFree = sqlite3Malloc(4 + nSuperJournal + nSuperPtr + 2);
  if( !zFree ){
    rc = SQLITE_NOMEM_BKPT;
    goto delsuper_out;
  }
  zFree[0] = zFree[1] = zFree[2] = zFree[3] = 0;
  zSuperJournal = &zFree[4];
  zSuperPtr = &zSuperJournal[nSuperJournal+2];
  rc = sqlite3OsRead(pSuper, zSuperJournal, (int)nSuperJournal, 0);
  if( rc!=SQLITE_OK ) goto delsuper_out;
  zSuperJournal[nSuperJournal] = 0;
  zSuperJournal[nSuperJournal+1] = 0;

  zJournal = zSuperJournal;
  while( (zJournal-zSuperJournal)<nSuperJournal ){
    int exists;
    rc = sqlite3OsAccess(pVfs, zJournal, SQLITE_ACCESS_EXISTS, &exists);
    if( rc!=SQLITE_OK ){
      goto delsuper_out;
    }
    if( exists ){
      /* One of the journals pointed to by the super-journal exists.
      ** Open it and check if it points at the super-journal. If
      ** so, return without deleting the super-journal file.
      ** NB:  zJournal is really a MAIN_JOURNAL.  But call it a
      ** SUPER_JOURNAL here so that the VFS will not send the zJournal
      ** name into sqlite3_database_file_object().
      */
      int c;
      int flags = (SQLITE_OPEN_READONLY|SQLITE_OPEN_SUPER_JOURNAL);
      rc = sqlite3OsOpen(pVfs, zJournal, pJournal, flags, 0);
      if( rc!=SQLITE_OK ){
        goto delsuper_out;
      }

      rc = readSuperJournal(pJournal, zSuperPtr, nSuperPtr);
      sqlite3OsClose(pJournal);
      if( rc!=SQLITE_OK ){
        goto delsuper_out;
      }

      c = zSuperPtr[0]!=0 && strcmp(zSuperPtr, zSuper)==0;
      if( c ){
        /* We have a match. Do not delete the super-journal file. */
        goto delsuper_out;
      }
    }
    zJournal += (sqlite3Strlen30(zJournal)+1);
  }

  sqlite3OsClose(pSuper);
  rc = sqlite3OsDelete(pVfs, zSuper, 0);

delsuper_out:
  sqlite3_free(zFree);
  if( pSuper ){
    sqlite3OsClose(pSuper);
    assert( !isOpen(pJournal) );
    sqlite3_free(pSuper);
  }
  return rc;
}
