
/*
** Btree.inTrans may take one of the following values.
**
** If the shared-data extension is enabled, there may be multiple users
** of the Btree structure. At most one of these may open a write transaction,
** but any number may have active read transactions.
**
** These values must match SQLITE_TXN_NONE, SQLITE_TXN_READ, and
** SQLITE_TXN_WRITE
*/
#define TRANS_NONE  0
#define TRANS_READ  1
#define TRANS_WRITE 2

#if TRANS_NONE!=SQLITE_TXN_NONE
# error wrong numeric code for no-transaction
#endif
#if TRANS_READ!=SQLITE_TXN_READ
# error wrong numeric code for read-transaction
#endif
#if TRANS_WRITE!=SQLITE_TXN_WRITE
# error wrong numeric code for write-transaction
#endif


/*
** An instance of this object represents a single database file.
**
** A single database file can be in use at the same time by two
** or more database connections.  When two or more connections are
** sharing the same database file, each connection has it own
** private Btree object for the file and each of those Btrees points
** to this one BtShared object.  BtShared.nRef is the number of
** connections currently sharing this database file.
**
** Fields in this structure are accessed under the BtShared.mutex
** mutex, except for nRef and pNext which are accessed under the
** global SQLITE_MUTEX_STATIC_MAIN mutex.  The pPager field
** may not be modified once it is initially set as long as nRef>0.
** The pSchema field may be set once under BtShared.mutex and
** thereafter is unchanged as long as nRef>0.
**
** isPending:
**
**   If a BtShared client fails to obtain a write-lock on a database
**   table (because there exists one or more read-locks on the table),
**   the shared-cache enters 'pending-lock' state and isPending is
**   set to true.
**
**   The shared-cache leaves the 'pending lock' state when either of
**   the following occur:
**
**     1) The current writer (BtShared.pWriter) concludes its transaction, OR
**     2) The number of locks held by other connections drops to zero.
**
**   while in the 'pending-lock' state, no connection may start a new
**   transaction.
**
**   This feature is included to help prevent writer-starvation.
*/
struct BtShared {
  Pager *pPager;        /* The page cache */
  sqlite3 *db;          /* Database connection currently using this Btree */
  BtCursor *pCursor;    /* A list of all open cursors */
  MemPage *pPage1;      /* First page of the database */
  u8 openFlags;         /* Flags to sqlite3BtreeOpen() */
#ifndef SQLITE_OMIT_AUTOVACUUM
  u8 autoVacuum;        /* True if auto-vacuum is enabled */
  u8 incrVacuum;        /* True if incr-vacuum is enabled */
  u8 bDoTruncate;       /* True to truncate db on commit */
#endif
  u8 inTransaction;     /* Transaction state */
  u8 max1bytePayload;   /* Maximum first byte of cell for a 1-byte payload */
  u8 nReserveWanted;    /* Desired number of extra bytes per page */
  u16 btsFlags;         /* Boolean parameters.  See BTS_* macros below */
  u16 maxLocal;         /* Maximum local payload in non-LEAFDATA tables */
  u16 minLocal;         /* Minimum local payload in non-LEAFDATA tables */
  u16 maxLeaf;          /* Maximum local payload in a LEAFDATA table */
  u16 minLeaf;          /* Minimum local payload in a LEAFDATA table */
  u32 pageSize;         /* Total number of bytes on a page */
  u32 usableSize;       /* Number of usable bytes on each page */
  int nTransaction;     /* Number of open transactions (read + write) */
  u32 nPage;            /* Number of pages in the database */
  void *pSchema;        /* Pointer to space allocated by sqlite3BtreeSchema() */
  void (*xFreeSchema)(void*);  /* Destructor for BtShared.pSchema */
  sqlite3_mutex *mutex; /* Non-recursive mutex required to access this object */
  Bitvec *pHasContent;  /* Set of pages moved to free-list this transaction */
#ifndef SQLITE_OMIT_SHARED_CACHE
  int nRef;             /* Number of references to this structure */
  BtShared *pNext;      /* Next on a list of sharable BtShared structs */
  BtLock *pLock;        /* List of locks held on this shared-btree struct */
  Btree *pWriter;       /* Btree with currently open write transaction */
#endif
  u8 *pTmpSpace;        /* Temp space sufficient to hold a single cell */
  int nPreformatSize;   /* Size of last cell written by TransferRow() */
};

