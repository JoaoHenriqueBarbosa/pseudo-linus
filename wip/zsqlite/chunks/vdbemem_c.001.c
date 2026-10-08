
/*
** Change pMem so that its MEM_Str or MEM_Blob value is stored in
** MEM.zMalloc, where it can be safely written.
**
** Return SQLITE_OK on success or SQLITE_NOMEM if malloc fails.
*/
SQLITE_PRIVATE int sqlite3VdbeMemMakeWriteable(Mem *pMem){
  assert( pMem!=0 );
  assert( pMem->db==0 || sqlite3_mutex_held(pMem->db->mutex) );
  assert( !sqlite3VdbeMemIsRowSet(pMem) );
  if( (pMem->flags & (MEM_Str|MEM_Blob))!=0 ){
    if( ExpandBlob(pMem) ) return SQLITE_NOMEM;
    if( pMem->szMalloc==0 || pMem->z!=pMem->zMalloc ){
      int rc = vdbeMemAddTerminator(pMem);
      if( rc ) return rc;
    }
  }
  pMem->flags &= ~MEM_Ephem;
#ifdef SQLITE_DEBUG
  pMem->pScopyFrom = 0;
#endif

  return SQLITE_OK;
}

/*
** If the given Mem* has a zero-filled tail, turn it into an ordinary
** blob stored in dynamically allocated space.
*/
#ifndef SQLITE_OMIT_INCRBLOB
SQLITE_PRIVATE int sqlite3VdbeMemExpandBlob(Mem *pMem){
  int nByte;
  assert( pMem!=0 );
  assert( pMem->flags & MEM_Zero );
  assert( (pMem->flags&MEM_Blob)!=0 || MemNullNochng(pMem) );
  testcase( sqlite3_value_nochange(pMem) );
  assert( !sqlite3VdbeMemIsRowSet(pMem) );
  assert( pMem->db==0 || sqlite3_mutex_held(pMem->db->mutex) );

  /* Set nByte to the number of bytes required to store the expanded blob. */
  nByte = pMem->n + pMem->u.nZero;
  if( nByte<=0 ){
    if( (pMem->flags & MEM_Blob)==0 ) return SQLITE_OK;
    nByte = 1;
  }
  if( sqlite3VdbeMemGrow(pMem, nByte, 1) ){
    return SQLITE_NOMEM_BKPT;
  }
  assert( pMem->z!=0 );
  assert( sqlite3DbMallocSize(pMem->db,pMem->z) >= nByte );

  memset(&pMem->z[pMem->n], 0, pMem->u.nZero);
  pMem->n += pMem->u.nZero;
  pMem->flags &= ~(MEM_Zero|MEM_Term);
  return SQLITE_OK;
}
#endif

/*
** Make sure the given Mem is \u0000 terminated.
*/
SQLITE_PRIVATE int sqlite3VdbeMemNulTerminate(Mem *pMem){
  assert( pMem!=0 );
  assert( pMem->db==0 || sqlite3_mutex_held(pMem->db->mutex) );
  testcase( (pMem->flags & (MEM_Term|MEM_Str))==(MEM_Term|MEM_Str) );
  testcase( (pMem->flags & (MEM_Term|MEM_Str))==0 );
  if( (pMem->flags & (MEM_Term|MEM_Str))!=MEM_Str ){
    return SQLITE_OK;   /* Nothing to do */
  }else{
    return vdbeMemAddTerminator(pMem);
  }
}

