
/*
** Usage:
**
**   rtreecheck(<rtree-table>);
**   rtreecheck(<database>, <rtree-table>);
**
** Invoking this SQL function runs an integrity-check on the named rtree
** table. The integrity-check verifies the following:
**
**   1. For each cell in the r-tree structure (%_node table), that:
**
**       a) for each dimension, (coord1 <= coord2).
**
**       b) unless the cell is on the root node, that the cell is bounded
**          by the parent cell on the parent node.
**
**       c) for leaf nodes, that there is an entry in the %_rowid
**          table corresponding to the cell's rowid value that
**          points to the correct node.
**
**       d) for cells on non-leaf nodes, that there is an entry in the
**          %_parent table mapping from the cell's child node to the
**          node that it resides on.
**
**   2. That there are the same number of entries in the %_rowid table
**      as there are leaf cells in the r-tree structure, and that there
**      is a leaf cell that corresponds to each entry in the %_rowid table.
**
**   3. That there are the same number of entries in the %_parent table
**      as there are non-leaf cells in the r-tree structure, and that
**      there is a non-leaf cell that corresponds to each entry in the
**      %_parent table.
*/
static void rtreecheck(
  sqlite3_context *ctx,
  int nArg,
  sqlite3_value **apArg
){
  if( nArg!=1 && nArg!=2 ){
    sqlite3_result_error(ctx,
        "wrong number of arguments to function rtreecheck()", -1
    );
  }else{
    int rc;
    char *zReport = 0;
    const char *zDb = (const char*)sqlite3_value_text(apArg[0]);
    const char *zTab;
    if( nArg==1 ){
      zTab = zDb;
      zDb = "main";
    }else{
      zTab = (const char*)sqlite3_value_text(apArg[1]);
    }
    rc = rtreeCheckTable(sqlite3_context_db_handle(ctx), zDb, zTab, &zReport);
    if( rc==SQLITE_OK ){
      sqlite3_result_text(ctx, zReport ? zReport : "ok", -1, SQLITE_TRANSIENT);
    }else{
      sqlite3_result_error_code(ctx, rc);
    }
    sqlite3_free(zReport);
  }
}

/* Conditionally include the geopoly code */
#ifdef SQLITE_ENABLE_GEOPOLY
/************** Include geopoly.c in the middle of rtree.c *******************/
#endif

/*
** Register the r-tree module with database handle db. This creates the
** virtual table module "rtree" and the debugging/analysis scalar
** function "rtreenode".
*/
SQLITE_PRIVATE int sqlite3RtreeInit(sqlite3 *db){
  const int utf8 = SQLITE_UTF8;
  int rc;

  rc = sqlite3_create_function(db, "rtreenode", 2, utf8, 0, rtreenode, 0, 0);
  if( rc==SQLITE_OK ){
    rc = sqlite3_create_function(db, "rtreedepth", 1, utf8, 0,rtreedepth, 0, 0);
  }
  if( rc==SQLITE_OK ){
    rc = sqlite3_create_function(db, "rtreecheck", -1, utf8, 0,rtreecheck, 0,0);
  }
  if( rc==SQLITE_OK ){
#ifdef SQLITE_RTREE_INT_ONLY
    void *c = (void *)RTREE_COORD_INT32;
#else
    void *c = (void *)RTREE_COORD_REAL32;
#endif
    rc = sqlite3_create_module_v2(db, "rtree", &rtreeModule, c, 0);
  }
  if( rc==SQLITE_OK ){
    void *c = (void *)RTREE_COORD_INT32;
    rc = sqlite3_create_module_v2(db, "rtree_i32", &rtreeModule, c, 0);
  }
#ifdef SQLITE_ENABLE_GEOPOLY
  if( rc==SQLITE_OK ){
    rc = sqlite3_geopoly_init(db);
  }
#endif

  return rc;
}

/*
** This routine deletes the RtreeGeomCallback object that was attached
** one of the SQL functions create by sqlite3_rtree_geometry_callback()
** or sqlite3_rtree_query_callback().  In other words, this routine is the
** destructor for an RtreeGeomCallback objecct.  This routine is called when
** the corresponding SQL function is deleted.
*/
static void rtreeFreeCallback(void *p){
  RtreeGeomCallback *pInfo = (RtreeGeomCallback*)p;
  if( pInfo->xDestructor ) pInfo->xDestructor(pInfo->pContext);
  sqlite3_free(p);
}

/*
** This routine frees the BLOB that is returned by geomCallback().
*/
static void rtreeMatchArgFree(void *pArg){
  int i;
  RtreeMatchArg *p = (RtreeMatchArg*)pArg;
  for(i=0; i<p->nParam; i++){
    sqlite3_value_free(p->apSqlParam[i]);
  }
  sqlite3_free(p);
}

