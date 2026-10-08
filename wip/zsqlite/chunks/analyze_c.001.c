#endif

/*
** Reclaim all memory of a StatAccum structure.
*/
static void statAccumDestructor(void *pOld){
  StatAccum *p = (StatAccum*)pOld;
#ifdef SQLITE_ENABLE_STAT4
  if( p->mxSample ){
    int i;
    for(i=0; i<p->nCol; i++) sampleClear(p->db, p->aBest+i);
    for(i=0; i<p->mxSample; i++) sampleClear(p->db, p->a+i);
    sampleClear(p->db, &p->current);
  }
#endif
  sqlite3DbFree(p->db, p);
}

/*
** Implementation of the stat_init(N,K,C,L) SQL function. The four parameters
** are:
**     N:    The number of columns in the index including the rowid/pk (note 1)
**     K:    The number of columns in the index excluding the rowid/pk.
**     C:    Estimated number of rows in the index
**     L:    A limit on the number of rows to scan, or 0 for no-limit
**
** Note 1:  In the special case of the covering index that implements a
** WITHOUT ROWID table, N is the number of PRIMARY KEY columns, not the
** total number of columns in the table.
**
** For indexes on ordinary rowid tables, N==K+1.  But for indexes on
** WITHOUT ROWID tables, N=K+P where P is the number of columns in the
** PRIMARY KEY of the table.  The covering index that implements the
** original WITHOUT ROWID table as N==K as a special case.
**
** This routine allocates the StatAccum object in heap memory. The return
** value is a pointer to the StatAccum object.  The datatype of the
** return value is BLOB, but it is really just a pointer to the StatAccum
** object.
*/
static void statInit(
  sqlite3_context *context,
  int argc,
  sqlite3_value **argv
){
  StatAccum *p;
  int nCol;                       /* Number of columns in index being sampled */
  int nKeyCol;                    /* Number of key columns */
  int nColUp;                     /* nCol rounded up for alignment */
  int n;                          /* Bytes of space to allocate */
  sqlite3 *db = sqlite3_context_db_handle(context);   /* Database connection */
#ifdef SQLITE_ENABLE_STAT4
  /* Maximum number of samples.  0 if STAT4 data is not collected */
  int mxSample = OptimizationEnabled(db,SQLITE_Stat4) ?SQLITE_STAT4_SAMPLES :0;
#endif

  /* Decode the three function arguments */
  UNUSED_PARAMETER(argc);
  nCol = sqlite3_value_int(argv[0]);
  assert( nCol>0 );
  nColUp = sizeof(tRowcnt)<8 ? (nCol+1)&~1 : nCol;
  nKeyCol = sqlite3_value_int(argv[1]);
  assert( nKeyCol<=nCol );
  assert( nKeyCol>0 );

  /* Allocate the space required for the StatAccum object */
  n = sizeof(*p)
    + sizeof(tRowcnt)*nColUp;                    /* StatAccum.anDLt */
#ifdef SQLITE_ENABLE_STAT4
  n += sizeof(tRowcnt)*nColUp;                   /* StatAccum.anEq */
  if( mxSample ){
    n += sizeof(tRowcnt)*nColUp                  /* StatAccum.anLt */
      + sizeof(StatSample)*(nCol+mxSample)       /* StatAccum.aBest[], a[] */
      + sizeof(tRowcnt)*3*nColUp*(nCol+mxSample);
  }
#endif
  p = sqlite3DbMallocZero(db, n);
  if( p==0 ){
    sqlite3_result_error_nomem(context);
    return;
  }

  p->db = db;
  p->nEst = sqlite3_value_int64(argv[2]);
  p->nRow = 0;
  p->nLimit = sqlite3_value_int64(argv[3]);
  p->nCol = nCol;
  p->nKeyCol = nKeyCol;
  p->nSkipAhead = 0;
  p->current.anDLt = (tRowcnt*)&p[1];

#ifdef SQLITE_ENABLE_STAT4
  p->current.anEq = &p->current.anDLt[nColUp];
  p->mxSample = p->nLimit==0 ? mxSample : 0;
  if( mxSample ){
    u8 *pSpace;                     /* Allocated space not yet assigned */
    int i;                          /* Used to iterate through p->aSample[] */

    p->iGet = -1;
    p->nPSample = (tRowcnt)(p->nEst/(mxSample/3+1) + 1);
    p->current.anLt = &p->current.anEq[nColUp];
    p->iPrn = 0x689e962d*(u32)nCol ^ 0xd0944565*(u32)sqlite3_value_int(argv[2]);

    /* Set up the StatAccum.a[] and aBest[] arrays */
    p->a = (struct StatSample*)&p->current.anLt[nColUp];
    p->aBest = &p->a[mxSample];
    pSpace = (u8*)(&p->a[mxSample+nCol]);
    for(i=0; i<(mxSample+nCol); i++){
      p->a[i].anEq = (tRowcnt *)pSpace; pSpace += (sizeof(tRowcnt) * nColUp);
      p->a[i].anLt = (tRowcnt *)pSpace; pSpace += (sizeof(tRowcnt) * nColUp);
      p->a[i].anDLt = (tRowcnt *)pSpace; pSpace += (sizeof(tRowcnt) * nColUp);
    }
    assert( (pSpace - (u8*)p)==n );

    for(i=0; i<nCol; i++){
      p->aBest[i].iCol = i;
    }
  }
#endif

  /* Return a pointer to the allocated object to the caller.  Note that
  ** only the pointer (the 2nd parameter) matters.  The size of the object
  ** (given by the 3rd parameter) is never used and can be any positive
  ** value. */
  sqlite3_result_blob(context, p, sizeof(*p), statAccumDestructor);
}
static const FuncDef statInitFuncdef = {
  4,               /* nArg */
  SQLITE_UTF8,     /* funcFlags */
  0,               /* pUserData */
  0,               /* pNext */
  statInit,        /* xSFunc */
  0,               /* xFinalize */
  0, 0,            /* xValue, xInverse */
  "stat_init",     /* zName */
  {0}
};

