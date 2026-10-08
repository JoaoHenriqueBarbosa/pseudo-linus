
/*
** pArg is a function argument that might be an SQL value or a JSON
** value.  Figure out what it is and encode it as a JSONB blob.
** Return the results in pParse.
**
** pParse is uninitialized upon entry.  This routine will handle the
** initialization of pParse.  The result will be contained in
** pParse->aBlob and pParse->nBlob.  pParse->aBlob might be dynamically
** allocated (if pParse->nBlobAlloc is greater than zero) in which case
** the caller is responsible for freeing the space allocated to pParse->aBlob
** when it has finished with it.  Or pParse->aBlob might be a static string
** or a value obtained from sqlite3_value_blob(pArg).
**
** If the argument is a BLOB that is clearly not a JSONB, then this
** function might set an error message in ctx and return non-zero.
** It might also set an error message and return non-zero on an OOM error.
*/
static int jsonFunctionArgToBlob(
  sqlite3_context *ctx,
  sqlite3_value *pArg,
  JsonParse *pParse
){
  int eType = sqlite3_value_type(pArg);
  static u8 aNull[] = { 0x00 };
  memset(pParse, 0, sizeof(pParse[0]));
  pParse->db = sqlite3_context_db_handle(ctx);
  switch( eType ){
    default: {
      pParse->aBlob = aNull;
      pParse->nBlob = 1;
      return 0;
    }
    case SQLITE_BLOB: {
      if( jsonFuncArgMightBeBinary(pArg) ){
        pParse->aBlob = (u8*)sqlite3_value_blob(pArg);
        pParse->nBlob = sqlite3_value_bytes(pArg);
      }else{
        sqlite3_result_error(ctx, "JSON cannot hold BLOB values", -1);
        return 1;
      }
      break;
    }
    case SQLITE_TEXT: {
      const char *zJson = (const char*)sqlite3_value_text(pArg);
      int nJson = sqlite3_value_bytes(pArg);
      if( zJson==0 ) return 1;
      if( sqlite3_value_subtype(pArg)==JSON_SUBTYPE ){
        pParse->zJson = (char*)zJson;
        pParse->nJson = nJson;
        if( jsonConvertTextToBlob(pParse, ctx) ){
          sqlite3_result_error(ctx, "malformed JSON", -1);
          sqlite3DbFree(pParse->db, pParse->aBlob);
          memset(pParse, 0, sizeof(pParse[0]));
          return 1;
        }
      }else{
        jsonBlobAppendNode(pParse, JSONB_TEXTRAW, nJson, zJson);
      }
      break;
    }
    case SQLITE_FLOAT: {
      double r = sqlite3_value_double(pArg);
      if( NEVER(sqlite3IsNaN(r)) ){
        jsonBlobAppendNode(pParse, JSONB_NULL, 0, 0);
      }else{
        int n = sqlite3_value_bytes(pArg);
        const char *z = (const char*)sqlite3_value_text(pArg);
        if( z==0 ) return 1;
        if( z[0]=='I' ){
          jsonBlobAppendNode(pParse, JSONB_FLOAT, 5, "9e999");
        }else if( z[0]=='-' && z[1]=='I' ){
          jsonBlobAppendNode(pParse, JSONB_FLOAT, 6, "-9e999");
        }else{
          jsonBlobAppendNode(pParse, JSONB_FLOAT, n, z);
        }
      }
      break;
    }
    case SQLITE_INTEGER: {
      int n = sqlite3_value_bytes(pArg);
      const char *z = (const char*)sqlite3_value_text(pArg);
      if( z==0 ) return 1;
      jsonBlobAppendNode(pParse, JSONB_INT, n, z);
      break;
    }
  }
  if( pParse->oom ){
    sqlite3_result_error_nomem(ctx);
    return 1;
  }else{
    return 0;
  }
}

/*
** Generate a bad path error.
**
** If ctx is not NULL then push the error message into ctx and return NULL.
** If ctx is NULL, then return the text of the error message.
*/
static char *jsonBadPathError(
  sqlite3_context *ctx,     /* The function call containing the error */
  const char *zPath         /* The path with the problem */
){
  char *zMsg = sqlite3_mprintf("bad JSON path: %Q", zPath);
  if( ctx==0 ) return zMsg;
  if( zMsg ){
    sqlite3_result_error(ctx, zMsg, -1);
    sqlite3_free(zMsg);
  }else{
    sqlite3_result_error_nomem(ctx);
  }
  return 0;
}

