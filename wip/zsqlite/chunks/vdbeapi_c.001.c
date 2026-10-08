
/* Destroy an sqlite3_value object previously obtained from
** sqlite3_value_dup().
*/
SQLITE_API void sqlite3_value_free(sqlite3_value *pOld){
  sqlite3ValueFree(pOld);
}


/**************************** sqlite3_result_  *******************************
** The following routines are used by user-defined functions to specify
** the function result.
**
** The setStrOrError() function calls sqlite3VdbeMemSetStr() to store the
** result as a string or blob.  Appropriate errors are set if the string/blob
** is too big or if an OOM occurs.
**
** The invokeValueDestructor(P,X) routine invokes destructor function X()
** on value P if P is not going to be used and need to be destroyed.
*/
static void setResultStrOrError(
  sqlite3_context *pCtx,  /* Function context */
  const char *z,          /* String pointer */
  int n,                  /* Bytes in string, or negative */
  u8 enc,                 /* Encoding of z.  0 for BLOBs */
  void (*xDel)(void*)     /* Destructor function */
){
  Mem *pOut = pCtx->pOut;
  int rc = sqlite3VdbeMemSetStr(pOut, z, n, enc, xDel);
  if( rc ){
    if( rc==SQLITE_TOOBIG ){
      sqlite3_result_error_toobig(pCtx);
    }else{
      /* The only errors possible from sqlite3VdbeMemSetStr are
      ** SQLITE_TOOBIG and SQLITE_NOMEM */
      assert( rc==SQLITE_NOMEM );
      sqlite3_result_error_nomem(pCtx);
    }
    return;
  }
  sqlite3VdbeChangeEncoding(pOut, pCtx->enc);
  if( sqlite3VdbeMemTooBig(pOut) ){
    sqlite3_result_error_toobig(pCtx);
  }
}
static int invokeValueDestructor(
  const void *p,             /* Value to destroy */
  void (*xDel)(void*),       /* The destructor */
  sqlite3_context *pCtx      /* Set a SQLITE_TOOBIG error if not NULL */
){
  assert( xDel!=SQLITE_DYNAMIC );
  if( xDel==0 ){
    /* noop */
  }else if( xDel==SQLITE_TRANSIENT ){
    /* noop */
  }else{
    xDel((void*)p);
  }
#ifdef SQLITE_ENABLE_API_ARMOR
  if( pCtx!=0 ){
    sqlite3_result_error_toobig(pCtx);
  }
#else
  assert( pCtx!=0 );
  sqlite3_result_error_toobig(pCtx);
#endif
  return SQLITE_TOOBIG;
}
SQLITE_API void sqlite3_result_blob(
  sqlite3_context *pCtx,
  const void *z,
  int n,
  void (*xDel)(void *)
){
#ifdef SQLITE_ENABLE_API_ARMOR
  if( pCtx==0 || n<0 ){
    invokeValueDestructor(z, xDel, pCtx);
    return;
  }
#endif
  assert( n>=0 );
  assert( sqlite3_mutex_held(pCtx->pOut->db->mutex) );
  setResultStrOrError(pCtx, z, n, 0, xDel);
}
SQLITE_API void sqlite3_result_blob64(
  sqlite3_context *pCtx,
  const void *z,
  sqlite3_uint64 n,
  void (*xDel)(void *)
){
  assert( xDel!=SQLITE_DYNAMIC );
#ifdef SQLITE_ENABLE_API_ARMOR
  if( pCtx==0 ){
    invokeValueDestructor(z, xDel, 0);
    return;
  }
#endif
  assert( sqlite3_mutex_held(pCtx->pOut->db->mutex) );
  if( n>0x7fffffff ){
    (void)invokeValueDestructor(z, xDel, pCtx);
  }else{
    setResultStrOrError(pCtx, z, (int)n, 0, xDel);
  }
}
SQLITE_API void sqlite3_result_double(sqlite3_context *pCtx, double rVal){
#ifdef SQLITE_ENABLE_API_ARMOR
  if( pCtx==0 ) return;
#endif
  assert( sqlite3_mutex_held(pCtx->pOut->db->mutex) );
  sqlite3VdbeMemSetDouble(pCtx->pOut, rVal);
}
SQLITE_API void sqlite3_result_error(sqlite3_context *pCtx, const char *z, int n){
#ifdef SQLITE_ENABLE_API_ARMOR
  if( pCtx==0 ) return;
#endif
  assert( sqlite3_mutex_held(pCtx->pOut->db->mutex) );
  pCtx->isError = SQLITE_ERROR;
  sqlite3VdbeMemSetStr(pCtx->pOut, z, n, SQLITE_UTF8, SQLITE_TRANSIENT);
}
#ifndef SQLITE_OMIT_UTF16
SQLITE_API void sqlite3_result_error16(sqlite3_context *pCtx, const void *z, int n){
#ifdef SQLITE_ENABLE_API_ARMOR
  if( pCtx==0 ) return;
#endif
  assert( sqlite3_mutex_held(pCtx->pOut->db->mutex) );
  pCtx->isError = SQLITE_ERROR;
  sqlite3VdbeMemSetStr(pCtx->pOut, z, n, SQLITE_UTF16NATIVE, SQLITE_TRANSIENT);
}
#endif
SQLITE_API void sqlite3_result_int(sqlite3_context *pCtx, int iVal){
#ifdef SQLITE_ENABLE_API_ARMOR
  if( pCtx==0 ) return;
#endif
  assert( sqlite3_mutex_held(pCtx->pOut->db->mutex) );
  sqlite3VdbeMemSetInt64(pCtx->pOut, (i64)iVal);
}
SQLITE_API void sqlite3_result_int64(sqlite3_context *pCtx, i64 iVal){
#ifdef SQLITE_ENABLE_API_ARMOR
  if( pCtx==0 ) return;
#endif
  assert( sqlite3_mutex_held(pCtx->pOut->db->mutex) );
  sqlite3VdbeMemSetInt64(pCtx->pOut, iVal);
}
SQLITE_API void sqlite3_result_null(sqlite3_context *pCtx){
#ifdef SQLITE_ENABLE_API_ARMOR
  if( pCtx==0 ) return;
#endif
  assert( sqlite3_mutex_held(pCtx->pOut->db->mutex) );
  sqlite3VdbeMemSetNull(pCtx->pOut);
}
SQLITE_API void sqlite3_result_pointer(
  sqlite3_context *pCtx,
  void *pPtr,
  const char *zPType,
  void (*xDestructor)(void*)
){
  Mem *pOut;
#ifdef SQLITE_ENABLE_API_ARMOR
  if( pCtx==0 ){
    invokeValueDestructor(pPtr, xDestructor, 0);
    return;
  }
#endif
  pOut = pCtx->pOut;
  assert( sqlite3_mutex_held(pOut->db->mutex) );
  sqlite3VdbeMemRelease(pOut);
  pOut->flags = MEM_Null;
  sqlite3VdbeMemSetPointer(pOut, pPtr, zPType, xDestructor);
}
SQLITE_API void sqlite3_result_subtype(sqlite3_context *pCtx, unsigned int eSubtype){
  Mem *pOut;
#ifdef SQLITE_ENABLE_API_ARMOR
  if( pCtx==0 ) return;
#endif
#if defined(SQLITE_STRICT_SUBTYPE) && SQLITE_STRICT_SUBTYPE+0!=0
  if( pCtx->pFunc!=0
   && (pCtx->pFunc->funcFlags & SQLITE_RESULT_SUBTYPE)==0
  ){
    char zErr[200];
    sqlite3_snprintf(sizeof(zErr), zErr,
                     "misuse of sqlite3_result_subtype() by %s()",
                     pCtx->pFunc->zName);
    sqlite3_result_error(pCtx, zErr, -1);
    return;
  }
#endif /* SQLITE_STRICT_SUBTYPE */
  pOut = pCtx->pOut;
  assert( sqlite3_mutex_held(pOut->db->mutex) );
  pOut->eSubtype = eSubtype & 0xff;
  pOut->flags |= MEM_Subtype;
}
SQLITE_API void sqlite3_result_text(
  sqlite3_context *pCtx,
  const char *z,
  int n,
  void (*xDel)(void *)
){
#ifdef SQLITE_ENABLE_API_ARMOR
  if( pCtx==0 ){
    invokeValueDestructor(z, xDel, 0);
    return;
  }
#endif
  assert( sqlite3_mutex_held(pCtx->pOut->db->mutex) );
  setResultStrOrError(pCtx, z, n, SQLITE_UTF8, xDel);
}
SQLITE_API void sqlite3_result_text64(
  sqlite3_context *pCtx,
  const char *z,
  sqlite3_uint64 n,
  void (*xDel)(void *),
  unsigned char enc
){
#ifdef SQLITE_ENABLE_API_ARMOR
  if( pCtx==0 ){
    invokeValueDestructor(z, xDel, 0);
    return;
  }
#endif
  assert( sqlite3_mutex_held(pCtx->pOut->db->mutex) );
  assert( xDel!=SQLITE_DYNAMIC );
  if( enc!=SQLITE_UTF8 ){
    if( enc==SQLITE_UTF16 ) enc = SQLITE_UTF16NATIVE;
    n &= ~(u64)1;
  }
  if( n>0x7fffffff ){
    (void)invokeValueDestructor(z, xDel, pCtx);
  }else{
    setResultStrOrError(pCtx, z, (int)n, enc, xDel);
    sqlite3VdbeMemZeroTerminateIfAble(pCtx->pOut);
  }
}
#ifndef SQLITE_OMIT_UTF16
SQLITE_API void sqlite3_result_text16(
  sqlite3_context *pCtx,
  const void *z,
  int n,
  void (*xDel)(void *)
){
  assert( sqlite3_mutex_held(pCtx->pOut->db->mutex) );
  setResultStrOrError(pCtx, z, n & ~(u64)1, SQLITE_UTF16NATIVE, xDel);
}
SQLITE_API void sqlite3_result_text16be(
  sqlite3_context *pCtx,
  const void *z,
  int n,
  void (*xDel)(void *)
){
  assert( sqlite3_mutex_held(pCtx->pOut->db->mutex) );
  setResultStrOrError(pCtx, z, n & ~(u64)1, SQLITE_UTF16BE, xDel);
}
SQLITE_API void sqlite3_result_text16le(
  sqlite3_context *pCtx,
  const void *z,
  int n,
  void (*xDel)(void *)
){
  assert( sqlite3_mutex_held(pCtx->pOut->db->mutex) );
  setResultStrOrError(pCtx, z, n & ~(u64)1, SQLITE_UTF16LE, xDel);
}
#endif /* SQLITE_OMIT_UTF16 */
SQLITE_API void sqlite3_result_value(sqlite3_context *pCtx, sqlite3_value *pValue){
  Mem *pOut;

#ifdef SQLITE_ENABLE_API_ARMOR
  if( pCtx==0 ) return;
  if( pValue==0 ){
    sqlite3_result_null(pCtx);
    return;
  }
#endif
  pOut = pCtx->pOut;
  assert( sqlite3_mutex_held(pCtx->pOut->db->mutex) );
  sqlite3VdbeMemCopy(pOut, pValue);
  sqlite3VdbeChangeEncoding(pOut, pCtx->enc);
  if( sqlite3VdbeMemTooBig(pOut) ){
    sqlite3_result_error_toobig(pCtx);
  }
}
SQLITE_API void sqlite3_result_zeroblob(sqlite3_context *pCtx, int n){
  sqlite3_result_zeroblob64(pCtx, n>0 ? n : 0);
}
SQLITE_API int sqlite3_result_zeroblob64(sqlite3_context *pCtx, u64 n){
  Mem *pOut;

#ifdef SQLITE_ENABLE_API_ARMOR
  if( pCtx==0 ) return SQLITE_MISUSE_BKPT;
#endif
  pOut = pCtx->pOut;
  assert( sqlite3_mutex_held(pOut->db->mutex) );
  if( n>(u64)pOut->db->aLimit[SQLITE_LIMIT_LENGTH] ){
    sqlite3_result_error_toobig(pCtx);
    return SQLITE_TOOBIG;
  }
#ifndef SQLITE_OMIT_INCRBLOB
  sqlite3VdbeMemSetZeroBlob(pCtx->pOut, (int)n);
  return SQLITE_OK;
#else
  return sqlite3VdbeMemSetZeroBlob(pCtx->pOut, (int)n);
#endif
}
SQLITE_API void sqlite3_result_error_code(sqlite3_context *pCtx, int errCode){
#ifdef SQLITE_ENABLE_API_ARMOR
  if( pCtx==0 ) return;
#endif
  pCtx->isError = errCode ? errCode : -1;
#ifdef SQLITE_DEBUG
  if( pCtx->pVdbe ) pCtx->pVdbe->rcApp = errCode;
#endif
  if( pCtx->pOut->flags & MEM_Null ){
    setResultStrOrError(pCtx, sqlite3ErrStr(errCode), -1, SQLITE_UTF8,
                        SQLITE_STATIC);
  }
}

