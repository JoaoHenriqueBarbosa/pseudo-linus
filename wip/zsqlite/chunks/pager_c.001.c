
/*
** Bits of the Pager.doNotSpill flag.  See further description below.
*/
#define SPILLFLAG_OFF         0x01 /* Never spill cache.  Set via pragma */
#define SPILLFLAG_ROLLBACK    0x02 /* Current rolling back, so do not spill */
#define SPILLFLAG_NOSYNC      0x04 /* Spill is ok, but do not sync */

/*
** An open page cache is an instance of struct Pager. A description of
** some of the more important member variables follows:
**
** eState
**
**   The current 'state' of the pager object. See the comment and state
**   diagram above for a description of the pager state.
**
** eLock
**
**   For a real on-disk database, the current lock held on the database file -
**   NO_LOCK, SHARED_LOCK, RESERVED_LOCK or EXCLUSIVE_LOCK.
**
**   For a temporary or in-memory database (neither of which require any
**   locks), this variable is always set to EXCLUSIVE_LOCK. Since such
**   databases always have Pager.exclusiveMode==1, this tricks the pager
**   logic into thinking that it already has all the locks it will ever
**   need (and no reason to release them).
**
**   In some (obscure) circumstances, this variable may also be set to
**   UNKNOWN_LOCK. See the comment above the #define of UNKNOWN_LOCK for
**   details.
**
** changeCountDone
**
**   This boolean variable is used to make sure that the change-counter
**   (the 4-byte header field at byte offset 24 of the database file) is
**   not updated more often than necessary.
**
**   It is set to true when the change-counter field is updated, which
**   can only happen if an exclusive lock is held on the database file.
**   It is cleared (set to false) whenever an exclusive lock is
**   relinquished on the database file. Each time a transaction is committed,
**   The changeCountDone flag is inspected. If it is true, the work of
**   updating the change-counter is omitted for the current transaction.
**
**   This mechanism means that when running in exclusive mode, a connection
**   need only update the change-counter once, for the first transaction
**   committed.
**
** setSuper
**
**   When PagerCommitPhaseOne() is called to commit a transaction, it may
**   (or may not) specify a super-journal name to be written into the
**   journal file before it is synced to disk.
**
**   Whether or not a journal file contains a super-journal pointer affects
**   the way in which the journal file is finalized after the transaction is
**   committed or rolled back when running in "journal_mode=PERSIST" mode.
**   If a journal file does not contain a super-journal pointer, it is
**   finalized by overwriting the first journal header with zeroes. If
**   it does contain a super-journal pointer the journal file is finalized
**   by truncating it to zero bytes, just as if the connection were
**   running in "journal_mode=truncate" mode.
**
**   Journal files that contain super-journal pointers cannot be finalized
**   simply by overwriting the first journal-header with zeroes, as the
**   super-journal pointer could interfere with hot-journal rollback of any
**   subsequently interrupted transaction that reuses the journal file.
**
**   The flag is cleared as soon as the journal file is finalized (either
**   by PagerCommitPhaseTwo or PagerRollback). If an IO error prevents the
**   journal file from being successfully finalized, the setSuper flag
**   is cleared anyway (and the pager will move to ERROR state).
**
** doNotSpill
**
**   This variables control the behavior of cache-spills  (calls made by
**   the pcache module to the pagerStress() routine to write cached data
**   to the file-system in order to free up memory).
**
**   When bits SPILLFLAG_OFF or SPILLFLAG_ROLLBACK of doNotSpill are set,
**   writing to the database from pagerStress() is disabled altogether.
**   The SPILLFLAG_ROLLBACK case is done in a very obscure case that
**   comes up during savepoint rollback that requires the pcache module
**   to allocate a new page to prevent the journal file from being written
**   while it is being traversed by code in pager_playback().  The SPILLFLAG_OFF
**   case is a user preference.
**
**   If the SPILLFLAG_NOSYNC bit is set, writing to the database from
**   pagerStress() is permitted, but syncing the journal file is not.
**   This flag is set by sqlite3PagerWrite() when the file-system sector-size
**   is larger than the database page-size in order to prevent a journal sync
**   from happening in between the journalling of two pages on the same sector.
**
** subjInMemory
**
**   This is a boolean variable. If true, then any required sub-journal
**   is opened as an in-memory journal file. If false, then in-memory
**   sub-journals are only used for in-memory pager files.
**
**   This variable is updated by the upper layer each time a new
**   write-transaction is opened.
**
** dbSize, dbOrigSize, dbFileSize
**
**   Variable dbSize is set to the number of pages in the database file.
**   It is valid in PAGER_READER and higher states (all states except for
**   OPEN and ERROR).
**
**   dbSize is set based on the size of the database file, which may be
**   larger than the size of the database (the value stored at offset
**   28 of the database header by the btree). If the size of the file
**   is not an integer multiple of the page-size, the value stored in
**   dbSize is rounded down (i.e. a 5KB file with 2K page-size has dbSize==2).
**   Except, any file that is greater than 0 bytes in size is considered
**   to have at least one page. (i.e. a 1KB file with 2K page-size leads
**   to dbSize==1).
**
**   During a write-transaction, if pages with page-numbers greater than
**   dbSize are modified in the cache, dbSize is updated accordingly.
**   Similarly, if the database is truncated using PagerTruncateImage(),
**   dbSize is updated.
**
**   Variables dbOrigSize and dbFileSize are valid in states
**   PAGER_WRITER_LOCKED and higher. dbOrigSize is a copy of the dbSize
**   variable at the start of the transaction. It is used during rollback,
**   and to determine whether or not pages need to be journalled before
**   being modified.
**
**   Throughout a write-transaction, dbFileSize contains the size of
**   the file on disk in pages. It is set to a copy of dbSize when the
**   write-transaction is first opened, and updated when VFS calls are made
**   to write or truncate the database file on disk.
**
**   The only reason the dbFileSize variable is required is to suppress
**   unnecessary calls to xTruncate() after committing a transaction. If,
**   when a transaction is committed, the dbFileSize variable indicates
**   that the database file is larger than the database image (Pager.dbSize),
**   pager_truncate() is called. The pager_truncate() call uses xFilesize()
**   to measure the database file on disk, and then truncates it if required.
**   dbFileSize is not used when rolling back a transaction. In this case
**   pager_truncate() is called unconditionally (which means there may be
**   a call to xFilesize() that is not strictly required). In either case,
**   pager_truncate() may cause the file to become smaller or larger.
**
** dbHintSize
**
**   The dbHintSize variable is used to limit the number of calls made to
**   the VFS xFileControl(FCNTL_SIZE_HINT) method.
**
**   dbHintSize is set to a copy of the dbSize variable when a
**   write-transaction is opened (at the same time as dbFileSize and
**   dbOrigSize). If the xFileControl(FCNTL_SIZE_HINT) method is called,
**   dbHintSize is increased to the number of pages that correspond to the
**   size-hint passed to the method call. See pager_write_pagelist() for
**   details.
**
** errCode
**
**   The Pager.errCode variable is only ever used in PAGER_ERROR state. It
**   is set to zero in all other states. In PAGER_ERROR state, Pager.errCode
**   is always set to SQLITE_FULL, SQLITE_IOERR or one of the SQLITE_IOERR_XXX
**   sub-codes.
**
** syncFlags, walSyncFlags
**
**   syncFlags is either SQLITE_SYNC_NORMAL (0x02) or SQLITE_SYNC_FULL (0x03).
**   syncFlags is used for rollback mode.  walSyncFlags is used for WAL mode
**   and contains the flags used to sync the checkpoint operations in the
**   lower two bits, and sync flags used for transaction commits in the WAL
**   file in bits 0x04 and 0x08.  In other words, to get the correct sync flags
**   for checkpoint operations, use (walSyncFlags&0x03) and to get the correct
**   sync flags for transaction commit, use ((walSyncFlags>>2)&0x03).  Note
**   that with synchronous=NORMAL in WAL mode, transaction commit is not synced
**   meaning that the 0x04 and 0x08 bits are both zero.
*/
struct Pager {
  sqlite3_vfs *pVfs;          /* OS functions to use for IO */
  u8 exclusiveMode;           /* Boolean. True if locking_mode==EXCLUSIVE */
  u8 journalMode;             /* One of the PAGER_JOURNALMODE_* values */
  u8 useJournal;              /* Use a rollback journal on this file */
  u8 noSync;                  /* Do not sync the journal if true */
  u8 fullSync;                /* Do extra syncs of the journal for robustness */
  u8 extraSync;               /* sync directory after journal delete */
  u8 syncFlags;               /* SYNC_NORMAL or SYNC_FULL otherwise */
  u8 walSyncFlags;            /* See description above */
  u8 tempFile;                /* zFilename is a temporary or immutable file */
  u8 noLock;                  /* Do not lock (except in WAL mode) */
  u8 readOnly;                /* True for a read-only database */
  u8 memDb;                   /* True to inhibit all file I/O */
  u8 memVfs;                  /* VFS-implemented memory database */

