
/*
** Rtree virtual table module xBestIndex method. There are three
** table scan strategies to choose from (in order from most to
** least desirable):
**
**   idxNum     idxStr        Strategy
**   ------------------------------------------------
**     1        Unused        Direct lookup by rowid.
**     2        See below     R-tree query or full-table scan.
**   ------------------------------------------------
**
** If strategy 1 is used, then idxStr is not meaningful. If strategy
** 2 is used, idxStr is formatted to contain 2 bytes for each
** constraint used. The first two bytes of idxStr correspond to
** the constraint in sqlite3_index_info.aConstraintUsage[] with
** (argvIndex==1) etc.
**
** The first of each pair of bytes in idxStr identifies the constraint
** operator as follows:
**
**   Operator    Byte Value
**   ----------------------
**      =        0x41 ('A')
**     <=        0x42 ('B')
**      <        0x43 ('C')
**     >=        0x44 ('D')
**      >        0x45 ('E')
**   MATCH       0x46 ('F')
**   ----------------------
**
** The second of each pair of bytes identifies the coordinate column
** to which the constraint applies. The leftmost coordinate column
** is 'a', the second from the left 'b' etc.
*/
static int rtreeBestIndex(sqlite3_vtab *tab, sqlite3_index_info *pIdxInfo){
  Rtree *pRtree = (Rtree*)tab;
  int rc = SQLITE_OK;
  int ii;
  int bMatch = 0;                 /* True if there exists a MATCH constraint */
  i64 nRow;                       /* Estimated rows returned by this scan */

  int iIdx = 0;
  char zIdxStr[RTREE_MAX_DIMENSIONS*8+1];
  memset(zIdxStr, 0, sizeof(zIdxStr));

  /* Check if there exists a MATCH constraint - even an unusable one. If there
  ** is, do not consider the lookup-by-rowid plan as using such a plan would
  ** require the VDBE to evaluate the MATCH constraint, which is not currently
  ** possible. */
  for(ii=0; ii<pIdxInfo->nConstraint; ii++){
    if( pIdxInfo->aConstraint[ii].op==SQLITE_INDEX_CONSTRAINT_MATCH ){
      bMatch = 1;
    }
  }

  assert( pIdxInfo->idxStr==0 );
  for(ii=0; ii<pIdxInfo->nConstraint && iIdx<(int)(sizeof(zIdxStr)-1); ii++){
    struct sqlite3_index_constraint *p = &pIdxInfo->aConstraint[ii];

    if( bMatch==0 && p->usable
     && p->iColumn<=0 && p->op==SQLITE_INDEX_CONSTRAINT_EQ
    ){
      /* We have an equality constraint on the rowid. Use strategy 1. */
      int jj;
      for(jj=0; jj<ii; jj++){
        pIdxInfo->aConstraintUsage[jj].argvIndex = 0;
        pIdxInfo->aConstraintUsage[jj].omit = 0;
      }
      pIdxInfo->idxNum = 1;
      pIdxInfo->aConstraintUsage[ii].argvIndex = 1;
      pIdxInfo->aConstraintUsage[jj].omit = 1;

      /* This strategy involves a two rowid lookups on an B-Tree structures
      ** and then a linear search of an R-Tree node. This should be
      ** considered almost as quick as a direct rowid lookup (for which
      ** sqlite uses an internal cost of 0.0). It is expected to return
      ** a single row.
      */
      pIdxInfo->estimatedCost = 30.0;
      pIdxInfo->estimatedRows = 1;
      pIdxInfo->idxFlags = SQLITE_INDEX_SCAN_UNIQUE;
      return SQLITE_OK;
    }

    if( p->usable
    && ((p->iColumn>0 && p->iColumn<=pRtree->nDim2)
        || p->op==SQLITE_INDEX_CONSTRAINT_MATCH)
    ){
      u8 op;
      u8 doOmit = 1;
      switch( p->op ){
        case SQLITE_INDEX_CONSTRAINT_EQ:    op = RTREE_EQ;    doOmit = 0; break;
        case SQLITE_INDEX_CONSTRAINT_GT:    op = RTREE_GT;    doOmit = 0; break;
        case SQLITE_INDEX_CONSTRAINT_LE:    op = RTREE_LE;    break;
        case SQLITE_INDEX_CONSTRAINT_LT:    op = RTREE_LT;    doOmit = 0; break;
        case SQLITE_INDEX_CONSTRAINT_GE:    op = RTREE_GE;    break;
        case SQLITE_INDEX_CONSTRAINT_MATCH: op = RTREE_MATCH; break;
        default:                            op = 0;           break;
      }
      if( op ){
        zIdxStr[iIdx++] = op;
        zIdxStr[iIdx++] = (char)(p->iColumn - 1 + '0');
        pIdxInfo->aConstraintUsage[ii].argvIndex = (iIdx/2);
        pIdxInfo->aConstraintUsage[ii].omit = doOmit;
      }
    }
  }

  pIdxInfo->idxNum = 2;
  pIdxInfo->needToFreeIdxStr = 1;
  if( iIdx>0 ){
    pIdxInfo->idxStr = sqlite3_malloc( iIdx+1 );
    if( pIdxInfo->idxStr==0 ){
      return SQLITE_NOMEM;
    }
    memcpy(pIdxInfo->idxStr, zIdxStr, iIdx+1);
  }

  nRow = pRtree->nRowEst >> (iIdx/2);
  pIdxInfo->estimatedCost = (double)6.0 * (double)nRow;
  pIdxInfo->estimatedRows = nRow;

  return rc;
}

