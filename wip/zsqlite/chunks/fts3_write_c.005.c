
/*
** Insert a record into the %_segdir table.
*/
static int fts3WriteSegdir(
  Fts3Table *p,                   /* Virtual table handle */
  sqlite3_int64 iLevel,           /* Value for "level" field (absolute level) */
  int iIdx,                       /* Value for "idx" field */
  sqlite3_int64 iStartBlock,      /* Value for "start_block" field */
  sqlite3_int64 iLeafEndBlock,    /* Value for "leaves_end_block" field */
  sqlite3_int64 iEndBlock,        /* Value for "end_block" field */
  sqlite3_int64 nLeafData,        /* Bytes of leaf data in segment */
  char *zRoot,                    /* Blob value for "root" field */
  int nRoot                       /* Number of bytes in buffer zRoot */
){
  sqlite3_stmt *pStmt;
  int rc = fts3SqlStmt(p, SQL_INSERT_SEGDIR, &pStmt, 0);
  if( rc==SQLITE_OK ){
    sqlite3_bind_int64(pStmt, 1, iLevel);
    sqlite3_bind_int(pStmt, 2, iIdx);
    sqlite3_bind_int64(pStmt, 3, iStartBlock);
    sqlite3_bind_int64(pStmt, 4, iLeafEndBlock);
    if( nLeafData==0 ){
      sqlite3_bind_int64(pStmt, 5, iEndBlock);
    }else{
      char *zEnd = sqlite3_mprintf("%lld %lld", iEndBlock, nLeafData);
      if( !zEnd ) return SQLITE_NOMEM;
      sqlite3_bind_text(pStmt, 5, zEnd, -1, sqlite3_free);
    }
    sqlite3_bind_blob(pStmt, 6, zRoot, nRoot, SQLITE_STATIC);
    sqlite3_step(pStmt);
    rc = sqlite3_reset(pStmt);
    sqlite3_bind_null(pStmt, 6);
  }
  return rc;
}

/*
** Return the size of the common prefix (if any) shared by zPrev and
** zNext, in bytes. For example,
**
**   fts3PrefixCompress("abc", 3, "abcdef", 6)   // returns 3
**   fts3PrefixCompress("abX", 3, "abcdef", 6)   // returns 2
**   fts3PrefixCompress("abX", 3, "Xbcdef", 6)   // returns 0
*/
static int fts3PrefixCompress(
  const char *zPrev,              /* Buffer containing previous term */
  int nPrev,                      /* Size of buffer zPrev in bytes */
  const char *zNext,              /* Buffer containing next term */
  int nNext                       /* Size of buffer zNext in bytes */
){
  int n;
  for(n=0; n<nPrev && n<nNext && zPrev[n]==zNext[n]; n++);
  assert_fts3_nc( n<nNext );
  return n;
}

