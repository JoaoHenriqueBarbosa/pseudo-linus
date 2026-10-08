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
** This file contains code for implementations of the r-tree and r*-tree
** algorithms packaged as an SQLite virtual table module.
*/

/*
** Database Format of R-Tree Tables
** --------------------------------
**
** The data structure for a single virtual r-tree table is stored in three
** native SQLite tables declared as follows. In each case, the '%' character
** in the table name is replaced with the user-supplied name of the r-tree
** table.
**
**   CREATE TABLE %_node(nodeno INTEGER PRIMARY KEY, data BLOB)
**   CREATE TABLE %_parent(nodeno INTEGER PRIMARY KEY, parentnode INTEGER)
**   CREATE TABLE %_rowid(rowid INTEGER PRIMARY KEY, nodeno INTEGER, ...)
**
** The data for each node of the r-tree structure is stored in the %_node
** table. For each node that is not the root node of the r-tree, there is
** an entry in the %_parent table associating the node with its parent.
** And for each row of data in the table, there is an entry in the %_rowid
** table that maps from the entries rowid to the id of the node that it
** is stored on.  If the r-tree contains auxiliary columns, those are stored
** on the end of the %_rowid table.
**
** The root node of an r-tree always exists, even if the r-tree table is
** empty. The nodeno of the root node is always 1. All other nodes in the
** table must be the same size as the root node. The content of each node
** is formatted as follows:
**
**   1. If the node is the root node (node 1), then the first 2 bytes
**      of the node contain the tree depth as a big-endian integer.
**      For non-root nodes, the first 2 bytes are left unused.
**
**   2. The next 2 bytes contain the number of entries currently
**      stored in the node.
**
**   3. The remainder of the node contains the node entries. Each entry
**      consists of a single 8-byte integer followed by an even number
**      of 4-byte coordinates. For leaf nodes the integer is the rowid
**      of a record. For internal nodes it is the node number of a
**      child page.
*/

#if !defined(SQLITE_CORE) \
  || (defined(SQLITE_ENABLE_RTREE) && !defined(SQLITE_OMIT_VIRTUALTABLE))

#ifndef SQLITE_CORE
/*   #include "sqlite3ext.h" */
  SQLITE_EXTENSION_INIT1
#else
/*   #include "sqlite3.h" */
#endif
SQLITE_PRIVATE int sqlite3GetToken(const unsigned char*,int*); /* In the SQLite core */

/*
** If building separately, we will need some setup that is normally
** found in sqliteInt.h
*/
#if !defined(SQLITE_AMALGAMATION)
#include "sqlite3rtree.h"
typedef sqlite3_int64 i64;
typedef sqlite3_uint64 u64;
typedef unsigned char u8;
typedef unsigned short u16;
typedef unsigned int u32;
#if !defined(NDEBUG) && !defined(SQLITE_DEBUG)
# define NDEBUG 1
#endif
#if defined(NDEBUG) && defined(SQLITE_DEBUG)
# undef NDEBUG
#endif
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
#endif /* !defined(SQLITE_AMALGAMATION) */

/* Macro to check for 4-byte alignment.  Only used inside of assert() */
#ifdef SQLITE_DEBUG
# define FOUR_BYTE_ALIGNED(X)  ((((char*)(X) - (char*)0) & 3)==0)
#endif

/* #include <string.h> */
/* #include <stdio.h> */
/* #include <assert.h> */
/* #include <stdlib.h> */

/*  The following macro is used to suppress compiler warnings.
*/
#ifndef UNUSED_PARAMETER
# define UNUSED_PARAMETER(x) (void)(x)
#endif

typedef struct Rtree Rtree;
typedef struct RtreeCursor RtreeCursor;
typedef struct RtreeNode RtreeNode;
typedef struct RtreeCell RtreeCell;
typedef struct RtreeConstraint RtreeConstraint;
typedef struct RtreeMatchArg RtreeMatchArg;
typedef struct RtreeGeomCallback RtreeGeomCallback;
typedef union RtreeCoord RtreeCoord;
typedef struct RtreeSearchPoint RtreeSearchPoint;

/* The rtree may have between 1 and RTREE_MAX_DIMENSIONS dimensions. */
#define RTREE_MAX_DIMENSIONS 5

/* Maximum number of auxiliary columns */
#define RTREE_MAX_AUX_COLUMN 100

/* Size of hash table Rtree.aHash. This hash table is not expected to
** ever contain very many entries, so a fixed number of buckets is
** used.
*/
#define HASHSIZE 97

/* The xBestIndex method of this virtual table requires an estimate of
** the number of rows in the virtual table to calculate the costs of
** various strategies. If possible, this estimate is loaded from the
** sqlite_stat1 table (with RTREE_MIN_ROWEST as a hard-coded minimum).
** Otherwise, if no sqlite_stat1 entry is available, use
** RTREE_DEFAULT_ROWEST.
*/
#define RTREE_DEFAULT_ROWEST 1048576
#define RTREE_MIN_ROWEST         100

