


/**************************************************************************
** Interface to code in fts5_config.c. fts5_config.c contains contains code
** to parse the arguments passed to the CREATE VIRTUAL TABLE statement.
*/

typedef struct Fts5Config Fts5Config;

/*
** An instance of the following structure encodes all information that can
** be gleaned from the CREATE VIRTUAL TABLE statement.
**
** And all information loaded from the %_config table.
**
** nAutomerge:
**   The minimum number of segments that an auto-merge operation should
**   attempt to merge together. A value of 1 sets the object to use the
**   compile time default. Zero disables auto-merge altogether.
**
** bContentlessDelete:
**   True if the contentless_delete option was present in the CREATE
**   VIRTUAL TABLE statement.
**
** zContent:
**
** zContentRowid:
**   The value of the content_rowid= option, if one was specified. Or
**   the string "rowid" otherwise. This text is not quoted - if it is
**   used as part of an SQL statement it needs to be quoted appropriately.
**
** zContentExprlist:
**
** pzErrmsg:
**   This exists in order to allow the fts5_index.c module to return a
**   decent error message if it encounters a file-format version it does
**   not understand.
**
** bColumnsize:
**   True if the %_docsize table is created.
**
** bPrefixIndex:
**   This is only used for debugging. If set to false, any prefix indexes
**   are ignored. This value is configured using:
**
**       INSERT INTO tbl(tbl, rank) VALUES('prefix-index', $bPrefixIndex);
**
*/
struct Fts5Config {
  sqlite3 *db;                    /* Database handle */
  char *zDb;                      /* Database holding FTS index (e.g. "main") */
  char *zName;                    /* Name of FTS index */
  int nCol;                       /* Number of columns */
  char **azCol;                   /* Column names */
  u8 *abUnindexed;                /* True for unindexed columns */
  int nPrefix;                    /* Number of prefix indexes */
  int *aPrefix;                   /* Sizes in bytes of nPrefix prefix indexes */
  int eContent;                   /* An FTS5_CONTENT value */
  int bContentlessDelete;         /* "contentless_delete=" option (dflt==0) */
  char *zContent;                 /* content table */
  char *zContentRowid;            /* "content_rowid=" option value */
  int bColumnsize;                /* "columnsize=" option value (dflt==1) */
  int bTokendata;                 /* "tokendata=" option value (dflt==0) */
  int eDetail;                    /* FTS5_DETAIL_XXX value */
  char *zContentExprlist;
  Fts5Tokenizer *pTok;
  fts5_tokenizer *pTokApi;
  int bLock;                      /* True when table is preparing statement */
  int ePattern;                   /* FTS_PATTERN_XXX constant */

  /* Values loaded from the %_config table */
  int iVersion;                   /* fts5 file format 'version' */
  int iCookie;                    /* Incremented when %_config is modified */
  int pgsz;                       /* Approximate page size used in %_data */
  int nAutomerge;                 /* 'automerge' setting */
  int nCrisisMerge;               /* Maximum allowed segments per level */
  int nUsermerge;                 /* 'usermerge' setting */
  int nHashSize;                  /* Bytes of memory for in-memory hash */
  char *zRank;                    /* Name of rank function */
  char *zRankArgs;                /* Arguments to rank function */
  int bSecureDelete;              /* 'secure-delete' */
  int nDeleteMerge;           /* 'deletemerge' */

  /* If non-NULL, points to sqlite3_vtab.base.zErrmsg. Often NULL. */
  char **pzErrmsg;

#ifdef SQLITE_DEBUG
  int bPrefixIndex;               /* True to use prefix-indexes */
#endif
};

/* Current expected value of %_config table 'version' field. And
** the expected version if the 'secure-delete' option has ever been
** set on the table.  */
#define FTS5_CURRENT_VERSION               4
#define FTS5_CURRENT_VERSION_SECUREDELETE  5

#define FTS5_CONTENT_NORMAL   0
#define FTS5_CONTENT_NONE     1
#define FTS5_CONTENT_EXTERNAL 2

#define FTS5_DETAIL_FULL      0
#define FTS5_DETAIL_NONE      1
#define FTS5_DETAIL_COLUMNS   2

#define FTS5_PATTERN_NONE     0
#define FTS5_PATTERN_LIKE     65  /* matches SQLITE_INDEX_CONSTRAINT_LIKE */
#define FTS5_PATTERN_GLOB     66  /* matches SQLITE_INDEX_CONSTRAINT_GLOB */

