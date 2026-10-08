
/* Context for recursion of json_pretty()
*/
typedef struct JsonPretty JsonPretty;
struct JsonPretty {
  JsonParse *pParse;        /* The BLOB being rendered */
  JsonString *pOut;         /* Generate pretty output into this string */
  const char *zIndent;      /* Use this text for indentation */
  u32 szIndent;             /* Bytes in zIndent[] */
  u32 nIndent;              /* Current level of indentation */
};

/* Append indentation to the pretty JSON under construction */
static void jsonPrettyIndent(JsonPretty *pPretty){
  u32 jj;
  for(jj=0; jj<pPretty->nIndent; jj++){
    jsonAppendRaw(pPretty->pOut, pPretty->zIndent, pPretty->szIndent);
  }
}

/*
** Translate the binary JSONB representation of JSON beginning at
** pParse->aBlob[i] into a JSON text string.  Append the JSON
** text onto the end of pOut.  Return the index in pParse->aBlob[]
** of the first byte past the end of the element that is translated.
**
** This is a variant of jsonTranslateBlobToText() that "pretty-prints"
** the output.  Extra whitespace is inserted to make the JSON easier
** for humans to read.
**
** If an error is detected in the BLOB input, the pOut->eErr flag
** might get set to JSTRING_MALFORMED.  But not all BLOB input errors
** are detected.  So a malformed JSONB input might either result
** in an error, or in incorrect JSON.
**
** The pOut->eErr JSTRING_OOM flag is set on a OOM.
*/
static u32 jsonTranslateBlobToPrettyText(
  JsonPretty *pPretty,       /* Pretty-printing context */
  u32 i                      /* Start rendering at this index */
){
  u32 sz, n, j, iEnd;
  const JsonParse *pParse = pPretty->pParse;
  JsonString *pOut = pPretty->pOut;
  n = jsonbPayloadSize(pParse, i, &sz);
  if( n==0 ){
    pOut->eErr |= JSTRING_MALFORMED;
    return pParse->nBlob+1;
  }
  switch( pParse->aBlob[i] & 0x0f ){
    case JSONB_ARRAY: {
      j = i+n;
      iEnd = j+sz;
      jsonAppendChar(pOut, '[');
      if( j<iEnd ){
        jsonAppendChar(pOut, '\n');
        pPretty->nIndent++;
        while( pOut->eErr==0 ){
          jsonPrettyIndent(pPretty);
          j = jsonTranslateBlobToPrettyText(pPretty, j);
          if( j>=iEnd ) break;
          jsonAppendRawNZ(pOut, ",\n", 2);
        }
        jsonAppendChar(pOut, '\n');
        pPretty->nIndent--;
        jsonPrettyIndent(pPretty);
      }
      jsonAppendChar(pOut, ']');
      i = iEnd;
      break;
    }
    case JSONB_OBJECT: {
      j = i+n;
      iEnd = j+sz;
      jsonAppendChar(pOut, '{');
      if( j<iEnd ){
        jsonAppendChar(pOut, '\n');
        pPretty->nIndent++;
        while( pOut->eErr==0 ){
          jsonPrettyIndent(pPretty);
          j = jsonTranslateBlobToText(pParse, j, pOut);
          if( j>iEnd ){
            pOut->eErr |= JSTRING_MALFORMED;
            break;
          }
          jsonAppendRawNZ(pOut, ": ", 2);
          j = jsonTranslateBlobToPrettyText(pPretty, j);
          if( j>=iEnd ) break;
          jsonAppendRawNZ(pOut, ",\n", 2);
        }
        jsonAppendChar(pOut, '\n');
        pPretty->nIndent--;
        jsonPrettyIndent(pPretty);
      }
      jsonAppendChar(pOut, '}');
      i = iEnd;
      break;
    }
    default: {
      i = jsonTranslateBlobToText(pParse, i, pOut);
      break;
    }
  }
  return i;
}


/* Return true if the input pJson
**
** For performance reasons, this routine does not do a detailed check of the
** input BLOB to ensure that it is well-formed.  Hence, false positives are
** possible.  False negatives should never occur, however.
*/
static int jsonFuncArgMightBeBinary(sqlite3_value *pJson){
  u32 sz, n;
  const u8 *aBlob;
  int nBlob;
  JsonParse s;
  if( sqlite3_value_type(pJson)!=SQLITE_BLOB ) return 0;
  aBlob = sqlite3_value_blob(pJson);
  nBlob = sqlite3_value_bytes(pJson);
  if( nBlob<1 ) return 0;
  if( NEVER(aBlob==0) || (aBlob[0] & 0x0f)>JSONB_OBJECT ) return 0;
  memset(&s, 0, sizeof(s));
  s.aBlob = (u8*)aBlob;
  s.nBlob = nBlob;
  n = jsonbPayloadSize(&s, 0, &sz);
  if( n==0 ) return 0;
  if( sz+n!=(u32)nBlob ) return 0;
  if( (aBlob[0] & 0x0f)<=JSONB_FALSE && sz>0 ) return 0;
  return sz+n==(u32)nBlob;
}

