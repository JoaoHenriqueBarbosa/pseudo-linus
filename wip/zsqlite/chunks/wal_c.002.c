static int walIndexPage(
  Wal *pWal,               /* The WAL context */
  int iPage,               /* The page we seek */
  volatile u32 **ppPage    /* Write the page pointer here */
){
  SEH_INJECT_FAULT;
  if( pWal->nWiData<=iPage || (*ppPage = pWal->apWiData[iPage])==0 ){
    return walIndexPageRealloc(pWal, iPage, ppPage);
  }
  return SQLITE_OK;
}

/*
** Return a pointer to the WalCkptInfo structure in the wal-index.
*/
static volatile WalCkptInfo *walCkptInfo(Wal *pWal){
  assert( pWal->nWiData>0 && pWal->apWiData[0] );
  SEH_INJECT_FAULT;
  return (volatile WalCkptInfo*)&(pWal->apWiData[0][sizeof(WalIndexHdr)/2]);
}

/*
** Return a pointer to the WalIndexHdr structure in the wal-index.
*/
static volatile WalIndexHdr *walIndexHdr(Wal *pWal){
  assert( pWal->nWiData>0 && pWal->apWiData[0] );
  SEH_INJECT_FAULT;
  return (volatile WalIndexHdr*)pWal->apWiData[0];
}

/*
** The argument to this macro must be of type u32. On a little-endian
** architecture, it returns the u32 value that results from interpreting
** the 4 bytes as a big-endian value. On a big-endian architecture, it
** returns the value that would be produced by interpreting the 4 bytes
** of the input value as a little-endian integer.
*/
#define BYTESWAP32(x) ( \
    (((x)&0x000000FF)<<24) + (((x)&0x0000FF00)<<8)  \
  + (((x)&0x00FF0000)>>8)  + (((x)&0xFF000000)>>24) \
)

/*
** Generate or extend an 8 byte checksum based on the data in
** array aByte[] and the initial values of aIn[0] and aIn[1] (or
** initial values of 0 and 0 if aIn==NULL).
**
** The checksum is written back into aOut[] before returning.
**
** nByte must be a positive multiple of 8.
*/
static void walChecksumBytes(
  int nativeCksum, /* True for native byte-order, false for non-native */
  u8 *a,           /* Content to be checksummed */
  int nByte,       /* Bytes of content in a[].  Must be a multiple of 8. */
  const u32 *aIn,  /* Initial checksum value input */
  u32 *aOut        /* OUT: Final checksum value output */
){
  u32 s1, s2;
  u32 *aData = (u32 *)a;
  u32 *aEnd = (u32 *)&a[nByte];

  if( aIn ){
    s1 = aIn[0];
    s2 = aIn[1];
  }else{
    s1 = s2 = 0;
  }

  assert( nByte>=8 );
  assert( (nByte&0x00000007)==0 );
  assert( nByte<=65536 );
  assert( nByte%4==0 );

  if( !nativeCksum ){
    do {
      s1 += BYTESWAP32(aData[0]) + s2;
      s2 += BYTESWAP32(aData[1]) + s1;
      aData += 2;
    }while( aData<aEnd );
  }else if( nByte%64==0 ){
    do {
      s1 += *aData++ + s2;
      s2 += *aData++ + s1;
      s1 += *aData++ + s2;
      s2 += *aData++ + s1;
      s1 += *aData++ + s2;
      s2 += *aData++ + s1;
      s1 += *aData++ + s2;
      s2 += *aData++ + s1;
      s1 += *aData++ + s2;
      s2 += *aData++ + s1;
      s1 += *aData++ + s2;
      s2 += *aData++ + s1;
      s1 += *aData++ + s2;
      s2 += *aData++ + s1;
      s1 += *aData++ + s2;
      s2 += *aData++ + s1;
    }while( aData<aEnd );
  }else{
    do {
      s1 += *aData++ + s2;
      s2 += *aData++ + s1;
    }while( aData<aEnd );
  }
  assert( aData==aEnd );

  aOut[0] = s1;
  aOut[1] = s2;
}

