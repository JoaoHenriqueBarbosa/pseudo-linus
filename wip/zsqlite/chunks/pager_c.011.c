
/*
** Ensure that the sub-journal file is open. If it is already open, this
** function is a no-op.
**
** SQLITE_OK is returned if everything goes according to plan. An
** SQLITE_IOERR_XXX error code is returned if a call to sqlite3OsOpen()
** fails.
*/
static int openSubJournal(Pager *pPager){
  int rc = SQLITE_OK;
  if( !isOpen(pPager->sjfd) ){
    const int flags =  SQLITE_OPEN_SUBJOURNAL | SQLITE_OPEN_READWRITE
      | SQLITE_OPEN_CREATE | SQLITE_OPEN_EXCLUSIVE
      | SQLITE_OPEN_DELETEONCLOSE;
    int nStmtSpill = sqlite3Config.nStmtSpill;
    if( pPager->journalMode==PAGER_JOURNALMODE_MEMORY || pPager->subjInMemory ){
      nStmtSpill = -1;
    }
    rc = sqlite3JournalOpen(pPager->pVfs, 0, pPager->sjfd, flags, nStmtSpill);
  }
  return rc;
}

/*
** Append a record of the current state of page pPg to the sub-journal.
**
** If successful, set the bit corresponding to pPg->pgno in the bitvecs
** for all open savepoints before returning.
**
** This function returns SQLITE_OK if everything is successful, an IO
** error code if the attempt to write to the sub-journal fails, or
** SQLITE_NOMEM if a malloc fails while setting a bit in a savepoint
** bitvec.
*/
static int subjournalPage(PgHdr *pPg){
  int rc = SQLITE_OK;
  Pager *pPager = pPg->pPager;
  if( pPager->journalMode!=PAGER_JOURNALMODE_OFF ){

    /* Open the sub-journal, if it has not already been opened */
    assert( pPager->useJournal );
    assert( isOpen(pPager->jfd) || pagerUseWal(pPager) );
    assert( isOpen(pPager->sjfd) || pPager->nSubRec==0 );
    assert( pagerUseWal(pPager)
         || pageInJournal(pPager, pPg)
         || pPg->pgno>pPager->dbOrigSize
    );
    rc = openSubJournal(pPager);

    /* If the sub-journal was opened successfully (or was already open),
    ** write the journal record into the file.  */
    if( rc==SQLITE_OK ){
      void *pData = pPg->pData;
      i64 offset = (i64)pPager->nSubRec*(4+pPager->pageSize);
      char *pData2;
      pData2 = pData;
      PAGERTRACE(("STMT-JOURNAL %d page %d\n", PAGERID(pPager), pPg->pgno));
      rc = write32bits(pPager->sjfd, offset, pPg->pgno);
      if( rc==SQLITE_OK ){
        rc = sqlite3OsWrite(pPager->sjfd, pData2, pPager->pageSize, offset+4);
      }
    }
  }
  if( rc==SQLITE_OK ){
    pPager->nSubRec++;
    assert( pPager->nSavepoint>0 );
    rc = addToSavepointBitvecs(pPager, pPg->pgno);
  }
  return rc;
}
static int subjournalPageIfRequired(PgHdr *pPg){
  if( subjRequiresPage(pPg) ){
    return subjournalPage(pPg);
  }else{
    return SQLITE_OK;
  }
}