  /**************************************************************************
  ** The following block contains those class members that change during
  ** routine operation.  Class members not in this block are either fixed
  ** when the pager is first created or else only change when there is a
  ** significant mode change (such as changing the page_size, locking_mode,
  ** or the journal_mode).  From another view, these class members describe
  ** the "state" of the pager, while other class members describe the
  ** "configuration" of the pager.
  */
  u8 eState;                  /* Pager state (OPEN, READER, WRITER_LOCKED..) */
  u8 eLock;                   /* Current lock held on database file */
  u8 changeCountDone;         /* Set after incrementing the change-counter */
  u8 setSuper;                /* Super-jrnl name is written into jrnl */
  u8 doNotSpill;              /* Do not spill the cache when non-zero */
  u8 subjInMemory;            /* True to use in-memory sub-journals */
  u8 bUseFetch;               /* True to use xFetch() */
  u8 hasHeldSharedLock;       /* True if a shared lock has ever been held */
  Pgno dbSize;                /* Number of pages in the database */
  Pgno dbOrigSize;            /* dbSize before the current transaction */
  Pgno dbFileSize;            /* Number of pages in the database file */
  Pgno dbHintSize;            /* Value passed to FCNTL_SIZE_HINT call */
  int errCode;                /* One of several kinds of errors */
  int nRec;                   /* Pages journalled since last j-header written */
  u32 cksumInit;              /* Quasi-random value added to every checksum */
  u32 nSubRec;                /* Number of records written to sub-journal */
  Bitvec *pInJournal;         /* One bit for each page in the database file */
  sqlite3_file *fd;           /* File descriptor for database */
  sqlite3_file *jfd;          /* File descriptor for main journal */
  sqlite3_file *sjfd;         /* File descriptor for sub-journal */
  i64 journalOff;             /* Current write offset in the journal file */
  i64 journalHdr;             /* Byte offset to previous journal header */
  sqlite3_backup *pBackup;    /* Pointer to list of ongoing backup processes */
  PagerSavepoint *aSavepoint; /* Array of active savepoints */
  int nSavepoint;             /* Number of elements in aSavepoint[] */
  u32 iDataVersion;           /* Changes whenever database content changes */
  char dbFileVers[16];        /* Changes whenever database file changes */