/*
** Add term zTerm to the SegmentNode. It is guaranteed that zTerm is larger
** (according to memcmp) than the previous term.
*/
static int fts3NodeAddTerm(
  Fts3Table *p,                   /* Virtual table handle */
  SegmentNode **ppTree,           /* IN/OUT: SegmentNode handle */
  int isCopyTerm,                 /* True if zTerm/nTerm is transient */
  const char *zTerm,              /* Pointer to buffer containing term */
  int nTerm                       /* Size of term in bytes */
){
  SegmentNode *pTree = *ppTree;
  int rc;
  SegmentNode *pNew;

  /* First try to append the term to the current node. Return early if
  ** this is possible.
  */
  if( pTree ){
    int nData = pTree->nData;     /* Current size of node in bytes */
    int nReq = nData;             /* Required space after adding zTerm */
    int nPrefix;                  /* Number of bytes of prefix compression */
    int nSuffix;                  /* Suffix length */

    nPrefix = fts3PrefixCompress(pTree->zTerm, pTree->nTerm, zTerm, nTerm);
    nSuffix = nTerm-nPrefix;

    /* If nSuffix is zero or less, then zTerm/nTerm must be a prefix of
    ** pWriter->zTerm/pWriter->nTerm. i.e. must be equal to or less than when
    ** compared with BINARY collation. This indicates corruption.  */
    if( nSuffix<=0 ) return FTS_CORRUPT_VTAB;

    nReq += sqlite3Fts3VarintLen(nPrefix)+sqlite3Fts3VarintLen(nSuffix)+nSuffix;
    if( nReq<=p->nNodeSize || !pTree->zTerm ){

      if( nReq>p->nNodeSize ){
        /* An unusual case: this is the first term to be added to the node
        ** and the static node buffer (p->nNodeSize bytes) is not large
        ** enough. Use a separately malloced buffer instead This wastes
        ** p->nNodeSize bytes, but since this scenario only comes about when
        ** the database contain two terms that share a prefix of almost 2KB,
        ** this is not expected to be a serious problem.
        */
        assert( pTree->aData==(char *)&pTree[1] );
        pTree->aData = (char *)sqlite3_malloc64(nReq);
        if( !pTree->aData ){
          return SQLITE_NOMEM;
        }
      }

      if( pTree->zTerm ){
        /* There is no prefix-length field for first term in a node */
        nData += sqlite3Fts3PutVarint(&pTree->aData[nData], nPrefix);
      }

      nData += sqlite3Fts3PutVarint(&pTree->aData[nData], nSuffix);
      memcpy(&pTree->aData[nData], &zTerm[nPrefix], nSuffix);
      pTree->nData = nData + nSuffix;
      pTree->nEntry++;

      if( isCopyTerm ){
        if( pTree->nMalloc<nTerm ){
          char *zNew = sqlite3_realloc64(pTree->zMalloc, (i64)nTerm*2);
          if( !zNew ){
            return SQLITE_NOMEM;
          }
          pTree->nMalloc = nTerm*2;
          pTree->zMalloc = zNew;
        }
        pTree->zTerm = pTree->zMalloc;
        memcpy(pTree->zTerm, zTerm, nTerm);
        pTree->nTerm = nTerm;
      }else{
        pTree->zTerm = (char *)zTerm;
        pTree->nTerm = nTerm;
      }
      return SQLITE_OK;
    }
  }

  /* If control flows to here, it was not possible to append zTerm to the
  ** current node. Create a new node (a right-sibling of the current node).
  ** If this is the first node in the tree, the term is added to it.
  **
  ** Otherwise, the term is not added to the new node, it is left empty for
  ** now. Instead, the term is inserted into the parent of pTree. If pTree
  ** has no parent, one is created here.
  */
  pNew = (SegmentNode *)sqlite3_malloc64(sizeof(SegmentNode) + p->nNodeSize);
  if( !pNew ){
    return SQLITE_NOMEM;
  }
  memset(pNew, 0, sizeof(SegmentNode));
  pNew->nData = 1 + FTS3_VARINT_MAX;
  pNew->aData = (char *)&pNew[1];

  if( pTree ){
    SegmentNode *pParent = pTree->pParent;
    rc = fts3NodeAddTerm(p, &pParent, isCopyTerm, zTerm, nTerm);
    if( pTree->pParent==0 ){
      pTree->pParent = pParent;
    }
    pTree->pRight = pNew;
    pNew->pLeftmost = pTree->pLeftmost;
    pNew->pParent = pParent;
    pNew->zMalloc = pTree->zMalloc;
    pNew->nMalloc = pTree->nMalloc;
    pTree->zMalloc = 0;
  }else{
    pNew->pLeftmost = pNew;
    rc = fts3NodeAddTerm(p, &pNew, isCopyTerm, zTerm, nTerm);
  }

  *ppTree = pNew;
  return rc;
}

/*
** Helper function for fts3NodeWrite().
*/
static int fts3TreeFinishNode(
  SegmentNode *pTree,
  int iHeight,
  sqlite3_int64 iLeftChild
){
  int nStart;
  assert( iHeight>=1 && iHeight<128 );
  nStart = FTS3_VARINT_MAX - sqlite3Fts3VarintLen(iLeftChild);
  pTree->aData[nStart] = (char)iHeight;
  sqlite3Fts3PutVarint(&pTree->aData[nStart+1], iLeftChild);
  return nStart;
}

