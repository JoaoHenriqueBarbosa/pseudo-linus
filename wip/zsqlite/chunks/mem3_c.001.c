
/*
** Free an outstanding memory allocation.
**
** This function assumes that the necessary mutexes, if any, are
** already held by the caller. Hence "Unsafe".
*/
static void memsys3FreeUnsafe(void *pOld){
  Mem3Block *p = (Mem3Block*)pOld;
  int i;
  u32 size, x;
  assert( sqlite3_mutex_held(mem3.mutex) );
  assert( p>mem3.aPool && p<&mem3.aPool[mem3.nPool] );
  i = p - mem3.aPool;
  assert( (mem3.aPool[i-1].u.hdr.size4x&1)==1 );
  size = mem3.aPool[i-1].u.hdr.size4x/4;
  assert( i+size<=mem3.nPool+1 );
  mem3.aPool[i-1].u.hdr.size4x &= ~1;
  mem3.aPool[i+size-1].u.hdr.prevSize = size;
  mem3.aPool[i+size-1].u.hdr.size4x &= ~2;
  memsys3Link(i);

  /* Try to expand the key using the newly freed chunk */
  if( mem3.iKeyBlk ){
    while( (mem3.aPool[mem3.iKeyBlk-1].u.hdr.size4x&2)==0 ){
      size = mem3.aPool[mem3.iKeyBlk-1].u.hdr.prevSize;
      mem3.iKeyBlk -= size;
      mem3.szKeyBlk += size;
      memsys3Unlink(mem3.iKeyBlk);
      x = mem3.aPool[mem3.iKeyBlk-1].u.hdr.size4x & 2;
      mem3.aPool[mem3.iKeyBlk-1].u.hdr.size4x = mem3.szKeyBlk*4 | x;
      mem3.aPool[mem3.iKeyBlk+mem3.szKeyBlk-1].u.hdr.prevSize = mem3.szKeyBlk;
    }
    x = mem3.aPool[mem3.iKeyBlk-1].u.hdr.size4x & 2;
    while( (mem3.aPool[mem3.iKeyBlk+mem3.szKeyBlk-1].u.hdr.size4x&1)==0 ){
      memsys3Unlink(mem3.iKeyBlk+mem3.szKeyBlk);
      mem3.szKeyBlk += mem3.aPool[mem3.iKeyBlk+mem3.szKeyBlk-1].u.hdr.size4x/4;
      mem3.aPool[mem3.iKeyBlk-1].u.hdr.size4x = mem3.szKeyBlk*4 | x;
      mem3.aPool[mem3.iKeyBlk+mem3.szKeyBlk-1].u.hdr.prevSize = mem3.szKeyBlk;
    }
  }
}

/*
** Return the size of an outstanding allocation, in bytes.  The
** size returned omits the 8-byte header overhead.  This only
** works for chunks that are currently checked out.
*/
static int memsys3Size(void *p){
  Mem3Block *pBlock;
  assert( p!=0 );
  pBlock = (Mem3Block*)p;
  assert( (pBlock[-1].u.hdr.size4x&1)!=0 );
  return (pBlock[-1].u.hdr.size4x&~3)*2 - 4;
}

/*
** Round up a request size to the next valid allocation size.
*/
static int memsys3Roundup(int n){
  if( n<=12 ){
    return 12;
  }else{
    return ((n+11)&~7) - 4;
  }
}

/*
** Allocate nBytes of memory.
*/
static void *memsys3Malloc(int nBytes){
  sqlite3_int64 *p;
  assert( nBytes>0 );          /* malloc.c filters out 0 byte requests */
  memsys3Enter();
  p = memsys3MallocUnsafe(nBytes);
  memsys3Leave();
  return (void*)p;
}

/*
** Free memory.
*/
static void memsys3Free(void *pPrior){
  assert( pPrior );
  memsys3Enter();
  memsys3FreeUnsafe(pPrior);
  memsys3Leave();
}

/*
** Change the size of an existing memory allocation
*/
static void *memsys3Realloc(void *pPrior, int nBytes){
  int nOld;
  void *p;
  if( pPrior==0 ){
    return sqlite3_malloc(nBytes);
  }
  if( nBytes<=0 ){
    sqlite3_free(pPrior);
    return 0;
  }
  nOld = memsys3Size(pPrior);
  if( nBytes<=nOld && nBytes>=nOld-128 ){
    return pPrior;
  }
  memsys3Enter();
  p = memsys3MallocUnsafe(nBytes);
  if( p ){
    if( nOld<nBytes ){
      memcpy(p, pPrior, nOld);
    }else{
      memcpy(p, pPrior, nBytes);
    }
    memsys3FreeUnsafe(pPrior);
  }
  memsys3Leave();
  return p;
}