/*
** Add MEM_Str to the set of representations for the given Mem.  This
** routine is only called if pMem is a number of some kind, not a NULL
** or a BLOB.
**
** Existing representations MEM_Int, MEM_Real, or MEM_IntReal are invalidated
** if bForce is true but are retained if bForce is false.
**
** A MEM_Null value will never be passed to this function. This function is
** used for converting values to text for returning to the user (i.e. via
** sqlite3_value_text()), or for ensuring that values to be used as btree
** keys are strings. In the former case a NULL pointer is returned the
** user and the latter is an internal programming error.
*/
SQLITE_PRIVATE int sqlite3VdbeMemStringify(Mem *pMem, u8 enc, u8 bForce){
  const int nByte = 32;

  assert( pMem!=0 );
  assert( pMem->db==0 || sqlite3_mutex_held(pMem->db->mutex) );
  assert( !(pMem->flags&MEM_Zero) );
  assert( !(pMem->flags&(MEM_Str|MEM_Blob)) );
  assert( pMem->flags&(MEM_Int|MEM_Real|MEM_IntReal) );
  assert( !sqlite3VdbeMemIsRowSet(pMem) );
  assert( EIGHT_BYTE_ALIGNMENT(pMem) );


  if( sqlite3VdbeMemClearAndResize(pMem, nByte) ){
    pMem->enc = 0;
    return SQLITE_NOMEM_BKPT;
  }

  vdbeMemRenderNum(nByte, pMem->z, pMem);
  assert( pMem->z!=0 );
  assert( pMem->n==(int)sqlite3Strlen30NN(pMem->z) );
  pMem->enc = SQLITE_UTF8;
  pMem->flags |= MEM_Str|MEM_Term;
  if( bForce ) pMem->flags &= ~(MEM_Int|MEM_Real|MEM_IntReal);
  sqlite3VdbeChangeEncoding(pMem, enc);
  return SQLITE_OK;
}

/*
** Memory cell pMem contains the context of an aggregate function.
** This routine calls the finalize method for that function.  The
** result of the aggregate is stored back into pMem.
**
** Return SQLITE_ERROR if the finalizer reports an error.  SQLITE_OK
** otherwise.
*/
SQLITE_PRIVATE int sqlite3VdbeMemFinalize(Mem *pMem, FuncDef *pFunc){
  sqlite3_context ctx;
  Mem t;
  assert( pFunc!=0 );
  assert( pMem!=0 );
  assert( pMem->db!=0 );
  assert( pFunc->xFinalize!=0 );
  assert( (pMem->flags & MEM_Null)!=0 || pFunc==pMem->u.pDef );
  assert( sqlite3_mutex_held(pMem->db->mutex) );
  memset(&ctx, 0, sizeof(ctx));
  memset(&t, 0, sizeof(t));
  t.flags = MEM_Null;
  t.db = pMem->db;
  ctx.pOut = &t;
  ctx.pMem = pMem;
  ctx.pFunc = pFunc;
  ctx.enc = ENC(t.db);
  pFunc->xFinalize(&ctx); /* IMP: R-24505-23230 */
  assert( (pMem->flags & MEM_Dyn)==0 );
  if( pMem->szMalloc>0 ) sqlite3DbFreeNN(pMem->db, pMem->zMalloc);
  memcpy(pMem, &t, sizeof(t));
  return ctx.isError;
}

/*
** Memory cell pAccum contains the context of an aggregate function.
** This routine calls the xValue method for that function and stores
** the results in memory cell pMem.
**
** SQLITE_ERROR is returned if xValue() reports an error. SQLITE_OK
** otherwise.
*/
#ifndef SQLITE_OMIT_WINDOWFUNC
SQLITE_PRIVATE int sqlite3VdbeMemAggValue(Mem *pAccum, Mem *pOut, FuncDef *pFunc){
  sqlite3_context ctx;
  assert( pFunc!=0 );
  assert( pFunc->xValue!=0 );
  assert( (pAccum->flags & MEM_Null)!=0 || pFunc==pAccum->u.pDef );
  assert( pAccum->db!=0 );
  assert( sqlite3_mutex_held(pAccum->db->mutex) );
  memset(&ctx, 0, sizeof(ctx));
  sqlite3VdbeMemSetNull(pOut);
  ctx.pOut = pOut;
  ctx.pMem = pAccum;
  ctx.pFunc = pFunc;
  ctx.enc = ENC(pAccum->db);
  pFunc->xValue(&ctx);
  return ctx.isError;
}
#endif /* SQLITE_OMIT_WINDOWFUNC */