/*
** Allowed values for BtShared.btsFlags
*/
#define BTS_READ_ONLY        0x0001   /* Underlying file is readonly */
#define BTS_PAGESIZE_FIXED   0x0002   /* Page size can no longer be changed */
#define BTS_SECURE_DELETE    0x0004   /* PRAGMA secure_delete is enabled */
#define BTS_OVERWRITE        0x0008   /* Overwrite deleted content with zeros */
#define BTS_FAST_SECURE      0x000c   /* Combination of the previous two */
#define BTS_INITIALLY_EMPTY  0x0010   /* Database was empty at trans start */
#define BTS_NO_WAL           0x0020   /* Do not open write-ahead-log files */
#define BTS_EXCLUSIVE        0x0040   /* pWriter has an exclusive lock */
#define BTS_PENDING          0x0080   /* Waiting for read-locks to clear */

/*
** An instance of the following structure is used to hold information
** about a cell.  The parseCellPtr() function fills in this structure
** based on information extract from the raw disk page.
*/
struct CellInfo {
  i64 nKey;      /* The key for INTKEY tables, or nPayload otherwise */
  u8 *pPayload;  /* Pointer to the start of payload */
  u32 nPayload;  /* Bytes of payload */
  u16 nLocal;    /* Amount of payload held locally, not on overflow */
  u16 nSize;     /* Size of the cell content on the main b-tree page */
};

/*
** Maximum depth of an SQLite B-Tree structure. Any B-Tree deeper than
** this will be declared corrupt. This value is calculated based on a
** maximum database size of 2^31 pages a minimum fanout of 2 for a
** root-node and 3 for all other internal nodes.
**
** If a tree that appears to be taller than this is encountered, it is
** assumed that the database is corrupt.
*/
#define BTCURSOR_MAX_DEPTH 20

/*
** A cursor is a pointer to a particular entry within a particular
** b-tree within a database file.
**
** The entry is identified by its MemPage and the index in
** MemPage.aCell[] of the entry.
**
** A single database file can be shared by two more database connections,
** but cursors cannot be shared.  Each cursor is associated with a
** particular database connection identified BtCursor.pBtree.db.
**
** Fields in this structure are accessed under the BtShared.mutex
** found at self->pBt->mutex.
**
** skipNext meaning:
** The meaning of skipNext depends on the value of eState:
**
**   eState            Meaning of skipNext
**   VALID             skipNext is meaningless and is ignored
**   INVALID           skipNext is meaningless and is ignored
**   SKIPNEXT          sqlite3BtreeNext() is a no-op if skipNext>0 and
**                     sqlite3BtreePrevious() is no-op if skipNext<0.
**   REQUIRESEEK       restoreCursorPosition() restores the cursor to
**                     eState=SKIPNEXT if skipNext!=0
**   FAULT             skipNext holds the cursor fault error code.
*/
struct BtCursor {
  u8 eState;                /* One of the CURSOR_XXX constants (see below) */
  u8 curFlags;              /* zero or more BTCF_* flags defined below */
  u8 curPagerFlags;         /* Flags to send to sqlite3PagerGet() */
  u8 hints;                 /* As configured by CursorSetHints() */
  int skipNext;    /* Prev() is noop if negative. Next() is noop if positive.
                   ** Error code if eState==CURSOR_FAULT */
  Btree *pBtree;            /* The Btree to which this cursor belongs */
  Pgno *aOverflow;          /* Cache of overflow page locations */
  void *pKey;               /* Saved key that was cursor last known position */
  /* All fields above are zeroed when the cursor is allocated.  See
  ** sqlite3BtreeCursorZero().  Fields that follow must be manually
  ** initialized. */
#define BTCURSOR_FIRST_UNINIT pBt   /* Name of first uninitialized field */
  BtShared *pBt;            /* The BtShared this cursor points to */
  BtCursor *pNext;          /* Forms a linked list of all cursors */
  CellInfo info;            /* A parse of the cell we are pointing at */
  i64 nKey;                 /* Size of pKey, or last integer key */
  Pgno pgnoRoot;            /* The root page of this tree */
  i8 iPage;                 /* Index of current page in apPage */
  u8 curIntKey;             /* Value of apPage[0]->intKey */
  u16 ix;                   /* Current index for apPage[iPage] */
  u16 aiIdx[BTCURSOR_MAX_DEPTH-1];     /* Current index in apPage[i] */
  struct KeyInfo *pKeyInfo;            /* Arg passed to comparison function */
  MemPage *pPage;                        /* Current page */
  MemPage *apPage[BTCURSOR_MAX_DEPTH-1]; /* Stack of parents of current page */
};

