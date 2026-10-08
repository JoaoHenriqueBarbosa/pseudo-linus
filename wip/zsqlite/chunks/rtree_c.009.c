
/*
** The second argument to this function contains the text of an SQL statement
** that returns a single integer value. The statement is compiled and executed
** using database connection db. If successful, the integer value returned
** is written to *piVal and SQLITE_OK returned. Otherwise, an SQLite error
** code is returned and the value of *piVal after returning is not defined.
*/
static int getIntFromStmt(sqlite3 *db, const char *zSql, int *piVal){
  int rc = SQLITE_NOMEM;
  if( zSql ){
    sqlite3_stmt *pStmt = 0;
    rc = sqlite3_prepare_v2(db, zSql, -1, &pStmt, 0);
    if( rc==SQLITE_OK ){
      if( SQLITE_ROW==sqlite3_step(pStmt) ){
        *piVal = sqlite3_column_int(pStmt, 0);
      }
      rc = sqlite3_finalize(pStmt);
    }
  }
  return rc;
}

/*
** This function is called from within the xConnect() or xCreate() method to
** determine the node-size used by the rtree table being created or connected
** to. If successful, pRtree->iNodeSize is populated and SQLITE_OK returned.
** Otherwise, an SQLite error code is returned.
**
** If this function is being called as part of an xConnect(), then the rtree
** table already exists. In this case the node-size is determined by inspecting
** the root node of the tree.
**
** Otherwise, for an xCreate(), use 64 bytes less than the database page-size.
** This ensures that each node is stored on a single database page. If the
** database page-size is so large that more than RTREE_MAXCELLS entries
** would fit in a single node, use a smaller node-size.
*/
static int getNodeSize(
  sqlite3 *db,                    /* Database handle */
  Rtree *pRtree,                  /* Rtree handle */
  int isCreate,                   /* True for xCreate, false for xConnect */
  char **pzErr                    /* OUT: Error message, if any */
){
  int rc;
  char *zSql;
  if( isCreate ){
    int iPageSize = 0;
    zSql = sqlite3_mprintf("PRAGMA %Q.page_size", pRtree->zDb);
    rc = getIntFromStmt(db, zSql, &iPageSize);
    if( rc==SQLITE_OK ){
      pRtree->iNodeSize = iPageSize-64;
      if( (4+pRtree->nBytesPerCell*RTREE_MAXCELLS)<pRtree->iNodeSize ){
        pRtree->iNodeSize = 4+pRtree->nBytesPerCell*RTREE_MAXCELLS;
      }
    }else{
      *pzErr = sqlite3_mprintf("%s", sqlite3_errmsg(db));
    }
  }else{
    zSql = sqlite3_mprintf(
        "SELECT length(data) FROM '%q'.'%q_node' WHERE nodeno = 1",
        pRtree->zDb, pRtree->zName
    );
    rc = getIntFromStmt(db, zSql, &pRtree->iNodeSize);
    if( rc!=SQLITE_OK ){
      *pzErr = sqlite3_mprintf("%s", sqlite3_errmsg(db));
    }else if( pRtree->iNodeSize<(512-64) ){
      rc = SQLITE_CORRUPT_VTAB;
      RTREE_IS_CORRUPT(pRtree);
      *pzErr = sqlite3_mprintf("undersize RTree blobs in \"%q_node\"",
                               pRtree->zName);
    }
  }

  sqlite3_free(zSql);
  return rc;
}

/*
** Return the length of a token
*/
static int rtreeTokenLength(const char *z){
  int dummy = 0;
  return sqlite3GetToken((const unsigned char*)z,&dummy);
}