/*
** Given that a JSONB_ARRAY object starts at offset i, return
** the number of entries in that array.
*/
static u32 jsonbArrayCount(JsonParse *pParse, u32 iRoot){
  u32 n, sz, i, iEnd;
  u32 k = 0;
  n = jsonbPayloadSize(pParse, iRoot, &sz);
  iEnd = iRoot+n+sz;
  for(i=iRoot+n; n>0 && i<iEnd; i+=sz+n, k++){
    n = jsonbPayloadSize(pParse, i, &sz);
  }
  return k;
}

/*
** Edit the payload size of the element at iRoot by the amount in
** pParse->delta.
*/
static void jsonAfterEditSizeAdjust(JsonParse *pParse, u32 iRoot){
  u32 sz = 0;
  u32 nBlob;
  assert( pParse->delta!=0 );
  assert( pParse->nBlobAlloc >= pParse->nBlob );
  nBlob = pParse->nBlob;
  pParse->nBlob = pParse->nBlobAlloc;
  (void)jsonbPayloadSize(pParse, iRoot, &sz);
  pParse->nBlob = nBlob;
  sz += pParse->delta;
  pParse->delta += jsonBlobChangePayloadSize(pParse, iRoot, sz);
}

/*
** Modify the JSONB blob at pParse->aBlob by removing nDel bytes of
** content beginning at iDel, and replacing them with nIns bytes of
** content given by aIns.
**
** nDel may be zero, in which case no bytes are removed.  But iDel is
** still important as new bytes will be insert beginning at iDel.
**
** aIns may be zero, in which case space is created to hold nIns bytes
** beginning at iDel, but that space is uninitialized.
**
** Set pParse->oom if an OOM occurs.
*/
static void jsonBlobEdit(
  JsonParse *pParse,     /* The JSONB to be modified is in pParse->aBlob */
  u32 iDel,              /* First byte to be removed */
  u32 nDel,              /* Number of bytes to remove */
  const u8 *aIns,        /* Content to insert */
  u32 nIns               /* Bytes of content to insert */
){
  i64 d = (i64)nIns - (i64)nDel;
  if( d!=0 ){
    if( pParse->nBlob + d > pParse->nBlobAlloc ){
      jsonBlobExpand(pParse, pParse->nBlob+d);
      if( pParse->oom ) return;
    }
    memmove(&pParse->aBlob[iDel+nIns],
            &pParse->aBlob[iDel+nDel],
            pParse->nBlob - (iDel+nDel));
    pParse->nBlob += d;
    pParse->delta += d;
  }
  if( nIns && aIns ) memcpy(&pParse->aBlob[iDel], aIns, nIns);
}

/*
** Return the number of escaped newlines to be ignored.
** An escaped newline is a one of the following byte sequences:
**
**    0x5c 0x0a
**    0x5c 0x0d
**    0x5c 0x0d 0x0a
**    0x5c 0xe2 0x80 0xa8
**    0x5c 0xe2 0x80 0xa9
*/
static u32 jsonBytesToBypass(const char *z, u32 n){
  u32 i = 0;
  while( i+1<n ){
    if( z[i]!='\\' ) return i;
    if( z[i+1]=='\n' ){
      i += 2;
      continue;
    }
    if( z[i+1]=='\r' ){
      if( i+2<n && z[i+2]=='\n' ){
        i += 3;
      }else{
        i += 2;
      }
      continue;
    }
    if( 0xe2==(u8)z[i+1]
     && i+3<n
     && 0x80==(u8)z[i+2]
     && (0xa8==(u8)z[i+3] || 0xa9==(u8)z[i+3])
    ){
      i += 4;
      continue;
    }
    break;
  }
  return i;
}

