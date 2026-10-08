
/*
** Open an in-memory journal file.
*/
SQLITE_PRIVATE void sqlite3MemJournalOpen(sqlite3_file *pJfd){
  sqlite3JournalOpen(0, 0, pJfd, 0, -1);
}

#if defined(SQLITE_ENABLE_ATOMIC_WRITE) \
 || defined(SQLITE_ENABLE_BATCH_ATOMIC_WRITE)
/*
** If the argument p points to a MemJournal structure that is not an
** in-memory-only journal file (i.e. is one that was opened with a +ve
** nSpill parameter or as SQLITE_OPEN_MAIN_JOURNAL), and the underlying
** file has not yet been created, create it now.
*/
SQLITE_PRIVATE int sqlite3JournalCreate(sqlite3_file *pJfd){
  int rc = SQLITE_OK;
  MemJournal *p = (MemJournal*)pJfd;
  if( pJfd->pMethods==&MemJournalMethods && (
#ifdef SQLITE_ENABLE_ATOMIC_WRITE
     p->nSpill>0
#else
     /* While this appears to not be possible without ATOMIC_WRITE, the
     ** paths are complex, so it seems prudent to leave the test in as
     ** a NEVER(), in case our analysis is subtly flawed. */
     NEVER(p->nSpill>0)
#endif
#ifdef SQLITE_ENABLE_BATCH_ATOMIC_WRITE
     || (p->flags & SQLITE_OPEN_MAIN_JOURNAL)
#endif
  )){
    rc = memjrnlCreateFile(p);
  }
  return rc;
}
#endif

/*
** The file-handle passed as the only argument is open on a journal file.
** Return true if this "journal file" is currently stored in heap memory,
** or false otherwise.
*/
SQLITE_PRIVATE int sqlite3JournalIsInMemory(sqlite3_file *p){
  return p->pMethods==&MemJournalMethods;
}

/*
** Return the number of bytes required to store a JournalFile that uses vfs
** pVfs to create the underlying on-disk files.
*/
SQLITE_PRIVATE int sqlite3JournalSize(sqlite3_vfs *pVfs){
  return MAX(pVfs->szOsFile, (int)sizeof(MemJournal));
}

