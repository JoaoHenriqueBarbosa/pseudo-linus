
/*
** Compare two object labels.  Return 1 if they are equal and
** 0 if they differ.  Return -1 if an OOM occurs.
*/
static int jsonLabelCompare(
  const char *zLeft,          /* The left label */
  u32 nLeft,                  /* Size of the left label in bytes */
  int rawLeft,                /* True if zLeft contains no escapes */
  const char *zRight,         /* The right label */
  u32 nRight,                 /* Size of the right label in bytes */
  int rawRight                /* True if zRight is escape-free */
){
  if( rawLeft && rawRight ){
    /* Simpliest case:  Neither label contains escapes.  A simple
    ** memcmp() is sufficient. */
    if( nLeft!=nRight ) return 0;
    return memcmp(zLeft, zRight, nLeft)==0;
  }else{
    return jsonLabelCompareEscaped(zLeft, nLeft, rawLeft,
                                   zRight, nRight, rawRight);
  }
}

/*
** Error returns from jsonLookupStep()
*/
#define JSON_LOOKUP_ERROR      0xffffffff
#define JSON_LOOKUP_NOTFOUND   0xfffffffe
#define JSON_LOOKUP_PATHERROR  0xfffffffd
#define JSON_LOOKUP_ISERROR(x) ((x)>=JSON_LOOKUP_PATHERROR)

/* Forward declaration */
static u32 jsonLookupStep(JsonParse*,u32,const char*,u32);


/* This helper routine for jsonLookupStep() populates pIns with
** binary data that is to be inserted into pParse.
**
** In the common case, pIns just points to pParse->aIns and pParse->nIns.
** But if the zPath of the original edit operation includes path elements
** that go deeper, additional substructure must be created.
**
** For example:
**
**     json_insert('{}', '$.a.b.c', 123);
**
** The search stops at '$.a'  But additional substructure must be
** created for the ".b.c" part of the patch so that the final result
** is:  {"a":{"b":{"c"::123}}}.  This routine populates pIns with
** the binary equivalent of {"b":{"c":123}} so that it can be inserted.
**
** The caller is responsible for resetting pIns when it has finished
** using the substructure.
*/
static u32 jsonCreateEditSubstructure(
  JsonParse *pParse,  /* The original JSONB that is being edited */
  JsonParse *pIns,    /* Populate this with the blob data to insert */
  const char *zTail   /* Tail of the path that determins substructure */
){
  static const u8 emptyObject[] = { JSONB_ARRAY, JSONB_OBJECT };
  int rc;
  memset(pIns, 0, sizeof(*pIns));
  pIns->db = pParse->db;
  if( zTail[0]==0 ){
    /* No substructure.  Just insert what is given in pParse. */
    pIns->aBlob = pParse->aIns;
    pIns->nBlob = pParse->nIns;
    rc = 0;
  }else{
    /* Construct the binary substructure */
    pIns->nBlob = 1;
    pIns->aBlob = (u8*)&emptyObject[zTail[0]=='.'];
    pIns->eEdit = pParse->eEdit;
    pIns->nIns = pParse->nIns;
    pIns->aIns = pParse->aIns;
    rc = jsonLookupStep(pIns, 0, zTail, 0);
    pParse->oom |= pIns->oom;
  }
  return rc;  /* Error code only */
}

