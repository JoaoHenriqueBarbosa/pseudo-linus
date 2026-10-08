
/*
** json_type(JSON)
** json_type(JSON, PATH)
**
** Return the top-level "type" of a JSON string.  json_type() raises an
** error if either the JSON or PATH inputs are not well-formed.
*/
static void jsonTypeFunc(
  sqlite3_context *ctx,
  int argc,
  sqlite3_value **argv
){
  JsonParse *p;          /* The parse */
  const char *zPath = 0;
  u32 i;

  p = jsonParseFuncArg(ctx, argv[0], 0);
  if( p==0 ) return;
  if( argc==2 ){
    zPath = (const char*)sqlite3_value_text(argv[1]);
    if( zPath==0 ) goto json_type_done;
    if( zPath[0]!='$' ){
      jsonBadPathError(ctx, zPath);
      goto json_type_done;
    }
    i = jsonLookupStep(p, 0, zPath+1, 0);
    if( JSON_LOOKUP_ISERROR(i) ){
      if( i==JSON_LOOKUP_NOTFOUND ){
        /* no-op */
      }else if( i==JSON_LOOKUP_PATHERROR ){
        jsonBadPathError(ctx, zPath);
      }else{
        sqlite3_result_error(ctx, "malformed JSON", -1);
      }
      goto json_type_done;
    }
  }else{
    i = 0;
  }
  sqlite3_result_text(ctx, jsonbType[p->aBlob[i]&0x0f], -1, SQLITE_STATIC);
json_type_done:
  jsonParseFree(p);
}

/*
** json_pretty(JSON)
** json_pretty(JSON, INDENT)
**
** Return text that is a pretty-printed rendering of the input JSON.
** If the argument is not valid JSON, return NULL.
**
** The INDENT argument is text that is used for indentation.  If omitted,
** it defaults to four spaces (the same as PostgreSQL).
*/
static void jsonPrettyFunc(
  sqlite3_context *ctx,
  int argc,
  sqlite3_value **argv
){
  JsonString s;          /* The output string */
  JsonPretty x;          /* Pretty printing context */

  memset(&x, 0, sizeof(x));
  x.pParse = jsonParseFuncArg(ctx, argv[0], 0);
  if( x.pParse==0 ) return;
  x.pOut = &s;
  jsonStringInit(&s, ctx);
  if( argc==1 || (x.zIndent = (const char*)sqlite3_value_text(argv[1]))==0 ){
    x.zIndent = "    ";
    x.szIndent = 4;
  }else{
    x.szIndent = (u32)strlen(x.zIndent);
  }
  jsonTranslateBlobToPrettyText(&x, 0);
  jsonReturnString(&s, 0, 0);
  jsonParseFree(x.pParse);
}

