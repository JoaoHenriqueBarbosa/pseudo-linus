
/*
** Perform a read or write operation on a blob
*/
static int blobReadWrite(
  sqlite3_blob *pBlob,
  void *z,
  int n,
  int iOffset,
  int (*xCall)(BtCursor*, u32, u32, void*)
){
  int rc;
  Incrblob *p = (Incrblob *)pBlob;
  Vdbe *v;
  sqlite3 *db;

  if( p==0 ) return SQLITE_MISUSE_BKPT;
  db = p->db;
  sqlite3_mutex_enter(db->mutex);
  v = (Vdbe*)p->pStmt;

  if( n<0 || iOffset<0 || ((sqlite3_int64)iOffset+n)>p->nByte ){
    /* Request is out of range. Return a transient error. */
    rc = SQLITE_ERROR;
  }else if( v==0 ){
    /* If there is no statement handle, then the blob-handle has
    ** already been invalidated. Return SQLITE_ABORT in this case.
    */
    rc = SQLITE_ABORT;
  }else{
    /* Call either BtreeData() or BtreePutData(). If SQLITE_ABORT is
    ** returned, clean-up the statement handle.
    */
    assert( db == v->db );
    sqlite3BtreeEnterCursor(p->pCsr);

#ifdef SQLITE_ENABLE_PREUPDATE_HOOK
    if( xCall==sqlite3BtreePutData && db->xPreUpdateCallback ){
      /* If a pre-update hook is registered and this is a write cursor,
      ** invoke it here.
      **
      ** TODO: The preupdate-hook is passed SQLITE_DELETE, even though this
      ** operation should really be an SQLITE_UPDATE. This is probably
      ** incorrect, but is convenient because at this point the new.* values
      ** are not easily obtainable. And for the sessions module, an
      ** SQLITE_UPDATE where the PK columns do not change is handled in the
      ** same way as an SQLITE_DELETE (the SQLITE_DELETE code is actually
      ** slightly more efficient). Since you cannot write to a PK column
      ** using the incremental-blob API, this works. For the sessions module
      ** anyhow.
      */
      sqlite3_int64 iKey;
      iKey = sqlite3BtreeIntegerKey(p->pCsr);
      assert( v->apCsr[0]!=0 );
      assert( v->apCsr[0]->eCurType==CURTYPE_BTREE );
      sqlite3VdbePreUpdateHook(
          v, v->apCsr[0], SQLITE_DELETE, p->zDb, p->pTab, iKey, -1, p->iCol
      );
    }
#endif

    rc = xCall(p->pCsr, iOffset+p->iOffset, n, z);
    sqlite3BtreeLeaveCursor(p->pCsr);
    if( rc==SQLITE_ABORT ){
      sqlite3VdbeFinalize(v);
      p->pStmt = 0;
    }else{
      v->rc = rc;
    }
  }
  sqlite3Error(db, rc);
  rc = sqlite3ApiExit(db, rc);
  sqlite3_mutex_leave(db->mutex);
  return rc;
}

/*
** Read data from a blob handle.
*/
SQLITE_API int sqlite3_blob_read(sqlite3_blob *pBlob, void *z, int n, int iOffset){
  return blobReadWrite(pBlob, z, n, iOffset, sqlite3BtreePayloadChecked);
}

/*
** Write data to a blob handle.
*/
SQLITE_API int sqlite3_blob_write(sqlite3_blob *pBlob, const void *z, int n, int iOffset){
  return blobReadWrite(pBlob, (void *)z, n, iOffset, sqlite3BtreePutData);
}

/*
** Query a blob handle for the size of the data.
**
** The Incrblob.nByte field is fixed for the lifetime of the Incrblob
** so no mutex is required for access.
*/
SQLITE_API int sqlite3_blob_bytes(sqlite3_blob *pBlob){
  Incrblob *p = (Incrblob *)pBlob;
  return (p && p->pStmt) ? p->nByte : 0;
}

/*
** Move an existing blob handle to point to a different row of the same
** database table.
**
** If an error occurs, or if the specified row does not exist or does not
** contain a blob or text value, then an error code is returned and the
** database handle error code and message set. If this happens, then all
** subsequent calls to sqlite3_blob_xxx() functions (except blob_close())
** immediately return SQLITE_ABORT.
*/
SQLITE_API int sqlite3_blob_reopen(sqlite3_blob *pBlob, sqlite3_int64 iRow){
  int rc;
  Incrblob *p = (Incrblob *)pBlob;
  sqlite3 *db;

  if( p==0 ) return SQLITE_MISUSE_BKPT;
  db = p->db;
  sqlite3_mutex_enter(db->mutex);

  if( p->pStmt==0 ){
    /* If there is no statement handle, then the blob-handle has
    ** already been invalidated. Return SQLITE_ABORT in this case.
    */
    rc = SQLITE_ABORT;
  }else{
    char *zErr;
    ((Vdbe*)p->pStmt)->rc = SQLITE_OK;
    rc = blobSeekToRow(p, iRow, &zErr);
    if( rc!=SQLITE_OK ){
      sqlite3ErrorWithMsg(db, rc, (zErr ? "%s" : (char*)0), zErr);
      sqlite3DbFree(db, zErr);
    }
    assert( rc!=SQLITE_SCHEMA );
  }

  rc = sqlite3ApiExit(db, rc);
  assert( rc==SQLITE_OK || p->pStmt==0 );
  sqlite3_mutex_leave(db->mutex);
  return rc;
}

#endif /* #ifndef SQLITE_OMIT_INCRBLOB */