/*
** This function is the implementation of both the xConnect and xCreate
** methods of the r-tree virtual table.
**
**   argv[0]   -> module name
**   argv[1]   -> database name
**   argv[2]   -> table name
**   argv[...] -> column names...
*/
static int rtreeInit(
  sqlite3 *db,                        /* Database connection */
  void *pAux,                         /* One of the RTREE_COORD_* constants */
  int argc, const char *const*argv,   /* Parameters to CREATE TABLE statement */
  sqlite3_vtab **ppVtab,              /* OUT: New virtual table */
  char **pzErr,                       /* OUT: Error message, if any */
  int isCreate                        /* True for xCreate, false for xConnect */
){
  int rc = SQLITE_OK;
  Rtree *pRtree;
  int nDb;              /* Length of string argv[1] */
  int nName;            /* Length of string argv[2] */
  int eCoordType = (pAux ? RTREE_COORD_INT32 : RTREE_COORD_REAL32);
  sqlite3_str *pSql;
  char *zSql;
  int ii = 4;
  int iErr;

  const char *aErrMsg[] = {
    0,                                                    /* 0 */
    "Wrong number of columns for an rtree table",         /* 1 */
    "Too few columns for an rtree table",                 /* 2 */
    "Too many columns for an rtree table",                /* 3 */
    "Auxiliary rtree columns must be last"                /* 4 */
  };

  assert( RTREE_MAX_AUX_COLUMN<256 ); /* Aux columns counted by a u8 */
  if( argc<6 || argc>RTREE_MAX_AUX_COLUMN+3 ){
    *pzErr = sqlite3_mprintf("%s", aErrMsg[2 + (argc>=6)]);
    return SQLITE_ERROR;
  }

  sqlite3_vtab_config(db, SQLITE_VTAB_CONSTRAINT_SUPPORT, 1);
  sqlite3_vtab_config(db, SQLITE_VTAB_INNOCUOUS);


  /* Allocate the sqlite3_vtab structure */
  nDb = (int)strlen(argv[1]);
  nName = (int)strlen(argv[2]);
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
  pRtree->eCoordType = (u8)eCoordType;
  memcpy(pRtree->zDb, argv[1], nDb);
  memcpy(pRtree->zName, argv[2], nName);
  memcpy(pRtree->zNodeName, argv[2], nName);
  memcpy(&pRtree->zNodeName[nName], "_node", 6);


  /* Create/Connect to the underlying relational database schema. If
  ** that is successful, call sqlite3_declare_vtab() to configure
  ** the r-tree table schema.
  */
  pSql = sqlite3_str_new(db);
  sqlite3_str_appendf(pSql, "CREATE TABLE x(%.*s INT",
                      rtreeTokenLength(argv[3]), argv[3]);
  for(ii=4; ii<argc; ii++){
    const char *zArg = argv[ii];
    if( zArg[0]=='+' ){
      pRtree->nAux++;
      sqlite3_str_appendf(pSql, ",%.*s", rtreeTokenLength(zArg+1), zArg+1);
    }else if( pRtree->nAux>0 ){
      break;
    }else{
      static const char *azFormat[] = {",%.*s REAL", ",%.*s INT"};
      pRtree->nDim2++;
      sqlite3_str_appendf(pSql, azFormat[eCoordType],
                          rtreeTokenLength(zArg), zArg);
    }
  }
  sqlite3_str_appendf(pSql, ");");
  zSql = sqlite3_str_finish(pSql);
  if( !zSql ){
    rc = SQLITE_NOMEM;
  }else if( ii<argc ){
    *pzErr = sqlite3_mprintf("%s", aErrMsg[4]);
    rc = SQLITE_ERROR;
  }else if( SQLITE_OK!=(rc = sqlite3_declare_vtab(db, zSql)) ){
    *pzErr = sqlite3_mprintf("%s", sqlite3_errmsg(db));
  }
  sqlite3_free(zSql);
  if( rc ) goto rtreeInit_fail;
  pRtree->nDim = pRtree->nDim2/2;
  if( pRtree->nDim<1 ){
    iErr = 2;
  }else if( pRtree->nDim2>RTREE_MAX_DIMENSIONS*2 ){
    iErr = 3;
  }else if( pRtree->nDim2 % 2 ){
    iErr = 1;
  }else{
    iErr = 0;
  }
  if( iErr ){
    *pzErr = sqlite3_mprintf("%s", aErrMsg[iErr]);
    goto rtreeInit_fail;
  }
  pRtree->nBytesPerCell = 8 + pRtree->nDim2*4;

  /* Figure out the node size to use. */
  rc = getNodeSize(db, pRtree, isCreate, pzErr);
  if( rc ) goto rtreeInit_fail;
  rc = rtreeSqlInit(pRtree, db, argv[1], argv[2], isCreate);
  if( rc ){
    *pzErr = sqlite3_mprintf("%s", sqlite3_errmsg(db));
    goto rtreeInit_fail;
  }

  *ppVtab = (sqlite3_vtab *)pRtree;
  return SQLITE_OK;

rtreeInit_fail:
  if( rc==SQLITE_OK ) rc = SQLITE_ERROR;
  assert( *ppVtab==0 );
  assert( pRtree->nBusy==1 );
  rtreeRelease(pRtree);
  return rc;
}