/*
** Return the N-dimensional volumn of the cell stored in *p.
*/
static RtreeDValue cellArea(Rtree *pRtree, RtreeCell *p){
  RtreeDValue area = (RtreeDValue)1;
  assert( pRtree->nDim>=1 && pRtree->nDim<=5 );
#ifndef SQLITE_RTREE_INT_ONLY
  if( pRtree->eCoordType==RTREE_COORD_REAL32 ){
    switch( pRtree->nDim ){
      case 5:  area  = p->aCoord[9].f - p->aCoord[8].f;
      case 4:  area *= p->aCoord[7].f - p->aCoord[6].f;
      case 3:  area *= p->aCoord[5].f - p->aCoord[4].f;
      case 2:  area *= p->aCoord[3].f - p->aCoord[2].f;
      default: area *= p->aCoord[1].f - p->aCoord[0].f;
    }
  }else
#endif
  {
    switch( pRtree->nDim ){
      case 5:  area  = (i64)p->aCoord[9].i - (i64)p->aCoord[8].i;
      case 4:  area *= (i64)p->aCoord[7].i - (i64)p->aCoord[6].i;
      case 3:  area *= (i64)p->aCoord[5].i - (i64)p->aCoord[4].i;
      case 2:  area *= (i64)p->aCoord[3].i - (i64)p->aCoord[2].i;
      default: area *= (i64)p->aCoord[1].i - (i64)p->aCoord[0].i;
    }
  }
  return area;
}

/*
** Return the margin length of cell p. The margin length is the sum
** of the objects size in each dimension.
*/
static RtreeDValue cellMargin(Rtree *pRtree, RtreeCell *p){
  RtreeDValue margin = 0;
  int ii = pRtree->nDim2 - 2;
  do{
    margin += (DCOORD(p->aCoord[ii+1]) - DCOORD(p->aCoord[ii]));
    ii -= 2;
  }while( ii>=0 );
  return margin;
}

/*
** Store the union of cells p1 and p2 in p1.
*/
static void cellUnion(Rtree *pRtree, RtreeCell *p1, RtreeCell *p2){
  int ii = 0;
  if( pRtree->eCoordType==RTREE_COORD_REAL32 ){
    do{
      p1->aCoord[ii].f = MIN(p1->aCoord[ii].f, p2->aCoord[ii].f);
      p1->aCoord[ii+1].f = MAX(p1->aCoord[ii+1].f, p2->aCoord[ii+1].f);
      ii += 2;
    }while( ii<pRtree->nDim2 );
  }else{
    do{
      p1->aCoord[ii].i = MIN(p1->aCoord[ii].i, p2->aCoord[ii].i);
      p1->aCoord[ii+1].i = MAX(p1->aCoord[ii+1].i, p2->aCoord[ii+1].i);
      ii += 2;
    }while( ii<pRtree->nDim2 );
  }
}

/*
** Return true if the area covered by p2 is a subset of the area covered
** by p1. False otherwise.
*/
static int cellContains(Rtree *pRtree, RtreeCell *p1, RtreeCell *p2){
  int ii;
  if( pRtree->eCoordType==RTREE_COORD_INT32 ){
    for(ii=0; ii<pRtree->nDim2; ii+=2){
      RtreeCoord *a1 = &p1->aCoord[ii];
      RtreeCoord *a2 = &p2->aCoord[ii];
      if( a2[0].i<a1[0].i || a2[1].i>a1[1].i ) return 0;
    }
  }else{
    for(ii=0; ii<pRtree->nDim2; ii+=2){
      RtreeCoord *a1 = &p1->aCoord[ii];
      RtreeCoord *a2 = &p2->aCoord[ii];
      if( a2[0].f<a1[0].f || a2[1].f>a1[1].f ) return 0;
    }
  }
  return 1;
}

