
/*
** Determine the largest segment index value that exists within absolute
** level iAbsLevel+1. If no error occurs, set *piIdx to this value plus
** one before returning SQLITE_OK. Or, if there are no segments at all
** within level iAbsLevel, set *piIdx to zero.
**
** If an error occurs, return an SQLite error code. The final value of
** *piIdx is undefined in this case.
*/
static int fts3IncrmergeOutputIdx(
  Fts3Table *p,                   /* FTS Table handle */
  sqlite3_int64 iAbsLevel,        /* Absolute index of input segments */
  int *piIdx                      /* OUT: Next free index at iAbsLevel+1 */
){
  int rc;
  sqlite3_stmt *pOutputIdx = 0;   /* SQL used to find output index */

  rc = fts3SqlStmt(p, SQL_NEXT_SEGMENT_INDEX, &pOutputIdx, 0);
  if( rc==SQLITE_OK ){
    sqlite3_bind_int64(pOutputIdx, 1, iAbsLevel+1);
    sqlite3_step(pOutputIdx);
    *piIdx = sqlite3_column_int(pOutputIdx, 0);
    rc = sqlite3_reset(pOutputIdx);
  }

  return rc;
}

/*
** Allocate an appendable output segment on absolute level iAbsLevel+1
** with idx value iIdx.
**
** In the %_segdir table, a segment is defined by the values in three
** columns:
**
**     start_block
**     leaves_end_block
**     end_block
**
** When an appendable segment is allocated, it is estimated that the
** maximum number of leaf blocks that may be required is the sum of the
** number of leaf blocks consumed by the input segments, plus the number
** of input segments, multiplied by two. This value is stored in stack
** variable nLeafEst.
**
** A total of 16*nLeafEst blocks are allocated when an appendable segment
** is created ((1 + end_block - start_block)==16*nLeafEst). The contiguous
** array of leaf nodes starts at the first block allocated. The array
** of interior nodes that are parents of the leaf nodes start at block
** (start_block + (1 + end_block - start_block) / 16). And so on.
**
** In the actual code below, the value "16" is replaced with the
** pre-processor macro FTS_MAX_APPENDABLE_HEIGHT.
*/
static int fts3IncrmergeWriter(
  Fts3Table *p,                   /* Fts3 table handle */
  sqlite3_int64 iAbsLevel,        /* Absolute level of input segments */
  int iIdx,                       /* Index of new output segment */
  Fts3MultiSegReader *pCsr,       /* Cursor that data will be read from */
  IncrmergeWriter *pWriter        /* Populate this object */
){
  int rc;                         /* Return Code */
  int i;                          /* Iterator variable */
  int nLeafEst = 0;               /* Blocks allocated for leaf nodes */
  sqlite3_stmt *pLeafEst = 0;     /* SQL used to determine nLeafEst */
  sqlite3_stmt *pFirstBlock = 0;  /* SQL used to determine first block */

  /* Calculate nLeafEst. */
  rc = fts3SqlStmt(p, SQL_MAX_LEAF_NODE_ESTIMATE, &pLeafEst, 0);
  if( rc==SQLITE_OK ){
    sqlite3_bind_int64(pLeafEst, 1, iAbsLevel);
    sqlite3_bind_int64(pLeafEst, 2, pCsr->nSegment);
    if( SQLITE_ROW==sqlite3_step(pLeafEst) ){
      nLeafEst = sqlite3_column_int(pLeafEst, 0);
    }
    rc = sqlite3_reset(pLeafEst);
  }
  if( rc!=SQLITE_OK ) return rc;

  /* Calculate the first block to use in the output segment */
  rc = fts3SqlStmt(p, SQL_NEXT_SEGMENTS_ID, &pFirstBlock, 0);
  if( rc==SQLITE_OK ){
    if( SQLITE_ROW==sqlite3_step(pFirstBlock) ){
      pWriter->iStart = sqlite3_column_int64(pFirstBlock, 0);
      pWriter->iEnd = pWriter->iStart - 1;
      pWriter->iEnd += nLeafEst * FTS_MAX_APPENDABLE_HEIGHT;
    }
    rc = sqlite3_reset(pFirstBlock);
  }
  if( rc!=SQLITE_OK ) return rc;

  /* Insert the marker in the %_segments table to make sure nobody tries
  ** to steal the space just allocated. This is also used to identify
  ** appendable segments.  */
  rc = fts3WriteSegment(p, pWriter->iEnd, 0, 0);
  if( rc!=SQLITE_OK ) return rc;

  pWriter->iAbsLevel = iAbsLevel;
  pWriter->nLeafEst = nLeafEst;
  pWriter->iIdx = iIdx;

  /* Set up the array of NodeWriter objects */
  for(i=0; i<FTS_MAX_APPENDABLE_HEIGHT; i++){
    pWriter->aNodeWriter[i].iBlock = pWriter->iStart + i*pWriter->nLeafEst;
  }
  return SQLITE_OK;
}

