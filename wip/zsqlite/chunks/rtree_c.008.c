


/*
** The xUpdate method for rtree module virtual tables.
*/
static int rtreeUpdate(
  sqlite3_vtab *pVtab,
  int nData,
  sqlite3_value **aData,
  sqlite_int64 *pRowid
){
  Rtree *pRtree = (Rtree *)pVtab;
  int rc = SQLITE_OK;
  RtreeCell cell;                 /* New cell to insert if nData>1 */
  int bHaveRowid = 0;             /* Set to 1 after new rowid is determined */

  if( pRtree->nNodeRef ){
    /* Unable to write to the btree while another cursor is reading from it,
    ** since the write might do a rebalance which would disrupt the read
    ** cursor. */
    return SQLITE_LOCKED_VTAB;
  }
  rtreeReference(pRtree);
  assert(nData>=1);

  memset(&cell, 0, sizeof(cell));

  /* Constraint handling. A write operation on an r-tree table may return
  ** SQLITE_CONSTRAINT for two reasons:
  **
  **   1. A duplicate rowid value, or
  **   2. The supplied data violates the "x2>=x1" constraint.
  **
  ** In the first case, if the conflict-handling mode is REPLACE, then
  ** the conflicting row can be removed before proceeding. In the second
  ** case, SQLITE_CONSTRAINT must be returned regardless of the
  ** conflict-handling mode specified by the user.
  */
  if( nData>1 ){
    int ii;
    int nn = nData - 4;

    if( nn > pRtree->nDim2 ) nn = pRtree->nDim2;
    /* Populate the cell.aCoord[] array. The first coordinate is aData[3].
    **
    ** NB: nData can only be less than nDim*2+3 if the rtree is mis-declared
    ** with "column" that are interpreted as table constraints.
    ** Example:  CREATE VIRTUAL TABLE bad USING rtree(x,y,CHECK(y>5));
    ** This problem was discovered after years of use, so we silently ignore
    ** these kinds of misdeclared tables to avoid breaking any legacy.
    */

#ifndef SQLITE_RTREE_INT_ONLY
    if( pRtree->eCoordType==RTREE_COORD_REAL32 ){
      for(ii=0; ii<nn; ii+=2){
        cell.aCoord[ii].f = rtreeValueDown(aData[ii+3]);
        cell.aCoord[ii+1].f = rtreeValueUp(aData[ii+4]);
        if( cell.aCoord[ii].f>cell.aCoord[ii+1].f ){
          rc = rtreeConstraintError(pRtree, ii+1);
          goto constraint;
        }
      }
    }else
#endif
    {
      for(ii=0; ii<nn; ii+=2){
        cell.aCoord[ii].i = sqlite3_value_int(aData[ii+3]);
        cell.aCoord[ii+1].i = sqlite3_value_int(aData[ii+4]);
        if( cell.aCoord[ii].i>cell.aCoord[ii+1].i ){
          rc = rtreeConstraintError(pRtree, ii+1);
          goto constraint;
        }
      }
    }

    /* If a rowid value was supplied, check if it is already present in
    ** the table. If so, the constraint has failed. */
    if( sqlite3_value_type(aData[2])!=SQLITE_NULL ){
      cell.iRowid = sqlite3_value_int64(aData[2]);
      if( sqlite3_value_type(aData[0])==SQLITE_NULL
       || sqlite3_value_int64(aData[0])!=cell.iRowid
      ){
        int steprc;
        sqlite3_bind_int64(pRtree->pReadRowid, 1, cell.iRowid);
        steprc = sqlite3_step(pRtree->pReadRowid);
        rc = sqlite3_reset(pRtree->pReadRowid);
        if( SQLITE_ROW==steprc ){
          if( sqlite3_vtab_on_conflict(pRtree->db)==SQLITE_REPLACE ){
            rc = rtreeDeleteRowid(pRtree, cell.iRowid);
          }else{
            rc = rtreeConstraintError(pRtree, 0);
            goto constraint;
          }
        }
      }
      bHaveRowid = 1;
    }
  }

  /* If aData[0] is not an SQL NULL value, it is the rowid of a
  ** record to delete from the r-tree table. The following block does
  ** just that.
  */
  if( sqlite3_value_type(aData[0])!=SQLITE_NULL ){
    rc = rtreeDeleteRowid(pRtree, sqlite3_value_int64(aData[0]));
  }

  /* If the aData[] array contains more than one element, elements
  ** (aData[2]..aData[argc-1]) contain a new record to insert into
  ** the r-tree structure.
  */
  if( rc==SQLITE_OK && nData>1 ){
    /* Insert the new record into the r-tree */
    RtreeNode *pLeaf = 0;

    /* Figure out the rowid of the new row. */
    if( bHaveRowid==0 ){
      rc = rtreeNewRowid(pRtree, &cell.iRowid);
    }
    *pRowid = cell.iRowid;

    if( rc==SQLITE_OK ){
      rc = ChooseLeaf(pRtree, &cell, 0, &pLeaf);
    }
    if( rc==SQLITE_OK ){
      int rc2;
      rc = rtreeInsertCell(pRtree, pLeaf, &cell, 0);
      rc2 = nodeRelease(pRtree, pLeaf);
      if( rc==SQLITE_OK ){
        rc = rc2;
      }
    }
    if( rc==SQLITE_OK && pRtree->nAux ){
      sqlite3_stmt *pUp = pRtree->pWriteAux;
      int jj;
      sqlite3_bind_int64(pUp, 1, *pRowid);
      for(jj=0; jj<pRtree->nAux; jj++){
        sqlite3_bind_value(pUp, jj+2, aData[pRtree->nDim2+3+jj]);
      }
      sqlite3_step(pUp);
      rc = sqlite3_reset(pUp);
    }
  }

constraint:
  rtreeRelease(pRtree);
  return rc;
}

