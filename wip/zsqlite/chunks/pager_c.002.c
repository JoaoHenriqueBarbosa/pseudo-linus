#endif

#ifndef SQLITE_OMIT_WAL
# define pagerUseWal(x) ((x)->pWal!=0)
#else
# define pagerUseWal(x) 0
# define pagerRollbackWal(x) 0
# define pagerWalFrames(v,w,x,y) 0
# define pagerOpenWalIfPresent(z) SQLITE_OK
# define pagerBeginReadTransaction(z) SQLITE_OK
#endif

#ifndef NDEBUG
/*
** Usage:
**
**   assert( assert_pager_state(pPager) );
**
** This function runs many asserts to try to find inconsistencies in
** the internal state of the Pager object.
*/
static int assert_pager_state(Pager *p){
  Pager *pPager = p;

  /* State must be valid. */
  assert( p->eState==PAGER_OPEN
       || p->eState==PAGER_READER
       || p->eState==PAGER_WRITER_LOCKED
       || p->eState==PAGER_WRITER_CACHEMOD
       || p->eState==PAGER_WRITER_DBMOD
       || p->eState==PAGER_WRITER_FINISHED
       || p->eState==PAGER_ERROR
  );

  /* Regardless of the current state, a temp-file connection always behaves
  ** as if it has an exclusive lock on the database file. It never updates
  ** the change-counter field, so the changeCountDone flag is always set.
  */
  assert( p->tempFile==0 || p->eLock==EXCLUSIVE_LOCK );
  assert( p->tempFile==0 || pPager->changeCountDone );

  /* If the useJournal flag is clear, the journal-mode must be "OFF".
  ** And if the journal-mode is "OFF", the journal file must not be open.
  */
  assert( p->journalMode==PAGER_JOURNALMODE_OFF || p->useJournal );
  assert( p->journalMode!=PAGER_JOURNALMODE_OFF || !isOpen(p->jfd) );

  /* Check that MEMDB implies noSync. And an in-memory journal. Since
  ** this means an in-memory pager performs no IO at all, it cannot encounter
  ** either SQLITE_IOERR or SQLITE_FULL during rollback or while finalizing
  ** a journal file. (although the in-memory journal implementation may
  ** return SQLITE_IOERR_NOMEM while the journal file is being written). It
  ** is therefore not possible for an in-memory pager to enter the ERROR
  ** state.
  */
  if( MEMDB ){
    assert( !isOpen(p->fd) );
    assert( p->noSync );
    assert( p->journalMode==PAGER_JOURNALMODE_OFF
         || p->journalMode==PAGER_JOURNALMODE_MEMORY
    );
    assert( p->eState!=PAGER_ERROR && p->eState!=PAGER_OPEN );
    assert( pagerUseWal(p)==0 );
  }

  /* If changeCountDone is set, a RESERVED lock or greater must be held
  ** on the file.
  */
  assert( pPager->changeCountDone==0 || pPager->eLock>=RESERVED_LOCK );
  assert( p->eLock!=PENDING_LOCK );

  switch( p->eState ){
    case PAGER_OPEN:
      assert( !MEMDB );
      assert( pPager->errCode==SQLITE_OK );
      assert( sqlite3PcacheRefCount(pPager->pPCache)==0 || pPager->tempFile );
      break;

    case PAGER_READER:
      assert( pPager->errCode==SQLITE_OK );
      assert( p->eLock!=UNKNOWN_LOCK );
      assert( p->eLock>=SHARED_LOCK );
      break;

    case PAGER_WRITER_LOCKED:
      assert( p->eLock!=UNKNOWN_LOCK );
      assert( pPager->errCode==SQLITE_OK );
      if( !pagerUseWal(pPager) ){
        assert( p->eLock>=RESERVED_LOCK );
      }
      assert( pPager->dbSize==pPager->dbOrigSize );
      assert( pPager->dbOrigSize==pPager->dbFileSize );
      assert( pPager->dbOrigSize==pPager->dbHintSize );
      assert( pPager->setSuper==0 );
      break;

    case PAGER_WRITER_CACHEMOD:
      assert( p->eLock!=UNKNOWN_LOCK );
      assert( pPager->errCode==SQLITE_OK );
      if( !pagerUseWal(pPager) ){
        /* It is possible that if journal_mode=wal here that neither the
        ** journal file nor the WAL file are open. This happens during
        ** a rollback transaction that switches from journal_mode=off
        ** to journal_mode=wal.
        */
        assert( p->eLock>=RESERVED_LOCK );
        assert( isOpen(p->jfd)
             || p->journalMode==PAGER_JOURNALMODE_OFF
             || p->journalMode==PAGER_JOURNALMODE_WAL
        );
      }
      assert( pPager->dbOrigSize==pPager->dbFileSize );
      assert( pPager->dbOrigSize==pPager->dbHintSize );
      break;

    case PAGER_WRITER_DBMOD:
      assert( p->eLock==EXCLUSIVE_LOCK );
      assert( pPager->errCode==SQLITE_OK );
      assert( !pagerUseWal(pPager) );
      assert( p->eLock>=EXCLUSIVE_LOCK );
      assert( isOpen(p->jfd)
           || p->journalMode==PAGER_JOURNALMODE_OFF
           || p->journalMode==PAGER_JOURNALMODE_WAL
           || (sqlite3OsDeviceCharacteristics(p->fd)&SQLITE_IOCAP_BATCH_ATOMIC)
      );
      assert( pPager->dbOrigSize<=pPager->dbHintSize );
      break;

    case PAGER_WRITER_FINISHED:
      assert( p->eLock==EXCLUSIVE_LOCK );
      assert( pPager->errCode==SQLITE_OK );
      assert( !pagerUseWal(pPager) );
      assert( isOpen(p->jfd)
           || p->journalMode==PAGER_JOURNALMODE_OFF
           || p->journalMode==PAGER_JOURNALMODE_WAL
           || (sqlite3OsDeviceCharacteristics(p->fd)&SQLITE_IOCAP_BATCH_ATOMIC)
      );
      break;

    case PAGER_ERROR:
      /* There must be at least one outstanding reference to the pager if
      ** in ERROR state. Otherwise the pager should have already dropped
      ** back to OPEN state.
      */
      assert( pPager->errCode!=SQLITE_OK );
      assert( sqlite3PcacheRefCount(pPager->pPCache)>0 || pPager->tempFile );
      break;
  }

  return 1;
}
#endif /* ifndef NDEBUG */

