
/*
** Merge the entire database so that there is one segment for each
** iIndex/iLangid combination.
*/
static int fts3DoOptimize(Fts3Table *p, int bReturnDone){
  int bSeenDone = 0;
  int rc;
  sqlite3_stmt *pAllLangid = 0;

  rc = sqlite3Fts3PendingTermsFlush(p);
  if( rc==SQLITE_OK ){
    rc = fts3SqlStmt(p, SQL_SELECT_ALL_LANGID, &pAllLangid, 0);
  }
  if( rc==SQLITE_OK ){
    int rc2;
    sqlite3_bind_int(pAllLangid, 1, p->iPrevLangid);
    sqlite3_bind_int(pAllLangid, 2, p->nIndex);
    while( sqlite3_step(pAllLangid)==SQLITE_ROW ){
      int i;
      int iLangid = sqlite3_column_int(pAllLangid, 0);
      for(i=0; rc==SQLITE_OK && i<p->nIndex; i++){
        rc = fts3SegmentMerge(p, iLangid, i, FTS3_SEGCURSOR_ALL);
        if( rc==SQLITE_DONE ){
          bSeenDone = 1;
          rc = SQLITE_OK;
        }
      }
    }
    rc2 = sqlite3_reset(pAllLangid);
    if( rc==SQLITE_OK ) rc = rc2;
  }

  sqlite3Fts3SegmentsClose(p);

  return (rc==SQLITE_OK && bReturnDone && bSeenDone) ? SQLITE_DONE : rc;
}

/*
** This function is called when the user executes the following statement:
**
**     INSERT INTO <tbl>(<tbl>) VALUES('rebuild');
**
** The entire FTS index is discarded and rebuilt. If the table is one
** created using the content=xxx option, then the new index is based on
** the current contents of the xxx table. Otherwise, it is rebuilt based
** on the contents of the %_content table.
*/
static int fts3DoRebuild(Fts3Table *p){
  int rc;                         /* Return Code */

  rc = fts3DeleteAll(p, 0);
  if( rc==SQLITE_OK ){
    u32 *aSz = 0;
    u32 *aSzIns = 0;
    u32 *aSzDel = 0;
    sqlite3_stmt *pStmt = 0;
    int nEntry = 0;

    /* Compose and prepare an SQL statement to loop through the content table */
    char *zSql = sqlite3_mprintf("SELECT %s" , p->zReadExprlist);
    if( !zSql ){
      rc = SQLITE_NOMEM;
    }else{
      rc = sqlite3_prepare_v2(p->db, zSql, -1, &pStmt, 0);
      sqlite3_free(zSql);
    }

    if( rc==SQLITE_OK ){
      sqlite3_int64 nByte = sizeof(u32) * ((sqlite3_int64)p->nColumn+1)*3;
      aSz = (u32 *)sqlite3_malloc64(nByte);
      if( aSz==0 ){
        rc = SQLITE_NOMEM;
      }else{
        memset(aSz, 0, nByte);
        aSzIns = &aSz[p->nColumn+1];
        aSzDel = &aSzIns[p->nColumn+1];
      }
    }

    while( rc==SQLITE_OK && SQLITE_ROW==sqlite3_step(pStmt) ){
      int iCol;
      int iLangid = langidFromSelect(p, pStmt);
      rc = fts3PendingTermsDocid(p, 0, iLangid, sqlite3_column_int64(pStmt, 0));
      memset(aSz, 0, sizeof(aSz[0]) * (p->nColumn+1));
      for(iCol=0; rc==SQLITE_OK && iCol<p->nColumn; iCol++){
        if( p->abNotindexed[iCol]==0 ){
          const char *z = (const char *) sqlite3_column_text(pStmt, iCol+1);
          rc = fts3PendingTermsAdd(p, iLangid, z, iCol, &aSz[iCol]);
          aSz[p->nColumn] += sqlite3_column_bytes(pStmt, iCol+1);
        }
      }
      if( p->bHasDocsize ){
        fts3InsertDocsize(&rc, p, aSz);
      }
      if( rc!=SQLITE_OK ){
        sqlite3_finalize(pStmt);
        pStmt = 0;
      }else{
        nEntry++;
        for(iCol=0; iCol<=p->nColumn; iCol++){
          aSzIns[iCol] += aSz[iCol];
        }
      }
    }
    if( p->bFts4 ){
      fts3UpdateDocTotals(&rc, p, aSzIns, aSzDel, nEntry);
    }
    sqlite3_free(aSz);

    if( pStmt ){
      int rc2 = sqlite3_finalize(pStmt);
      if( rc==SQLITE_OK ){
        rc = rc2;
      }
    }
  }

  return rc;
}


