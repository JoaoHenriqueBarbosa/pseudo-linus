/*
** 2001 September 15
**
** The author disclaims copyright to this source code.  In place of
** a legal notice, here is a blessing:
**
**    May you do good and not evil.
**    May you find forgiveness for yourself and forgive others.
**    May you share freely, never taking more than you give.
**
*************************************************************************
** This is the implementation of the page cache subsystem or "pager".
**
** The pager is used to access a database disk file.  It implements
** atomic commit and rollback through the use of a journal file that
** is separate from the database file.  The pager also implements file
** locking to prevent two processes from writing the same database
** file simultaneously, or one process from reading the database while
** another is writing.
*/
#ifndef SQLITE_OMIT_DISKIO
/* #include "sqliteInt.h" */
/************** Include wal.h in the middle of pager.c ***********************/


/******************* NOTES ON THE DESIGN OF THE PAGER ************************
**
** This comment block describes invariants that hold when using a rollback
** journal.  These invariants do not apply for journal_mode=WAL,
** journal_mode=MEMORY, or journal_mode=OFF.
**
** Within this comment block, a page is deemed to have been synced
** automatically as soon as it is written when PRAGMA synchronous=OFF.
** Otherwise, the page is not synced until the xSync method of the VFS
** is called successfully on the file containing the page.
**
** Definition:  A page of the database file is said to be "overwriteable" if
** one or more of the following are true about the page:
**
**     (a)  The original content of the page as it was at the beginning of
**          the transaction has been written into the rollback journal and
**          synced.
**
**     (b)  The page was a freelist leaf page at the start of the transaction.
**
**     (c)  The page number is greater than the largest page that existed in
**          the database file at the start of the transaction.
**
** (1) A page of the database file is never overwritten unless one of the
**     following are true:
**
**     (a) The page and all other pages on the same sector are overwriteable.
**
**     (b) The atomic page write optimization is enabled, and the entire
**         transaction other than the update of the transaction sequence
**         number consists of a single page change.
**
** (2) The content of a page written into the rollback journal exactly matches
**     both the content in the database when the rollback journal was written
**     and the content in the database at the beginning of the current
**     transaction.
**
** (3) Writes to the database file are an integer multiple of the page size
**     in length and are aligned on a page boundary.
**
** (4) Reads from the database file are either aligned on a page boundary and
**     an integer multiple of the page size in length or are taken from the
**     first 100 bytes of the database file.
**
** (5) All writes to the database file are synced prior to the rollback journal
**     being deleted, truncated, or zeroed.
**
** (6) If a super-journal file is used, then all writes to the database file
**     are synced prior to the super-journal being deleted.
**
** Definition: Two databases (or the same database at two points it time)
** are said to be "logically equivalent" if they give the same answer to
** all queries.  Note in particular the content of freelist leaf
** pages can be changed arbitrarily without affecting the logical equivalence
** of the database.
**
** (7) At any time, if any subset, including the empty set and the total set,
**     of the unsynced changes to a rollback journal are removed and the
**     journal is rolled back, the resulting database file will be logically
**     equivalent to the database file at the beginning of the transaction.
**
** (8) When a transaction is rolled back, the xTruncate method of the VFS
**     is called to restore the database file to the same size it was at
**     the beginning of the transaction.  (In some VFSes, the xTruncate
**     method is a no-op, but that does not change the fact the SQLite will
**     invoke it.)
**
** (9) Whenever the database file is modified, at least one bit in the range
**     of bytes from 24 through 39 inclusive will be changed prior to releasing
**     the EXCLUSIVE lock, thus signaling other connections on the same
**     database to flush their caches.
**
** (10) The pattern of bits in bytes 24 through 39 shall not repeat in less
**      than one billion transactions.
**
** (11) A database file is well-formed at the beginning and at the conclusion
**      of every transaction.
**
** (12) An EXCLUSIVE lock is held on the database file when writing to
**      the database file.
**
** (13) A SHARED lock is held on the database file while reading any
**      content out of the database file.
**
******************************************************************************/

/*
** Macros for troubleshooting.  Normally turned off
*/
#if 0
int sqlite3PagerTrace=1;  /* True to enable tracing */
#define sqlite3DebugPrintf printf
#define PAGERTRACE(X)     if( sqlite3PagerTrace ){ sqlite3DebugPrintf X; }
#else
#define PAGERTRACE(X)
#endif

