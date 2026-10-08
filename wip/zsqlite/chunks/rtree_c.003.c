#elif SQLITE_BYTEORDER==1234 && GCC_VERSION>=4003000
#define RTREE_DECODE_COORD(eInt, a, r) {                        \
    RtreeCoord c;    /* Coordinate decoded */                   \
    c.u = __builtin_bswap32(*(u32*)a);                          \
    r = eInt ? (sqlite3_rtree_dbl)c.i : (sqlite3_rtree_dbl)c.f; \
}
#elif SQLITE_BYTEORDER==1234
#define RTREE_DECODE_COORD(eInt, a, r) {                        \
    RtreeCoord c;    /* Coordinate decoded */                   \
    memcpy(&c.u,a,4);                                           \
    c.u = ((c.u>>24)&0xff)|((c.u>>8)&0xff00)|                   \
          ((c.u&0xff)<<24)|((c.u&0xff00)<<8);                   \
    r = eInt ? (sqlite3_rtree_dbl)c.i : (sqlite3_rtree_dbl)c.f; \
}
#elif SQLITE_BYTEORDER==4321
#define RTREE_DECODE_COORD(eInt, a, r) {                        \
    RtreeCoord c;    /* Coordinate decoded */                   \
    memcpy(&c.u,a,4);                                           \
    r = eInt ? (sqlite3_rtree_dbl)c.i : (sqlite3_rtree_dbl)c.f; \
}
#else
#define RTREE_DECODE_COORD(eInt, a, r) {                        \
    RtreeCoord c;    /* Coordinate decoded */                   \
    c.u = ((u32)a[0]<<24) + ((u32)a[1]<<16)                     \
           +((u32)a[2]<<8) + a[3];                              \
    r = eInt ? (sqlite3_rtree_dbl)c.i : (sqlite3_rtree_dbl)c.f; \
}
#endif

/*
** Check the RTree node or entry given by pCellData and p against the MATCH
** constraint pConstraint.
*/
static int rtreeCallbackConstraint(
  RtreeConstraint *pConstraint,  /* The constraint to test */
  int eInt,                      /* True if RTree holding integer coordinates */
  u8 *pCellData,                 /* Raw cell content */
  RtreeSearchPoint *pSearch,     /* Container of this cell */
  sqlite3_rtree_dbl *prScore,    /* OUT: score for the cell */
  int *peWithin                  /* OUT: visibility of the cell */
){
  sqlite3_rtree_query_info *pInfo = pConstraint->pInfo; /* Callback info */
  int nCoord = pInfo->nCoord;                           /* No. of coordinates */
  int rc;                                             /* Callback return code */
  RtreeCoord c;                                       /* Translator union */
  sqlite3_rtree_dbl aCoord[RTREE_MAX_DIMENSIONS*2];   /* Decoded coordinates */

  assert( pConstraint->op==RTREE_MATCH || pConstraint->op==RTREE_QUERY );
  assert( nCoord==2 || nCoord==4 || nCoord==6 || nCoord==8 || nCoord==10 );

  if( pConstraint->op==RTREE_QUERY && pSearch->iLevel==1 ){
    pInfo->iRowid = readInt64(pCellData);
  }
  pCellData += 8;
#ifndef SQLITE_RTREE_INT_ONLY
  if( eInt==0 ){
    switch( nCoord ){
      case 10:  readCoord(pCellData+36, &c); aCoord[9] = c.f;
                readCoord(pCellData+32, &c); aCoord[8] = c.f;
      case 8:   readCoord(pCellData+28, &c); aCoord[7] = c.f;
                readCoord(pCellData+24, &c); aCoord[6] = c.f;
      case 6:   readCoord(pCellData+20, &c); aCoord[5] = c.f;
                readCoord(pCellData+16, &c); aCoord[4] = c.f;
      case 4:   readCoord(pCellData+12, &c); aCoord[3] = c.f;
                readCoord(pCellData+8,  &c); aCoord[2] = c.f;
      default:  readCoord(pCellData+4,  &c); aCoord[1] = c.f;
                readCoord(pCellData,    &c); aCoord[0] = c.f;
    }
  }else
#endif
  {
    switch( nCoord ){
      case 10:  readCoord(pCellData+36, &c); aCoord[9] = c.i;
                readCoord(pCellData+32, &c); aCoord[8] = c.i;
      case 8:   readCoord(pCellData+28, &c); aCoord[7] = c.i;
                readCoord(pCellData+24, &c); aCoord[6] = c.i;
      case 6:   readCoord(pCellData+20, &c); aCoord[5] = c.i;
                readCoord(pCellData+16, &c); aCoord[4] = c.i;
      case 4:   readCoord(pCellData+12, &c); aCoord[3] = c.i;
                readCoord(pCellData+8,  &c); aCoord[2] = c.i;
      default:  readCoord(pCellData+4,  &c); aCoord[1] = c.i;
                readCoord(pCellData,    &c); aCoord[0] = c.i;
    }
  }
  if( pConstraint->op==RTREE_MATCH ){
    int eWithin = 0;
    rc = pConstraint->u.xGeom((sqlite3_rtree_geometry*)pInfo,
                              nCoord, aCoord, &eWithin);
    if( eWithin==0 ) *peWithin = NOT_WITHIN;
    *prScore = RTREE_ZERO;
  }else{
    pInfo->aCoord = aCoord;
    pInfo->iLevel = pSearch->iLevel - 1;
    pInfo->rScore = pInfo->rParentScore = pSearch->rScore;
    pInfo->eWithin = pInfo->eParentWithin = pSearch->eWithin;
    rc = pConstraint->u.xQueryFunc(pInfo);
    if( pInfo->eWithin<*peWithin ) *peWithin = pInfo->eWithin;
    if( pInfo->rScore<*prScore || *prScore<RTREE_ZERO ){
      *prScore = pInfo->rScore;
    }
  }
  return rc;
}

