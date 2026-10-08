

/*
** Arguments pLeft and pRight point to linked-lists of hash-entry objects,
** each sorted in key order. This function merges the two lists into a
** single list and returns a pointer to its first element.
*/
static Fts5HashEntry *fts5HashEntryMerge(
  Fts5HashEntry *pLeft,
  Fts5HashEntry *pRight
){
  Fts5HashEntry *p1 = pLeft;
  Fts5HashEntry *p2 = pRight;
  Fts5HashEntry *pRet = 0;
  Fts5HashEntry **ppOut = &pRet;

  while( p1 || p2 ){
    if( p1==0 ){
      *ppOut = p2;
      p2 = 0;
    }else if( p2==0 ){
      *ppOut = p1;
      p1 = 0;
    }else{
      char *zKey1 = fts5EntryKey(p1);
      char *zKey2 = fts5EntryKey(p2);
      int nMin = MIN(p1->nKey, p2->nKey);

      int cmp = memcmp(zKey1, zKey2, nMin);
      if( cmp==0 ){
        cmp = p1->nKey - p2->nKey;
      }
      assert( cmp!=0 );

      if( cmp>0 ){
        /* p2 is smaller */
        *ppOut = p2;
        ppOut = &p2->pScanNext;
        p2 = p2->pScanNext;
      }else{
        /* p1 is smaller */
        *ppOut = p1;
        ppOut = &p1->pScanNext;
        p1 = p1->pScanNext;
      }
      *ppOut = 0;
    }
  }

  return pRet;
}

/*
** Link all tokens from hash table iHash into a list in sorted order. The
** tokens are not removed from the hash table.
*/
static int fts5HashEntrySort(
  Fts5Hash *pHash,
  const char *pTerm, int nTerm,   /* Query prefix, if any */
  Fts5HashEntry **ppSorted
){
  const int nMergeSlot = 32;
  Fts5HashEntry **ap;
  Fts5HashEntry *pList;
  int iSlot;
  int i;

  *ppSorted = 0;
  ap = sqlite3_malloc64(sizeof(Fts5HashEntry*) * nMergeSlot);
  if( !ap ) return SQLITE_NOMEM;
  memset(ap, 0, sizeof(Fts5HashEntry*) * nMergeSlot);

  for(iSlot=0; iSlot<pHash->nSlot; iSlot++){
    Fts5HashEntry *pIter;
    for(pIter=pHash->aSlot[iSlot]; pIter; pIter=pIter->pHashNext){
      if( pTerm==0
       || (pIter->nKey>=nTerm && 0==memcmp(fts5EntryKey(pIter), pTerm, nTerm))
      ){
        Fts5HashEntry *pEntry = pIter;
        pEntry->pScanNext = 0;
        for(i=0; ap[i]; i++){
          pEntry = fts5HashEntryMerge(pEntry, ap[i]);
          ap[i] = 0;
        }
        ap[i] = pEntry;
      }
    }
  }

  pList = 0;
  for(i=0; i<nMergeSlot; i++){
    pList = fts5HashEntryMerge(pList, ap[i]);
  }

  sqlite3_free(ap);
  *ppSorted = pList;
  return SQLITE_OK;
}

/*
** Query the hash table for a doclist associated with term pTerm/nTerm.
*/
static int sqlite3Fts5HashQuery(
  Fts5Hash *pHash,                /* Hash table to query */
  int nPre,
  const char *pTerm, int nTerm,   /* Query term */
  void **ppOut,                   /* OUT: Pointer to new object */
  int *pnDoclist                  /* OUT: Size of doclist in bytes */
){
  unsigned int iHash = fts5HashKey(pHash->nSlot, (const u8*)pTerm, nTerm);
  char *zKey = 0;
  Fts5HashEntry *p;

  for(p=pHash->aSlot[iHash]; p; p=p->pHashNext){
    zKey = fts5EntryKey(p);
    if( nTerm==p->nKey && memcmp(zKey, pTerm, nTerm)==0 ) break;
  }

  if( p ){
    int nHashPre = sizeof(Fts5HashEntry) + nTerm;
    int nList = p->nData - nHashPre;
    u8 *pRet = (u8*)(*ppOut = sqlite3_malloc64(nPre + nList + 10));
    if( pRet ){
      Fts5HashEntry *pFaux = (Fts5HashEntry*)&pRet[nPre-nHashPre];
      memcpy(&pRet[nPre], &((u8*)p)[nHashPre], nList);
      nList += fts5HashAddPoslistSize(pHash, p, pFaux);
      *pnDoclist = nList;
    }else{
      *pnDoclist = 0;
      return SQLITE_NOMEM;
    }
  }else{
    *ppOut = 0;
    *pnDoclist = 0;
  }

  return SQLITE_OK;
}

