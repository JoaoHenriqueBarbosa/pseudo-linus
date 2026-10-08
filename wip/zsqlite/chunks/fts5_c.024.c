
/*
** One object per %_data table.
**
** nContentlessDelete:
**   The number of contentless delete operations since the most recent
**   call to fts5IndexFlush() or fts5IndexDiscardData(). This is tracked
**   so that extra auto-merge work can be done by fts5IndexFlush() to
**   account for the delete operations.
*/
struct Fts5Index {
  Fts5Config *pConfig;            /* Virtual table configuration */
  char *zDataTbl;                 /* Name of %_data table */
  int nWorkUnit;                  /* Leaf pages in a "unit" of work */

  /*
  ** Variables related to the accumulation of tokens and doclists within the
  ** in-memory hash tables before they are flushed to disk.
  */
  Fts5Hash *pHash;                /* Hash table for in-memory data */
  int nPendingData;               /* Current bytes of pending data */
  i64 iWriteRowid;                /* Rowid for current doc being written */
  int bDelete;                    /* Current write is a delete */
  int nContentlessDelete;         /* Number of contentless delete ops */
  int nPendingRow;                /* Number of INSERT in hash table */

  /* Error state. */
  int rc;                         /* Current error code */
  int flushRc;

  /* State used by the fts5DataXXX() functions. */
  sqlite3_blob *pReader;          /* RO incr-blob open on %_data table */
  sqlite3_stmt *pWriter;          /* "INSERT ... %_data VALUES(?,?)" */
  sqlite3_stmt *pDeleter;         /* "DELETE FROM %_data ... id>=? AND id<=?" */
  sqlite3_stmt *pIdxWriter;       /* "INSERT ... %_idx VALUES(?,?,?,?)" */
  sqlite3_stmt *pIdxDeleter;      /* "DELETE FROM %_idx WHERE segid=?" */
  sqlite3_stmt *pIdxSelect;
  sqlite3_stmt *pIdxNextSelect;
  int nRead;                      /* Total number of blocks read */

  sqlite3_stmt *pDeleteFromIdx;

  sqlite3_stmt *pDataVersion;
  i64 iStructVersion;             /* data_version when pStruct read */
  Fts5Structure *pStruct;         /* Current db structure (or NULL) */
};

struct Fts5DoclistIter {
  u8 *aEof;                       /* Pointer to 1 byte past end of doclist */

  /* Output variables. aPoslist==0 at EOF */
  i64 iRowid;
  u8 *aPoslist;
  int nPoslist;
  int nSize;
};

/*
** The contents of the "structure" record for each index are represented
** using an Fts5Structure record in memory. Which uses instances of the
** other Fts5StructureXXX types as components.
**
** nOriginCntr:
**   This value is set to non-zero for structure records created for
**   contentlessdelete=1 tables only. In that case it represents the
**   origin value to apply to the next top-level segment created.
*/
struct Fts5StructureSegment {
  int iSegid;                     /* Segment id */
  int pgnoFirst;                  /* First leaf page number in segment */
  int pgnoLast;                   /* Last leaf page number in segment */

  /* contentlessdelete=1 tables only: */
  u64 iOrigin1;
  u64 iOrigin2;
  int nPgTombstone;               /* Number of tombstone hash table pages */
  u64 nEntryTombstone;            /* Number of tombstone entries that "count" */
  u64 nEntry;                     /* Number of rows in this segment */
};
struct Fts5StructureLevel {
  int nMerge;                     /* Number of segments in incr-merge */
  int nSeg;                       /* Total number of segments on level */
  Fts5StructureSegment *aSeg;     /* Array of segments. aSeg[0] is oldest. */
};
struct Fts5Structure {
  int nRef;                       /* Object reference count */
  u64 nWriteCounter;              /* Total leaves written to level 0 */
  u64 nOriginCntr;                /* Origin value for next top-level segment */
  int nSegment;                   /* Total segments in this structure */
  int nLevel;                     /* Number of levels in this index */
  Fts5StructureLevel aLevel[1];   /* Array of nLevel level objects */
};