#ifdef SQLITE_DEBUG
/*
** Return a pointer to a human readable string in a static buffer
** containing the state of the Pager object passed as an argument. This
** is intended to be used within debuggers. For example, as an alternative
** to "print *pPager" in gdb:
**
** (gdb) printf "%s", print_pager_state(pPager)
**
** This routine has external linkage in order to suppress compiler warnings
** about an unused function.  It is enclosed within SQLITE_DEBUG and so does
** not appear in normal builds.
*/
char *print_pager_state(Pager *p){
  static char zRet[1024];

  sqlite3_snprintf(1024, zRet,
      "Filename:      %s\n"
      "State:         %s errCode=%d\n"
      "Lock:          %s\n"
      "Locking mode:  locking_mode=%s\n"
      "Journal mode:  journal_mode=%s\n"
      "Backing store: tempFile=%d memDb=%d useJournal=%d\n"
      "Journal:       journalOff=%lld journalHdr=%lld\n"
      "Size:          dbsize=%d dbOrigSize=%d dbFileSize=%d\n"
      , p->zFilename
      , p->eState==PAGER_OPEN            ? "OPEN" :
        p->eState==PAGER_READER          ? "READER" :
        p->eState==PAGER_WRITER_LOCKED   ? "WRITER_LOCKED" :
        p->eState==PAGER_WRITER_CACHEMOD ? "WRITER_CACHEMOD" :
        p->eState==PAGER_WRITER_DBMOD    ? "WRITER_DBMOD" :
        p->eState==PAGER_WRITER_FINISHED ? "WRITER_FINISHED" :
        p->eState==PAGER_ERROR           ? "ERROR" : "?error?"
      , (int)p->errCode
      , p->eLock==NO_LOCK         ? "NO_LOCK" :
        p->eLock==RESERVED_LOCK   ? "RESERVED" :
        p->eLock==EXCLUSIVE_LOCK  ? "EXCLUSIVE" :
        p->eLock==SHARED_LOCK     ? "SHARED" :
        p->eLock==UNKNOWN_LOCK    ? "UNKNOWN" : "?error?"
      , p->exclusiveMode ? "exclusive" : "normal"
      , p->journalMode==PAGER_JOURNALMODE_MEMORY   ? "memory" :
        p->journalMode==PAGER_JOURNALMODE_OFF      ? "off" :
        p->journalMode==PAGER_JOURNALMODE_DELETE   ? "delete" :
        p->journalMode==PAGER_JOURNALMODE_PERSIST  ? "persist" :
        p->journalMode==PAGER_JOURNALMODE_TRUNCATE ? "truncate" :
        p->journalMode==PAGER_JOURNALMODE_WAL      ? "wal" : "?error?"
      , (int)p->tempFile, (int)p->memDb, (int)p->useJournal
      , p->journalOff, p->journalHdr
      , (int)p->dbSize, (int)p->dbOrigSize, (int)p->dbFileSize
  );

  return zRet;
}
#endif

