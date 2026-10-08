
/*
** Mark this cursor as an incremental blob cursor.
*/
SQLITE_PRIVATE void sqlite3BtreeIncrblobCursor(BtCursor *pCur){
  pCur->curFlags |= BTCF_Incrblob;
  pCur->pBtree->hasIncrblobCur = 1;
}
#endif

/*
** Set both the "read version" (single byte at byte offset 18) and
** "write version" (single byte at byte offset 19) fields in the database
** header to iVersion.
*/
SQLITE_PRIVATE int sqlite3BtreeSetVersion(Btree *pBtree, int iVersion){
  BtShared *pBt = pBtree->pBt;
  int rc;                         /* Return code */

  assert( iVersion==1 || iVersion==2 );

  /* If setting the version fields to 1, do not automatically open the
  ** WAL connection, even if the version fields are currently set to 2.
  */
  pBt->btsFlags &= ~BTS_NO_WAL;
  if( iVersion==1 ) pBt->btsFlags |= BTS_NO_WAL;

  rc = sqlite3BtreeBeginTrans(pBtree, 0, 0);
  if( rc==SQLITE_OK ){
    u8 *aData = pBt->pPage1->aData;
    if( aData[18]!=(u8)iVersion || aData[19]!=(u8)iVersion ){
      rc = sqlite3BtreeBeginTrans(pBtree, 2, 0);
      if( rc==SQLITE_OK ){
        rc = sqlite3PagerWrite(pBt->pPage1->pDbPage);
        if( rc==SQLITE_OK ){
          aData[18] = (u8)iVersion;
          aData[19] = (u8)iVersion;
        }
      }
    }
  }

  pBt->btsFlags &= ~BTS_NO_WAL;
  return rc;
}

/*
** Return true if the cursor has a hint specified.  This routine is
** only used from within assert() statements
*/
SQLITE_PRIVATE int sqlite3BtreeCursorHasHint(BtCursor *pCsr, unsigned int mask){
  return (pCsr->hints & mask)!=0;
}

/*
** Return true if the given Btree is read-only.
*/
SQLITE_PRIVATE int sqlite3BtreeIsReadonly(Btree *p){
  return (p->pBt->btsFlags & BTS_READ_ONLY)!=0;
}

/*
** Return the size of the header added to each page by this module.
*/
SQLITE_PRIVATE int sqlite3HeaderSizeBtree(void){ return ROUND8(sizeof(MemPage)); }

/*
** If no transaction is active and the database is not a temp-db, clear
** the in-memory pager cache.
*/
SQLITE_PRIVATE void sqlite3BtreeClearCache(Btree *p){
  BtShared *pBt = p->pBt;
  if( pBt->inTransaction==TRANS_NONE ){
    sqlite3PagerClearCache(pBt->pPager);
  }
}

#if !defined(SQLITE_OMIT_SHARED_CACHE)
/*
** Return true if the Btree passed as the only argument is sharable.
*/
SQLITE_PRIVATE int sqlite3BtreeSharable(Btree *p){
  return p->sharable;
}

/*
** Return the number of connections to the BtShared object accessed by
** the Btree handle passed as the only argument. For private caches
** this is always 1. For shared caches it may be 1 or greater.
*/
SQLITE_PRIVATE int sqlite3BtreeConnectionCount(Btree *p){
  testcase( p->sharable );
  return p->pBt->nRef;
}
#endif

