

/*
** The z string points to the first character of a token that is
** associated with an error.  If db does not already have an error
** byte offset recorded, try to compute the error byte offset for
** z and set the error byte offset in db.
*/
SQLITE_PRIVATE void sqlite3RecordErrorByteOffset(sqlite3 *db, const char *z){
  const Parse *pParse;
  const char *zText;
  const char *zEnd;
  assert( z!=0 );
  if( NEVER(db==0) ) return;
  if( db->errByteOffset!=(-2) ) return;
  pParse = db->pParse;
  if( NEVER(pParse==0) ) return;
  zText =pParse->zTail;
  if( NEVER(zText==0) ) return;
  zEnd = &zText[strlen(zText)];
  if( SQLITE_WITHIN(z,zText,zEnd) ){
    db->errByteOffset = (int)(z-zText);
  }
}

/*
** If pExpr has a byte offset for the start of a token, record that as
** as the error offset.
*/
SQLITE_PRIVATE void sqlite3RecordErrorOffsetOfExpr(sqlite3 *db, const Expr *pExpr){
  while( pExpr
     && (ExprHasProperty(pExpr,EP_OuterON|EP_InnerON) || pExpr->w.iOfst<=0)
  ){
    pExpr = pExpr->pLeft;
  }
  if( pExpr==0 ) return;
  db->errByteOffset = pExpr->w.iOfst;
}

/*
** Enlarge the memory allocation on a StrAccum object so that it is
** able to accept at least N more bytes of text.
**
** Return the number of bytes of text that StrAccum is able to accept
** after the attempted enlargement.  The value returned might be zero.
*/
SQLITE_PRIVATE int sqlite3StrAccumEnlarge(StrAccum *p, i64 N){
  char *zNew;
  assert( p->nChar+N >= p->nAlloc ); /* Only called if really needed */
  if( p->accError ){
    testcase(p->accError==SQLITE_TOOBIG);
    testcase(p->accError==SQLITE_NOMEM);
    return 0;
  }
  if( p->mxAlloc==0 ){
    sqlite3StrAccumSetError(p, SQLITE_TOOBIG);
    return p->nAlloc - p->nChar - 1;
  }else{
    char *zOld = isMalloced(p) ? p->zText : 0;
    i64 szNew = p->nChar + N + 1;
    if( szNew+p->nChar<=p->mxAlloc ){
      /* Force exponential buffer size growth as long as it does not overflow,
      ** to avoid having to call this routine too often */
      szNew += p->nChar;
    }
    if( szNew > p->mxAlloc ){
      sqlite3_str_reset(p);
      sqlite3StrAccumSetError(p, SQLITE_TOOBIG);
      return 0;
    }else{
      p->nAlloc = (int)szNew;
    }
    if( p->db ){
      zNew = sqlite3DbRealloc(p->db, zOld, p->nAlloc);
    }else{
      zNew = sqlite3Realloc(zOld, p->nAlloc);
    }
    if( zNew ){
      assert( p->zText!=0 || p->nChar==0 );
      if( !isMalloced(p) && p->nChar>0 ) memcpy(zNew, p->zText, p->nChar);
      p->zText = zNew;
      p->nAlloc = sqlite3DbMallocSize(p->db, zNew);
      p->printfFlags |= SQLITE_PRINTF_MALLOCED;
    }else{
      sqlite3_str_reset(p);
      sqlite3StrAccumSetError(p, SQLITE_NOMEM);
      return 0;
    }
  }
  assert( N>=0 && N<=0x7fffffff );
  return (int)N;
}

/*
** Append N copies of character c to the given string buffer.
*/
SQLITE_API void sqlite3_str_appendchar(sqlite3_str *p, int N, char c){
  testcase( p->nChar + (i64)N > 0x7fffffff );
  if( p->nChar+(i64)N >= p->nAlloc && (N = sqlite3StrAccumEnlarge(p, N))<=0 ){
    return;
  }
  while( (N--)>0 ) p->zText[p->nChar++] = c;
}

/*
** The StrAccum "p" is not large enough to accept N new bytes of z[].
** So enlarge if first, then do the append.
**
** This is a helper routine to sqlite3_str_append() that does special-case
** work (enlarging the buffer) using tail recursion, so that the
** sqlite3_str_append() routine can use fast calling semantics.
*/
static void SQLITE_NOINLINE enlargeAndAppend(StrAccum *p, const char *z, int N){
  N = sqlite3StrAccumEnlarge(p, N);
  if( N>0 ){
    memcpy(&p->zText[p->nChar], z, N);
    p->nChar += N;
  }
}