/*
** An rtree virtual-table object.
*/
struct Rtree {
  sqlite3_vtab base;          /* Base class.  Must be first */
  sqlite3 *db;                /* Host database connection */
  int iNodeSize;              /* Size in bytes of each node in the node table */
  u8 nDim;                    /* Number of dimensions */
  u8 nDim2;                   /* Twice the number of dimensions */
  u8 eCoordType;              /* RTREE_COORD_REAL32 or RTREE_COORD_INT32 */
  u8 nBytesPerCell;           /* Bytes consumed per cell */
  u8 inWrTrans;               /* True if inside write transaction */
  u8 nAux;                    /* # of auxiliary columns in %_rowid */
#ifdef SQLITE_ENABLE_GEOPOLY
  u8 nAuxNotNull;             /* Number of initial not-null aux columns */
#endif
#ifdef SQLITE_DEBUG
  u8 bCorrupt;                /* Shadow table corruption detected */
#endif
  int iDepth;                 /* Current depth of the r-tree structure */
  char *zDb;                  /* Name of database containing r-tree table */
  char *zName;                /* Name of r-tree table */
  char *zNodeName;            /* Name of the %_node table */
  u32 nBusy;                  /* Current number of users of this structure */
  i64 nRowEst;                /* Estimated number of rows in this table */
  u32 nCursor;                /* Number of open cursors */
  u32 nNodeRef;               /* Number RtreeNodes with positive nRef */
  char *zReadAuxSql;          /* SQL for statement to read aux data */

  /* List of nodes removed during a CondenseTree operation. List is
  ** linked together via the pointer normally used for hash chains -
  ** RtreeNode.pNext. RtreeNode.iNode stores the depth of the sub-tree
  ** headed by the node (leaf nodes have RtreeNode.iNode==0).
  */
  RtreeNode *pDeleted;

  /* Blob I/O on xxx_node */
  sqlite3_blob *pNodeBlob;

  /* Statements to read/write/delete a record from xxx_node */
  sqlite3_stmt *pWriteNode;
  sqlite3_stmt *pDeleteNode;

  /* Statements to read/write/delete a record from xxx_rowid */
  sqlite3_stmt *pReadRowid;
  sqlite3_stmt *pWriteRowid;
  sqlite3_stmt *pDeleteRowid;

  /* Statements to read/write/delete a record from xxx_parent */
  sqlite3_stmt *pReadParent;
  sqlite3_stmt *pWriteParent;
  sqlite3_stmt *pDeleteParent;

  /* Statement for writing to the "aux:" fields, if there are any */
  sqlite3_stmt *pWriteAux;

  RtreeNode *aHash[HASHSIZE]; /* Hash table of in-memory nodes. */
};

/* Possible values for Rtree.eCoordType: */
#define RTREE_COORD_REAL32 0
#define RTREE_COORD_INT32  1

/*
** If SQLITE_RTREE_INT_ONLY is defined, then this virtual table will
** only deal with integer coordinates.  No floating point operations
** will be done.
*/
#ifdef SQLITE_RTREE_INT_ONLY
  typedef sqlite3_int64 RtreeDValue;       /* High accuracy coordinate */
  typedef int RtreeValue;                  /* Low accuracy coordinate */
# define RTREE_ZERO 0
#else
  typedef double RtreeDValue;              /* High accuracy coordinate */
  typedef float RtreeValue;                /* Low accuracy coordinate */
# define RTREE_ZERO 0.0
#endif

/*
** Set the Rtree.bCorrupt flag
*/
#ifdef SQLITE_DEBUG
# define RTREE_IS_CORRUPT(X) ((X)->bCorrupt = 1)
#else
# define RTREE_IS_CORRUPT(X)
#endif

/*
** When doing a search of an r-tree, instances of the following structure
** record intermediate results from the tree walk.
**
** The id is always a node-id.  For iLevel>=1 the id is the node-id of
** the node that the RtreeSearchPoint represents.  When iLevel==0, however,
** the id is of the parent node and the cell that RtreeSearchPoint
** represents is the iCell-th entry in the parent node.
*/
struct RtreeSearchPoint {
  RtreeDValue rScore;    /* The score for this node.  Smallest goes first. */
  sqlite3_int64 id;      /* Node ID */
  u8 iLevel;             /* 0=entries.  1=leaf node.  2+ for higher */
  u8 eWithin;            /* PARTLY_WITHIN or FULLY_WITHIN */
  u8 iCell;              /* Cell index within the node */
};

/*
** The minimum number of cells allowed for a node is a third of the
** maximum. In Gutman's notation:
**
**     m = M/3
**
** If an R*-tree "Reinsert" operation is required, the same number of
** cells are removed from the overfull node and reinserted into the tree.
*/
#define RTREE_MINCELLS(p) ((((p)->iNodeSize-4)/(p)->nBytesPerCell)/3)
#define RTREE_REINSERT(p) RTREE_MINCELLS(p)
#define RTREE_MAXCELLS 51

