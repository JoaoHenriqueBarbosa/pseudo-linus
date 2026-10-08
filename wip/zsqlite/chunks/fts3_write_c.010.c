
/*
** Append a term and (optionally) doclist to the FTS segment node currently
** stored in blob *pNode. The node need not contain any terms, but the
** header must be written before this function is called.
**
** A node header is a single 0x00 byte for a leaf node, or a height varint
** followed by the left-hand-child varint for an internal node.
**
** The term to be appended is passed via arguments zTerm/nTerm. For a
** leaf node, the doclist is passed as aDoclist/nDoclist. For an internal
** node, both aDoclist and nDoclist must be passed 0.
**
** If the size of the value in blob pPrev is zero, then this is the first
** term written to the node. Otherwise, pPrev contains a copy of the
** previous term. Before this function returns, it is updated to contain a
** copy of zTerm/nTerm.
**
** It is assumed that the buffer associated with pNode is already large
** enough to accommodate the new entry. The buffer associated with pPrev
** is extended by this function if requrired.
**
** If an error (i.e. OOM condition) occurs, an SQLite error code is
** returned. Otherwise, SQLITE_OK.
*/
static int fts3AppendToNode(
  Blob *pNode,                    /* Current node image to append to */
  Blob *pPrev,                    /* Buffer containing previous term written */
  const char *zTerm,              /* New term to write */
  int nTerm,                      /* Size of zTerm in bytes */
  const char *aDoclist,           /* Doclist (or NULL) to write */
  int nDoclist                    /* Size of aDoclist in bytes */
){
  int rc = SQLITE_OK;             /* Return code */
  int bFirst = (pPrev->n==0);     /* True if this is the first term written */
  int nPrefix;                    /* Size of term prefix in bytes */
  int nSuffix;                    /* Size of term suffix in bytes */

  /* Node must have already been started. There must be a doclist for a
  ** leaf node, and there must not be a doclist for an internal node.  */
  assert( pNode->n>0 );
  assert_fts3_nc( (pNode->a[0]=='\0')==(aDoclist!=0) );

  blobGrowBuffer(pPrev, nTerm, &rc);
  if( rc!=SQLITE_OK ) return rc;
  assert( pPrev!=0 );
  assert( pPrev->a!=0 );

  nPrefix = fts3PrefixCompress(pPrev->a, pPrev->n, zTerm, nTerm);
  nSuffix = nTerm - nPrefix;
  if( nSuffix<=0 ) return FTS_CORRUPT_VTAB;
  memcpy(pPrev->a, zTerm, nTerm);
  pPrev->n = nTerm;

  if( bFirst==0 ){
    pNode->n += sqlite3Fts3PutVarint(&pNode->a[pNode->n], nPrefix);
  }
  pNode->n += sqlite3Fts3PutVarint(&pNode->a[pNode->n], nSuffix);
  memcpy(&pNode->a[pNode->n], &zTerm[nPrefix], nSuffix);
  pNode->n += nSuffix;

  if( aDoclist ){
    pNode->n += sqlite3Fts3PutVarint(&pNode->a[pNode->n], nDoclist);
    memcpy(&pNode->a[pNode->n], aDoclist, nDoclist);
    pNode->n += nDoclist;
  }

  assert( pNode->n<=pNode->nAlloc );

  return SQLITE_OK;
}