/*
** Write the buffer for the segment node pTree and all of its peers to the
** database. Then call this function recursively to write the parent of
** pTree and its peers to the database.
**
** Except, if pTree is a root node, do not write it to the database. Instead,
** set output variables *paRoot and *pnRoot to contain the root node.
**
** If successful, SQLITE_OK is returned and output variable *piLast is
** set to the largest blockid written to the database (or zero if no
** blocks were written to the db). Otherwise, an SQLite error code is
** returned.
*/
static int fts3NodeWrite(
  Fts3Table *p,                   /* Virtual table handle */
  SegmentNode *pTree,             /* SegmentNode handle */
  int iHeight,                    /* Height of this node in tree */
  sqlite3_int64 iLeaf,            /* Block id of first leaf node */
  sqlite3_int64 iFree,            /* Block id of next free slot in %_segments */
  sqlite3_int64 *piLast,          /* OUT: Block id of last entry written */
  char **paRoot,                  /* OUT: Data for root node */
  int *pnRoot                     /* OUT: Size of root node in bytes */
){
  int rc = SQLITE_OK;

  if( !pTree->pParent ){
    /* Root node of the tree. */
    int nStart = fts3TreeFinishNode(pTree, iHeight, iLeaf);
    *piLast = iFree-1;
    *pnRoot = pTree->nData - nStart;
    *paRoot = &pTree->aData[nStart];
  }else{
    SegmentNode *pIter;
    sqlite3_int64 iNextFree = iFree;
    sqlite3_int64 iNextLeaf = iLeaf;
    for(pIter=pTree->pLeftmost; pIter && rc==SQLITE_OK; pIter=pIter->pRight){
      int nStart = fts3TreeFinishNode(pIter, iHeight, iNextLeaf);
      int nWrite = pIter->nData - nStart;

      rc = fts3WriteSegment(p, iNextFree, &pIter->aData[nStart], nWrite);
      iNextFree++;
      iNextLeaf += (pIter->nEntry+1);
    }
    if( rc==SQLITE_OK ){
      assert( iNextLeaf==iFree );
      rc = fts3NodeWrite(
          p, pTree->pParent, iHeight+1, iFree, iNextFree, piLast, paRoot, pnRoot
      );
    }
  }

  return rc;
}

/*
** Free all memory allocations associated with the tree pTree.
*/
static void fts3NodeFree(SegmentNode *pTree){
  if( pTree ){
    SegmentNode *p = pTree->pLeftmost;
    fts3NodeFree(p->pParent);
    while( p ){
      SegmentNode *pRight = p->pRight;
      if( p->aData!=(char *)&p[1] ){
        sqlite3_free(p->aData);
      }
      assert( pRight==0 || p->zMalloc==0 );
      sqlite3_free(p->zMalloc);
      sqlite3_free(p);
      p = pRight;
    }
  }
}