/* Force an SQLITE_TOOBIG error. */
SQLITE_API void sqlite3_result_error_toobig(sqlite3_context *pCtx){
#ifdef SQLITE_ENABLE_API_ARMOR
  if( pCtx==0 ) return;
#endif
  assert( sqlite3_mutex_held(pCtx->pOut->db->mutex) );
  pCtx->isError = SQLITE_TOOBIG;
  sqlite3VdbeMemSetStr(pCtx->pOut, "string or blob too big", -1,
                       SQLITE_UTF8, SQLITE_STATIC);
}

/* An SQLITE_NOMEM error. */
SQLITE_API void sqlite3_result_error_nomem(sqlite3_context *pCtx){
#ifdef SQLITE_ENABLE_API_ARMOR
  if( pCtx==0 ) return;
#endif
  assert( sqlite3_mutex_held(pCtx->pOut->db->mutex) );
  sqlite3VdbeMemSetNull(pCtx->pOut);
  pCtx->isError = SQLITE_NOMEM_BKPT;
  sqlite3OomFault(pCtx->pOut->db);
}

#ifndef SQLITE_UNTESTABLE
/* Force the INT64 value currently stored as the result to be
** a MEM_IntReal value.  See the SQLITE_TESTCTRL_RESULT_INTREAL
** test-control.
*/
SQLITE_PRIVATE void sqlite3ResultIntReal(sqlite3_context *pCtx){
  assert( sqlite3_mutex_held(pCtx->pOut->db->mutex) );
  if( pCtx->pOut->flags & MEM_Int ){
    pCtx->pOut->flags &= ~MEM_Int;
    pCtx->pOut->flags |= MEM_IntReal;
  }
}
#endif


/*
** This function is called after a transaction has been committed. It
** invokes callbacks registered with sqlite3_wal_hook() as required.
*/
static int doWalCallbacks(sqlite3 *db){
  int rc = SQLITE_OK;
#ifndef SQLITE_OMIT_WAL
  int i;
  for(i=0; i<db->nDb; i++){
    Btree *pBt = db->aDb[i].pBt;
    if( pBt ){
      int nEntry;
      sqlite3BtreeEnter(pBt);
      nEntry = sqlite3PagerWalCallback(sqlite3BtreePager(pBt));
      sqlite3BtreeLeave(pBt);
      if( nEntry>0 && db->xWalCallback && rc==SQLITE_OK ){
        rc = db->xWalCallback(db->pWalArg, db, db->aDb[i].zDbSName, nEntry);
      }
    }
  }
#endif
  return rc;
}