/*
** If there is the possibility of concurrent access to the SHM file
** from multiple threads and/or processes, then do a memory barrier.
*/
static void walShmBarrier(Wal *pWal){
  if( pWal->exclusiveMode!=WAL_HEAPMEMORY_MODE ){
    sqlite3OsShmBarrier(pWal->pDbFd);
  }
}

/*
** Add the SQLITE_NO_TSAN as part of the return-type of a function
** definition as a hint that the function contains constructs that
** might give false-positive TSAN warnings.
**
** See tag-20200519-1.
*/
#if defined(__clang__) && !defined(SQLITE_NO_TSAN)
# define SQLITE_NO_TSAN __attribute__((no_sanitize_thread))
#else
# define SQLITE_NO_TSAN
#endif

/*
** Write the header information in pWal->hdr into the wal-index.
**
** The checksum on pWal->hdr is updated before it is written.
*/
static SQLITE_NO_TSAN void walIndexWriteHdr(Wal *pWal){
  volatile WalIndexHdr *aHdr = walIndexHdr(pWal);
  const int nCksum = offsetof(WalIndexHdr, aCksum);

  assert( pWal->writeLock );
  pWal->hdr.isInit = 1;
  pWal->hdr.iVersion = WALINDEX_MAX_VERSION;
  walChecksumBytes(1, (u8*)&pWal->hdr, nCksum, 0, pWal->hdr.aCksum);
  /* Possible TSAN false-positive.  See tag-20200519-1 */
  memcpy((void*)&aHdr[1], (const void*)&pWal->hdr, sizeof(WalIndexHdr));
  walShmBarrier(pWal);
  memcpy((void*)&aHdr[0], (const void*)&pWal->hdr, sizeof(WalIndexHdr));
}

/*
** This function encodes a single frame header and writes it to a buffer
** supplied by the caller. A frame-header is made up of a series of
** 4-byte big-endian integers, as follows:
**
**     0: Page number.
**     4: For commit records, the size of the database image in pages
**        after the commit. For all other records, zero.
**     8: Salt-1 (copied from the wal-header)
**    12: Salt-2 (copied from the wal-header)
**    16: Checksum-1.
**    20: Checksum-2.
*/
static void walEncodeFrame(
  Wal *pWal,                      /* The write-ahead log */
  u32 iPage,                      /* Database page number for frame */
  u32 nTruncate,                  /* New db size (or 0 for non-commit frames) */
  u8 *aData,                      /* Pointer to page data */
  u8 *aFrame                      /* OUT: Write encoded frame here */
){
  int nativeCksum;                /* True for native byte-order checksums */
  u32 *aCksum = pWal->hdr.aFrameCksum;
  assert( WAL_FRAME_HDRSIZE==24 );
  sqlite3Put4byte(&aFrame[0], iPage);
  sqlite3Put4byte(&aFrame[4], nTruncate);
  if( pWal->iReCksum==0 ){
    memcpy(&aFrame[8], pWal->hdr.aSalt, 8);

    nativeCksum = (pWal->hdr.bigEndCksum==SQLITE_BIGENDIAN);
    walChecksumBytes(nativeCksum, aFrame, 8, aCksum, aCksum);
    walChecksumBytes(nativeCksum, aData, pWal->szPage, aCksum, aCksum);

    sqlite3Put4byte(&aFrame[16], aCksum[0]);
    sqlite3Put4byte(&aFrame[20], aCksum[1]);
  }else{
    memset(&aFrame[8], 0, 16);
  }
}

