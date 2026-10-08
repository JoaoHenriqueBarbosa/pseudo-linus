
/*
** Sleep for nMicro microseconds. Return the number of microseconds
** actually slept.
*/
static int memdbSleep(sqlite3_vfs *pVfs, int nMicro){
  return ORIGVFS(pVfs)->xSleep(ORIGVFS(pVfs), nMicro);
}

#if 0  /* Never used.  Modern cores only call xCurrentTimeInt64() */
/*
** Return the current time as a Julian Day number in *pTimeOut.
*/
static int memdbCurrentTime(sqlite3_vfs *pVfs, double *pTimeOut){
  return ORIGVFS(pVfs)->xCurrentTime(ORIGVFS(pVfs), pTimeOut);
}
#endif

static int memdbGetLastError(sqlite3_vfs *pVfs, int a, char *b){
  return ORIGVFS(pVfs)->xGetLastError(ORIGVFS(pVfs), a, b);
}
static int memdbCurrentTimeInt64(sqlite3_vfs *pVfs, sqlite3_int64 *p){
  return ORIGVFS(pVfs)->xCurrentTimeInt64(ORIGVFS(pVfs), p);
}

/*
** Translate a database connection pointer and schema name into a
** MemFile pointer.
*/
static MemFile *memdbFromDbSchema(sqlite3 *db, const char *zSchema){
  MemFile *p = 0;
  MemStore *pStore;
  int rc = sqlite3_file_control(db, zSchema, SQLITE_FCNTL_FILE_POINTER, &p);
  if( rc ) return 0;
  if( p->base.pMethods!=&memdb_io_methods ) return 0;
  pStore = p->pStore;
  memdbEnter(pStore);
  if( pStore->zFName!=0 ) p = 0;
  memdbLeave(pStore);
  return p;
}

/*
** Return the serialization of a database
*/
SQLITE_API unsigned char *sqlite3_serialize(
  sqlite3 *db,              /* The database connection */
  const char *zSchema,      /* Which database within the connection */
  sqlite3_int64 *piSize,    /* Write size here, if not NULL */
  unsigned int mFlags       /* Maybe SQLITE_SERIALIZE_NOCOPY */
){
  MemFile *p;
  int iDb;
  Btree *pBt;
  sqlite3_int64 sz;
  int szPage = 0;
  sqlite3_stmt *pStmt = 0;
  unsigned char *pOut;
  char *zSql;
  int rc;

#ifdef SQLITE_ENABLE_API_ARMOR
  if( !sqlite3SafetyCheckOk(db) ){
    (void)SQLITE_MISUSE_BKPT;
    return 0;
  }
#endif

  if( zSchema==0 ) zSchema = db->aDb[0].zDbSName;
  p = memdbFromDbSchema(db, zSchema);
  iDb = sqlite3FindDbName(db, zSchema);
  if( piSize ) *piSize = -1;
  if( iDb<0 ) return 0;
  if( p ){
    MemStore *pStore = p->pStore;
    assert( pStore->pMutex==0 );
    if( piSize ) *piSize = pStore->sz;
    if( mFlags & SQLITE_SERIALIZE_NOCOPY ){
      pOut = pStore->aData;
    }else{
      pOut = sqlite3_malloc64( pStore->sz );
      if( pOut ) memcpy(pOut, pStore->aData, pStore->sz);
    }
    return pOut;
  }
  pBt = db->aDb[iDb].pBt;
  if( pBt==0 ) return 0;
  szPage = sqlite3BtreeGetPageSize(pBt);
  zSql = sqlite3_mprintf("PRAGMA \"%w\".page_count", zSchema);
  rc = zSql ? sqlite3_prepare_v2(db, zSql, -1, &pStmt, 0) : SQLITE_NOMEM;
  sqlite3_free(zSql);
  if( rc ) return 0;
  rc = sqlite3_step(pStmt);
  if( rc!=SQLITE_ROW ){
    pOut = 0;
  }else{
    sz = sqlite3_column_int64(pStmt, 0)*szPage;
    if( sz==0 ){
      sqlite3_reset(pStmt);
      sqlite3_exec(db, "BEGIN IMMEDIATE; COMMIT;", 0, 0, 0);
      rc = sqlite3_step(pStmt);
      if( rc==SQLITE_ROW ){
        sz = sqlite3_column_int64(pStmt, 0)*szPage;
      }
    }
    if( piSize ) *piSize = sz;
    if( mFlags & SQLITE_SERIALIZE_NOCOPY ){
      pOut = 0;
    }else{
      pOut = sqlite3_malloc64( sz );
      if( pOut ){
        int nPage = sqlite3_column_int(pStmt, 0);
        Pager *pPager = sqlite3BtreePager(pBt);
        int pgno;
        for(pgno=1; pgno<=nPage; pgno++){
          DbPage *pPage = 0;
          unsigned char *pTo = pOut + szPage*(sqlite3_int64)(pgno-1);
          rc = sqlite3PagerGet(pPager, pgno, (DbPage**)&pPage, 0);
          if( rc==SQLITE_OK ){
            memcpy(pTo, sqlite3PagerGetData(pPage), szPage);
          }else{
            memset(pTo, 0, szPage);
          }
          sqlite3PagerUnref(pPage);
        }
      }
    }
  }
  sqlite3_finalize(pStmt);
  return pOut;
}

