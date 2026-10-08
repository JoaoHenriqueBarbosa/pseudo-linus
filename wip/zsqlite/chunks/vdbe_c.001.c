
/*
** Processing is determine by the affinity parameter:
**
** SQLITE_AFF_INTEGER:
** SQLITE_AFF_REAL:
** SQLITE_AFF_NUMERIC:
**    Try to convert pRec to an integer representation or a
**    floating-point representation if an integer representation
**    is not possible.  Note that the integer representation is
**    always preferred, even if the affinity is REAL, because
**    an integer representation is more space efficient on disk.
**
** SQLITE_AFF_FLEXNUM:
**    If the value is text, then try to convert it into a number of
**    some kind (integer or real) but do not make any other changes.
**
** SQLITE_AFF_TEXT:
**    Convert pRec to a text representation.
**
** SQLITE_AFF_BLOB:
** SQLITE_AFF_NONE:
**    No-op.  pRec is unchanged.
*/
static void applyAffinity(
  Mem *pRec,          /* The value to apply affinity to */
  char affinity,      /* The affinity to be applied */
  u8 enc              /* Use this text encoding */
){
  if( affinity>=SQLITE_AFF_NUMERIC ){
    assert( affinity==SQLITE_AFF_INTEGER || affinity==SQLITE_AFF_REAL
             || affinity==SQLITE_AFF_NUMERIC || affinity==SQLITE_AFF_FLEXNUM );
    if( (pRec->flags & MEM_Int)==0 ){ /*OPTIMIZATION-IF-FALSE*/
      if( (pRec->flags & (MEM_Real|MEM_IntReal))==0 ){
        if( pRec->flags & MEM_Str ) applyNumericAffinity(pRec,1);
      }else if( affinity<=SQLITE_AFF_REAL ){
        sqlite3VdbeIntegerAffinity(pRec);
      }
    }
  }else if( affinity==SQLITE_AFF_TEXT ){
    /* Only attempt the conversion to TEXT if there is an integer or real
    ** representation (blob and NULL do not get converted) but no string
    ** representation.  It would be harmless to repeat the conversion if
    ** there is already a string rep, but it is pointless to waste those
    ** CPU cycles. */
    if( 0==(pRec->flags&MEM_Str) ){ /*OPTIMIZATION-IF-FALSE*/
      if( (pRec->flags&(MEM_Real|MEM_Int|MEM_IntReal)) ){
        testcase( pRec->flags & MEM_Int );
        testcase( pRec->flags & MEM_Real );
        testcase( pRec->flags & MEM_IntReal );
        sqlite3VdbeMemStringify(pRec, enc, 1);
      }
    }
    pRec->flags &= ~(MEM_Real|MEM_Int|MEM_IntReal);
  }
}

/*
** Try to convert the type of a function argument or a result column
** into a numeric representation.  Use either INTEGER or REAL whichever
** is appropriate.  But only do the conversion if it is possible without
** loss of information and return the revised type of the argument.
*/
SQLITE_API int sqlite3_value_numeric_type(sqlite3_value *pVal){
  int eType = sqlite3_value_type(pVal);
  if( eType==SQLITE_TEXT ){
    Mem *pMem = (Mem*)pVal;
    applyNumericAffinity(pMem, 0);
    eType = sqlite3_value_type(pVal);
  }
  return eType;
}

/*
** Exported version of applyAffinity(). This one works on sqlite3_value*,
** not the internal Mem* type.
*/
SQLITE_PRIVATE void sqlite3ValueApplyAffinity(
  sqlite3_value *pVal,
  u8 affinity,
  u8 enc
){
  applyAffinity((Mem *)pVal, affinity, enc);
}

/*
** pMem currently only holds a string type (or maybe a BLOB that we can
** interpret as a string if we want to).  Compute its corresponding
** numeric type, if has one.  Set the pMem->u.r and pMem->u.i fields
** accordingly.
*/
static u16 SQLITE_NOINLINE computeNumericType(Mem *pMem){
  int rc;
  sqlite3_int64 ix;
  assert( (pMem->flags & (MEM_Int|MEM_Real|MEM_IntReal))==0 );
  assert( (pMem->flags & (MEM_Str|MEM_Blob))!=0 );
  if( ExpandBlob(pMem) ){
    pMem->u.i = 0;
    return MEM_Int;
  }
  rc = sqlite3AtoF(pMem->z, &pMem->u.r, pMem->n, pMem->enc);
  if( rc<=0 ){
    if( rc==0 && sqlite3Atoi64(pMem->z, &ix, pMem->n, pMem->enc)<=1 ){
      pMem->u.i = ix;
      return MEM_Int;
    }else{
      return MEM_Real;
    }
  }else if( rc==1 && sqlite3Atoi64(pMem->z, &ix, pMem->n, pMem->enc)==0 ){
    pMem->u.i = ix;
    return MEM_Int;
  }
  return MEM_Real;
}

