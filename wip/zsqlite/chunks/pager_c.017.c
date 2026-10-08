
/*
** Return the full pathname of the database file.
**
** Except, if the pager is in-memory only, then return an empty string if
** nullIfMemDb is true.  This routine is called with nullIfMemDb==1 when
** used to report the filename to the user, for compatibility with legacy
** behavior.  But when the Btree needs to know the filename for matching to
** shared cache, it uses nullIfMemDb==0 so that in-memory databases can
** participate in shared-cache.
**
** The return value to this routine is always safe to use with
** sqlite3_uri_parameter() and sqlite3_filename_database() and friends.
*/
SQLITE_PRIVATE const char *sqlite3PagerFilename(const Pager *pPager, int nullIfMemDb){
  static const char zFake[8] = { 0, 0, 0, 0, 0, 0, 0, 0 };
  if( nullIfMemDb && (pPager->memDb || sqlite3IsMemdb(pPager->pVfs)) ){
    return &zFake[4];
  }else{
    return pPager->zFilename;
  }
}

/*
** Return the VFS structure for the pager.
*/
SQLITE_PRIVATE sqlite3_vfs *sqlite3PagerVfs(Pager *pPager){
  return pPager->pVfs;
}

/*
** Return the file handle for the database file associated
** with the pager.  This might return NULL if the file has
** not yet been opened.
*/
SQLITE_PRIVATE sqlite3_file *sqlite3PagerFile(Pager *pPager){
  return pPager->fd;
}

/*
** Return the file handle for the journal file (if it exists).
** This will be either the rollback journal or the WAL file.
*/
SQLITE_PRIVATE sqlite3_file *sqlite3PagerJrnlFile(Pager *pPager){
#ifdef SQLITE_OMIT_WAL
  return pPager->jfd;
#else
  return pPager->pWal ? sqlite3WalFile(pPager->pWal) : pPager->jfd;
#endif
}

/*
** Return the full pathname of the journal file.
*/
SQLITE_PRIVATE const char *sqlite3PagerJournalname(Pager *pPager){
  return pPager->zJournal;
}

