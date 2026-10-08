/*
** 2009 Oct 23
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
** This file is part of the SQLite FTS3 extension module. Specifically,
** this file contains code to insert, update and delete rows from FTS3
** tables. It also contains code to merge FTS3 b-tree segments. Some
** of the sub-routines used to merge segments are also used by the query
** code in fts3.c.
*/

/* #include "fts3Int.h" */
#if !defined(SQLITE_CORE) || defined(SQLITE_ENABLE_FTS3)

/* #include <string.h> */
/* #include <assert.h> */
/* #include <stdlib.h> */
/* #include <stdio.h> */

#define FTS_MAX_APPENDABLE_HEIGHT 16

/*
** When full-text index nodes are loaded from disk, the buffer that they
** are loaded into has the following number of bytes of padding at the end
** of it. i.e. if a full-text index node is 900 bytes in size, then a buffer
** of 920 bytes is allocated for it.
**
** This means that if we have a pointer into a buffer containing node data,
** it is always safe to read up to two varints from it without risking an
** overread, even if the node data is corrupted.
*/
#define FTS3_NODE_PADDING (FTS3_VARINT_MAX*2)

/*
** Under certain circumstances, b-tree nodes (doclists) can be loaded into
** memory incrementally instead of all at once. This can be a big performance
** win (reduced IO and CPU) if SQLite stops calling the virtual table xNext()
** method before retrieving all query results (as may happen, for example,
** if a query has a LIMIT clause).
**
** Incremental loading is used for b-tree nodes FTS3_NODE_CHUNK_THRESHOLD
** bytes and larger. Nodes are loaded in chunks of FTS3_NODE_CHUNKSIZE bytes.
** The code is written so that the hard lower-limit for each of these values
** is 1. Clearly such small values would be inefficient, but can be useful
** for testing purposes.
**
** If this module is built with SQLITE_TEST defined, these constants may
** be overridden at runtime for testing purposes. File fts3_test.c contains
** a Tcl interface to read and write the values.
*/
#ifdef SQLITE_TEST
int test_fts3_node_chunksize = (4*1024);
int test_fts3_node_chunk_threshold = (4*1024)*4;
# define FTS3_NODE_CHUNKSIZE       test_fts3_node_chunksize
# define FTS3_NODE_CHUNK_THRESHOLD test_fts3_node_chunk_threshold
#else
# define FTS3_NODE_CHUNKSIZE (4*1024)
# define FTS3_NODE_CHUNK_THRESHOLD (FTS3_NODE_CHUNKSIZE*4)
#endif

/*
** The values that may be meaningfully bound to the :1 parameter in
** statements SQL_REPLACE_STAT and SQL_SELECT_STAT.
*/
#define FTS_STAT_DOCTOTAL      0
#define FTS_STAT_INCRMERGEHINT 1
#define FTS_STAT_AUTOINCRMERGE 2

/*
** If FTS_LOG_MERGES is defined, call sqlite3_log() to report each automatic
** and incremental merge operation that takes place. This is used for
** debugging FTS only, it should not usually be turned on in production
** systems.
*/
#ifdef FTS3_LOG_MERGES
static void fts3LogMerge(int nMerge, sqlite3_int64 iAbsLevel){
  sqlite3_log(SQLITE_OK, "%d-way merge from level %d", nMerge, (int)iAbsLevel);
}
#else
#define fts3LogMerge(x, y)
#endif


typedef struct PendingList PendingList;
typedef struct SegmentNode SegmentNode;
typedef struct SegmentWriter SegmentWriter;

/*
** An instance of the following data structure is used to build doclists
** incrementally. See function fts3PendingListAppend() for details.
*/
struct PendingList {
  int nData;
  char *aData;
  int nSpace;
  sqlite3_int64 iLastDocid;
  sqlite3_int64 iLastCol;
  sqlite3_int64 iLastPos;
};


/*
** Each cursor has a (possibly empty) linked list of the following objects.
*/
struct Fts3DeferredToken {
  Fts3PhraseToken *pToken;        /* Pointer to corresponding expr token */
  int iCol;                       /* Column token must occur in */
  Fts3DeferredToken *pNext;       /* Next in list of deferred tokens */
  PendingList *pList;             /* Doclist is assembled here */
};

