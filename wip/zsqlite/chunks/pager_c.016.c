

/*
** When this function is called, the database file has been completely
** updated to reflect the changes made by the current transaction and
** synced to disk. The journal file still exists in the file-system
** though, and if a failure occurs at this point it will eventually
** be used as a hot-journal and the current transaction rolled back.
**
** This function finalizes the journal file, either by deleting,
** truncating or partially zeroing it, so that it cannot be used
** for hot-journal rollback. Once this is done the transaction is
** irrevocably committed.
**
** If an error occurs, an IO error code is returned and the pager
** moves into the error state. Otherwise, SQLITE_OK is returned.
*/
SQLITE_PRIVATE int sqlite3PagerCommitPhaseTwo(Pager *pPager){
  int rc = SQLITE_OK;                  /* Return code */

  /* This routine should not be called if a prior error has occurred.
  ** But if (due to a coding error elsewhere in the system) it does get
  ** called, just return the same error code without doing anything. */
  if( NEVER(pPager->errCode) ) return pPager->errCode;
  pPager->iDataVersion++;

  assert( pPager->eState==PAGER_WRITER_LOCKED
       || pPager->eState==PAGER_WRITER_FINISHED
       || (pagerUseWal(pPager) && pPager->eState==PAGER_WRITER_CACHEMOD)
  );
  assert( assert_pager_state(pPager) );

  /* An optimization. If the database was not actually modified during
  ** this transaction, the pager is running in exclusive-mode and is
  ** using persistent journals, then this function is a no-op.
  **
  ** The start of the journal file currently contains a single journal
  ** header with the nRec field set to 0. If such a journal is used as
  ** a hot-journal during hot-journal rollback, 0 changes will be made
  ** to the database file. So there is no need to zero the journal
  ** header. Since the pager is in exclusive mode, there is no need
  ** to drop any locks either.
  */
  if( pPager->eState==PAGER_WRITER_LOCKED
   && pPager->exclusiveMode
   && pPager->journalMode==PAGER_JOURNALMODE_PERSIST
  ){
    assert( pPager->journalOff==JOURNAL_HDR_SZ(pPager) || !pPager->journalOff );
    pPager->eState = PAGER_READER;
    return SQLITE_OK;
  }

  PAGERTRACE(("COMMIT %d\n", PAGERID(pPager)));
  rc = pager_end_transaction(pPager, pPager->setSuper, 1);
  return pager_error(pPager, rc);
}

/*
** If a write transaction is open, then all changes made within the
** transaction are reverted and the current write-transaction is closed.
** The pager falls back to PAGER_READER state if successful, or PAGER_ERROR
** state if an error occurs.
**
** If the pager is already in PAGER_ERROR state when this function is called,
** it returns Pager.errCode immediately. No work is performed in this case.
**
** Otherwise, in rollback mode, this function performs two functions:
**
**   1) It rolls back the journal file, restoring all database file and
**      in-memory cache pages to the state they were in when the transaction
**      was opened, and
**
**   2) It finalizes the journal file, so that it is not used for hot
**      rollback at any point in the future.
**
** Finalization of the journal file (task 2) is only performed if the
** rollback is successful.
**
** In WAL mode, all cache-entries containing data modified within the
** current transaction are either expelled from the cache or reverted to
** their pre-transaction state by re-reading data from the database or
** WAL files. The WAL transaction is then closed.
*/
SQLITE_PRIVATE int sqlite3PagerRollback(Pager *pPager){
  int rc = SQLITE_OK;                  /* Return code */
  PAGERTRACE(("ROLLBACK %d\n", PAGERID(pPager)));

  /* PagerRollback() is a no-op if called in READER or OPEN state. If
  ** the pager is already in the ERROR state, the rollback is not
  ** attempted here. Instead, the error code is returned to the caller.
  */
  assert( assert_pager_state(pPager) );
  if( pPager->eState==PAGER_ERROR ) return pPager->errCode;
  if( pPager->eState<=PAGER_READER ) return SQLITE_OK;

  if( pagerUseWal(pPager) ){
    int rc2;
    rc = sqlite3PagerSavepoint(pPager, SAVEPOINT_ROLLBACK, -1);
    rc2 = pager_end_transaction(pPager, pPager->setSuper, 0);
    if( rc==SQLITE_OK ) rc = rc2;
  }else if( !isOpen(pPager->jfd) || pPager->eState==PAGER_WRITER_LOCKED ){
    int eState = pPager->eState;
    rc = pager_end_transaction(pPager, 0, 0);
    if( !MEMDB && eState>PAGER_WRITER_LOCKED ){
      /* This can happen using journal_mode=off. Move the pager to the error
      ** state to indicate that the contents of the cache may not be trusted.
      ** Any active readers will get SQLITE_ABORT.
      */
      pPager->errCode = SQLITE_ABORT;
      pPager->eState = PAGER_ERROR;
      setGetterMethod(pPager);
      return rc;
    }
  }else{
    rc = pager_playback(pPager, 0);
  }

  assert( pPager->eState==PAGER_READER || rc!=SQLITE_OK );
  assert( rc==SQLITE_OK || rc==SQLITE_FULL || rc==SQLITE_CORRUPT
          || rc==SQLITE_NOMEM || (rc&0xFF)==SQLITE_IOERR
          || rc==SQLITE_CANTOPEN
  );

  /* If an error occurs during a ROLLBACK, we can no longer trust the pager
  ** cache. So call pager_error() on the way out to make any error persistent.
  */
  return pager_error(pPager, rc);
}