#ifdef SQLITE_ENABLE_STAT4
/*
** pNew and pOld are both candidate non-periodic samples selected for
** the same column (pNew->iCol==pOld->iCol). Ignoring this column and
** considering only any trailing columns and the sample hash value, this
** function returns true if sample pNew is to be preferred over pOld.
** In other words, if we assume that the cardinalities of the selected
** column for pNew and pOld are equal, is pNew to be preferred over pOld.
**
** This function assumes that for each argument sample, the contents of
** the anEq[] array from pSample->anEq[pSample->iCol+1] onwards are valid.
*/
static int sampleIsBetterPost(
  StatAccum *pAccum,
  StatSample *pNew,
  StatSample *pOld
){
  int nCol = pAccum->nCol;
  int i;
  assert( pNew->iCol==pOld->iCol );
  for(i=pNew->iCol+1; i<nCol; i++){
    if( pNew->anEq[i]>pOld->anEq[i] ) return 1;
    if( pNew->anEq[i]<pOld->anEq[i] ) return 0;
  }
  if( pNew->iHash>pOld->iHash ) return 1;
  return 0;
}
#endif

#ifdef SQLITE_ENABLE_STAT4
/*
** Return true if pNew is to be preferred over pOld.
**
** This function assumes that for each argument sample, the contents of
** the anEq[] array from pSample->anEq[pSample->iCol] onwards are valid.
*/
static int sampleIsBetter(
  StatAccum *pAccum,
  StatSample *pNew,
  StatSample *pOld
){
  tRowcnt nEqNew = pNew->anEq[pNew->iCol];
  tRowcnt nEqOld = pOld->anEq[pOld->iCol];

  assert( pOld->isPSample==0 && pNew->isPSample==0 );
  assert( IsStat4 || (pNew->iCol==0 && pOld->iCol==0) );

  if( (nEqNew>nEqOld) ) return 1;
  if( nEqNew==nEqOld ){
    if( pNew->iCol<pOld->iCol ) return 1;
    return (pNew->iCol==pOld->iCol && sampleIsBetterPost(pAccum, pNew, pOld));
  }
  return 0;
}