/*
** The smallest possible node-size is (512-64)==448 bytes. And the largest
** supported cell size is 48 bytes (8 byte rowid + ten 4 byte coordinates).
** Therefore all non-root nodes must contain at least 3 entries. Since
** 3^40 is greater than 2^64, an r-tree structure always has a depth of
** 40 or less.
*/
#define RTREE_MAX_DEPTH 40


/*
** Number of entries in the cursor RtreeNode cache.  The first entry is
** used to cache the RtreeNode for RtreeCursor.sPoint.  The remaining
** entries cache the RtreeNode for the first elements of the priority queue.
*/
#define RTREE_CACHE_SZ  5

/*
** An rtree cursor object.
*/
struct RtreeCursor {
  sqlite3_vtab_cursor base;         /* Base class.  Must be first */
  u8 atEOF;                         /* True if at end of search */
  u8 bPoint;                        /* True if sPoint is valid */
  u8 bAuxValid;                     /* True if pReadAux is valid */
  int iStrategy;                    /* Copy of idxNum search parameter */
  int nConstraint;                  /* Number of entries in aConstraint */
  RtreeConstraint *aConstraint;     /* Search constraints. */
  int nPointAlloc;                  /* Number of slots allocated for aPoint[] */
  int nPoint;                       /* Number of slots used in aPoint[] */
  int mxLevel;                      /* iLevel value for root of the tree */
  RtreeSearchPoint *aPoint;         /* Priority queue for search points */
  sqlite3_stmt *pReadAux;           /* Statement to read aux-data */
  RtreeSearchPoint sPoint;          /* Cached next search point */
  RtreeNode *aNode[RTREE_CACHE_SZ]; /* Rtree node cache */
  u32 anQueue[RTREE_MAX_DEPTH+1];   /* Number of queued entries by iLevel */
};

/* Return the Rtree of a RtreeCursor */
#define RTREE_OF_CURSOR(X)   ((Rtree*)((X)->base.pVtab))

/*
** A coordinate can be either a floating point number or a integer.  All
** coordinates within a single R-Tree are always of the same time.
*/
union RtreeCoord {
  RtreeValue f;      /* Floating point value */
  int i;             /* Integer value */
  u32 u;             /* Unsigned for byte-order conversions */
};

/*
** The argument is an RtreeCoord. Return the value stored within the RtreeCoord
** formatted as a RtreeDValue (double or int64). This macro assumes that local
** variable pRtree points to the Rtree structure associated with the
** RtreeCoord.
*/
#ifdef SQLITE_RTREE_INT_ONLY
# define DCOORD(coord) ((RtreeDValue)coord.i)
#else
# define DCOORD(coord) (                           \
    (pRtree->eCoordType==RTREE_COORD_REAL32) ?      \
      ((double)coord.f) :                           \
      ((double)coord.i)                             \
  )
#endif

/*
** A search constraint.
*/
struct RtreeConstraint {
  int iCoord;                     /* Index of constrained coordinate */
  int op;                         /* Constraining operation */
  union {
    RtreeDValue rValue;             /* Constraint value. */
    int (*xGeom)(sqlite3_rtree_geometry*,int,RtreeDValue*,int*);
    int (*xQueryFunc)(sqlite3_rtree_query_info*);
  } u;
  sqlite3_rtree_query_info *pInfo;  /* xGeom and xQueryFunc argument */
};

/* Possible values for RtreeConstraint.op */
#define RTREE_EQ    0x41  /* A */
#define RTREE_LE    0x42  /* B */
#define RTREE_LT    0x43  /* C */
#define RTREE_GE    0x44  /* D */
#define RTREE_GT    0x45  /* E */
#define RTREE_MATCH 0x46  /* F: Old-style sqlite3_rtree_geometry_callback() */
#define RTREE_QUERY 0x47  /* G: New-style sqlite3_rtree_query_callback() */

/* Special operators available only on cursors.  Needs to be consecutive
** with the normal values above, but must be less than RTREE_MATCH.  These
** are used in the cursor for contraints such as x=NULL (RTREE_FALSE) or
** x<'xyz' (RTREE_TRUE) */
#define RTREE_TRUE  0x3f  /* ? */
#define RTREE_FALSE 0x40  /* @ */

/*
** An rtree structure node.
*/
struct RtreeNode {
  RtreeNode *pParent;         /* Parent node */
  i64 iNode;                  /* The node number */
  int nRef;                   /* Number of references to this node */
  int isDirty;                /* True if the node needs to be written to disk */
  u8 *zData;                  /* Content of the node, as should be on disk */
  RtreeNode *pNext;           /* Next node in this hash collision chain */
};