/*
** Append the current term and doclist pointed to by cursor pCsr to the
** appendable b-tree segment opened for writing by pWriter.
**
** Return SQLITE_OK if successful, or an SQLite error code otherwise.
*/
static int fts3IncrmergeAppend(
  Fts3Table *p,                   /* Fts3 table handle */
  IncrmergeWriter *pWriter,       /* Writer object */
  Fts3MultiSegReader *pCsr        /* Cursor containing term and doclist */
){
  const char *zTerm = pCsr->zTerm;
  int nTerm = pCsr->nTerm;
  const char *aDoclist = pCsr->aDoclist;
  int nDoclist = pCsr->nDoclist;
  int rc = SQLITE_OK;           /* Return code */
  int nSpace;                   /* Total space in bytes required on leaf */
  int nPrefix;                  /* Size of prefix shared with previous term */
  int nSuffix;                  /* Size of suffix (nTerm - nPrefix) */
  NodeWriter *pLeaf;            /* Object used to write leaf nodes */

  pLeaf = &pWriter->aNodeWriter[0];
  nPrefix = fts3PrefixCompress(pLeaf->key.a, pLeaf->key.n, zTerm, nTerm);
  nSuffix = nTerm - nPrefix;
  if(nSuffix<=0 ) return FTS_CORRUPT_VTAB;

  nSpace  = sqlite3Fts3VarintLen(nPrefix);
  nSpace += sqlite3Fts3VarintLen(nSuffix) + nSuffix;
  nSpace += sqlite3Fts3VarintLen(nDoclist) + nDoclist;

  /* If the current block is not empty, and if adding this term/doclist
  ** to the current block would make it larger than Fts3Table.nNodeSize bytes,
  ** and if there is still room for another leaf page, write this block out to
  ** the database. */
  if( pLeaf->block.n>0
   && (pLeaf->block.n + nSpace)>p->nNodeSize
   && pLeaf->iBlock < (pWriter->iStart + pWriter->nLeafEst)
  ){
    rc = fts3WriteSegment(p, pLeaf->iBlock, pLeaf->block.a, pLeaf->block.n);
    pWriter->nWork++;

    /* Add the current term to the parent node. The term added to the
    ** parent must:
    **
    **   a) be greater than the largest term on the leaf node just written
    **      to the database (still available in pLeaf->key), and
    **
    **   b) be less than or equal to the term about to be added to the new
    **      leaf node (zTerm/nTerm).
    **
    ** In other words, it must be the prefix of zTerm 1 byte longer than
    ** the common prefix (if any) of zTerm and pWriter->zTerm.
    */
    if( rc==SQLITE_OK ){
      rc = fts3IncrmergePush(p, pWriter, zTerm, nPrefix+1);
    }

    /* Advance to the next output block */
    pLeaf->iBlock++;
    pLeaf->key.n = 0;
    pLeaf->block.n = 0;

    nSuffix = nTerm;
    nSpace  = 1;
    nSpace += sqlite3Fts3VarintLen(nSuffix) + nSuffix;
    nSpace += sqlite3Fts3VarintLen(nDoclist) + nDoclist;
  }

  pWriter->nLeafData += nSpace;
  blobGrowBuffer(&pLeaf->block, pLeaf->block.n + nSpace, &rc);
  if( rc==SQLITE_OK ){
    if( pLeaf->block.n==0 ){
      pLeaf->block.n = 1;
      pLeaf->block.a[0] = '\0';
    }
    rc = fts3AppendToNode(
        &pLeaf->block, &pLeaf->key, zTerm, nTerm, aDoclist, nDoclist
    );
  }

  return rc;
}

