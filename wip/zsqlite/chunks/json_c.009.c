
/****************************************************************************
** SQL functions used for testing and debugging
****************************************************************************/

#if SQLITE_DEBUG
/*
** Decode JSONB bytes in aBlob[] starting at iStart through but not
** including iEnd.  Indent the
** content by nIndent spaces.
*/
static void jsonDebugPrintBlob(
  JsonParse *pParse, /* JSON content */
  u32 iStart,        /* Start rendering here */
  u32 iEnd,          /* Do not render this byte or any byte after this one */
  int nIndent,       /* Indent by this many spaces */
  sqlite3_str *pOut  /* Generate output into this sqlite3_str object */
){
  while( iStart<iEnd ){
    u32 i, n, nn, sz = 0;
    int showContent = 1;
    u8 x = pParse->aBlob[iStart] & 0x0f;
    u32 savedNBlob = pParse->nBlob;
    sqlite3_str_appendf(pOut, "%5d:%*s", iStart, nIndent, "");
    if( pParse->nBlobAlloc>pParse->nBlob ){
      pParse->nBlob = pParse->nBlobAlloc;
    }
    nn = n = jsonbPayloadSize(pParse, iStart, &sz);
    if( nn==0 ) nn = 1;
    if( sz>0 && x<JSONB_ARRAY ){
      nn += sz;
    }
    for(i=0; i<nn; i++){
      sqlite3_str_appendf(pOut, " %02x", pParse->aBlob[iStart+i]);
    }
    if( n==0 ){
      sqlite3_str_appendf(pOut, "   ERROR invalid node size\n");
      iStart = n==0 ? iStart+1 : iEnd;
      continue;
    }
    pParse->nBlob = savedNBlob;
    if( iStart+n+sz>iEnd ){
      iEnd = iStart+n+sz;
      if( iEnd>pParse->nBlob ){
        if( pParse->nBlobAlloc>0 && iEnd>pParse->nBlobAlloc ){
          iEnd = pParse->nBlobAlloc;
        }else{
          iEnd = pParse->nBlob;
        }
      }
    }
    sqlite3_str_appendall(pOut,"  <-- ");
    switch( x ){
      case JSONB_NULL:     sqlite3_str_appendall(pOut,"null"); break;
      case JSONB_TRUE:     sqlite3_str_appendall(pOut,"true"); break;
      case JSONB_FALSE:    sqlite3_str_appendall(pOut,"false"); break;
      case JSONB_INT:      sqlite3_str_appendall(pOut,"int"); break;
      case JSONB_INT5:     sqlite3_str_appendall(pOut,"int5"); break;
      case JSONB_FLOAT:    sqlite3_str_appendall(pOut,"float"); break;
      case JSONB_FLOAT5:   sqlite3_str_appendall(pOut,"float5"); break;
      case JSONB_TEXT:     sqlite3_str_appendall(pOut,"text"); break;
      case JSONB_TEXTJ:    sqlite3_str_appendall(pOut,"textj"); break;
      case JSONB_TEXT5:    sqlite3_str_appendall(pOut,"text5"); break;
      case JSONB_TEXTRAW:  sqlite3_str_appendall(pOut,"textraw"); break;
      case JSONB_ARRAY: {
        sqlite3_str_appendf(pOut,"array, %u bytes\n", sz);
        jsonDebugPrintBlob(pParse, iStart+n, iStart+n+sz, nIndent+2, pOut);
        showContent = 0;
        break;
      }
      case JSONB_OBJECT: {
        sqlite3_str_appendf(pOut, "object, %u bytes\n", sz);
        jsonDebugPrintBlob(pParse, iStart+n, iStart+n+sz, nIndent+2, pOut);
        showContent = 0;
        break;
      }
      default: {
        sqlite3_str_appendall(pOut, "ERROR: unknown node type\n");
        showContent = 0;
        break;
      }
    }
    if( showContent ){
      if( sz==0 && x<=JSONB_FALSE ){
        sqlite3_str_append(pOut, "\n", 1);
      }else{
        u32 j;
        sqlite3_str_appendall(pOut, ": \"");
        for(j=iStart+n; j<iStart+n+sz; j++){
          u8 c = pParse->aBlob[j];
          if( c<0x20 || c>=0x7f ) c = '.';
          sqlite3_str_append(pOut, (char*)&c, 1);
        }
        sqlite3_str_append(pOut, "\"\n", 2);
      }
    }
    iStart += n + sz;
  }
}
static void jsonShowParse(JsonParse *pParse){
  sqlite3_str out;
  char zBuf[1000];
  if( pParse==0 ){
    printf("NULL pointer\n");
    return;
  }else{
    printf("nBlobAlloc = %u\n", pParse->nBlobAlloc);
    printf("nBlob = %u\n", pParse->nBlob);
    printf("delta = %d\n", pParse->delta);
    if( pParse->nBlob==0 ) return;
    printf("content (bytes 0..%u):\n", pParse->nBlob-1);
  }
  sqlite3StrAccumInit(&out, 0, zBuf, sizeof(zBuf), 1000000);
  jsonDebugPrintBlob(pParse, 0, pParse->nBlob, 0, &out);
  printf("%s", sqlite3_str_value(&out));
  sqlite3_str_reset(&out);
}
#endif /* SQLITE_DEBUG */

