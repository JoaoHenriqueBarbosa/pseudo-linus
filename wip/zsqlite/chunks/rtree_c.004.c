
#if 0
/* Tracing routines for the RtreeSearchPoint queue */
static void tracePoint(RtreeSearchPoint *p, int idx, RtreeCursor *pCur){
  if( idx<0 ){ printf(" s"); }else{ printf("%2d", idx); }
  printf(" %d.%05lld.%02d %g %d",
    p->iLevel, p->id, p->iCell, p->rScore, p->eWithin
  );
  idx++;
  if( idx<RTREE_CACHE_SZ ){
    printf(" %p\n", pCur->aNode[idx]);
  }else{
    printf("\n");
  }
}
static void traceQueue(RtreeCursor *pCur, const char *zPrefix){
  int ii;
  printf("=== %9s ", zPrefix);
  if( pCur->bPoint ){
    tracePoint(&pCur->sPoint, -1, pCur);
  }
  for(ii=0; ii<pCur->nPoint; ii++){
    if( ii>0 || pCur->bPoint ) printf("              ");
    tracePoint(&pCur->aPoint[ii], ii, pCur);
  }
}
# define RTREE_QUEUE_TRACE(A,B) traceQueue(A,B)
#else
# define RTREE_QUEUE_TRACE(A,B)   /* no-op */
#endif

/* Remove the search point with the lowest current score.
*/
static void rtreeSearchPointPop(RtreeCursor *p){
  int i, j, k, n;
  i = 1 - p->bPoint;
  assert( i==0 || i==1 );
  if( p->aNode[i] ){
    nodeRelease(RTREE_OF_CURSOR(p), p->aNode[i]);
    p->aNode[i] = 0;
  }
  if( p->bPoint ){
    p->anQueue[p->sPoint.iLevel]--;
    p->bPoint = 0;
  }else if( ALWAYS(p->nPoint) ){
    p->anQueue[p->aPoint[0].iLevel]--;
    n = --p->nPoint;
    p->aPoint[0] = p->aPoint[n];
    if( n<RTREE_CACHE_SZ-1 ){
      p->aNode[1] = p->aNode[n+1];
      p->aNode[n+1] = 0;
    }
    i = 0;
    while( (j = i*2+1)<n ){
      k = j+1;
      if( k<n && rtreeSearchPointCompare(&p->aPoint[k], &p->aPoint[j])<0 ){
        if( rtreeSearchPointCompare(&p->aPoint[k], &p->aPoint[i])<0 ){
          rtreeSearchPointSwap(p, i, k);
          i = k;
        }else{
          break;
        }
      }else{
        if( rtreeSearchPointCompare(&p->aPoint[j], &p->aPoint[i])<0 ){
          rtreeSearchPointSwap(p, i, j);
          i = j;
        }else{
          break;
        }
      }
    }
  }
}


/*
** Continue the search on cursor pCur until the front of the queue
** contains an entry suitable for returning as a result-set row,
** or until the RtreeSearchPoint queue is empty, indicating that the
** query has completed.
*/
static int rtreeStepToLeaf(RtreeCursor *pCur){
  RtreeSearchPoint *p;
  Rtree *pRtree = RTREE_OF_CURSOR(pCur);
  RtreeNode *pNode;
  int eWithin;
  int rc = SQLITE_OK;
  int nCell;
  int nConstraint = pCur->nConstraint;
  int ii;
  int eInt;
  RtreeSearchPoint x;

  eInt = pRtree->eCoordType==RTREE_COORD_INT32;
  while( (p = rtreeSearchPointFirst(pCur))!=0 && p->iLevel>0 ){
    u8 *pCellData;
    pNode = rtreeNodeOfFirstSearchPoint(pCur, &rc);
    if( rc ) return rc;
    nCell = NCELL(pNode);
    assert( nCell<200 );
    pCellData = pNode->zData + (4+pRtree->nBytesPerCell*p->iCell);
    while( p->iCell<nCell ){
      sqlite3_rtree_dbl rScore = (sqlite3_rtree_dbl)-1;
      eWithin = FULLY_WITHIN;
      for(ii=0; ii<nConstraint; ii++){
        RtreeConstraint *pConstraint = pCur->aConstraint + ii;
        if( pConstraint->op>=RTREE_MATCH ){
          rc = rtreeCallbackConstraint(pConstraint, eInt, pCellData, p,
                                       &rScore, &eWithin);
          if( rc ) return rc;
        }else if( p->iLevel==1 ){
          rtreeLeafConstraint(pConstraint, eInt, pCellData, &eWithin);
        }else{
          rtreeNonleafConstraint(pConstraint, eInt, pCellData, &eWithin);
        }
        if( eWithin==NOT_WITHIN ){
          p->iCell++;
          pCellData += pRtree->nBytesPerCell;
          break;
        }
      }
      if( eWithin==NOT_WITHIN ) continue;
      p->iCell++;
      x.iLevel = p->iLevel - 1;
      if( x.iLevel ){
        x.id = readInt64(pCellData);
        for(ii=0; ii<pCur->nPoint; ii++){
          if( pCur->aPoint[ii].id==x.id ){
            RTREE_IS_CORRUPT(pRtree);
            return SQLITE_CORRUPT_VTAB;
          }
        }
        x.iCell = 0;
      }else{
        x.id = p->id;
        x.iCell = p->iCell - 1;
      }
      if( p->iCell>=nCell ){
        RTREE_QUEUE_TRACE(pCur, "POP-S:");
        rtreeSearchPointPop(pCur);
      }
      if( rScore<RTREE_ZERO ) rScore = RTREE_ZERO;
      p = rtreeSearchPointNew(pCur, rScore, x.iLevel);
      if( p==0 ) return SQLITE_NOMEM;
      p->eWithin = (u8)eWithin;
      p->id = x.id;
      p->iCell = x.iCell;
      RTREE_QUEUE_TRACE(pCur, "PUSH-S:");
      break;
    }
    if( p->iCell>=nCell ){
      RTREE_QUEUE_TRACE(pCur, "POP-Se:");
      rtreeSearchPointPop(pCur);
    }
  }
  pCur->atEOF = p==0;
  return SQLITE_OK;
}