/*
** This function is called by the pcache layer when it has reached some
** soft memory limit. The first argument is a pointer to a Pager object
** (cast as a void*). The pager is always 'purgeable' (not an in-memory
** database). The second argument is a reference to a page that is
** currently dirty but has no outstanding references. The page
** is always associated with the Pager object passed as the first
** argument.
**
** The job of this function is to make pPg clean by writing its contents
** out to the database file, if possible. This may involve syncing the
** journal file.
**
** If successful, sqlite3PcacheMakeClean() is called on the page and
** SQLITE_OK returned. If an IO error occurs while trying to make the
** page clean, the IO error code is returned. If the page cannot be
** made clean for some other reason, but no error occurs, then SQLITE_OK
** is returned by sqlite3PcacheMakeClean() is not called.
*/
static int pagerStress(void *p, PgHdr *pPg){
  Pager *pPager = (Pager *)p;
  int rc = SQLITE_OK;

  assert( pPg->pPager==pPager );
  assert( pPg->flags&PGHDR_DIRTY );

  /* The doNotSpill NOSYNC bit is set during times when doing a sync of
  ** journal (and adding a new header) is not allowed.  This occurs
  ** during calls to sqlite3PagerWrite() while trying to journal multiple
  ** pages belonging to the same sector.
  **
  ** The doNotSpill ROLLBACK and OFF bits inhibits all cache spilling
  ** regardless of whether or not a sync is required.  This is set during
  ** a rollback or by user request, respectively.
  **
  ** Spilling is also prohibited when in an error state since that could
  ** lead to database corruption.   In the current implementation it
  ** is impossible for sqlite3PcacheFetch() to be called with createFlag==3
  ** while in the error state, hence it is impossible for this routine to
  ** be called in the error state.  Nevertheless, we include a NEVER()
  ** test for the error state as a safeguard against future changes.
  */
  if( NEVER(pPager->errCode) ) return SQLITE_OK;
  testcase( pPager->doNotSpill & SPILLFLAG_ROLLBACK );
  testcase( pPager->doNotSpill & SPILLFLAG_OFF );
  testcase( pPager->doNotSpill & SPILLFLAG_NOSYNC );
  if( pPager->doNotSpill
   && ((pPager->doNotSpill & (SPILLFLAG_ROLLBACK|SPILLFLAG_OFF))!=0
      || (pPg->flags & PGHDR_NEED_SYNC)!=0)
  ){
    return SQLITE_OK;
  }

  pPager->aStat[PAGER_STAT_SPILL]++;
  pPg->pDirty = 0;
  if( pagerUseWal(pPager) ){
    /* Write a single frame for this page to the log. */
    rc = subjournalPageIfRequired(pPg);
    if( rc==SQLITE_OK ){
      rc = pagerWalFrames(pPager, pPg, 0, 0);
    }
  }else{

#ifdef SQLITE_ENABLE_BATCH_ATOMIC_WRITE
    if( pPager->tempFile==0 ){
      rc = sqlite3JournalCreate(pPager->jfd);
      if( rc!=SQLITE_OK ) return pager_error(pPager, rc);
    }
#endif

    /* Sync the journal file if required. */
    if( pPg->flags&PGHDR_NEED_SYNC
     || pPager->eState==PAGER_WRITER_CACHEMOD
    ){
      rc = syncJournal(pPager, 1);
    }

    /* Write the contents of the page out to the database file. */
    if( rc==SQLITE_OK ){
      assert( (pPg->flags&PGHDR_NEED_SYNC)==0 );
      rc = pager_write_pagelist(pPager, pPg);
    }
  }

  /* Mark the page as clean. */
  if( rc==SQLITE_OK ){
    PAGERTRACE(("STRESS %d page %d\n", PAGERID(pPager), pPg->pgno));
    sqlite3PcacheMakeClean(pPg);
  }

  return pager_error(pPager, rc);
}

/*
** Flush all unreferenced dirty pages to disk.
*/
SQLITE_PRIVATE int sqlite3PagerFlush(Pager *pPager){
  int rc = pPager->errCode;
  if( !MEMDB ){
    PgHdr *pList = sqlite3PcacheDirtyList(pPager->pPCache);
    assert( assert_pager_state(pPager) );
    while( rc==SQLITE_OK && pList ){
      PgHdr *pNext = pList->pDirty;
      if( pList->nRef==0 ){
        rc = pagerStress((void*)pPager, pList);
      }
      pList = pNext;
    }
  }

  return rc;
}