#ifdef SQLITE_DEBUG
/*
** SQL function:   json_parse(JSON)
**
** Parse JSON using jsonParseFuncArg().  Return text that is a
** human-readable dump of the binary JSONB for the input parameter.
*/
static void jsonParseFunc(
  sqlite3_context *ctx,
  int argc,
  sqlite3_value **argv
){
  JsonParse *p;        /* The parse */
  sqlite3_str out;

  assert( argc>=1 );
  sqlite3StrAccumInit(&out, 0, 0, 0, 1000000);
  p = jsonParseFuncArg(ctx, argv[0], 0);
  if( p==0 ) return;
  if( argc==1 ){
    jsonDebugPrintBlob(p, 0, p->nBlob, 0, &out);
    sqlite3_result_text64(ctx,out.zText,out.nChar,SQLITE_TRANSIENT,SQLITE_UTF8);
  }else{
    jsonShowParse(p);
  }
  jsonParseFree(p);
  sqlite3_str_reset(&out);
}
#endif /* SQLITE_DEBUG */

/****************************************************************************
** Scalar SQL function implementations
****************************************************************************/

/*
** Implementation of the json_quote(VALUE) function.  Return a JSON value
** corresponding to the SQL value input.  Mostly this means putting
** double-quotes around strings and returning the unquoted string "null"
** when given a NULL input.
*/
static void jsonQuoteFunc(
  sqlite3_context *ctx,
  int argc,
  sqlite3_value **argv
){
  JsonString jx;
  UNUSED_PARAMETER(argc);

  jsonStringInit(&jx, ctx);
  jsonAppendSqlValue(&jx, argv[0]);
  jsonReturnString(&jx, 0, 0);
  sqlite3_result_subtype(ctx, JSON_SUBTYPE);
}

/*
** Implementation of the json_array(VALUE,...) function.  Return a JSON
** array that contains all values given in arguments.  Or if any argument
** is a BLOB, throw an error.
*/
static void jsonArrayFunc(
  sqlite3_context *ctx,
  int argc,
  sqlite3_value **argv
){
  int i;
  JsonString jx;

  jsonStringInit(&jx, ctx);
  jsonAppendChar(&jx, '[');
  for(i=0; i<argc; i++){
    jsonAppendSeparator(&jx);
    jsonAppendSqlValue(&jx, argv[i]);
  }
  jsonAppendChar(&jx, ']');
  jsonReturnString(&jx, 0, 0);
  sqlite3_result_subtype(ctx, JSON_SUBTYPE);
}

/*
** json_array_length(JSON)
** json_array_length(JSON, PATH)
**
** Return the number of elements in the top-level JSON array.
** Return 0 if the input is not a well-formed JSON array.
*/
static void jsonArrayLengthFunc(
  sqlite3_context *ctx,
  int argc,
  sqlite3_value **argv
){
  JsonParse *p;          /* The parse */
  sqlite3_int64 cnt = 0;
  u32 i;
  u8 eErr = 0;

  p = jsonParseFuncArg(ctx, argv[0], 0);
  if( p==0 ) return;
  if( argc==2 ){
    const char *zPath = (const char*)sqlite3_value_text(argv[1]);
    if( zPath==0 ){
      jsonParseFree(p);
      return;
    }
    i = jsonLookupStep(p, 0, zPath[0]=='$' ? zPath+1 : "@", 0);
    if( JSON_LOOKUP_ISERROR(i) ){
      if( i==JSON_LOOKUP_NOTFOUND ){
        /* no-op */
      }else if( i==JSON_LOOKUP_PATHERROR ){
        jsonBadPathError(ctx, zPath);
      }else{
        sqlite3_result_error(ctx, "malformed JSON", -1);
      }
      eErr = 1;
      i = 0;
    }
  }else{
    i = 0;
  }
  if( (p->aBlob[i] & 0x0f)==JSONB_ARRAY ){
    cnt = jsonbArrayCount(p, i);
  }
  if( !eErr ) sqlite3_result_int64(ctx, cnt);
  jsonParseFree(p);
}

/* True if the string is all alphanumerics and underscores */
static int jsonAllAlphanum(const char *z, int n){
  int i;
  for(i=0; i<n && (sqlite3Isalnum(z[i]) || z[i]=='_'); i++){}
  return i==n;
}

