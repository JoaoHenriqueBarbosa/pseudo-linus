
/*
** The second and subsequent arguments to this function are a printf()
** style format string and arguments. This function formats the string and
** appends it to the report being accumuated in pCheck.
*/
static void rtreeCheckAppendMsg(RtreeCheck *pCheck, const char *zFmt, ...){
  va_list ap;
  va_start(ap, zFmt);
  if( pCheck->rc==SQLITE_OK && pCheck->nErr<RTREE_CHECK_MAX_ERROR ){
    char *z = sqlite3_vmprintf(zFmt, ap);
    if( z==0 ){
      pCheck->rc = SQLITE_NOMEM;
    }else{
      pCheck->zReport = sqlite3_mprintf("%z%s%z",
          pCheck->zReport, (pCheck->zReport ? "\n" : ""), z
      );
      if( pCheck->zReport==0 ){
        pCheck->rc = SQLITE_NOMEM;
      }
    }
    pCheck->nErr++;
  }
  va_end(ap);
}

/*
** This function is a no-op if there is already an error code stored
** in the RtreeCheck object indicated by the first argument. NULL is
** returned in this case.
**
** Otherwise, the contents of rtree table node iNode are loaded from
** the database and copied into a buffer obtained from sqlite3_malloc().
** If no error occurs, a pointer to the buffer is returned and (*pnNode)
** is set to the size of the buffer in bytes.
**
** Or, if an error does occur, NULL is returned and an error code left
** in the RtreeCheck object. The final value of *pnNode is undefined in
** this case.
*/
static u8 *rtreeCheckGetNode(RtreeCheck *pCheck, i64 iNode, int *pnNode){
  u8 *pRet = 0;                   /* Return value */

  if( pCheck->rc==SQLITE_OK && pCheck->pGetNode==0 ){
    pCheck->pGetNode = rtreeCheckPrepare(pCheck,
        "SELECT data FROM %Q.'%q_node' WHERE nodeno=?",
        pCheck->zDb, pCheck->zTab
    );
  }

  if( pCheck->rc==SQLITE_OK ){
    sqlite3_bind_int64(pCheck->pGetNode, 1, iNode);
    if( sqlite3_step(pCheck->pGetNode)==SQLITE_ROW ){
      int nNode = sqlite3_column_bytes(pCheck->pGetNode, 0);
      const u8 *pNode = (const u8*)sqlite3_column_blob(pCheck->pGetNode, 0);
      pRet = sqlite3_malloc64(nNode);
      if( pRet==0 ){
        pCheck->rc = SQLITE_NOMEM;
      }else{
        memcpy(pRet, pNode, nNode);
        *pnNode = nNode;
      }
    }
    rtreeCheckReset(pCheck, pCheck->pGetNode);
    if( pCheck->rc==SQLITE_OK && pRet==0 ){
      rtreeCheckAppendMsg(pCheck, "Node %lld missing from database", iNode);
    }
  }

  return pRet;
}

/*
** This function is used to check that the %_parent (if bLeaf==0) or %_rowid
** (if bLeaf==1) table contains a specified entry. The schemas of the
** two tables are:
**
**   CREATE TABLE %_parent(nodeno INTEGER PRIMARY KEY, parentnode INTEGER)
**   CREATE TABLE %_rowid(rowid INTEGER PRIMARY KEY, nodeno INTEGER, ...)
**
** In both cases, this function checks that there exists an entry with
** IPK value iKey and the second column set to iVal.
**
*/
static void rtreeCheckMapping(
  RtreeCheck *pCheck,             /* RtreeCheck object */
  int bLeaf,                      /* True for a leaf cell, false for interior */
  i64 iKey,                       /* Key for mapping */
  i64 iVal                        /* Expected value for mapping */
){
  int rc;
  sqlite3_stmt *pStmt;
  const char *azSql[2] = {
    "SELECT parentnode FROM %Q.'%q_parent' WHERE nodeno=?1",
    "SELECT nodeno FROM %Q.'%q_rowid' WHERE rowid=?1"
  };

  assert( bLeaf==0 || bLeaf==1 );
  if( pCheck->aCheckMapping[bLeaf]==0 ){
    pCheck->aCheckMapping[bLeaf] = rtreeCheckPrepare(pCheck,
        azSql[bLeaf], pCheck->zDb, pCheck->zTab
    );
  }
  if( pCheck->rc!=SQLITE_OK ) return;

  pStmt = pCheck->aCheckMapping[bLeaf];
  sqlite3_bind_int64(pStmt, 1, iKey);
  rc = sqlite3_step(pStmt);
  if( rc==SQLITE_DONE ){
    rtreeCheckAppendMsg(pCheck, "Mapping (%lld -> %lld) missing from %s table",
        iKey, iVal, (bLeaf ? "%_rowid" : "%_parent")
    );
  }else if( rc==SQLITE_ROW ){
    i64 ii = sqlite3_column_int64(pStmt, 0);
    if( ii!=iVal ){
      rtreeCheckAppendMsg(pCheck,
          "Found (%lld -> %lld) in %s table, expected (%lld -> %lld)",
          iKey, ii, (bLeaf ? "%_rowid" : "%_parent"), iKey, iVal
      );
    }
  }
  rtreeCheckReset(pCheck, pStmt);
}

