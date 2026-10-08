
/*
** If the reference count has reached zero, rollback any active
** transaction and unlock the pager.
**
** Except, in locking_mode=EXCLUSIVE when there is nothing to in
** the rollback journal, the unlock is not performed and there is
** nothing to rollback, so this routine is a no-op.
*/
static void pagerUnlockIfUnused(Pager *pPager){
  if( sqlite3PcacheRefCount(pPager->pPCache)==0 ){
    assert( pPager->nMmapOut==0 ); /* because page1 is never memory mapped */
    pagerUnlockAndRollback(pPager);
  }
}

/*
** The page getter methods each try to acquire a reference to a
** page with page number pgno. If the requested reference is
** successfully obtained, it is copied to *ppPage and SQLITE_OK returned.
**
** There are different implementations of the getter method depending
** on the current state of the pager.
**
**     getPageNormal()         --  The normal getter
**     getPageError()          --  Used if the pager is in an error state
**     getPageMmap()           --  Used if memory-mapped I/O is enabled
**
** If the requested page is already in the cache, it is returned.
** Otherwise, a new page object is allocated and populated with data
** read from the database file. In some cases, the pcache module may
** choose not to allocate a new page object and may reuse an existing
** object with no outstanding references.
**
** The extra data appended to a page is always initialized to zeros the
** first time a page is loaded into memory. If the page requested is
** already in the cache when this function is called, then the extra
** data is left as it was when the page object was last used.
**
** If the database image is smaller than the requested page or if
** the flags parameter contains the PAGER_GET_NOCONTENT bit and the
** requested page is not already stored in the cache, then no
** actual disk read occurs. In this case the memory image of the
** page is initialized to all zeros.
**
** If PAGER_GET_NOCONTENT is true, it means that we do not care about
** the contents of the page. This occurs in two scenarios:
**
**   a) When reading a free-list leaf page from the database, and
**
**   b) When a savepoint is being rolled back and we need to load
**      a new page into the cache to be filled with the data read
**      from the savepoint journal.
**
** If PAGER_GET_NOCONTENT is true, then the data returned is zeroed instead
** of being read from the database. Additionally, the bits corresponding
** to pgno in Pager.pInJournal (bitvec of pages already written to the
** journal file) and the PagerSavepoint.pInSavepoint bitvecs of any open
** savepoints are set. This means if the page is made writable at any
** point in the future, using a call to sqlite3PagerWrite(), its contents
** will not be journaled. This saves IO.
**
** The acquisition might fail for several reasons.  In all cases,
** an appropriate error code is returned and *ppPage is set to NULL.
**
** See also sqlite3PagerLookup().  Both this routine and Lookup() attempt
** to find a page in the in-memory cache first.  If the page is not already
** in memory, this routine goes to disk to read it in whereas Lookup()
** just returns 0.  This routine acquires a read-lock the first time it
** has to go to disk, and could also playback an old journal if necessary.
** Since Lookup() never goes to disk, it never has to deal with locks
** or journal files.
*/
static int getPageNormal(
  Pager *pPager,      /* The pager open on the database file */
  Pgno pgno,          /* Page number to fetch */
  DbPage **ppPage,    /* Write a pointer to the page here */
  int flags           /* PAGER_GET_XXX flags */
){
  int rc = SQLITE_OK;
  PgHdr *pPg;
  u8 noContent;                   /* True if PAGER_GET_NOCONTENT is set */
  sqlite3_pcache_page *pBase;

  assert( pPager->errCode==SQLITE_OK );
  assert( pPager->eState>=PAGER_READER );
  assert( assert_pager_state(pPager) );
  assert( pPager->hasHeldSharedLock==1 );

  if( pgno==0 ) return SQLITE_CORRUPT_BKPT;
  pBase = sqlite3PcacheFetch(pPager->pPCache, pgno, 3);
  if( pBase==0 ){
    pPg = 0;
    rc = sqlite3PcacheFetchStress(pPager->pPCache, pgno, &pBase);
    if( rc!=SQLITE_OK ) goto pager_acquire_err;
    if( pBase==0 ){
      rc = SQLITE_NOMEM_BKPT;
      goto pager_acquire_err;
    }
  }
  pPg = *ppPage = sqlite3PcacheFetchFinish(pPager->pPCache, pgno, pBase);
  assert( pPg==(*ppPage) );
  assert( pPg->pgno==pgno );
  assert( pPg->pPager==pPager || pPg->pPager==0 );

  noContent = (flags & PAGER_GET_NOCONTENT)!=0;
  if( pPg->pPager && !noContent ){
    /* In this case the pcache already contains an initialized copy of
    ** the page. Return without further ado.  */
    assert( pgno!=PAGER_SJ_PGNO(pPager) );
    pPager->aStat[PAGER_STAT_HIT]++;
    return SQLITE_OK;

  }else{
    /* The pager cache has created a new page. Its content needs to
    ** be initialized. But first some error checks:
    **
    ** (*) obsolete.  Was: maximum page number is 2^31
    ** (2) Never try to fetch the locking page
    */
    if( pgno==PAGER_SJ_PGNO(pPager) ){
      rc = SQLITE_CORRUPT_BKPT;
      goto pager_acquire_err;
    }

    pPg->pPager = pPager;

    assert( !isOpen(pPager->fd) || !MEMDB );
    if( !isOpen(pPager->fd) || pPager->dbSize<pgno || noContent ){
      if( pgno>pPager->mxPgno ){
        rc = SQLITE_FULL;
        if( pgno<=pPager->dbSize ){
          sqlite3PcacheRelease(pPg);
          pPg = 0;
        }
        goto pager_acquire_err;
      }
      if( noContent ){
        /* Failure to set the bits in the InJournal bit-vectors is benign.
        ** It merely means that we might do some extra work to journal a
        ** page that does not need to be journaled.  Nevertheless, be sure
        ** to test the case where a malloc error occurs while trying to set
        ** a bit in a bit vector.
        */
        sqlite3BeginBenignMalloc();
        if( pgno<=pPager->dbOrigSize ){
          TESTONLY( rc = ) sqlite3BitvecSet(pPager->pInJournal, pgno);
          testcase( rc==SQLITE_NOMEM );
        }
        TESTONLY( rc = ) addToSavepointBitvecs(pPager, pgno);
        testcase( rc==SQLITE_NOMEM );
        sqlite3EndBenignMalloc();
      }
      memset(pPg->pData, 0, pPager->pageSize);
      IOTRACE(("ZERO %p %d\n", pPager, pgno));
    }else{
      assert( pPg->pPager==pPager );
      pPager->aStat[PAGER_STAT_MISS]++;
      rc = readDbPage(pPg);
      if( rc!=SQLITE_OK ){
        goto pager_acquire_err;
      }
    }
    pager_set_pagehash(pPg);
  }
  return SQLITE_OK;

pager_acquire_err:
  assert( rc!=SQLITE_OK );
  if( pPg ){
    sqlite3PcacheDrop(pPg);
  }
  pagerUnlockIfUnused(pPager);
  *ppPage = 0;
  return rc;
}