/*
** An object of type Fts5SegWriter is used to write to segments.
*/
struct Fts5PageWriter {
  int pgno;                       /* Page number for this page */
  int iPrevPgidx;                 /* Previous value written into pgidx */
  Fts5Buffer buf;                 /* Buffer containing leaf data */
  Fts5Buffer pgidx;               /* Buffer containing page-index */
  Fts5Buffer term;                /* Buffer containing previous term on page */
};
struct Fts5DlidxWriter {
  int pgno;                       /* Page number for this page */
  int bPrevValid;                 /* True if iPrev is valid */
  i64 iPrev;                      /* Previous rowid value written to page */
  Fts5Buffer buf;                 /* Buffer containing page data */
};
struct Fts5SegWriter {
  int iSegid;                     /* Segid to write to */
  Fts5PageWriter writer;          /* PageWriter object */
  i64 iPrevRowid;                 /* Previous rowid written to current leaf */
  u8 bFirstRowidInDoclist;        /* True if next rowid is first in doclist */
  u8 bFirstRowidInPage;           /* True if next rowid is first in page */
  /* TODO1: Can use (writer.pgidx.n==0) instead of bFirstTermInPage */
  u8 bFirstTermInPage;            /* True if next term will be first in leaf */
  int nLeafWritten;               /* Number of leaf pages written */
  int nEmpty;                     /* Number of contiguous term-less nodes */

  int nDlidx;                     /* Allocated size of aDlidx[] array */
  Fts5DlidxWriter *aDlidx;        /* Array of Fts5DlidxWriter objects */

  /* Values to insert into the %_idx table */
  Fts5Buffer btterm;              /* Next term to insert into %_idx table */
  int iBtPage;                    /* Page number corresponding to btterm */
};

typedef struct Fts5CResult Fts5CResult;
struct Fts5CResult {
  u16 iFirst;                     /* aSeg[] index of firstest iterator */
  u8 bTermEq;                     /* True if the terms are equal */
};

/*
** Object for iterating through a single segment, visiting each term/rowid
** pair in the segment.
**
** pSeg:
**   The segment to iterate through.
**
** iLeafPgno:
**   Current leaf page number within segment.
**
** iLeafOffset:
**   Byte offset within the current leaf that is the first byte of the
**   position list data (one byte passed the position-list size field).
**
** pLeaf:
**   Buffer containing current leaf page data. Set to NULL at EOF.
**
** iTermLeafPgno, iTermLeafOffset:
**   Leaf page number containing the last term read from the segment. And
**   the offset immediately following the term data.
**
** flags:
**   Mask of FTS5_SEGITER_XXX values. Interpreted as follows:
**
**   FTS5_SEGITER_ONETERM:
**     If set, set the iterator to point to EOF after the current doclist
**     has been exhausted. Do not proceed to the next term in the segment.
**
**   FTS5_SEGITER_REVERSE:
**     This flag is only ever set if FTS5_SEGITER_ONETERM is also set. If
**     it is set, iterate through rowid in descending order instead of the
**     default ascending order.
**
** iRowidOffset/nRowidOffset/aRowidOffset:
**     These are used if the FTS5_SEGITER_REVERSE flag is set.
**
**     For each rowid on the page corresponding to the current term, the
**     corresponding aRowidOffset[] entry is set to the byte offset of the
**     start of the "position-list-size" field within the page.
**
** iTermIdx:
**     Index of current term on iTermLeafPgno.
**
** apTombstone/nTombstone:
**     These are used for contentless_delete=1 tables only. When the cursor
**     is first allocated, the apTombstone[] array is allocated so that it
**     is large enough for all tombstones hash pages associated with the
**     segment. The pages themselves are loaded lazily from the database as
**     they are required.
*/
struct Fts5SegIter {
  Fts5StructureSegment *pSeg;     /* Segment to iterate through */
  int flags;                      /* Mask of configuration flags */
  int iLeafPgno;                  /* Current leaf page number */
  Fts5Data *pLeaf;                /* Current leaf data */
  Fts5Data *pNextLeaf;            /* Leaf page (iLeafPgno+1) */
  i64 iLeafOffset;                /* Byte offset within current leaf */
  Fts5TombstoneArray *pTombArray; /* Array of tombstone pages */

  /* Next method */
  void (*xNext)(Fts5Index*, Fts5SegIter*, int*);

  /* The page and offset from which the current term was read. The offset
  ** is the offset of the first rowid in the current doclist.  */
  int iTermLeafPgno;
  int iTermLeafOffset;

  int iPgidxOff;                  /* Next offset in pgidx */
  int iEndofDoclist;

  /* The following are only used if the FTS5_SEGITER_REVERSE flag is set. */
  int iRowidOffset;               /* Current entry in aRowidOffset[] */
  int nRowidOffset;               /* Allocated size of aRowidOffset[] array */
  int *aRowidOffset;              /* Array of offset to rowid fields */

  Fts5DlidxIter *pDlidx;          /* If there is a doclist-index */

  /* Variables populated based on current entry. */
  Fts5Buffer term;                /* Current term */
  i64 iRowid;                     /* Current rowid */
  int nPos;                       /* Number of bytes in current position list */
  u8 bDel;                        /* True if the delete flag is set */
};

/*
** Array of tombstone pages. Reference counted.
*/
struct Fts5TombstoneArray {
  int nRef;                       /* Number of pointers to this object */
  int nTombstone;
  Fts5Data *apTombstone[1];       /* Array of tombstone pages */
};