/*
** Check the internal RTree node given by pCellData against constraint p.
** If this constraint cannot be satisfied by any child within the node,
** set *peWithin to NOT_WITHIN.
*/
static void rtreeNonleafConstraint(
  RtreeConstraint *p,        /* The constraint to test */
  int eInt,                  /* True if RTree holds integer coordinates */
  u8 *pCellData,             /* Raw cell content as appears on disk */
  int *peWithin              /* Adjust downward, as appropriate */
){
  sqlite3_rtree_dbl val;     /* Coordinate value convert to a double */

  /* p->iCoord might point to either a lower or upper bound coordinate
  ** in a coordinate pair.  But make pCellData point to the lower bound.
  */
  pCellData += 8 + 4*(p->iCoord&0xfe);

  assert(p->op==RTREE_LE || p->op==RTREE_LT || p->op==RTREE_GE
      || p->op==RTREE_GT || p->op==RTREE_EQ || p->op==RTREE_TRUE
      || p->op==RTREE_FALSE );
  assert( FOUR_BYTE_ALIGNED(pCellData) );
  switch( p->op ){
    case RTREE_TRUE:  return;   /* Always satisfied */
    case RTREE_FALSE: break;    /* Never satisfied */
    case RTREE_EQ:
      RTREE_DECODE_COORD(eInt, pCellData, val);
      /* val now holds the lower bound of the coordinate pair */
      if( p->u.rValue>=val ){
        pCellData += 4;
        RTREE_DECODE_COORD(eInt, pCellData, val);
        /* val now holds the upper bound of the coordinate pair */
        if( p->u.rValue<=val ) return;
      }
      break;
    case RTREE_LE:
    case RTREE_LT:
      RTREE_DECODE_COORD(eInt, pCellData, val);
      /* val now holds the lower bound of the coordinate pair */
      if( p->u.rValue>=val ) return;
      break;

    default:
      pCellData += 4;
      RTREE_DECODE_COORD(eInt, pCellData, val);
      /* val now holds the upper bound of the coordinate pair */
      if( p->u.rValue<=val ) return;
      break;
  }
  *peWithin = NOT_WITHIN;
}