/*
** Input z[0..n] defines JSON escape sequence including the leading '\\'.
** Decode that escape sequence into a single character.  Write that
** character into *piOut.  Return the number of bytes in the escape sequence.
**
** If there is a syntax error of some kind (for example too few characters
** after the '\\' to complete the encoding) then *piOut is set to
** JSON_INVALID_CHAR.
*/
static u32 jsonUnescapeOneChar(const char *z, u32 n, u32 *piOut){
  assert( n>0 );
  assert( z[0]=='\\' );
  if( n<2 ){
    *piOut = JSON_INVALID_CHAR;
    return n;
  }
  switch( (u8)z[1] ){
    case 'u': {
      u32 v, vlo;
      if( n<6 ){
        *piOut = JSON_INVALID_CHAR;
        return n;
      }
      v = jsonHexToInt4(&z[2]);
      if( (v & 0xfc00)==0xd800
       && n>=12
       && z[6]=='\\'
       && z[7]=='u'
       && ((vlo = jsonHexToInt4(&z[8]))&0xfc00)==0xdc00
      ){
        *piOut = ((v&0x3ff)<<10) + (vlo&0x3ff) + 0x10000;
        return 12;
      }else{
        *piOut = v;
        return 6;
      }
    }
    case 'b': {   *piOut = '\b';  return 2; }
    case 'f': {   *piOut = '\f';  return 2; }
    case 'n': {   *piOut = '\n';  return 2; }
    case 'r': {   *piOut = '\r';  return 2; }
    case 't': {   *piOut = '\t';  return 2; }
    case 'v': {   *piOut = '\v';  return 2; }
    case '0': {   *piOut = 0;     return 2; }
    case '\'':
    case '"':
    case '/':
    case '\\':{   *piOut = z[1];  return 2; }
    case 'x': {
      if( n<4 ){
        *piOut = JSON_INVALID_CHAR;
        return n;
      }
      *piOut = (jsonHexToInt(z[2])<<4) | jsonHexToInt(z[3]);
      return 4;
    }
    case 0xe2:
    case '\r':
    case '\n': {
      u32 nSkip = jsonBytesToBypass(z, n);
      if( nSkip==0 ){
        *piOut = JSON_INVALID_CHAR;
        return n;
      }else if( nSkip==n ){
        *piOut = 0;
        return n;
      }else if( z[nSkip]=='\\' ){
        return nSkip + jsonUnescapeOneChar(&z[nSkip], n-nSkip, piOut);
      }else{
        int sz = sqlite3Utf8ReadLimited((u8*)&z[nSkip], n-nSkip, piOut);
        return nSkip + sz;
      }
    }
    default: {
      *piOut = JSON_INVALID_CHAR;
      return 2;
    }
  }
}


/*
** Compare two object labels.  Return 1 if they are equal and
** 0 if they differ.
**
** In this version, we know that one or the other or both of the
** two comparands contains an escape sequence.
*/
static SQLITE_NOINLINE int jsonLabelCompareEscaped(
  const char *zLeft,          /* The left label */
  u32 nLeft,                  /* Size of the left label in bytes */
  int rawLeft,                /* True if zLeft contains no escapes */
  const char *zRight,         /* The right label */
  u32 nRight,                 /* Size of the right label in bytes */
  int rawRight                /* True if zRight is escape-free */
){
  u32 cLeft, cRight;
  assert( rawLeft==0 || rawRight==0 );
  while( 1 /*exit-by-return*/ ){
    if( nLeft==0 ){
      cLeft = 0;
    }else if( rawLeft || zLeft[0]!='\\' ){
      cLeft = ((u8*)zLeft)[0];
      if( cLeft>=0xc0 ){
        int sz = sqlite3Utf8ReadLimited((u8*)zLeft, nLeft, &cLeft);
        zLeft += sz;
        nLeft -= sz;
      }else{
        zLeft++;
        nLeft--;
      }
    }else{
      u32 n = jsonUnescapeOneChar(zLeft, nLeft, &cLeft);
      zLeft += n;
      assert( n<=nLeft );
      nLeft -= n;
    }
    if( nRight==0 ){
      cRight = 0;
    }else if( rawRight || zRight[0]!='\\' ){
      cRight = ((u8*)zRight)[0];
      if( cRight>=0xc0 ){
        int sz = sqlite3Utf8ReadLimited((u8*)zRight, nRight, &cRight);
        zRight += sz;
        nRight -= sz;
      }else{
        zRight++;
        nRight--;
      }
    }else{
      u32 n = jsonUnescapeOneChar(zRight, nRight, &cRight);
      zRight += n;
      assert( n<=nRight );
      nRight -= n;
    }
    if( cLeft!=cRight ) return 0;
    if( cLeft==0 ) return 1;
  }
}