/*
** Implementation of a scalar function that decodes r-tree nodes to
** human readable strings. This can be used for debugging and analysis.
**
** The scalar function takes two arguments: (1) the number of dimensions
** to the rtree (between 1 and 5, inclusive) and (2) a blob of data containing
** an r-tree node.  For a two-dimensional r-tree structure called "rt", to
** deserialize all nodes, a statement like:
**
**   SELECT rtreenode(2, data) FROM rt_node;
**
** The human readable string takes the form of a Tcl list with one
** entry for each cell in the r-tree node. Each entry is itself a
** list, containing the 8-byte rowid/pageno followed by the
** <num-dimension>*2 coordinates.
*/
static void rtreenode(sqlite3_context *ctx, int nArg, sqlite3_value **apArg){
  RtreeNode node;
  Rtree tree;
  int ii;
  int nData;
  int errCode;
  sqlite3_str *pOut;

  UNUSED_PARAMETER(nArg);
  memset(&node, 0, sizeof(RtreeNode));
  memset(&tree, 0, sizeof(Rtree));
  tree.nDim = (u8)sqlite3_value_int(apArg[0]);
  if( tree.nDim<1 || tree.nDim>5 ) return;
  tree.nDim2 = tree.nDim*2;
  tree.nBytesPerCell = 8 + 8 * tree.nDim;
  node.zData = (u8 *)sqlite3_value_blob(apArg[1]);
  if( node.zData==0 ) return;
  nData = sqlite3_value_bytes(apArg[1]);
  if( nData<4 ) return;
  if( nData<NCELL(&node)*tree.nBytesPerCell ) return;

  pOut = sqlite3_str_new(0);
  for(ii=0; ii<NCELL(&node); ii++){
    RtreeCell cell;
    int jj;

    nodeGetCell(&tree, &node, ii, &cell);
    if( ii>0 ) sqlite3_str_append(pOut, " ", 1);
    sqlite3_str_appendf(pOut, "{%lld", cell.iRowid);
    for(jj=0; jj<tree.nDim2; jj++){
#ifndef SQLITE_RTREE_INT_ONLY
      sqlite3_str_appendf(pOut, " %g", (double)cell.aCoord[jj].f);
#else
      sqlite3_str_appendf(pOut, " %d", cell.aCoord[jj].i);
#endif
    }
    sqlite3_str_append(pOut, "}", 1);
  }
  errCode = sqlite3_str_errcode(pOut);
  sqlite3_result_text(ctx, sqlite3_str_finish(pOut), -1, sqlite3_free);
  sqlite3_result_error_code(ctx, errCode);
}

/* This routine implements an SQL function that returns the "depth" parameter
** from the front of a blob that is an r-tree node.  For example:
**
**     SELECT rtreedepth(data) FROM rt_node WHERE nodeno=1;
**
** The depth value is 0 for all nodes other than the root node, and the root
** node always has nodeno=1, so the example above is the primary use for this
** routine.  This routine is intended for testing and analysis only.
*/
static void rtreedepth(sqlite3_context *ctx, int nArg, sqlite3_value **apArg){
  UNUSED_PARAMETER(nArg);
  if( sqlite3_value_type(apArg[0])!=SQLITE_BLOB
   || sqlite3_value_bytes(apArg[0])<2

  ){
    sqlite3_result_error(ctx, "Invalid argument to rtreedepth()", -1);
  }else{
    u8 *zBlob = (u8 *)sqlite3_value_blob(apArg[0]);
    if( zBlob ){
      sqlite3_result_int(ctx, readInt16(zBlob));
    }else{
      sqlite3_result_error_nomem(ctx);
    }
  }
}

/*
** Context object passed between the various routines that make up the
** implementation of integrity-check function rtreecheck().
*/
typedef struct RtreeCheck RtreeCheck;
struct RtreeCheck {
  sqlite3 *db;                    /* Database handle */
  const char *zDb;                /* Database containing rtree table */
  const char *zTab;               /* Name of rtree table */
  int bInt;                       /* True for rtree_i32 table */
  int nDim;                       /* Number of dimensions for this rtree tbl */
  sqlite3_stmt *pGetNode;         /* Statement used to retrieve nodes */
  sqlite3_stmt *aCheckMapping[2]; /* Statements to query %_parent/%_rowid */
  int nLeaf;                      /* Number of leaf cells in table */
  int nNonLeaf;                   /* Number of non-leaf cells in table */
  int rc;                         /* Return code */
  char *zReport;                  /* Message to report */
  int nErr;                       /* Number of lines in zReport */
};

#define RTREE_CHECK_MAX_ERROR 100

/*
** Reset SQL statement pStmt. If the sqlite3_reset() call returns an error,
** and RtreeCheck.rc==SQLITE_OK, set RtreeCheck.rc to the error code.
*/
static void rtreeCheckReset(RtreeCheck *pCheck, sqlite3_stmt *pStmt){
  int rc = sqlite3_reset(pStmt);
  if( pCheck->rc==SQLITE_OK ) pCheck->rc = rc;
}

/*
** The second and subsequent arguments to this function are a format string
** and printf style arguments. This function formats the string and attempts
** to compile it as an SQL statement.
**
** If successful, a pointer to the new SQL statement is returned. Otherwise,
** NULL is returned and an error code left in RtreeCheck.rc.
*/
static sqlite3_stmt *rtreeCheckPrepare(
  RtreeCheck *pCheck,             /* RtreeCheck object */
  const char *zFmt, ...           /* Format string and trailing args */
){
  va_list ap;
  char *z;
  sqlite3_stmt *pRet = 0;

  va_start(ap, zFmt);
  z = sqlite3_vmprintf(zFmt, ap);

  if( pCheck->rc==SQLITE_OK ){
    if( z==0 ){
      pCheck->rc = SQLITE_NOMEM;
    }else{
      pCheck->rc = sqlite3_prepare_v2(pCheck->db, z, -1, &pRet, 0);
    }
  }

  sqlite3_free(z);
  va_end(ap);
  return pRet;
}