/*
** This function opens a cursor used to read the input data for an
** incremental merge operation. Specifically, it opens a cursor to scan
** the oldest nSeg segments (idx=0 through idx=(nSeg-1)) in absolute
** level iAbsLevel.
*/
static int fts3IncrmergeCsr(
  Fts3Table *p,                   /* FTS3 table handle */
  sqlite3_int64 iAbsLevel,        /* Absolute level to open */
  int nSeg,                       /* Number of segments to merge */
  Fts3MultiSegReader *pCsr        /* Cursor object to populate */
){
  int rc;                         /* Return Code */
  sqlite3_stmt *pStmt = 0;        /* Statement used to read %_segdir entry */
  sqlite3_int64 nByte;            /* Bytes allocated at pCsr->apSegment[] */

  /* Allocate space for the Fts3MultiSegReader.aCsr[] array */
  memset(pCsr, 0, sizeof(*pCsr));
  nByte = sizeof(Fts3SegReader *) * nSeg;
  pCsr->apSegment = (Fts3SegReader **)sqlite3_malloc64(nByte);

  if( pCsr->apSegment==0 ){
    rc = SQLITE_NOMEM;
  }else{
    memset(pCsr->apSegment, 0, nByte);
    rc = fts3SqlStmt(p, SQL_SELECT_LEVEL, &pStmt, 0);
  }
  if( rc==SQLITE_OK ){
    int i;
    int rc2;
    sqlite3_bind_int64(pStmt, 1, iAbsLevel);
    assert( pCsr->nSegment==0 );
    for(i=0; rc==SQLITE_OK && sqlite3_step(pStmt)==SQLITE_ROW && i<nSeg; i++){
      rc = sqlite3Fts3SegReaderNew(i, 0,
          sqlite3_column_int64(pStmt, 1),        /* segdir.start_block */
          sqlite3_column_int64(pStmt, 2),        /* segdir.leaves_end_block */
          sqlite3_column_int64(pStmt, 3),        /* segdir.end_block */
          sqlite3_column_blob(pStmt, 4),         /* segdir.root */
          sqlite3_column_bytes(pStmt, 4),        /* segdir.root */
          &pCsr->apSegment[i]
      );
      pCsr->nSegment++;
    }
    rc2 = sqlite3_reset(pStmt);
    if( rc==SQLITE_OK ) rc = rc2;
  }

  return rc;
}

typedef struct IncrmergeWriter IncrmergeWriter;
typedef struct NodeWriter NodeWriter;
typedef struct Blob Blob;
typedef struct NodeReader NodeReader;

/*
** An instance of the following structure is used as a dynamic buffer
** to build up nodes or other blobs of data in.
**
** The function blobGrowBuffer() is used to extend the allocation.
*/
struct Blob {
  char *a;                        /* Pointer to allocation */
  int n;                          /* Number of valid bytes of data in a[] */
  int nAlloc;                     /* Allocated size of a[] (nAlloc>=n) */
};

/*
** This structure is used to build up buffers containing segment b-tree
** nodes (blocks).
*/
struct NodeWriter {
  sqlite3_int64 iBlock;           /* Current block id */
  Blob key;                       /* Last key written to the current block */
  Blob block;                     /* Current block image */
};

/*
** An object of this type contains the state required to create or append
** to an appendable b-tree segment.
*/
struct IncrmergeWriter {
  int nLeafEst;                   /* Space allocated for leaf blocks */
  int nWork;                      /* Number of leaf pages flushed */
  sqlite3_int64 iAbsLevel;        /* Absolute level of input segments */
  int iIdx;                       /* Index of *output* segment in iAbsLevel+1 */
  sqlite3_int64 iStart;           /* Block number of first allocated block */
  sqlite3_int64 iEnd;             /* Block number of last allocated block */
  sqlite3_int64 nLeafData;        /* Bytes of leaf page data so far */
  u8 bNoLeafData;                 /* If true, store 0 for segment size */
  NodeWriter aNodeWriter[FTS_MAX_APPENDABLE_HEIGHT];
};