/*
** json_valid(JSON)
** json_valid(JSON, FLAGS)
**
** Check the JSON argument to see if it is well-formed.  The FLAGS argument
** encodes the various constraints on what is meant by "well-formed":
**
**     0x01      Canonical RFC-8259 JSON text
**     0x02      JSON text with optional JSON-5 extensions
**     0x04      Superficially appears to be JSONB
**     0x08      Strictly well-formed JSONB
**
** If the FLAGS argument is omitted, it defaults to 1.  Useful values for
** FLAGS include:
**
**    1          Strict canonical JSON text
**    2          JSON text perhaps with JSON-5 extensions
**    4          Superficially appears to be JSONB
**    5          Canonical JSON text or superficial JSONB
**    6          JSON-5 text or superficial JSONB
**    8          Strict JSONB
**    9          Canonical JSON text or strict JSONB
**    10         JSON-5 text or strict JSONB
**
** Other flag combinations are redundant.  For example, every canonical
** JSON text is also well-formed JSON-5 text, so FLAG values 2 and 3
** are the same.  Similarly, any input that passes a strict JSONB validation
** will also pass the superficial validation so 12 through 15 are the same
** as 8 through 11 respectively.
**
** This routine runs in linear time to validate text and when doing strict
** JSONB validation.  Superficial JSONB validation is constant time,
** assuming the BLOB is already in memory.  The performance advantage
** of superficial JSONB validation is why that option is provided.
** Application developers can choose to do fast superficial validation or
** slower strict validation, according to their specific needs.
**
** Only the lower four bits of the FLAGS argument are currently used.
** Higher bits are reserved for future expansion.   To facilitate
** compatibility, the current implementation raises an error if any bit
** in FLAGS is set other than the lower four bits.
**
** The original circa 2015 implementation of the JSON routines in
** SQLite only supported canonical RFC-8259 JSON text and the json_valid()
** function only accepted one argument.  That is why the default value
** for the FLAGS argument is 1, since FLAGS=1 causes this routine to only
** recognize canonical RFC-8259 JSON text as valid.  The extra FLAGS
** argument was added when the JSON routines were extended to support
** JSON5-like extensions and binary JSONB stored in BLOBs.
**
** Return Values:
**
**   *   Raise an error if FLAGS is outside the range of 1 to 15.
**   *   Return NULL if the input is NULL
**   *   Return 1 if the input is well-formed.
**   *   Return 0 if the input is not well-formed.
*/
static void jsonValidFunc(
  sqlite3_context *ctx,
  int argc,
  sqlite3_value **argv
){
  JsonParse *p;          /* The parse */
  u8 flags = 1;
  u8 res = 0;
  if( argc==2 ){
    i64 f = sqlite3_value_int64(argv[1]);
    if( f<1 || f>15 ){
      sqlite3_result_error(ctx, "FLAGS parameter to json_valid() must be"
                                " between 1 and 15", -1);
      return;
    }
    flags = f & 0x0f;
  }
  switch( sqlite3_value_type(argv[0]) ){
    case SQLITE_NULL: {
#ifdef SQLITE_LEGACY_JSON_VALID
      /* Incorrect legacy behavior was to return FALSE for a NULL input */
      sqlite3_result_int(ctx, 0);
#endif
      return;
    }
    case SQLITE_BLOB: {
      if( jsonFuncArgMightBeBinary(argv[0]) ){
        if( flags & 0x04 ){
          /* Superficial checking only - accomplished by the
          ** jsonFuncArgMightBeBinary() call above. */
          res = 1;
        }else if( flags & 0x08 ){
          /* Strict checking.  Check by translating BLOB->TEXT->BLOB.  If
          ** no errors occur, call that a "strict check". */
          JsonParse px;
          u32 iErr;
          memset(&px, 0, sizeof(px));
          px.aBlob = (u8*)sqlite3_value_blob(argv[0]);
          px.nBlob = sqlite3_value_bytes(argv[0]);
          iErr = jsonbValidityCheck(&px, 0, px.nBlob, 1);
          res = iErr==0;
        }
        break;
      }
      /* Fall through into interpreting the input as text.  See note
      ** above at tag-20240123-a. */
      /* no break */ deliberate_fall_through
    }
    default: {
      JsonParse px;
      if( (flags & 0x3)==0 ) break;
      memset(&px, 0, sizeof(px));

      p = jsonParseFuncArg(ctx, argv[0], JSON_KEEPERROR);
      if( p ){
        if( p->oom ){
          sqlite3_result_error_nomem(ctx);
        }else if( p->nErr ){
          /* no-op */
        }else if( (flags & 0x02)!=0 || p->hasNonstd==0 ){
          res = 1;
        }
        jsonParseFree(p);
      }else{
        sqlite3_result_error_nomem(ctx);
      }
      break;
    }
  }
  sqlite3_result_int(ctx, res);
}

/*
** json_error_position(JSON)
**
** If the argument is NULL, return NULL
**
** If the argument is BLOB, do a full validity check and return non-zero
** if the check fails.  The return value is the approximate 1-based offset
** to the byte of the element that contains the first error.
**
** Otherwise interpret the argument is TEXT (even if it is numeric) and
** return the 1-based character position for where the parser first recognized
** that the input was not valid JSON, or return 0 if the input text looks
** ok.  JSON-5 extensions are accepted.
*/
static void jsonErrorFunc(
  sqlite3_context *ctx,
  int argc,
  sqlite3_value **argv
){
  i64 iErrPos = 0;       /* Error position to be returned */
  JsonParse s;

  assert( argc==1 );
  UNUSED_PARAMETER(argc);
  memset(&s, 0, sizeof(s));
  s.db = sqlite3_context_db_handle(ctx);
  if( jsonFuncArgMightBeBinary(argv[0]) ){
    s.aBlob = (u8*)sqlite3_value_blob(argv[0]);
    s.nBlob = sqlite3_value_bytes(argv[0]);
    iErrPos = (i64)jsonbValidityCheck(&s, 0, s.nBlob, 1);
  }else{
    s.zJson = (char*)sqlite3_value_text(argv[0]);
    if( s.zJson==0 ) return;  /* NULL input or OOM */
    s.nJson = sqlite3_value_bytes(argv[0]);
    if( jsonConvertTextToBlob(&s,0) ){
      if( s.oom ){
        iErrPos = -1;
      }else{
        /* Convert byte-offset s.iErr into a character offset */
        u32 k;
        assert( s.zJson!=0 );  /* Because s.oom is false */
        for(k=0; k<s.iErr && ALWAYS(s.zJson[k]); k++){
          if( (s.zJson[k] & 0xc0)!=0x80 ) iErrPos++;
        }
        iErrPos++;
      }
    }
  }
  jsonParseReset(&s);
  if( iErrPos<0 ){
    sqlite3_result_error_nomem(ctx);
  }else{
    sqlite3_result_int64(ctx, iErrPos);
  }
}