/* argv[0] is a BLOB that seems likely to be a JSONB.  Subsequent
** arguments come in parse where each pair contains a JSON path and
** content to insert or set at that patch.  Do the updates
** and return the result.
**
** The specific operation is determined by eEdit, which can be one
** of JEDIT_INS, JEDIT_REPL, or JEDIT_SET.
*/
static void jsonInsertIntoBlob(
  sqlite3_context *ctx,
  int argc,
  sqlite3_value **argv,
  int eEdit                /* JEDIT_INS, JEDIT_REPL, or JEDIT_SET */
){
  int i;
  u32 rc = 0;
  const char *zPath = 0;
  int flgs;
  JsonParse *p;
  JsonParse ax;

  assert( (argc&1)==1 );
  flgs = argc==1 ? 0 : JSON_EDITABLE;
  p = jsonParseFuncArg(ctx, argv[0], flgs);
  if( p==0 ) return;
  for(i=1; i<argc-1; i+=2){
    if( sqlite3_value_type(argv[i])==SQLITE_NULL ) continue;
    zPath = (const char*)sqlite3_value_text(argv[i]);
    if( zPath==0 ){
      sqlite3_result_error_nomem(ctx);
      jsonParseFree(p);
      return;
    }
    if( zPath[0]!='$' ) goto jsonInsertIntoBlob_patherror;
    if( jsonFunctionArgToBlob(ctx, argv[i+1], &ax) ){
      jsonParseReset(&ax);
      jsonParseFree(p);
      return;
    }
    if( zPath[1]==0 ){
      if( eEdit==JEDIT_REPL || eEdit==JEDIT_SET ){
        jsonBlobEdit(p, 0, p->nBlob, ax.aBlob, ax.nBlob);
      }
      rc = 0;
   }else{
      p->eEdit = eEdit;
      p->nIns = ax.nBlob;
      p->aIns = ax.aBlob;
      p->delta = 0;
      rc = jsonLookupStep(p, 0, zPath+1, 0);
    }
    jsonParseReset(&ax);
    if( rc==JSON_LOOKUP_NOTFOUND ) continue;
    if( JSON_LOOKUP_ISERROR(rc) ) goto jsonInsertIntoBlob_patherror;
  }
  jsonReturnParse(ctx, p);
  jsonParseFree(p);
  return;

jsonInsertIntoBlob_patherror:
  jsonParseFree(p);
  if( rc==JSON_LOOKUP_ERROR ){
    sqlite3_result_error(ctx, "malformed JSON", -1);
  }else{
    jsonBadPathError(ctx, zPath);
  }
  return;
}

/*
** If pArg is a blob that seems like a JSONB blob, then initialize
** p to point to that JSONB and return TRUE.  If pArg does not seem like
** a JSONB blob, then return FALSE;
**
** This routine is only called if it is already known that pArg is a
** blob.  The only open question is whether or not the blob appears
** to be a JSONB blob.
*/
static int jsonArgIsJsonb(sqlite3_value *pArg, JsonParse *p){
  u32 n, sz = 0;
  p->aBlob = (u8*)sqlite3_value_blob(pArg);
  p->nBlob = (u32)sqlite3_value_bytes(pArg);
  if( p->nBlob==0 ){
    p->aBlob = 0;
    return 0;
  }
  if( NEVER(p->aBlob==0) ){
    return 0;
  }
  if( (p->aBlob[0] & 0x0f)<=JSONB_OBJECT
   && (n = jsonbPayloadSize(p, 0, &sz))>0
   && sz+n==p->nBlob
   && ((p->aBlob[0] & 0x0f)>JSONB_FALSE || sz==0)
  ){
    return 1;
  }
  p->aBlob = 0;
  p->nBlob = 0;
  return 0;
}