/*
** Copy the contents of sample *pNew into the p->a[] array. If necessary,
** remove the least desirable sample from p->a[] to make room.
*/
static void sampleInsert(StatAccum *p, StatSample *pNew, int nEqZero){
  StatSample *pSample = 0;
  int i;

  assert( IsStat4 || nEqZero==0 );

  /* StatAccum.nMaxEqZero is set to the maximum number of leading 0
  ** values in the anEq[] array of any sample in StatAccum.a[]. In
  ** other words, if nMaxEqZero is n, then it is guaranteed that there
  ** are no samples with StatSample.anEq[m]==0 for (m>=n). */
  if( nEqZero>p->nMaxEqZero ){
    p->nMaxEqZero = nEqZero;
  }
  if( pNew->isPSample==0 ){
    StatSample *pUpgrade = 0;
    assert( pNew->anEq[pNew->iCol]>0 );

    /* This sample is being added because the prefix that ends in column
    ** iCol occurs many times in the table. However, if we have already
    ** added a sample that shares this prefix, there is no need to add
    ** this one. Instead, upgrade the priority of the highest priority
    ** existing sample that shares this prefix.  */
    for(i=p->nSample-1; i>=0; i--){
      StatSample *pOld = &p->a[i];
      if( pOld->anEq[pNew->iCol]==0 ){
        if( pOld->isPSample ) return;
        assert( pOld->iCol>pNew->iCol );
        assert( sampleIsBetter(p, pNew, pOld) );
        if( pUpgrade==0 || sampleIsBetter(p, pOld, pUpgrade) ){
          pUpgrade = pOld;
        }
      }
    }
    if( pUpgrade ){
      pUpgrade->iCol = pNew->iCol;
      pUpgrade->anEq[pUpgrade->iCol] = pNew->anEq[pUpgrade->iCol];
      goto find_new_min;
    }
  }

  /* If necessary, remove sample iMin to make room for the new sample. */
  if( p->nSample>=p->mxSample ){
    StatSample *pMin = &p->a[p->iMin];
    tRowcnt *anEq = pMin->anEq;
    tRowcnt *anLt = pMin->anLt;
    tRowcnt *anDLt = pMin->anDLt;
    sampleClear(p->db, pMin);
    memmove(pMin, &pMin[1], sizeof(p->a[0])*(p->nSample-p->iMin-1));
    pSample = &p->a[p->nSample-1];
    pSample->nRowid = 0;
    pSample->anEq = anEq;
    pSample->anDLt = anDLt;
    pSample->anLt = anLt;
    p->nSample = p->mxSample-1;
  }

  /* The "rows less-than" for the rowid column must be greater than that
  ** for the last sample in the p->a[] array. Otherwise, the samples would
  ** be out of order. */
  assert( p->nSample==0
       || pNew->anLt[p->nCol-1] > p->a[p->nSample-1].anLt[p->nCol-1] );

  /* Insert the new sample */
  pSample = &p->a[p->nSample];
  sampleCopy(p, pSample, pNew);
  p->nSample++;

  /* Zero the first nEqZero entries in the anEq[] array. */
  memset(pSample->anEq, 0, sizeof(tRowcnt)*nEqZero);

find_new_min:
  if( p->nSample>=p->mxSample ){
    int iMin = -1;
    for(i=0; i<p->mxSample; i++){
      if( p->a[i].isPSample ) continue;
      if( iMin<0 || sampleIsBetter(p, &p->a[iMin], &p->a[i]) ){
        iMin = i;
      }
    }
    assert( iMin>=0 );
    p->iMin = iMin;
  }
}
#endif /* SQLITE_ENABLE_STAT4 */

#ifdef SQLITE_ENABLE_STAT4
/*
** Field iChng of the index being scanned has changed. So at this point
** p->current contains a sample that reflects the previous row of the
** index. The value of anEq[iChng] and subsequent anEq[] elements are
** correct at this point.
*/
static void samplePushPrevious(StatAccum *p, int iChng){
  int i;

  /* Check if any samples from the aBest[] array should be pushed
  ** into IndexSample.a[] at this point.  */
  for(i=(p->nCol-2); i>=iChng; i--){
    StatSample *pBest = &p->aBest[i];
    pBest->anEq[i] = p->current.anEq[i];
    if( p->nSample<p->mxSample || sampleIsBetter(p, pBest, &p->a[p->iMin]) ){
      sampleInsert(p, pBest, i);
    }
  }

  /* Check that no sample contains an anEq[] entry with an index of
  ** p->nMaxEqZero or greater set to zero. */
  for(i=p->nSample-1; i>=0; i--){
    int j;
    for(j=p->nMaxEqZero; j<p->nCol; j++) assert( p->a[i].anEq[j]>0 );
  }

  /* Update the anEq[] fields of any samples already collected. */
  if( iChng<p->nMaxEqZero ){
    for(i=p->nSample-1; i>=0; i--){
      int j;
      for(j=iChng; j<p->nCol; j++){
        if( p->a[i].anEq[j]==0 ) p->a[i].anEq[j] = p->current.anEq[j];
      }
    }
    p->nMaxEqZero = iChng;
  }
}
#endif /* SQLITE_ENABLE_STAT4 */

