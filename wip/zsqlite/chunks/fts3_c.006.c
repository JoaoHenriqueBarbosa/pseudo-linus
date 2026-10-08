
/*
** Merge two position-lists as required by the NEAR operator. The argument
** position lists correspond to the left and right phrases of an expression
** like:
**
**     "phrase 1" NEAR "phrase number 2"
**
** Position list *pp1 corresponds to the left-hand side of the NEAR
** expression and *pp2 to the right. As usual, the indexes in the position
** lists are the offsets of the last token in each phrase (tokens "1" and "2"
** in the example above).
**
** The output position list - written to *pp - is a copy of *pp2 with those
** entries that are not sufficiently NEAR entries in *pp1 removed.
*/
static int fts3PoslistNearMerge(
  char **pp,                      /* Output buffer */
  char *aTmp,                     /* Temporary buffer space */
  int nRight,                     /* Maximum difference in token positions */
  int nLeft,                      /* Maximum difference in token positions */
  char **pp1,                     /* IN/OUT: Left input list */
  char **pp2                      /* IN/OUT: Right input list */
){
  char *p1 = *pp1;
  char *p2 = *pp2;

  char *pTmp1 = aTmp;
  char *pTmp2;
  char *aTmp2;
  int res = 1;

  fts3PoslistPhraseMerge(&pTmp1, nRight, 0, 0, pp1, pp2);
  aTmp2 = pTmp2 = pTmp1;
  *pp1 = p1;
  *pp2 = p2;
  fts3PoslistPhraseMerge(&pTmp2, nLeft, 1, 0, pp2, pp1);
  if( pTmp1!=aTmp && pTmp2!=aTmp2 ){
    fts3PoslistMerge(pp, &aTmp, &aTmp2);
  }else if( pTmp1!=aTmp ){
    fts3PoslistCopy(pp, &aTmp);
  }else if( pTmp2!=aTmp2 ){
    fts3PoslistCopy(pp, &aTmp2);
  }else{
    res = 0;
  }

  return res;
}

/*
** An instance of this function is used to merge together the (potentially
** large number of) doclists for each term that matches a prefix query.
** See function fts3TermSelectMerge() for details.
*/
typedef struct TermSelect TermSelect;
struct TermSelect {
  char *aaOutput[16];             /* Malloc'd output buffers */
  int anOutput[16];               /* Size each output buffer in bytes */
};

/*
** This function is used to read a single varint from a buffer. Parameter
** pEnd points 1 byte past the end of the buffer. When this function is
** called, if *pp points to pEnd or greater, then the end of the buffer
** has been reached. In this case *pp is set to 0 and the function returns.
**
** If *pp does not point to or past pEnd, then a single varint is read
** from *pp. *pp is then set to point 1 byte past the end of the read varint.
**
** If bDescIdx is false, the value read is added to *pVal before returning.
** If it is true, the value read is subtracted from *pVal before this
** function returns.
*/
static void fts3GetDeltaVarint3(
  char **pp,                      /* IN/OUT: Point to read varint from */
  char *pEnd,                     /* End of buffer */
  int bDescIdx,                   /* True if docids are descending */
  sqlite3_int64 *pVal             /* IN/OUT: Integer value */
){
  if( *pp>=pEnd ){
    *pp = 0;
  }else{
    u64 iVal;
    *pp += sqlite3Fts3GetVarintU(*pp, &iVal);
    if( bDescIdx ){
      *pVal = (i64)((u64)*pVal - iVal);
    }else{
      *pVal = (i64)((u64)*pVal + iVal);
    }
  }
}

/*
** This function is used to write a single varint to a buffer. The varint
** is written to *pp. Before returning, *pp is set to point 1 byte past the
** end of the value written.
**
** If *pbFirst is zero when this function is called, the value written to
** the buffer is that of parameter iVal.
**
** If *pbFirst is non-zero when this function is called, then the value
** written is either (iVal-*piPrev) (if bDescIdx is zero) or (*piPrev-iVal)
** (if bDescIdx is non-zero).
**
** Before returning, this function always sets *pbFirst to 1 and *piPrev
** to the value of parameter iVal.
*/
static void fts3PutDeltaVarint3(
  char **pp,                      /* IN/OUT: Output pointer */
  int bDescIdx,                   /* True for descending docids */
  sqlite3_int64 *piPrev,          /* IN/OUT: Previous value written to list */
  int *pbFirst,                   /* IN/OUT: True after first int written */
  sqlite3_int64 iVal              /* Write this value to the list */
){
  sqlite3_uint64 iWrite;
  if( bDescIdx==0 || *pbFirst==0 ){
    assert_fts3_nc( *pbFirst==0 || iVal>=*piPrev );
    iWrite = (u64)iVal - (u64)*piPrev;
  }else{
    assert_fts3_nc( *piPrev>=iVal );
    iWrite = (u64)*piPrev - (u64)iVal;
  }
  assert( *pbFirst || *piPrev==0 );
  assert_fts3_nc( *pbFirst==0 || iWrite>0 );
  *pp += sqlite3Fts3PutVarint(*pp, iWrite);
  *piPrev = iVal;
  *pbFirst = 1;
}