  int nMmapOut;               /* Number of mmap pages currently outstanding */
  sqlite3_int64 szMmap;       /* Desired maximum mmap size */
  PgHdr *pMmapFreelist;       /* List of free mmap page headers (pDirty) */
  /*
  ** End of the routinely-changing class members
  ***************************************************************************/

  u16 nExtra;                 /* Add this many bytes to each in-memory page */
  i16 nReserve;               /* Number of unused bytes at end of each page */
  u32 vfsFlags;               /* Flags for sqlite3_vfs.xOpen() */
  u32 sectorSize;             /* Assumed sector size during rollback */
  Pgno mxPgno;                /* Maximum allowed size of the database */
  Pgno lckPgno;               /* Page number for the locking page */
  i64 pageSize;               /* Number of bytes in a page */
  i64 journalSizeLimit;       /* Size limit for persistent journal files */
  char *zFilename;            /* Name of the database file */
  char *zJournal;             /* Name of the journal file */
  int (*xBusyHandler)(void*); /* Function to call when busy */
  void *pBusyHandlerArg;      /* Context argument for xBusyHandler */
  u32 aStat[4];               /* Total cache hits, misses, writes, spills */
#ifdef SQLITE_TEST
  int nRead;                  /* Database pages read */
#endif
  void (*xReiniter)(DbPage*); /* Call this routine when reloading pages */
  int (*xGet)(Pager*,Pgno,DbPage**,int); /* Routine to fetch a patch */
  char *pTmpSpace;            /* Pager.pageSize bytes of space for tmp use */
  PCache *pPCache;            /* Pointer to page cache object */
#ifndef SQLITE_OMIT_WAL
  Wal *pWal;                  /* Write-ahead log used by "journal_mode=wal" */
  char *zWal;                 /* File name for write-ahead log */
#endif
};