/*
** Called when a transaction starts.
*/
static int rtreeBeginTransaction(sqlite3_vtab *pVtab){
  Rtree *pRtree = (Rtree *)pVtab;
  assert( pRtree->inWrTrans==0 );
  pRtree->inWrTrans = 1;
  return SQLITE_OK;
}

/*
** Called when a transaction completes (either by COMMIT or ROLLBACK).
** The sqlite3_blob object should be released at this point.
*/
static int rtreeEndTransaction(sqlite3_vtab *pVtab){
  Rtree *pRtree = (Rtree *)pVtab;
  pRtree->inWrTrans = 0;
  nodeBlobReset(pRtree);
  return SQLITE_OK;
}
static int rtreeRollback(sqlite3_vtab *pVtab){
  return rtreeEndTransaction(pVtab);
}

/*
** The xRename method for rtree module virtual tables.
*/
static int rtreeRename(sqlite3_vtab *pVtab, const char *zNewName){
  Rtree *pRtree = (Rtree *)pVtab;
  int rc = SQLITE_NOMEM;
  char *zSql = sqlite3_mprintf(
    "ALTER TABLE %Q.'%q_node'   RENAME TO \"%w_node\";"
    "ALTER TABLE %Q.'%q_parent' RENAME TO \"%w_parent\";"
    "ALTER TABLE %Q.'%q_rowid'  RENAME TO \"%w_rowid\";"
    , pRtree->zDb, pRtree->zName, zNewName
    , pRtree->zDb, pRtree->zName, zNewName
    , pRtree->zDb, pRtree->zName, zNewName
  );
  if( zSql ){
    nodeBlobReset(pRtree);
    rc = sqlite3_exec(pRtree->db, zSql, 0, 0, 0);
    sqlite3_free(zSql);
  }
  return rc;
}

/*
** The xSavepoint method.
**
** This module does not need to do anything to support savepoints. However,
** it uses this hook to close any open blob handle. This is done because a
** DROP TABLE command - which fortunately always opens a savepoint - cannot
** succeed if there are any open blob handles. i.e. if the blob handle were
** not closed here, the following would fail:
**
**   BEGIN;
**     INSERT INTO rtree...
**     DROP TABLE <tablename>;    -- Would fail with SQLITE_LOCKED
**   COMMIT;
*/
static int rtreeSavepoint(sqlite3_vtab *pVtab, int iSavepoint){
  Rtree *pRtree = (Rtree *)pVtab;
  u8 iwt = pRtree->inWrTrans;
  UNUSED_PARAMETER(iSavepoint);
  pRtree->inWrTrans = 0;
  nodeBlobReset(pRtree);
  pRtree->inWrTrans = iwt;
  return SQLITE_OK;
}