static int sqlite3Fts5HashScanInit(
  Fts5Hash *p,                    /* Hash table to query */
  const char *pTerm, int nTerm    /* Query prefix */
){
  return fts5HashEntrySort(p, pTerm, nTerm, &p->pScan);
}

#ifdef SQLITE_DEBUG
static int fts5HashCount(Fts5Hash *pHash){
  int nEntry = 0;
  int ii;
  for(ii=0; ii<pHash->nSlot; ii++){
    Fts5HashEntry *p = 0;
    for(p=pHash->aSlot[ii]; p; p=p->pHashNext){
      nEntry++;
    }
  }
  return nEntry;
}
#endif

/*
** Return true if the hash table is empty, false otherwise.
*/
static int sqlite3Fts5HashIsEmpty(Fts5Hash *pHash){
  assert( pHash->nEntry==fts5HashCount(pHash) );
  return pHash->nEntry==0;
}

static void sqlite3Fts5HashScanNext(Fts5Hash *p){
  assert( !sqlite3Fts5HashScanEof(p) );
  p->pScan = p->pScan->pScanNext;
}

static int sqlite3Fts5HashScanEof(Fts5Hash *p){
  return (p->pScan==0);
}

static void sqlite3Fts5HashScanEntry(
  Fts5Hash *pHash,
  const char **pzTerm,            /* OUT: term (nul-terminated) */
  int *pnTerm,                    /* OUT: Size of term in bytes */
  const u8 **ppDoclist,           /* OUT: pointer to doclist */
  int *pnDoclist                  /* OUT: size of doclist in bytes */
){
  Fts5HashEntry *p;
  if( (p = pHash->pScan) ){
    char *zKey = fts5EntryKey(p);
    int nTerm = p->nKey;
    fts5HashAddPoslistSize(pHash, p, 0);
    *pzTerm = zKey;
    *pnTerm = nTerm;
    *ppDoclist = (const u8*)&zKey[nTerm];
    *pnDoclist = p->nData - (sizeof(Fts5HashEntry) + nTerm);
  }else{
    *pzTerm = 0;
    *pnTerm = 0;
    *ppDoclist = 0;
    *pnDoclist = 0;
  }
}

/*
** 2014 May 31
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
** Low level access to the FTS index stored in the database file. The
** routines in this file file implement all read and write access to the
** %_data table. Other parts of the system access this functionality via
** the interface defined in fts5Int.h.
*/


/* #include "fts5Int.h" */

/*
** Overview:
**
** The %_data table contains all the FTS indexes for an FTS5 virtual table.
** As well as the main term index, there may be up to 31 prefix indexes.
** The format is similar to FTS3/4, except that:
**
**   * all segment b-tree leaf data is stored in fixed size page records
**     (e.g. 1000 bytes). A single doclist may span multiple pages. Care is
**     taken to ensure it is possible to iterate in either direction through
**     the entries in a doclist, or to seek to a specific entry within a
**     doclist, without loading it into memory.
**
**   * large doclists that span many pages have associated "doclist index"
**     records that contain a copy of the first rowid on each page spanned by
**     the doclist. This is used to speed up seek operations, and merges of
**     large doclists with very small doclists.
**
**   * extra fields in the "structure record" record the state of ongoing
**     incremental merge operations.
**
*/


#define FTS5_OPT_WORK_UNIT  1000  /* Number of leaf pages per optimize step */
#define FTS5_WORK_UNIT      64    /* Number of leaf pages in unit of work */

#define FTS5_MIN_DLIDX_SIZE 4     /* Add dlidx if this many empty pages */

#define FTS5_MAIN_PREFIX '0'

#if FTS5_MAX_PREFIX_INDEXES > 31
# error "FTS5_MAX_PREFIX_INDEXES is too large"
#endif

#define FTS5_MAX_LEVEL 64

/*
** There are two versions of the format used for the structure record:
**
**   1. the legacy format, that may be read by all fts5 versions, and
**
**   2. the V2 format, which is used by contentless_delete=1 databases.
**
** Both begin with a 4-byte "configuration cookie" value. Then, a legacy
** format structure record contains a varint - the number of levels in
** the structure. Whereas a V2 structure record contains the constant
** 4 bytes [0xff 0x00 0x00 0x01]. This is unambiguous as the value of a
** varint has to be at least 16256 to begin with "0xFF". And the default
** maximum number of levels is 64.
**
** See below for more on structure record formats.
*/
#define FTS5_STRUCTURE_V2 "\xFF\x00\x00\x01"

