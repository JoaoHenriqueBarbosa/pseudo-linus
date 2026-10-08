
/* Return the number of cells in a node  */
#define NCELL(pNode) readInt16(&(pNode)->zData[2])

/*
** A single cell from a node, deserialized
*/
struct RtreeCell {
  i64 iRowid;                                 /* Node or entry ID */
  RtreeCoord aCoord[RTREE_MAX_DIMENSIONS*2];  /* Bounding box coordinates */
};


/*
** This object becomes the sqlite3_user_data() for the SQL functions
** that are created by sqlite3_rtree_geometry_callback() and
** sqlite3_rtree_query_callback() and which appear on the right of MATCH
** operators in order to constrain a search.
**
** xGeom and xQueryFunc are the callback functions.  Exactly one of
** xGeom and xQueryFunc fields is non-NULL, depending on whether the
** SQL function was created using sqlite3_rtree_geometry_callback() or
** sqlite3_rtree_query_callback().
**
** This object is deleted automatically by the destructor mechanism in
** sqlite3_create_function_v2().
*/
struct RtreeGeomCallback {
  int (*xGeom)(sqlite3_rtree_geometry*, int, RtreeDValue*, int*);
  int (*xQueryFunc)(sqlite3_rtree_query_info*);
  void (*xDestructor)(void*);
  void *pContext;
};

/*
** An instance of this structure (in the form of a BLOB) is returned by
** the SQL functions that sqlite3_rtree_geometry_callback() and
** sqlite3_rtree_query_callback() create, and is read as the right-hand
** operand to the MATCH operator of an R-Tree.
*/
struct RtreeMatchArg {
  u32 iSize;                  /* Size of this object */
  RtreeGeomCallback cb;       /* Info about the callback functions */
  int nParam;                 /* Number of parameters to the SQL function */
  sqlite3_value **apSqlParam; /* Original SQL parameter values */
  RtreeDValue aParam[1];      /* Values for parameters to the SQL function */
};

#ifndef MAX
# define MAX(x,y) ((x) < (y) ? (y) : (x))
#endif
#ifndef MIN
# define MIN(x,y) ((x) > (y) ? (y) : (x))
#endif

/* What version of GCC is being used.  0 means GCC is not being used .
** Note that the GCC_VERSION macro will also be set correctly when using
** clang, since clang works hard to be gcc compatible.  So the gcc
** optimizations will also work when compiling with clang.
*/
#ifndef GCC_VERSION
#if defined(__GNUC__) && !defined(SQLITE_DISABLE_INTRINSIC)
# define GCC_VERSION (__GNUC__*1000000+__GNUC_MINOR__*1000+__GNUC_PATCHLEVEL__)
#else
# define GCC_VERSION 0
#endif
#endif

/* The testcase() macro should already be defined in the amalgamation.  If
** it is not, make it a no-op.
*/
#ifndef SQLITE_AMALGAMATION
# if defined(SQLITE_COVERAGE_TEST) || defined(SQLITE_DEBUG)
    unsigned int sqlite3RtreeTestcase = 0;
#   define testcase(X)  if( X ){ sqlite3RtreeTestcase += __LINE__; }
# else
#   define testcase(X)
# endif
#endif

/*
** Make sure that the compiler intrinsics we desire are enabled when
** compiling with an appropriate version of MSVC unless prevented by
** the SQLITE_DISABLE_INTRINSIC define.
*/
#if !defined(SQLITE_DISABLE_INTRINSIC)
#  if defined(_MSC_VER) && _MSC_VER>=1400
#    if !defined(_WIN32_WCE)
/* #      include <intrin.h> */
#      pragma intrinsic(_byteswap_ulong)
#      pragma intrinsic(_byteswap_uint64)
#    else
/* #      include <cmnintrin.h> */
#    endif
#  endif
#endif