/*
** An object of the following type is used to read data from a single
** FTS segment node. See the following functions:
**
**     nodeReaderInit()
**     nodeReaderNext()
**     nodeReaderRelease()
*/
struct NodeReader {
  const char *aNode;
  int nNode;
  int iOff;                       /* Current offset within aNode[] */

  /* Output variables. Containing the current node entry. */
  sqlite3_int64 iChild;           /* Pointer to child node */
  Blob term;                      /* Current term */
  const char *aDoclist;           /* Pointer to doclist */
  int nDoclist;                   /* Size of doclist in bytes */
};

/*
** If *pRc is not SQLITE_OK when this function is called, it is a no-op.
** Otherwise, if the allocation at pBlob->a is not already at least nMin
** bytes in size, extend (realloc) it to be so.
**
** If an OOM error occurs, set *pRc to SQLITE_NOMEM and leave pBlob->a
** unmodified. Otherwise, if the allocation succeeds, update pBlob->nAlloc
** to reflect the new size of the pBlob->a[] buffer.
*/
static void blobGrowBuffer(Blob *pBlob, int nMin, int *pRc){
  if( *pRc==SQLITE_OK && nMin>pBlob->nAlloc ){
    int nAlloc = nMin;
    char *a = (char *)sqlite3_realloc64(pBlob->a, nAlloc);
    if( a ){
      pBlob->nAlloc = nAlloc;
      pBlob->a = a;
    }else{
      *pRc = SQLITE_NOMEM;
    }
  }
}

/*
** Attempt to advance the node-reader object passed as the first argument to
** the next entry on the node.
**
** Return an error code if an error occurs (SQLITE_NOMEM is possible).
** Otherwise return SQLITE_OK. If there is no next entry on the node
** (e.g. because the current entry is the last) set NodeReader->aNode to
** NULL to indicate EOF. Otherwise, populate the NodeReader structure output
** variables for the new entry.
*/
static int nodeReaderNext(NodeReader *p){
  int bFirst = (p->term.n==0);    /* True for first term on the node */
  int nPrefix = 0;                /* Bytes to copy from previous term */
  int nSuffix = 0;                /* Bytes to append to the prefix */
  int rc = SQLITE_OK;             /* Return code */

  assert( p->aNode );
  if( p->iChild && bFirst==0 ) p->iChild++;
  if( p->iOff>=p->nNode ){
    /* EOF */
    p->aNode = 0;
  }else{
    if( bFirst==0 ){
      p->iOff += fts3GetVarint32(&p->aNode[p->iOff], &nPrefix);
    }
    p->iOff += fts3GetVarint32(&p->aNode[p->iOff], &nSuffix);

    if( nPrefix>p->term.n || nSuffix>p->nNode-p->iOff || nSuffix==0 ){
      return FTS_CORRUPT_VTAB;
    }
    blobGrowBuffer(&p->term, nPrefix+nSuffix, &rc);
    if( rc==SQLITE_OK && ALWAYS(p->term.a!=0) ){
      memcpy(&p->term.a[nPrefix], &p->aNode[p->iOff], nSuffix);
      p->term.n = nPrefix+nSuffix;
      p->iOff += nSuffix;
      if( p->iChild==0 ){
        p->iOff += fts3GetVarint32(&p->aNode[p->iOff], &p->nDoclist);
        if( (p->nNode-p->iOff)<p->nDoclist ){
          return FTS_CORRUPT_VTAB;
        }
        p->aDoclist = &p->aNode[p->iOff];
        p->iOff += p->nDoclist;
      }
    }
  }

  assert_fts3_nc( p->iOff<=p->nNode );
  return rc;
}

/*
** Release all dynamic resources held by node-reader object *p.
*/
static void nodeReaderRelease(NodeReader *p){
  sqlite3_free(p->term.a);
}

/*
** Initialize a node-reader object to read the node in buffer aNode/nNode.
**
** If successful, SQLITE_OK is returned and the NodeReader object set to
** point to the first entry on the node (if any). Otherwise, an SQLite
** error code is returned.
*/
static int nodeReaderInit(NodeReader *p, const char *aNode, int nNode){
  memset(p, 0, sizeof(NodeReader));
  p->aNode = aNode;
  p->nNode = nNode;

  /* Figure out if this is a leaf or an internal node. */
  if( aNode && aNode[0] ){
    /* An internal node. */
    p->iOff = 1 + sqlite3Fts3GetVarint(&p->aNode[1], &p->iChild);
  }else{
    p->iOff = 1;
  }

  return aNode ? nodeReaderNext(p) : SQLITE_OK;
}