/*
** Argument is a pointer to an Fts5Data structure that contains a
** leaf page.
*/
#define ASSERT_SZLEAF_OK(x) assert( \
    (x)->szLeaf==(x)->nn || (x)->szLeaf==fts5GetU16(&(x)->p[2]) \
)

#define FTS5_SEGITER_ONETERM 0x01
#define FTS5_SEGITER_REVERSE 0x02

/*
** Argument is a pointer to an Fts5Data structure that contains a leaf
** page. This macro evaluates to true if the leaf contains no terms, or
** false if it contains at least one term.
*/
#define fts5LeafIsTermless(x) ((x)->szLeaf >= (x)->nn)

#define fts5LeafTermOff(x, i) (fts5GetU16(&(x)->p[(x)->szLeaf + (i)*2]))

#define fts5LeafFirstRowidOff(x) (fts5GetU16((x)->p))

/*
** Object for iterating through the merged results of one or more segments,
** visiting each term/rowid pair in the merged data.
**
** nSeg is always a power of two greater than or equal to the number of
** segments that this object is merging data from. Both the aSeg[] and
** aFirst[] arrays are sized at nSeg entries. The aSeg[] array is padded
** with zeroed objects - these are handled as if they were iterators opened
** on empty segments.
**
** The results of comparing segments aSeg[N] and aSeg[N+1], where N is an
** even number, is stored in aFirst[(nSeg+N)/2]. The "result" of the
** comparison in this context is the index of the iterator that currently
** points to the smaller term/rowid combination. Iterators at EOF are
** considered to be greater than all other iterators.
**
** aFirst[1] contains the index in aSeg[] of the iterator that points to
** the smallest key overall. aFirst[0] is unused.
**
** poslist:
**   Used by sqlite3Fts5IterPoslist() when the poslist needs to be buffered.
**   There is no way to tell if this is populated or not.
**
** pColset:
**   If not NULL, points to an object containing a set of column indices.
**   Only matches that occur in one of these columns will be returned.
**   The Fts5Iter does not own the Fts5Colset object, and so it is not
**   freed when the iterator is closed - it is owned by the upper layer.
*/
struct Fts5Iter {
  Fts5IndexIter base;             /* Base class containing output vars */
  Fts5TokenDataIter *pTokenDataIter;

  Fts5Index *pIndex;              /* Index that owns this iterator */
  Fts5Buffer poslist;             /* Buffer containing current poslist */
  Fts5Colset *pColset;            /* Restrict matches to these columns */

  /* Invoked to set output variables. */
  void (*xSetOutputs)(Fts5Iter*, Fts5SegIter*);

  int nSeg;                       /* Size of aSeg[] array */
  int bRev;                       /* True to iterate in reverse order */
  u8 bSkipEmpty;                  /* True to skip deleted entries */

  i64 iSwitchRowid;               /* Firstest rowid of other than aFirst[1] */
  Fts5CResult *aFirst;            /* Current merge state (see above) */
  Fts5SegIter aSeg[1];            /* Array of segment iterators */
};

/*
** An instance of the following type is used to iterate through the contents
** of a doclist-index record.
**
** pData:
**   Record containing the doclist-index data.
**
** bEof:
**   Set to true once iterator has reached EOF.
**
** iOff:
**   Set to the current offset within record pData.
*/
struct Fts5DlidxLvl {
  Fts5Data *pData;              /* Data for current page of this level */
  int iOff;                     /* Current offset into pData */
  int bEof;                     /* At EOF already */
  int iFirstOff;                /* Used by reverse iterators */

  /* Output variables */
  int iLeafPgno;                /* Page number of current leaf page */
  i64 iRowid;                   /* First rowid on leaf iLeafPgno */
};
struct Fts5DlidxIter {
  int nLvl;
  int iSegid;
  Fts5DlidxLvl aLvl[1];
};

static void fts5PutU16(u8 *aOut, u16 iVal){
  aOut[0] = (iVal>>8);
  aOut[1] = (iVal&0xFF);
}

static u16 fts5GetU16(const u8 *aIn){
  return ((u16)aIn[0] << 8) + aIn[1];
}

/*
** The only argument points to a buffer at least 8 bytes in size. This
** function interprets the first 8 bytes of the buffer as a 64-bit big-endian
** unsigned integer and returns the result.
*/
static u64 fts5GetU64(u8 *a){
  return ((u64)a[0] << 56)
       + ((u64)a[1] << 48)
       + ((u64)a[2] << 40)
       + ((u64)a[3] << 32)
       + ((u64)a[4] << 24)
       + ((u64)a[5] << 16)
       + ((u64)a[6] << 8)
       + ((u64)a[7] << 0);
}