/*
** Append N bytes of text from z to the StrAccum object.  Increase the
** size of the memory allocation for StrAccum if necessary.
*/
SQLITE_API void sqlite3_str_append(sqlite3_str *p, const char *z, int N){
  assert( z!=0 || N==0 );
  assert( p->zText!=0 || p->nChar==0 || p->accError );
  assert( N>=0 );
  assert( p->accError==0 || p->nAlloc==0 || p->mxAlloc==0 );
  if( p->nChar+N >= p->nAlloc ){
    enlargeAndAppend(p,z,N);
  }else if( N ){
    assert( p->zText );
    p->nChar += N;
    memcpy(&p->zText[p->nChar-N], z, N);
  }
}

/*
** Append the complete text of zero-terminated string z[] to the p string.
*/
SQLITE_API void sqlite3_str_appendall(sqlite3_str *p, const char *z){
  sqlite3_str_append(p, z, sqlite3Strlen30(z));
}


/*
** Finish off a string by making sure it is zero-terminated.
** Return a pointer to the resulting string.  Return a NULL
** pointer if any kind of error was encountered.
*/
static SQLITE_NOINLINE char *strAccumFinishRealloc(StrAccum *p){
  char *zText;
  assert( p->mxAlloc>0 && !isMalloced(p) );
  zText = sqlite3DbMallocRaw(p->db, p->nChar+1 );
  if( zText ){
    memcpy(zText, p->zText, p->nChar+1);
    p->printfFlags |= SQLITE_PRINTF_MALLOCED;
  }else{
    sqlite3StrAccumSetError(p, SQLITE_NOMEM);
  }
  p->zText = zText;
  return zText;
}
SQLITE_PRIVATE char *sqlite3StrAccumFinish(StrAccum *p){
  if( p->zText ){
    p->zText[p->nChar] = 0;
    if( p->mxAlloc>0 && !isMalloced(p) ){
      return strAccumFinishRealloc(p);
    }
  }
  return p->zText;
}

/*
** Use the content of the StrAccum passed as the second argument
** as the result of an SQL function.
*/
SQLITE_PRIVATE void sqlite3ResultStrAccum(sqlite3_context *pCtx, StrAccum *p){
  if( p->accError ){
    sqlite3_result_error_code(pCtx, p->accError);
    sqlite3_str_reset(p);
  }else if( isMalloced(p) ){
    sqlite3_result_text(pCtx, p->zText, p->nChar, SQLITE_DYNAMIC);
  }else{
    sqlite3_result_text(pCtx, "", 0, SQLITE_STATIC);
    sqlite3_str_reset(p);
  }
}

/*
** This singleton is an sqlite3_str object that is returned if
** sqlite3_malloc() fails to provide space for a real one.  This
** sqlite3_str object accepts no new text and always returns
** an SQLITE_NOMEM error.
*/
static sqlite3_str sqlite3OomStr = {
   0, 0, 0, 0, 0, SQLITE_NOMEM, 0
};

/* Finalize a string created using sqlite3_str_new().
*/
SQLITE_API char *sqlite3_str_finish(sqlite3_str *p){
  char *z;
  if( p!=0 && p!=&sqlite3OomStr ){
    z = sqlite3StrAccumFinish(p);
    sqlite3_free(p);
  }else{
    z = 0;
  }
  return z;
}

/* Return any error code associated with p */
SQLITE_API int sqlite3_str_errcode(sqlite3_str *p){
  return p ? p->accError : SQLITE_NOMEM;
}

/* Return the current length of p in bytes */
SQLITE_API int sqlite3_str_length(sqlite3_str *p){
  return p ? p->nChar : 0;
}

/* Return the current value for p */
SQLITE_API char *sqlite3_str_value(sqlite3_str *p){
  if( p==0 || p->nChar==0 ) return 0;
  p->zText[p->nChar] = 0;
  return p->zText;
}