/*
** Return the numeric type for pMem, either MEM_Int or MEM_Real or both or
** none.
**
** Unlike applyNumericAffinity(), this routine does not modify pMem->flags.
** But it does set pMem->u.r and pMem->u.i appropriately.
*/
static u16 numericType(Mem *pMem){
  assert( (pMem->flags & MEM_Null)==0
       || pMem->db==0 || pMem->db->mallocFailed );
  if( pMem->flags & (MEM_Int|MEM_Real|MEM_IntReal|MEM_Null) ){
    testcase( pMem->flags & MEM_Int );
    testcase( pMem->flags & MEM_Real );
    testcase( pMem->flags & MEM_IntReal );
    return pMem->flags & (MEM_Int|MEM_Real|MEM_IntReal|MEM_Null);
  }
  assert( pMem->flags & (MEM_Str|MEM_Blob) );
  testcase( pMem->flags & MEM_Str );
  testcase( pMem->flags & MEM_Blob );
  return computeNumericType(pMem);
  return 0;
}

#ifdef SQLITE_DEBUG
/*
** Write a nice string representation of the contents of cell pMem
** into buffer zBuf, length nBuf.
*/
SQLITE_PRIVATE void sqlite3VdbeMemPrettyPrint(Mem *pMem, StrAccum *pStr){
  int f = pMem->flags;
  static const char *const encnames[] = {"(X)", "(8)", "(16LE)", "(16BE)"};
  if( f&MEM_Blob ){
    int i;
    char c;
    if( f & MEM_Dyn ){
      c = 'z';
      assert( (f & (MEM_Static|MEM_Ephem))==0 );
    }else if( f & MEM_Static ){
      c = 't';
      assert( (f & (MEM_Dyn|MEM_Ephem))==0 );
    }else if( f & MEM_Ephem ){
      c = 'e';
      assert( (f & (MEM_Static|MEM_Dyn))==0 );
    }else{
      c = 's';
    }
    sqlite3_str_appendf(pStr, "%cx[", c);
    for(i=0; i<25 && i<pMem->n; i++){
      sqlite3_str_appendf(pStr, "%02X", ((int)pMem->z[i] & 0xFF));
    }
    sqlite3_str_appendf(pStr, "|");
    for(i=0; i<25 && i<pMem->n; i++){
      char z = pMem->z[i];
      sqlite3_str_appendchar(pStr, 1, (z<32||z>126)?'.':z);
    }
    sqlite3_str_appendf(pStr,"]");
    if( f & MEM_Zero ){
      sqlite3_str_appendf(pStr, "+%dz",pMem->u.nZero);
    }
  }else if( f & MEM_Str ){
    int j;
    u8 c;
    if( f & MEM_Dyn ){
      c = 'z';
      assert( (f & (MEM_Static|MEM_Ephem))==0 );
    }else if( f & MEM_Static ){
      c = 't';
      assert( (f & (MEM_Dyn|MEM_Ephem))==0 );
    }else if( f & MEM_Ephem ){
      c = 'e';
      assert( (f & (MEM_Static|MEM_Dyn))==0 );
    }else{
      c = 's';
    }
    sqlite3_str_appendf(pStr, " %c%d[", c, pMem->n);
    for(j=0; j<25 && j<pMem->n; j++){
      c = pMem->z[j];
      sqlite3_str_appendchar(pStr, 1, (c>=0x20&&c<=0x7f) ? c : '.');
    }
    sqlite3_str_appendf(pStr, "]%s", encnames[pMem->enc]);
    if( f & MEM_Term ){
      sqlite3_str_appendf(pStr, "(0-term)");
    }
  }
}
#endif

