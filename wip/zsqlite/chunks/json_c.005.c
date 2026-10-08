

/*
** Parse a complete JSON string.  Return 0 on success or non-zero if there
** are any errors.  If an error occurs, free all memory held by pParse,
** but not pParse itself.
**
** pParse must be initialized to an empty parse object prior to calling
** this routine.
*/
static int jsonConvertTextToBlob(
  JsonParse *pParse,           /* Initialize and fill this JsonParse object */
  sqlite3_context *pCtx        /* Report errors here */
){
  int i;
  const char *zJson = pParse->zJson;
  i = jsonTranslateTextToBlob(pParse, 0);
  if( pParse->oom ) i = -1;
  if( i>0 ){
#ifdef SQLITE_DEBUG
    assert( pParse->iDepth==0 );
    if( sqlite3Config.bJsonSelfcheck ){
      assert( jsonbValidityCheck(pParse, 0, pParse->nBlob, 0)==0 );
    }
#endif
    while( jsonIsspace(zJson[i]) ) i++;
    if( zJson[i] ){
      i += json5Whitespace(&zJson[i]);
      if( zJson[i] ){
        if( pCtx ) sqlite3_result_error(pCtx, "malformed JSON", -1);
        jsonParseReset(pParse);
        return 1;
      }
      pParse->hasNonstd = 1;
    }
  }
  if( i<=0 ){
    if( pCtx!=0 ){
      if( pParse->oom ){
        sqlite3_result_error_nomem(pCtx);
      }else{
        sqlite3_result_error(pCtx, "malformed JSON", -1);
      }
    }
    jsonParseReset(pParse);
    return 1;
  }
  return 0;
}

/*
** The input string pStr is a well-formed JSON text string.  Convert
** this into the JSONB format and make it the return value of the
** SQL function.
*/
static void jsonReturnStringAsBlob(JsonString *pStr){
  JsonParse px;
  memset(&px, 0, sizeof(px));
  jsonStringTerminate(pStr);
  if( pStr->eErr ){
    sqlite3_result_error_nomem(pStr->pCtx);
    return;
  }
  px.zJson = pStr->zBuf;
  px.nJson = pStr->nUsed;
  px.db = sqlite3_context_db_handle(pStr->pCtx);
  (void)jsonTranslateTextToBlob(&px, 0);
  if( px.oom ){
    sqlite3DbFree(px.db, px.aBlob);
    sqlite3_result_error_nomem(pStr->pCtx);
  }else{
    assert( px.nBlobAlloc>0 );
    assert( !px.bReadOnly );
    sqlite3_result_blob(pStr->pCtx, px.aBlob, px.nBlob, SQLITE_DYNAMIC);
  }
}

/* The byte at index i is a node type-code.  This routine
** determines the payload size for that node and writes that
** payload size in to *pSz.  It returns the offset from i to the
** beginning of the payload.  Return 0 on error.
*/
static u32 jsonbPayloadSize(const JsonParse *pParse, u32 i, u32 *pSz){
  u8 x;
  u32 sz;
  u32 n;
  if( NEVER(i>pParse->nBlob) ){
    *pSz = 0;
    return 0;
  }
  x = pParse->aBlob[i]>>4;
  if( x<=11 ){
    sz = x;
    n = 1;
  }else if( x==12 ){
    if( i+1>=pParse->nBlob ){
      *pSz = 0;
      return 0;
    }
    sz = pParse->aBlob[i+1];
    n = 2;
  }else if( x==13 ){
    if( i+2>=pParse->nBlob ){
      *pSz = 0;
      return 0;
    }
    sz = (pParse->aBlob[i+1]<<8) + pParse->aBlob[i+2];
    n = 3;
  }else if( x==14 ){
    if( i+4>=pParse->nBlob ){
      *pSz = 0;
      return 0;
    }
    sz = ((u32)pParse->aBlob[i+1]<<24) + (pParse->aBlob[i+2]<<16) +
         (pParse->aBlob[i+3]<<8) + pParse->aBlob[i+4];
    n = 5;
  }else{
    if( i+8>=pParse->nBlob
     || pParse->aBlob[i+1]!=0
     || pParse->aBlob[i+2]!=0
     || pParse->aBlob[i+3]!=0
     || pParse->aBlob[i+4]!=0
    ){
      *pSz = 0;
      return 0;
    }
    sz = (pParse->aBlob[i+5]<<24) + (pParse->aBlob[i+6]<<16) +
         (pParse->aBlob[i+7]<<8) + pParse->aBlob[i+8];
    n = 9;
  }
  if( (i64)i+sz+n > pParse->nBlob
   && (i64)i+sz+n > pParse->nBlob-pParse->delta
  ){
    sz = 0;
    n = 0;
  }
  *pSz = sz;
  return n;
}


