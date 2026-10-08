
/*
** If pParse->aBlob is not previously editable (because it is taken
** from sqlite3_value_blob(), as indicated by the fact that
** pParse->nBlobAlloc==0 and pParse->nBlob>0) then make it editable
** by making a copy into space obtained from malloc.
**
** Return true on success.  Return false on OOM.
*/
static int jsonBlobMakeEditable(JsonParse *pParse, u32 nExtra){
  u8 *aOld;
  u32 nSize;
  assert( !pParse->bReadOnly );
  if( pParse->oom ) return 0;
  if( pParse->nBlobAlloc>0 ) return 1;
  aOld = pParse->aBlob;
  nSize = pParse->nBlob + nExtra;
  pParse->aBlob = 0;
  if( jsonBlobExpand(pParse, nSize) ){
    return 0;
  }
  assert( pParse->nBlobAlloc >= pParse->nBlob + nExtra );
  memcpy(pParse->aBlob, aOld, pParse->nBlob);
  return 1;
}

/* Expand pParse->aBlob and append one bytes.
*/
static SQLITE_NOINLINE void jsonBlobExpandAndAppendOneByte(
  JsonParse *pParse,
  u8 c
){
  jsonBlobExpand(pParse, pParse->nBlob+1);
  if( pParse->oom==0 ){
    assert( pParse->nBlob+1<=pParse->nBlobAlloc );
    pParse->aBlob[pParse->nBlob++] = c;
  }
}

/* Append a single character.
*/
static void jsonBlobAppendOneByte(JsonParse *pParse, u8 c){
  if( pParse->nBlob >= pParse->nBlobAlloc ){
    jsonBlobExpandAndAppendOneByte(pParse, c);
  }else{
    pParse->aBlob[pParse->nBlob++] = c;
  }
}

/* Slow version of jsonBlobAppendNode() that first resizes the
** pParse->aBlob structure.
*/
static void jsonBlobAppendNode(JsonParse*,u8,u32,const void*);
static SQLITE_NOINLINE void jsonBlobExpandAndAppendNode(
  JsonParse *pParse,
  u8 eType,
  u32 szPayload,
  const void *aPayload
){
  if( jsonBlobExpand(pParse, pParse->nBlob+szPayload+9) ) return;
  jsonBlobAppendNode(pParse, eType, szPayload, aPayload);
}


/* Append an node type byte together with the payload size and
** possibly also the payload.
**
** If aPayload is not NULL, then it is a pointer to the payload which
** is also appended.  If aPayload is NULL, the pParse->aBlob[] array
** is resized (if necessary) so that it is big enough to hold the
** payload, but the payload is not appended and pParse->nBlob is left
** pointing to where the first byte of payload will eventually be.
*/
static void jsonBlobAppendNode(
  JsonParse *pParse,          /* The JsonParse object under construction */
  u8 eType,                   /* Node type.  One of JSONB_* */
  u32 szPayload,              /* Number of bytes of payload */
  const void *aPayload        /* The payload.  Might be NULL */
){
  u8 *a;
  if( pParse->nBlob+szPayload+9 > pParse->nBlobAlloc ){
    jsonBlobExpandAndAppendNode(pParse,eType,szPayload,aPayload);
    return;
  }
  assert( pParse->aBlob!=0 );
  a = &pParse->aBlob[pParse->nBlob];
  if( szPayload<=11 ){
    a[0] = eType | (szPayload<<4);
    pParse->nBlob += 1;
  }else if( szPayload<=0xff ){
    a[0] = eType | 0xc0;
    a[1] = szPayload & 0xff;
    pParse->nBlob += 2;
  }else if( szPayload<=0xffff ){
    a[0] = eType | 0xd0;
    a[1] = (szPayload >> 8) & 0xff;
    a[2] = szPayload & 0xff;
    pParse->nBlob += 3;
  }else{
    a[0] = eType | 0xe0;
    a[1] = (szPayload >> 24) & 0xff;
    a[2] = (szPayload >> 16) & 0xff;
    a[3] = (szPayload >> 8) & 0xff;
    a[4] = szPayload & 0xff;
    pParse->nBlob += 5;
  }
  if( aPayload ){
    pParse->nBlob += szPayload;
    memcpy(&pParse->aBlob[pParse->nBlob-szPayload], aPayload, szPayload);
  }
}