/*
** Macros to determine whether the machine is big or little endian,
** and whether or not that determination is run-time or compile-time.
**
** For best performance, an attempt is made to guess at the byte-order
** using C-preprocessor macros.  If that is unsuccessful, or if
** -DSQLITE_RUNTIME_BYTEORDER=1 is set, then byte-order is determined
** at run-time.
*/
#ifndef SQLITE_BYTEORDER /* Replicate changes at tag-20230904a */
# if defined(__BYTE_ORDER__) && __BYTE_ORDER__==__ORDER_BIG_ENDIAN__
#   define SQLITE_BYTEORDER 4321
# elif defined(__BYTE_ORDER__) && __BYTE_ORDER__==__ORDER_LITTLE_ENDIAN__
#   define SQLITE_BYTEORDER 1234
# elif defined(__BIG_ENDIAN__) && __BIG_ENDIAN__==1
#   define SQLITE_BYTEORDER 4321
# elif defined(i386)    || defined(__i386__)      || defined(_M_IX86) ||    \
     defined(__x86_64)  || defined(__x86_64__)    || defined(_M_X64)  ||    \
     defined(_M_AMD64)  || defined(_M_ARM)        || defined(__x86)   ||    \
     defined(__ARMEL__) || defined(__AARCH64EL__) || defined(_M_ARM64)
#   define SQLITE_BYTEORDER 1234
# elif defined(sparc)   || defined(__ARMEB__)     || defined(__AARCH64EB__)
#   define SQLITE_BYTEORDER 4321
# else
#   define SQLITE_BYTEORDER 0
# endif
#endif


/* What version of MSVC is being used.  0 means MSVC is not being used */
#ifndef MSVC_VERSION
#if defined(_MSC_VER) && !defined(SQLITE_DISABLE_INTRINSIC)
# define MSVC_VERSION _MSC_VER
#else
# define MSVC_VERSION 0
#endif
#endif

/*
** Functions to deserialize a 16 bit integer, 32 bit real number and
** 64 bit integer. The deserialized value is returned.
*/
static int readInt16(u8 *p){
  return (p[0]<<8) + p[1];
}
static void readCoord(u8 *p, RtreeCoord *pCoord){
  assert( FOUR_BYTE_ALIGNED(p) );
#if SQLITE_BYTEORDER==1234 && MSVC_VERSION>=1300
  pCoord->u = _byteswap_ulong(*(u32*)p);
#elif SQLITE_BYTEORDER==1234 && GCC_VERSION>=4003000
  pCoord->u = __builtin_bswap32(*(u32*)p);
#elif SQLITE_BYTEORDER==4321
  pCoord->u = *(u32*)p;
#else
  pCoord->u = (
    (((u32)p[0]) << 24) +
    (((u32)p[1]) << 16) +
    (((u32)p[2]) <<  8) +
    (((u32)p[3]) <<  0)
  );
#endif
}
static i64 readInt64(u8 *p){
#if SQLITE_BYTEORDER==1234 && MSVC_VERSION>=1300
  u64 x;
  memcpy(&x, p, 8);
  return (i64)_byteswap_uint64(x);
#elif SQLITE_BYTEORDER==1234 && GCC_VERSION>=4003000
  u64 x;
  memcpy(&x, p, 8);
  return (i64)__builtin_bswap64(x);
#elif SQLITE_BYTEORDER==4321
  i64 x;
  memcpy(&x, p, 8);
  return x;
#else
  return (i64)(
    (((u64)p[0]) << 56) +
    (((u64)p[1]) << 48) +
    (((u64)p[2]) << 40) +
    (((u64)p[3]) << 32) +
    (((u64)p[4]) << 24) +
    (((u64)p[5]) << 16) +
    (((u64)p[6]) <<  8) +
    (((u64)p[7]) <<  0)
  );
#endif
}