/* Forward references to the various page getters */
static int getPageNormal(Pager*,Pgno,DbPage**,int);
static int getPageError(Pager*,Pgno,DbPage**,int);
#if SQLITE_MAX_MMAP_SIZE>0
static int getPageMMap(Pager*,Pgno,DbPage**,int);
#endif

/*
** Set the Pager.xGet method for the appropriate routine used to fetch
** content from the pager.
*/
static void setGetterMethod(Pager *pPager){
  if( pPager->errCode ){
    pPager->xGet = getPageError;
#if SQLITE_MAX_MMAP_SIZE>0
  }else if( USEFETCH(pPager) ){
    pPager->xGet = getPageMMap;
#endif /* SQLITE_MAX_MMAP_SIZE>0 */
  }else{
    pPager->xGet = getPageNormal;
  }
}

/*
** Return true if it is necessary to write page *pPg into the sub-journal.
** A page needs to be written into the sub-journal if there exists one
** or more open savepoints for which:
**
**   * The page-number is less than or equal to PagerSavepoint.nOrig, and
**   * The bit corresponding to the page-number is not set in
**     PagerSavepoint.pInSavepoint.
*/
static int subjRequiresPage(PgHdr *pPg){
  Pager *pPager = pPg->pPager;
  PagerSavepoint *p;
  Pgno pgno = pPg->pgno;
  int i;
  for(i=0; i<pPager->nSavepoint; i++){
    p = &pPager->aSavepoint[i];
    if( p->nOrig>=pgno && 0==sqlite3BitvecTestNotNull(p->pInSavepoint, pgno) ){
      for(i=i+1; i<pPager->nSavepoint; i++){
        pPager->aSavepoint[i].bTruncateOnRelease = 0;
      }
      return 1;
    }
  }
  return 0;
}

#ifdef SQLITE_DEBUG
/*
** Return true if the page is already in the journal file.
*/
static int pageInJournal(Pager *pPager, PgHdr *pPg){
  return sqlite3BitvecTest(pPager->pInJournal, pPg->pgno);
}
#endif

/*
** Read a 32-bit integer from the given file descriptor.  Store the integer
** that is read in *pRes.  Return SQLITE_OK if everything worked, or an
** error code is something goes wrong.
**
** All values are stored on disk as big-endian.
*/
static int read32bits(sqlite3_file *fd, i64 offset, u32 *pRes){
  unsigned char ac[4];
  int rc = sqlite3OsRead(fd, ac, sizeof(ac), offset);
  if( rc==SQLITE_OK ){
    *pRes = sqlite3Get4byte(ac);
  }
  return rc;
}

/*
** Write a 32-bit integer into a string buffer in big-endian byte order.
*/
#define put32bits(A,B)  sqlite3Put4byte((u8*)A,B)


/*
** Write a 32-bit integer into the given file descriptor.  Return SQLITE_OK
** on success or an error code is something goes wrong.
*/
static int write32bits(sqlite3_file *fd, i64 offset, u32 val){
  char ac[4];
  put32bits(ac, val);
  return sqlite3OsWrite(fd, ac, 4, offset);
}