/*
** The following two macros are used within the PAGERTRACE() macros above
** to print out file-descriptors.
**
** PAGERID() takes a pointer to a Pager struct as its argument. The
** associated file-descriptor is returned. FILEHANDLEID() takes an sqlite3_file
** struct as its argument.
*/
#define PAGERID(p) (SQLITE_PTR_TO_INT(p->fd))
#define FILEHANDLEID(fd) (SQLITE_PTR_TO_INT(fd))

/*
** The Pager.eState variable stores the current 'state' of a pager. A
** pager may be in any one of the seven states shown in the following
** state diagram.
**
**                            OPEN <------+------+
**                              |         |      |
**                              V         |      |
**               +---------> READER-------+      |
**               |              |                |
**               |              V                |
**               |<-------WRITER_LOCKED------> ERROR
**               |              |                ^
**               |              V                |
**               |<------WRITER_CACHEMOD-------->|
**               |              |                |
**               |              V                |
**               |<-------WRITER_DBMOD---------->|
**               |              |                |
**               |              V                |
**               +<------WRITER_FINISHED-------->+
**
**
** List of state transitions and the C [function] that performs each:
**
**   OPEN              -> READER              [sqlite3PagerSharedLock]
**   READER            -> OPEN                [pager_unlock]
**
**   READER            -> WRITER_LOCKED       [sqlite3PagerBegin]
**   WRITER_LOCKED     -> WRITER_CACHEMOD     [pager_open_journal]
**   WRITER_CACHEMOD   -> WRITER_DBMOD        [syncJournal]
**   WRITER_DBMOD      -> WRITER_FINISHED     [sqlite3PagerCommitPhaseOne]
**   WRITER_***        -> READER              [pager_end_transaction]
**
**   WRITER_***        -> ERROR               [pager_error]
**   ERROR             -> OPEN                [pager_unlock]
**
**
**  OPEN:
**
**    The pager starts up in this state. Nothing is guaranteed in this
**    state - the file may or may not be locked and the database size is
**    unknown. The database may not be read or written.
**
**    * No read or write transaction is active.
**    * Any lock, or no lock at all, may be held on the database file.
**    * The dbSize, dbOrigSize and dbFileSize variables may not be trusted.
**
**  READER:
**
**    In this state all the requirements for reading the database in
**    rollback (non-WAL) mode are met. Unless the pager is (or recently
**    was) in exclusive-locking mode, a user-level read transaction is
**    open. The database size is known in this state.
**
**    A connection running with locking_mode=normal enters this state when
**    it opens a read-transaction on the database and returns to state
**    OPEN after the read-transaction is completed. However a connection
**    running in locking_mode=exclusive (including temp databases) remains in
**    this state even after the read-transaction is closed. The only way
**    a locking_mode=exclusive connection can transition from READER to OPEN
**    is via the ERROR state (see below).
**
**    * A read transaction may be active (but a write-transaction cannot).
**    * A SHARED or greater lock is held on the database file.
**    * The dbSize variable may be trusted (even if a user-level read
**      transaction is not active). The dbOrigSize and dbFileSize variables
**      may not be trusted at this point.
**    * If the database is a WAL database, then the WAL connection is open.
**    * Even if a read-transaction is not open, it is guaranteed that
**      there is no hot-journal in the file-system.
**
**  WRITER_LOCKED:
**
**    The pager moves to this state from READER when a write-transaction
**    is first opened on the database. In WRITER_LOCKED state, all locks
**    required to start a write-transaction are held, but no actual
**    modifications to the cache or database have taken place.
**
**    In rollback mode, a RESERVED or (if the transaction was opened with
**    BEGIN EXCLUSIVE) EXCLUSIVE lock is obtained on the database file when
**    moving to this state, but the journal file is not written to or opened
**    to in this state. If the transaction is committed or rolled back while
**    in WRITER_LOCKED state, all that is required is to unlock the database
**    file.
**
**    IN WAL mode, WalBeginWriteTransaction() is called to lock the log file.
**    If the connection is running with locking_mode=exclusive, an attempt
**    is made to obtain an EXCLUSIVE lock on the database file.
**
**    * A write transaction is active.
**    * If the connection is open in rollback-mode, a RESERVED or greater
**      lock is held on the database file.
**    * If the connection is open in WAL-mode, a WAL write transaction
**      is open (i.e. sqlite3WalBeginWriteTransaction() has been successfully
**      called).
**    * The dbSize, dbOrigSize and dbFileSize variables are all valid.
**    * The contents of the pager cache have not been modified.
**    * The journal file may or may not be open.
**    * Nothing (not even the first header) has been written to the journal.
**
**  WRITER_CACHEMOD:
**
**    A pager moves from WRITER_LOCKED state to this state when a page is
**    first modified by the upper layer. In rollback mode the journal file
**    is opened (if it is not already open) and a header written to the
**    start of it. The database file on disk has not been modified.
**
**    * A write transaction is active.
**    * A RESERVED or greater lock is held on the database file.
**    * The journal file is open and the first header has been written
**      to it, but the header has not been synced to disk.
**    * The contents of the page cache have been modified.
**
**  WRITER_DBMOD:
**
**    The pager transitions from WRITER_CACHEMOD into WRITER_DBMOD state
**    when it modifies the contents of the database file. WAL connections
**    never enter this state (since they do not modify the database file,
**    just the log file).
**
**    * A write transaction is active.
**    * An EXCLUSIVE or greater lock is held on the database file.
**    * The journal file is open and the first header has been written
**      and synced to disk.
**    * The contents of the page cache have been modified (and possibly
**      written to disk).
**
**  WRITER_FINISHED:
**
**    It is not possible for a WAL connection to enter this state.
**
**    A rollback-mode pager changes to WRITER_FINISHED state from WRITER_DBMOD
**    state after the entire transaction has been successfully written into the
**    database file. In this state the transaction may be committed simply
**    by finalizing the journal file. Once in WRITER_FINISHED state, it is
**    not possible to modify the database further. At this point, the upper
**    layer must either commit or rollback the transaction.
**
**    * A write transaction is active.
**    * An EXCLUSIVE or greater lock is held on the database file.
**    * All writing and syncing of journal and database data has finished.
**      If no error occurred, all that remains is to finalize the journal to
**      commit the transaction. If an error did occur, the caller will need
**      to rollback the transaction.
**
**  ERROR:
**
**    The ERROR state is entered when an IO or disk-full error (including
**    SQLITE_IOERR_NOMEM) occurs at a point in the code that makes it
**    difficult to be sure that the in-memory pager state (cache contents,
**    db size etc.) are consistent with the contents of the file-system.
**
**    Temporary pager files may enter the ERROR state, but in-memory pagers
**    cannot.
**
**    For example, if an IO error occurs while performing a rollback,
**    the contents of the page-cache may be left in an inconsistent state.
**    At this point it would be dangerous to change back to READER state
**    (as usually happens after a rollback). Any subsequent readers might
**    report database corruption (due to the inconsistent cache), and if
**    they upgrade to writers, they may inadvertently corrupt the database
**    file. To avoid this hazard, the pager switches into the ERROR state
**    instead of READER following such an error.
**
**    Once it has entered the ERROR state, any attempt to use the pager
**    to read or write data returns an error. Eventually, once all
**    outstanding transactions have been abandoned, the pager is able to
**    transition back to OPEN state, discarding the contents of the
**    page-cache and any other in-memory state at the same time. Everything
**    is reloaded from disk (and, if necessary, hot-journal rollback performed)
**    when a read-transaction is next opened on the pager (transitioning
**    the pager into READER state). At that point the system has recovered
**    from the error.
**
**    Specifically, the pager jumps into the ERROR state if:
**
**      1. An error occurs while attempting a rollback. This happens in
**         function sqlite3PagerRollback().
**
**      2. An error occurs while attempting to finalize a journal file
**         following a commit in function sqlite3PagerCommitPhaseTwo().
**
**      3. An error occurs while attempting to write to the journal or
**         database file in function pagerStress() in order to free up
**         memory.
**
**    In other cases, the error is returned to the b-tree layer. The b-tree
**    layer then attempts a rollback operation. If the error condition
**    persists, the pager enters the ERROR state via condition (1) above.
**
**    Condition (3) is necessary because it can be triggered by a read-only
**    statement executed within a transaction. In this case, if the error
**    code were simply returned to the user, the b-tree layer would not
**    automatically attempt a rollback, as it assumes that an error in a
**    read-only statement cannot leave the pager in an internally inconsistent
**    state.
**
**    * The Pager.errCode variable is set to something other than SQLITE_OK.
**    * There are one or more outstanding references to pages (after the
**      last reference is dropped the pager should move back to OPEN state).
**    * The pager is not an in-memory pager.
**
**
** Notes:
**
**   * A pager is never in WRITER_DBMOD or WRITER_FINISHED state if the
**     connection is open in WAL mode. A WAL connection is always in one
**     of the first four states.
**
**   * Normally, a connection open in exclusive mode is never in PAGER_OPEN
**     state. There are two exceptions: immediately after exclusive-mode has
**     been turned on (and before any read or write transactions are
**     executed), and when the pager is leaving the "error state".
**
**   * See also: assert_pager_state().
*/
#define PAGER_OPEN                  0
#define PAGER_READER                1
#define PAGER_WRITER_LOCKED         2
#define PAGER_WRITER_CACHEMOD       3
#define PAGER_WRITER_DBMOD          4
#define PAGER_WRITER_FINISHED       5
#define PAGER_ERROR                 6