/*
** Check to see if the frame with header in aFrame[] and content
** in aData[] is valid.  If it is a valid frame, fill *piPage and
** *pnTruncate and return true.  Return if the frame is not valid.
*/
static int walDecodeFrame(
  Wal *pWal,                      /* The write-ahead log */
  u32 *piPage,                    /* OUT: Database page number for frame */
  u32 *pnTruncate,                /* OUT: New db size (or 0 if not commit) */
  u8 *aData,                      /* Pointer to page data (for checksum) */
  u8 *aFrame                      /* Frame data */
){
  int nativeCksum;                /* True for native byte-order checksums */
  u32 *aCksum = pWal->hdr.aFrameCksum;
  u32 pgno;                       /* Page number of the frame */
  assert( WAL_FRAME_HDRSIZE==24 );

  /* A frame is only valid if the salt values in the frame-header
  ** match the salt values in the wal-header.
  */
  if( memcmp(&pWal->hdr.aSalt, &aFrame[8], 8)!=0 ){
    return 0;
  }

  /* A frame is only valid if the page number is greater than zero.
  */
  pgno = sqlite3Get4byte(&aFrame[0]);
  if( pgno==0 ){
    return 0;
  }

  /* A frame is only valid if a checksum of the WAL header,
  ** all prior frames, the first 16 bytes of this frame-header,
  ** and the frame-data matches the checksum in the last 8
  ** bytes of this frame-header.
  */
  nativeCksum = (pWal->hdr.bigEndCksum==SQLITE_BIGENDIAN);
  walChecksumBytes(nativeCksum, aFrame, 8, aCksum, aCksum);
  walChecksumBytes(nativeCksum, aData, pWal->szPage, aCksum, aCksum);
  if( aCksum[0]!=sqlite3Get4byte(&aFrame[16])
   || aCksum[1]!=sqlite3Get4byte(&aFrame[20])
  ){
    /* Checksum failed. */
    return 0;
  }

  /* If we reach this point, the frame is valid.  Return the page number
  ** and the new database size.
  */
  *piPage = pgno;
  *pnTruncate = sqlite3Get4byte(&aFrame[4]);
  return 1;
}


#if defined(SQLITE_TEST) && defined(SQLITE_DEBUG)
/*
** Names of locks.  This routine is used to provide debugging output and is not
** a part of an ordinary build.
*/
static const char *walLockName(int lockIdx){
  if( lockIdx==WAL_WRITE_LOCK ){
    return "WRITE-LOCK";
  }else if( lockIdx==WAL_CKPT_LOCK ){
    return "CKPT-LOCK";
  }else if( lockIdx==WAL_RECOVER_LOCK ){
    return "RECOVER-LOCK";
  }else{
    static char zName[15];
    sqlite3_snprintf(sizeof(zName), zName, "READ-LOCK[%d]",
                     lockIdx-WAL_READ_LOCK(0));
    return zName;
  }
}
#endif /*defined(SQLITE_TEST) || defined(SQLITE_DEBUG) */


/*
** Set or release locks on the WAL.  Locks are either shared or exclusive.
** A lock cannot be moved directly between shared and exclusive - it must go
** through the unlocked state first.
**
** In locking_mode=EXCLUSIVE, all of these routines become no-ops.
*/
static int walLockShared(Wal *pWal, int lockIdx){
  int rc;
  if( pWal->exclusiveMode ) return SQLITE_OK;
  rc = sqlite3OsShmLock(pWal->pDbFd, lockIdx, 1,
                        SQLITE_SHM_LOCK | SQLITE_SHM_SHARED);
  WALTRACE(("WAL%p: acquire SHARED-%s %s\n", pWal,
            walLockName(lockIdx), rc ? "failed" : "ok"));
  VVA_ONLY( pWal->lockError = (u8)(rc!=SQLITE_OK && (rc&0xFF)!=SQLITE_BUSY); )
#ifdef SQLITE_USE_SEH
  if( rc==SQLITE_OK ) pWal->lockMask |= (1 << lockIdx);
#endif
  return rc;
}
static void walUnlockShared(Wal *pWal, int lockIdx){
  if( pWal->exclusiveMode ) return;
  (void)sqlite3OsShmLock(pWal->pDbFd, lockIdx, 1,
                         SQLITE_SHM_UNLOCK | SQLITE_SHM_SHARED);
#ifdef SQLITE_USE_SEH
  pWal->lockMask &= ~(1 << lockIdx);
#endif
  WALTRACE(("WAL%p: release SHARED-%s\n", pWal, walLockName(lockIdx)));
}
static int walLockExclusive(Wal *pWal, int lockIdx, int n){
  int rc;
  if( pWal->exclusiveMode ) return SQLITE_OK;
  rc = sqlite3OsShmLock(pWal->pDbFd, lockIdx, n,
                        SQLITE_SHM_LOCK | SQLITE_SHM_EXCLUSIVE);
  WALTRACE(("WAL%p: acquire EXCLUSIVE-%s cnt=%d %s\n", pWal,
            walLockName(lockIdx), n, rc ? "failed" : "ok"));
  VVA_ONLY( pWal->lockError = (u8)(rc!=SQLITE_OK && (rc&0xFF)!=SQLITE_BUSY); )
#ifdef SQLITE_USE_SEH
  if( rc==SQLITE_OK ){
    pWal->lockMask |= (((1<<n)-1) << (SQLITE_SHM_NLOCK+lockIdx));
  }
#endif
  return rc;
}
static void walUnlockExclusive(Wal *pWal, int lockIdx, int n){
  if( pWal->exclusiveMode ) return;
  (void)sqlite3OsShmLock(pWal->pDbFd, lockIdx, n,
                         SQLITE_SHM_UNLOCK | SQLITE_SHM_EXCLUSIVE);
#ifdef SQLITE_USE_SEH
  pWal->lockMask &= ~(((1<<n)-1) << (SQLITE_SHM_NLOCK+lockIdx));
#endif
  WALTRACE(("WAL%p: release EXCLUSIVE-%s cnt=%d\n", pWal,
             walLockName(lockIdx), n));
}