/*
** Return TRUE if the database file is opened read-only.  Return FALSE
** if the database is (in theory) writable.
*/
SQLITE_PRIVATE u8 sqlite3PagerIsreadonly(Pager *pPager){
  return pPager->readOnly;
}

#ifdef SQLITE_DEBUG
/*
** Return the sum of the reference counts for all pages held by pPager.
*/
SQLITE_PRIVATE int sqlite3PagerRefcount(Pager *pPager){
  return sqlite3PcacheRefCount(pPager->pPCache);
}
#endif

/*
** Return the approximate number of bytes of memory currently
** used by the pager and its associated cache.
*/
SQLITE_PRIVATE int sqlite3PagerMemUsed(Pager *pPager){
  int perPageSize = pPager->pageSize + pPager->nExtra
    + (int)(sizeof(PgHdr) + 5*sizeof(void*));
  return perPageSize*sqlite3PcachePagecount(pPager->pPCache)
           + sqlite3MallocSize(pPager)
           + pPager->pageSize;
}

/*
** Return the number of references to the specified page.
*/
SQLITE_PRIVATE int sqlite3PagerPageRefcount(DbPage *pPage){
  return sqlite3PcachePageRefcount(pPage);
}

#ifdef SQLITE_TEST
/*
** This routine is used for testing and analysis only.
*/
SQLITE_PRIVATE int *sqlite3PagerStats(Pager *pPager){
  static int a[11];
  a[0] = sqlite3PcacheRefCount(pPager->pPCache);
  a[1] = sqlite3PcachePagecount(pPager->pPCache);
  a[2] = sqlite3PcacheGetCachesize(pPager->pPCache);
  a[3] = pPager->eState==PAGER_OPEN ? -1 : (int) pPager->dbSize;
  a[4] = pPager->eState;
  a[5] = pPager->errCode;
  a[6] = (int)pPager->aStat[PAGER_STAT_HIT] & 0x7fffffff;
  a[7] = (int)pPager->aStat[PAGER_STAT_MISS] & 0x7fffffff;
  a[8] = 0;  /* Used to be pPager->nOvfl */
  a[9] = pPager->nRead;
  a[10] = (int)pPager->aStat[PAGER_STAT_WRITE] & 0x7fffffff;
  return a;
}
#endif

/*
** Parameter eStat must be one of SQLITE_DBSTATUS_CACHE_HIT, _MISS, _WRITE,
** or _WRITE+1.  The SQLITE_DBSTATUS_CACHE_WRITE+1 case is a translation
** of SQLITE_DBSTATUS_CACHE_SPILL.  The _SPILL case is not contiguous because
** it was added later.
**
** Before returning, *pnVal is incremented by the
** current cache hit or miss count, according to the value of eStat. If the
** reset parameter is non-zero, the cache hit or miss count is zeroed before
** returning.
*/
SQLITE_PRIVATE void sqlite3PagerCacheStat(Pager *pPager, int eStat, int reset, u64 *pnVal){

  assert( eStat==SQLITE_DBSTATUS_CACHE_HIT
       || eStat==SQLITE_DBSTATUS_CACHE_MISS
       || eStat==SQLITE_DBSTATUS_CACHE_WRITE
       || eStat==SQLITE_DBSTATUS_CACHE_WRITE+1
  );

  assert( SQLITE_DBSTATUS_CACHE_HIT+1==SQLITE_DBSTATUS_CACHE_MISS );
  assert( SQLITE_DBSTATUS_CACHE_HIT+2==SQLITE_DBSTATUS_CACHE_WRITE );
  assert( PAGER_STAT_HIT==0 && PAGER_STAT_MISS==1
           && PAGER_STAT_WRITE==2 && PAGER_STAT_SPILL==3 );

  eStat -= SQLITE_DBSTATUS_CACHE_HIT;
  *pnVal += pPager->aStat[eStat];
  if( reset ){
    pPager->aStat[eStat] = 0;
  }
}