/*
** Remove an entry from the %_segdir table. This involves running the
** following two statements:
**
**   DELETE FROM %_segdir WHERE level = :iAbsLevel AND idx = :iIdx
**   UPDATE %_segdir SET idx = idx - 1 WHERE level = :iAbsLevel AND idx > :iIdx
**
** The DELETE statement removes the specific %_segdir level. The UPDATE
** statement ensures that the remaining segments have contiguously allocated
** idx values.
*/
static int fts3RemoveSegdirEntry(
  Fts3Table *p,                   /* FTS3 table handle */
  sqlite3_int64 iAbsLevel,        /* Absolute level to delete from */
  int iIdx                        /* Index of %_segdir entry to delete */
){
  int rc;                         /* Return code */
  sqlite3_stmt *pDelete = 0;      /* DELETE statement */

  rc = fts3SqlStmt(p, SQL_DELETE_SEGDIR_ENTRY, &pDelete, 0);
  if( rc==SQLITE_OK ){
    sqlite3_bind_int64(pDelete, 1, iAbsLevel);
    sqlite3_bind_int(pDelete, 2, iIdx);
    sqlite3_step(pDelete);
    rc = sqlite3_reset(pDelete);
  }

  return rc;
}

/*
** One or more segments have just been removed from absolute level iAbsLevel.
** Update the 'idx' values of the remaining segments in the level so that
** the idx values are a contiguous sequence starting from 0.
*/
static int fts3RepackSegdirLevel(
  Fts3Table *p,                   /* FTS3 table handle */
  sqlite3_int64 iAbsLevel         /* Absolute level to repack */
){
  int rc;                         /* Return code */
  int *aIdx = 0;                  /* Array of remaining idx values */
  int nIdx = 0;                   /* Valid entries in aIdx[] */
  int nAlloc = 0;                 /* Allocated size of aIdx[] */
  int i;                          /* Iterator variable */
  sqlite3_stmt *pSelect = 0;      /* Select statement to read idx values */
  sqlite3_stmt *pUpdate = 0;      /* Update statement to modify idx values */

  rc = fts3SqlStmt(p, SQL_SELECT_INDEXES, &pSelect, 0);
  if( rc==SQLITE_OK ){
    int rc2;
    sqlite3_bind_int64(pSelect, 1, iAbsLevel);
    while( SQLITE_ROW==sqlite3_step(pSelect) ){
      if( nIdx>=nAlloc ){
        int *aNew;
        nAlloc += 16;
        aNew = sqlite3_realloc64(aIdx, nAlloc*sizeof(int));
        if( !aNew ){
          rc = SQLITE_NOMEM;
          break;
        }
        aIdx = aNew;
      }
      aIdx[nIdx++] = sqlite3_column_int(pSelect, 0);
    }
    rc2 = sqlite3_reset(pSelect);
    if( rc==SQLITE_OK ) rc = rc2;
  }

  if( rc==SQLITE_OK ){
    rc = fts3SqlStmt(p, SQL_SHIFT_SEGDIR_ENTRY, &pUpdate, 0);
  }
  if( rc==SQLITE_OK ){
    sqlite3_bind_int64(pUpdate, 2, iAbsLevel);
  }

  assert( p->bIgnoreSavepoint==0 );
  p->bIgnoreSavepoint = 1;
  for(i=0; rc==SQLITE_OK && i<nIdx; i++){
    if( aIdx[i]!=i ){
      sqlite3_bind_int(pUpdate, 3, aIdx[i]);
      sqlite3_bind_int(pUpdate, 1, i);
      sqlite3_step(pUpdate);
      rc = sqlite3_reset(pUpdate);
    }
  }
  p->bIgnoreSavepoint = 0;

  sqlite3_free(aIdx);
  return rc;
}