/*
** Compute a hash on a page number.  The resulting hash value must land
** between 0 and (HASHTABLE_NSLOT-1).  The walHashNext() function advances
** the hash to the next value in the event of a collision.
*/
static int walHash(u32 iPage){
  assert( iPage>0 );
  assert( (HASHTABLE_NSLOT & (HASHTABLE_NSLOT-1))==0 );
  return (iPage*HASHTABLE_HASH_1) & (HASHTABLE_NSLOT-1);
}
static int walNextHash(int iPriorHash){
  return (iPriorHash+1)&(HASHTABLE_NSLOT-1);
}

/*
** An instance of the WalHashLoc object is used to describe the location
** of a page hash table in the wal-index.  This becomes the return value
** from walHashGet().
*/
typedef struct WalHashLoc WalHashLoc;
struct WalHashLoc {
  volatile ht_slot *aHash;  /* Start of the wal-index hash table */
  volatile u32 *aPgno;      /* aPgno[1] is the page of first frame indexed */
  u32 iZero;                /* One less than the frame number of first indexed*/
};

/*
** Return pointers to the hash table and page number array stored on
** page iHash of the wal-index. The wal-index is broken into 32KB pages
** numbered starting from 0.
**
** Set output variable pLoc->aHash to point to the start of the hash table
** in the wal-index file. Set pLoc->iZero to one less than the frame
** number of the first frame indexed by this hash table. If a
** slot in the hash table is set to N, it refers to frame number
** (pLoc->iZero+N) in the log.
**
** Finally, set pLoc->aPgno so that pLoc->aPgno[0] is the page number of the
** first frame indexed by the hash table, frame (pLoc->iZero).
*/
static int walHashGet(
  Wal *pWal,                      /* WAL handle */
  int iHash,                      /* Find the iHash'th table */
  WalHashLoc *pLoc                /* OUT: Hash table location */
){
  int rc;                         /* Return code */

  rc = walIndexPage(pWal, iHash, &pLoc->aPgno);
  assert( rc==SQLITE_OK || iHash>0 );

  if( pLoc->aPgno ){
    pLoc->aHash = (volatile ht_slot *)&pLoc->aPgno[HASHTABLE_NPAGE];
    if( iHash==0 ){
      pLoc->aPgno = &pLoc->aPgno[WALINDEX_HDR_SIZE/sizeof(u32)];
      pLoc->iZero = 0;
    }else{
      pLoc->iZero = HASHTABLE_NPAGE_ONE + (iHash-1)*HASHTABLE_NPAGE;
    }
  }else if( NEVER(rc==SQLITE_OK) ){
    rc = SQLITE_ERROR;
  }
  return rc;
}