#if SQLITE_MAX_MMAP_SIZE>0
/* The page getter for when memory-mapped I/O is enabled */
static int getPageMMap(
  Pager *pPager,      /* The pager open on the database file */
  Pgno pgno,          /* Page number to fetch */
  DbPage **ppPage,    /* Write a pointer to the page here */
  int flags           /* PAGER_GET_XXX flags */
){
  int rc = SQLITE_OK;
  PgHdr *pPg = 0;
  u32 iFrame = 0;                 /* Frame to read from WAL file */

  /* It is acceptable to use a read-only (mmap) page for any page except
  ** page 1 if there is no write-transaction open or the ACQUIRE_READONLY
  ** flag was specified by the caller. And so long as the db is not a
  ** temporary or in-memory database.  */
  const int bMmapOk = (pgno>1
   && (pPager->eState==PAGER_READER || (flags & PAGER_GET_READONLY))
  );

  assert( USEFETCH(pPager) );

  /* Optimization note:  Adding the "pgno<=1" term before "pgno==0" here
  ** allows the compiler optimizer to reuse the results of the "pgno>1"
  ** test in the previous statement, and avoid testing pgno==0 in the
  ** common case where pgno is large. */
  if( pgno<=1 && pgno==0 ){
    return SQLITE_CORRUPT_BKPT;
  }
  assert( pPager->eState>=PAGER_READER );
  assert( assert_pager_state(pPager) );
  assert( pPager->hasHeldSharedLock==1 );
  assert( pPager->errCode==SQLITE_OK );

  if( bMmapOk && pagerUseWal(pPager) ){
    rc = sqlite3WalFindFrame(pPager->pWal, pgno, &iFrame);
    if( rc!=SQLITE_OK ){
      *ppPage = 0;
      return rc;
    }
  }
  if( bMmapOk && iFrame==0 ){
    void *pData = 0;
    rc = sqlite3OsFetch(pPager->fd,
        (i64)(pgno-1) * pPager->pageSize, pPager->pageSize, &pData
    );
    if( rc==SQLITE_OK && pData ){
      if( pPager->eState>PAGER_READER || pPager->tempFile ){
        pPg = sqlite3PagerLookup(pPager, pgno);
      }
      if( pPg==0 ){
        rc = pagerAcquireMapPage(pPager, pgno, pData, &pPg);
      }else{
        sqlite3OsUnfetch(pPager->fd, (i64)(pgno-1)*pPager->pageSize, pData);
      }
      if( pPg ){
        assert( rc==SQLITE_OK );
        *ppPage = pPg;
        return SQLITE_OK;
      }
    }
    if( rc!=SQLITE_OK ){
      *ppPage = 0;
      return rc;
    }
  }
  return getPageNormal(pPager, pgno, ppPage, flags);
}
#endif /* SQLITE_MAX_MMAP_SIZE>0 */

