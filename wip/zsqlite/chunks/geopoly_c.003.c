
/*
** SQL function:    geopoly_overlap(P1,P2)
**
** Determine whether or not P1 and P2 overlap. Return value:
**
**   0     The two polygons are disjoint
**   1     They overlap
**   2     P1 is completely contained within P2
**   3     P2 is completely contained within P1
**   4     P1 and P2 are the same polygon
**   NULL  Either P1 or P2 or both are not valid polygons
*/
static void geopolyOverlapFunc(
  sqlite3_context *context,
  int argc,
  sqlite3_value **argv
){
  GeoPoly *p1 = geopolyFuncParam(context, argv[0], 0);
  GeoPoly *p2 = geopolyFuncParam(context, argv[1], 0);
  (void)argc;
  if( p1 && p2 ){
    int x = geopolyOverlap(p1, p2);
    if( x<0 ){
      sqlite3_result_error_nomem(context);
    }else{
      sqlite3_result_int(context, x);
    }
  }
  sqlite3_free(p1);
  sqlite3_free(p2);
}

/*
** Enable or disable debugging output
*/
static void geopolyDebugFunc(
  sqlite3_context *context,
  int argc,
  sqlite3_value **argv
){
  (void)context;
  (void)argc;
#ifdef GEOPOLY_ENABLE_DEBUG
  geo_debug = sqlite3_value_int(argv[0]);
#else
  (void)argv;
#endif
}

/*
** This function is the implementation of both the xConnect and xCreate
** methods of the geopoly virtual table.
**
**   argv[0]   -> module name
**   argv[1]   -> database name
**   argv[2]   -> table name
**   argv[...] -> column names...
*/
static int geopolyInit(
  sqlite3 *db,                        /* Database connection */
  void *pAux,                         /* One of the RTREE_COORD_* constants */
  int argc, const char *const*argv,   /* Parameters to CREATE TABLE statement */
  sqlite3_vtab **ppVtab,              /* OUT: New virtual table */
  char **pzErr,                       /* OUT: Error message, if any */
  int isCreate                        /* True for xCreate, false for xConnect */
){
  int rc = SQLITE_OK;
  Rtree *pRtree;
  sqlite3_int64 nDb;              /* Length of string argv[1] */
  sqlite3_int64 nName;            /* Length of string argv[2] */
  sqlite3_str *pSql;
  char *zSql;
  int ii;
  (void)pAux;

  sqlite3_vtab_config(db, SQLITE_VTAB_CONSTRAINT_SUPPORT, 1);
  sqlite3_vtab_config(db, SQLITE_VTAB_INNOCUOUS);

  /* Allocate the sqlite3_vtab structure */
  nDb = strlen(argv[1]);
  nName = strlen(argv[2]);
  pRtree = (Rtree *)sqlite3_malloc64(sizeof(Rtree)+nDb+nName*2+8);
  if( !pRtree ){
    return SQLITE_NOMEM;
  }
  memset(pRtree, 0, sizeof(Rtree)+nDb+nName*2+8);
  pRtree->nBusy = 1;
  pRtree->base.pModule = &rtreeModule;
  pRtree->zDb = (char *)&pRtree[1];
  pRtree->zName = &pRtree->zDb[nDb+1];
  pRtree->zNodeName = &pRtree->zName[nName+1];
  pRtree->eCoordType = RTREE_COORD_REAL32;
  pRtree->nDim = 2;
  pRtree->nDim2 = 4;
  memcpy(pRtree->zDb, argv[1], nDb);
  memcpy(pRtree->zName, argv[2], nName);
  memcpy(pRtree->zNodeName, argv[2], nName);
  memcpy(&pRtree->zNodeName[nName], "_node", 6);


  /* Create/Connect to the underlying relational database schema. If
  ** that is successful, call sqlite3_declare_vtab() to configure
  ** the r-tree table schema.
  */
  pSql = sqlite3_str_new(db);
  sqlite3_str_appendf(pSql, "CREATE TABLE x(_shape");
  pRtree->nAux = 1;         /* Add one for _shape */
  pRtree->nAuxNotNull = 1;  /* The _shape column is always not-null */
  for(ii=3; ii<argc; ii++){
    pRtree->nAux++;
    sqlite3_str_appendf(pSql, ",%s", argv[ii]);
  }
  sqlite3_str_appendf(pSql, ");");
  zSql = sqlite3_str_finish(pSql);
  if( !zSql ){
    rc = SQLITE_NOMEM;
  }else if( SQLITE_OK!=(rc = sqlite3_declare_vtab(db, zSql)) ){
    *pzErr = sqlite3_mprintf("%s", sqlite3_errmsg(db));
  }
  sqlite3_free(zSql);
  if( rc ) goto geopolyInit_fail;
  pRtree->nBytesPerCell = 8 + pRtree->nDim2*4;

  /* Figure out the node size to use. */
  rc = getNodeSize(db, pRtree, isCreate, pzErr);
  if( rc ) goto geopolyInit_fail;
  rc = rtreeSqlInit(pRtree, db, argv[1], argv[2], isCreate);
  if( rc ){
    *pzErr = sqlite3_mprintf("%s", sqlite3_errmsg(db));
    goto geopolyInit_fail;
  }

  *ppVtab = (sqlite3_vtab *)pRtree;
  return SQLITE_OK;

geopolyInit_fail:
  if( rc==SQLITE_OK ) rc = SQLITE_ERROR;
  assert( *ppVtab==0 );
  assert( pRtree->nBusy==1 );
  rtreeRelease(pRtree);
  return rc;
}