/*
** This function is called to release all dynamic resources held by the
** merge-writer object pWriter, and if no error has occurred, to flush
** all outstanding node buffers held by pWriter to disk.
**
** If *pRc is not SQLITE_OK when this function is called, then no attempt
** is made to write any data to disk. Instead, this function serves only
** to release outstanding resources.
**
** Otherwise, if *pRc is initially SQLITE_OK and an error occurs while
** flushing buffers to disk, *pRc is set to an SQLite error code before
** returning.
*/
static void fts3IncrmergeRelease(
  Fts3Table *p,                   /* FTS3 table handle */
  IncrmergeWriter *pWriter,       /* Merge-writer object */
  int *pRc                        /* IN/OUT: Error code */
){
  int i;                          /* Used to iterate through non-root layers */
  int iRoot;                      /* Index of root in pWriter->aNodeWriter */
  NodeWriter *pRoot;              /* NodeWriter for root node */
  int rc = *pRc;                  /* Error code */

  /* Set iRoot to the index in pWriter->aNodeWriter[] of the output segment
  ** root node. If the segment fits entirely on a single leaf node, iRoot
  ** will be set to 0. If the root node is the parent of the leaves, iRoot
  ** will be 1. And so on.  */
  for(iRoot=FTS_MAX_APPENDABLE_HEIGHT-1; iRoot>=0; iRoot--){
    NodeWriter *pNode = &pWriter->aNodeWriter[iRoot];
    if( pNode->block.n>0 ) break;
    assert( *pRc || pNode->block.nAlloc==0 );
    assert( *pRc || pNode->key.nAlloc==0 );
    sqlite3_free(pNode->block.a);
    sqlite3_free(pNode->key.a);
  }

  /* Empty output segment. This is a no-op. */
  if( iRoot<0 ) return;

  /* The entire output segment fits on a single node. Normally, this means
  ** the node would be stored as a blob in the "root" column of the %_segdir
  ** table. However, this is not permitted in this case. The problem is that
  ** space has already been reserved in the %_segments table, and so the
  ** start_block and end_block fields of the %_segdir table must be populated.
  ** And, by design or by accident, released versions of FTS cannot handle
  ** segments that fit entirely on the root node with start_block!=0.
  **
  ** Instead, create a synthetic root node that contains nothing but a
  ** pointer to the single content node. So that the segment consists of a
  ** single leaf and a single interior (root) node.
  **
  ** Todo: Better might be to defer allocating space in the %_segments
  ** table until we are sure it is needed.
  */
  if( iRoot==0 ){
    Blob *pBlock = &pWriter->aNodeWriter[1].block;
    blobGrowBuffer(pBlock, 1 + FTS3_VARINT_MAX, &rc);
    if( rc==SQLITE_OK ){
      pBlock->a[0] = 0x01;
      pBlock->n = 1 + sqlite3Fts3PutVarint(
          &pBlock->a[1], pWriter->aNodeWriter[0].iBlock
      );
    }
    iRoot = 1;
  }
  pRoot = &pWriter->aNodeWriter[iRoot];

  /* Flush all currently outstanding nodes to disk. */
  for(i=0; i<iRoot; i++){
    NodeWriter *pNode = &pWriter->aNodeWriter[i];
    if( pNode->block.n>0 && rc==SQLITE_OK ){
      rc = fts3WriteSegment(p, pNode->iBlock, pNode->block.a, pNode->block.n);
    }
    sqlite3_free(pNode->block.a);
    sqlite3_free(pNode->key.a);
  }

  /* Write the %_segdir record. */
  if( rc==SQLITE_OK ){
    rc = fts3WriteSegdir(p,
        pWriter->iAbsLevel+1,               /* level */
        pWriter->iIdx,                      /* idx */
        pWriter->iStart,                    /* start_block */
        pWriter->aNodeWriter[0].iBlock,     /* leaves_end_block */
        pWriter->iEnd,                      /* end_block */
        (pWriter->bNoLeafData==0 ? pWriter->nLeafData : 0),   /* end_block */
        pRoot->block.a, pRoot->block.n      /* root */
    );
  }
  sqlite3_free(pRoot->block.a);
  sqlite3_free(pRoot->key.a);

  *pRc = rc;
}

/*
** Compare the term in buffer zLhs (size in bytes nLhs) with that in
** zRhs (size in bytes nRhs) using memcmp. If one term is a prefix of
** the other, it is considered to be smaller than the other.
**
** Return -ve if zLhs is smaller than zRhs, 0 if it is equal, or +ve
** if it is greater.
*/
static int fts3TermCmp(
  const char *zLhs, int nLhs,     /* LHS of comparison */
  const char *zRhs, int nRhs      /* RHS of comparison */
){
  int nCmp = MIN(nLhs, nRhs);
  int res;

  if( nCmp && ALWAYS(zLhs) && ALWAYS(zRhs) ){
    res = memcmp(zLhs, zRhs, nCmp);
  }else{
    res = 0;
  }
  if( res==0 ) res = nLhs - nRhs;

  return res;
}


