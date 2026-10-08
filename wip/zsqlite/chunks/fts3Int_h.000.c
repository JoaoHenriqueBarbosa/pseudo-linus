/*
** 2009 Nov 12
**
** The author disclaims copyright to this source code.  In place of
** a legal notice, here is a blessing:
**
**    May you do good and not evil.
**    May you find forgiveness for yourself and forgive others.
**    May you share freely, never taking more than you give.
**
******************************************************************************
**
*/
#ifndef _FTSINT_H
#define _FTSINT_H

#if !defined(NDEBUG) && !defined(SQLITE_DEBUG)
# define NDEBUG 1
#endif

/* FTS3/FTS4 require virtual tables */
#ifdef SQLITE_OMIT_VIRTUALTABLE
# undef SQLITE_ENABLE_FTS3
# undef SQLITE_ENABLE_FTS4
#endif

/*
** FTS4 is really an extension for FTS3.  It is enabled using the
** SQLITE_ENABLE_FTS3 macro.  But to avoid confusion we also all
** the SQLITE_ENABLE_FTS4 macro to serve as an alisse for SQLITE_ENABLE_FTS3.
*/
#if defined(SQLITE_ENABLE_FTS4) && !defined(SQLITE_ENABLE_FTS3)
# define SQLITE_ENABLE_FTS3
#endif

#if !defined(SQLITE_CORE) || defined(SQLITE_ENABLE_FTS3)

/* If not building as part of the core, include sqlite3ext.h. */
#ifndef SQLITE_CORE
/* # include "sqlite3ext.h" */
SQLITE_EXTENSION_INIT3
#endif

/* #include "sqlite3.h" */
/************** Include fts3_tokenizer.h in the middle of fts3Int.h **********/
/************** Include fts3_hash.h in the middle of fts3Int.h ***************/

/*
** This constant determines the maximum depth of an FTS expression tree
** that the library will create and use. FTS uses recursion to perform
** various operations on the query tree, so the disadvantage of a large
** limit is that it may allow very large queries to use large amounts
** of stack space (perhaps causing a stack overflow).
*/
#ifndef SQLITE_FTS3_MAX_EXPR_DEPTH
# define SQLITE_FTS3_MAX_EXPR_DEPTH 12
#endif


/*
** This constant controls how often segments are merged. Once there are
** FTS3_MERGE_COUNT segments of level N, they are merged into a single
** segment of level N+1.
*/
#define FTS3_MERGE_COUNT 16

/*
** This is the maximum amount of data (in bytes) to store in the
** Fts3Table.pendingTerms hash table. Normally, the hash table is
** populated as documents are inserted/updated/deleted in a transaction
** and used to create a new segment when the transaction is committed.
** However if this limit is reached midway through a transaction, a new
** segment is created and the hash table cleared immediately.
*/
#define FTS3_MAX_PENDING_DATA (1*1024*1024)

/*
** Macro to return the number of elements in an array. SQLite has a
** similar macro called ArraySize(). Use a different name to avoid
** a collision when building an amalgamation with built-in FTS3.
*/
#define SizeofArray(X) ((int)(sizeof(X)/sizeof(X[0])))


#ifndef MIN
# define MIN(x,y) ((x)<(y)?(x):(y))
#endif
#ifndef MAX
# define MAX(x,y) ((x)>(y)?(x):(y))
#endif

/*
** Maximum length of a varint encoded integer. The varint format is different
** from that used by SQLite, so the maximum length is 10, not 9.
*/
#define FTS3_VARINT_MAX 10

#define FTS3_BUFFER_PADDING 8

/*
** FTS4 virtual tables may maintain multiple indexes - one index of all terms
** in the document set and zero or more prefix indexes. All indexes are stored
** as one or more b+-trees in the %_segments and %_segdir tables.
**
** It is possible to determine which index a b+-tree belongs to based on the
** value stored in the "%_segdir.level" column. Given this value L, the index
** that the b+-tree belongs to is (L<<10). In other words, all b+-trees with
** level values between 0 and 1023 (inclusive) belong to index 0, all levels
** between 1024 and 2047 to index 1, and so on.
**
** It is considered impossible for an index to use more than 1024 levels. In
** theory though this may happen, but only after at least
** (FTS3_MERGE_COUNT^1024) separate flushes of the pending-terms tables.
*/
#define FTS3_SEGDIR_MAXLEVEL      1024
#define FTS3_SEGDIR_MAXLEVEL_STR "1024"

/*
** The testcase() macro is only used by the amalgamation.  If undefined,
** make it a no-op.
*/
#ifndef testcase
# define testcase(X)
#endif

/*
** Terminator values for position-lists and column-lists.
*/
#define POS_COLUMN  (1)     /* Column-list terminator */
#define POS_END     (0)     /* Position-list terminator */