/*
** An instance of this structure is used to iterate through the terms on
** a contiguous set of segment b-tree leaf nodes. Although the details of
** this structure are only manipulated by code in this file, opaque handles
** of type Fts3SegReader* are also used by code in fts3.c to iterate through
** terms when querying the full-text index. See functions:
**
**   sqlite3Fts3SegReaderNew()
**   sqlite3Fts3SegReaderFree()
**   sqlite3Fts3SegReaderIterate()
**
** Methods used to manipulate Fts3SegReader structures:
**
**   fts3SegReaderNext()
**   fts3SegReaderFirstDocid()
**   fts3SegReaderNextDocid()
*/
struct Fts3SegReader {
  int iIdx;                       /* Index within level, or 0x7FFFFFFF for PT */
  u8 bLookup;                     /* True for a lookup only */
  u8 rootOnly;                    /* True for a root-only reader */

  sqlite3_int64 iStartBlock;      /* Rowid of first leaf block to traverse */
  sqlite3_int64 iLeafEndBlock;    /* Rowid of final leaf block to traverse */
  sqlite3_int64 iEndBlock;        /* Rowid of final block in segment (or 0) */
  sqlite3_int64 iCurrentBlock;    /* Current leaf block (or 0) */

  char *aNode;                    /* Pointer to node data (or NULL) */
  int nNode;                      /* Size of buffer at aNode (or 0) */
  int nPopulate;                  /* If >0, bytes of buffer aNode[] loaded */
  sqlite3_blob *pBlob;            /* If not NULL, blob handle to read node */

  Fts3HashElem **ppNextElem;

  /* Variables set by fts3SegReaderNext(). These may be read directly
  ** by the caller. They are valid from the time SegmentReaderNew() returns
  ** until SegmentReaderNext() returns something other than SQLITE_OK
  ** (i.e. SQLITE_DONE).
  */
  int nTerm;                      /* Number of bytes in current term */
  char *zTerm;                    /* Pointer to current term */
  int nTermAlloc;                 /* Allocated size of zTerm buffer */
  char *aDoclist;                 /* Pointer to doclist of current entry */
  int nDoclist;                   /* Size of doclist in current entry */

  /* The following variables are used by fts3SegReaderNextDocid() to iterate
  ** through the current doclist (aDoclist/nDoclist).
  */
  char *pOffsetList;
  int nOffsetList;                /* For descending pending seg-readers only */
  sqlite3_int64 iDocid;
};

#define fts3SegReaderIsPending(p) ((p)->ppNextElem!=0)
#define fts3SegReaderIsRootOnly(p) ((p)->rootOnly!=0)

/*
** An instance of this structure is used to create a segment b-tree in the
** database. The internal details of this type are only accessed by the
** following functions:
**
**   fts3SegWriterAdd()
**   fts3SegWriterFlush()
**   fts3SegWriterFree()
*/
struct SegmentWriter {
  SegmentNode *pTree;             /* Pointer to interior tree structure */
  sqlite3_int64 iFirst;           /* First slot in %_segments written */
  sqlite3_int64 iFree;            /* Next free slot in %_segments */
  char *zTerm;                    /* Pointer to previous term buffer */
  int nTerm;                      /* Number of bytes in zTerm */
  int nMalloc;                    /* Size of malloc'd buffer at zMalloc */
  char *zMalloc;                  /* Malloc'd space (possibly) used for zTerm */
  int nSize;                      /* Size of allocation at aData */
  int nData;                      /* Bytes of data in aData */
  char *aData;                    /* Pointer to block from malloc() */
  i64 nLeafData;                  /* Number of bytes of leaf data written */
};