/*
** This macro is used by various functions that merge doclists. The two
** arguments are 64-bit docid values. If the value of the stack variable
** bDescDoclist is 0 when this macro is invoked, then it returns (i1-i2).
** Otherwise, (i2-i1).
**
** Using this makes it easier to write code that can merge doclists that are
** sorted in either ascending or descending order.
*/
/* #define DOCID_CMP(i1, i2) ((bDescDoclist?-1:1) * (i64)((u64)i1-i2)) */
#define DOCID_CMP(i1, i2) ((bDescDoclist?-1:1) * (i1>i2?1:((i1==i2)?0:-1)))

/*
** This function does an "OR" merge of two doclists (output contains all
** positions contained in either argument doclist). If the docids in the
** input doclists are sorted in ascending order, parameter bDescDoclist
** should be false. If they are sorted in ascending order, it should be
** passed a non-zero value.
**
** If no error occurs, *paOut is set to point at an sqlite3_malloc'd buffer
** containing the output doclist and SQLITE_OK is returned. In this case
** *pnOut is set to the number of bytes in the output doclist.
**
** If an error occurs, an SQLite error code is returned. The output values
** are undefined in this case.
*/
static int fts3DoclistOrMerge(
  int bDescDoclist,               /* True if arguments are desc */
  char *a1, int n1,               /* First doclist */
  char *a2, int n2,               /* Second doclist */
  char **paOut, int *pnOut        /* OUT: Malloc'd doclist */
){
  int rc = SQLITE_OK;
  sqlite3_int64 i1 = 0;
  sqlite3_int64 i2 = 0;
  sqlite3_int64 iPrev = 0;
  char *pEnd1 = &a1[n1];
  char *pEnd2 = &a2[n2];
  char *p1 = a1;
  char *p2 = a2;
  char *p;
  char *aOut;
  int bFirstOut = 0;

  *paOut = 0;
  *pnOut = 0;

  /* Allocate space for the output. Both the input and output doclists
  ** are delta encoded. If they are in ascending order (bDescDoclist==0),
  ** then the first docid in each list is simply encoded as a varint. For
  ** each subsequent docid, the varint stored is the difference between the
  ** current and previous docid (a positive number - since the list is in
  ** ascending order).
  **
  ** The first docid written to the output is therefore encoded using the
  ** same number of bytes as it is in whichever of the input lists it is
  ** read from. And each subsequent docid read from the same input list
  ** consumes either the same or less bytes as it did in the input (since
  ** the difference between it and the previous value in the output must
  ** be a positive value less than or equal to the delta value read from
  ** the input list). The same argument applies to all but the first docid
  ** read from the 'other' list. And to the contents of all position lists
  ** that will be copied and merged from the input to the output.
  **
  ** However, if the first docid copied to the output is a negative number,
  ** then the encoding of the first docid from the 'other' input list may
  ** be larger in the output than it was in the input (since the delta value
  ** may be a larger positive integer than the actual docid).
  **
  ** The space required to store the output is therefore the sum of the
  ** sizes of the two inputs, plus enough space for exactly one of the input
  ** docids to grow.
  **
  ** A symetric argument may be made if the doclists are in descending
  ** order.
  */
  aOut = sqlite3_malloc64((i64)n1+n2+FTS3_VARINT_MAX-1+FTS3_BUFFER_PADDING);
  if( !aOut ) return SQLITE_NOMEM;

  p = aOut;
  fts3GetDeltaVarint3(&p1, pEnd1, 0, &i1);
  fts3GetDeltaVarint3(&p2, pEnd2, 0, &i2);
  while( p1 || p2 ){
    sqlite3_int64 iDiff = DOCID_CMP(i1, i2);

    if( p2 && p1 && iDiff==0 ){
      fts3PutDeltaVarint3(&p, bDescDoclist, &iPrev, &bFirstOut, i1);
      rc = fts3PoslistMerge(&p, &p1, &p2);
      if( rc ) break;
      fts3GetDeltaVarint3(&p1, pEnd1, bDescDoclist, &i1);
      fts3GetDeltaVarint3(&p2, pEnd2, bDescDoclist, &i2);
    }else if( !p2 || (p1 && iDiff<0) ){
      fts3PutDeltaVarint3(&p, bDescDoclist, &iPrev, &bFirstOut, i1);
      fts3PoslistCopy(&p, &p1);
      fts3GetDeltaVarint3(&p1, pEnd1, bDescDoclist, &i1);
    }else{
      fts3PutDeltaVarint3(&p, bDescDoclist, &iPrev, &bFirstOut, i2);
      fts3PoslistCopy(&p, &p2);
      fts3GetDeltaVarint3(&p2, pEnd2, bDescDoclist, &i2);
    }

    assert( (p-aOut)<=((p1?(p1-a1):n1)+(p2?(p2-a2):n2)+FTS3_VARINT_MAX-1) );
  }

  if( rc!=SQLITE_OK ){
    sqlite3_free(aOut);
    p = aOut = 0;
  }else{
    assert( (p-aOut)<=n1+n2+FTS3_VARINT_MAX-1 );
    memset(&aOut[(p-aOut)], 0, FTS3_BUFFER_PADDING);
  }
  *paOut = aOut;
  *pnOut = (int)(p-aOut);
  return rc;
}