/*
** Functions to serialize a 16 bit integer, 32 bit real number and
** 64 bit integer. The value returned is the number of bytes written
** to the argument buffer (always 2, 4 and 8 respectively).
*/
static void writeInt16(u8 *p, int i){
  p[0] = (i>> 8)&0xFF;
  p[1] = (i>> 0)&0xFF;
}
static int writeCoord(u8 *p, RtreeCoord *pCoord){
  u32 i;
  assert( FOUR_BYTE_ALIGNED(p) );
  assert( sizeof(RtreeCoord)==4 );
  assert( sizeof(u32)==4 );
#if SQLITE_BYTEORDER==1234 && GCC_VERSION>=4003000
  i = __builtin_bswap32(pCoord->u);
  memcpy(p, &i, 4);
#elif SQLITE_BYTEORDER==1234 && MSVC_VERSION>=1300
  i = _byteswap_ulong(pCoord->u);
  memcpy(p, &i, 4);
#elif SQLITE_BYTEORDER==4321
  i = pCoord->u;
  memcpy(p, &i, 4);
#else
  i = pCoord->u;
  p[0] = (i>>24)&0xFF;
  p[1] = (i>>16)&0xFF;
  p[2] = (i>> 8)&0xFF;
  p[3] = (i>> 0)&0xFF;
#endif
  return 4;
}
static int writeInt64(u8 *p, i64 i){
#if SQLITE_BYTEORDER==1234 && GCC_VERSION>=4003000
  i = (i64)__builtin_bswap64((u64)i);
  memcpy(p, &i, 8);
#elif SQLITE_BYTEORDER==1234 && MSVC_VERSION>=1300
  i = (i64)_byteswap_uint64((u64)i);
  memcpy(p, &i, 8);
#elif SQLITE_BYTEORDER==4321
  memcpy(p, &i, 8);
#else
  p[0] = (i>>56)&0xFF;
  p[1] = (i>>48)&0xFF;
  p[2] = (i>>40)&0xFF;
  p[3] = (i>>32)&0xFF;
  p[4] = (i>>24)&0xFF;
  p[5] = (i>>16)&0xFF;
  p[6] = (i>> 8)&0xFF;
  p[7] = (i>> 0)&0xFF;
#endif
  return 8;
}

/*
** Increment the reference count of node p.
*/
static void nodeReference(RtreeNode *p){
  if( p ){
    assert( p->nRef>0 );
    p->nRef++;
  }
}

/*
** Clear the content of node p (set all bytes to 0x00).
*/
static void nodeZero(Rtree *pRtree, RtreeNode *p){
  memset(&p->zData[2], 0, pRtree->iNodeSize-2);
  p->isDirty = 1;
}

/*
** Given a node number iNode, return the corresponding key to use
** in the Rtree.aHash table.
*/
static unsigned int nodeHash(i64 iNode){
  return ((unsigned)iNode) % HASHSIZE;
}

/*
** Search the node hash table for node iNode. If found, return a pointer
** to it. Otherwise, return 0.
*/
static RtreeNode *nodeHashLookup(Rtree *pRtree, i64 iNode){
  RtreeNode *p;
  for(p=pRtree->aHash[nodeHash(iNode)]; p && p->iNode!=iNode; p=p->pNext);
  return p;
}

/*
** Add node pNode to the node hash table.
*/
static void nodeHashInsert(Rtree *pRtree, RtreeNode *pNode){
  int iHash;
  assert( pNode->pNext==0 );
  iHash = nodeHash(pNode->iNode);
  pNode->pNext = pRtree->aHash[iHash];
  pRtree->aHash[iHash] = pNode;
}

/*
** Remove node pNode from the node hash table.
*/
static void nodeHashDelete(Rtree *pRtree, RtreeNode *pNode){
  RtreeNode **pp;
  if( pNode->iNode!=0 ){
    pp = &pRtree->aHash[nodeHash(pNode->iNode)];
    for( ; (*pp)!=pNode; pp = &(*pp)->pNext){ assert(*pp); }
    *pp = pNode->pNext;
    pNode->pNext = 0;
  }
}

/*
** Allocate and return new r-tree node. Initially, (RtreeNode.iNode==0),
** indicating that node has not yet been assigned a node number. It is
** assigned a node number when nodeWrite() is called to write the
** node contents out to the database.
*/
static RtreeNode *nodeNew(Rtree *pRtree, RtreeNode *pParent){
  RtreeNode *pNode;
  pNode = (RtreeNode *)sqlite3_malloc64(sizeof(RtreeNode) + pRtree->iNodeSize);
  if( pNode ){
    memset(pNode, 0, sizeof(RtreeNode) + pRtree->iNodeSize);
    pNode->zData = (u8 *)&pNode[1];
    pNode->nRef = 1;
    pRtree->nNodeRef++;
    pNode->pParent = pParent;
    pNode->isDirty = 1;
    nodeReference(pParent);
  }
  return pNode;
}

/*
** Clear the Rtree.pNodeBlob object
*/
static void nodeBlobReset(Rtree *pRtree){
  sqlite3_blob *pBlob = pRtree->pNodeBlob;
  pRtree->pNodeBlob = 0;
  sqlite3_blob_close(pBlob);
}