/*
** Reset an StrAccum string.  Reclaim all malloced memory.
*/
SQLITE_API void sqlite3_str_reset(StrAccum *p){
  if( isMalloced(p) ){
    sqlite3DbFree(p->db, p->zText);
    p->printfFlags &= ~SQLITE_PRINTF_MALLOCED;
  }
  p->nAlloc = 0;
  p->nChar = 0;
  p->zText = 0;
}

/*
** Initialize a string accumulator.
**
** p:     The accumulator to be initialized.
** db:    Pointer to a database connection.  May be NULL.  Lookaside
**        memory is used if not NULL. db->mallocFailed is set appropriately
**        when not NULL.
** zBase: An initial buffer.  May be NULL in which case the initial buffer
**        is malloced.
** n:     Size of zBase in bytes.  If total space requirements never exceed
**        n then no memory allocations ever occur.
** mx:    Maximum number of bytes to accumulate.  If mx==0 then no memory
**        allocations will ever occur.
*/
SQLITE_PRIVATE void sqlite3StrAccumInit(StrAccum *p, sqlite3 *db, char *zBase, int n, int mx){
  p->zText = zBase;
  p->db = db;
  p->nAlloc = n;
  p->mxAlloc = mx;
  p->nChar = 0;
  p->accError = 0;
  p->printfFlags = 0;
}

/* Allocate and initialize a new dynamic string object */
SQLITE_API sqlite3_str *sqlite3_str_new(sqlite3 *db){
  sqlite3_str *p = sqlite3_malloc64(sizeof(*p));
  if( p ){
    sqlite3StrAccumInit(p, 0, 0, 0,
            db ? db->aLimit[SQLITE_LIMIT_LENGTH] : SQLITE_MAX_LENGTH);
  }else{
    p = &sqlite3OomStr;
  }
  return p;
}

/*
** Print into memory obtained from sqliteMalloc().  Use the internal
** %-conversion extensions.
*/
SQLITE_PRIVATE char *sqlite3VMPrintf(sqlite3 *db, const char *zFormat, va_list ap){
  char *z;
  char zBase[SQLITE_PRINT_BUF_SIZE];
  StrAccum acc;
  assert( db!=0 );
  sqlite3StrAccumInit(&acc, db, zBase, sizeof(zBase),
                      db->aLimit[SQLITE_LIMIT_LENGTH]);
  acc.printfFlags = SQLITE_PRINTF_INTERNAL;
  sqlite3_str_vappendf(&acc, zFormat, ap);
  z = sqlite3StrAccumFinish(&acc);
  if( acc.accError==SQLITE_NOMEM ){
    sqlite3OomFault(db);
  }
  return z;
}

/*
** Print into memory obtained from sqliteMalloc().  Use the internal
** %-conversion extensions.
*/
SQLITE_PRIVATE char *sqlite3MPrintf(sqlite3 *db, const char *zFormat, ...){
  va_list ap;
  char *z;
  va_start(ap, zFormat);
  z = sqlite3VMPrintf(db, zFormat, ap);
  va_end(ap);
  return z;
}

/*
** Print into memory obtained from sqlite3_malloc().  Omit the internal
** %-conversion extensions.
*/
SQLITE_API char *sqlite3_vmprintf(const char *zFormat, va_list ap){
  char *z;
  char zBase[SQLITE_PRINT_BUF_SIZE];
  StrAccum acc;

#ifdef SQLITE_ENABLE_API_ARMOR
  if( zFormat==0 ){
    (void)SQLITE_MISUSE_BKPT;
    return 0;
  }
#endif
#ifndef SQLITE_OMIT_AUTOINIT
  if( sqlite3_initialize() ) return 0;
#endif
  sqlite3StrAccumInit(&acc, 0, zBase, sizeof(zBase), SQLITE_MAX_LENGTH);
  sqlite3_str_vappendf(&acc, zFormat, ap);
  z = sqlite3StrAccumFinish(&acc);
  return z;
}

/*
** Print into memory obtained from sqlite3_malloc()().  Omit the internal
** %-conversion extensions.
*/
SQLITE_API char *sqlite3_mprintf(const char *zFormat, ...){
  va_list ap;
  char *z;
#ifndef SQLITE_OMIT_AUTOINIT
  if( sqlite3_initialize() ) return 0;
#endif
  va_start(ap, zFormat);
  z = sqlite3_vmprintf(zFormat, ap);
  va_end(ap);
  return z;
}