/*
** This function does a "phrase" merge of two doclists. In a phrase merge,
** the output contains a copy of each position from the right-hand input
** doclist for which there is a position in the left-hand input doclist
** exactly nDist tokens before it.
**
** If the docids in the input doclists are sorted in ascending order,
** parameter bDescDoclist should be false. If they are sorted in ascending
** order, it should be passed a non-zero value.
**
** The right-hand input doclist is overwritten by this function.
*/
static int fts3DoclistPhraseMerge(
  int bDescDoclist,               /* True if arguments are desc */
  int nDist,                      /* Distance from left to right (1=adjacent) */
  char *aLeft, int nLeft,         /* Left doclist */
  char **paRight, int *pnRight    /* IN/OUT: Right/output doclist */
){
  sqlite3_int64 i1 = 0;
  sqlite3_int64 i2 = 0;
  sqlite3_int64 iPrev = 0;
  char *aRight = *paRight;
  char *pEnd1 = &aLeft[nLeft];
  char *pEnd2 = &aRight[*pnRight];
  char *p1 = aLeft;
  char *p2 = aRight;
  char *p;
  int bFirstOut = 0;
  char *aOut;

  assert( nDist>0 );
  if( bDescDoclist ){
    aOut = sqlite3_malloc64((sqlite3_int64)*pnRight + FTS3_VARINT_MAX);
    if( aOut==0 ) return SQLITE_NOMEM;
  }else{
    aOut = aRight;
  }
  p = aOut;

  fts3GetDeltaVarint3(&p1, pEnd1, 0, &i1);
  fts3GetDeltaVarint3(&p2, pEnd2, 0, &i2);

  while( p1 && p2 ){
    sqlite3_int64 iDiff = DOCID_CMP(i1, i2);
    if( iDiff==0 ){
      char *pSave = p;
      sqlite3_int64 iPrevSave = iPrev;
      int bFirstOutSave = bFirstOut;

      fts3PutDeltaVarint3(&p, bDescDoclist, &iPrev, &bFirstOut, i1);
      if( 0==fts3PoslistPhraseMerge(&p, nDist, 0, 1, &p1, &p2) ){
        p = pSave;
        iPrev = iPrevSave;
        bFirstOut = bFirstOutSave;
      }
      fts3GetDeltaVarint3(&p1, pEnd1, bDescDoclist, &i1);
      fts3GetDeltaVarint3(&p2, pEnd2, bDescDoclist, &i2);
    }else if( iDiff<0 ){
      fts3PoslistCopy(0, &p1);
      fts3GetDeltaVarint3(&p1, pEnd1, bDescDoclist, &i1);
    }else{
      fts3PoslistCopy(0, &p2);
      fts3GetDeltaVarint3(&p2, pEnd2, bDescDoclist, &i2);
    }
  }

  *pnRight = (int)(p - aOut);
  if( bDescDoclist ){
    sqlite3_free(aRight);
    *paRight = aOut;
  }

  return SQLITE_OK;
}

/*
** Argument pList points to a position list nList bytes in size. This
** function checks to see if the position list contains any entries for
** a token in position 0 (of any column). If so, it writes argument iDelta
** to the output buffer pOut, followed by a position list consisting only
** of the entries from pList at position 0, and terminated by an 0x00 byte.
** The value returned is the number of bytes written to pOut (if any).
*/
SQLITE_PRIVATE int sqlite3Fts3FirstFilter(
  sqlite3_int64 iDelta,           /* Varint that may be written to pOut */
  char *pList,                    /* Position list (no 0x00 term) */
  int nList,                      /* Size of pList in bytes */
  char *pOut                      /* Write output here */
){
  int nOut = 0;
  int bWritten = 0;               /* True once iDelta has been written */
  char *p = pList;
  char *pEnd = &pList[nList];

  if( *p!=0x01 ){
    if( *p==0x02 ){
      nOut += sqlite3Fts3PutVarint(&pOut[nOut], iDelta);
      pOut[nOut++] = 0x02;
      bWritten = 1;
    }
    fts3ColumnlistCopy(0, &p);
  }

  while( p<pEnd ){
    sqlite3_int64 iCol;
    p++;
    p += sqlite3Fts3GetVarint(p, &iCol);
    if( *p==0x02 ){
      if( bWritten==0 ){
        nOut += sqlite3Fts3PutVarint(&pOut[nOut], iDelta);
        bWritten = 1;
      }
      pOut[nOut++] = 0x01;
      nOut += sqlite3Fts3PutVarint(&pOut[nOut], iCol);
      pOut[nOut++] = 0x02;
    }
    fts3ColumnlistCopy(0, &p);
  }
  if( bWritten ){
    pOut[nOut++] = 0x00;
  }

  return nOut;
}