/*
** This function populates the pRtree->nRowEst variable with an estimate
** of the number of rows in the virtual table. If possible, this is based
** on sqlite_stat1 data. Otherwise, use RTREE_DEFAULT_ROWEST.
*/
static int rtreeQueryStat1(sqlite3 *db, Rtree *pRtree){
  const char *zFmt = "SELECT stat FROM %Q.sqlite_stat1 WHERE tbl = '%q_rowid'";
  char *zSql;
  sqlite3_stmt *p;
  int rc;
  i64 nRow = RTREE_MIN_ROWEST;

  rc = sqlite3_table_column_metadata(
      db, pRtree->zDb, "sqlite_stat1",0,0,0,0,0,0
  );
  if( rc!=SQLITE_OK ){
    pRtree->nRowEst = RTREE_DEFAULT_ROWEST;
    return rc==SQLITE_ERROR ? SQLITE_OK : rc;
  }
  zSql = sqlite3_mprintf(zFmt, pRtree->zDb, pRtree->zName);
  if( zSql==0 ){
    rc = SQLITE_NOMEM;
  }else{
    rc = sqlite3_prepare_v2(db, zSql, -1, &p, 0);
    if( rc==SQLITE_OK ){
      if( sqlite3_step(p)==SQLITE_ROW ) nRow = sqlite3_column_int64(p, 0);
      rc = sqlite3_finalize(p);
    }
    sqlite3_free(zSql);
  }
  pRtree->nRowEst = MAX(nRow, RTREE_MIN_ROWEST);
  return rc;
}


/*
** Return true if zName is the extension on one of the shadow tables used
** by this module.
*/
static int rtreeShadowName(const char *zName){
  static const char *azName[] = {
    "node", "parent", "rowid"
  };
  unsigned int i;
  for(i=0; i<sizeof(azName)/sizeof(azName[0]); i++){
    if( sqlite3_stricmp(zName, azName[i])==0 ) return 1;
  }
  return 0;
}

/* Forward declaration */
static int rtreeIntegrity(sqlite3_vtab*, const char*, const char*, int, char**);

static sqlite3_module rtreeModule = {
  4,                          /* iVersion */
  rtreeCreate,                /* xCreate - create a table */
  rtreeConnect,               /* xConnect - connect to an existing table */
  rtreeBestIndex,             /* xBestIndex - Determine search strategy */
  rtreeDisconnect,            /* xDisconnect - Disconnect from a table */
  rtreeDestroy,               /* xDestroy - Drop a table */
  rtreeOpen,                  /* xOpen - open a cursor */
  rtreeClose,                 /* xClose - close a cursor */
  rtreeFilter,                /* xFilter - configure scan constraints */
  rtreeNext,                  /* xNext - advance a cursor */
  rtreeEof,                   /* xEof */
  rtreeColumn,                /* xColumn - read data */
  rtreeRowid,                 /* xRowid - read data */
  rtreeUpdate,                /* xUpdate - write data */
  rtreeBeginTransaction,      /* xBegin - begin transaction */
  rtreeEndTransaction,        /* xSync - sync transaction */
  rtreeEndTransaction,        /* xCommit - commit transaction */
  rtreeRollback,              /* xRollback - rollback transaction */
  0,                          /* xFindFunction - function overloading */
  rtreeRename,                /* xRename - rename the table */
  rtreeSavepoint,             /* xSavepoint */
  0,                          /* xRelease */
  0,                          /* xRollbackTo */
  rtreeShadowName,            /* xShadowName */
  rtreeIntegrity              /* xIntegrity */
};

