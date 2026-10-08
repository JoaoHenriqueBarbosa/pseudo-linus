
#endif /* SQLITE_ENABLE_SNAPSHOT */
#endif /* !SQLITE_OMIT_WAL */

#ifdef SQLITE_ENABLE_ZIPVFS
/*
** A read-lock must be held on the pager when this function is called. If
** the pager is in WAL mode and the WAL file currently contains one or more
** frames, return the size in bytes of the page images stored within the
** WAL frames. Otherwise, if this is not a WAL database or the WAL file
** is empty, return 0.
*/
SQLITE_PRIVATE int sqlite3PagerWalFramesize(Pager *pPager){
  assert( pPager->eState>=PAGER_READER );
  return sqlite3WalFramesize(pPager->pWal);
}
#endif

#if defined(SQLITE_USE_SEH) && !defined(SQLITE_OMIT_WAL)
SQLITE_PRIVATE int sqlite3PagerWalSystemErrno(Pager *pPager){
  return sqlite3WalSystemErrno(pPager->pWal);
}
#endif

#endif /* SQLITE_OMIT_DISKIO */