/*
** Unlock the database file to level eLock, which must be either NO_LOCK
** or SHARED_LOCK. Regardless of whether or not the call to xUnlock()
** succeeds, set the Pager.eLock variable to match the (attempted) new lock.
**
** Except, if Pager.eLock is set to UNKNOWN_LOCK when this function is
** called, do not modify it. See the comment above the #define of
** UNKNOWN_LOCK for an explanation of this.
*/
static int pagerUnlockDb(Pager *pPager, int eLock){
  int rc = SQLITE_OK;

  assert( !pPager->exclusiveMode || pPager->eLock==eLock );
  assert( eLock==NO_LOCK || eLock==SHARED_LOCK );
  assert( eLock!=NO_LOCK || pagerUseWal(pPager)==0 );
  if( isOpen(pPager->fd) ){
    assert( pPager->eLock>=eLock );
    rc = pPager->noLock ? SQLITE_OK : sqlite3OsUnlock(pPager->fd, eLock);
    if( pPager->eLock!=UNKNOWN_LOCK ){
      pPager->eLock = (u8)eLock;
    }
    IOTRACE(("UNLOCK %p %d\n", pPager, eLock))
  }
  pPager->changeCountDone = pPager->tempFile; /* ticket fb3b3024ea238d5c */
  return rc;
}

/*
** Lock the database file to level eLock, which must be either SHARED_LOCK,
** RESERVED_LOCK or EXCLUSIVE_LOCK. If the caller is successful, set the
** Pager.eLock variable to the new locking state.
**
** Except, if Pager.eLock is set to UNKNOWN_LOCK when this function is
** called, do not modify it unless the new locking state is EXCLUSIVE_LOCK.
** See the comment above the #define of UNKNOWN_LOCK for an explanation
** of this.
*/
static int pagerLockDb(Pager *pPager, int eLock){
  int rc = SQLITE_OK;

  assert( eLock==SHARED_LOCK || eLock==RESERVED_LOCK || eLock==EXCLUSIVE_LOCK );
  if( pPager->eLock<eLock || pPager->eLock==UNKNOWN_LOCK ){
    rc = pPager->noLock ? SQLITE_OK : sqlite3OsLock(pPager->fd, eLock);
    if( rc==SQLITE_OK && (pPager->eLock!=UNKNOWN_LOCK||eLock==EXCLUSIVE_LOCK) ){
      pPager->eLock = (u8)eLock;
      IOTRACE(("LOCK %p %d\n", pPager, eLock))
    }
  }
  return rc;
}

/*
** This function determines whether or not the atomic-write or
** atomic-batch-write optimizations can be used with this pager. The
** atomic-write optimization can be used if:
**
**  (a) the value returned by OsDeviceCharacteristics() indicates that
**      a database page may be written atomically, and
**  (b) the value returned by OsSectorSize() is less than or equal
**      to the page size.
**
** If it can be used, then the value returned is the size of the journal
** file when it contains rollback data for exactly one page.
**
** The atomic-batch-write optimization can be used if OsDeviceCharacteristics()
** returns a value with the SQLITE_IOCAP_BATCH_ATOMIC bit set. -1 is
** returned in this case.
**
** If neither optimization can be used, 0 is returned.
*/
static int jrnlBufferSize(Pager *pPager){
  assert( !MEMDB );

#if defined(SQLITE_ENABLE_ATOMIC_WRITE) \
 || defined(SQLITE_ENABLE_BATCH_ATOMIC_WRITE)
  int dc;                           /* Device characteristics */

  assert( isOpen(pPager->fd) );
  dc = sqlite3OsDeviceCharacteristics(pPager->fd);
#else
  UNUSED_PARAMETER(pPager);
#endif

#ifdef SQLITE_ENABLE_BATCH_ATOMIC_WRITE
  if( pPager->dbSize>0 && (dc&SQLITE_IOCAP_BATCH_ATOMIC) ){
    return -1;
  }
#endif

#ifdef SQLITE_ENABLE_ATOMIC_WRITE
  {
    int nSector = pPager->sectorSize;
    int szPage = pPager->pageSize;

    assert(SQLITE_IOCAP_ATOMIC512==(512>>8));
    assert(SQLITE_IOCAP_ATOMIC64K==(65536>>8));
    if( 0==(dc&(SQLITE_IOCAP_ATOMIC|(szPage>>8)) || nSector>szPage) ){
      return 0;
    }
  }

  return JOURNAL_HDR_SZ(pPager) + JOURNAL_PG_SZ(pPager);
#endif

  return 0;
}