/*
** Obtain a reference to an r-tree node.
*/
static int nodeAcquire(
  Rtree *pRtree,             /* R-tree structure */
  i64 iNode,                 /* Node number to load */
  RtreeNode *pParent,        /* Either the parent node or NULL */
  RtreeNode **ppNode         /* OUT: Acquired node */
){
  int rc = SQLITE_OK;
  RtreeNode *pNode = 0;

  /* Check if the requested node is already in the hash table. If so,
  ** increase its reference count and return it.
  */
  if( (pNode = nodeHashLookup(pRtree, iNode))!=0 ){
    if( pParent && ALWAYS(pParent!=pNode->pParent) ){
      RTREE_IS_CORRUPT(pRtree);
      return SQLITE_CORRUPT_VTAB;
    }
    pNode->nRef++;
    *ppNode = pNode;
    return SQLITE_OK;
  }

  if( pRtree->pNodeBlob ){
    sqlite3_blob *pBlob = pRtree->pNodeBlob;
    pRtree->pNodeBlob = 0;
    rc = sqlite3_blob_reopen(pBlob, iNode);
    pRtree->pNodeBlob = pBlob;
    if( rc ){
      nodeBlobReset(pRtree);
      if( rc==SQLITE_NOMEM ) return SQLITE_NOMEM;
    }
  }
  if( pRtree->pNodeBlob==0 ){
    rc = sqlite3_blob_open(pRtree->db, pRtree->zDb, pRtree->zNodeName,
                           "data", iNode, 0,
                           &pRtree->pNodeBlob);
  }
  if( rc ){
    *ppNode = 0;
    /* If unable to open an sqlite3_blob on the desired row, that can only
    ** be because the shadow tables hold erroneous data. */
    if( rc==SQLITE_ERROR ){
      rc = SQLITE_CORRUPT_VTAB;
      RTREE_IS_CORRUPT(pRtree);
    }
  }else if( pRtree->iNodeSize==sqlite3_blob_bytes(pRtree->pNodeBlob) ){
    pNode = (RtreeNode *)sqlite3_malloc64(sizeof(RtreeNode)+pRtree->iNodeSize);
    if( !pNode ){
      rc = SQLITE_NOMEM;
    }else{
      pNode->pParent = pParent;
      pNode->zData = (u8 *)&pNode[1];
      pNode->nRef = 1;
      pRtree->nNodeRef++;
      pNode->iNode = iNode;
      pNode->isDirty = 0;
      pNode->pNext = 0;
      rc = sqlite3_blob_read(pRtree->pNodeBlob, pNode->zData,
                             pRtree->iNodeSize, 0);
    }
  }

  /* If the root node was just loaded, set pRtree->iDepth to the height
  ** of the r-tree structure. A height of zero means all data is stored on
  ** the root node. A height of one means the children of the root node
  ** are the leaves, and so on. If the depth as specified on the root node
  ** is greater than RTREE_MAX_DEPTH, the r-tree structure must be corrupt.
  */
  if( rc==SQLITE_OK && pNode && iNode==1 ){
    pRtree->iDepth = readInt16(pNode->zData);
    if( pRtree->iDepth>RTREE_MAX_DEPTH ){
      rc = SQLITE_CORRUPT_VTAB;
      RTREE_IS_CORRUPT(pRtree);
    }
  }

  /* If no error has occurred so far, check if the "number of entries"
  ** field on the node is too large. If so, set the return code to
  ** SQLITE_CORRUPT_VTAB.
  */
  if( pNode && rc==SQLITE_OK ){
    if( NCELL(pNode)>((pRtree->iNodeSize-4)/pRtree->nBytesPerCell) ){
      rc = SQLITE_CORRUPT_VTAB;
      RTREE_IS_CORRUPT(pRtree);
    }
  }

  if( rc==SQLITE_OK ){
    if( pNode!=0 ){
      nodeReference(pParent);
      nodeHashInsert(pRtree, pNode);
    }else{
      rc = SQLITE_CORRUPT_VTAB;
      RTREE_IS_CORRUPT(pRtree);
    }
    *ppNode = pNode;
  }else{
    nodeBlobReset(pRtree);
    if( pNode ){
      pRtree->nNodeRef--;
      sqlite3_free(pNode);
    }
    *ppNode = 0;
  }

  return rc;
}