#ifdef SQLITE_DEBUG
/*
** Print the value of a register for tracing purposes:
*/
static void memTracePrint(Mem *p){
  if( p->flags & MEM_Undefined ){
    printf(" undefined");
  }else if( p->flags & MEM_Null ){
    printf(p->flags & MEM_Zero ? " NULL-nochng" : " NULL");
  }else if( (p->flags & (MEM_Int|MEM_Str))==(MEM_Int|MEM_Str) ){
    printf(" si:%lld", p->u.i);
  }else if( (p->flags & (MEM_IntReal))!=0 ){
    printf(" ir:%lld", p->u.i);
  }else if( p->flags & MEM_Int ){
    printf(" i:%lld", p->u.i);
#ifndef SQLITE_OMIT_FLOATING_POINT
  }else if( p->flags & MEM_Real ){
    printf(" r:%.17g", p->u.r);
#endif
  }else if( sqlite3VdbeMemIsRowSet(p) ){
    printf(" (rowset)");
  }else{
    StrAccum acc;
    char zBuf[1000];
    sqlite3StrAccumInit(&acc, 0, zBuf, sizeof(zBuf), 0);
    sqlite3VdbeMemPrettyPrint(p, &acc);
    printf(" %s", sqlite3StrAccumFinish(&acc));
  }
  if( p->flags & MEM_Subtype ) printf(" subtype=0x%02x", p->eSubtype);
}
static void registerTrace(int iReg, Mem *p){
  printf("R[%d] = ", iReg);
  memTracePrint(p);
  if( p->pScopyFrom ){
    printf(" <== R[%d]", (int)(p->pScopyFrom - &p[-iReg]));
  }
  printf("\n");
  sqlite3VdbeCheckMemInvariants(p);
}
/**/ void sqlite3PrintMem(Mem *pMem){
  memTracePrint(pMem);
  printf("\n");
  fflush(stdout);
}
#endif

#ifdef SQLITE_DEBUG
/*
** Show the values of all registers in the virtual machine.  Used for
** interactive debugging.
*/
SQLITE_PRIVATE void sqlite3VdbeRegisterDump(Vdbe *v){
  int i;
  for(i=1; i<v->nMem; i++) registerTrace(i, v->aMem+i);
}
#endif /* SQLITE_DEBUG */


#ifdef SQLITE_DEBUG
#  define REGISTER_TRACE(R,M) if(db->flags&SQLITE_VdbeTrace)registerTrace(R,M)
#else
#  define REGISTER_TRACE(R,M)
#endif

#ifndef NDEBUG
/*
** This function is only called from within an assert() expression. It
** checks that the sqlite3.nTransaction variable is correctly set to
** the number of non-transaction savepoints currently in the
** linked list starting at sqlite3.pSavepoint.
**
** Usage:
**
**     assert( checkSavepointCount(db) );
*/
static int checkSavepointCount(sqlite3 *db){
  int n = 0;
  Savepoint *p;
  for(p=db->pSavepoint; p; p=p->pNext) n++;
  assert( n==(db->nSavepoint + db->isTransactionSavepoint) );
  return 1;
}
#endif

/*
** Return the register of pOp->p2 after first preparing it to be
** overwritten with an integer value.
*/
static SQLITE_NOINLINE Mem *out2PrereleaseWithClear(Mem *pOut){
  sqlite3VdbeMemSetNull(pOut);
  pOut->flags = MEM_Int;
  return pOut;
}
static Mem *out2Prerelease(Vdbe *p, VdbeOp *pOp){
  Mem *pOut;
  assert( pOp->p2>0 );
  assert( pOp->p2<=(p->nMem+1 - p->nCursor) );
  pOut = &p->aMem[pOp->p2];
  memAboutToChange(p, pOut);
  if( VdbeMemDynamic(pOut) ){ /*OPTIMIZATION-IF-FALSE*/
    return out2PrereleaseWithClear(pOut);
  }else{
    pOut->flags = MEM_Int;
    return pOut;
  }
}

/*
** Compute a bloom filter hash using pOp->p4.i registers from aMem[] beginning
** with pOp->p3.  Return the hash.
*/
static u64 filterHash(const Mem *aMem, const Op *pOp){
  int i, mx;
  u64 h = 0;

  assert( pOp->p4type==P4_INT32 );
  for(i=pOp->p3, mx=i+pOp->p4.i; i<mx; i++){
    const Mem *p = &aMem[i];
    if( p->flags & (MEM_Int|MEM_IntReal) ){
      h += p->u.i;
    }else if( p->flags & MEM_Real ){
      h += sqlite3VdbeIntValue(p);
    }else if( p->flags & (MEM_Str|MEM_Blob) ){
      /* All strings have the same hash and all blobs have the same hash,
      ** though, at least, those hashes are different from each other and
      ** from NULL. */
      h += 4093 + (p->flags & (MEM_Str|MEM_Blob));
    }
  }
  return h;
}