#ifndef SQLITE_OMIT_AUTOVACUUM
/*
** Move the page pPg to location pgno in the file.
**
** There must be no references to the page previously located at
** pgno (which we call pPgOld) though that page is allowed to be
** in cache.  If the page previously located at pgno is not already
** in the rollback journal, it is not put there by by this routine.
**
** References to the page pPg remain valid. Updating any
** meta-data associated with pPg (i.e. data stored in the nExtra bytes
** allocated along with the page) is the responsibility of the caller.
**
** A transaction must be active when this routine is called. It used to be
** required that a statement transaction was not active, but this restriction
** has been removed (CREATE INDEX needs to move a page when a statement
** transaction is active).
**
** If the fourth argument, isCommit, is non-zero, then this page is being
** moved as part of a database reorganization just before the transaction
** is being committed. In this case, it is guaranteed that the database page
** pPg refers to will not be written to again within this transaction.
**
** This function may return SQLITE_NOMEM or an IO error code if an error
** occurs. Otherwise, it returns SQLITE_OK.
*/
SQLITE_PRIVATE int sqlite3PagerMovepage(Pager *pPager, DbPage *pPg, Pgno pgno, int isCommit){
  PgHdr *pPgOld;               /* The page being overwritten. */
  Pgno needSyncPgno = 0;       /* Old value of pPg->pgno, if sync is required */
  int rc;                      /* Return code */
  Pgno origPgno;               /* The original page number */

  assert( pPg->nRef>0 );
  assert( pPager->eState==PAGER_WRITER_CACHEMOD
       || pPager->eState==PAGER_WRITER_DBMOD
  );
  assert( assert_pager_state(pPager) );

  /* In order to be able to rollback, an in-memory database must journal
  ** the page we are moving from.
  */
  assert( pPager->tempFile || !MEMDB );
  if( pPager->tempFile ){
    rc = sqlite3PagerWrite(pPg);
    if( rc ) return rc;
  }

  /* If the page being moved is dirty and has not been saved by the latest
  ** savepoint, then save the current contents of the page into the
  ** sub-journal now. This is required to handle the following scenario:
  **
  **   BEGIN;
  **     <journal page X, then modify it in memory>
  **     SAVEPOINT one;
  **       <Move page X to location Y>
  **     ROLLBACK TO one;
  **
  ** If page X were not written to the sub-journal here, it would not
  ** be possible to restore its contents when the "ROLLBACK TO one"
  ** statement were is processed.
  **
  ** subjournalPage() may need to allocate space to store pPg->pgno into
  ** one or more savepoint bitvecs. This is the reason this function
  ** may return SQLITE_NOMEM.
  */
  if( (pPg->flags & PGHDR_DIRTY)!=0
   && SQLITE_OK!=(rc = subjournalPageIfRequired(pPg))
  ){
    return rc;
  }

  PAGERTRACE(("MOVE %d page %d (needSync=%d) moves to %d\n",
      PAGERID(pPager), pPg->pgno, (pPg->flags&PGHDR_NEED_SYNC)?1:0, pgno));
  IOTRACE(("MOVE %p %d %d\n", pPager, pPg->pgno, pgno))

  /* If the journal needs to be sync()ed before page pPg->pgno can
  ** be written to, store pPg->pgno in local variable needSyncPgno.
  **
  ** If the isCommit flag is set, there is no need to remember that
  ** the journal needs to be sync()ed before database page pPg->pgno
  ** can be written to. The caller has already promised not to write to it.
  */
  if( (pPg->flags&PGHDR_NEED_SYNC) && !isCommit ){
    needSyncPgno = pPg->pgno;
    assert( pPager->journalMode==PAGER_JOURNALMODE_OFF ||
            pageInJournal(pPager, pPg) || pPg->pgno>pPager->dbOrigSize );
    assert( pPg->flags&PGHDR_DIRTY );
  }

  /* If the cache contains a page with page-number pgno, remove it
  ** from its hash chain. Also, if the PGHDR_NEED_SYNC flag was set for
  ** page pgno before the 'move' operation, it needs to be retained
  ** for the page moved there.
  */
  pPg->flags &= ~PGHDR_NEED_SYNC;
  pPgOld = sqlite3PagerLookup(pPager, pgno);
  assert( !pPgOld || pPgOld->nRef==1 || CORRUPT_DB );
  if( pPgOld ){
    if( NEVER(pPgOld->nRef>1) ){
      sqlite3PagerUnrefNotNull(pPgOld);
      return SQLITE_CORRUPT_BKPT;
    }
    pPg->flags |= (pPgOld->flags&PGHDR_NEED_SYNC);
    if( pPager->tempFile ){
      /* Do not discard pages from an in-memory database since we might
      ** need to rollback later.  Just move the page out of the way. */
      sqlite3PcacheMove(pPgOld, pPager->dbSize+1);
    }else{
      sqlite3PcacheDrop(pPgOld);
    }
  }

  origPgno = pPg->pgno;
  sqlite3PcacheMove(pPg, pgno);
  sqlite3PcacheMakeDirty(pPg);

  /* For an in-memory database, make sure the original page continues
  ** to exist, in case the transaction needs to roll back.  Use pPgOld
  ** as the original page since it has already been allocated.
  */
  if( pPager->tempFile && pPgOld ){
    sqlite3PcacheMove(pPgOld, origPgno);
    sqlite3PagerUnrefNotNull(pPgOld);
  }

  if( needSyncPgno ){
    /* If needSyncPgno is non-zero, then the journal file needs to be
    ** sync()ed before any data is written to database file page needSyncPgno.
    ** Currently, no such page exists in the page-cache and the
    ** "is journaled" bitvec flag has been set. This needs to be remedied by
    ** loading the page into the pager-cache and setting the PGHDR_NEED_SYNC
    ** flag.
    **
    ** If the attempt to load the page into the page-cache fails, (due
    ** to a malloc() or IO failure), clear the bit in the pInJournal[]
    ** array. Otherwise, if the page is loaded and written again in
    ** this transaction, it may be written to the database file before
    ** it is synced into the journal file. This way, it may end up in
    ** the journal file twice, but that is not a problem.
    */
    PgHdr *pPgHdr;
    rc = sqlite3PagerGet(pPager, needSyncPgno, &pPgHdr, 0);
    if( rc!=SQLITE_OK ){
      if( needSyncPgno<=pPager->dbOrigSize ){
        assert( pPager->pTmpSpace!=0 );
        sqlite3BitvecClear(pPager->pInJournal, needSyncPgno, pPager->pTmpSpace);
      }
      return rc;
    }
    pPgHdr->flags |= PGHDR_NEED_SYNC;
    sqlite3PcacheMakeDirty(pPgHdr);
    sqlite3PagerUnrefNotNull(pPgHdr);
  }

  return SQLITE_OK;
}
#endif