/* Change the payload size for the node at index i to be szPayload.
*/
static int jsonBlobChangePayloadSize(
  JsonParse *pParse,
  u32 i,
  u32 szPayload
){
  u8 *a;
  u8 szType;
  u8 nExtra;
  u8 nNeeded;
  int delta;
  if( pParse->oom ) return 0;
  a = &pParse->aBlob[i];
  szType = a[0]>>4;
  if( szType<=11 ){
    nExtra = 0;
  }else if( szType==12 ){
    nExtra = 1;
  }else if( szType==13 ){
    nExtra = 2;
  }else{
    nExtra = 4;
  }
  if( szPayload<=11 ){
    nNeeded = 0;
  }else if( szPayload<=0xff ){
    nNeeded = 1;
  }else if( szPayload<=0xffff ){
    nNeeded = 2;
  }else{
    nNeeded = 4;
  }
  delta = nNeeded - nExtra;
  if( delta ){
    u32 newSize = pParse->nBlob + delta;
    if( delta>0 ){
      if( newSize>pParse->nBlobAlloc && jsonBlobExpand(pParse, newSize) ){
        return 0;  /* OOM error.  Error state recorded in pParse->oom. */
      }
      a = &pParse->aBlob[i];
      memmove(&a[1+delta], &a[1], pParse->nBlob - (i+1));
    }else{
      memmove(&a[1], &a[1-delta], pParse->nBlob - (i+1-delta));
    }
    pParse->nBlob = newSize;
  }
  if( nNeeded==0 ){
    a[0] = (a[0] & 0x0f) | (szPayload<<4);
  }else if( nNeeded==1 ){
    a[0] = (a[0] & 0x0f) | 0xc0;
    a[1] = szPayload & 0xff;
  }else if( nNeeded==2 ){
    a[0] = (a[0] & 0x0f) | 0xd0;
    a[1] = (szPayload >> 8) & 0xff;
    a[2] = szPayload & 0xff;
  }else{
    a[0] = (a[0] & 0x0f) | 0xe0;
    a[1] = (szPayload >> 24) & 0xff;
    a[2] = (szPayload >> 16) & 0xff;
    a[3] = (szPayload >> 8) & 0xff;
    a[4] = szPayload & 0xff;
  }
  return delta;
}

/*
** If z[0] is 'u' and is followed by exactly 4 hexadecimal character,
** then set *pOp to JSONB_TEXTJ and return true.  If not, do not make
** any changes to *pOp and return false.
*/
static int jsonIs4HexB(const char *z, int *pOp){
  if( z[0]!='u' ) return 0;
  if( !jsonIs4Hex(&z[1]) ) return 0;
  *pOp = JSONB_TEXTJ;
  return 1;
}