/*
** json_extract(JSON, PATH, ...)
** "->"(JSON,PATH)
** "->>"(JSON,PATH)
**
** Return the element described by PATH.  Return NULL if that PATH element
** is not found.
**
** If JSON_JSON is set or if more that one PATH argument is supplied then
** always return a JSON representation of the result.  If JSON_SQL is set,
** then always return an SQL representation of the result.  If neither flag
** is present and argc==2, then return JSON for objects and arrays and SQL
** for all other values.
**
** When multiple PATH arguments are supplied, the result is a JSON array
** containing the result of each PATH.
**
** Abbreviated JSON path expressions are allows if JSON_ABPATH, for
** compatibility with PG.
*/
static void jsonExtractFunc(
  sqlite3_context *ctx,
  int argc,
  sqlite3_value **argv
){
  JsonParse *p = 0;      /* The parse */
  int flags;             /* Flags associated with the function */
  int i;                 /* Loop counter */
  JsonString jx;         /* String for array result */

  if( argc<2 ) return;
  p = jsonParseFuncArg(ctx, argv[0], 0);
  if( p==0 ) return;
  flags = SQLITE_PTR_TO_INT(sqlite3_user_data(ctx));
  jsonStringInit(&jx, ctx);
  if( argc>2 ){
    jsonAppendChar(&jx, '[');
  }
  for(i=1; i<argc; i++){
    /* With a single PATH argument */
    const char *zPath = (const char*)sqlite3_value_text(argv[i]);
    int nPath;
    u32 j;
    if( zPath==0 ) goto json_extract_error;
    nPath = sqlite3Strlen30(zPath);
    if( zPath[0]=='$' ){
      j = jsonLookupStep(p, 0, zPath+1, 0);
    }else if( (flags & JSON_ABPATH) ){
      /* The -> and ->> operators accept abbreviated PATH arguments.  This
      ** is mostly for compatibility with PostgreSQL, but also for
      ** convenience.
      **
      **     NUMBER   ==>  $[NUMBER]     // PG compatible
      **     LABEL    ==>  $.LABEL       // PG compatible
      **     [NUMBER] ==>  $[NUMBER]     // Not PG.  Purely for convenience
      */
      jsonStringInit(&jx, ctx);
      if( sqlite3_value_type(argv[i])==SQLITE_INTEGER ){
        jsonAppendRawNZ(&jx, "[", 1);
        jsonAppendRaw(&jx, zPath, nPath);
        jsonAppendRawNZ(&jx, "]", 2);
      }else if( jsonAllAlphanum(zPath, nPath) ){
        jsonAppendRawNZ(&jx, ".", 1);
        jsonAppendRaw(&jx, zPath, nPath);
      }else if( zPath[0]=='[' && nPath>=3 && zPath[nPath-1]==']' ){
        jsonAppendRaw(&jx, zPath, nPath);
      }else{
        jsonAppendRawNZ(&jx, ".\"", 2);
        jsonAppendRaw(&jx, zPath, nPath);
        jsonAppendRawNZ(&jx, "\"", 1);
      }
      jsonStringTerminate(&jx);
      j = jsonLookupStep(p, 0, jx.zBuf, 0);
      jsonStringReset(&jx);
    }else{
      jsonBadPathError(ctx, zPath);
      goto json_extract_error;
    }
    if( j<p->nBlob ){
      if( argc==2 ){
        if( flags & JSON_JSON ){
          jsonStringInit(&jx, ctx);
          jsonTranslateBlobToText(p, j, &jx);
          jsonReturnString(&jx, 0, 0);
          jsonStringReset(&jx);
          assert( (flags & JSON_BLOB)==0 );
          sqlite3_result_subtype(ctx, JSON_SUBTYPE);
        }else{
          jsonReturnFromBlob(p, j, ctx, 0);
          if( (flags & (JSON_SQL|JSON_BLOB))==0
           && (p->aBlob[j]&0x0f)>=JSONB_ARRAY
          ){
            sqlite3_result_subtype(ctx, JSON_SUBTYPE);
          }
        }
      }else{
        jsonAppendSeparator(&jx);
        jsonTranslateBlobToText(p, j, &jx);
      }
    }else if( j==JSON_LOOKUP_NOTFOUND ){
      if( argc==2 ){
        goto json_extract_error;  /* Return NULL if not found */
      }else{
        jsonAppendSeparator(&jx);
        jsonAppendRawNZ(&jx, "null", 4);
      }
    }else if( j==JSON_LOOKUP_ERROR ){
      sqlite3_result_error(ctx, "malformed JSON", -1);
      goto json_extract_error;
    }else{
      jsonBadPathError(ctx, zPath);
      goto json_extract_error;
    }
  }
  if( argc>2 ){
    jsonAppendChar(&jx, ']');
    jsonReturnString(&jx, 0, 0);
    if( (flags & JSON_BLOB)==0 ){
      sqlite3_result_subtype(ctx, JSON_SUBTYPE);
    }
  }
json_extract_error:
  jsonStringReset(&jx);
  jsonParseFree(p);
  return;
}