/*
** Each call to sqlite3_rtree_geometry_callback() or
** sqlite3_rtree_query_callback() creates an ordinary SQLite
** scalar function that is implemented by this routine.
**
** All this function does is construct an RtreeMatchArg object that
** contains the geometry-checking callback routines and a list of
** parameters to this function, then return that RtreeMatchArg object
** as a BLOB.
**
** The R-Tree MATCH operator will read the returned BLOB, deserialize
** the RtreeMatchArg object, and use the RtreeMatchArg object to figure
** out which elements of the R-Tree should be returned by the query.
*/
static void geomCallback(sqlite3_context *ctx, int nArg, sqlite3_value **aArg){
  RtreeGeomCallback *pGeomCtx = (RtreeGeomCallback *)sqlite3_user_data(ctx);
  RtreeMatchArg *pBlob;
  sqlite3_int64 nBlob;
  int memErr = 0;

  nBlob = sizeof(RtreeMatchArg) + (nArg-1)*sizeof(RtreeDValue)
           + nArg*sizeof(sqlite3_value*);
  pBlob = (RtreeMatchArg *)sqlite3_malloc64(nBlob);
  if( !pBlob ){
    sqlite3_result_error_nomem(ctx);
  }else{
    int i;
    pBlob->iSize = nBlob;
    pBlob->cb = pGeomCtx[0];
    pBlob->apSqlParam = (sqlite3_value**)&pBlob->aParam[nArg];
    pBlob->nParam = nArg;
    for(i=0; i<nArg; i++){
      pBlob->apSqlParam[i] = sqlite3_value_dup(aArg[i]);
      if( pBlob->apSqlParam[i]==0 ) memErr = 1;
#ifdef SQLITE_RTREE_INT_ONLY
      pBlob->aParam[i] = sqlite3_value_int64(aArg[i]);
#else
      pBlob->aParam[i] = sqlite3_value_double(aArg[i]);
#endif
    }
    if( memErr ){
      sqlite3_result_error_nomem(ctx);
      rtreeMatchArgFree(pBlob);
    }else{
      sqlite3_result_pointer(ctx, pBlob, "RtreeMatchArg", rtreeMatchArgFree);
    }
  }
}

/*
** Register a new geometry function for use with the r-tree MATCH operator.
*/
SQLITE_API int sqlite3_rtree_geometry_callback(
  sqlite3 *db,                  /* Register SQL function on this connection */
  const char *zGeom,            /* Name of the new SQL function */
  int (*xGeom)(sqlite3_rtree_geometry*,int,RtreeDValue*,int*), /* Callback */
  void *pContext                /* Extra data associated with the callback */
){
  RtreeGeomCallback *pGeomCtx;      /* Context object for new user-function */

  /* Allocate and populate the context object. */
  pGeomCtx = (RtreeGeomCallback *)sqlite3_malloc(sizeof(RtreeGeomCallback));
  if( !pGeomCtx ) return SQLITE_NOMEM;
  pGeomCtx->xGeom = xGeom;
  pGeomCtx->xQueryFunc = 0;
  pGeomCtx->xDestructor = 0;
  pGeomCtx->pContext = pContext;
  return sqlite3_create_function_v2(db, zGeom, -1, SQLITE_ANY,
      (void *)pGeomCtx, geomCallback, 0, 0, rtreeFreeCallback
  );
}

/*
** Register a new 2nd-generation geometry function for use with the
** r-tree MATCH operator.
*/
SQLITE_API int sqlite3_rtree_query_callback(
  sqlite3 *db,                 /* Register SQL function on this connection */
  const char *zQueryFunc,      /* Name of new SQL function */
  int (*xQueryFunc)(sqlite3_rtree_query_info*), /* Callback */
  void *pContext,              /* Extra data passed into the callback */
  void (*xDestructor)(void*)   /* Destructor for the extra data */
){
  RtreeGeomCallback *pGeomCtx;      /* Context object for new user-function */

  /* Allocate and populate the context object. */
  pGeomCtx = (RtreeGeomCallback *)sqlite3_malloc(sizeof(RtreeGeomCallback));
  if( !pGeomCtx ){
    if( xDestructor ) xDestructor(pContext);
    return SQLITE_NOMEM;
  }
  pGeomCtx->xGeom = 0;
  pGeomCtx->xQueryFunc = xQueryFunc;
  pGeomCtx->xDestructor = xDestructor;
  pGeomCtx->pContext = pContext;
  return sqlite3_create_function_v2(db, zQueryFunc, -1, SQLITE_ANY,
      (void *)pGeomCtx, geomCallback, 0, 0, rtreeFreeCallback
  );
}

#if !SQLITE_CORE
#ifdef _WIN32
__declspec(dllexport)
#endif
SQLITE_API int sqlite3_rtree_init(
  sqlite3 *db,
  char **pzErrMsg,
  const sqlite3_api_routines *pApi
){
  SQLITE_EXTENSION_INIT2(pApi)
  return sqlite3RtreeInit(db);
}
#endif

#endif

