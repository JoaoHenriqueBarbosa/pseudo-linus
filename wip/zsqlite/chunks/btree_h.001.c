SQLITE_PRIVATE int sqlite3BtreeIsReadonly(Btree *pBt);
SQLITE_PRIVATE int sqlite3HeaderSizeBtree(void);

#ifdef SQLITE_DEBUG
SQLITE_PRIVATE sqlite3_uint64 sqlite3BtreeSeekCount(Btree*);
#else
# define sqlite3BtreeSeekCount(X) 0
#endif

#ifndef NDEBUG
SQLITE_PRIVATE int sqlite3BtreeCursorIsValid(BtCursor*);
#endif
SQLITE_PRIVATE int sqlite3BtreeCursorIsValidNN(BtCursor*);

SQLITE_PRIVATE int sqlite3BtreeCount(sqlite3*, BtCursor*, i64*);

#ifdef SQLITE_TEST
SQLITE_PRIVATE int sqlite3BtreeCursorInfo(BtCursor*, int*, int);
SQLITE_PRIVATE void sqlite3BtreeCursorList(Btree*);
#endif

#ifndef SQLITE_OMIT_WAL
SQLITE_PRIVATE   int sqlite3BtreeCheckpoint(Btree*, int, int *, int *);
#endif

SQLITE_PRIVATE int sqlite3BtreeTransferRow(BtCursor*, BtCursor*, i64);

SQLITE_PRIVATE void sqlite3BtreeClearCache(Btree*);

/*
** If we are not using shared cache, then there is no need to
** use mutexes to access the BtShared structures.  So make the
** Enter and Leave procedures no-ops.
*/
#ifndef SQLITE_OMIT_SHARED_CACHE
SQLITE_PRIVATE   void sqlite3BtreeEnter(Btree*);
SQLITE_PRIVATE   void sqlite3BtreeEnterAll(sqlite3*);
SQLITE_PRIVATE   int sqlite3BtreeSharable(Btree*);
SQLITE_PRIVATE   void sqlite3BtreeEnterCursor(BtCursor*);
SQLITE_PRIVATE   int sqlite3BtreeConnectionCount(Btree*);
#else
# define sqlite3BtreeEnter(X)
# define sqlite3BtreeEnterAll(X)
# define sqlite3BtreeSharable(X) 0
# define sqlite3BtreeEnterCursor(X)
# define sqlite3BtreeConnectionCount(X) 1
#endif

#if !defined(SQLITE_OMIT_SHARED_CACHE) && SQLITE_THREADSAFE
SQLITE_PRIVATE   void sqlite3BtreeLeave(Btree*);
SQLITE_PRIVATE   void sqlite3BtreeLeaveCursor(BtCursor*);
SQLITE_PRIVATE   void sqlite3BtreeLeaveAll(sqlite3*);
#ifndef NDEBUG
  /* These routines are used inside assert() statements only. */
SQLITE_PRIVATE   int sqlite3BtreeHoldsMutex(Btree*);
SQLITE_PRIVATE   int sqlite3BtreeHoldsAllMutexes(sqlite3*);
SQLITE_PRIVATE   int sqlite3SchemaMutexHeld(sqlite3*,int,Schema*);
#endif
#else

# define sqlite3BtreeLeave(X)
# define sqlite3BtreeLeaveCursor(X)
# define sqlite3BtreeLeaveAll(X)

# define sqlite3BtreeHoldsMutex(X) 1
# define sqlite3BtreeHoldsAllMutexes(X) 1
# define sqlite3SchemaMutexHeld(X,Y,Z) 1
#endif


#endif /* SQLITE_BTREE_H */