/* The page getter method for when the pager is an error state */
static int getPageError(
  Pager *pPager,      /* The pager open on the database file */
  Pgno pgno,          /* Page number to fetch */
  DbPage **ppPage,    /* Write a pointer to the page here */
  int flags           /* PAGER_GET_XXX flags */
){
  UNUSED_PARAMETER(pgno);
  UNUSED_PARAMETER(flags);
  assert( pPager->errCode!=SQLITE_OK );
  *ppPage = 0;
  return pPager->errCode;
}


/* Dispatch all page fetch requests to the appropriate getter method.
*/
SQLITE_PRIVATE int sqlite3PagerGet(
  Pager *pPager,      /* The pager open on the database file */
  Pgno pgno,          /* Page number to fetch */
  DbPage **ppPage,    /* Write a pointer to the page here */
  int flags           /* PAGER_GET_XXX flags */
){
#if 0   /* Trace page fetch by setting to 1 */
  int rc;
  printf("PAGE %u\n", pgno);
  fflush(stdout);
  rc = pPager->xGet(pPager, pgno, ppPage, flags);
  if( rc ){
    printf("PAGE %u failed with 0x%02x\n", pgno, rc);
    fflush(stdout);
  }
  return rc;
#else
  /* Normal, high-speed version of sqlite3PagerGet() */
  return pPager->xGet(pPager, pgno, ppPage, flags);
#endif
}

/*
** Acquire a page if it is already in the in-memory cache.  Do
** not read the page from disk.  Return a pointer to the page,
** or 0 if the page is not in cache.
**
** See also sqlite3PagerGet().  The difference between this routine
** and sqlite3PagerGet() is that _get() will go to the disk and read
** in the page if the page is not already in cache.  This routine
** returns NULL if the page is not in cache or if a disk I/O error
** has ever happened.
*/
SQLITE_PRIVATE DbPage *sqlite3PagerLookup(Pager *pPager, Pgno pgno){
  sqlite3_pcache_page *pPage;
  assert( pPager!=0 );
  assert( pgno!=0 );
  assert( pPager->pPCache!=0 );
  pPage = sqlite3PcacheFetch(pPager->pPCache, pgno, 0);
  assert( pPage==0 || pPager->hasHeldSharedLock );
  if( pPage==0 ) return 0;
  return sqlite3PcacheFetchFinish(pPager->pPCache, pgno, pPage);
}

/*
** Release a page reference.
**
** The sqlite3PagerUnref() and sqlite3PagerUnrefNotNull() may only be used
** if we know that the page being released is not the last reference to page1.
** The btree layer always holds page1 open until the end, so these first
** two routines can be used to release any page other than BtShared.pPage1.
** The assert() at tag-20230419-2 proves that this constraint is always
** honored.
**
** Use sqlite3PagerUnrefPageOne() to release page1.  This latter routine
** checks the total number of outstanding pages and if the number of
** pages reaches zero it drops the database lock.
*/
SQLITE_PRIVATE void sqlite3PagerUnrefNotNull(DbPage *pPg){
  TESTONLY( Pager *pPager = pPg->pPager; )
  assert( pPg!=0 );
  if( pPg->flags & PGHDR_MMAP ){
    assert( pPg->pgno!=1 );  /* Page1 is never memory mapped */
    pagerReleaseMapPage(pPg);
  }else{
    sqlite3PcacheRelease(pPg);
  }
  /* Do not use this routine to release the last reference to page1 */
  assert( sqlite3PcacheRefCount(pPager->pPCache)>0 ); /* tag-20230419-2 */
}
SQLITE_PRIVATE void sqlite3PagerUnref(DbPage *pPg){
  if( pPg ) sqlite3PagerUnrefNotNull(pPg);
}
SQLITE_PRIVATE void sqlite3PagerUnrefPageOne(DbPage *pPg){
  Pager *pPager;
  assert( pPg!=0 );
  assert( pPg->pgno==1 );
  assert( (pPg->flags & PGHDR_MMAP)==0 ); /* Page1 is never memory mapped */
  pPager = pPg->pPager;
  sqlite3PcacheRelease(pPg);
  pagerUnlockIfUnused(pPager);
}