/*
** The assert_fts3_nc() macro is similar to the assert() macro, except that it
** is used for assert() conditions that are true only if it can be
** guranteed that the database is not corrupt.
*/
#ifdef SQLITE_DEBUG
SQLITE_API extern int sqlite3_fts3_may_be_corrupt;
# define assert_fts3_nc(x) assert(sqlite3_fts3_may_be_corrupt || (x))
#else
# define assert_fts3_nc(x) assert(x)
#endif

/*
** This section provides definitions to allow the
** FTS3 extension to be compiled outside of the
** amalgamation.
*/
#ifndef SQLITE_AMALGAMATION
/*
** Macros indicating that conditional expressions are always true or
** false.
*/
#if defined(SQLITE_COVERAGE_TEST) || defined(SQLITE_MUTATION_TEST)
# define SQLITE_OMIT_AUXILIARY_SAFETY_CHECKS 1
#endif
#if defined(SQLITE_OMIT_AUXILIARY_SAFETY_CHECKS)
# define ALWAYS(X)      (1)
# define NEVER(X)       (0)
#elif !defined(NDEBUG)
# define ALWAYS(X)      ((X)?1:(assert(0),0))
# define NEVER(X)       ((X)?(assert(0),1):0)
#else
# define ALWAYS(X)      (X)
# define NEVER(X)       (X)
#endif

/*
** Internal types used by SQLite.
*/
typedef unsigned char u8;         /* 1-byte (or larger) unsigned integer */
typedef short int i16;            /* 2-byte (or larger) signed integer */
typedef unsigned int u32;         /* 4-byte unsigned integer */
typedef sqlite3_uint64 u64;       /* 8-byte unsigned integer */
typedef sqlite3_int64 i64;        /* 8-byte signed integer */

/*
** Macro used to suppress compiler warnings for unused parameters.
*/
#define UNUSED_PARAMETER(x) (void)(x)

/*
** Activate assert() only if SQLITE_TEST is enabled.
*/
#if !defined(NDEBUG) && !defined(SQLITE_DEBUG)
# define NDEBUG 1
#endif

/*
** The TESTONLY macro is used to enclose variable declarations or
** other bits of code that are needed to support the arguments
** within testcase() and assert() macros.
*/
#if defined(SQLITE_DEBUG) || defined(SQLITE_COVERAGE_TEST)
# define TESTONLY(X)  X
#else
# define TESTONLY(X)
#endif

#define LARGEST_INT64  (0xffffffff|(((i64)0x7fffffff)<<32))
#define SMALLEST_INT64 (((i64)-1) - LARGEST_INT64)

#define deliberate_fall_through

#endif /* SQLITE_AMALGAMATION */

#ifdef SQLITE_DEBUG
SQLITE_PRIVATE int sqlite3Fts3Corrupt(void);
# define FTS_CORRUPT_VTAB sqlite3Fts3Corrupt()
#else
# define FTS_CORRUPT_VTAB SQLITE_CORRUPT_VTAB
#endif

typedef struct Fts3Table Fts3Table;
typedef struct Fts3Cursor Fts3Cursor;
typedef struct Fts3Expr Fts3Expr;
typedef struct Fts3Phrase Fts3Phrase;
typedef struct Fts3PhraseToken Fts3PhraseToken;

typedef struct Fts3Doclist Fts3Doclist;
typedef struct Fts3SegFilter Fts3SegFilter;
typedef struct Fts3DeferredToken Fts3DeferredToken;
typedef struct Fts3SegReader Fts3SegReader;
typedef struct Fts3MultiSegReader Fts3MultiSegReader;

typedef struct MatchinfoBuffer MatchinfoBuffer;

/*
** A connection to a fulltext index is an instance of the following
** structure. The xCreate and xConnect methods create an instance
** of this structure and xDestroy and xDisconnect free that instance.
** All other methods receive a pointer to the structure as one of their
** arguments.
*/
struct Fts3Table {
  sqlite3_vtab base;              /* Base class used by SQLite core */
  sqlite3 *db;                    /* The database connection */
  const char *zDb;                /* logical database name */
  const char *zName;              /* virtual table name */
  int nColumn;                    /* number of named columns in virtual table */
  char **azColumn;                /* column names.  malloced */
  u8 *abNotindexed;               /* True for 'notindexed' columns */
  sqlite3_tokenizer *pTokenizer;  /* tokenizer for inserts and queries */
  char *zContentTbl;              /* content=xxx option, or NULL */
  char *zLanguageid;              /* languageid=xxx option, or NULL */
  int nAutoincrmerge;             /* Value configured by 'automerge' */
  u32 nLeafAdd;                   /* Number of leaf blocks added this trans */
  int bLock;                      /* Used to prevent recursive content= tbls */

  /* Precompiled statements used by the implementation. Each of these
  ** statements is run and reset within a single virtual table API call.
  */
  sqlite3_stmt *aStmt[40];
  sqlite3_stmt *pSeekStmt;        /* Cache for fts3CursorSeekStmt() */

