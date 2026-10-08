

/*
** GEOPOLY virtual table module xColumn method.
*/
static int geopolyColumn(sqlite3_vtab_cursor *cur, sqlite3_context *ctx, int i){
  Rtree *pRtree = (Rtree *)cur->pVtab;
  RtreeCursor *pCsr = (RtreeCursor *)cur;
  RtreeSearchPoint *p = rtreeSearchPointFirst(pCsr);
  int rc = SQLITE_OK;
  RtreeNode *pNode = rtreeNodeOfFirstSearchPoint(pCsr, &rc);

  if( rc ) return rc;
  if( p==0 ) return SQLITE_OK;
  if( i==0 && sqlite3_vtab_nochange(ctx) ) return SQLITE_OK;
  if( i<=pRtree->nAux ){
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
    sqlite3_result_value(ctx, sqlite3_column_value(pCsr->pReadAux, i+2));
  }
  return SQLITE_OK;
}


/*
** The xUpdate method for GEOPOLY module virtual tables.
**
** For DELETE:
**
**     argv[0] = the rowid to be deleted
**
** For INSERT:
**
**     argv[0] = SQL NULL
**     argv[1] = rowid to insert, or an SQL NULL to select automatically
**     argv[2] = _shape column
**     argv[3] = first application-defined column....
**
** For UPDATE:
**
**     argv[0] = rowid to modify.  Never NULL
**     argv[1] = rowid after the change.  Never NULL
**     argv[2] = new value for _shape
**     argv[3] = new value for first application-defined column....
*/
static int geopolyUpdate(
  sqlite3_vtab *pVtab,
  int nData,
  sqlite3_value **aData,
  sqlite_int64 *pRowid
){
  Rtree *pRtree = (Rtree *)pVtab;
  int rc = SQLITE_OK;
  RtreeCell cell;                 /* New cell to insert if nData>1 */
  i64 oldRowid;                   /* The old rowid */
  int oldRowidValid;              /* True if oldRowid is valid */
  i64 newRowid;                   /* The new rowid */
  int newRowidValid;              /* True if newRowid is valid */
  int coordChange = 0;            /* Change in coordinates */

  if( pRtree->nNodeRef ){
    /* Unable to write to the btree while another cursor is reading from it,
    ** since the write might do a rebalance which would disrupt the read
    ** cursor. */
    return SQLITE_LOCKED_VTAB;
  }
  rtreeReference(pRtree);
  assert(nData>=1);

  oldRowidValid = sqlite3_value_type(aData[0])!=SQLITE_NULL;;
  oldRowid = oldRowidValid ? sqlite3_value_int64(aData[0]) : 0;
  newRowidValid = nData>1 && sqlite3_value_type(aData[1])!=SQLITE_NULL;
  newRowid = newRowidValid ? sqlite3_value_int64(aData[1]) : 0;
  cell.iRowid = newRowid;

  if( nData>1                                 /* not a DELETE */
   && (!oldRowidValid                         /* INSERT */
        || !sqlite3_value_nochange(aData[2])  /* UPDATE _shape */
        || oldRowid!=newRowid)                /* Rowid change */
  ){
    assert( aData[2]!=0 );
    geopolyBBox(0, aData[2], cell.aCoord, &rc);
    if( rc ){
      if( rc==SQLITE_ERROR ){
        pVtab->zErrMsg =
          sqlite3_mprintf("_shape does not contain a valid polygon");
      }
      goto geopoly_update_end;
    }
    coordChange = 1;

    /* If a rowid value was supplied, check if it is already present in
    ** the table. If so, the constraint has failed. */
    if( newRowidValid && (!oldRowidValid || oldRowid!=newRowid) ){
      int steprc;
      sqlite3_bind_int64(pRtree->pReadRowid, 1, cell.iRowid);
      steprc = sqlite3_step(pRtree->pReadRowid);
      rc = sqlite3_reset(pRtree->pReadRowid);
      if( SQLITE_ROW==steprc ){
        if( sqlite3_vtab_on_conflict(pRtree->db)==SQLITE_REPLACE ){
          rc = rtreeDeleteRowid(pRtree, cell.iRowid);
        }else{
          rc = rtreeConstraintError(pRtree, 0);
        }
      }
    }
  }

  /* If aData[0] is not an SQL NULL value, it is the rowid of a
  ** record to delete from the r-tree table. The following block does
  ** just that.
  */
  if( rc==SQLITE_OK && (nData==1 || (coordChange && oldRowidValid)) ){
    rc = rtreeDeleteRowid(pRtree, oldRowid);
  }

  /* If the aData[] array contains more than one element, elements
  ** (aData[2]..aData[argc-1]) contain a new record to insert into
  ** the r-tree structure.
  */
  if( rc==SQLITE_OK && nData>1 && coordChange ){
    /* Insert the new record into the r-tree */
    RtreeNode *pLeaf = 0;
    if( !newRowidValid ){
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
  }

  /* Change the data */
  if( rc==SQLITE_OK && nData>1 ){
    sqlite3_stmt *pUp = pRtree->pWriteAux;
    int jj;
    int nChange = 0;
    sqlite3_bind_int64(pUp, 1, cell.iRowid);
    assert( pRtree->nAux>=1 );
    if( sqlite3_value_nochange(aData[2]) ){
      sqlite3_bind_null(pUp, 2);
    }else{
      GeoPoly *p = 0;
      if( sqlite3_value_type(aData[2])==SQLITE_TEXT
       && (p = geopolyFuncParam(0, aData[2], &rc))!=0
       && rc==SQLITE_OK
      ){
        sqlite3_bind_blob(pUp, 2, p->hdr, 4+8*p->nVertex, SQLITE_TRANSIENT);
      }else{
        sqlite3_bind_value(pUp, 2, aData[2]);
      }
      sqlite3_free(p);
      nChange = 1;
    }
    for(jj=1; jj<nData-2; jj++){
      nChange++;
      sqlite3_bind_value(pUp, jj+2, aData[jj+2]);
    }
    if( nChange ){
      sqlite3_step(pUp);
      rc = sqlite3_reset(pUp);
    }
  }

geopoly_update_end:
  rtreeRelease(pRtree);
  return rc;
}

/*
** Report that geopoly_overlap() is an overloaded function suitable
** for use in xBestIndex.
*/
static int geopolyFindFunction(
  sqlite3_vtab *pVtab,
  int nArg,
  const char *zName,
  void (**pxFunc)(sqlite3_context*,int,sqlite3_value**),
  void **ppArg
){
  (void)pVtab;
  (void)nArg;
  if( sqlite3_stricmp(zName, "geopoly_overlap")==0 ){
    *pxFunc = geopolyOverlapFunc;
    *ppArg = 0;
    return SQLITE_INDEX_CONSTRAINT_FUNCTION;
  }
  if( sqlite3_stricmp(zName, "geopoly_within")==0 ){
    *pxFunc = geopolyWithinFunc;
    *ppArg = 0;
    return SQLITE_INDEX_CONSTRAINT_FUNCTION+1;
  }
  return 0;
}


static sqlite3_module geopolyModule = {
  3,                          /* iVersion */
  geopolyCreate,              /* xCreate - create a table */
  geopolyConnect,             /* xConnect - connect to an existing table */
  geopolyBestIndex,           /* xBestIndex - Determine search strategy */
  rtreeDisconnect,            /* xDisconnect - Disconnect from a table */
  rtreeDestroy,               /* xDestroy - Drop a table */
  rtreeOpen,                  /* xOpen - open a cursor */
  rtreeClose,                 /* xClose - close a cursor */
  geopolyFilter,              /* xFilter - configure scan constraints */
  rtreeNext,                  /* xNext - advance a cursor */
  rtreeEof,                   /* xEof */
  geopolyColumn,              /* xColumn - read data */
  rtreeRowid,                 /* xRowid - read data */
  geopolyUpdate,              /* xUpdate - write data */
  rtreeBeginTransaction,      /* xBegin - begin transaction */
  rtreeEndTransaction,        /* xSync - sync transaction */
  rtreeEndTransaction,        /* xCommit - commit transaction */
  rtreeEndTransaction,        /* xRollback - rollback transaction */
  geopolyFindFunction,        /* xFindFunction - function overloading */
  rtreeRename,                /* xRename - rename the table */
  rtreeSavepoint,             /* xSavepoint */
  0,                          /* xRelease */
  0,                          /* xRollbackTo */
  rtreeShadowName,            /* xShadowName */
  rtreeIntegrity              /* xIntegrity */
};

static int sqlite3_geopoly_init(sqlite3 *db){
  int rc = SQLITE_OK;
  static const struct {
    void (*xFunc)(sqlite3_context*,int,sqlite3_value**);
    signed char nArg;
    unsigned char bPure;
    const char *zName;
  } aFunc[] = {
     { geopolyAreaFunc,          1, 1,    "geopoly_area"             },
     { geopolyBlobFunc,          1, 1,    "geopoly_blob"             },
     { geopolyJsonFunc,          1, 1,    "geopoly_json"             },
     { geopolySvgFunc,          -1, 1,    "geopoly_svg"              },
     { geopolyWithinFunc,        2, 1,    "geopoly_within"           },
     { geopolyContainsPointFunc, 3, 1,    "geopoly_contains_point"   },
     { geopolyOverlapFunc,       2, 1,    "geopoly_overlap"          },
     { geopolyDebugFunc,         1, 0,    "geopoly_debug"            },
     { geopolyBBoxFunc,          1, 1,    "geopoly_bbox"             },
     { geopolyXformFunc,         7, 1,    "geopoly_xform"            },
     { geopolyRegularFunc,       4, 1,    "geopoly_regular"          },
     { geopolyCcwFunc,           1, 1,    "geopoly_ccw"              },
  };
  static const struct {
    void (*xStep)(sqlite3_context*,int,sqlite3_value**);
    void (*xFinal)(sqlite3_context*);
    const char *zName;
  } aAgg[] = {
     { geopolyBBoxStep, geopolyBBoxFinal, "geopoly_group_bbox"    },
  };
  unsigned int i;
  for(i=0; i<sizeof(aFunc)/sizeof(aFunc[0]) && rc==SQLITE_OK; i++){
    int enc;
    if( aFunc[i].bPure ){
      enc = SQLITE_UTF8|SQLITE_DETERMINISTIC|SQLITE_INNOCUOUS;
    }else{
      enc = SQLITE_UTF8|SQLITE_DIRECTONLY;
    }
    rc = sqlite3_create_function(db, aFunc[i].zName, aFunc[i].nArg,
                                 enc, 0,
                                 aFunc[i].xFunc, 0, 0);
  }
  for(i=0; i<sizeof(aAgg)/sizeof(aAgg[0]) && rc==SQLITE_OK; i++){
    rc = sqlite3_create_function(db, aAgg[i].zName, 1,
              SQLITE_UTF8|SQLITE_DETERMINISTIC|SQLITE_INNOCUOUS, 0,
              0, aAgg[i].xStep, aAgg[i].xFinal);
  }
  if( rc==SQLITE_OK ){
    rc = sqlite3_create_module_v2(db, "geopoly", &geopolyModule, 0, 0);
  }
  return rc;
}