/*
** This function is called at the start of every write transaction.
** There must already be a RESERVED or EXCLUSIVE lock on the database
** file when this routine is called.
**
** Open the journal file for pager pPager and write a journal header
** to the start of it. If there are active savepoints, open the sub-journal
** as well. This function is only used when the journal file is being
** opened to write a rollback log for a transaction. It is not used
** when opening a hot journal file to roll it back.
**
** If the journal file is already open (as it may be in exclusive mode),
** then this function just writes a journal header to the start of the
** already open file.
**
** Whether or not the journal file is opened by this function, the
** Pager.pInJournal bitvec structure is allocated.
**
** Return SQLITE_OK if everything is successful. Otherwise, return
** SQLITE_NOMEM if the attempt to allocate Pager.pInJournal fails, or
** an IO error code if opening or writing the journal file fails.
*/
static int pager_open_journal(Pager *pPager){
  int rc = SQLITE_OK;                        /* Return code */
  sqlite3_vfs * const pVfs = pPager->pVfs;   /* Local cache of vfs pointer */

  assert( pPager->eState==PAGER_WRITER_LOCKED );
  assert( assert_pager_state(pPager) );
  assert( pPager->pInJournal==0 );

  /* If already in the error state, this function is a no-op.  But on
  ** the other hand, this routine is never called if we are already in
  ** an error state. */
  if( NEVER(pPager->errCode) ) return pPager->errCode;

  if( !pagerUseWal(pPager) && pPager->journalMode!=PAGER_JOURNALMODE_OFF ){
    pPager->pInJournal = sqlite3BitvecCreate(pPager->dbSize);
    if( pPager->pInJournal==0 ){
      return SQLITE_NOMEM_BKPT;
    }

    /* Open the journal file if it is not already open. */
    if( !isOpen(pPager->jfd) ){
      if( pPager->journalMode==PAGER_JOURNALMODE_MEMORY ){
        sqlite3MemJournalOpen(pPager->jfd);
      }else{
        int flags = SQLITE_OPEN_READWRITE|SQLITE_OPEN_CREATE;
        int nSpill;

        if( pPager->tempFile ){
          flags |= (SQLITE_OPEN_DELETEONCLOSE|SQLITE_OPEN_TEMP_JOURNAL);
          flags |= SQLITE_OPEN_EXCLUSIVE;
          nSpill = sqlite3Config.nStmtSpill;
        }else{
          flags |= SQLITE_OPEN_MAIN_JOURNAL;
          nSpill = jrnlBufferSize(pPager);
        }

        /* Verify that the database still has the same name as it did when
        ** it was originally opened. */
        rc = databaseIsUnmoved(pPager);
        if( rc==SQLITE_OK ){
          rc = sqlite3JournalOpen (
              pVfs, pPager->zJournal, pPager->jfd, flags, nSpill
          );
        }
      }
      assert( rc!=SQLITE_OK || isOpen(pPager->jfd) );
    }


    /* Write the first journal header to the journal file and open
    ** the sub-journal if necessary.
    */
    if( rc==SQLITE_OK ){
      /* TODO: Check if all of these are really required. */
      pPager->nRec = 0;
      pPager->journalOff = 0;
      pPager->setSuper = 0;
      pPager->journalHdr = 0;
      rc = writeJournalHdr(pPager);
    }
  }

  if( rc!=SQLITE_OK ){
    sqlite3BitvecDestroy(pPager->pInJournal);
    pPager->pInJournal = 0;
    pPager->journalOff = 0;
  }else{
    assert( pPager->eState==PAGER_WRITER_LOCKED );
    pPager->eState = PAGER_WRITER_CACHEMOD;
  }

  return rc;
}