/*
** Type SegmentNode is used by the following three functions to create
** the interior part of the segment b+-tree structures (everything except
** the leaf nodes). These functions and type are only ever used by code
** within the fts3SegWriterXXX() family of functions described above.
**
**   fts3NodeAddTerm()
**   fts3NodeWrite()
**   fts3NodeFree()
**
** When a b+tree is written to the database (either as a result of a merge
** or the pending-terms table being flushed), leaves are written into the
** database file as soon as they are completely populated. The interior of
** the tree is assembled in memory and written out only once all leaves have
** been populated and stored. This is Ok, as the b+-tree fanout is usually
** very large, meaning that the interior of the tree consumes relatively
** little memory.
*/
struct SegmentNode {
  SegmentNode *pParent;           /* Parent node (or NULL for root node) */
  SegmentNode *pRight;            /* Pointer to right-sibling */
  SegmentNode *pLeftmost;         /* Pointer to left-most node of this depth */
  int nEntry;                     /* Number of terms written to node so far */
  char *zTerm;                    /* Pointer to previous term buffer */
  int nTerm;                      /* Number of bytes in zTerm */
  int nMalloc;                    /* Size of malloc'd buffer at zMalloc */
  char *zMalloc;                  /* Malloc'd space (possibly) used for zTerm */
  int nData;                      /* Bytes of valid data so far */
  char *aData;                    /* Node data */
};

/*
** Valid values for the second argument to fts3SqlStmt().
*/
#define SQL_DELETE_CONTENT             0
#define SQL_IS_EMPTY                   1
#define SQL_DELETE_ALL_CONTENT         2
#define SQL_DELETE_ALL_SEGMENTS        3
#define SQL_DELETE_ALL_SEGDIR          4
#define SQL_DELETE_ALL_DOCSIZE         5
#define SQL_DELETE_ALL_STAT            6
#define SQL_SELECT_CONTENT_BY_ROWID    7
#define SQL_NEXT_SEGMENT_INDEX         8
#define SQL_INSERT_SEGMENTS            9
#define SQL_NEXT_SEGMENTS_ID          10
#define SQL_INSERT_SEGDIR             11
#define SQL_SELECT_LEVEL              12
#define SQL_SELECT_LEVEL_RANGE        13
#define SQL_SELECT_LEVEL_COUNT        14
#define SQL_SELECT_SEGDIR_MAX_LEVEL   15
#define SQL_DELETE_SEGDIR_LEVEL       16
#define SQL_DELETE_SEGMENTS_RANGE     17
#define SQL_CONTENT_INSERT            18
#define SQL_DELETE_DOCSIZE            19
#define SQL_REPLACE_DOCSIZE           20
#define SQL_SELECT_DOCSIZE            21
#define SQL_SELECT_STAT               22
#define SQL_REPLACE_STAT              23

#define SQL_SELECT_ALL_PREFIX_LEVEL   24
#define SQL_DELETE_ALL_TERMS_SEGDIR   25
#define SQL_DELETE_SEGDIR_RANGE       26
#define SQL_SELECT_ALL_LANGID         27
#define SQL_FIND_MERGE_LEVEL          28
#define SQL_MAX_LEAF_NODE_ESTIMATE    29
#define SQL_DELETE_SEGDIR_ENTRY       30
#define SQL_SHIFT_SEGDIR_ENTRY        31
#define SQL_SELECT_SEGDIR             32
#define SQL_CHOMP_SEGDIR              33
#define SQL_SEGMENT_IS_APPENDABLE     34
#define SQL_SELECT_INDEXES            35
#define SQL_SELECT_MXLEVEL            36

#define SQL_SELECT_LEVEL_RANGE2       37
#define SQL_UPDATE_LEVEL_IDX          38
#define SQL_UPDATE_LEVEL              39