static int sqlite3Fts5ConfigParse(
    Fts5Global*, sqlite3*, int, const char **, Fts5Config**, char**
);
static void sqlite3Fts5ConfigFree(Fts5Config*);

static int sqlite3Fts5ConfigDeclareVtab(Fts5Config *pConfig);

static int sqlite3Fts5Tokenize(
  Fts5Config *pConfig,            /* FTS5 Configuration object */
  int flags,                      /* FTS5_TOKENIZE_* flags */
  const char *pText, int nText,   /* Text to tokenize */
  void *pCtx,                     /* Context passed to xToken() */
  int (*xToken)(void*, int, const char*, int, int, int)    /* Callback */
);

static void sqlite3Fts5Dequote(char *z);

/* Load the contents of the %_config table */
static int sqlite3Fts5ConfigLoad(Fts5Config*, int);

/* Set the value of a single config attribute */
static int sqlite3Fts5ConfigSetValue(Fts5Config*, const char*, sqlite3_value*, int*);

static int sqlite3Fts5ConfigParseRank(const char*, char**, char**);

/*
** End of interface to code in fts5_config.c.
**************************************************************************/

/**************************************************************************
** Interface to code in fts5_buffer.c.
*/

/*
** Buffer object for the incremental building of string data.
*/
typedef struct Fts5Buffer Fts5Buffer;
struct Fts5Buffer {
  u8 *p;
  int n;
  int nSpace;
};

static int sqlite3Fts5BufferSize(int*, Fts5Buffer*, u32);
static void sqlite3Fts5BufferAppendVarint(int*, Fts5Buffer*, i64);
static void sqlite3Fts5BufferAppendBlob(int*, Fts5Buffer*, u32, const u8*);
static void sqlite3Fts5BufferAppendString(int *, Fts5Buffer*, const char*);
static void sqlite3Fts5BufferFree(Fts5Buffer*);
static void sqlite3Fts5BufferZero(Fts5Buffer*);
static void sqlite3Fts5BufferSet(int*, Fts5Buffer*, int, const u8*);
static void sqlite3Fts5BufferAppendPrintf(int *, Fts5Buffer*, char *zFmt, ...);

static char *sqlite3Fts5Mprintf(int *pRc, const char *zFmt, ...);

#define fts5BufferZero(x)             sqlite3Fts5BufferZero(x)
#define fts5BufferAppendVarint(a,b,c) sqlite3Fts5BufferAppendVarint(a,b,(i64)c)
#define fts5BufferFree(a)             sqlite3Fts5BufferFree(a)
#define fts5BufferAppendBlob(a,b,c,d) sqlite3Fts5BufferAppendBlob(a,b,c,d)
#define fts5BufferSet(a,b,c,d)        sqlite3Fts5BufferSet(a,b,c,d)

#define fts5BufferGrow(pRc,pBuf,nn) ( \
  (u32)((pBuf)->n) + (u32)(nn) <= (u32)((pBuf)->nSpace) ? 0 : \
    sqlite3Fts5BufferSize((pRc),(pBuf),(nn)+(pBuf)->n) \
)

/* Write and decode big-endian 32-bit integer values */
static void sqlite3Fts5Put32(u8*, int);
static int sqlite3Fts5Get32(const u8*);

#define FTS5_POS2COLUMN(iPos) (int)(iPos >> 32)
#define FTS5_POS2OFFSET(iPos) (int)(iPos & 0x7FFFFFFF)

typedef struct Fts5PoslistReader Fts5PoslistReader;
struct Fts5PoslistReader {
  /* Variables used only by sqlite3Fts5PoslistIterXXX() functions. */
  const u8 *a;                    /* Position list to iterate through */
  int n;                          /* Size of buffer at a[] in bytes */
  int i;                          /* Current offset in a[] */

  u8 bFlag;                       /* For client use (any custom purpose) */

  /* Output variables */
  u8 bEof;                        /* Set to true at EOF */
  i64 iPos;                       /* (iCol<<32) + iPos */
};
static int sqlite3Fts5PoslistReaderInit(
  const u8 *a, int n,             /* Poslist buffer to iterate through */
  Fts5PoslistReader *pIter        /* Iterator object to initialize */
);
static int sqlite3Fts5PoslistReaderNext(Fts5PoslistReader*);

typedef struct Fts5PoslistWriter Fts5PoslistWriter;
struct Fts5PoslistWriter {
  i64 iPrev;
};
static int sqlite3Fts5PoslistWriterAppend(Fts5Buffer*, Fts5PoslistWriter*, i64);
static void sqlite3Fts5PoslistSafeAppend(Fts5Buffer*, i64*, i64);