/*
** Rtree virtual table module xNext method.
*/
static int rtreeNext(sqlite3_vtab_cursor *pVtabCursor){
  RtreeCursor *pCsr = (RtreeCursor *)pVtabCursor;
  int rc = SQLITE_OK;

  /* Move to the next entry that matches the configured constraints. */
  RTREE_QUEUE_TRACE(pCsr, "POP-Nx:");
  if( pCsr->bAuxValid ){
    pCsr->bAuxValid = 0;
    sqlite3_reset(pCsr->pReadAux);
  }
  rtreeSearchPointPop(pCsr);
  rc = rtreeStepToLeaf(pCsr);
  return rc;
}

/*
** Rtree virtual table module xRowid method.
*/
static int rtreeRowid(sqlite3_vtab_cursor *pVtabCursor, sqlite_int64 *pRowid){
  RtreeCursor *pCsr = (RtreeCursor *)pVtabCursor;
  RtreeSearchPoint *p = rtreeSearchPointFirst(pCsr);
  int rc = SQLITE_OK;
  RtreeNode *pNode = rtreeNodeOfFirstSearchPoint(pCsr, &rc);
  if( rc==SQLITE_OK && ALWAYS(p) ){
    if( p->iCell>=NCELL(pNode) ){
      rc = SQLITE_ABORT;
    }else{
      *pRowid = nodeGetRowid(RTREE_OF_CURSOR(pCsr), pNode, p->iCell);
    }
  }
  return rc;
}

/*
** Rtree virtual table module xColumn method.
*/
static int rtreeColumn(sqlite3_vtab_cursor *cur, sqlite3_context *ctx, int i){
  Rtree *pRtree = (Rtree *)cur->pVtab;
  RtreeCursor *pCsr = (RtreeCursor *)cur;
  RtreeSearchPoint *p = rtreeSearchPointFirst(pCsr);
  RtreeCoord c;
  int rc = SQLITE_OK;
  RtreeNode *pNode = rtreeNodeOfFirstSearchPoint(pCsr, &rc);

  if( rc ) return rc;
  if( NEVER(p==0) ) return SQLITE_OK;
  if( p->iCell>=NCELL(pNode) ) return SQLITE_ABORT;
  if( i==0 ){
    sqlite3_result_int64(ctx, nodeGetRowid(pRtree, pNode, p->iCell));
  }else if( i<=pRtree->nDim2 ){
    nodeGetCoord(pRtree, pNode, p->iCell, i-1, &c);
#ifndef SQLITE_RTREE_INT_ONLY
    if( pRtree->eCoordType==RTREE_COORD_REAL32 ){
      sqlite3_result_double(ctx, c.f);
    }else
#endif
    {
      assert( pRtree->eCoordType==RTREE_COORD_INT32 );
      sqlite3_result_int(ctx, c.i);
    }
  }else{
    if( !pCsr->bAuxValid ){
      if( pCsr->pReadAux==0 ){
        rc = sqlite3_prepare_v3(pRtree->db, pRtree->zReadAuxSql, -1, 0,
                                &pCsr->pReadAux, 0);
        if( rc ) return rc;
      }
      sqlite3_bind_int64(pCsr->pReadAux, 1,
          nodeGetRowid(pRtree, pNode, p->iCell));
      rc = sqlite3_step(pCsr->pReadAux);
      if( rc==SQLITE_ROW ){
        pCsr->bAuxValid = 1;
      }else{
        sqlite3_reset(pCsr->pReadAux);
        if( rc==SQLITE_DONE ) rc = SQLITE_OK;
        return rc;
      }
    }
    sqlite3_result_value(ctx,
         sqlite3_column_value(pCsr->pReadAux, i - pRtree->nDim2 + 1));
  }
  return SQLITE_OK;
}