static int rtreeSqlInit(
  Rtree *pRtree,
  sqlite3 *db,
  const char *zDb,
  const char *zPrefix,
  int isCreate
){
  int rc = SQLITE_OK;

  #define N_STATEMENT 8
  static const char *azSql[N_STATEMENT] = {
    /* Write the xxx_node table */
    "INSERT OR REPLACE INTO '%q'.'%q_node' VALUES(?1, ?2)",
    "DELETE FROM '%q'.'%q_node' WHERE nodeno = ?1",

    /* Read and write the xxx_rowid table */
    "SELECT nodeno FROM '%q'.'%q_rowid' WHERE rowid = ?1",
    "INSERT OR REPLACE INTO '%q'.'%q_rowid' VALUES(?1, ?2)",
    "DELETE FROM '%q'.'%q_rowid' WHERE rowid = ?1",

    /* Read and write the xxx_parent table */
    "SELECT parentnode FROM '%q'.'%q_parent' WHERE nodeno = ?1",
    "INSERT OR REPLACE INTO '%q'.'%q_parent' VALUES(?1, ?2)",
    "DELETE FROM '%q'.'%q_parent' WHERE nodeno = ?1"
  };
  sqlite3_stmt **appStmt[N_STATEMENT];
  int i;
  const int f = SQLITE_PREPARE_PERSISTENT|SQLITE_PREPARE_NO_VTAB;

  pRtree->db = db;

  if( isCreate ){
    char *zCreate;
    sqlite3_str *p = sqlite3_str_new(db);
    int ii;
    sqlite3_str_appendf(p,
       "CREATE TABLE \"%w\".\"%w_rowid\"(rowid INTEGER PRIMARY KEY,nodeno",
       zDb, zPrefix);
    for(ii=0; ii<pRtree->nAux; ii++){
      sqlite3_str_appendf(p,",a%d",ii);
    }
    sqlite3_str_appendf(p,
      ");CREATE TABLE \"%w\".\"%w_node\"(nodeno INTEGER PRIMARY KEY,data);",
      zDb, zPrefix);
    sqlite3_str_appendf(p,
    "CREATE TABLE \"%w\".\"%w_parent\"(nodeno INTEGER PRIMARY KEY,parentnode);",
      zDb, zPrefix);
    sqlite3_str_appendf(p,
       "INSERT INTO \"%w\".\"%w_node\"VALUES(1,zeroblob(%d))",
       zDb, zPrefix, pRtree->iNodeSize);
    zCreate = sqlite3_str_finish(p);
    if( !zCreate ){
      return SQLITE_NOMEM;
    }
    rc = sqlite3_exec(db, zCreate, 0, 0, 0);
    sqlite3_free(zCreate);
    if( rc!=SQLITE_OK ){
      return rc;
    }
  }

  appStmt[0] = &pRtree->pWriteNode;
  appStmt[1] = &pRtree->pDeleteNode;
  appStmt[2] = &pRtree->pReadRowid;
  appStmt[3] = &pRtree->pWriteRowid;
  appStmt[4] = &pRtree->pDeleteRowid;
  appStmt[5] = &pRtree->pReadParent;
  appStmt[6] = &pRtree->pWriteParent;
  appStmt[7] = &pRtree->pDeleteParent;

  rc = rtreeQueryStat1(db, pRtree);
  for(i=0; i<N_STATEMENT && rc==SQLITE_OK; i++){
    char *zSql;
    const char *zFormat;
    if( i!=3 || pRtree->nAux==0 ){
       zFormat = azSql[i];
    }else {
       /* An UPSERT is very slightly slower than REPLACE, but it is needed
       ** if there are auxiliary columns */
       zFormat = "INSERT INTO\"%w\".\"%w_rowid\"(rowid,nodeno)VALUES(?1,?2)"
                  "ON CONFLICT(rowid)DO UPDATE SET nodeno=excluded.nodeno";
    }
    zSql = sqlite3_mprintf(zFormat, zDb, zPrefix);
    if( zSql ){
      rc = sqlite3_prepare_v3(db, zSql, -1, f, appStmt[i], 0);
    }else{
      rc = SQLITE_NOMEM;
    }
    sqlite3_free(zSql);
  }
  if( pRtree->nAux && rc!=SQLITE_NOMEM ){
    pRtree->zReadAuxSql = sqlite3_mprintf(
       "SELECT * FROM \"%w\".\"%w_rowid\" WHERE rowid=?1",
       zDb, zPrefix);
    if( pRtree->zReadAuxSql==0 ){
      rc = SQLITE_NOMEM;
    }else{
      sqlite3_str *p = sqlite3_str_new(db);
      int ii;
      char *zSql;
      sqlite3_str_appendf(p, "UPDATE \"%w\".\"%w_rowid\"SET ", zDb, zPrefix);
      for(ii=0; ii<pRtree->nAux; ii++){
        if( ii ) sqlite3_str_append(p, ",", 1);
#ifdef SQLITE_ENABLE_GEOPOLY
        if( ii<pRtree->nAuxNotNull ){
          sqlite3_str_appendf(p,"a%d=coalesce(?%d,a%d)",ii,ii+2,ii);
        }else
#endif
        {
          sqlite3_str_appendf(p,"a%d=?%d",ii,ii+2);
        }
      }
      sqlite3_str_appendf(p, " WHERE rowid=?1");
      zSql = sqlite3_str_finish(p);
      if( zSql==0 ){
        rc = SQLITE_NOMEM;
      }else{
        rc = sqlite3_prepare_v3(db, zSql, -1, f, &pRtree->pWriteAux, 0);
        sqlite3_free(zSql);
      }
    }
  }

  return rc;
}