/*
** If the memory cell contains a value that must be freed by
** invoking the external callback in Mem.xDel, then this routine
** will free that value.  It also sets Mem.flags to MEM_Null.
**
** This is a helper routine for sqlite3VdbeMemSetNull() and
** for sqlite3VdbeMemRelease().  Use those other routines as the
** entry point for releasing Mem resources.
*/
static SQLITE_NOINLINE void vdbeMemClearExternAndSetNull(Mem *p){
  assert( p->db==0 || sqlite3_mutex_held(p->db->mutex) );
  assert( VdbeMemDynamic(p) );
  if( p->flags&MEM_Agg ){
    sqlite3VdbeMemFinalize(p, p->u.pDef);
    assert( (p->flags & MEM_Agg)==0 );
    testcase( p->flags & MEM_Dyn );
  }
  if( p->flags&MEM_Dyn ){
    assert( p->xDel!=SQLITE_DYNAMIC && p->xDel!=0 );
    p->xDel((void *)p->z);
  }
  p->flags = MEM_Null;
}

/*
** Release memory held by the Mem p, both external memory cleared
** by p->xDel and memory in p->zMalloc.
**
** This is a helper routine invoked by sqlite3VdbeMemRelease() in
** the unusual case where there really is memory in p that needs
** to be freed.
*/
static SQLITE_NOINLINE void vdbeMemClear(Mem *p){
  if( VdbeMemDynamic(p) ){
    vdbeMemClearExternAndSetNull(p);
  }
  if( p->szMalloc ){
    sqlite3DbFreeNN(p->db, p->zMalloc);
    p->szMalloc = 0;
  }
  p->z = 0;
}

/*
** Release any memory resources held by the Mem.  Both the memory that is
** free by Mem.xDel and the Mem.zMalloc allocation are freed.
**
** Use this routine prior to clean up prior to abandoning a Mem, or to
** reset a Mem back to its minimum memory utilization.
**
** Use sqlite3VdbeMemSetNull() to release just the Mem.xDel space
** prior to inserting new content into the Mem.
*/
SQLITE_PRIVATE void sqlite3VdbeMemRelease(Mem *p){
  assert( sqlite3VdbeCheckMemInvariants(p) );
  if( VdbeMemDynamic(p) || p->szMalloc ){
    vdbeMemClear(p);
  }
}

/* Like sqlite3VdbeMemRelease() but faster for cases where we
** know in advance that the Mem is not MEM_Dyn or MEM_Agg.
*/
SQLITE_PRIVATE void sqlite3VdbeMemReleaseMalloc(Mem *p){
  assert( !VdbeMemDynamic(p) );
  if( p->szMalloc ) vdbeMemClear(p);
}

/*
** Return some kind of integer value which is the best we can do
** at representing the value that *pMem describes as an integer.
** If pMem is an integer, then the value is exact.  If pMem is
** a floating-point then the value returned is the integer part.
** If pMem is a string or blob, then we make an attempt to convert
** it into an integer and return that.  If pMem represents an
** an SQL-NULL value, return 0.
**
** If pMem represents a string value, its encoding might be changed.
*/
static SQLITE_NOINLINE i64 memIntValue(const Mem *pMem){
  i64 value = 0;
  sqlite3Atoi64(pMem->z, &value, pMem->n, pMem->enc);
  return value;
}
SQLITE_PRIVATE i64 sqlite3VdbeIntValue(const Mem *pMem){
  int flags;
  assert( pMem!=0 );
  assert( pMem->db==0 || sqlite3_mutex_held(pMem->db->mutex) );
  assert( EIGHT_BYTE_ALIGNMENT(pMem) );
  flags = pMem->flags;
  if( flags & (MEM_Int|MEM_IntReal) ){
    testcase( flags & MEM_IntReal );
    return pMem->u.i;
  }else if( flags & MEM_Real ){
    return sqlite3RealToI64(pMem->u.r);
  }else if( (flags & (MEM_Str|MEM_Blob))!=0 && pMem->z!=0 ){
    return memIntValue(pMem);
  }else{
    return 0;
  }
}