/*
** This function is used to obtain an SQLite prepared statement handle
** for the statement identified by the second argument. If successful,
** *pp is set to the requested statement handle and SQLITE_OK returned.
** Otherwise, an SQLite error code is returned and *pp is set to 0.
**
** If argument apVal is not NULL, then it must point to an array with
** at least as many entries as the requested statement has bound
** parameters. The values are bound to the statements parameters before
** returning.
*/
static int fts3SqlStmt(
  Fts3Table *p,                   /* Virtual table handle */
  int eStmt,                      /* One of the SQL_XXX constants above */
  sqlite3_stmt **pp,              /* OUT: Statement handle */
  sqlite3_value **apVal           /* Values to bind to statement */
){
  const char *azSql[] = {
/* 0  */  "DELETE FROM %Q.'%q_content' WHERE rowid = ?",
/* 1  */  "SELECT NOT EXISTS(SELECT docid FROM %Q.'%q_content' WHERE rowid!=?)",
/* 2  */  "DELETE FROM %Q.'%q_content'",
/* 3  */  "DELETE FROM %Q.'%q_segments'",
/* 4  */  "DELETE FROM %Q.'%q_segdir'",
/* 5  */  "DELETE FROM %Q.'%q_docsize'",
/* 6  */  "DELETE FROM %Q.'%q_stat'",
/* 7  */  "SELECT %s WHERE rowid=?",
/* 8  */  "SELECT (SELECT max(idx) FROM %Q.'%q_segdir' WHERE level = ?) + 1",
/* 9  */  "REPLACE INTO %Q.'%q_segments'(blockid, block) VALUES(?, ?)",
/* 10 */  "SELECT coalesce((SELECT max(blockid) FROM %Q.'%q_segments') + 1, 1)",
/* 11 */  "REPLACE INTO %Q.'%q_segdir' VALUES(?,?,?,?,?,?)",

          /* Return segments in order from oldest to newest.*/
/* 12 */  "SELECT idx, start_block, leaves_end_block, end_block, root "
            "FROM %Q.'%q_segdir' WHERE level = ? ORDER BY idx ASC",
/* 13 */  "SELECT idx, start_block, leaves_end_block, end_block, root "
            "FROM %Q.'%q_segdir' WHERE level BETWEEN ? AND ?"
            "ORDER BY level DESC, idx ASC",

/* 14 */  "SELECT count(*) FROM %Q.'%q_segdir' WHERE level = ?",
/* 15 */  "SELECT max(level) FROM %Q.'%q_segdir' WHERE level BETWEEN ? AND ?",

/* 16 */  "DELETE FROM %Q.'%q_segdir' WHERE level = ?",
/* 17 */  "DELETE FROM %Q.'%q_segments' WHERE blockid BETWEEN ? AND ?",
/* 18 */  "INSERT INTO %Q.'%q_content' VALUES(%s)",
/* 19 */  "DELETE FROM %Q.'%q_docsize' WHERE docid = ?",
/* 20 */  "REPLACE INTO %Q.'%q_docsize' VALUES(?,?)",
/* 21 */  "SELECT size FROM %Q.'%q_docsize' WHERE docid=?",
/* 22 */  "SELECT value FROM %Q.'%q_stat' WHERE id=?",
/* 23 */  "REPLACE INTO %Q.'%q_stat' VALUES(?,?)",
/* 24 */  "",
/* 25 */  "",

/* 26 */ "DELETE FROM %Q.'%q_segdir' WHERE level BETWEEN ? AND ?",
/* 27 */ "SELECT ? UNION SELECT level / (1024 * ?) FROM %Q.'%q_segdir'",

/* This statement is used to determine which level to read the input from
** when performing an incremental merge. It returns the absolute level number
** of the oldest level in the db that contains at least ? segments. Or,
** if no level in the FTS index contains more than ? segments, the statement
** returns zero rows.  */
/* 28 */ "SELECT level, count(*) AS cnt FROM %Q.'%q_segdir' "
         "  GROUP BY level HAVING cnt>=?"
         "  ORDER BY (level %% 1024) ASC, 2 DESC LIMIT 1",

/* Estimate the upper limit on the number of leaf nodes in a new segment
** created by merging the oldest :2 segments from absolute level :1. See
** function sqlite3Fts3Incrmerge() for details.  */
/* 29 */ "SELECT 2 * total(1 + leaves_end_block - start_block) "
         "  FROM (SELECT * FROM %Q.'%q_segdir' "
         "        WHERE level = ? ORDER BY idx ASC LIMIT ?"
         "  )",

/* SQL_DELETE_SEGDIR_ENTRY
**   Delete the %_segdir entry on absolute level :1 with index :2.  */
/* 30 */ "DELETE FROM %Q.'%q_segdir' WHERE level = ? AND idx = ?",

/* SQL_SHIFT_SEGDIR_ENTRY
**   Modify the idx value for the segment with idx=:3 on absolute level :2
**   to :1.  */
/* 31 */ "UPDATE %Q.'%q_segdir' SET idx = ? WHERE level=? AND idx=?",

/* SQL_SELECT_SEGDIR
**   Read a single entry from the %_segdir table. The entry from absolute
**   level :1 with index value :2.  */
/* 32 */  "SELECT idx, start_block, leaves_end_block, end_block, root "
            "FROM %Q.'%q_segdir' WHERE level = ? AND idx = ?",

/* SQL_CHOMP_SEGDIR
**   Update the start_block (:1) and root (:2) fields of the %_segdir
**   entry located on absolute level :3 with index :4.  */
/* 33 */  "UPDATE %Q.'%q_segdir' SET start_block = ?, root = ?"
            "WHERE level = ? AND idx = ?",

/* SQL_SEGMENT_IS_APPENDABLE
**   Return a single row if the segment with end_block=? is appendable. Or
**   no rows otherwise.  */
/* 34 */  "SELECT 1 FROM %Q.'%q_segments' WHERE blockid=? AND block IS NULL",

/* SQL_SELECT_INDEXES
**   Return the list of valid segment indexes for absolute level ?  */
/* 35 */  "SELECT idx FROM %Q.'%q_segdir' WHERE level=? ORDER BY 1 ASC",

/* SQL_SELECT_MXLEVEL
**   Return the largest relative level in the FTS index or indexes.  */
/* 36 */  "SELECT max( level %% 1024 ) FROM %Q.'%q_segdir'",

          /* Return segments in order from oldest to newest.*/
/* 37 */  "SELECT level, idx, end_block "
            "FROM %Q.'%q_segdir' WHERE level BETWEEN ? AND ? "
            "ORDER BY level DESC, idx ASC",

          /* Update statements used while promoting segments */
/* 38 */  "UPDATE OR FAIL %Q.'%q_segdir' SET level=-1,idx=? "
            "WHERE level=? AND idx=?",
/* 39 */  "UPDATE OR FAIL %Q.'%q_segdir' SET level=? WHERE level=-1"

  };
  int rc = SQLITE_OK;
  sqlite3_stmt *pStmt;

  assert( SizeofArray(azSql)==SizeofArray(p->aStmt) );
  assert( eStmt<SizeofArray(azSql) && eStmt>=0 );

  pStmt = p->aStmt[eStmt];
  if( !pStmt ){
    int f = SQLITE_PREPARE_PERSISTENT|SQLITE_PREPARE_NO_VTAB;
    char *zSql;
    if( eStmt==SQL_CONTENT_INSERT ){
      zSql = sqlite3_mprintf(azSql[eStmt], p->zDb, p->zName, p->zWriteExprlist);
    }else if( eStmt==SQL_SELECT_CONTENT_BY_ROWID ){
      f &= ~SQLITE_PREPARE_NO_VTAB;
      zSql = sqlite3_mprintf(azSql[eStmt], p->zReadExprlist);
    }else{
      zSql = sqlite3_mprintf(azSql[eStmt], p->zDb, p->zName);
    }
    if( !zSql ){
      rc = SQLITE_NOMEM;
    }else{
      rc = sqlite3_prepare_v3(p->db, zSql, -1, f, &pStmt, NULL);
      sqlite3_free(zSql);
      assert( rc==SQLITE_OK || pStmt==0 );
      p->aStmt[eStmt] = pStmt;
    }
  }
  if( apVal ){
    int i;
    int nParam = sqlite3_bind_parameter_count(pStmt);
    for(i=0; rc==SQLITE_OK && i<nParam; i++){
      rc = sqlite3_bind_value(pStmt, i+1, apVal[i]);
    }
  }
  *pp = pStmt;
  return rc;
}
