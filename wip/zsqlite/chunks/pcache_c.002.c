
/*
** Close a cache.
*/
SQLITE_PRIVATE void sqlite3PcacheClose(PCache *pCache){
  assert( pCache->pCache!=0 );
  pcacheTrace(("%p.CLOSE\n",pCache));
  sqlite3GlobalConfig.pcache2.xDestroy(pCache->pCache);
}

/*
** Discard the contents of the cache.
*/
SQLITE_PRIVATE void sqlite3PcacheClear(PCache *pCache){
  sqlite3PcacheTruncate(pCache, 0);
}

/*
** Merge two lists of pages connected by pDirty and in pgno order.
** Do not bother fixing the pDirtyPrev pointers.
*/
static PgHdr *pcacheMergeDirtyList(PgHdr *pA, PgHdr *pB){
  PgHdr result, *pTail;
  pTail = &result;
  assert( pA!=0 && pB!=0 );
  for(;;){
    if( pA->pgno<pB->pgno ){
      pTail->pDirty = pA;
      pTail = pA;
      pA = pA->pDirty;
      if( pA==0 ){
        pTail->pDirty = pB;
        break;
      }
    }else{
      pTail->pDirty = pB;
      pTail = pB;
      pB = pB->pDirty;
      if( pB==0 ){
        pTail->pDirty = pA;
        break;
      }
    }
  }
  return result.pDirty;
}

/*
** Sort the list of pages in ascending order by pgno.  Pages are
** connected by pDirty pointers.  The pDirtyPrev pointers are
** corrupted by this sort.
**
** Since there cannot be more than 2^31 distinct pages in a database,
** there cannot be more than 31 buckets required by the merge sorter.
** One extra bucket is added to catch overflow in case something
** ever changes to make the previous sentence incorrect.
*/
#define N_SORT_BUCKET  32
static PgHdr *pcacheSortDirtyList(PgHdr *pIn){
  PgHdr *a[N_SORT_BUCKET], *p;
  int i;
  memset(a, 0, sizeof(a));
  while( pIn ){
    p = pIn;
    pIn = p->pDirty;
    p->pDirty = 0;
    for(i=0; ALWAYS(i<N_SORT_BUCKET-1); i++){
      if( a[i]==0 ){
        a[i] = p;
        break;
      }else{
        p = pcacheMergeDirtyList(a[i], p);
        a[i] = 0;
      }
    }
    if( NEVER(i==N_SORT_BUCKET-1) ){
      /* To get here, there need to be 2^(N_SORT_BUCKET) elements in
      ** the input list.  But that is impossible.
      */
      a[i] = pcacheMergeDirtyList(a[i], p);
    }
  }
  p = a[0];
  for(i=1; i<N_SORT_BUCKET; i++){
    if( a[i]==0 ) continue;
    p = p ? pcacheMergeDirtyList(p, a[i]) : a[i];
  }
  return p;
}

/*
** Return a list of all dirty pages in the cache, sorted by page number.
*/
SQLITE_PRIVATE PgHdr *sqlite3PcacheDirtyList(PCache *pCache){
  PgHdr *p;
  for(p=pCache->pDirty; p; p=p->pDirtyNext){
    p->pDirty = p->pDirtyNext;
  }
  return pcacheSortDirtyList(pCache->pDirty);
}

/*
** Return the total number of references to all pages held by the cache.
**
** This is not the total number of pages referenced, but the sum of the
** reference count for all pages.
*/
SQLITE_PRIVATE i64 sqlite3PcacheRefCount(PCache *pCache){
  return pCache->nRefSum;
}

/*
** Return the number of references to the page supplied as an argument.
*/
SQLITE_PRIVATE i64 sqlite3PcachePageRefcount(PgHdr *p){
  return p->nRef;
}

/*
** Return the total number of pages in the cache.
*/
SQLITE_PRIVATE int sqlite3PcachePagecount(PCache *pCache){
  assert( pCache->pCache!=0 );
  return sqlite3GlobalConfig.pcache2.xPagecount(pCache->pCache);
}

#ifdef SQLITE_TEST
/*
** Get the suggested cache-size value.
*/
SQLITE_PRIVATE int sqlite3PcacheGetCachesize(PCache *pCache){
  return numberOfCachePages(pCache);
}
#endif

/*
** Set the suggested cache-size value.
*/
SQLITE_PRIVATE void sqlite3PcacheSetCachesize(PCache *pCache, int mxPage){
  assert( pCache->pCache!=0 );
  pCache->szCache = mxPage;
  sqlite3GlobalConfig.pcache2.xCachesize(pCache->pCache,
                                         numberOfCachePages(pCache));
}

/*
** Set the suggested cache-spill value.  Make no changes if if the
** argument is zero.  Return the effective cache-spill size, which will
** be the larger of the szSpill and szCache.
*/
SQLITE_PRIVATE int sqlite3PcacheSetSpillsize(PCache *p, int mxPage){
  int res;
  assert( p->pCache!=0 );
  if( mxPage ){
    if( mxPage<0 ){
      mxPage = (int)((-1024*(i64)mxPage)/(p->szPage+p->szExtra));
    }
    p->szSpill = mxPage;
  }
  res = numberOfCachePages(p);
  if( res<p->szSpill ) res = p->szSpill;
  return res;
}

/*
** Free up as much memory as possible from the page cache.
*/
SQLITE_PRIVATE void sqlite3PcacheShrink(PCache *pCache){
  assert( pCache->pCache!=0 );
  sqlite3GlobalConfig.pcache2.xShrink(pCache->pCache);
}

/*
** Return the size of the header added by this middleware layer
** in the page-cache hierarchy.
*/
SQLITE_PRIVATE int sqlite3HeaderSizePcache(void){ return ROUND8(sizeof(PgHdr)); }

/*
** Return the number of dirty pages currently in the cache, as a percentage
** of the configured cache size.
*/
SQLITE_PRIVATE int sqlite3PCachePercentDirty(PCache *pCache){
  PgHdr *pDirty;
  int nDirty = 0;
  int nCache = numberOfCachePages(pCache);
  for(pDirty=pCache->pDirty; pDirty; pDirty=pDirty->pDirtyNext) nDirty++;
  return nCache ? (int)(((i64)nDirty * 100) / nCache) : 0;
}

#ifdef SQLITE_DIRECT_OVERFLOW_READ
/*
** Return true if there are one or more dirty pages in the cache. Else false.
*/
SQLITE_PRIVATE int sqlite3PCacheIsDirty(PCache *pCache){
  return (pCache->pDirty!=0);
}
#endif

#if defined(SQLITE_CHECK_PAGES) || defined(SQLITE_DEBUG)
/*
** For all dirty pages currently in the cache, invoke the specified
** callback. This is only used if the SQLITE_CHECK_PAGES macro is
** defined.
*/
SQLITE_PRIVATE void sqlite3PcacheIterateDirty(PCache *pCache, void (*xIter)(PgHdr *)){
  PgHdr *pDirty;
  for(pDirty=pCache->pDirty; pDirty; pDirty=pDirty->pDirtyNext){
    xIter(pDirty);
  }
}
#endif