/*
** Query to see if the entry in the %_segments table with blockid iEnd is
** NULL. If no error occurs and the entry is NULL, set *pbRes 1 before
** returning. Otherwise, set *pbRes to 0.
**
** Or, if an error occurs while querying the database, return an SQLite
** error code. The final value of *pbRes is undefined in this case.
**
** This is used to test if a segment is an "appendable" segment. If it
** is, then a NULL entry has been inserted into the %_segments table
** with blockid %_segdir.end_block.
*/
static int fts3IsAppendable(Fts3Table *p, sqlite3_int64 iEnd, int *pbRes){
  int bRes = 0;                   /* Result to set *pbRes to */
  sqlite3_stmt *pCheck = 0;       /* Statement to query database with */
  int rc;                         /* Return code */

  rc = fts3SqlStmt(p, SQL_SEGMENT_IS_APPENDABLE, &pCheck, 0);
  if( rc==SQLITE_OK ){
    sqlite3_bind_int64(pCheck, 1, iEnd);
    if( SQLITE_ROW==sqlite3_step(pCheck) ) bRes = 1;
    rc = sqlite3_reset(pCheck);
  }

  *pbRes = bRes;
  return rc;
}

/*
** This function is called when initializing an incremental-merge operation.
** It checks if the existing segment with index value iIdx at absolute level
** (iAbsLevel+1) can be appended to by the incremental merge. If it can, the
** merge-writer object *pWriter is initialized to write to it.
**
** An existing segment can be appended to by an incremental merge if:
**
**   * It was initially created as an appendable segment (with all required
**     space pre-allocated), and
**
**   * The first key read from the input (arguments zKey and nKey) is
**     greater than the largest key currently stored in the potential
**     output segment.
*/
static int fts3IncrmergeLoad(
  Fts3Table *p,                   /* Fts3 table handle */
  sqlite3_int64 iAbsLevel,        /* Absolute level of input segments */
  int iIdx,                       /* Index of candidate output segment */
  const char *zKey,               /* First key to write */
  int nKey,                       /* Number of bytes in nKey */
  IncrmergeWriter *pWriter        /* Populate this object */
){
  int rc;                         /* Return code */
  sqlite3_stmt *pSelect = 0;      /* SELECT to read %_segdir entry */

  rc = fts3SqlStmt(p, SQL_SELECT_SEGDIR, &pSelect, 0);
  if( rc==SQLITE_OK ){
    sqlite3_int64 iStart = 0;     /* Value of %_segdir.start_block */
    sqlite3_int64 iLeafEnd = 0;   /* Value of %_segdir.leaves_end_block */
    sqlite3_int64 iEnd = 0;       /* Value of %_segdir.end_block */
    const char *aRoot = 0;        /* Pointer to %_segdir.root buffer */
    int nRoot = 0;                /* Size of aRoot[] in bytes */
    int rc2;                      /* Return code from sqlite3_reset() */
    int bAppendable = 0;          /* Set to true if segment is appendable */

    /* Read the %_segdir entry for index iIdx absolute level (iAbsLevel+1) */
    sqlite3_bind_int64(pSelect, 1, iAbsLevel+1);
    sqlite3_bind_int(pSelect, 2, iIdx);
    if( sqlite3_step(pSelect)==SQLITE_ROW ){
      iStart = sqlite3_column_int64(pSelect, 1);
      iLeafEnd = sqlite3_column_int64(pSelect, 2);
      fts3ReadEndBlockField(pSelect, 3, &iEnd, &pWriter->nLeafData);
      if( pWriter->nLeafData<0 ){
        pWriter->nLeafData = pWriter->nLeafData * -1;
      }
      pWriter->bNoLeafData = (pWriter->nLeafData==0);
      nRoot = sqlite3_column_bytes(pSelect, 4);
      aRoot = sqlite3_column_blob(pSelect, 4);
      if( aRoot==0 ){
        sqlite3_reset(pSelect);
        return nRoot ? SQLITE_NOMEM : FTS_CORRUPT_VTAB;
      }
    }else{
      return sqlite3_reset(pSelect);
    }

    /* Check for the zero-length marker in the %_segments table */
    rc = fts3IsAppendable(p, iEnd, &bAppendable);

    /* Check that zKey/nKey is larger than the largest key the candidate */
    if( rc==SQLITE_OK && bAppendable ){
      char *aLeaf = 0;
      int nLeaf = 0;

      rc = sqlite3Fts3ReadBlock(p, iLeafEnd, &aLeaf, &nLeaf, 0);
      if( rc==SQLITE_OK ){
        NodeReader reader;
        for(rc = nodeReaderInit(&reader, aLeaf, nLeaf);
            rc==SQLITE_OK && reader.aNode;
            rc = nodeReaderNext(&reader)
        ){
          assert( reader.aNode );
        }
        if( fts3TermCmp(zKey, nKey, reader.term.a, reader.term.n)<=0 ){
          bAppendable = 0;
        }
        nodeReaderRelease(&reader);
      }
      sqlite3_free(aLeaf);
    }

    if( rc==SQLITE_OK && bAppendable ){
      /* It is possible to append to this segment. Set up the IncrmergeWriter
      ** object to do so.  */
      int i;
      int nHeight = (int)aRoot[0];
      NodeWriter *pNode;
      if( nHeight<1 || nHeight>=FTS_MAX_APPENDABLE_HEIGHT ){
        sqlite3_reset(pSelect);
        return FTS_CORRUPT_VTAB;
      }

      pWriter->nLeafEst = (int)((iEnd - iStart) + 1)/FTS_MAX_APPENDABLE_HEIGHT;
      pWriter->iStart = iStart;
      pWriter->iEnd = iEnd;
      pWriter->iAbsLevel = iAbsLevel;
      pWriter->iIdx = iIdx;

      for(i=nHeight+1; i<FTS_MAX_APPENDABLE_HEIGHT; i++){
        pWriter->aNodeWriter[i].iBlock = pWriter->iStart + i*pWriter->nLeafEst;
      }

      pNode = &pWriter->aNodeWriter[nHeight];
      pNode->iBlock = pWriter->iStart + pWriter->nLeafEst*nHeight;
      blobGrowBuffer(&pNode->block,
          MAX(nRoot, p->nNodeSize)+FTS3_NODE_PADDING, &rc
      );
      if( rc==SQLITE_OK ){
        memcpy(pNode->block.a, aRoot, nRoot);
        pNode->block.n = nRoot;
        memset(&pNode->block.a[nRoot], 0, FTS3_NODE_PADDING);
      }

      for(i=nHeight; i>=0 && rc==SQLITE_OK; i--){
        NodeReader reader;
        memset(&reader, 0, sizeof(reader));
        pNode = &pWriter->aNodeWriter[i];

        if( pNode->block.a){
          rc = nodeReaderInit(&reader, pNode->block.a, pNode->block.n);
          while( reader.aNode && rc==SQLITE_OK ) rc = nodeReaderNext(&reader);
          blobGrowBuffer(&pNode->key, reader.term.n, &rc);
          if( rc==SQLITE_OK ){
            assert_fts3_nc( reader.term.n>0 || reader.aNode==0 );
            if( reader.term.n>0 ){
              memcpy(pNode->key.a, reader.term.a, reader.term.n);
            }
            pNode->key.n = reader.term.n;
            if( i>0 ){
              char *aBlock = 0;
              int nBlock = 0;
              pNode = &pWriter->aNodeWriter[i-1];
              pNode->iBlock = reader.iChild;
              rc = sqlite3Fts3ReadBlock(p, reader.iChild, &aBlock, &nBlock,0);
              blobGrowBuffer(&pNode->block,
                  MAX(nBlock, p->nNodeSize)+FTS3_NODE_PADDING, &rc
              );
              if( rc==SQLITE_OK ){
                memcpy(pNode->block.a, aBlock, nBlock);
                pNode->block.n = nBlock;
                memset(&pNode->block.a[nBlock], 0, FTS3_NODE_PADDING);
              }
              sqlite3_free(aBlock);
            }
          }
        }
        nodeReaderRelease(&reader);
      }
    }

    rc2 = sqlite3_reset(pSelect);
    if( rc==SQLITE_OK ) rc = rc2;
  }

  return rc;
}