/*
** Legal values for BtCursor.curFlags
*/
#define BTCF_WriteFlag    0x01   /* True if a write cursor */
#define BTCF_ValidNKey    0x02   /* True if info.nKey is valid */
#define BTCF_ValidOvfl    0x04   /* True if aOverflow is valid */
#define BTCF_AtLast       0x08   /* Cursor is pointing to the last entry */
#define BTCF_Incrblob     0x10   /* True if an incremental I/O handle */
#define BTCF_Multiple     0x20   /* Maybe another cursor on the same btree */
#define BTCF_Pinned       0x40   /* Cursor is busy and cannot be moved */

/*
** Potential values for BtCursor.eState.
**
** CURSOR_INVALID:
**   Cursor does not point to a valid entry. This can happen (for example)
**   because the table is empty or because BtreeCursorFirst() has not been
**   called.
**
** CURSOR_VALID:
**   Cursor points to a valid entry. getPayload() etc. may be called.
**
** CURSOR_SKIPNEXT:
**   Cursor is valid except that the Cursor.skipNext field is non-zero
**   indicating that the next sqlite3BtreeNext() or sqlite3BtreePrevious()
**   operation should be a no-op.
**
** CURSOR_REQUIRESEEK:
**   The table that this cursor was opened on still exists, but has been
**   modified since the cursor was last used. The cursor position is saved
**   in variables BtCursor.pKey and BtCursor.nKey. When a cursor is in
**   this state, restoreCursorPosition() can be called to attempt to
**   seek the cursor to the saved position.
**
** CURSOR_FAULT:
**   An unrecoverable error (an I/O error or a malloc failure) has occurred
**   on a different connection that shares the BtShared cache with this
**   cursor.  The error has left the cache in an inconsistent state.
**   Do nothing else with this cursor.  Any attempt to use the cursor
**   should return the error code stored in BtCursor.skipNext
*/
#define CURSOR_VALID             0
#define CURSOR_INVALID           1
#define CURSOR_SKIPNEXT          2
#define CURSOR_REQUIRESEEK       3
#define CURSOR_FAULT             4

/*
** The database page the PENDING_BYTE occupies. This page is never used.
*/
#define PENDING_BYTE_PAGE(pBt)  ((Pgno)((PENDING_BYTE/((pBt)->pageSize))+1))

/*
** These macros define the location of the pointer-map entry for a
** database page. The first argument to each is the number of usable
** bytes on each page of the database (often 1024). The second is the
** page number to look up in the pointer map.
**
** PTRMAP_PAGENO returns the database page number of the pointer-map
** page that stores the required pointer. PTRMAP_PTROFFSET returns
** the offset of the requested map entry.
**
** If the pgno argument passed to PTRMAP_PAGENO is a pointer-map page,
** then pgno is returned. So (pgno==PTRMAP_PAGENO(pgsz, pgno)) can be
** used to test if pgno is a pointer-map page. PTRMAP_ISPAGE implements
** this test.
*/
#define PTRMAP_PAGENO(pBt, pgno) ptrmapPageno(pBt, pgno)
#define PTRMAP_PTROFFSET(pgptrmap, pgno) (5*(pgno-pgptrmap-1))
#define PTRMAP_ISPAGE(pBt, pgno) (PTRMAP_PAGENO((pBt),(pgno))==(pgno))