/*
** Details:
**
** The %_data table managed by this module,
**
**     CREATE TABLE %_data(id INTEGER PRIMARY KEY, block BLOB);
**
** , contains the following 6 types of records. See the comments surrounding
** the FTS5_*_ROWID macros below for a description of how %_data rowids are
** assigned to each fo them.
**
** 1. Structure Records:
**
**   The set of segments that make up an index - the index structure - are
**   recorded in a single record within the %_data table. The record consists
**   of a single 32-bit configuration cookie value followed by a list of
**   SQLite varints.
**
**   If the structure record is a V2 record, the configuration cookie is
**   followed by the following 4 bytes: [0xFF 0x00 0x00 0x01].
**
**   Next, the record continues with three varints:
**
**     + number of levels,
**     + total number of segments on all levels,
**     + value of write counter.
**
**   Then, for each level from 0 to nMax:
**
**     + number of input segments in ongoing merge.
**     + total number of segments in level.
**     + for each segment from oldest to newest:
**         + segment id (always > 0)
**         + first leaf page number (often 1, always greater than 0)
**         + final leaf page number
**
**      Then, for V2 structures only:
**
**         + lower origin counter value,
**         + upper origin counter value,
**         + the number of tombstone hash pages.
**
** 2. The Averages Record:
**
**   A single record within the %_data table. The data is a list of varints.
**   The first value is the number of rows in the index. Then, for each column
**   from left to right, the total number of tokens in the column for all
**   rows of the table.
**
** 3. Segment leaves:
**
**   TERM/DOCLIST FORMAT:
**
**     Most of each segment leaf is taken up by term/doclist data. The
**     general format of term/doclist, starting with the first term
**     on the leaf page, is:
**
**         varint : size of first term
**         blob:    first term data
**         doclist: first doclist
**         zero-or-more {
**           varint:  number of bytes in common with previous term
**           varint:  number of bytes of new term data (nNew)
**           blob:    nNew bytes of new term data
**           doclist: next doclist
**         }
**
**     doclist format:
**
**         varint:  first rowid
**         poslist: first poslist
**         zero-or-more {
**           varint:  rowid delta (always > 0)
**           poslist: next poslist
**         }
**
**     poslist format:
**
**         varint: size of poslist in bytes multiplied by 2, not including
**                 this field. Plus 1 if this entry carries the "delete" flag.
**         collist: collist for column 0
**         zero-or-more {
**           0x01 byte
**           varint: column number (I)
**           collist: collist for column I
**         }
**
**     collist format:
**
**         varint: first offset + 2
**         zero-or-more {
**           varint: offset delta + 2
**         }
**
**   PAGE FORMAT
**
**     Each leaf page begins with a 4-byte header containing 2 16-bit
**     unsigned integer fields in big-endian format. They are:
**
**       * The byte offset of the first rowid on the page, if it exists
**         and occurs before the first term (otherwise 0).
**
**       * The byte offset of the start of the page footer. If the page
**         footer is 0 bytes in size, then this field is the same as the
**         size of the leaf page in bytes.
**
**     The page footer consists of a single varint for each term located
**     on the page. Each varint is the byte offset of the current term
**     within the page, delta-compressed against the previous value. In
**     other words, the first varint in the footer is the byte offset of
**     the first term, the second is the byte offset of the second less that
**     of the first, and so on.
**
**     The term/doclist format described above is accurate if the entire
**     term/doclist data fits on a single leaf page. If this is not the case,
**     the format is changed in two ways:
**
**       + if the first rowid on a page occurs before the first term, it
**         is stored as a literal value:
**
**             varint:  first rowid
**
**       + the first term on each page is stored in the same way as the
**         very first term of the segment:
**
**             varint : size of first term
**             blob:    first term data
**
** 5. Segment doclist indexes:
**
**   Doclist indexes are themselves b-trees, however they usually consist of
**   a single leaf record only. The format of each doclist index leaf page
**   is:
**
**     * Flags byte. Bits are:
**         0x01: Clear if leaf is also the root page, otherwise set.
**
**     * Page number of fts index leaf page. As a varint.
**
**     * First rowid on page indicated by previous field. As a varint.
**
**     * A list of varints, one for each subsequent termless page. A
**       positive delta if the termless page contains at least one rowid,
**       or an 0x00 byte otherwise.
**
**   Internal doclist index nodes are:
**
**     * Flags byte. Bits are:
**         0x01: Clear for root page, otherwise set.
**
**     * Page number of first child page. As a varint.
**
**     * Copy of first rowid on page indicated by previous field. As a varint.
**
**     * A list of delta-encoded varints - the first rowid on each subsequent
**       child page.
**
** 6. Tombstone Hash Page
**
**   These records are only ever present in contentless_delete=1 tables.
**   There are zero or more of these associated with each segment. They
**   are used to store the tombstone rowids for rows contained in the
**   associated segments.
**
**   The set of nHashPg tombstone hash pages associated with a single
**   segment together form a single hash table containing tombstone rowids.
**   To find the page of the hash on which a key might be stored:
**
**       iPg = (rowid % nHashPg)
**
**   Then, within page iPg, which has nSlot slots:
**
**       iSlot = (rowid / nHashPg) % nSlot
**
**   Each tombstone hash page begins with an 8 byte header:
**
**     1-byte:  Key-size (the size in bytes of each slot). Either 4 or 8.
**     1-byte:  rowid-0-tombstone flag. This flag is only valid on the
**              first tombstone hash page for each segment (iPg=0). If set,
**              the hash table contains rowid 0. If clear, it does not.
**              Rowid 0 is handled specially.
**     2-bytes: unused.
**     4-bytes: Big-endian integer containing number of entries on page.
**
**   Following this are nSlot 4 or 8 byte slots (depending on the key-size
**   in the first byte of the page header). The number of slots may be
**   determined based on the size of the page record and the key-size:
**
**     nSlot = (nByte - 8) / key-size
*/