/*
** The Pager.eLock variable is almost always set to one of the
** following locking-states, according to the lock currently held on
** the database file: NO_LOCK, SHARED_LOCK, RESERVED_LOCK or EXCLUSIVE_LOCK.
** This variable is kept up to date as locks are taken and released by
** the pagerLockDb() and pagerUnlockDb() wrappers.
**
** If the VFS xLock() or xUnlock() returns an error other than SQLITE_BUSY
** (i.e. one of the SQLITE_IOERR subtypes), it is not clear whether or not
** the operation was successful. In these circumstances pagerLockDb() and
** pagerUnlockDb() take a conservative approach - eLock is always updated
** when unlocking the file, and only updated when locking the file if the
** VFS call is successful. This way, the Pager.eLock variable may be set
** to a less exclusive (lower) value than the lock that is actually held
** at the system level, but it is never set to a more exclusive value.
**
** This is usually safe. If an xUnlock fails or appears to fail, there may
** be a few redundant xLock() calls or a lock may be held for longer than
** required, but nothing really goes wrong.
**
** The exception is when the database file is unlocked as the pager moves
** from ERROR to OPEN state. At this point there may be a hot-journal file
** in the file-system that needs to be rolled back (as part of an OPEN->SHARED
** transition, by the same pager or any other). If the call to xUnlock()
** fails at this point and the pager is left holding an EXCLUSIVE lock, this
** can confuse the call to xCheckReservedLock() call made later as part
** of hot-journal detection.
**
** xCheckReservedLock() is defined as returning true "if there is a RESERVED
** lock held by this process or any others". So xCheckReservedLock may
** return true because the caller itself is holding an EXCLUSIVE lock (but
** doesn't know it because of a previous error in xUnlock). If this happens
** a hot-journal may be mistaken for a journal being created by an active
** transaction in another process, causing SQLite to read from the database
** without rolling it back.
**
** To work around this, if a call to xUnlock() fails when unlocking the
** database in the ERROR state, Pager.eLock is set to UNKNOWN_LOCK. It
** is only changed back to a real locking state after a successful call
** to xLock(EXCLUSIVE). Also, the code to do the OPEN->SHARED state transition
** omits the check for a hot-journal if Pager.eLock is set to UNKNOWN_LOCK
** lock. Instead, it assumes a hot-journal exists and obtains an EXCLUSIVE
** lock on the database file before attempting to roll it back. See function
** PagerSharedLock() for more detail.
**
** Pager.eLock may only be set to UNKNOWN_LOCK when the pager is in
** PAGER_OPEN state.
*/
#define UNKNOWN_LOCK                (EXCLUSIVE_LOCK+1)