static void fts3StartNode(Blob *pNode, int iHeight, sqlite3_int64 iChild){
  pNode->a[0] = (char)iHeight;
  if( iChild ){
    assert( pNode->nAlloc>=1+sqlite3Fts3VarintLen(iChild) );
    pNode->n = 1 + sqlite3Fts3PutVarint(&pNode->a[1], iChild);
  }else{
    assert( pNode->nAlloc>=1 );
    pNode->n = 1;
  }
}

/*
** The first two arguments are a pointer to and the size of a segment b-tree
** node. The node may be a leaf or an internal node.
**
** This function creates a new node image in blob object *pNew by copying
** all terms that are greater than or equal to zTerm/nTerm (for leaf nodes)
** or greater than zTerm/nTerm (for internal nodes) from aNode/nNode.
*/
static int fts3TruncateNode(
  const char *aNode,              /* Current node image */
  int nNode,                      /* Size of aNode in bytes */
  Blob *pNew,                     /* OUT: Write new node image here */
  const char *zTerm,              /* Omit all terms smaller than this */
  int nTerm,                      /* Size of zTerm in bytes */
  sqlite3_int64 *piBlock          /* OUT: Block number in next layer down */
){
  NodeReader reader;              /* Reader object */
  Blob prev = {0, 0, 0};          /* Previous term written to new node */
  int rc = SQLITE_OK;             /* Return code */
  int bLeaf;                       /* True for a leaf node */

  if( nNode<1 ) return FTS_CORRUPT_VTAB;
  bLeaf = aNode[0]=='\0';

  /* Allocate required output space */
  blobGrowBuffer(pNew, nNode, &rc);
  if( rc!=SQLITE_OK ) return rc;
  pNew->n = 0;

  /* Populate new node buffer */
  for(rc = nodeReaderInit(&reader, aNode, nNode);
      rc==SQLITE_OK && reader.aNode;
      rc = nodeReaderNext(&reader)
  ){
    if( pNew->n==0 ){
      int res = fts3TermCmp(reader.term.a, reader.term.n, zTerm, nTerm);
      if( res<0 || (bLeaf==0 && res==0) ) continue;
      fts3StartNode(pNew, (int)aNode[0], reader.iChild);
      *piBlock = reader.iChild;
    }
    rc = fts3AppendToNode(
        pNew, &prev, reader.term.a, reader.term.n,
        reader.aDoclist, reader.nDoclist
    );
    if( rc!=SQLITE_OK ) break;
  }
  if( pNew->n==0 ){
    fts3StartNode(pNew, (int)aNode[0], reader.iChild);
    *piBlock = reader.iChild;
  }
  assert( pNew->n<=pNew->nAlloc );

  nodeReaderRelease(&reader);
  sqlite3_free(prev.a);
  return rc;
}