/*
** Search along zPath to find the Json element specified.  Return an
** index into pParse->aBlob[] for the start of that element's value.
**
** If the value found by this routine is the value half of label/value pair
** within an object, then set pPath->iLabel to the start of the corresponding
** label, before returning.
**
** Return one of the JSON_LOOKUP error codes if problems are seen.
**
** This routine will also modify the blob.  If pParse->eEdit is one of
** JEDIT_DEL, JEDIT_REPL, JEDIT_INS, or JEDIT_SET, then changes might be
** made to the selected value.  If an edit is performed, then the return
** value does not necessarily point to the select element.  If an edit
** is performed, the return value is only useful for detecting error
** conditions.
*/
static u32 jsonLookupStep(
  JsonParse *pParse,      /* The JSON to search */
  u32 iRoot,              /* Begin the search at this element of aBlob[] */
  const char *zPath,      /* The path to search */
  u32 iLabel              /* Label if iRoot is a value of in an object */
){
  u32 i, j, k, nKey, sz, n, iEnd, rc;
  const char *zKey;
  u8 x;

  if( zPath[0]==0 ){
    if( pParse->eEdit && jsonBlobMakeEditable(pParse, pParse->nIns) ){
      n = jsonbPayloadSize(pParse, iRoot, &sz);
      sz += n;
      if( pParse->eEdit==JEDIT_DEL ){
        if( iLabel>0 ){
          sz += iRoot - iLabel;
          iRoot = iLabel;
        }
        jsonBlobEdit(pParse, iRoot, sz, 0, 0);
      }else if( pParse->eEdit==JEDIT_INS ){
        /* Already exists, so json_insert() is a no-op */
      }else{
        /* json_set() or json_replace() */
        jsonBlobEdit(pParse, iRoot, sz, pParse->aIns, pParse->nIns);
      }
    }
    pParse->iLabel = iLabel;
    return iRoot;
  }
  if( zPath[0]=='.' ){
    int rawKey = 1;
    x = pParse->aBlob[iRoot];
    zPath++;
    if( zPath[0]=='"' ){
      zKey = zPath + 1;
      for(i=1; zPath[i] && zPath[i]!='"'; i++){}
      nKey = i-1;
      if( zPath[i] ){
        i++;
      }else{
        return JSON_LOOKUP_PATHERROR;
      }
      testcase( nKey==0 );
      rawKey = memchr(zKey, '\\', nKey)==0;
    }else{
      zKey = zPath;
      for(i=0; zPath[i] && zPath[i]!='.' && zPath[i]!='['; i++){}
      nKey = i;
      if( nKey==0 ){
        return JSON_LOOKUP_PATHERROR;
      }
    }
    if( (x & 0x0f)!=JSONB_OBJECT ) return JSON_LOOKUP_NOTFOUND;
    n = jsonbPayloadSize(pParse, iRoot, &sz);
    j = iRoot + n;  /* j is the index of a label */
    iEnd = j+sz;
    while( j<iEnd ){
      int rawLabel;
      const char *zLabel;
      x = pParse->aBlob[j] & 0x0f;
      if( x<JSONB_TEXT || x>JSONB_TEXTRAW ) return JSON_LOOKUP_ERROR;
      n = jsonbPayloadSize(pParse, j, &sz);
      if( n==0 ) return JSON_LOOKUP_ERROR;
      k = j+n;  /* k is the index of the label text */
      if( k+sz>=iEnd ) return JSON_LOOKUP_ERROR;
      zLabel = (const char*)&pParse->aBlob[k];
      rawLabel = x==JSONB_TEXT || x==JSONB_TEXTRAW;
      if( jsonLabelCompare(zKey, nKey, rawKey, zLabel, sz, rawLabel) ){
        u32 v = k+sz;  /* v is the index of the value */
        if( ((pParse->aBlob[v])&0x0f)>JSONB_OBJECT ) return JSON_LOOKUP_ERROR;
        n = jsonbPayloadSize(pParse, v, &sz);
        if( n==0 || v+n+sz>iEnd ) return JSON_LOOKUP_ERROR;
        assert( j>0 );
        rc = jsonLookupStep(pParse, v, &zPath[i], j);
        if( pParse->delta ) jsonAfterEditSizeAdjust(pParse, iRoot);
        return rc;
      }
      j = k+sz;
      if( ((pParse->aBlob[j])&0x0f)>JSONB_OBJECT ) return JSON_LOOKUP_ERROR;
      n = jsonbPayloadSize(pParse, j, &sz);
      if( n==0 ) return JSON_LOOKUP_ERROR;
      j += n+sz;
    }
    if( j>iEnd ) return JSON_LOOKUP_ERROR;
    if( pParse->eEdit>=JEDIT_INS ){
      u32 nIns;          /* Total bytes to insert (label+value) */
      JsonParse v;       /* BLOB encoding of the value to be inserted */
      JsonParse ix;      /* Header of the label to be inserted */
      testcase( pParse->eEdit==JEDIT_INS );
      testcase( pParse->eEdit==JEDIT_SET );
      memset(&ix, 0, sizeof(ix));
      ix.db = pParse->db;
      jsonBlobAppendNode(&ix, rawKey?JSONB_TEXTRAW:JSONB_TEXT5, nKey, 0);
      pParse->oom |= ix.oom;
      rc = jsonCreateEditSubstructure(pParse, &v, &zPath[i]);
      if( !JSON_LOOKUP_ISERROR(rc)
       && jsonBlobMakeEditable(pParse, ix.nBlob+nKey+v.nBlob)
      ){
        assert( !pParse->oom );
        nIns = ix.nBlob + nKey + v.nBlob;
        jsonBlobEdit(pParse, j, 0, 0, nIns);
        if( !pParse->oom ){
          assert( pParse->aBlob!=0 ); /* Because pParse->oom!=0 */
          assert( ix.aBlob!=0 );      /* Because pPasre->oom!=0 */
          memcpy(&pParse->aBlob[j], ix.aBlob, ix.nBlob);
          k = j + ix.nBlob;
          memcpy(&pParse->aBlob[k], zKey, nKey);
          k += nKey;
          memcpy(&pParse->aBlob[k], v.aBlob, v.nBlob);
          if( ALWAYS(pParse->delta) ) jsonAfterEditSizeAdjust(pParse, iRoot);
        }
      }
      jsonParseReset(&v);
      jsonParseReset(&ix);
      return rc;
    }
  }else if( zPath[0]=='[' ){
    x = pParse->aBlob[iRoot] & 0x0f;
    if( x!=JSONB_ARRAY )  return JSON_LOOKUP_NOTFOUND;
    n = jsonbPayloadSize(pParse, iRoot, &sz);
    k = 0;
    i = 1;
    while( sqlite3Isdigit(zPath[i]) ){
      k = k*10 + zPath[i] - '0';
      i++;
    }
    if( i<2 || zPath[i]!=']' ){
      if( zPath[1]=='#' ){
        k = jsonbArrayCount(pParse, iRoot);
        i = 2;
        if( zPath[2]=='-' && sqlite3Isdigit(zPath[3]) ){
          unsigned int nn = 0;
          i = 3;
          do{
            nn = nn*10 + zPath[i] - '0';
            i++;
          }while( sqlite3Isdigit(zPath[i]) );
          if( nn>k ) return JSON_LOOKUP_NOTFOUND;
          k -= nn;
        }
        if( zPath[i]!=']' ){
          return JSON_LOOKUP_PATHERROR;
        }
      }else{
        return JSON_LOOKUP_PATHERROR;
      }
    }
    j = iRoot+n;
    iEnd = j+sz;
    while( j<iEnd ){
      if( k==0 ){
        rc = jsonLookupStep(pParse, j, &zPath[i+1], 0);
        if( pParse->delta ) jsonAfterEditSizeAdjust(pParse, iRoot);
        return rc;
      }
      k--;
      n = jsonbPayloadSize(pParse, j, &sz);
      if( n==0 ) return JSON_LOOKUP_ERROR;
      j += n+sz;
    }
    if( j>iEnd ) return JSON_LOOKUP_ERROR;
    if( k>0 ) return JSON_LOOKUP_NOTFOUND;
    if( pParse->eEdit>=JEDIT_INS ){
      JsonParse v;
      testcase( pParse->eEdit==JEDIT_INS );
      testcase( pParse->eEdit==JEDIT_SET );
      rc = jsonCreateEditSubstructure(pParse, &v, &zPath[i+1]);
      if( !JSON_LOOKUP_ISERROR(rc)
       && jsonBlobMakeEditable(pParse, v.nBlob)
      ){
        assert( !pParse->oom );
        jsonBlobEdit(pParse, j, 0, v.aBlob, v.nBlob);
      }
      jsonParseReset(&v);
      if( pParse->delta ) jsonAfterEditSizeAdjust(pParse, iRoot);
      return rc;
    }
  }else{
    return JSON_LOOKUP_PATHERROR;
  }
  return JSON_LOOKUP_NOTFOUND;
}