/*
** The pointer map is a lookup table that identifies the parent page for
** each child page in the database file.  The parent page is the page that
** contains a pointer to the child.  Every page in the database contains
** 0 or 1 parent pages.  (In this context 'database page' refers
** to any page that is not part of the pointer map itself.)  Each pointer map
** entry consists of a single byte 'type' and a 4 byte parent page number.
** The PTRMAP_XXX identifiers below are the valid types.
**
** The purpose of the pointer map is to facility moving pages from one
** position in the file to another as part of autovacuum.  When a page
** is moved, the pointer in its parent must be updated to point to the
** new location.  The pointer map is used to locate the parent page quickly.
**
** PTRMAP_ROOTPAGE: The database page is a root-page. The page-number is not
**                  used in this case.
**
** PTRMAP_FREEPAGE: The database page is an unused (free) page. The page-number
**                  is not used in this case.
**
** PTRMAP_OVERFLOW1: The database page is the first page in a list of
**                   overflow pages. The page number identifies the page that
**                   contains the cell with a pointer to this overflow page.
**
** PTRMAP_OVERFLOW2: The database page is the second or later page in a list of
**                   overflow pages. The page-number identifies the previous
**                   page in the overflow page list.
**
** PTRMAP_BTREE: The database page is a non-root btree page. The page number
**               identifies the parent page in the btree.
*/
#define PTRMAP_ROOTPAGE 1
#define PTRMAP_FREEPAGE 2
#define PTRMAP_OVERFLOW1 3
#define PTRMAP_OVERFLOW2 4
#define PTRMAP_BTREE 5

/* A bunch of assert() statements to check the transaction state variables
** of handle p (type Btree*) are internally consistent.
*/
#define btreeIntegrity(p) \
  assert( p->pBt->inTransaction!=TRANS_NONE || p->pBt->nTransaction==0 ); \
  assert( p->pBt->inTransaction>=p->inTrans );


/*
** The ISAUTOVACUUM macro is used within balance_nonroot() to determine
** if the database supports auto-vacuum or not. Because it is used
** within an expression that is an argument to another macro
** (sqliteMallocRaw), it is not possible to use conditional compilation.
** So, this macro is defined instead.
*/
#ifndef SQLITE_OMIT_AUTOVACUUM
#define ISAUTOVACUUM(pBt) (pBt->autoVacuum)
#else
#define ISAUTOVACUUM(pBt) 0
#endif


/*
** This structure is passed around through all the PRAGMA integrity_check
** checking routines in order to keep track of some global state information.
**
** The aRef[] array is allocated so that there is 1 bit for each page in
** the database. As the integrity-check proceeds, for each page used in
** the database the corresponding bit is set. This allows integrity-check to
** detect pages that are used twice and orphaned pages (both of which
** indicate corruption).
*/
typedef struct IntegrityCk IntegrityCk;
struct IntegrityCk {
  BtShared *pBt;    /* The tree being checked out */
  Pager *pPager;    /* The associated pager.  Also accessible by pBt->pPager */
  u8 *aPgRef;       /* 1 bit per page in the db (see above) */
  Pgno nCkPage;     /* Pages in the database.  0 for partial check */
  int mxErr;        /* Stop accumulating errors when this reaches zero */
  int nErr;         /* Number of messages written to zErrMsg so far */
  int rc;           /* SQLITE_OK, SQLITE_NOMEM, or SQLITE_INTERRUPT */
  u32 nStep;        /* Number of steps into the integrity_check process */
  const char *zPfx; /* Error message prefix */
  Pgno v0;          /* Value for first %u substitution in zPfx (root page) */
  Pgno v1;          /* Value for second %u substitution in zPfx (current pg) */
  int v2;           /* Value for third %d substitution in zPfx */
  StrAccum errMsg;  /* Accumulate the error message text here */
  u32 *heap;        /* Min-heap used for analyzing cell coverage */
  sqlite3 *db;      /* Database connection running the check */
  i64 nRow;         /* Number of rows visited in current tree */
};

/*