/*
** Indexes for use with Pager.aStat[]. The Pager.aStat[] array contains
** the values accessed by passing SQLITE_DBSTATUS_CACHE_HIT, CACHE_MISS
** or CACHE_WRITE to sqlite3_db_status().
*/
#define PAGER_STAT_HIT   0
#define PAGER_STAT_MISS  1
#define PAGER_STAT_WRITE 2
#define PAGER_STAT_SPILL 3

/*
** The following global variables hold counters used for
** testing purposes only.  These variables do not exist in
** a non-testing build.  These variables are not thread-safe.
*/
#ifdef SQLITE_TEST
SQLITE_API int sqlite3_pager_readdb_count = 0;    /* Number of full pages read from DB */
SQLITE_API int sqlite3_pager_writedb_count = 0;   /* Number of full pages written to DB */
SQLITE_API int sqlite3_pager_writej_count = 0;    /* Number of pages written to journal */
# define PAGER_INCR(v)  v++
#else
# define PAGER_INCR(v)
#endif



/*
** Journal files begin with the following magic string.  The data
** was obtained from /dev/random.  It is used only as a sanity check.
**
** Since version 2.8.0, the journal format contains additional sanity
** checking information.  If the power fails while the journal is being
** written, semi-random garbage data might appear in the journal
** file after power is restored.  If an attempt is then made
** to roll the journal back, the database could be corrupted.  The additional
** sanity checking data is an attempt to discover the garbage in the
** journal and ignore it.
**
** The sanity checking information for the new journal format consists
** of a 32-bit checksum on each page of data.  The checksum covers both
** the page number and the pPager->pageSize bytes of data for the page.
** This cksum is initialized to a 32-bit random value that appears in the
** journal file right after the header.  The random initializer is important,
** because garbage data that appears at the end of a journal is likely
** data that was once in other files that have now been deleted.  If the
** garbage data came from an obsolete journal file, the checksums might
** be correct.  But by initializing the checksum to random value which
** is different for every journal, we minimize that risk.
*/
static const unsigned char aJournalMagic[] = {
  0xd9, 0xd5, 0x05, 0xf9, 0x20, 0xa1, 0x63, 0xd7,
};

/*
** The size of the of each page record in the journal is given by
** the following macro.
*/
#define JOURNAL_PG_SZ(pPager)  ((pPager->pageSize) + 8)

/*
** The journal header size for this pager. This is usually the same
** size as a single disk sector. See also setSectorSize().
*/
#define JOURNAL_HDR_SZ(pPager) (pPager->sectorSize)

/*
** The macro MEMDB is true if we are dealing with an in-memory database.
** We do this as a macro so that if the SQLITE_OMIT_MEMORYDB macro is set,
** the value of MEMDB will be a constant and the compiler will optimize
** out code that would never execute.
*/
#ifdef SQLITE_OMIT_MEMORYDB
# define MEMDB 0
#else
# define MEMDB pPager->memDb
#endif

/*
** The macro USEFETCH is true if we are allowed to use the xFetch and xUnfetch
** interfaces to access the database using memory-mapped I/O.
*/
#if SQLITE_MAX_MMAP_SIZE>0
# define USEFETCH(x) ((x)->bUseFetch)
#else
# define USEFETCH(x) 0
#endif

/*
** The argument to this macro is a file descriptor (type sqlite3_file*).
** Return 0 if it is not open, or non-zero (but not 1) if it is.
**
** This is so that expressions can be written as:
**
**   if( isOpen(pPager->jfd) ){ ...
**
** instead of
**
**   if( pPager->jfd->pMethods ){ ...
*/
#define isOpen(pFd) ((pFd)->pMethods!=0)

#ifdef SQLITE_DIRECT_OVERFLOW_READ
/*
** Return true if page pgno can be read directly from the database file
** by the b-tree layer. This is the case if:
**
**   * the database file is open,
**   * there are no dirty pages in the cache, and
**   * the desired page is not currently in the wal file.
*/
SQLITE_PRIVATE int sqlite3PagerDirectReadOk(Pager *pPager, Pgno pgno){
  if( pPager->fd->pMethods==0 ) return 0;
  if( sqlite3PCacheIsDirty(pPager->pPCache) ) return 0;
#ifndef SQLITE_OMIT_WAL
  if( pPager->pWal ){
    u32 iRead = 0;
    (void)sqlite3WalFindFrame(pPager->pWal, pgno, &iRead);
    return iRead==0;
  }
#endif
  return 1;
}