/****************************************************************************
** Aggregate SQL function implementations
****************************************************************************/
/*
** json_group_array(VALUE)
**
** Return a JSON array composed of all values in the aggregate.
*/
static void jsonArrayStep(
  sqlite3_context *ctx,
  int argc,
  sqlite3_value **argv
){
  JsonString *pStr;
  UNUSED_PARAMETER(argc);
  pStr = (JsonString*)sqlite3_aggregate_context(ctx, sizeof(*pStr));
  if( pStr ){
    if( pStr->zBuf==0 ){
      jsonStringInit(pStr, ctx);
      jsonAppendChar(pStr, '[');
    }else if( pStr->nUsed>1 ){
      jsonAppendChar(pStr, ',');
    }
    pStr->pCtx = ctx;
    jsonAppendSqlValue(pStr, argv[0]);
  }
}
static void jsonArrayCompute(sqlite3_context *ctx, int isFinal){
  JsonString *pStr;
  pStr = (JsonString*)sqlite3_aggregate_context(ctx, 0);
  if( pStr ){
    int flags;
    pStr->pCtx = ctx;
    jsonAppendChar(pStr, ']');
    flags = SQLITE_PTR_TO_INT(sqlite3_user_data(ctx));
    if( pStr->eErr ){
      jsonReturnString(pStr, 0, 0);
      return;
    }else if( flags & JSON_BLOB ){
      jsonReturnStringAsBlob(pStr);
      if( isFinal ){
        if( !pStr->bStatic ) sqlite3RCStrUnref(pStr->zBuf);
      }else{
        jsonStringTrimOneChar(pStr);
      }
      return;
    }else if( isFinal ){
      sqlite3_result_text(ctx, pStr->zBuf, (int)pStr->nUsed,
                          pStr->bStatic ? SQLITE_TRANSIENT :
                              sqlite3RCStrUnref);
      pStr->bStatic = 1;
    }else{
      sqlite3_result_text(ctx, pStr->zBuf, (int)pStr->nUsed, SQLITE_TRANSIENT);
      jsonStringTrimOneChar(pStr);
    }
  }else{
    sqlite3_result_text(ctx, "[]", 2, SQLITE_STATIC);
  }
  sqlite3_result_subtype(ctx, JSON_SUBTYPE);
}
static void jsonArrayValue(sqlite3_context *ctx){
  jsonArrayCompute(ctx, 0);
}
static void jsonArrayFinal(sqlite3_context *ctx){
  jsonArrayCompute(ctx, 1);
}

#ifndef SQLITE_OMIT_WINDOWFUNC
/*
** This method works for both json_group_array() and json_group_object().
** It works by removing the first element of the group by searching forward
** to the first comma (",") that is not within a string and deleting all
** text through that comma.
*/
static void jsonGroupInverse(
  sqlite3_context *ctx,
  int argc,
  sqlite3_value **argv
){
  unsigned int i;
  int inStr = 0;
  int nNest = 0;
  char *z;
  char c;
  JsonString *pStr;
  UNUSED_PARAMETER(argc);
  UNUSED_PARAMETER(argv);
  pStr = (JsonString*)sqlite3_aggregate_context(ctx, 0);
#ifdef NEVER
  /* pStr is always non-NULL since jsonArrayStep() or jsonObjectStep() will
  ** always have been called to initialize it */
  if( NEVER(!pStr) ) return;
#endif
  z = pStr->zBuf;
  for(i=1; i<pStr->nUsed && ((c = z[i])!=',' || inStr || nNest); i++){
    if( c=='"' ){
      inStr = !inStr;
    }else if( c=='\\' ){
      i++;
    }else if( !inStr ){
      if( c=='{' || c=='[' ) nNest++;
      if( c=='}' || c==']' ) nNest--;
    }
  }
  if( i<pStr->nUsed ){
    pStr->nUsed -= i;
    memmove(&z[1], &z[i+1], (size_t)pStr->nUsed-1);
    z[pStr->nUsed] = 0;
  }else{
    pStr->nUsed = 1;
  }
}