/*
** Generate a JsonParse object, containing valid JSONB in aBlob and nBlob,
** from the SQL function argument pArg.  Return a pointer to the new
** JsonParse object.
**
** Ownership of the new JsonParse object is passed to the caller.  The
** caller should invoke jsonParseFree() on the return value when it
** has finished using it.
**
** If any errors are detected, an appropriate error messages is set
** using sqlite3_result_error() or the equivalent and this routine
** returns NULL.  This routine also returns NULL if the pArg argument
** is an SQL NULL value, but no error message is set in that case.  This
** is so that SQL functions that are given NULL arguments will return
** a NULL value.
*/
static JsonParse *jsonParseFuncArg(
  sqlite3_context *ctx,
  sqlite3_value *pArg,
  u32 flgs
){
  int eType;                   /* Datatype of pArg */
  JsonParse *p = 0;            /* Value to be returned */
  JsonParse *pFromCache = 0;   /* Value taken from cache */
  sqlite3 *db;                 /* The database connection */

  assert( ctx!=0 );
  eType = sqlite3_value_type(pArg);
  if( eType==SQLITE_NULL ){
    return 0;
  }
  pFromCache = jsonCacheSearch(ctx, pArg);
  if( pFromCache ){
    pFromCache->nJPRef++;
    if( (flgs & JSON_EDITABLE)==0 ){
      return pFromCache;
    }
  }
  db = sqlite3_context_db_handle(ctx);
rebuild_from_cache:
  p = sqlite3DbMallocZero(db, sizeof(*p));
  if( p==0 ) goto json_pfa_oom;
  memset(p, 0, sizeof(*p));
  p->db = db;
  p->nJPRef = 1;
  if( pFromCache!=0 ){
    u32 nBlob = pFromCache->nBlob;
    p->aBlob = sqlite3DbMallocRaw(db, nBlob);
    if( p->aBlob==0 ) goto json_pfa_oom;
    memcpy(p->aBlob, pFromCache->aBlob, nBlob);
    p->nBlobAlloc = p->nBlob = nBlob;
    p->hasNonstd = pFromCache->hasNonstd;
    jsonParseFree(pFromCache);
    return p;
  }
  if( eType==SQLITE_BLOB ){
    if( jsonArgIsJsonb(pArg,p) ){
      if( (flgs & JSON_EDITABLE)!=0 && jsonBlobMakeEditable(p, 0)==0 ){
        goto json_pfa_oom;
      }
      return p;
    }
    /* If the blob is not valid JSONB, fall through into trying to cast
    ** the blob into text which is then interpreted as JSON.  (tag-20240123-a)
    **
    ** This goes against all historical documentation about how the SQLite
    ** JSON functions were suppose to work.  From the beginning, blob was
    ** reserved for expansion and a blob value should have raised an error.
    ** But it did not, due to a bug.  And many applications came to depend
    ** upon this buggy behavior, espeically when using the CLI and reading
    ** JSON text using readfile(), which returns a blob.  For this reason
    ** we will continue to support the bug moving forward.
    ** See for example https://sqlite.org/forum/forumpost/012136abd5292b8d
    */
  }
  p->zJson = (char*)sqlite3_value_text(pArg);
  p->nJson = sqlite3_value_bytes(pArg);
  if( db->mallocFailed ) goto json_pfa_oom;
  if( p->nJson==0 ) goto json_pfa_malformed;
  assert( p->zJson!=0 );
  if( jsonConvertTextToBlob(p, (flgs & JSON_KEEPERROR) ? 0 : ctx) ){
    if( flgs & JSON_KEEPERROR ){
      p->nErr = 1;
      return p;
    }else{
      jsonParseFree(p);
      return 0;
    }
  }else{
    int isRCStr = sqlite3ValueIsOfClass(pArg, sqlite3RCStrUnref);
    int rc;
    if( !isRCStr ){
      char *zNew = sqlite3RCStrNew( p->nJson );
      if( zNew==0 ) goto json_pfa_oom;
      memcpy(zNew, p->zJson, p->nJson);
      p->zJson = zNew;
      p->zJson[p->nJson] = 0;
    }else{
      sqlite3RCStrRef(p->zJson);
    }
    p->bJsonIsRCStr = 1;
    rc = jsonCacheInsert(ctx, p);
    if( rc==SQLITE_NOMEM ) goto json_pfa_oom;
    if( flgs & JSON_EDITABLE ){
      pFromCache = p;
      p = 0;
      goto rebuild_from_cache;
    }
  }
  return p;

json_pfa_malformed:
  if( flgs & JSON_KEEPERROR ){
    p->nErr = 1;
    return p;
  }else{
    jsonParseFree(p);
    sqlite3_result_error(ctx, "malformed JSON", -1);
    return 0;
  }

json_pfa_oom:
  jsonParseFree(pFromCache);
  jsonParseFree(p);
  sqlite3_result_error_nomem(ctx);
  return 0;
}

/*
** Make the return value of a JSON function either the raw JSONB blob
** or make it JSON text, depending on whether the JSON_BLOB flag is
** set on the function.
*/
static void jsonReturnParse(
  sqlite3_context *ctx,
  JsonParse *p
){
  int flgs;
  if( p->oom ){
    sqlite3_result_error_nomem(ctx);
    return;
  }
  flgs = SQLITE_PTR_TO_INT(sqlite3_user_data(ctx));
  if( flgs & JSON_BLOB ){
    if( p->nBlobAlloc>0 && !p->bReadOnly ){
      sqlite3_result_blob(ctx, p->aBlob, p->nBlob, SQLITE_DYNAMIC);
      p->nBlobAlloc = 0;
    }else{
      sqlite3_result_blob(ctx, p->aBlob, p->nBlob, SQLITE_TRANSIENT);
    }
  }else{
    JsonString s;
    jsonStringInit(&s, ctx);
    p->delta = 0;
    jsonTranslateBlobToText(p, 0, &s);
    jsonReturnString(&s, p, ctx);
    sqlite3_result_subtype(ctx, JSON_SUBTYPE);
  }
}