/*
** Remove all terms smaller than zTerm/nTerm from segment iIdx in absolute
** level iAbsLevel. This may involve deleting entries from the %_segments
** table, and modifying existing entries in both the %_segments and %_segdir
** tables.
**
** SQLITE_OK is returned if the segment is updated successfully. Or an
** SQLite error code otherwise.
*/
static int fts3TruncateSegment(
  Fts3Table *p,                   /* FTS3 table handle */
  sqlite3_int64 iAbsLevel,        /* Absolute level of segment to modify */
  int iIdx,                       /* Index within level of segment to modify */
  const char *zTerm,              /* Remove terms smaller than this */
  int nTerm                      /* Number of bytes in buffer zTerm */
){
  int rc = SQLITE_OK;             /* Return code */
  Blob root = {0,0,0};            /* New root page image */
  Blob block = {0,0,0};           /* Buffer used for any other block */
  sqlite3_int64 iBlock = 0;       /* Block id */
  sqlite3_int64 iNewStart = 0;    /* New value for iStartBlock */
  sqlite3_int64 iOldStart = 0;    /* Old value for iStartBlock */
  sqlite3_stmt *pFetch = 0;       /* Statement used to fetch segdir */

  rc = fts3SqlStmt(p, SQL_SELECT_SEGDIR, &pFetch, 0);
  if( rc==SQLITE_OK ){
    int rc2;                      /* sqlite3_reset() return code */
    sqlite3_bind_int64(pFetch, 1, iAbsLevel);
    sqlite3_bind_int(pFetch, 2, iIdx);
    if( SQLITE_ROW==sqlite3_step(pFetch) ){
      const char *aRoot = sqlite3_column_blob(pFetch, 4);
      int nRoot = sqlite3_column_bytes(pFetch, 4);
      iOldStart = sqlite3_column_int64(pFetch, 1);
      rc = fts3TruncateNode(aRoot, nRoot, &root, zTerm, nTerm, &iBlock);
    }
    rc2 = sqlite3_reset(pFetch);
    if( rc==SQLITE_OK ) rc = rc2;
  }

  while( rc==SQLITE_OK && iBlock ){
    char *aBlock = 0;
    int nBlock = 0;
    iNewStart = iBlock;

    rc = sqlite3Fts3ReadBlock(p, iBlock, &aBlock, &nBlock, 0);
    if( rc==SQLITE_OK ){
      rc = fts3TruncateNode(aBlock, nBlock, &block, zTerm, nTerm, &iBlock);
    }
    if( rc==SQLITE_OK ){
      rc = fts3WriteSegment(p, iNewStart, block.a, block.n);
    }
    sqlite3_free(aBlock);
  }

  /* Variable iNewStart now contains the first valid leaf node. */
  if( rc==SQLITE_OK && iNewStart ){
    sqlite3_stmt *pDel = 0;
    rc = fts3SqlStmt(p, SQL_DELETE_SEGMENTS_RANGE, &pDel, 0);
    if( rc==SQLITE_OK ){
      sqlite3_bind_int64(pDel, 1, iOldStart);
      sqlite3_bind_int64(pDel, 2, iNewStart-1);
      sqlite3_step(pDel);
      rc = sqlite3_reset(pDel);
    }
  }

  if( rc==SQLITE_OK ){
    sqlite3_stmt *pChomp = 0;
    rc = fts3SqlStmt(p, SQL_CHOMP_SEGDIR, &pChomp, 0);
    if( rc==SQLITE_OK ){
      sqlite3_bind_int64(pChomp, 1, iNewStart);
      sqlite3_bind_blob(pChomp, 2, root.a, root.n, SQLITE_STATIC);
      sqlite3_bind_int64(pChomp, 3, iAbsLevel);
      sqlite3_bind_int(pChomp, 4, iIdx);
      sqlite3_step(pChomp);
      rc = sqlite3_reset(pChomp);
      sqlite3_bind_null(pChomp, 2);
    }
  }

  sqlite3_free(root.a);
  sqlite3_free(block.a);
  return rc;
}