/*
** Implementation of the stat_push SQL function:  stat_push(P,C,R)
** Arguments:
**
**    P     Pointer to the StatAccum object created by stat_init()
**    C     Index of left-most column to differ from previous row
**    R     Rowid for the current row.  Might be a key record for
**          WITHOUT ROWID tables.
**
** The purpose of this routine is to collect statistical data and/or
** samples from the index being analyzed into the StatAccum object.
** The stat_get() SQL function will be used afterwards to
** retrieve the information gathered.
**
** This SQL function usually returns NULL, but might return an integer
** if it wants the byte-code to do special processing.
**
** The R parameter is only used for STAT4
*/
static void statPush(
  sqlite3_context *context,
  int argc,
  sqlite3_value **argv
){
  int i;

  /* The three function arguments */
  StatAccum *p = (StatAccum*)sqlite3_value_blob(argv[0]);
  int iChng = sqlite3_value_int(argv[1]);

  UNUSED_PARAMETER( argc );
  UNUSED_PARAMETER( context );
  assert( p->nCol>0 );
  assert( iChng<p->nCol );

  if( p->nRow==0 ){
    /* This is the first call to this function. Do initialization. */
#ifdef SQLITE_ENABLE_STAT4
    for(i=0; i<p->nCol; i++) p->current.anEq[i] = 1;
#endif
  }else{
    /* Second and subsequent calls get processed here */
#ifdef SQLITE_ENABLE_STAT4
    if( p->mxSample ) samplePushPrevious(p, iChng);
#endif

    /* Update anDLt[], anLt[] and anEq[] to reflect the values that apply
    ** to the current row of the index. */
#ifdef SQLITE_ENABLE_STAT4
    for(i=0; i<iChng; i++){
      p->current.anEq[i]++;
    }
#endif
    for(i=iChng; i<p->nCol; i++){
      p->current.anDLt[i]++;
#ifdef SQLITE_ENABLE_STAT4
      if( p->mxSample ) p->current.anLt[i] += p->current.anEq[i];
      p->current.anEq[i] = 1;
#endif
    }
  }

  p->nRow++;
#ifdef SQLITE_ENABLE_STAT4
  if( p->mxSample ){
    tRowcnt nLt;
    if( sqlite3_value_type(argv[2])==SQLITE_INTEGER ){
      sampleSetRowidInt64(p->db, &p->current, sqlite3_value_int64(argv[2]));
    }else{
      sampleSetRowid(p->db, &p->current, sqlite3_value_bytes(argv[2]),
                                         sqlite3_value_blob(argv[2]));
    }
    p->current.iHash = p->iPrn = p->iPrn*1103515245 + 12345;

    nLt = p->current.anLt[p->nCol-1];
    /* Check if this is to be a periodic sample. If so, add it. */
    if( (nLt/p->nPSample)!=(nLt+1)/p->nPSample ){
      p->current.isPSample = 1;
      p->current.iCol = 0;
      sampleInsert(p, &p->current, p->nCol-1);
      p->current.isPSample = 0;
    }

    /* Update the aBest[] array. */
    for(i=0; i<(p->nCol-1); i++){
      p->current.iCol = i;
      if( i>=iChng || sampleIsBetterPost(p, &p->current, &p->aBest[i]) ){
        sampleCopy(p, &p->aBest[i], &p->current);
      }
    }
  }else
#endif
  if( p->nLimit && p->nRow>(tRowcnt)p->nLimit*(p->nSkipAhead+1) ){
    p->nSkipAhead++;
    sqlite3_result_int(context, p->current.anDLt[0]>0);
  }
}