/*
** Use nodeAcquire() to obtain the leaf node containing the record with
** rowid iRowid. If successful, set *ppLeaf to point to the node and
** return SQLITE_OK. If there is no such record in the table, set
** *ppLeaf to 0 and return SQLITE_OK. If an error occurs, set *ppLeaf
** to zero and return an SQLite error code.
*/
static int findLeafNode(
  Rtree *pRtree,              /* RTree to search */
  i64 iRowid,                 /* The rowid searching for */
  RtreeNode **ppLeaf,         /* Write the node here */
  sqlite3_int64 *piNode       /* Write the node-id here */
){
  int rc;
  *ppLeaf = 0;
  sqlite3_bind_int64(pRtree->pReadRowid, 1, iRowid);
  if( sqlite3_step(pRtree->pReadRowid)==SQLITE_ROW ){
    i64 iNode = sqlite3_column_int64(pRtree->pReadRowid, 0);
    if( piNode ) *piNode = iNode;
    rc = nodeAcquire(pRtree, iNode, 0, ppLeaf);
    sqlite3_reset(pRtree->pReadRowid);
  }else{
    rc = sqlite3_reset(pRtree->pReadRowid);
  }
  return rc;
}

/*
** This function is called to configure the RtreeConstraint object passed
** as the second argument for a MATCH constraint. The value passed as the
** first argument to this function is the right-hand operand to the MATCH
** operator.
*/
static int deserializeGeometry(sqlite3_value *pValue, RtreeConstraint *pCons){
  RtreeMatchArg *pBlob, *pSrc;       /* BLOB returned by geometry function */
  sqlite3_rtree_query_info *pInfo;   /* Callback information */

  pSrc = sqlite3_value_pointer(pValue, "RtreeMatchArg");
  if( pSrc==0 ) return SQLITE_ERROR;
  pInfo = (sqlite3_rtree_query_info*)
                sqlite3_malloc64( sizeof(*pInfo)+pSrc->iSize );
  if( !pInfo ) return SQLITE_NOMEM;
  memset(pInfo, 0, sizeof(*pInfo));
  pBlob = (RtreeMatchArg*)&pInfo[1];
  memcpy(pBlob, pSrc, pSrc->iSize);
  pInfo->pContext = pBlob->cb.pContext;
  pInfo->nParam = pBlob->nParam;
  pInfo->aParam = pBlob->aParam;
  pInfo->apSqlParam = pBlob->apSqlParam;

  if( pBlob->cb.xGeom ){
    pCons->u.xGeom = pBlob->cb.xGeom;
  }else{
    pCons->op = RTREE_QUERY;
    pCons->u.xQueryFunc = pBlob->cb.xQueryFunc;
  }
  pCons->pInfo = pInfo;
  return SQLITE_OK;
}

SQLITE_PRIVATE int sqlite3IntFloatCompare(i64,double);