/*
** Allocate and initialize a new Pager object and put a pointer to it
** in *ppPager. The pager should eventually be freed by passing it
** to sqlite3PagerClose().
**
** The zFilename argument is the path to the database file to open.
** If zFilename is NULL then a randomly-named temporary file is created
** and used as the file to be cached. Temporary files are be deleted
** automatically when they are closed. If zFilename is ":memory:" then
** all information is held in cache. It is never written to disk.
** This can be used to implement an in-memory database.
**
** The nExtra parameter specifies the number of bytes of space allocated
** along with each page reference. This space is available to the user
** via the sqlite3PagerGetExtra() API.  When a new page is allocated, the
** first 8 bytes of this space are zeroed but the remainder is uninitialized.
** (The extra space is used by btree as the MemPage object.)
**
** The flags argument is used to specify properties that affect the
** operation of the pager. It should be passed some bitwise combination
** of the PAGER_* flags.
**
** The vfsFlags parameter is a bitmask to pass to the flags parameter
** of the xOpen() method of the supplied VFS when opening files.
**
** If the pager object is allocated and the specified file opened
** successfully, SQLITE_OK is returned and *ppPager set to point to
** the new pager object. If an error occurs, *ppPager is set to NULL
** and error code returned. This function may return SQLITE_NOMEM
** (sqlite3Malloc() is used to allocate memory), SQLITE_CANTOPEN or
** various SQLITE_IO_XXX errors.
*/
SQLITE_PRIVATE int sqlite3PagerOpen(
  sqlite3_vfs *pVfs,       /* The virtual file system to use */
  Pager **ppPager,         /* OUT: Return the Pager structure here */
  const char *zFilename,   /* Name of the database file to open */
  int nExtra,              /* Extra bytes append to each in-memory page */
  int flags,               /* flags controlling this file */
  int vfsFlags,            /* flags passed through to sqlite3_vfs.xOpen() */
  void (*xReinit)(DbPage*) /* Function to reinitialize pages */
){
  u8 *pPtr;
  Pager *pPager = 0;       /* Pager object to allocate and return */
  int rc = SQLITE_OK;      /* Return code */
  int tempFile = 0;        /* True for temp files (incl. in-memory files) */
  int memDb = 0;           /* True if this is an in-memory file */
  int memJM = 0;           /* Memory journal mode */
  int readOnly = 0;        /* True if this is a read-only file */
  int journalFileSize;     /* Bytes to allocate for each journal fd */
  char *zPathname = 0;     /* Full path to database file */
  int nPathname = 0;       /* Number of bytes in zPathname */
  int useJournal = (flags & PAGER_OMIT_JOURNAL)==0; /* False to omit journal */
  int pcacheSize = sqlite3PcacheSize();       /* Bytes to allocate for PCache */
  u32 szPageDflt = SQLITE_DEFAULT_PAGE_SIZE;  /* Default page size */
  const char *zUri = 0;    /* URI args to copy */
  int nUriByte = 1;        /* Number of bytes of URI args at *zUri */

  /* Figure out how much space is required for each journal file-handle
  ** (there are two of them, the main journal and the sub-journal).  */
  journalFileSize = ROUND8(sqlite3JournalSize(pVfs));

  /* Set the output variable to NULL in case an error occurs. */
  *ppPager = 0;

#ifndef SQLITE_OMIT_MEMORYDB
  if( flags & PAGER_MEMORY ){
    memDb = 1;
    if( zFilename && zFilename[0] ){
      zPathname = sqlite3DbStrDup(0, zFilename);
      if( zPathname==0  ) return SQLITE_NOMEM_BKPT;
      nPathname = sqlite3Strlen30(zPathname);
      zFilename = 0;
    }
  }
#endif

  /* Compute and store the full pathname in an allocated buffer pointed
  ** to by zPathname, length nPathname. Or, if this is a temporary file,
  ** leave both nPathname and zPathname set to 0.
  */
  if( zFilename && zFilename[0] ){
    const char *z;
    nPathname = pVfs->mxPathname+1;
    zPathname = sqlite3DbMallocRaw(0, nPathname*2);
    if( zPathname==0 ){
      return SQLITE_NOMEM_BKPT;
    }
    zPathname[0] = 0; /* Make sure initialized even if FullPathname() fails */
    rc = sqlite3OsFullPathname(pVfs, zFilename, nPathname, zPathname);
    if( rc!=SQLITE_OK ){
      if( rc==SQLITE_OK_SYMLINK ){
        if( vfsFlags & SQLITE_OPEN_NOFOLLOW ){
          rc = SQLITE_CANTOPEN_SYMLINK;
        }else{
          rc = SQLITE_OK;
        }
      }
    }
    nPathname = sqlite3Strlen30(zPathname);
    z = zUri = &zFilename[sqlite3Strlen30(zFilename)+1];
    while( *z ){
      z += strlen(z)+1;
      z += strlen(z)+1;
    }
    nUriByte = (int)(&z[1] - zUri);
    assert( nUriByte>=1 );
    if( rc==SQLITE_OK && nPathname+8>pVfs->mxPathname ){
      /* This branch is taken when the journal path required by
      ** the database being opened will be more than pVfs->mxPathname
      ** bytes in length. This means the database cannot be opened,
      ** as it will not be possible to open the journal file or even
      ** check for a hot-journal before reading.
      */
      rc = SQLITE_CANTOPEN_BKPT;
    }
    if( rc!=SQLITE_OK ){
      sqlite3DbFree(0, zPathname);
      return rc;
    }
  }

  /* Allocate memory for the Pager structure, PCache object, the
  ** three file descriptors, the database file name and the journal
  ** file name. The layout in memory is as follows:
  **
  **     Pager object                    (sizeof(Pager) bytes)
  **     PCache object                   (sqlite3PcacheSize() bytes)
  **     Database file handle            (pVfs->szOsFile bytes)
  **     Sub-journal file handle         (journalFileSize bytes)
  **     Main journal file handle        (journalFileSize bytes)
  **     Ptr back to the Pager           (sizeof(Pager*) bytes)
  **     \0\0\0\0 database prefix        (4 bytes)
  **     Database file name              (nPathname+1 bytes)
  **     URI query parameters            (nUriByte bytes)
  **     Journal filename                (nPathname+8+1 bytes)
  **     WAL filename                    (nPathname+4+1 bytes)
  **     \0\0\0 terminator               (3 bytes)
  **
  ** Some 3rd-party software, over which we have no control, depends on
  ** the specific order of the filenames and the \0 separators between them
  ** so that it can (for example) find the database filename given the WAL
  ** filename without using the sqlite3_filename_database() API.  This is a
  ** misuse of SQLite and a bug in the 3rd-party software, but the 3rd-party
  ** software is in widespread use, so we try to avoid changing the filename
  ** order and formatting if possible.  In particular, the details of the
  ** filename format expected by 3rd-party software should be as follows:
  **
  **   - Main Database Path
  **   - \0
  **   - Multiple URI components consisting of:
  **     - Key
  **     - \0
  **     - Value
  **     - \0
  **   - \0
  **   - Journal Path
  **   - \0
  **   - WAL Path (zWALName)
  **   - \0
  **
  ** The sqlite3_create_filename() interface and the databaseFilename() utility
  ** that is used by sqlite3_filename_database() and kin also depend on the
  ** specific formatting and order of the various filenames, so if the format
  ** changes here, be sure to change it there as well.
  */
  assert( SQLITE_PTRSIZE==sizeof(Pager*) );
  pPtr = (u8 *)sqlite3MallocZero(
    ROUND8(sizeof(*pPager)) +            /* Pager structure */
    ROUND8(pcacheSize) +                 /* PCache object */
    ROUND8(pVfs->szOsFile) +             /* The main db file */
    journalFileSize * 2 +                /* The two journal files */
    SQLITE_PTRSIZE +                     /* Space to hold a pointer */
    4 +                                  /* Database prefix */
    nPathname + 1 +                      /* database filename */
    nUriByte +                           /* query parameters */
    nPathname + 8 + 1 +                  /* Journal filename */
#ifndef SQLITE_OMIT_WAL
    nPathname + 4 + 1 +                  /* WAL filename */
#endif
    3                                    /* Terminator */
  );
  assert( EIGHT_BYTE_ALIGNMENT(SQLITE_INT_TO_PTR(journalFileSize)) );
  if( !pPtr ){
    sqlite3DbFree(0, zPathname);
    return SQLITE_NOMEM_BKPT;
  }
  pPager = (Pager*)pPtr;                  pPtr += ROUND8(sizeof(*pPager));
  pPager->pPCache = (PCache*)pPtr;        pPtr += ROUND8(pcacheSize);
  pPager->fd = (sqlite3_file*)pPtr;       pPtr += ROUND8(pVfs->szOsFile);
  pPager->sjfd = (sqlite3_file*)pPtr;     pPtr += journalFileSize;
  pPager->jfd =  (sqlite3_file*)pPtr;     pPtr += journalFileSize;
  assert( EIGHT_BYTE_ALIGNMENT(pPager->jfd) );
  memcpy(pPtr, &pPager, SQLITE_PTRSIZE);  pPtr += SQLITE_PTRSIZE;

  /* Fill in the Pager.zFilename and pPager.zQueryParam fields */
                                          pPtr += 4;  /* Skip zero prefix */
  pPager->zFilename = (char*)pPtr;
  if( nPathname>0 ){
    memcpy(pPtr, zPathname, nPathname);   pPtr += nPathname + 1;
    if( zUri ){
      memcpy(pPtr, zUri, nUriByte);       pPtr += nUriByte;
    }else{
                                          pPtr++;
    }
  }


  /* Fill in Pager.zJournal */
  if( nPathname>0 ){
    pPager->zJournal = (char*)pPtr;
    memcpy(pPtr, zPathname, nPathname);   pPtr += nPathname;
    memcpy(pPtr, "-journal",8);           pPtr += 8 + 1;
#ifdef SQLITE_ENABLE_8_3_NAMES
    sqlite3FileSuffix3(zFilename,pPager->zJournal);
    pPtr = (u8*)(pPager->zJournal + sqlite3Strlen30(pPager->zJournal)+1);
#endif
  }else{
    pPager->zJournal = 0;
  }

#ifndef SQLITE_OMIT_WAL
  /* Fill in Pager.zWal */
  if( nPathname>0 ){
    pPager->zWal = (char*)pPtr;
    memcpy(pPtr, zPathname, nPathname);   pPtr += nPathname;
    memcpy(pPtr, "-wal", 4);              pPtr += 4 + 1;
#ifdef SQLITE_ENABLE_8_3_NAMES
    sqlite3FileSuffix3(zFilename, pPager->zWal);
    pPtr = (u8*)(pPager->zWal + sqlite3Strlen30(pPager->zWal)+1);
#endif
  }else{
    pPager->zWal = 0;
  }
#endif
  (void)pPtr;  /* Suppress warning about unused pPtr value */

  if( nPathname ) sqlite3DbFree(0, zPathname);
  pPager->pVfs = pVfs;
  pPager->vfsFlags = vfsFlags;

  /* Open the pager file.
  */
  if( zFilename && zFilename[0] ){
    int fout = 0;                    /* VFS flags returned by xOpen() */
    rc = sqlite3OsOpen(pVfs, pPager->zFilename, pPager->fd, vfsFlags, &fout);
    assert( !memDb );
    pPager->memVfs = memJM = (fout&SQLITE_OPEN_MEMORY)!=0;
    readOnly = (fout&SQLITE_OPEN_READONLY)!=0;

    /* If the file was successfully opened for read/write access,
    ** choose a default page size in case we have to create the
    ** database file. The default page size is the maximum of:
    **
    **    + SQLITE_DEFAULT_PAGE_SIZE,
    **    + The value returned by sqlite3OsSectorSize()
    **    + The largest page size that can be written atomically.
    */
    if( rc==SQLITE_OK ){
      int iDc = sqlite3OsDeviceCharacteristics(pPager->fd);
      if( !readOnly ){
        setSectorSize(pPager);
        assert(SQLITE_DEFAULT_PAGE_SIZE<=SQLITE_MAX_DEFAULT_PAGE_SIZE);
        if( szPageDflt<pPager->sectorSize ){
          if( pPager->sectorSize>SQLITE_MAX_DEFAULT_PAGE_SIZE ){
            szPageDflt = SQLITE_MAX_DEFAULT_PAGE_SIZE;
          }else{
            szPageDflt = (u32)pPager->sectorSize;
          }
        }
#ifdef SQLITE_ENABLE_ATOMIC_WRITE
        {
          int ii;
          assert(SQLITE_IOCAP_ATOMIC512==(512>>8));
          assert(SQLITE_IOCAP_ATOMIC64K==(65536>>8));
          assert(SQLITE_MAX_DEFAULT_PAGE_SIZE<=65536);
          for(ii=szPageDflt; ii<=SQLITE_MAX_DEFAULT_PAGE_SIZE; ii=ii*2){
            if( iDc&(SQLITE_IOCAP_ATOMIC|(ii>>8)) ){
              szPageDflt = ii;
            }
          }
        }
#endif
      }
      pPager->noLock = sqlite3_uri_boolean(pPager->zFilename, "nolock", 0);
      if( (iDc & SQLITE_IOCAP_IMMUTABLE)!=0
       || sqlite3_uri_boolean(pPager->zFilename, "immutable", 0) ){
          vfsFlags |= SQLITE_OPEN_READONLY;
          goto act_like_temp_file;
      }
    }
  }else{
    /* If a temporary file is requested, it is not opened immediately.
    ** In this case we accept the default page size and delay actually
    ** opening the file until the first call to OsWrite().
    **
    ** This branch is also run for an in-memory database. An in-memory
    ** database is the same as a temp-file that is never written out to
    ** disk and uses an in-memory rollback journal.
    **
    ** This branch also runs for files marked as immutable.
    */
act_like_temp_file:
    tempFile = 1;
    pPager->eState = PAGER_READER;     /* Pretend we already have a lock */
    pPager->eLock = EXCLUSIVE_LOCK;    /* Pretend we are in EXCLUSIVE mode */
    pPager->noLock = 1;                /* Do no locking */
    readOnly = (vfsFlags&SQLITE_OPEN_READONLY);
  }

  /* The following call to PagerSetPagesize() serves to set the value of
  ** Pager.pageSize and to allocate the Pager.pTmpSpace buffer.
  */
  if( rc==SQLITE_OK ){
    assert( pPager->memDb==0 );
    rc = sqlite3PagerSetPagesize(pPager, &szPageDflt, -1);
    testcase( rc!=SQLITE_OK );
  }

  /* Initialize the PCache object. */
  if( rc==SQLITE_OK ){
    nExtra = ROUND8(nExtra);
    assert( nExtra>=8 && nExtra<1000 );
    rc = sqlite3PcacheOpen(szPageDflt, nExtra, !memDb,
                       !memDb?pagerStress:0, (void *)pPager, pPager->pPCache);
  }

  /* If an error occurred above, free the  Pager structure and close the file.
  */
  if( rc!=SQLITE_OK ){
    sqlite3OsClose(pPager->fd);
    sqlite3PageFree(pPager->pTmpSpace);
    sqlite3_free(pPager);
    return rc;
  }

  PAGERTRACE(("OPEN %d %s\n", FILEHANDLEID(pPager->fd), pPager->zFilename));
  IOTRACE(("OPEN %p %s\n", pPager, pPager->zFilename))

  pPager->useJournal = (u8)useJournal;
  /* pPager->stmtOpen = 0; */
  /* pPager->stmtInUse = 0; */
  /* pPager->nRef = 0; */
  /* pPager->stmtSize = 0; */
  /* pPager->stmtJSize = 0; */
  /* pPager->nPage = 0; */
  pPager->mxPgno = SQLITE_MAX_PAGE_COUNT;
  /* pPager->state = PAGER_UNLOCK; */
  /* pPager->errMask = 0; */
  pPager->tempFile = (u8)tempFile;
  assert( tempFile==PAGER_LOCKINGMODE_NORMAL
          || tempFile==PAGER_LOCKINGMODE_EXCLUSIVE );
  assert( PAGER_LOCKINGMODE_EXCLUSIVE==1 );
  pPager->exclusiveMode = (u8)tempFile;
  pPager->changeCountDone = pPager->tempFile;
  pPager->memDb = (u8)memDb;
  pPager->readOnly = (u8)readOnly;
  assert( useJournal || pPager->tempFile );
  sqlite3PagerSetFlags(pPager, (SQLITE_DEFAULT_SYNCHRONOUS+1)|PAGER_CACHESPILL);
  /* pPager->pFirst = 0; */
  /* pPager->pFirstSynced = 0; */
  /* pPager->pLast = 0; */
  pPager->nExtra = (u16)nExtra;
  pPager->journalSizeLimit = SQLITE_DEFAULT_JOURNAL_SIZE_LIMIT;
  assert( isOpen(pPager->fd) || tempFile );
  setSectorSize(pPager);
  if( !useJournal ){
    pPager->journalMode = PAGER_JOURNALMODE_OFF;
  }else if( memDb || memJM ){
    pPager->journalMode = PAGER_JOURNALMODE_MEMORY;
  }
  /* pPager->xBusyHandler = 0; */
  /* pPager->pBusyHandlerArg = 0; */
  pPager->xReiniter = xReinit;
  setGetterMethod(pPager);
  /* memset(pPager->aHash, 0, sizeof(pPager->aHash)); */
  /* pPager->szMmap = SQLITE_DEFAULT_MMAP_SIZE // will be set by btree.c */

  *ppPager = pPager;
  return SQLITE_OK;
}