/*
** For OP_Column, factor out the case where content is loaded from
** overflow pages, so that the code to implement this case is separate
** the common case where all content fits on the page.  Factoring out
** the code reduces register pressure and helps the common case
** to run faster.
*/
static SQLITE_NOINLINE int vdbeColumnFromOverflow(
  VdbeCursor *pC,       /* The BTree cursor from which we are reading */
  int iCol,             /* The column to read */
  int t,                /* The serial-type code for the column value */
  i64 iOffset,          /* Offset to the start of the content value */
  u32 cacheStatus,      /* Current Vdbe.cacheCtr value */
  u32 colCacheCtr,      /* Current value of the column cache counter */
  Mem *pDest            /* Store the value into this register. */
){
  int rc;
  sqlite3 *db = pDest->db;
  int encoding = pDest->enc;
  int len = sqlite3VdbeSerialTypeLen(t);
  assert( pC->eCurType==CURTYPE_BTREE );
  if( len>db->aLimit[SQLITE_LIMIT_LENGTH] ) return SQLITE_TOOBIG;
  if( len > 4000 && pC->pKeyInfo==0 ){
    /* Cache large column values that are on overflow pages using
    ** an RCStr (reference counted string) so that if they are reloaded,
    ** that do not have to be copied a second time.  The overhead of
    ** creating and managing the cache is such that this is only
    ** profitable for larger TEXT and BLOB values.
    **
    ** Only do this on table-btrees so that writes to index-btrees do not
    ** need to clear the cache.  This buys performance in the common case
    ** in exchange for generality.
    */
    VdbeTxtBlbCache *pCache;
    char *pBuf;
    if( pC->colCache==0 ){
      pC->pCache = sqlite3DbMallocZero(db, sizeof(VdbeTxtBlbCache) );
      if( pC->pCache==0 ) return SQLITE_NOMEM;
      pC->colCache = 1;
    }
    pCache = pC->pCache;
    if( pCache->pCValue==0
     || pCache->iCol!=iCol
     || pCache->cacheStatus!=cacheStatus
     || pCache->colCacheCtr!=colCacheCtr
     || pCache->iOffset!=sqlite3BtreeOffset(pC->uc.pCursor)
    ){
      if( pCache->pCValue ) sqlite3RCStrUnref(pCache->pCValue);
      pBuf = pCache->pCValue = sqlite3RCStrNew( len+3 );
      if( pBuf==0 ) return SQLITE_NOMEM;
      rc = sqlite3BtreePayload(pC->uc.pCursor, iOffset, len, pBuf);
      if( rc ) return rc;
      pBuf[len] = 0;
      pBuf[len+1] = 0;
      pBuf[len+2] = 0;
      pCache->iCol = iCol;
      pCache->cacheStatus = cacheStatus;
      pCache->colCacheCtr = colCacheCtr;
      pCache->iOffset = sqlite3BtreeOffset(pC->uc.pCursor);
    }else{
      pBuf = pCache->pCValue;
    }
    assert( t>=12 );
    sqlite3RCStrRef(pBuf);
    if( t&1 ){
      rc = sqlite3VdbeMemSetStr(pDest, pBuf, len, encoding,
                                sqlite3RCStrUnref);
      pDest->flags |= MEM_Term;
    }else{
      rc = sqlite3VdbeMemSetStr(pDest, pBuf, len, 0,
                                sqlite3RCStrUnref);
    }
  }else{
    rc = sqlite3VdbeMemFromBtree(pC->uc.pCursor, iOffset, len, pDest);
    if( rc ) return rc;
    sqlite3VdbeSerialGet((const u8*)pDest->z, t, pDest);
    if( (t&1)!=0 && encoding==SQLITE_UTF8 ){
      pDest->z[len] = 0;
      pDest->flags |= MEM_Term;
    }
  }
  pDest->flags &= ~MEM_Ephem;
  return rc;
}