/*
** Initialize this module.
*/
static int memsys3Init(void *NotUsed){
  UNUSED_PARAMETER(NotUsed);
  if( !sqlite3GlobalConfig.pHeap ){
    return SQLITE_ERROR;
  }

  /* Store a pointer to the memory block in global structure mem3. */
  assert( sizeof(Mem3Block)==8 );
  mem3.aPool = (Mem3Block *)sqlite3GlobalConfig.pHeap;
  mem3.nPool = (sqlite3GlobalConfig.nHeap / sizeof(Mem3Block)) - 2;

  /* Initialize the key block. */
  mem3.szKeyBlk = mem3.nPool;
  mem3.mnKeyBlk = mem3.szKeyBlk;
  mem3.iKeyBlk = 1;
  mem3.aPool[0].u.hdr.size4x = (mem3.szKeyBlk<<2) + 2;
  mem3.aPool[mem3.nPool].u.hdr.prevSize = mem3.nPool;
  mem3.aPool[mem3.nPool].u.hdr.size4x = 1;

  return SQLITE_OK;
}

/*
** Deinitialize this module.
*/
static void memsys3Shutdown(void *NotUsed){
  UNUSED_PARAMETER(NotUsed);
  mem3.mutex = 0;
  return;
}



/*
** Open the file indicated and write a log of all unfreed memory
** allocations into that log.
*/
SQLITE_PRIVATE void sqlite3Memsys3Dump(const char *zFilename){
#ifdef SQLITE_DEBUG
  FILE *out;
  u32 i, j;
  u32 size;
  if( zFilename==0 || zFilename[0]==0 ){
    out = stdout;
  }else{
    out = fopen(zFilename, "w");
    if( out==0 ){
      fprintf(stderr, "** Unable to output memory debug output log: %s **\n",
                      zFilename);
      return;
    }
  }
  memsys3Enter();
  fprintf(out, "CHUNKS:\n");
  for(i=1; i<=mem3.nPool; i+=size/4){
    size = mem3.aPool[i-1].u.hdr.size4x;
    if( size/4<=1 ){
      fprintf(out, "%p size error\n", &mem3.aPool[i]);
      assert( 0 );
      break;
    }
    if( (size&1)==0 && mem3.aPool[i+size/4-1].u.hdr.prevSize!=size/4 ){
      fprintf(out, "%p tail size does not match\n", &mem3.aPool[i]);
      assert( 0 );
      break;
    }
    if( ((mem3.aPool[i+size/4-1].u.hdr.size4x&2)>>1)!=(size&1) ){
      fprintf(out, "%p tail checkout bit is incorrect\n", &mem3.aPool[i]);
      assert( 0 );
      break;
    }
    if( size&1 ){
      fprintf(out, "%p %6d bytes checked out\n", &mem3.aPool[i], (size/4)*8-8);
    }else{
      fprintf(out, "%p %6d bytes free%s\n", &mem3.aPool[i], (size/4)*8-8,
                  i==mem3.iKeyBlk ? " **key**" : "");
    }
  }
  for(i=0; i<MX_SMALL-1; i++){
    if( mem3.aiSmall[i]==0 ) continue;
    fprintf(out, "small(%2d):", i);
    for(j = mem3.aiSmall[i]; j>0; j=mem3.aPool[j].u.list.next){
      fprintf(out, " %p(%d)", &mem3.aPool[j],
              (mem3.aPool[j-1].u.hdr.size4x/4)*8-8);
    }
    fprintf(out, "\n");
  }
  for(i=0; i<N_HASH; i++){
    if( mem3.aiHash[i]==0 ) continue;
    fprintf(out, "hash(%2d):", i);
    for(j = mem3.aiHash[i]; j>0; j=mem3.aPool[j].u.list.next){
      fprintf(out, " %p(%d)", &mem3.aPool[j],
              (mem3.aPool[j-1].u.hdr.size4x/4)*8-8);
    }
    fprintf(out, "\n");
  }
  fprintf(out, "key=%d\n", mem3.iKeyBlk);
  fprintf(out, "nowUsed=%d\n", mem3.nPool*8 - mem3.szKeyBlk*8);
  fprintf(out, "mxUsed=%d\n", mem3.nPool*8 - mem3.mnKeyBlk*8);
  sqlite3_mutex_leave(mem3.mutex);
  if( out==stdout ){
    fflush(stdout);
  }else{
    fclose(out);
  }
#else
  UNUSED_PARAMETER(zFilename);
#endif
}

/*
** This routine is the only routine in this file with external
** linkage.
**
** Populate the low-level memory allocation function pointers in
** sqlite3GlobalConfig.m with pointers to the routines in this file. The
** arguments specify the block of memory to manage.
**
** This routine is only called by sqlite3_config(), and therefore
** is not required to be threadsafe (it is not).
*/
SQLITE_PRIVATE const sqlite3_mem_methods *sqlite3MemGetMemsys3(void){
  static const sqlite3_mem_methods mempoolMethods = {
     memsys3Malloc,
     memsys3Free,
     memsys3Realloc,
     memsys3Size,
     memsys3Roundup,
     memsys3Init,
     memsys3Shutdown,
     0
  };
  return &mempoolMethods;
}

#endif /* SQLITE_ENABLE_MEMSYS3 */