static int sqlite3Fts5PoslistNext64(
  const u8 *a, int n,             /* Buffer containing poslist */
  int *pi,                        /* IN/OUT: Offset within a[] */
  i64 *piOff                      /* IN/OUT: Current offset */
);

/* Malloc utility */
static void *sqlite3Fts5MallocZero(int *pRc, sqlite3_int64 nByte);
static char *sqlite3Fts5Strndup(int *pRc, const char *pIn, int nIn);

/* Character set tests (like isspace(), isalpha() etc.) */
static int sqlite3Fts5IsBareword(char t);


/* Bucket of terms object used by the integrity-check in offsets=0 mode. */
typedef struct Fts5Termset Fts5Termset;
static int sqlite3Fts5TermsetNew(Fts5Termset**);
static int sqlite3Fts5TermsetAdd(Fts5Termset*, int, const char*, int, int *pbPresent);
static void sqlite3Fts5TermsetFree(Fts5Termset*);

/*
** End of interface to code in fts5_buffer.c.
**************************************************************************/

/**************************************************************************
** Interface to code in fts5_index.c. fts5_index.c contains contains code
** to access the data stored in the %_data table.
*/

typedef struct Fts5Index Fts5Index;
typedef struct Fts5IndexIter Fts5IndexIter;

struct Fts5IndexIter {
  i64 iRowid;
  const u8 *pData;
  int nData;
  u8 bEof;
};

#define sqlite3Fts5IterEof(x) ((x)->bEof)

/*
** Values used as part of the flags argument passed to IndexQuery().
*/
#define FTS5INDEX_QUERY_PREFIX      0x0001  /* Prefix query */
#define FTS5INDEX_QUERY_DESC        0x0002  /* Docs in descending rowid order */
#define FTS5INDEX_QUERY_TEST_NOIDX  0x0004  /* Do not use prefix index */
#define FTS5INDEX_QUERY_SCAN        0x0008  /* Scan query (fts5vocab) */

/* The following are used internally by the fts5_index.c module. They are
** defined here only to make it easier to avoid clashes with the flags
** above. */
#define FTS5INDEX_QUERY_SKIPEMPTY   0x0010
#define FTS5INDEX_QUERY_NOOUTPUT    0x0020
#define FTS5INDEX_QUERY_SKIPHASH    0x0040
#define FTS5INDEX_QUERY_NOTOKENDATA 0x0080
#define FTS5INDEX_QUERY_SCANONETERM 0x0100

/*
** Create/destroy an Fts5Index object.
*/
static int sqlite3Fts5IndexOpen(Fts5Config *pConfig, int bCreate, Fts5Index**, char**);
static int sqlite3Fts5IndexClose(Fts5Index *p);

/*
** Return a simple checksum value based on the arguments.
*/
static u64 sqlite3Fts5IndexEntryCksum(
  i64 iRowid,
  int iCol,
  int iPos,
  int iIdx,
  const char *pTerm,
  int nTerm
);

/*
** Argument p points to a buffer containing utf-8 text that is n bytes in
** size. Return the number of bytes in the nChar character prefix of the
** buffer, or 0 if there are less than nChar characters in total.
*/
static int sqlite3Fts5IndexCharlenToBytelen(
  const char *p,
  int nByte,
  int nChar
);

/*
** Open a new iterator to iterate though all rowids that match the
** specified token or token prefix.
*/
static int sqlite3Fts5IndexQuery(
  Fts5Index *p,                   /* FTS index to query */
  const char *pToken, int nToken, /* Token (or prefix) to query for */
  int flags,                      /* Mask of FTS5INDEX_QUERY_X flags */
  Fts5Colset *pColset,            /* Match these columns only */
  Fts5IndexIter **ppIter          /* OUT: New iterator object */
);

/*
** The various operations on open token or token prefix iterators opened
** using sqlite3Fts5IndexQuery().
*/
static int sqlite3Fts5IterNext(Fts5IndexIter*);
static int sqlite3Fts5IterNextFrom(Fts5IndexIter*, i64 iMatch);

/*
** Close an iterator opened by sqlite3Fts5IndexQuery().
*/
static void sqlite3Fts5IterClose(Fts5IndexIter*);

/*
** Close the reader blob handle, if it is open.
*/
static void sqlite3Fts5IndexCloseReader(Fts5Index*);