/*
** Return true if this is an in-memory or temp-file backed pager.
*/
SQLITE_PRIVATE int sqlite3PagerIsMemdb(Pager *pPager){
  return pPager->tempFile || pPager->memVfs;
}

/*
** Check that there are at least nSavepoint savepoints open. If there are
** currently less than nSavepoints open, then open one or more savepoints
** to make up the difference. If the number of savepoints is already
** equal to nSavepoint, then this function is a no-op.
**
** If a memory allocation fails, SQLITE_NOMEM is returned. If an error
** occurs while opening the sub-journal file, then an IO error code is
** returned. Otherwise, SQLITE_OK.
*/
static SQLITE_NOINLINE int pagerOpenSavepoint(Pager *pPager, int nSavepoint){
  int rc = SQLITE_OK;                       /* Return code */
  int nCurrent = pPager->nSavepoint;        /* Current number of savepoints */
  int ii;                                   /* Iterator variable */
  PagerSavepoint *aNew;                     /* New Pager.aSavepoint array */

  assert( pPager->eState>=PAGER_WRITER_LOCKED );
  assert( assert_pager_state(pPager) );
  assert( nSavepoint>nCurrent && pPager->useJournal );

  /* Grow the Pager.aSavepoint array using realloc(). Return SQLITE_NOMEM
  ** if the allocation fails. Otherwise, zero the new portion in case a
  ** malloc failure occurs while populating it in the for(...) loop below.
  */
  aNew = (PagerSavepoint *)sqlite3Realloc(
      pPager->aSavepoint, sizeof(PagerSavepoint)*nSavepoint
  );
  if( !aNew ){
    return SQLITE_NOMEM_BKPT;
  }
  memset(&aNew[nCurrent], 0, (nSavepoint-nCurrent) * sizeof(PagerSavepoint));
  pPager->aSavepoint = aNew;

  /* Populate the PagerSavepoint structures just allocated. */
  for(ii=nCurrent; ii<nSavepoint; ii++){
    aNew[ii].nOrig = pPager->dbSize;
    if( isOpen(pPager->jfd) && pPager->journalOff>0 ){
      aNew[ii].iOffset = pPager->journalOff;
    }else{
      aNew[ii].iOffset = JOURNAL_HDR_SZ(pPager);
    }
    aNew[ii].iSubRec = pPager->nSubRec;
    aNew[ii].pInSavepoint = sqlite3BitvecCreate(pPager->dbSize);
    aNew[ii].bTruncateOnRelease = 1;
    if( !aNew[ii].pInSavepoint ){
      return SQLITE_NOMEM_BKPT;
    }
    if( pagerUseWal(pPager) ){
      sqlite3WalSavepoint(pPager->pWal, aNew[ii].aWalData);
    }
    pPager->nSavepoint = ii+1;
  }
  assert( pPager->nSavepoint==nSavepoint );
  assertTruncateConstraint(pPager);
  return rc;
}
SQLITE_PRIVATE int sqlite3PagerOpenSavepoint(Pager *pPager, int nSavepoint){
  assert( pPager->eState>=PAGER_WRITER_LOCKED );
  assert( assert_pager_state(pPager) );

  if( nSavepoint>pPager->nSavepoint && pPager->useJournal ){
    return pagerOpenSavepoint(pPager, nSavepoint);
  }else{
    return SQLITE_OK;
  }
}