static RtreeDValue cellOverlap(
  Rtree *pRtree,
  RtreeCell *p,
  RtreeCell *aCell,
  int nCell
){
  int ii;
  RtreeDValue overlap = RTREE_ZERO;
  for(ii=0; ii<nCell; ii++){
    int jj;
    RtreeDValue o = (RtreeDValue)1;
    for(jj=0; jj<pRtree->nDim2; jj+=2){
      RtreeDValue x1, x2;
      x1 = MAX(DCOORD(p->aCoord[jj]), DCOORD(aCell[ii].aCoord[jj]));
      x2 = MIN(DCOORD(p->aCoord[jj+1]), DCOORD(aCell[ii].aCoord[jj+1]));
      if( x2<x1 ){
        o = (RtreeDValue)0;
        break;
      }else{
        o = o * (x2-x1);
      }
    }
    overlap += o;
  }
  return overlap;
}


/*
** This function implements the ChooseLeaf algorithm from Gutman[84].
** ChooseSubTree in r*tree terminology.
*/
static int ChooseLeaf(
  Rtree *pRtree,               /* Rtree table */
  RtreeCell *pCell,            /* Cell to insert into rtree */
  int iHeight,                 /* Height of sub-tree rooted at pCell */
  RtreeNode **ppLeaf           /* OUT: Selected leaf page */
){
  int rc;
  int ii;
  RtreeNode *pNode = 0;
  rc = nodeAcquire(pRtree, 1, 0, &pNode);

  for(ii=0; rc==SQLITE_OK && ii<(pRtree->iDepth-iHeight); ii++){
    int iCell;
    sqlite3_int64 iBest = 0;
    int bFound = 0;
    RtreeDValue fMinGrowth = RTREE_ZERO;
    RtreeDValue fMinArea = RTREE_ZERO;
    int nCell = NCELL(pNode);
    RtreeNode *pChild = 0;

    /* First check to see if there is are any cells in pNode that completely
    ** contains pCell.  If two or more cells in pNode completely contain pCell
    ** then pick the smallest.
    */
    for(iCell=0; iCell<nCell; iCell++){
      RtreeCell cell;
      nodeGetCell(pRtree, pNode, iCell, &cell);
      if( cellContains(pRtree, &cell, pCell) ){
        RtreeDValue area = cellArea(pRtree, &cell);
        if( bFound==0 || area<fMinArea ){
          iBest = cell.iRowid;
          fMinArea = area;
          bFound = 1;
        }
      }
    }
    if( !bFound ){
      /* No cells of pNode will completely contain pCell.  So pick the
      ** cell of pNode that grows by the least amount when pCell is added.
      ** Break ties by selecting the smaller cell.
      */
      for(iCell=0; iCell<nCell; iCell++){
        RtreeCell cell;
        RtreeDValue growth;
        RtreeDValue area;
        nodeGetCell(pRtree, pNode, iCell, &cell);
        area = cellArea(pRtree, &cell);
        cellUnion(pRtree, &cell, pCell);
        growth = cellArea(pRtree, &cell)-area;
        if( iCell==0
         || growth<fMinGrowth
         || (growth==fMinGrowth && area<fMinArea)
        ){
          fMinGrowth = growth;
          fMinArea = area;
          iBest = cell.iRowid;
        }
      }
    }

    rc = nodeAcquire(pRtree, iBest, pNode, &pChild);
    nodeRelease(pRtree, pNode);
    pNode = pChild;
  }

  *ppLeaf = pNode;
  return rc;
}

/*
** A cell with the same content as pCell has just been inserted into
** the node pNode. This function updates the bounding box cells in
** all ancestor elements.
*/
static int AdjustTree(
  Rtree *pRtree,                    /* Rtree table */
  RtreeNode *pNode,                 /* Adjust ancestry of this node. */
  RtreeCell *pCell                  /* This cell was just inserted */
){
  RtreeNode *p = pNode;
  int cnt = 0;
  int rc;
  while( p->pParent ){
    RtreeNode *pParent = p->pParent;
    RtreeCell cell;
    int iCell;

    cnt++;
    if( NEVER(cnt>100) ){
      RTREE_IS_CORRUPT(pRtree);
      return SQLITE_CORRUPT_VTAB;
    }
    rc = nodeParentIndex(pRtree, p, &iCell);
    if( NEVER(rc!=SQLITE_OK) ){
      RTREE_IS_CORRUPT(pRtree);
      return SQLITE_CORRUPT_VTAB;
    }

    nodeGetCell(pRtree, pParent, iCell, &cell);
    if( !cellContains(pRtree, &cell, pCell) ){
      cellUnion(pRtree, &cell, pCell);
      nodeOverwriteCell(pRtree, pParent, &cell, iCell);
    }

    p = pParent;
  }
  return SQLITE_OK;
}