/*
** Add a term to the segment being constructed by the SegmentWriter object
** *ppWriter. When adding the first term to a segment, *ppWriter should
** be passed NULL. This function will allocate a new SegmentWriter object
** and return it via the input/output variable *ppWriter in this case.
**
** If successful, SQLITE_OK is returned. Otherwise, an SQLite error code.
*/
static int fts3SegWriterAdd(
  Fts3Table *p,                   /* Virtual table handle */
  SegmentWriter **ppWriter,       /* IN/OUT: SegmentWriter handle */
  int isCopyTerm,                 /* True if buffer zTerm must be copied */
  const char *zTerm,              /* Pointer to buffer containing term */
  int nTerm,                      /* Size of term in bytes */
  const char *aDoclist,           /* Pointer to buffer containing doclist */
  int nDoclist                    /* Size of doclist in bytes */
){
  int nPrefix;                    /* Size of term prefix in bytes */
  int nSuffix;                    /* Size of term suffix in bytes */
  i64 nReq;                       /* Number of bytes required on leaf page */
  int nData;
  SegmentWriter *pWriter = *ppWriter;

  if( !pWriter ){
    int rc;
    sqlite3_stmt *pStmt;

    /* Allocate the SegmentWriter structure */
    pWriter = (SegmentWriter *)sqlite3_malloc64(sizeof(SegmentWriter));
    if( !pWriter ) return SQLITE_NOMEM;
    memset(pWriter, 0, sizeof(SegmentWriter));
    *ppWriter = pWriter;

    /* Allocate a buffer in which to accumulate data */
    pWriter->aData = (char *)sqlite3_malloc64(p->nNodeSize);
    if( !pWriter->aData ) return SQLITE_NOMEM;
    pWriter->nSize = p->nNodeSize;

    /* Find the next free blockid in the %_segments table */
    rc = fts3SqlStmt(p, SQL_NEXT_SEGMENTS_ID, &pStmt, 0);
    if( rc!=SQLITE_OK ) return rc;
    if( SQLITE_ROW==sqlite3_step(pStmt) ){
      pWriter->iFree = sqlite3_column_int64(pStmt, 0);
      pWriter->iFirst = pWriter->iFree;
    }
    rc = sqlite3_reset(pStmt);
    if( rc!=SQLITE_OK ) return rc;
  }
  nData = pWriter->nData;

  nPrefix = fts3PrefixCompress(pWriter->zTerm, pWriter->nTerm, zTerm, nTerm);
  nSuffix = nTerm-nPrefix;

  /* If nSuffix is zero or less, then zTerm/nTerm must be a prefix of
  ** pWriter->zTerm/pWriter->nTerm. i.e. must be equal to or less than when
  ** compared with BINARY collation. This indicates corruption.  */
  if( nSuffix<=0 ) return FTS_CORRUPT_VTAB;

  /* Figure out how many bytes are required by this new entry */
  nReq = sqlite3Fts3VarintLen(nPrefix) +    /* varint containing prefix size */
    sqlite3Fts3VarintLen(nSuffix) +         /* varint containing suffix size */
    nSuffix +                               /* Term suffix */
    sqlite3Fts3VarintLen(nDoclist) +        /* Size of doclist */
    nDoclist;                               /* Doclist data */

  if( nData>0 && nData+nReq>p->nNodeSize ){
    int rc;

    /* The current leaf node is full. Write it out to the database. */
    if( pWriter->iFree==LARGEST_INT64 ) return FTS_CORRUPT_VTAB;
    rc = fts3WriteSegment(p, pWriter->iFree++, pWriter->aData, nData);
    if( rc!=SQLITE_OK ) return rc;
    p->nLeafAdd++;

    /* Add the current term to the interior node tree. The term added to
    ** the interior tree must:
    **
    **   a) be greater than the largest term on the leaf node just written
    **      to the database (still available in pWriter->zTerm), and
    **
    **   b) be less than or equal to the term about to be added to the new
    **      leaf node (zTerm/nTerm).
    **
    ** In other words, it must be the prefix of zTerm 1 byte longer than
    ** the common prefix (if any) of zTerm and pWriter->zTerm.
    */
    assert( nPrefix<nTerm );
    rc = fts3NodeAddTerm(p, &pWriter->pTree, isCopyTerm, zTerm, nPrefix+1);
    if( rc!=SQLITE_OK ) return rc;

    nData = 0;
    pWriter->nTerm = 0;

    nPrefix = 0;
    nSuffix = nTerm;
    nReq = 1 +                              /* varint containing prefix size */
      sqlite3Fts3VarintLen(nTerm) +         /* varint containing suffix size */
      nTerm +                               /* Term suffix */
      sqlite3Fts3VarintLen(nDoclist) +      /* Size of doclist */
      nDoclist;                             /* Doclist data */
  }

  /* Increase the total number of bytes written to account for the new entry. */
  pWriter->nLeafData += nReq;

  /* If the buffer currently allocated is too small for this entry, realloc
  ** the buffer to make it large enough.
  */
  if( nReq>pWriter->nSize ){
    char *aNew = sqlite3_realloc64(pWriter->aData, nReq);
    if( !aNew ) return SQLITE_NOMEM;
    pWriter->aData = aNew;
    pWriter->nSize = nReq;
  }
  assert( nData+nReq<=pWriter->nSize );

  /* Append the prefix-compressed term and doclist to the buffer. */
  nData += sqlite3Fts3PutVarint(&pWriter->aData[nData], nPrefix);
  nData += sqlite3Fts3PutVarint(&pWriter->aData[nData], nSuffix);
  assert( nSuffix>0 );
  memcpy(&pWriter->aData[nData], &zTerm[nPrefix], nSuffix);
  nData += nSuffix;
  nData += sqlite3Fts3PutVarint(&pWriter->aData[nData], nDoclist);
  assert( nDoclist>0 );
  memcpy(&pWriter->aData[nData], aDoclist, nDoclist);
  pWriter->nData = nData + nDoclist;

  /* Save the current term so that it can be used to prefix-compress the next.
  ** If the isCopyTerm parameter is true, then the buffer pointed to by
  ** zTerm is transient, so take a copy of the term data. Otherwise, just
  ** store a copy of the pointer.
  */
  if( isCopyTerm ){
    if( nTerm>pWriter->nMalloc ){
      char *zNew = sqlite3_realloc64(pWriter->zMalloc, (i64)nTerm*2);
      if( !zNew ){
        return SQLITE_NOMEM;
      }
      pWriter->nMalloc = nTerm*2;
      pWriter->zMalloc = zNew;
      pWriter->zTerm = zNew;
    }
    assert( pWriter->zTerm==pWriter->zMalloc );
    assert( nTerm>0 );
    memcpy(pWriter->zTerm, zTerm, nTerm);
  }else{
    pWriter->zTerm = (char *)zTerm;
  }
  pWriter->nTerm = nTerm;

  return SQLITE_OK;
}