/*
** This function is called while writing an FTS segment each time a leaf o
** node is finished and written to disk. The key (zTerm/nTerm) is guaranteed
** to be greater than the largest key on the node just written, but smaller
** than or equal to the first key that will be written to the next leaf
** node.
**
** The block id of the leaf node just written to disk may be found in
** (pWriter->aNodeWriter[0].iBlock) when this function is called.
*/
static int fts3IncrmergePush(
  Fts3Table *p,                   /* Fts3 table handle */
  IncrmergeWriter *pWriter,       /* Writer object */
  const char *zTerm,              /* Term to write to internal node */
  int nTerm                       /* Bytes at zTerm */
){
  sqlite3_int64 iPtr = pWriter->aNodeWriter[0].iBlock;
  int iLayer;

  assert( nTerm>0 );
  for(iLayer=1; ALWAYS(iLayer<FTS_MAX_APPENDABLE_HEIGHT); iLayer++){
    sqlite3_int64 iNextPtr = 0;
    NodeWriter *pNode = &pWriter->aNodeWriter[iLayer];
    int rc = SQLITE_OK;
    int nPrefix;
    int nSuffix;
    int nSpace;

    /* Figure out how much space the key will consume if it is written to
    ** the current node of layer iLayer. Due to the prefix compression,
    ** the space required changes depending on which node the key is to
    ** be added to.  */
    nPrefix = fts3PrefixCompress(pNode->key.a, pNode->key.n, zTerm, nTerm);
    nSuffix = nTerm - nPrefix;
    if(nSuffix<=0 ) return FTS_CORRUPT_VTAB;
    nSpace  = sqlite3Fts3VarintLen(nPrefix);
    nSpace += sqlite3Fts3VarintLen(nSuffix) + nSuffix;

    if( pNode->key.n==0 || (pNode->block.n + nSpace)<=p->nNodeSize ){
      /* If the current node of layer iLayer contains zero keys, or if adding
      ** the key to it will not cause it to grow to larger than nNodeSize
      ** bytes in size, write the key here.  */

      Blob *pBlk = &pNode->block;
      if( pBlk->n==0 ){
        blobGrowBuffer(pBlk, p->nNodeSize, &rc);
        if( rc==SQLITE_OK ){
          pBlk->a[0] = (char)iLayer;
          pBlk->n = 1 + sqlite3Fts3PutVarint(&pBlk->a[1], iPtr);
        }
      }
      blobGrowBuffer(pBlk, pBlk->n + nSpace, &rc);
      blobGrowBuffer(&pNode->key, nTerm, &rc);

      if( rc==SQLITE_OK ){
        if( pNode->key.n ){
          pBlk->n += sqlite3Fts3PutVarint(&pBlk->a[pBlk->n], nPrefix);
        }
        pBlk->n += sqlite3Fts3PutVarint(&pBlk->a[pBlk->n], nSuffix);
        assert( nPrefix+nSuffix<=nTerm );
        assert( nPrefix>=0 );
        memcpy(&pBlk->a[pBlk->n], &zTerm[nPrefix], nSuffix);
        pBlk->n += nSuffix;

        memcpy(pNode->key.a, zTerm, nTerm);
        pNode->key.n = nTerm;
      }
    }else{
      /* Otherwise, flush the current node of layer iLayer to disk.
      ** Then allocate a new, empty sibling node. The key will be written
      ** into the parent of this node. */
      rc = fts3WriteSegment(p, pNode->iBlock, pNode->block.a, pNode->block.n);

      assert( pNode->block.nAlloc>=p->nNodeSize );
      pNode->block.a[0] = (char)iLayer;
      pNode->block.n = 1 + sqlite3Fts3PutVarint(&pNode->block.a[1], iPtr+1);

      iNextPtr = pNode->iBlock;
      pNode->iBlock++;
      pNode->key.n = 0;
    }

    if( rc!=SQLITE_OK || iNextPtr==0 ) return rc;
    iPtr = iNextPtr;
  }

  assert( 0 );
  return 0;
}