/*
** Convert a JSON BLOB into text and make that text the return value
** of an SQL function.
*/
static void jsonReturnTextJsonFromBlob(
  sqlite3_context *ctx,
  const u8 *aBlob,
  u32 nBlob
){
  JsonParse x;
  JsonString s;

  if( NEVER(aBlob==0) ) return;
  memset(&x, 0, sizeof(x));
  x.aBlob = (u8*)aBlob;
  x.nBlob = nBlob;
  jsonStringInit(&s, ctx);
  jsonTranslateBlobToText(&x, 0, &s);
  jsonReturnString(&s, 0, 0);
}


/*
** Return the value of the BLOB node at index i.
**
** If the value is a primitive, return it as an SQL value.
** If the value is an array or object, return it as either
** JSON text or the BLOB encoding, depending on the JSON_B flag
** on the userdata.
*/
static void jsonReturnFromBlob(
  JsonParse *pParse,          /* Complete JSON parse tree */
  u32 i,                      /* Index of the node */
  sqlite3_context *pCtx,      /* Return value for this function */
  int textOnly                /* return text JSON.  Disregard user-data */
){
  u32 n, sz;
  int rc;
  sqlite3 *db = sqlite3_context_db_handle(pCtx);

  n = jsonbPayloadSize(pParse, i, &sz);
  if( n==0 ){
    sqlite3_result_error(pCtx, "malformed JSON", -1);
    return;
  }
  switch( pParse->aBlob[i] & 0x0f ){
    case JSONB_NULL: {
      if( sz ) goto returnfromblob_malformed;
      sqlite3_result_null(pCtx);
      break;
    }
    case JSONB_TRUE: {
      if( sz ) goto returnfromblob_malformed;
      sqlite3_result_int(pCtx, 1);
      break;
    }
    case JSONB_FALSE: {
      if( sz ) goto returnfromblob_malformed;
      sqlite3_result_int(pCtx, 0);
      break;
    }
    case JSONB_INT5:
    case JSONB_INT: {
      sqlite3_int64 iRes = 0;
      char *z;
      int bNeg = 0;
      char x;
      if( sz==0 ) goto returnfromblob_malformed;
      x = (char)pParse->aBlob[i+n];
      if( x=='-' ){
        if( sz<2 ) goto returnfromblob_malformed;
        n++;
        sz--;
        bNeg = 1;
      }
      z = sqlite3DbStrNDup(db, (const char*)&pParse->aBlob[i+n], (int)sz);
      if( z==0 ) goto returnfromblob_oom;
      rc = sqlite3DecOrHexToI64(z, &iRes);
      sqlite3DbFree(db, z);
      if( rc==0 ){
        sqlite3_result_int64(pCtx, bNeg ? -iRes : iRes);
      }else if( rc==3 && bNeg ){
        sqlite3_result_int64(pCtx, SMALLEST_INT64);
      }else if( rc==1 ){
        goto returnfromblob_malformed;
      }else{
        if( bNeg ){ n--; sz++; }
        goto to_double;
      }
      break;
    }
    case JSONB_FLOAT5:
    case JSONB_FLOAT: {
      double r;
      char *z;
      if( sz==0 ) goto returnfromblob_malformed;
    to_double:
      z = sqlite3DbStrNDup(db, (const char*)&pParse->aBlob[i+n], (int)sz);
      if( z==0 ) goto returnfromblob_oom;
      rc = sqlite3AtoF(z, &r, sqlite3Strlen30(z), SQLITE_UTF8);
      sqlite3DbFree(db, z);
      if( rc<=0 ) goto returnfromblob_malformed;
      sqlite3_result_double(pCtx, r);
      break;
    }
    case JSONB_TEXTRAW:
    case JSONB_TEXT: {
      sqlite3_result_text(pCtx, (char*)&pParse->aBlob[i+n], sz,
                          SQLITE_TRANSIENT);
      break;
    }
    case JSONB_TEXT5:
    case JSONB_TEXTJ: {
      /* Translate JSON formatted string into raw text */
      u32 iIn, iOut;
      const char *z;
      char *zOut;
      u32 nOut = sz;
      z = (const char*)&pParse->aBlob[i+n];
      zOut = sqlite3DbMallocRaw(db, nOut+1);
      if( zOut==0 ) goto returnfromblob_oom;
      for(iIn=iOut=0; iIn<sz; iIn++){
        char c = z[iIn];
        if( c=='\\' ){
          u32 v;
          u32 szEscape = jsonUnescapeOneChar(&z[iIn], sz-iIn, &v);
          if( v<=0x7f ){
            zOut[iOut++] = (char)v;
          }else if( v<=0x7ff ){
            assert( szEscape>=2 );
            zOut[iOut++] = (char)(0xc0 | (v>>6));
            zOut[iOut++] = 0x80 | (v&0x3f);
          }else if( v<0x10000 ){
            assert( szEscape>=3 );
            zOut[iOut++] = 0xe0 | (v>>12);
            zOut[iOut++] = 0x80 | ((v>>6)&0x3f);
            zOut[iOut++] = 0x80 | (v&0x3f);
          }else if( v==JSON_INVALID_CHAR ){
            /* Silently ignore illegal unicode */
          }else{
            assert( szEscape>=4 );
            zOut[iOut++] = 0xf0 | (v>>18);
            zOut[iOut++] = 0x80 | ((v>>12)&0x3f);
            zOut[iOut++] = 0x80 | ((v>>6)&0x3f);
            zOut[iOut++] = 0x80 | (v&0x3f);
          }
          iIn += szEscape - 1;
        }else{
          zOut[iOut++] = c;
        }
      } /* end for() */
      assert( iOut<=nOut );
      zOut[iOut] = 0;
      sqlite3_result_text(pCtx, zOut, iOut, SQLITE_DYNAMIC);
      break;
    }
    case JSONB_ARRAY:
    case JSONB_OBJECT: {
      int flags = textOnly ? 0 : SQLITE_PTR_TO_INT(sqlite3_user_data(pCtx));
      if( flags & JSON_BLOB ){
        sqlite3_result_blob(pCtx, &pParse->aBlob[i], sz+n, SQLITE_TRANSIENT);
      }else{
        jsonReturnTextJsonFromBlob(pCtx, &pParse->aBlob[i], sz+n);
      }
      break;
    }
    default: {
      goto returnfromblob_malformed;
    }
  }
  return;

returnfromblob_oom:
  sqlite3_result_error_nomem(pCtx);
  return;

returnfromblob_malformed:
  sqlite3_result_error(pCtx, "malformed JSON", -1);
  return;
}