/*
** Argument pCell points to an array of coordinates stored on an rtree page.
** This function checks that the coordinates are internally consistent (no
** x1>x2 conditions) and adds an error message to the RtreeCheck object
** if they are not.
**
** Additionally, if pParent is not NULL, then it is assumed to point to
** the array of coordinates on the parent page that bound the page
** containing pCell. In this case it is also verified that the two
** sets of coordinates are mutually consistent and an error message added
** to the RtreeCheck object if they are not.
*/
static void rtreeCheckCellCoord(
  RtreeCheck *pCheck,
  i64 iNode,                      /* Node id to use in error messages */
  int iCell,                      /* Cell number to use in error messages */
  u8 *pCell,                      /* Pointer to cell coordinates */
  u8 *pParent                     /* Pointer to parent coordinates */
){
  RtreeCoord c1, c2;
  RtreeCoord p1, p2;
  int i;

  for(i=0; i<pCheck->nDim; i++){
    readCoord(&pCell[4*2*i], &c1);
    readCoord(&pCell[4*(2*i + 1)], &c2);

    /* printf("%e, %e\n", c1.u.f, c2.u.f); */
    if( pCheck->bInt ? c1.i>c2.i : c1.f>c2.f ){
      rtreeCheckAppendMsg(pCheck,
          "Dimension %d of cell %d on node %lld is corrupt", i, iCell, iNode
      );
    }

    if( pParent ){
      readCoord(&pParent[4*2*i], &p1);
      readCoord(&pParent[4*(2*i + 1)], &p2);

      if( (pCheck->bInt ? c1.i<p1.i : c1.f<p1.f)
       || (pCheck->bInt ? c2.i>p2.i : c2.f>p2.f)
      ){
        rtreeCheckAppendMsg(pCheck,
            "Dimension %d of cell %d on node %lld is corrupt relative to parent"
            , i, iCell, iNode
        );
      }
    }
  }
}

/*
** Run rtreecheck() checks on node iNode, which is at depth iDepth within
** the r-tree structure. Argument aParent points to the array of coordinates
** that bound node iNode on the parent node.
**
** If any problems are discovered, an error message is appended to the
** report accumulated in the RtreeCheck object.
*/
static void rtreeCheckNode(
  RtreeCheck *pCheck,
  int iDepth,                     /* Depth of iNode (0==leaf) */
  u8 *aParent,                    /* Buffer containing parent coords */
  i64 iNode                       /* Node to check */
){
  u8 *aNode = 0;
  int nNode = 0;

  assert( iNode==1 || aParent!=0 );
  assert( pCheck->nDim>0 );

  aNode = rtreeCheckGetNode(pCheck, iNode, &nNode);
  if( aNode ){
    if( nNode<4 ){
      rtreeCheckAppendMsg(pCheck,
          "Node %lld is too small (%d bytes)", iNode, nNode
      );
    }else{
      int nCell;                  /* Number of cells on page */
      int i;                      /* Used to iterate through cells */
      if( aParent==0 ){
        iDepth = readInt16(aNode);
        if( iDepth>RTREE_MAX_DEPTH ){
          rtreeCheckAppendMsg(pCheck, "Rtree depth out of range (%d)", iDepth);
          sqlite3_free(aNode);
          return;
        }
      }
      nCell = readInt16(&aNode[2]);
      if( (4 + nCell*(8 + pCheck->nDim*2*4))>nNode ){
        rtreeCheckAppendMsg(pCheck,
            "Node %lld is too small for cell count of %d (%d bytes)",
            iNode, nCell, nNode
        );
      }else{
        for(i=0; i<nCell; i++){
          u8 *pCell = &aNode[4 + i*(8 + pCheck->nDim*2*4)];
          i64 iVal = readInt64(pCell);
          rtreeCheckCellCoord(pCheck, iNode, i, &pCell[8], aParent);

          if( iDepth>0 ){
            rtreeCheckMapping(pCheck, 0, iVal, iNode);
            rtreeCheckNode(pCheck, iDepth-1, &pCell[8], iVal);
            pCheck->nNonLeaf++;
          }else{
            rtreeCheckMapping(pCheck, 1, iVal, iNode);
            pCheck->nLeaf++;
          }
        }
      }
    }
    sqlite3_free(aNode);
  }
}