/*
** GEOPOLY virtual table module xCreate method.
*/
static int geopolyCreate(
  sqlite3 *db,
  void *pAux,
  int argc, const char *const*argv,
  sqlite3_vtab **ppVtab,
  char **pzErr
){
  return geopolyInit(db, pAux, argc, argv, ppVtab, pzErr, 1);
}

/*
** GEOPOLY virtual table module xConnect method.
*/
static int geopolyConnect(
  sqlite3 *db,
  void *pAux,
  int argc, const char *const*argv,
  sqlite3_vtab **ppVtab,
  char **pzErr
){
  return geopolyInit(db, pAux, argc, argv, ppVtab, pzErr, 0);
}


/*
** GEOPOLY virtual table module xFilter method.
**
** Query plans:
**
**      1         rowid lookup
**      2         search for objects overlapping the same bounding box
**                that contains polygon argv[0]
**      3         search for objects overlapping the same bounding box
**                that contains polygon argv[0]
**      4         full table scan
*/
static int geopolyFilter(
  sqlite3_vtab_cursor *pVtabCursor,     /* The cursor to initialize */
  int idxNum,                           /* Query plan */
  const char *idxStr,                   /* Not Used */
  int argc, sqlite3_value **argv        /* Parameters to the query plan */
){
  Rtree *pRtree = (Rtree *)pVtabCursor->pVtab;
  RtreeCursor *pCsr = (RtreeCursor *)pVtabCursor;
  RtreeNode *pRoot = 0;
  int rc = SQLITE_OK;
  int iCell = 0;
  (void)idxStr;

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
    rc = findLeafNode(pRtree, iRowid, &pLeaf, &iNode);
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
    if( rc==SQLITE_OK && idxNum<=3 ){
      RtreeCoord bbox[4];
      RtreeConstraint *p;
      assert( argc==1 );
      assert( argv[0]!=0 );
      geopolyBBox(0, argv[0], bbox, &rc);
      if( rc ){
        goto geopoly_filter_end;
      }
      pCsr->aConstraint = p = sqlite3_malloc(sizeof(RtreeConstraint)*4);
      pCsr->nConstraint = 4;
      if( p==0 ){
        rc = SQLITE_NOMEM;
      }else{
        memset(pCsr->aConstraint, 0, sizeof(RtreeConstraint)*4);
        memset(pCsr->anQueue, 0, sizeof(u32)*(pRtree->iDepth + 1));
        if( idxNum==2 ){
          /* Overlap query */
          p->op = 'B';
          p->iCoord = 0;
          p->u.rValue = bbox[1].f;
          p++;
          p->op = 'D';
          p->iCoord = 1;
          p->u.rValue = bbox[0].f;
          p++;
          p->op = 'B';
          p->iCoord = 2;
          p->u.rValue = bbox[3].f;
          p++;
          p->op = 'D';
          p->iCoord = 3;
          p->u.rValue = bbox[2].f;
        }else{
          /* Within query */
          p->op = 'D';
          p->iCoord = 0;
          p->u.rValue = bbox[0].f;
          p++;
          p->op = 'B';
          p->iCoord = 1;
          p->u.rValue = bbox[1].f;
          p++;
          p->op = 'D';
          p->iCoord = 2;
          p->u.rValue = bbox[2].f;
          p++;
          p->op = 'B';
          p->iCoord = 3;
          p->u.rValue = bbox[3].f;
        }
      }
    }
    if( rc==SQLITE_OK ){
      RtreeSearchPoint *pNew;
      pNew = rtreeSearchPointNew(pCsr, RTREE_ZERO, (u8)(pRtree->iDepth+1));
      if( pNew==0 ){
        rc = SQLITE_NOMEM;
        goto geopoly_filter_end;
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

geopoly_filter_end:
  nodeRelease(pRtree, pRoot);
  rtreeRelease(pRtree);
  return rc;
}

/*
** Rtree virtual table module xBestIndex method. There are three
** table scan strategies to choose from (in order from most to
** least desirable):
**
**   idxNum     idxStr        Strategy
**   ------------------------------------------------
**     1        "rowid"       Direct lookup by rowid.
**     2        "rtree"       R-tree overlap query using geopoly_overlap()
**     3        "rtree"       R-tree within query using geopoly_within()
**     4        "fullscan"    full-table scan.
**   ------------------------------------------------
*/
static int geopolyBestIndex(sqlite3_vtab *tab, sqlite3_index_info *pIdxInfo){
  int ii;
  int iRowidTerm = -1;
  int iFuncTerm = -1;
  int idxNum = 0;
  (void)tab;

  for(ii=0; ii<pIdxInfo->nConstraint; ii++){
    struct sqlite3_index_constraint *p = &pIdxInfo->aConstraint[ii];
    if( !p->usable ) continue;
    if( p->iColumn<0 && p->op==SQLITE_INDEX_CONSTRAINT_EQ  ){
      iRowidTerm = ii;
      break;
    }
    if( p->iColumn==0 && p->op>=SQLITE_INDEX_CONSTRAINT_FUNCTION ){
      /* p->op==SQLITE_INDEX_CONSTRAINT_FUNCTION for geopoly_overlap()
      ** p->op==(SQLITE_INDEX_CONTRAINT_FUNCTION+1) for geopoly_within().
      ** See geopolyFindFunction() */
      iFuncTerm = ii;
      idxNum = p->op - SQLITE_INDEX_CONSTRAINT_FUNCTION + 2;
    }
  }

  if( iRowidTerm>=0 ){
    pIdxInfo->idxNum = 1;
    pIdxInfo->idxStr = "rowid";
    pIdxInfo->aConstraintUsage[iRowidTerm].argvIndex = 1;
    pIdxInfo->aConstraintUsage[iRowidTerm].omit = 1;
    pIdxInfo->estimatedCost = 30.0;
    pIdxInfo->estimatedRows = 1;
    pIdxInfo->idxFlags = SQLITE_INDEX_SCAN_UNIQUE;
    return SQLITE_OK;
  }
  if( iFuncTerm>=0 ){
    pIdxInfo->idxNum = idxNum;
    pIdxInfo->idxStr = "rtree";
    pIdxInfo->aConstraintUsage[iFuncTerm].argvIndex = 1;
    pIdxInfo->aConstraintUsage[iFuncTerm].omit = 0;
    pIdxInfo->estimatedCost = 300.0;
    pIdxInfo->estimatedRows = 10;
    return SQLITE_OK;
  }
  pIdxInfo->idxNum = 4;
  pIdxInfo->idxStr = "fullscan";
  pIdxInfo->estimatedCost = 3000000.0;
  pIdxInfo->estimatedRows = 100000;
  return SQLITE_OK;
}