  char *zReadExprlist;
  char *zWriteExprlist;

  int nNodeSize;                  /* Soft limit for node size */
  u8 bFts4;                       /* True for FTS4, false for FTS3 */
  u8 bHasStat;                    /* True if %_stat table exists (2==unknown) */
  u8 bHasDocsize;                 /* True if %_docsize table exists */
  u8 bDescIdx;                    /* True if doclists are in reverse order */
  u8 bIgnoreSavepoint;            /* True to ignore xSavepoint invocations */
  int nPgsz;                      /* Page size for host database */
  char *zSegmentsTbl;             /* Name of %_segments table */
  sqlite3_blob *pSegments;        /* Blob handle open on %_segments table */
  int iSavepoint;

  /*
  ** The following array of hash tables is used to buffer pending index
  ** updates during transactions. All pending updates buffered at any one
  ** time must share a common language-id (see the FTS4 langid= feature).
  ** The current language id is stored in variable iPrevLangid.
  **
  ** A single FTS4 table may have multiple full-text indexes. For each index
  ** there is an entry in the aIndex[] array. Index 0 is an index of all the
  ** terms that appear in the document set. Each subsequent index in aIndex[]
  ** is an index of prefixes of a specific length.
  **
  ** Variable nPendingData contains an estimate the memory consumed by the
  ** pending data structures, including hash table overhead, but not including
  ** malloc overhead.  When nPendingData exceeds nMaxPendingData, all hash
  ** tables are flushed to disk. Variable iPrevDocid is the docid of the most
  ** recently inserted record.
  */
  int nIndex;                     /* Size of aIndex[] */
  struct Fts3Index {
    int nPrefix;                  /* Prefix length (0 for main terms index) */
    Fts3Hash hPending;            /* Pending terms table for this index */
  } *aIndex;
  int nMaxPendingData;            /* Max pending data before flush to disk */
  int nPendingData;               /* Current bytes of pending data */
  sqlite_int64 iPrevDocid;        /* Docid of most recently inserted document */
  int iPrevLangid;                /* Langid of recently inserted document */
  int bPrevDelete;                /* True if last operation was a delete */

#if defined(SQLITE_DEBUG) || defined(SQLITE_COVERAGE_TEST)
  /* State variables used for validating that the transaction control
  ** methods of the virtual table are called at appropriate times.  These
  ** values do not contribute to FTS functionality; they are used for
  ** verifying the operation of the SQLite core.
  */
  int inTransaction;     /* True after xBegin but before xCommit/xRollback */
  int mxSavepoint;       /* Largest valid xSavepoint integer */
#endif

#if defined(SQLITE_DEBUG) || defined(SQLITE_TEST)
  /* True to disable the incremental doclist optimization. This is controled
  ** by special insert command 'test-no-incr-doclist'.  */
  int bNoIncrDoclist;

  /* Number of segments in a level */
  int nMergeCount;
#endif
};

/* Macro to find the number of segments to merge */
#if defined(SQLITE_DEBUG) || defined(SQLITE_TEST)
# define MergeCount(P) ((P)->nMergeCount)
#else
# define MergeCount(P) FTS3_MERGE_COUNT
#endif

/*
** When the core wants to read from the virtual table, it creates a
** virtual table cursor (an instance of the following structure) using
** the xOpen method. Cursors are destroyed using the xClose method.
*/
struct Fts3Cursor {
  sqlite3_vtab_cursor base;       /* Base class used by SQLite core */
  i16 eSearch;                    /* Search strategy (see below) */
  u8 isEof;                       /* True if at End Of Results */
  u8 isRequireSeek;               /* True if must seek pStmt to %_content row */
  u8 bSeekStmt;                   /* True if pStmt is a seek */
  sqlite3_stmt *pStmt;            /* Prepared statement in use by the cursor */
  Fts3Expr *pExpr;                /* Parsed MATCH query string */
  int iLangid;                    /* Language being queried for */
  int nPhrase;                    /* Number of matchable phrases in query */
  Fts3DeferredToken *pDeferred;   /* Deferred search tokens, if any */
  sqlite3_int64 iPrevId;          /* Previous id read from aDoclist */
  char *pNextId;                  /* Pointer into the body of aDoclist */
  char *aDoclist;                 /* List of docids for full-text queries */
  int nDoclist;                   /* Size of buffer at aDoclist */
  u8 bDesc;                       /* True to sort in descending order */
  int eEvalmode;                  /* An FTS3_EVAL_XX constant */
  int nRowAvg;                    /* Average size of database rows, in pages */
  sqlite3_int64 nDoc;             /* Documents in table */
  i64 iMinDocid;                  /* Minimum docid to return */
  i64 iMaxDocid;                  /* Maximum docid to return */
  int isMatchinfoNeeded;          /* True when aMatchinfo[] needs filling in */
  MatchinfoBuffer *pMIBuffer;     /* Buffer for matchinfo data */
};