/*
** The page handle passed as the first argument refers to a dirty page
** with a page number other than iNew. This function changes the page's
** page number to iNew and sets the value of the PgHdr.flags field to
** the value passed as the third parameter.
*/
SQLITE_PRIVATE void sqlite3PagerRekey(DbPage *pPg, Pgno iNew, u16 flags){
  assert( pPg->pgno!=iNew );
  pPg->flags = flags;
  sqlite3PcacheMove(pPg, iNew);
}

/*
** Return a pointer to the data for the specified page.
*/
SQLITE_PRIVATE void *sqlite3PagerGetData(DbPage *pPg){
  assert( pPg->nRef>0 || pPg->pPager->memDb );
  return pPg->pData;
}

/*
** Return a pointer to the Pager.nExtra bytes of "extra" space
** allocated along with the specified page.
*/
SQLITE_PRIVATE void *sqlite3PagerGetExtra(DbPage *pPg){
  return pPg->pExtra;
}

/*
** Get/set the locking-mode for this pager. Parameter eMode must be one
** of PAGER_LOCKINGMODE_QUERY, PAGER_LOCKINGMODE_NORMAL or
** PAGER_LOCKINGMODE_EXCLUSIVE. If the parameter is not _QUERY, then
** the locking-mode is set to the value specified.
**
** The returned value is either PAGER_LOCKINGMODE_NORMAL or
** PAGER_LOCKINGMODE_EXCLUSIVE, indicating the current (possibly updated)
** locking-mode.
*/
SQLITE_PRIVATE int sqlite3PagerLockingMode(Pager *pPager, int eMode){
  assert( eMode==PAGER_LOCKINGMODE_QUERY
            || eMode==PAGER_LOCKINGMODE_NORMAL
            || eMode==PAGER_LOCKINGMODE_EXCLUSIVE );
  assert( PAGER_LOCKINGMODE_QUERY<0 );
  assert( PAGER_LOCKINGMODE_NORMAL>=0 && PAGER_LOCKINGMODE_EXCLUSIVE>=0 );
  assert( pPager->exclusiveMode || 0==sqlite3WalHeapMemory(pPager->pWal) );
  if( eMode>=0 && !pPager->tempFile && !sqlite3WalHeapMemory(pPager->pWal) ){
    pPager->exclusiveMode = (u8)eMode;
  }
  return (int)pPager->exclusiveMode;
}