/*
** The maximum allowed sector size. 64KiB. If the xSectorsize() method
** returns a value larger than this, then MAX_SECTOR_SIZE is used instead.
** This could conceivably cause corruption following a power failure on
** such a system. This is currently an undocumented limit.
*/
#define MAX_SECTOR_SIZE 0x10000


/*
** An instance of the following structure is allocated for each active
** savepoint and statement transaction in the system. All such structures
** are stored in the Pager.aSavepoint[] array, which is allocated and
** resized using sqlite3Realloc().
**
** When a savepoint is created, the PagerSavepoint.iHdrOffset field is
** set to 0. If a journal-header is written into the main journal while
** the savepoint is active, then iHdrOffset is set to the byte offset
** immediately following the last journal record written into the main
** journal before the journal-header. This is required during savepoint
** rollback (see pagerPlaybackSavepoint()).
*/
typedef struct PagerSavepoint PagerSavepoint;
struct PagerSavepoint {
  i64 iOffset;                 /* Starting offset in main journal */
  i64 iHdrOffset;              /* See above */
  Bitvec *pInSavepoint;        /* Set of pages in this savepoint */
  Pgno nOrig;                  /* Original number of pages in file */
  Pgno iSubRec;                /* Index of first record in sub-journal */
  int bTruncateOnRelease;      /* If stmt journal may be truncated on RELEASE */
#ifndef SQLITE_OMIT_WAL
  u32 aWalData[WAL_SAVEPOINT_NDATA];        /* WAL savepoint context */
#endif
};