/*
** Check the leaf RTree cell given by pCellData against constraint p.
** If this constraint is not satisfied, set *peWithin to NOT_WITHIN.
** If the constraint is satisfied, leave *peWithin unchanged.
**
** The constraint is of the form:  xN op $val
**
** The op is given by p->op.  The xN is p->iCoord-th coordinate in
** pCellData.  $val is given by p->u.rValue.
*/
static void rtreeLeafConstraint(
  RtreeConstraint *p,        /* The constraint to test */
  int eInt,                  /* True if RTree holds integer coordinates */
  u8 *pCellData,             /* Raw cell content as appears on disk */
  int *peWithin              /* Adjust downward, as appropriate */
){
  RtreeDValue xN;      /* Coordinate value converted to a double */

  assert(p->op==RTREE_LE || p->op==RTREE_LT || p->op==RTREE_GE
      || p->op==RTREE_GT || p->op==RTREE_EQ || p->op==RTREE_TRUE
      || p->op==RTREE_FALSE );
  pCellData += 8 + p->iCoord*4;
  assert( FOUR_BYTE_ALIGNED(pCellData) );
  RTREE_DECODE_COORD(eInt, pCellData, xN);
  switch( p->op ){
    case RTREE_TRUE:  return;   /* Always satisfied */
    case RTREE_FALSE: break;    /* Never satisfied */
    case RTREE_LE:    if( xN <= p->u.rValue ) return;  break;
    case RTREE_LT:    if( xN <  p->u.rValue ) return;  break;
    case RTREE_GE:    if( xN >= p->u.rValue ) return;  break;
    case RTREE_GT:    if( xN >  p->u.rValue ) return;  break;
    default:          if( xN == p->u.rValue ) return;  break;
  }
  *peWithin = NOT_WITHIN;
}

/*
** One of the cells in node pNode is guaranteed to have a 64-bit
** integer value equal to iRowid. Return the index of this cell.
*/
static int nodeRowidIndex(
  Rtree *pRtree,
  RtreeNode *pNode,
  i64 iRowid,
  int *piIndex
){
  int ii;
  int nCell = NCELL(pNode);
  assert( nCell<200 );
  for(ii=0; ii<nCell; ii++){
    if( nodeGetRowid(pRtree, pNode, ii)==iRowid ){
      *piIndex = ii;
      return SQLITE_OK;
    }
  }
  RTREE_IS_CORRUPT(pRtree);
  return SQLITE_CORRUPT_VTAB;
}

/*
** Return the index of the cell containing a pointer to node pNode
** in its parent. If pNode is the root node, return -1.
*/
static int nodeParentIndex(Rtree *pRtree, RtreeNode *pNode, int *piIndex){
  RtreeNode *pParent = pNode->pParent;
  if( ALWAYS(pParent) ){
    return nodeRowidIndex(pRtree, pParent, pNode->iNode, piIndex);
  }else{
    *piIndex = -1;
    return SQLITE_OK;
  }
}

/*
** Compare two search points.  Return negative, zero, or positive if the first
** is less than, equal to, or greater than the second.
**
** The rScore is the primary key.  Smaller rScore values come first.
** If the rScore is a tie, then use iLevel as the tie breaker with smaller
** iLevel values coming first.  In this way, if rScore is the same for all
** SearchPoints, then iLevel becomes the deciding factor and the result
** is a depth-first search, which is the desired default behavior.
*/
static int rtreeSearchPointCompare(
  const RtreeSearchPoint *pA,
  const RtreeSearchPoint *pB
){
  if( pA->rScore<pB->rScore ) return -1;
  if( pA->rScore>pB->rScore ) return +1;
  if( pA->iLevel<pB->iLevel ) return -1;
  if( pA->iLevel>pB->iLevel ) return +1;
  return 0;
}

/*
** Interchange two search points in a cursor.
*/
static void rtreeSearchPointSwap(RtreeCursor *p, int i, int j){
  RtreeSearchPoint t = p->aPoint[i];
  assert( i<j );
  p->aPoint[i] = p->aPoint[j];
  p->aPoint[j] = t;
  i++; j++;
  if( i<RTREE_CACHE_SZ ){
    if( j>=RTREE_CACHE_SZ ){
      nodeRelease(RTREE_OF_CURSOR(p), p->aNode[i]);
      p->aNode[i] = 0;
    }else{
      RtreeNode *pTemp = p->aNode[i];
      p->aNode[i] = p->aNode[j];
      p->aNode[j] = pTemp;
    }
  }
}