/*
** This interface is used by the fts5vocab module.
*/
static const char *sqlite3Fts5IterTerm(Fts5IndexIter*, int*);
static int sqlite3Fts5IterNextScan(Fts5IndexIter*);
static void *sqlite3Fts5StructureRef(Fts5Index*);
static void sqlite3Fts5StructureRelease(void*);
static int sqlite3Fts5StructureTest(Fts5Index*, void*);

/*
** Used by xInstToken():
*/
static int sqlite3Fts5IterToken(Fts5IndexIter*, i64, int, int, const char**, int*);

/*
** Insert or remove data to or from the index. Each time a document is
** added to or removed from the index, this function is called one or more
** times.
**
** For an insert, it must be called once for each token in the new document.
** If the operation is a delete, it must be called (at least) once for each
** unique token in the document with an iCol value less than zero. The iPos
** argument is ignored for a delete.
*/
static int sqlite3Fts5IndexWrite(
  Fts5Index *p,                   /* Index to write to */
  int iCol,                       /* Column token appears in (-ve -> delete) */
  int iPos,                       /* Position of token within column */
  const char *pToken, int nToken  /* Token to add or remove to or from index */
);

/*
** Indicate that subsequent calls to sqlite3Fts5IndexWrite() pertain to
** document iDocid.
*/
static int sqlite3Fts5IndexBeginWrite(
  Fts5Index *p,                   /* Index to write to */
  int bDelete,                    /* True if current operation is a delete */
  i64 iDocid                      /* Docid to add or remove data from */
);

/*
** Flush any data stored in the in-memory hash tables to the database.
** Also close any open blob handles.
*/
static int sqlite3Fts5IndexSync(Fts5Index *p);

/*
** Discard any data stored in the in-memory hash tables. Do not write it
** to the database. Additionally, assume that the contents of the %_data
** table may have changed on disk. So any in-memory caches of %_data
** records must be invalidated.
*/
static int sqlite3Fts5IndexRollback(Fts5Index *p);

/*
** Get or set the "averages" values.
*/
static int sqlite3Fts5IndexGetAverages(Fts5Index *p, i64 *pnRow, i64 *anSize);
static int sqlite3Fts5IndexSetAverages(Fts5Index *p, const u8*, int);

/*
** Functions called by the storage module as part of integrity-check.
*/
static int sqlite3Fts5IndexIntegrityCheck(Fts5Index*, u64 cksum, int bUseCksum);

/*
** Called during virtual module initialization to register UDF
** fts5_decode() with SQLite
*/
static int sqlite3Fts5IndexInit(sqlite3*);

static int sqlite3Fts5IndexSetCookie(Fts5Index*, int);

/*
** Return the total number of entries read from the %_data table by
** this connection since it was created.
*/
static int sqlite3Fts5IndexReads(Fts5Index *p);

static int sqlite3Fts5IndexReinit(Fts5Index *p);
static int sqlite3Fts5IndexOptimize(Fts5Index *p);
static int sqlite3Fts5IndexMerge(Fts5Index *p, int nMerge);
static int sqlite3Fts5IndexReset(Fts5Index *p);

static int sqlite3Fts5IndexLoadConfig(Fts5Index *p);

static int sqlite3Fts5IndexGetOrigin(Fts5Index *p, i64 *piOrigin);
static int sqlite3Fts5IndexContentlessDelete(Fts5Index *p, i64 iOrigin, i64 iRowid);

static void sqlite3Fts5IndexIterClearTokendata(Fts5IndexIter*);

/* Used to populate hash tables for xInstToken in detail=none/column mode. */
static int sqlite3Fts5IndexIterWriteTokendata(
    Fts5IndexIter*, const char*, int, i64 iRowid, int iCol, int iOff
);

/*
** End of interface to code in fts5_index.c.
**************************************************************************/

/**************************************************************************
** Interface to code in fts5_varint.c.
*/
static int sqlite3Fts5GetVarint32(const unsigned char *p, u32 *v);
static int sqlite3Fts5GetVarintLen(u32 iVal);
static u8 sqlite3Fts5GetVarint(const unsigned char*, u64*);
static int sqlite3Fts5PutVarint(unsigned char *p, u64 v);

#define fts5GetVarint32(a,b) sqlite3Fts5GetVarint32(a,(u32*)&(b))
#define fts5GetVarint    sqlite3Fts5GetVarint

#define fts5FastGetVarint32(a, iOff, nVal) {      \
  nVal = (a)[iOff++];                             \
  if( nVal & 0x80 ){                              \
    iOff--;                                       \
    iOff += fts5GetVarint32(&(a)[iOff], nVal);    \
  }                                               \
}