/*
** Rowids for the averages and structure records in the %_data table.
*/
#define FTS5_AVERAGES_ROWID     1    /* Rowid used for the averages record */
#define FTS5_STRUCTURE_ROWID   10    /* The structure record */

/*
** Macros determining the rowids used by segment leaves and dlidx leaves
** and nodes. All nodes and leaves are stored in the %_data table with large
** positive rowids.
**
** Each segment has a unique non-zero 16-bit id.
**
** The rowid for each segment leaf is found by passing the segment id and
** the leaf page number to the FTS5_SEGMENT_ROWID macro. Leaves are numbered
** sequentially starting from 1.
*/
#define FTS5_DATA_ID_B     16     /* Max seg id number 65535 */
#define FTS5_DATA_DLI_B     1     /* Doclist-index flag (1 bit) */
#define FTS5_DATA_HEIGHT_B  5     /* Max dlidx tree height of 32 */
#define FTS5_DATA_PAGE_B   31     /* Max page number of 2147483648 */

#define fts5_dri(segid, dlidx, height, pgno) (                                 \
 ((i64)(segid)  << (FTS5_DATA_PAGE_B+FTS5_DATA_HEIGHT_B+FTS5_DATA_DLI_B)) +    \
 ((i64)(dlidx)  << (FTS5_DATA_PAGE_B + FTS5_DATA_HEIGHT_B)) +                  \
 ((i64)(height) << (FTS5_DATA_PAGE_B)) +                                       \
 ((i64)(pgno))                                                                 \
)

#define FTS5_SEGMENT_ROWID(segid, pgno)       fts5_dri(segid, 0, 0, pgno)
#define FTS5_DLIDX_ROWID(segid, height, pgno) fts5_dri(segid, 1, height, pgno)
#define FTS5_TOMBSTONE_ROWID(segid,ipg)       fts5_dri(segid+(1<<16), 0, 0, ipg)

#ifdef SQLITE_DEBUG
static int sqlite3Fts5Corrupt() { return SQLITE_CORRUPT_VTAB; }
#endif


/*
** Each time a blob is read from the %_data table, it is padded with this
** many zero bytes. This makes it easier to decode the various record formats
** without overreading if the records are corrupt.
*/
#define FTS5_DATA_ZERO_PADDING 8
#define FTS5_DATA_PADDING 20

typedef struct Fts5Data Fts5Data;
typedef struct Fts5DlidxIter Fts5DlidxIter;
typedef struct Fts5DlidxLvl Fts5DlidxLvl;
typedef struct Fts5DlidxWriter Fts5DlidxWriter;
typedef struct Fts5Iter Fts5Iter;
typedef struct Fts5PageWriter Fts5PageWriter;
typedef struct Fts5SegIter Fts5SegIter;
typedef struct Fts5DoclistIter Fts5DoclistIter;
typedef struct Fts5SegWriter Fts5SegWriter;
typedef struct Fts5Structure Fts5Structure;
typedef struct Fts5StructureLevel Fts5StructureLevel;
typedef struct Fts5StructureSegment Fts5StructureSegment;
typedef struct Fts5TokenDataIter Fts5TokenDataIter;
typedef struct Fts5TokenDataMap Fts5TokenDataMap;
typedef struct Fts5TombstoneArray Fts5TombstoneArray;

struct Fts5Data {
  u8 *p;                          /* Pointer to buffer containing record */
  int nn;                         /* Size of record in bytes */
  int szLeaf;                     /* Size of leaf without page-index */
};