/*
** This function is called to rollback or release (commit) a savepoint.
** The savepoint to release or rollback need not be the most recently
** created savepoint.
**
** Parameter op is always either SAVEPOINT_ROLLBACK or SAVEPOINT_RELEASE.
** If it is SAVEPOINT_RELEASE, then release and destroy the savepoint with
** index iSavepoint. If it is SAVEPOINT_ROLLBACK, then rollback all changes
** that have occurred since the specified savepoint was created.
**
** The savepoint to rollback or release is identified by parameter
** iSavepoint. A value of 0 means to operate on the outermost savepoint
** (the first created). A value of (Pager.nSavepoint-1) means operate
** on the most recently created savepoint. If iSavepoint is greater than
** (Pager.nSavepoint-1), then this function is a no-op.
**
** If a negative value is passed to this function, then the current
** transaction is rolled back. This is different to calling
** sqlite3PagerRollback() because this function does not terminate
** the transaction or unlock the database, it just restores the
** contents of the database to its original state.
**
** In any case, all savepoints with an index greater than iSavepoint
** are destroyed. If this is a release operation (op==SAVEPOINT_RELEASE),
** then savepoint iSavepoint is also destroyed.
**
** This function may return SQLITE_NOMEM if a memory allocation fails,
** or an IO error code if an IO error occurs while rolling back a
** savepoint. If no errors occur, SQLITE_OK is returned.
*/
SQLITE_PRIVATE int sqlite3PagerSavepoint(Pager *pPager, int op, int iSavepoint){
  int rc = pPager->errCode;

#ifdef SQLITE_ENABLE_ZIPVFS
  if( op==SAVEPOINT_RELEASE ) rc = SQLITE_OK;
#endif

  assert( op==SAVEPOINT_RELEASE || op==SAVEPOINT_ROLLBACK );
  assert( iSavepoint>=0 || op==SAVEPOINT_ROLLBACK );

  if( rc==SQLITE_OK && iSavepoint<pPager->nSavepoint ){
    int ii;            /* Iterator variable */
    int nNew;          /* Number of remaining savepoints after this op. */

    /* Figure out how many savepoints will still be active after this
    ** operation. Store this value in nNew. Then free resources associated
    ** with any savepoints that are destroyed by this operation.
    */
    nNew = iSavepoint + (( op==SAVEPOINT_RELEASE ) ? 0 : 1);
    for(ii=nNew; ii<pPager->nSavepoint; ii++){
      sqlite3BitvecDestroy(pPager->aSavepoint[ii].pInSavepoint);
    }
    pPager->nSavepoint = nNew;

    /* Truncate the sub-journal so that it only includes the parts
    ** that are still in use. */
    if( op==SAVEPOINT_RELEASE ){
      PagerSavepoint *pRel = &pPager->aSavepoint[nNew];
      if( pRel->bTruncateOnRelease && isOpen(pPager->sjfd) ){
        /* Only truncate if it is an in-memory sub-journal. */
        if( sqlite3JournalIsInMemory(pPager->sjfd) ){
          i64 sz = (pPager->pageSize+4)*(i64)pRel->iSubRec;
          rc = sqlite3OsTruncate(pPager->sjfd, sz);
          assert( rc==SQLITE_OK );
        }
        pPager->nSubRec = pRel->iSubRec;
      }
    }
    /* Else this is a rollback operation, playback the specified savepoint.
    ** If this is a temp-file, it is possible that the journal file has
    ** not yet been opened. In this case there have been no changes to
    ** the database file, so the playback operation can be skipped.
    */
    else if( pagerUseWal(pPager) || isOpen(pPager->jfd) ){
      PagerSavepoint *pSavepoint = (nNew==0)?0:&pPager->aSavepoint[nNew-1];
      rc = pagerPlaybackSavepoint(pPager, pSavepoint);
      assert(rc!=SQLITE_DONE);
    }

#ifdef SQLITE_ENABLE_ZIPVFS
    /* If the cache has been modified but the savepoint cannot be rolled
    ** back journal_mode=off, put the pager in the error state. This way,
    ** if the VFS used by this pager includes ZipVFS, the entire transaction
    ** can be rolled back at the ZipVFS level.  */
    else if(
        pPager->journalMode==PAGER_JOURNALMODE_OFF
     && pPager->eState>=PAGER_WRITER_CACHEMOD
    ){
      pPager->errCode = SQLITE_ABORT;
      pPager->eState = PAGER_ERROR;
      setGetterMethod(pPager);
    }
#endif
  }

  return rc;
}