/*
** Set the journal-mode for this pager. Parameter eMode must be one of:
**
**    PAGER_JOURNALMODE_DELETE
**    PAGER_JOURNALMODE_TRUNCATE
**    PAGER_JOURNALMODE_PERSIST
**    PAGER_JOURNALMODE_OFF
**    PAGER_JOURNALMODE_MEMORY
**    PAGER_JOURNALMODE_WAL
**
** The journalmode is set to the value specified if the change is allowed.
** The change may be disallowed for the following reasons:
**
**   *  An in-memory database can only have its journal_mode set to _OFF
**      or _MEMORY.
**
**   *  Temporary databases cannot have _WAL journalmode.
**
** The returned indicate the current (possibly updated) journal-mode.
*/
SQLITE_PRIVATE int sqlite3PagerSetJournalMode(Pager *pPager, int eMode){
  u8 eOld = pPager->journalMode;    /* Prior journalmode */

  /* The eMode parameter is always valid */
  assert(      eMode==PAGER_JOURNALMODE_DELETE    /* 0 */
            || eMode==PAGER_JOURNALMODE_PERSIST   /* 1 */
            || eMode==PAGER_JOURNALMODE_OFF       /* 2 */
            || eMode==PAGER_JOURNALMODE_TRUNCATE  /* 3 */
            || eMode==PAGER_JOURNALMODE_MEMORY    /* 4 */
            || eMode==PAGER_JOURNALMODE_WAL       /* 5 */ );

  /* This routine is only called from the OP_JournalMode opcode, and
  ** the logic there will never allow a temporary file to be changed
  ** to WAL mode.
  */
  assert( pPager->tempFile==0 || eMode!=PAGER_JOURNALMODE_WAL );

  /* Do allow the journalmode of an in-memory database to be set to
  ** anything other than MEMORY or OFF
  */
  if( MEMDB ){
    assert( eOld==PAGER_JOURNALMODE_MEMORY || eOld==PAGER_JOURNALMODE_OFF );
    if( eMode!=PAGER_JOURNALMODE_MEMORY && eMode!=PAGER_JOURNALMODE_OFF ){
      eMode = eOld;
    }
  }

  if( eMode!=eOld ){

    /* Change the journal mode. */
    assert( pPager->eState!=PAGER_ERROR );
    pPager->journalMode = (u8)eMode;

    /* When transitioning from TRUNCATE or PERSIST to any other journal
    ** mode except WAL, unless the pager is in locking_mode=exclusive mode,
    ** delete the journal file.
    */
    assert( (PAGER_JOURNALMODE_TRUNCATE & 5)==1 );
    assert( (PAGER_JOURNALMODE_PERSIST & 5)==1 );
    assert( (PAGER_JOURNALMODE_DELETE & 5)==0 );
    assert( (PAGER_JOURNALMODE_MEMORY & 5)==4 );
    assert( (PAGER_JOURNALMODE_OFF & 5)==0 );
    assert( (PAGER_JOURNALMODE_WAL & 5)==5 );

    assert( isOpen(pPager->fd) || pPager->exclusiveMode );
    if( !pPager->exclusiveMode && (eOld & 5)==1 && (eMode & 1)==0 ){
      /* In this case we would like to delete the journal file. If it is
      ** not possible, then that is not a problem. Deleting the journal file
      ** here is an optimization only.
      **
      ** Before deleting the journal file, obtain a RESERVED lock on the
      ** database file. This ensures that the journal file is not deleted
      ** while it is in use by some other client.
      */
      sqlite3OsClose(pPager->jfd);
      if( pPager->eLock>=RESERVED_LOCK ){
        sqlite3OsDelete(pPager->pVfs, pPager->zJournal, 0);
      }else{
        int rc = SQLITE_OK;
        int state = pPager->eState;
        assert( state==PAGER_OPEN || state==PAGER_READER );
        if( state==PAGER_OPEN ){
          rc = sqlite3PagerSharedLock(pPager);
        }
        if( pPager->eState==PAGER_READER ){
          assert( rc==SQLITE_OK );
          rc = pagerLockDb(pPager, RESERVED_LOCK);
        }
        if( rc==SQLITE_OK ){
          sqlite3OsDelete(pPager->pVfs, pPager->zJournal, 0);
        }
        if( rc==SQLITE_OK && state==PAGER_READER ){
          pagerUnlockDb(pPager, SHARED_LOCK);
        }else if( state==PAGER_OPEN ){
          pager_unlock(pPager);
        }
        assert( state==pPager->eState );
      }
    }else if( eMode==PAGER_JOURNALMODE_OFF || eMode==PAGER_JOURNALMODE_MEMORY ){
      sqlite3OsClose(pPager->jfd);
    }
  }

  /* Return the new journal mode */
  return (int)pPager->journalMode;
}