/*
** Translate the binary JSONB representation of JSON beginning at
** pParse->aBlob[i] into a JSON text string.  Append the JSON
** text onto the end of pOut.  Return the index in pParse->aBlob[]
** of the first byte past the end of the element that is translated.
**
** If an error is detected in the BLOB input, the pOut->eErr flag
** might get set to JSTRING_MALFORMED.  But not all BLOB input errors
** are detected.  So a malformed JSONB input might either result
** in an error, or in incorrect JSON.
**
** The pOut->eErr JSTRING_OOM flag is set on a OOM.
*/
static u32 jsonTranslateBlobToText(
  const JsonParse *pParse,       /* the complete parse of the JSON */
  u32 i,                         /* Start rendering at this index */
  JsonString *pOut               /* Write JSON here */
){
  u32 sz, n, j, iEnd;

  n = jsonbPayloadSize(pParse, i, &sz);
  if( n==0 ){
    pOut->eErr |= JSTRING_MALFORMED;
    return pParse->nBlob+1;
  }
  switch( pParse->aBlob[i] & 0x0f ){
    case JSONB_NULL: {
      jsonAppendRawNZ(pOut, "null", 4);
      return i+1;
    }
    case JSONB_TRUE: {
      jsonAppendRawNZ(pOut, "true", 4);
      return i+1;
    }
    case JSONB_FALSE: {
      jsonAppendRawNZ(pOut, "false", 5);
      return i+1;
    }
    case JSONB_INT:
    case JSONB_FLOAT: {
      if( sz==0 ) goto malformed_jsonb;
      jsonAppendRaw(pOut, (const char*)&pParse->aBlob[i+n], sz);
      break;
    }
    case JSONB_INT5: {  /* Integer literal in hexadecimal notation */
      u32 k = 2;
      sqlite3_uint64 u = 0;
      const char *zIn = (const char*)&pParse->aBlob[i+n];
      int bOverflow = 0;
      if( sz==0 ) goto malformed_jsonb;
      if( zIn[0]=='-' ){
        jsonAppendChar(pOut, '-');
        k++;
      }else if( zIn[0]=='+' ){
        k++;
      }
      for(; k<sz; k++){
        if( !sqlite3Isxdigit(zIn[k]) ){
          pOut->eErr |= JSTRING_MALFORMED;
          break;
        }else if( (u>>60)!=0 ){
          bOverflow = 1;
        }else{
          u = u*16 + sqlite3HexToInt(zIn[k]);
        }
      }
      jsonPrintf(100,pOut,bOverflow?"9.0e999":"%llu", u);
      break;
    }
    case JSONB_FLOAT5: { /* Float literal missing digits beside "." */
      u32 k = 0;
      const char *zIn = (const char*)&pParse->aBlob[i+n];
      if( sz==0 ) goto malformed_jsonb;
      if( zIn[0]=='-' ){
        jsonAppendChar(pOut, '-');
        k++;
      }
      if( zIn[k]=='.' ){
        jsonAppendChar(pOut, '0');
      }
      for(; k<sz; k++){
        jsonAppendChar(pOut, zIn[k]);
        if( zIn[k]=='.' && (k+1==sz || !sqlite3Isdigit(zIn[k+1])) ){
          jsonAppendChar(pOut, '0');
        }
      }
      break;
    }
    case JSONB_TEXT:
    case JSONB_TEXTJ: {
      jsonAppendChar(pOut, '"');
      jsonAppendRaw(pOut, (const char*)&pParse->aBlob[i+n], sz);
      jsonAppendChar(pOut, '"');
      break;
    }
    case JSONB_TEXT5: {
      const char *zIn;
      u32 k;
      u32 sz2 = sz;
      zIn = (const char*)&pParse->aBlob[i+n];
      jsonAppendChar(pOut, '"');
      while( sz2>0 ){
        for(k=0; k<sz2 && (jsonIsOk[(u8)zIn[k]] || zIn[k]=='\''); k++){}
        if( k>0 ){
          jsonAppendRawNZ(pOut, zIn, k);
          if( k>=sz2 ){
            break;
          }
          zIn += k;
          sz2 -= k;
        }
        if( zIn[0]=='"' ){
          jsonAppendRawNZ(pOut, "\\\"", 2);
          zIn++;
          sz2--;
          continue;
        }
        if( zIn[0]<=0x1f ){
          if( pOut->nUsed+7>pOut->nAlloc && jsonStringGrow(pOut,7) ) break;
          jsonAppendControlChar(pOut, zIn[0]);
          zIn++;
          sz2--;
          continue;
        }
        assert( zIn[0]=='\\' );
        assert( sz2>=1 );
        if( sz2<2 ){
          pOut->eErr |= JSTRING_MALFORMED;
          break;
        }
        switch( (u8)zIn[1] ){
          case '\'':
            jsonAppendChar(pOut, '\'');
            break;
          case 'v':
            jsonAppendRawNZ(pOut, "\\u0009", 6);
            break;
          case 'x':
            if( sz2<4 ){
              pOut->eErr |= JSTRING_MALFORMED;
              sz2 = 2;
              break;
            }
            jsonAppendRawNZ(pOut, "\\u00", 4);
            jsonAppendRawNZ(pOut, &zIn[2], 2);
            zIn += 2;
            sz2 -= 2;
            break;
          case '0':
            jsonAppendRawNZ(pOut, "\\u0000", 6);
            break;
          case '\r':
            if( sz2>2 && zIn[2]=='\n' ){
              zIn++;
              sz2--;
            }
            break;
          case '\n':
            break;
          case 0xe2:
            /* '\' followed by either U+2028 or U+2029 is ignored as
            ** whitespace.  Not that in UTF8, U+2028 is 0xe2 0x80 0x29.
            ** U+2029 is the same except for the last byte */
            if( sz2<4
             || 0x80!=(u8)zIn[2]
             || (0xa8!=(u8)zIn[3] && 0xa9!=(u8)zIn[3])
            ){
              pOut->eErr |= JSTRING_MALFORMED;
              sz2 = 2;
              break;
            }
            zIn += 2;
            sz2 -= 2;
            break;
          default:
            jsonAppendRawNZ(pOut, zIn, 2);
            break;
        }
        assert( sz2>=2 );
        zIn += 2;
        sz2 -= 2;
      }
      jsonAppendChar(pOut, '"');
      break;
    }
    case JSONB_TEXTRAW: {
      jsonAppendString(pOut, (const char*)&pParse->aBlob[i+n], sz);
      break;
    }
    case JSONB_ARRAY: {
      jsonAppendChar(pOut, '[');
      j = i+n;
      iEnd = j+sz;
      while( j<iEnd && pOut->eErr==0 ){
        j = jsonTranslateBlobToText(pParse, j, pOut);
        jsonAppendChar(pOut, ',');
      }
      if( j>iEnd ) pOut->eErr |= JSTRING_MALFORMED;
      if( sz>0 ) jsonStringTrimOneChar(pOut);
      jsonAppendChar(pOut, ']');
      break;
    }
    case JSONB_OBJECT: {
      int x = 0;
      jsonAppendChar(pOut, '{');
      j = i+n;
      iEnd = j+sz;
      while( j<iEnd && pOut->eErr==0 ){
        j = jsonTranslateBlobToText(pParse, j, pOut);
        jsonAppendChar(pOut, (x++ & 1) ? ',' : ':');
      }
      if( (x & 1)!=0 || j>iEnd ) pOut->eErr |= JSTRING_MALFORMED;
      if( sz>0 ) jsonStringTrimOneChar(pOut);
      jsonAppendChar(pOut, '}');
      break;
    }

    default: {
      malformed_jsonb:
      pOut->eErr |= JSTRING_MALFORMED;
      break;
    }
  }
  return i+n+sz;
}