/* Convert zSchema to a MemDB and initialize its content.
*/
SQLITE_API int sqlite3_deserialize(
  sqlite3 *db,            /* The database connection */
  const char *zSchema,    /* Which DB to reopen with the deserialization */
  unsigned char *pData,   /* The serialized database content */
  sqlite3_int64 szDb,     /* Number bytes in the deserialization */
  sqlite3_int64 szBuf,    /* Total size of buffer pData[] */
  unsigned mFlags         /* Zero or more SQLITE_DESERIALIZE_* flags */
){
  MemFile *p;
  char *zSql;
  sqlite3_stmt *pStmt = 0;
  int rc;
  int iDb;

#ifdef SQLITE_ENABLE_API_ARMOR
  if( !sqlite3SafetyCheckOk(db) ){
    return SQLITE_MISUSE_BKPT;
  }
  if( szDb<0 ) return SQLITE_MISUSE_BKPT;
  if( szBuf<0 ) return SQLITE_MISUSE_BKPT;
#endif

  sqlite3_mutex_enter(db->mutex);
  if( zSchema==0 ) zSchema = db->aDb[0].zDbSName;
  iDb = sqlite3FindDbName(db, zSchema);
  testcase( iDb==1 );
  if( iDb<2 && iDb!=0 ){
    rc = SQLITE_ERROR;
    goto end_deserialize;
  }
  zSql = sqlite3_mprintf("ATTACH x AS %Q", zSchema);
  if( zSql==0 ){
    rc = SQLITE_NOMEM;
  }else{
    rc = sqlite3_prepare_v2(db, zSql, -1, &pStmt, 0);
    sqlite3_free(zSql);
  }
  if( rc ) goto end_deserialize;
  db->init.iDb = (u8)iDb;
  db->init.reopenMemdb = 1;
  rc = sqlite3_step(pStmt);
  db->init.reopenMemdb = 0;
  if( rc!=SQLITE_DONE ){
    rc = SQLITE_ERROR;
    goto end_deserialize;
  }
  p = memdbFromDbSchema(db, zSchema);
  if( p==0 ){
    rc = SQLITE_ERROR;
  }else{
    MemStore *pStore = p->pStore;
    pStore->aData = pData;
    pData = 0;
    pStore->sz = szDb;
    pStore->szAlloc = szBuf;
    pStore->szMax = szBuf;
    if( pStore->szMax<sqlite3GlobalConfig.mxMemdbSize ){
      pStore->szMax = sqlite3GlobalConfig.mxMemdbSize;
    }
    pStore->mFlags = mFlags;
    rc = SQLITE_OK;
  }

end_deserialize:
  sqlite3_finalize(pStmt);
  if( pData && (mFlags & SQLITE_DESERIALIZE_FREEONCLOSE)!=0 ){
    sqlite3_free(pData);
  }
  sqlite3_mutex_leave(db->mutex);
  return rc;
}

/*
** Return true if the VFS is the memvfs.
*/
SQLITE_PRIVATE int sqlite3IsMemdb(const sqlite3_vfs *pVfs){
  return pVfs==&memdb_vfs;
}

/*
** This routine is called when the extension is loaded.
** Register the new VFS.
*/
SQLITE_PRIVATE int sqlite3MemdbInit(void){
  sqlite3_vfs *pLower = sqlite3_vfs_find(0);
  unsigned int sz;
  if( NEVER(pLower==0) ) return SQLITE_ERROR;
  sz = pLower->szOsFile;
  memdb_vfs.pAppData = pLower;
  /* The following conditional can only be true when compiled for
  ** Windows x86 and SQLITE_MAX_MMAP_SIZE=0.  We always leave
  ** it in, to be safe, but it is marked as NO_TEST since there
  ** is no way to reach it under most builds. */
  if( sz<sizeof(MemFile) ) sz = sizeof(MemFile); /*NO_TEST*/
  memdb_vfs.szOsFile = sz;
  return sqlite3_vfs_register(&memdb_vfs, 0);
}
#endif /* SQLITE_OMIT_DESERIALIZE */