/*
** Return the search point with the lowest current score.
*/
static RtreeSearchPoint *rtreeSearchPointFirst(RtreeCursor *pCur){
  return pCur->bPoint ? &pCur->sPoint : pCur->nPoint ? pCur->aPoint : 0;
}

/*
** Get the RtreeNode for the search point with the lowest score.
*/
static RtreeNode *rtreeNodeOfFirstSearchPoint(RtreeCursor *pCur, int *pRC){
  sqlite3_int64 id;
  int ii = 1 - pCur->bPoint;
  assert( ii==0 || ii==1 );
  assert( pCur->bPoint || pCur->nPoint );
  if( pCur->aNode[ii]==0 ){
    assert( pRC!=0 );
    id = ii ? pCur->aPoint[0].id : pCur->sPoint.id;
    *pRC = nodeAcquire(RTREE_OF_CURSOR(pCur), id, 0, &pCur->aNode[ii]);
  }
  return pCur->aNode[ii];
}

/*
** Push a new element onto the priority queue
*/
static RtreeSearchPoint *rtreeEnqueue(
  RtreeCursor *pCur,    /* The cursor */
  RtreeDValue rScore,   /* Score for the new search point */
  u8 iLevel             /* Level for the new search point */
){
  int i, j;
  RtreeSearchPoint *pNew;
  if( pCur->nPoint>=pCur->nPointAlloc ){
    int nNew = pCur->nPointAlloc*2 + 8;
    pNew = sqlite3_realloc64(pCur->aPoint, nNew*sizeof(pCur->aPoint[0]));
    if( pNew==0 ) return 0;
    pCur->aPoint = pNew;
    pCur->nPointAlloc = nNew;
  }
  i = pCur->nPoint++;
  pNew = pCur->aPoint + i;
  pNew->rScore = rScore;
  pNew->iLevel = iLevel;
  assert( iLevel<=RTREE_MAX_DEPTH );
  while( i>0 ){
    RtreeSearchPoint *pParent;
    j = (i-1)/2;
    pParent = pCur->aPoint + j;
    if( rtreeSearchPointCompare(pNew, pParent)>=0 ) break;
    rtreeSearchPointSwap(pCur, j, i);
    i = j;
    pNew = pParent;
  }
  return pNew;
}

/*
** Allocate a new RtreeSearchPoint and return a pointer to it.  Return
** NULL if malloc fails.
*/
static RtreeSearchPoint *rtreeSearchPointNew(
  RtreeCursor *pCur,    /* The cursor */
  RtreeDValue rScore,   /* Score for the new search point */
  u8 iLevel             /* Level for the new search point */
){
  RtreeSearchPoint *pNew, *pFirst;
  pFirst = rtreeSearchPointFirst(pCur);
  pCur->anQueue[iLevel]++;
  if( pFirst==0
   || pFirst->rScore>rScore
   || (pFirst->rScore==rScore && pFirst->iLevel>iLevel)
  ){
    if( pCur->bPoint ){
      int ii;
      pNew = rtreeEnqueue(pCur, rScore, iLevel);
      if( pNew==0 ) return 0;
      ii = (int)(pNew - pCur->aPoint) + 1;
      assert( ii==1 );
      if( ALWAYS(ii<RTREE_CACHE_SZ) ){
        assert( pCur->aNode[ii]==0 );
        pCur->aNode[ii] = pCur->aNode[0];
      }else{
        nodeRelease(RTREE_OF_CURSOR(pCur), pCur->aNode[0]);
      }
      pCur->aNode[0] = 0;
      *pNew = pCur->sPoint;
    }
    pCur->sPoint.rScore = rScore;
    pCur->sPoint.iLevel = iLevel;
    pCur->bPoint = 1;
    return &pCur->sPoint;
  }else{
    return rtreeEnqueue(pCur, rScore, iLevel);
  }
}