/*
** The second argument to this function must be either "_rowid" or
** "_parent". This function checks that the number of entries in the
** %_rowid or %_parent table is exactly nExpect. If not, it adds
** an error message to the report in the RtreeCheck object indicated
** by the first argument.
*/
static void rtreeCheckCount(RtreeCheck *pCheck, const char *zTbl, i64 nExpect){
  if( pCheck->rc==SQLITE_OK ){
    sqlite3_stmt *pCount;
    pCount = rtreeCheckPrepare(pCheck, "SELECT count(*) FROM %Q.'%q%s'",
        pCheck->zDb, pCheck->zTab, zTbl
    );
    if( pCount ){
      if( sqlite3_step(pCount)==SQLITE_ROW ){
        i64 nActual = sqlite3_column_int64(pCount, 0);
        if( nActual!=nExpect ){
          rtreeCheckAppendMsg(pCheck, "Wrong number of entries in %%%s table"
              " - expected %lld, actual %lld" , zTbl, nExpect, nActual
          );
        }
      }
      pCheck->rc = sqlite3_finalize(pCount);
    }
  }
}

/*
** This function does the bulk of the work for the rtree integrity-check.
** It is called by rtreecheck(), which is the SQL function implementation.
*/
static int rtreeCheckTable(
  sqlite3 *db,                    /* Database handle to access db through */
  const char *zDb,                /* Name of db ("main", "temp" etc.) */
  const char *zTab,               /* Name of rtree table to check */
  char **pzReport                 /* OUT: sqlite3_malloc'd report text */
){
  RtreeCheck check;               /* Common context for various routines */
  sqlite3_stmt *pStmt = 0;        /* Used to find column count of rtree table */
  int nAux = 0;                   /* Number of extra columns. */

  /* Initialize the context object */
  memset(&check, 0, sizeof(check));
  check.db = db;
  check.zDb = zDb;
  check.zTab = zTab;

  /* Find the number of auxiliary columns */
  pStmt = rtreeCheckPrepare(&check, "SELECT * FROM %Q.'%q_rowid'", zDb, zTab);
  if( pStmt ){
    nAux = sqlite3_column_count(pStmt) - 2;
    sqlite3_finalize(pStmt);
  }else
  if( check.rc!=SQLITE_NOMEM ){
    check.rc = SQLITE_OK;
  }

  /* Find number of dimensions in the rtree table. */
  pStmt = rtreeCheckPrepare(&check, "SELECT * FROM %Q.%Q", zDb, zTab);
  if( pStmt ){
    int rc;
    check.nDim = (sqlite3_column_count(pStmt) - 1 - nAux) / 2;
    if( check.nDim<1 ){
      rtreeCheckAppendMsg(&check, "Schema corrupt or not an rtree");
    }else if( SQLITE_ROW==sqlite3_step(pStmt) ){
      check.bInt = (sqlite3_column_type(pStmt, 1)==SQLITE_INTEGER);
    }
    rc = sqlite3_finalize(pStmt);
    if( rc!=SQLITE_CORRUPT ) check.rc = rc;
  }

  /* Do the actual integrity-check */
  if( check.nDim>=1 ){
    if( check.rc==SQLITE_OK ){
      rtreeCheckNode(&check, 0, 0, 1);
    }
    rtreeCheckCount(&check, "_rowid", check.nLeaf);
    rtreeCheckCount(&check, "_parent", check.nNonLeaf);
  }

  /* Finalize SQL statements used by the integrity-check */
  sqlite3_finalize(check.pGetNode);
  sqlite3_finalize(check.aCheckMapping[0]);
  sqlite3_finalize(check.aCheckMapping[1]);

  *pzReport = check.zReport;
  return check.rc;
}

/*
** Implementation of the xIntegrity method for Rtree.
*/
static int rtreeIntegrity(
  sqlite3_vtab *pVtab,   /* The virtual table to check */
  const char *zSchema,   /* Schema in which the virtual table lives */
  const char *zName,     /* Name of the virtual table */
  int isQuick,           /* True for a quick_check */
  char **pzErr           /* Write results here */
){
  Rtree *pRtree = (Rtree*)pVtab;
  int rc;
  assert( pzErr!=0 && *pzErr==0 );
  UNUSED_PARAMETER(zSchema);
  UNUSED_PARAMETER(zName);
  UNUSED_PARAMETER(isQuick);
  rc = rtreeCheckTable(pRtree->db, pRtree->zDb, pRtree->zName, pzErr);
  if( rc==SQLITE_OK && *pzErr ){
    *pzErr = sqlite3_mprintf("In RTree %s.%s:\n%z",
                 pRtree->zDb, pRtree->zName, *pzErr);
    if( (*pzErr)==0 ) rc = SQLITE_NOMEM;
  }
  return rc;
}