/*
** Check a single element of the JSONB in pParse for validity.
**
** The element to be checked starts at offset i and must end at on the
** last byte before iEnd.
**
** Return 0 if everything is correct.  Return the 1-based byte offset of the
** error if a problem is detected.  (In other words, if the error is at offset
** 0, return 1).
*/
static u32 jsonbValidityCheck(
  const JsonParse *pParse,    /* Input JSONB.  Only aBlob and nBlob are used */
  u32 i,                      /* Start of element as pParse->aBlob[i] */
  u32 iEnd,                   /* One more than the last byte of the element */
  u32 iDepth                  /* Current nesting depth */
){
  u32 n, sz, j, k;
  const u8 *z;
  u8 x;
  if( iDepth>JSON_MAX_DEPTH ) return i+1;
  sz = 0;
  n = jsonbPayloadSize(pParse, i, &sz);
  if( NEVER(n==0) ) return i+1;          /* Checked by caller */
  if( NEVER(i+n+sz!=iEnd) ) return i+1;  /* Checked by caller */
  z = pParse->aBlob;
  x = z[i] & 0x0f;
  switch( x ){
    case JSONB_NULL:
    case JSONB_TRUE:
    case JSONB_FALSE: {
      return n+sz==1 ? 0 : i+1;
    }
    case JSONB_INT: {
      if( sz<1 ) return i+1;
      j = i+n;
      if( z[j]=='-' ){
        j++;
        if( sz<2 ) return i+1;
      }
      k = i+n+sz;
      while( j<k ){
        if( sqlite3Isdigit(z[j]) ){
          j++;
        }else{
          return j+1;
        }
      }
      return 0;
    }
    case JSONB_INT5: {
      if( sz<3 ) return i+1;
      j = i+n;
      if( z[j]=='-' ){
        if( sz<4 ) return i+1;
        j++;
      }
      if( z[j]!='0' ) return i+1;
      if( z[j+1]!='x' && z[j+1]!='X' ) return j+2;
      j += 2;
      k = i+n+sz;
      while( j<k ){
        if( sqlite3Isxdigit(z[j]) ){
          j++;
        }else{
          return j+1;
        }
      }
      return 0;
    }
    case JSONB_FLOAT:
    case JSONB_FLOAT5: {
      u8 seen = 0;   /* 0: initial.  1: '.' seen  2: 'e' seen */
      if( sz<2 ) return i+1;
      j = i+n;
      k = j+sz;
      if( z[j]=='-' ){
        j++;
        if( sz<3 ) return i+1;
      }
      if( z[j]=='.' ){
        if( x==JSONB_FLOAT ) return j+1;
        if( !sqlite3Isdigit(z[j+1]) ) return j+1;
        j += 2;
        seen = 1;
      }else if( z[j]=='0' && x==JSONB_FLOAT ){
        if( j+3>k ) return j+1;
        if( z[j+1]!='.' && z[j+1]!='e' && z[j+1]!='E' ) return j+1;
        j++;
      }
      for(; j<k; j++){
        if( sqlite3Isdigit(z[j]) ) continue;
        if( z[j]=='.' ){
          if( seen>0 ) return j+1;
          if( x==JSONB_FLOAT && (j==k-1 || !sqlite3Isdigit(z[j+1])) ){
            return j+1;
          }
          seen = 1;
          continue;
        }
        if( z[j]=='e' || z[j]=='E' ){
          if( seen==2 ) return j+1;
          if( j==k-1 ) return j+1;
          if( z[j+1]=='+' || z[j+1]=='-' ){
            j++;
            if( j==k-1 ) return j+1;
          }
          seen = 2;
          continue;
        }
        return j+1;
      }
      if( seen==0 ) return i+1;
      return 0;
    }
    case JSONB_TEXT: {
      j = i+n;
      k = j+sz;
      while( j<k ){
        if( !jsonIsOk[z[j]] && z[j]!='\'' ) return j+1;
        j++;
      }
      return 0;
    }
    case JSONB_TEXTJ:
    case JSONB_TEXT5: {
      j = i+n;
      k = j+sz;
      while( j<k ){
        if( !jsonIsOk[z[j]] && z[j]!='\'' ){
          if( z[j]=='"' ){
            if( x==JSONB_TEXTJ ) return j+1;
          }else if( z[j]<=0x1f ){
            /* Control characters in JSON5 string literals are ok */
            if( x==JSONB_TEXTJ ) return j+1;
          }else if( NEVER(z[j]!='\\') || j+1>=k ){
            return j+1;
          }else if( strchr("\"\\/bfnrt",z[j+1])!=0 ){
            j++;
          }else if( z[j+1]=='u' ){
            if( j+5>=k ) return j+1;
            if( !jsonIs4Hex((const char*)&z[j+2]) ) return j+1;
            j++;
          }else if( x!=JSONB_TEXT5 ){
            return j+1;
          }else{
            u32 c = 0;
            u32 szC = jsonUnescapeOneChar((const char*)&z[j], k-j, &c);
            if( c==JSON_INVALID_CHAR ) return j+1;
            j += szC - 1;
          }
        }
        j++;
      }
      return 0;
    }
    case JSONB_TEXTRAW: {
      return 0;
    }
    case JSONB_ARRAY: {
      u32 sub;
      j = i+n;
      k = j+sz;
      while( j<k ){
        sz = 0;
        n = jsonbPayloadSize(pParse, j, &sz);
        if( n==0 ) return j+1;
        if( j+n+sz>k ) return j+1;
        sub = jsonbValidityCheck(pParse, j, j+n+sz, iDepth+1);
        if( sub ) return sub;
        j += n + sz;
      }
      assert( j==k );
      return 0;
    }
    case JSONB_OBJECT: {
      u32 cnt = 0;
      u32 sub;
      j = i+n;
      k = j+sz;
      while( j<k ){
        sz = 0;
        n = jsonbPayloadSize(pParse, j, &sz);
        if( n==0 ) return j+1;
        if( j+n+sz>k ) return j+1;
        if( (cnt & 1)==0 ){
          x = z[j] & 0x0f;
          if( x<JSONB_TEXT || x>JSONB_TEXTRAW ) return j+1;
        }
        sub = jsonbValidityCheck(pParse, j, j+n+sz, iDepth+1);
        if( sub ) return sub;
        cnt++;
        j += n + sz;
      }
      assert( j==k );
      if( (cnt & 1)!=0 ) return j+1;
      return 0;
    }
    default: {
      return i+1;
    }
  }
}