/*
** Return the best representation of pMem that we can get into a
** double.  If pMem is already a double or an integer, return its
** value.  If it is a string or blob, try to convert it to a double.
** If it is a NULL, return 0.0.
*/
static SQLITE_NOINLINE double memRealValue(Mem *pMem){
  /* (double)0 In case of SQLITE_OMIT_FLOATING_POINT... */
  double val = (double)0;
  sqlite3AtoF(pMem->z, &val, pMem->n, pMem->enc);
  return val;
}
SQLITE_PRIVATE double sqlite3VdbeRealValue(Mem *pMem){
  assert( pMem!=0 );
  assert( pMem->db==0 || sqlite3_mutex_held(pMem->db->mutex) );
  assert( EIGHT_BYTE_ALIGNMENT(pMem) );
  if( pMem->flags & MEM_Real ){
    return pMem->u.r;
  }else if( pMem->flags & (MEM_Int|MEM_IntReal) ){
    testcase( pMem->flags & MEM_IntReal );
    return (double)pMem->u.i;
  }else if( pMem->flags & (MEM_Str|MEM_Blob) ){
    return memRealValue(pMem);
  }else{
    /* (double)0 In case of SQLITE_OMIT_FLOATING_POINT... */
    return (double)0;
  }
}

/*
** Return 1 if pMem represents true, and return 0 if pMem represents false.
** Return the value ifNull if pMem is NULL.
*/
SQLITE_PRIVATE int sqlite3VdbeBooleanValue(Mem *pMem, int ifNull){
  testcase( pMem->flags & MEM_IntReal );
  if( pMem->flags & (MEM_Int|MEM_IntReal) ) return pMem->u.i!=0;
  if( pMem->flags & MEM_Null ) return ifNull;
  return sqlite3VdbeRealValue(pMem)!=0.0;
}

/*
** The MEM structure is already a MEM_Real or MEM_IntReal. Try to
** make it a MEM_Int if we can.
*/
SQLITE_PRIVATE void sqlite3VdbeIntegerAffinity(Mem *pMem){
  assert( pMem!=0 );
  assert( pMem->flags & (MEM_Real|MEM_IntReal) );
  assert( !sqlite3VdbeMemIsRowSet(pMem) );
  assert( pMem->db==0 || sqlite3_mutex_held(pMem->db->mutex) );
  assert( EIGHT_BYTE_ALIGNMENT(pMem) );

  if( pMem->flags & MEM_IntReal ){
    MemSetTypeFlag(pMem, MEM_Int);
  }else{
    i64 ix = sqlite3RealToI64(pMem->u.r);

    /* Only mark the value as an integer if
    **
    **    (1) the round-trip conversion real->int->real is a no-op, and
    **    (2) The integer is neither the largest nor the smallest
    **        possible integer (ticket #3922)
    **
    ** The second and third terms in the following conditional enforces
    ** the second condition under the assumption that addition overflow causes
    ** values to wrap around.
    */
    if( pMem->u.r==ix && ix>SMALLEST_INT64 && ix<LARGEST_INT64 ){
      pMem->u.i = ix;
      MemSetTypeFlag(pMem, MEM_Int);
    }
  }
}

/*
** Convert pMem to type integer.  Invalidate any prior representations.
*/
SQLITE_PRIVATE int sqlite3VdbeMemIntegerify(Mem *pMem){
  assert( pMem!=0 );
  assert( pMem->db==0 || sqlite3_mutex_held(pMem->db->mutex) );
  assert( !sqlite3VdbeMemIsRowSet(pMem) );
  assert( EIGHT_BYTE_ALIGNMENT(pMem) );

  pMem->u.i = sqlite3VdbeIntValue(pMem);
  MemSetTypeFlag(pMem, MEM_Int);
  return SQLITE_OK;
}