/*
** Rtree virtual table module xFilter method.
*/
static int rtreeFilter(
  sqlite3_vtab_cursor *pVtabCursor,
  int idxNum, const char *idxStr,
  int argc, sqlite3_value **argv
){
  Rtree *pRtree = (Rtree *)pVtabCursor->pVtab;
  RtreeCursor *pCsr = (RtreeCursor *)pVtabCursor;
  RtreeNode *pRoot = 0;
  int ii;
  int rc = SQLITE_OK;
  int iCell = 0;

  rtreeReference(pRtree);

  /* Reset the cursor to the same state as rtreeOpen() leaves it in. */
  resetCursor(pCsr);

  pCsr->iStrategy = idxNum;
  if( idxNum==1 ){
    /* Special case - lookup by rowid. */
    RtreeNode *pLeaf;        /* Leaf on which the required cell resides */
    RtreeSearchPoint *p;     /* Search point for the leaf */
    i64 iRowid = sqlite3_value_int64(argv[0]);
    i64 iNode = 0;
    int eType = sqlite3_value_numeric_type(argv[0]);
    if( eType==SQLITE_INTEGER
     || (eType==SQLITE_FLOAT
         && 0==sqlite3IntFloatCompare(iRowid,sqlite3_value_double(argv[0])))
    ){
      rc = findLeafNode(pRtree, iRowid, &pLeaf, &iNode);
    }else{
      rc = SQLITE_OK;
      pLeaf = 0;
    }
    if( rc==SQLITE_OK && pLeaf!=0 ){
      p = rtreeSearchPointNew(pCsr, RTREE_ZERO, 0);
      assert( p!=0 );  /* Always returns pCsr->sPoint */
      pCsr->aNode[0] = pLeaf;
      p->id = iNode;
      p->eWithin = PARTLY_WITHIN;
      rc = nodeRowidIndex(pRtree, pLeaf, iRowid, &iCell);
      p->iCell = (u8)iCell;
      RTREE_QUEUE_TRACE(pCsr, "PUSH-F1:");
    }else{
      pCsr->atEOF = 1;
    }
  }else{
    /* Normal case - r-tree scan. Set up the RtreeCursor.aConstraint array
    ** with the configured constraints.
    */
    rc = nodeAcquire(pRtree, 1, 0, &pRoot);
    if( rc==SQLITE_OK && argc>0 ){
      pCsr->aConstraint = sqlite3_malloc64(sizeof(RtreeConstraint)*argc);
      pCsr->nConstraint = argc;
      if( !pCsr->aConstraint ){
        rc = SQLITE_NOMEM;
      }else{
        memset(pCsr->aConstraint, 0, sizeof(RtreeConstraint)*argc);
        memset(pCsr->anQueue, 0, sizeof(u32)*(pRtree->iDepth + 1));
        assert( (idxStr==0 && argc==0)
                || (idxStr && (int)strlen(idxStr)==argc*2) );
        for(ii=0; ii<argc; ii++){
          RtreeConstraint *p = &pCsr->aConstraint[ii];
          int eType = sqlite3_value_numeric_type(argv[ii]);
          p->op = idxStr[ii*2];
          p->iCoord = idxStr[ii*2+1]-'0';
          if( p->op>=RTREE_MATCH ){
            /* A MATCH operator. The right-hand-side must be a blob that
            ** can be cast into an RtreeMatchArg object. One created using
            ** an sqlite3_rtree_geometry_callback() SQL user function.
            */
            rc = deserializeGeometry(argv[ii], p);
            if( rc!=SQLITE_OK ){
              break;
            }
            p->pInfo->nCoord = pRtree->nDim2;
            p->pInfo->anQueue = pCsr->anQueue;
            p->pInfo->mxLevel = pRtree->iDepth + 1;
          }else if( eType==SQLITE_INTEGER ){
            sqlite3_int64 iVal = sqlite3_value_int64(argv[ii]);
#ifdef SQLITE_RTREE_INT_ONLY
            p->u.rValue = iVal;
#else
            p->u.rValue = (double)iVal;
            if( iVal>=((sqlite3_int64)1)<<48
             || iVal<=-(((sqlite3_int64)1)<<48)
            ){
              if( p->op==RTREE_LT ) p->op = RTREE_LE;
              if( p->op==RTREE_GT ) p->op = RTREE_GE;
            }
#endif
          }else if( eType==SQLITE_FLOAT ){
#ifdef SQLITE_RTREE_INT_ONLY
            p->u.rValue = sqlite3_value_int64(argv[ii]);
#else
            p->u.rValue = sqlite3_value_double(argv[ii]);
#endif
          }else{
            p->u.rValue = RTREE_ZERO;
            if( eType==SQLITE_NULL ){
              p->op = RTREE_FALSE;
            }else if( p->op==RTREE_LT || p->op==RTREE_LE ){
              p->op = RTREE_TRUE;
            }else{
              p->op = RTREE_FALSE;
            }
          }
        }
      }
    }
    if( rc==SQLITE_OK ){
      RtreeSearchPoint *pNew;
      assert( pCsr->bPoint==0 );  /* Due to the resetCursor() call above */
      pNew = rtreeSearchPointNew(pCsr, RTREE_ZERO, (u8)(pRtree->iDepth+1));
      if( NEVER(pNew==0) ){       /* Because pCsr->bPoint was FALSE */
        return SQLITE_NOMEM;
      }
      pNew->id = 1;
      pNew->iCell = 0;
      pNew->eWithin = PARTLY_WITHIN;
      assert( pCsr->bPoint==1 );
      pCsr->aNode[0] = pRoot;
      pRoot = 0;
      RTREE_QUEUE_TRACE(pCsr, "PUSH-Fm:");
      rc = rtreeStepToLeaf(pCsr);
    }
  }

  nodeRelease(pRtree, pRoot);
  rtreeRelease(pRtree);
  return rc;
}
