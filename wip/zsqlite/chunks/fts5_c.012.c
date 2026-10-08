
/*
** Populate the Fts5Config.zContentExprlist string.
*/
static int fts5ConfigMakeExprlist(Fts5Config *p){
  int i;
  int rc = SQLITE_OK;
  Fts5Buffer buf = {0, 0, 0};

  sqlite3Fts5BufferAppendPrintf(&rc, &buf, "T.%Q", p->zContentRowid);
  if( p->eContent!=FTS5_CONTENT_NONE ){
    for(i=0; i<p->nCol; i++){
      if( p->eContent==FTS5_CONTENT_EXTERNAL ){
        sqlite3Fts5BufferAppendPrintf(&rc, &buf, ", T.%Q", p->azCol[i]);
      }else{
        sqlite3Fts5BufferAppendPrintf(&rc, &buf, ", T.c%d", i);
      }
    }
  }

  assert( p->zContentExprlist==0 );
  p->zContentExprlist = (char*)buf.p;
  return rc;
}

/*
** Arguments nArg/azArg contain the string arguments passed to the xCreate
** or xConnect method of the virtual table. This function attempts to
** allocate an instance of Fts5Config containing the results of parsing
** those arguments.
**
** If successful, SQLITE_OK is returned and *ppOut is set to point to the
** new Fts5Config object. If an error occurs, an SQLite error code is
** returned, *ppOut is set to NULL and an error message may be left in
** *pzErr. It is the responsibility of the caller to eventually free any
** such error message using sqlite3_free().
*/
static int sqlite3Fts5ConfigParse(
  Fts5Global *pGlobal,
  sqlite3 *db,
  int nArg,                       /* Number of arguments */
  const char **azArg,             /* Array of nArg CREATE VIRTUAL TABLE args */
  Fts5Config **ppOut,             /* OUT: Results of parse */
  char **pzErr                    /* OUT: Error message */
){
  int rc = SQLITE_OK;             /* Return code */
  Fts5Config *pRet;               /* New object to return */
  int i;
  sqlite3_int64 nByte;

  *ppOut = pRet = (Fts5Config*)sqlite3_malloc(sizeof(Fts5Config));
  if( pRet==0 ) return SQLITE_NOMEM;
  memset(pRet, 0, sizeof(Fts5Config));
  pRet->db = db;
  pRet->iCookie = -1;

  nByte = nArg * (sizeof(char*) + sizeof(u8));
  pRet->azCol = (char**)sqlite3Fts5MallocZero(&rc, nByte);
  pRet->abUnindexed = pRet->azCol ? (u8*)&pRet->azCol[nArg] : 0;
  pRet->zDb = sqlite3Fts5Strndup(&rc, azArg[1], -1);
  pRet->zName = sqlite3Fts5Strndup(&rc, azArg[2], -1);
  pRet->bColumnsize = 1;
  pRet->eDetail = FTS5_DETAIL_FULL;
#ifdef SQLITE_DEBUG
  pRet->bPrefixIndex = 1;
#endif
  if( rc==SQLITE_OK && sqlite3_stricmp(pRet->zName, FTS5_RANK_NAME)==0 ){
    *pzErr = sqlite3_mprintf("reserved fts5 table name: %s", pRet->zName);
    rc = SQLITE_ERROR;
  }

  assert( (pRet->abUnindexed && pRet->azCol) || rc!=SQLITE_OK );
  for(i=3; rc==SQLITE_OK && i<nArg; i++){
    const char *zOrig = azArg[i];
    const char *z;
    char *zOne = 0;
    char *zTwo = 0;
    int bOption = 0;
    int bMustBeCol = 0;

    z = fts5ConfigGobbleWord(&rc, zOrig, &zOne, &bMustBeCol);
    z = fts5ConfigSkipWhitespace(z);
    if( z && *z=='=' ){
      bOption = 1;
      assert( zOne!=0 );
      z++;
      if( bMustBeCol ) z = 0;
    }
    z = fts5ConfigSkipWhitespace(z);
    if( z && z[0] ){
      int bDummy;
      z = fts5ConfigGobbleWord(&rc, z, &zTwo, &bDummy);
      if( z && z[0] ) z = 0;
    }

    if( rc==SQLITE_OK ){
      if( z==0 ){
        *pzErr = sqlite3_mprintf("parse error in \"%s\"", zOrig);
        rc = SQLITE_ERROR;
      }else{
        if( bOption ){
          rc = fts5ConfigParseSpecial(pGlobal, pRet,
            ALWAYS(zOne)?zOne:"",
            zTwo?zTwo:"",
            pzErr
          );
        }else{
          rc = fts5ConfigParseColumn(pRet, zOne, zTwo, pzErr);
          zOne = 0;
        }
      }
    }

    sqlite3_free(zOne);
    sqlite3_free(zTwo);
  }

  /* We only allow contentless_delete=1 if the table is indeed contentless. */
  if( rc==SQLITE_OK
   && pRet->bContentlessDelete
   && pRet->eContent!=FTS5_CONTENT_NONE
  ){
    *pzErr = sqlite3_mprintf(
        "contentless_delete=1 requires a contentless table"
    );
    rc = SQLITE_ERROR;
  }

  /* We only allow contentless_delete=1 if columnsize=0 is not present.
  **
  ** This restriction may be removed at some point.
  */
  if( rc==SQLITE_OK && pRet->bContentlessDelete && pRet->bColumnsize==0 ){
    *pzErr = sqlite3_mprintf(
        "contentless_delete=1 is incompatible with columnsize=0"
    );
    rc = SQLITE_ERROR;
  }

  /* If a tokenizer= option was successfully parsed, the tokenizer has
  ** already been allocated. Otherwise, allocate an instance of the default
  ** tokenizer (unicode61) now.  */
  if( rc==SQLITE_OK && pRet->pTok==0 ){
    rc = fts5ConfigDefaultTokenizer(pGlobal, pRet);
  }

  /* If no zContent option was specified, fill in the default values. */
  if( rc==SQLITE_OK && pRet->zContent==0 ){
    const char *zTail = 0;
    assert( pRet->eContent==FTS5_CONTENT_NORMAL
         || pRet->eContent==FTS5_CONTENT_NONE
    );
    if( pRet->eContent==FTS5_CONTENT_NORMAL ){
      zTail = "content";
    }else if( pRet->bColumnsize ){
      zTail = "docsize";
    }

    if( zTail ){
      pRet->zContent = sqlite3Fts5Mprintf(
          &rc, "%Q.'%q_%s'", pRet->zDb, pRet->zName, zTail
      );
    }
  }

  if( rc==SQLITE_OK && pRet->zContentRowid==0 ){
    pRet->zContentRowid = sqlite3Fts5Strndup(&rc, "rowid", -1);
  }

  /* Formulate the zContentExprlist text */
  if( rc==SQLITE_OK ){
    rc = fts5ConfigMakeExprlist(pRet);
  }

  if( rc!=SQLITE_OK ){
    sqlite3Fts5ConfigFree(pRet);
    *ppOut = 0;
  }
  return rc;
}

/*
** Free the configuration object passed as the only argument.
*/
static void sqlite3Fts5ConfigFree(Fts5Config *pConfig){
  if( pConfig ){
    int i;
    if( pConfig->pTok ){
      pConfig->pTokApi->xDelete(pConfig->pTok);
    }
    sqlite3_free(pConfig->zDb);
    sqlite3_free(pConfig->zName);
    for(i=0; i<pConfig->nCol; i++){
      sqlite3_free(pConfig->azCol[i]);
    }
    sqlite3_free(pConfig->azCol);
    sqlite3_free(pConfig->aPrefix);
    sqlite3_free(pConfig->zRank);
    sqlite3_free(pConfig->zRankArgs);
    sqlite3_free(pConfig->zContent);
    sqlite3_free(pConfig->zContentRowid);
    sqlite3_free(pConfig->zContentExprlist);
    sqlite3_free(pConfig);
  }
}

/*
** Call sqlite3_declare_vtab() based on the contents of the configuration
** object passed as the only argument. Return SQLITE_OK if successful, or
** an SQLite error code if an error occurs.
*/
static int sqlite3Fts5ConfigDeclareVtab(Fts5Config *pConfig){
  int i;
  int rc = SQLITE_OK;
  char *zSql;

  zSql = sqlite3Fts5Mprintf(&rc, "CREATE TABLE x(");
  for(i=0; zSql && i<pConfig->nCol; i++){
    const char *zSep = (i==0?"":", ");
    zSql = sqlite3Fts5Mprintf(&rc, "%z%s%Q", zSql, zSep, pConfig->azCol[i]);
  }
  zSql = sqlite3Fts5Mprintf(&rc, "%z, %Q HIDDEN, %s HIDDEN)",
      zSql, pConfig->zName, FTS5_RANK_NAME
  );

  assert( zSql || rc==SQLITE_NOMEM );
  if( zSql ){
    rc = sqlite3_declare_vtab(pConfig->db, zSql);
    sqlite3_free(zSql);
  }

  return rc;
}

/*
** Tokenize the text passed via the second and third arguments.
**
** The callback is invoked once for each token in the input text. The
** arguments passed to it are, in order:
**
**     void *pCtx          // Copy of 4th argument to sqlite3Fts5Tokenize()
**     const char *pToken  // Pointer to buffer containing token
**     int nToken          // Size of token in bytes
**     int iStart          // Byte offset of start of token within input text
**     int iEnd            // Byte offset of end of token within input text
**     int iPos            // Position of token in input (first token is 0)
**
** If the callback returns a non-zero value the tokenization is abandoned
** and no further callbacks are issued.
**
** This function returns SQLITE_OK if successful or an SQLite error code
** if an error occurs. If the tokenization was abandoned early because
** the callback returned SQLITE_DONE, this is not an error and this function
** still returns SQLITE_OK. Or, if the tokenization was abandoned early
** because the callback returned another non-zero value, it is assumed
** to be an SQLite error code and returned to the caller.
*/
static int sqlite3Fts5Tokenize(
  Fts5Config *pConfig,            /* FTS5 Configuration object */
  int flags,                      /* FTS5_TOKENIZE_* flags */
  const char *pText, int nText,   /* Text to tokenize */
  void *pCtx,                     /* Context passed to xToken() */
  int (*xToken)(void*, int, const char*, int, int, int)    /* Callback */
){
  if( pText==0 ) return SQLITE_OK;
  return pConfig->pTokApi->xTokenize(
      pConfig->pTok, pCtx, flags, pText, nText, xToken
  );
}

/*
** Argument pIn points to the first character in what is expected to be
** a comma-separated list of SQL literals followed by a ')' character.
** If it actually is this, return a pointer to the ')'. Otherwise, return
** NULL to indicate a parse error.
*/
static const char *fts5ConfigSkipArgs(const char *pIn){
  const char *p = pIn;

  while( 1 ){
    p = fts5ConfigSkipWhitespace(p);
    p = fts5ConfigSkipLiteral(p);
    p = fts5ConfigSkipWhitespace(p);
    if( p==0 || *p==')' ) break;
    if( *p!=',' ){
      p = 0;
      break;
    }
    p++;
  }

  return p;
}

/*
** Parameter zIn contains a rank() function specification. The format of
** this is:
**
**   + Bareword (function name)
**   + Open parenthesis - "("
**   + Zero or more SQL literals in a comma separated list
**   + Close parenthesis - ")"
*/
static int sqlite3Fts5ConfigParseRank(
  const char *zIn,                /* Input string */
  char **pzRank,                  /* OUT: Rank function name */
  char **pzRankArgs               /* OUT: Rank function arguments */
){
  const char *p = zIn;
  const char *pRank;
  char *zRank = 0;
  char *zRankArgs = 0;
  int rc = SQLITE_OK;

  *pzRank = 0;
  *pzRankArgs = 0;

  if( p==0 ){
    rc = SQLITE_ERROR;
  }else{
    p = fts5ConfigSkipWhitespace(p);
    pRank = p;
    p = fts5ConfigSkipBareword(p);

    if( p ){
      zRank = sqlite3Fts5MallocZero(&rc, 1 + p - pRank);
      if( zRank ) memcpy(zRank, pRank, p-pRank);
    }else{
      rc = SQLITE_ERROR;
    }

    if( rc==SQLITE_OK ){
      p = fts5ConfigSkipWhitespace(p);
      if( *p!='(' ) rc = SQLITE_ERROR;
      p++;
    }
    if( rc==SQLITE_OK ){
      const char *pArgs;
      p = fts5ConfigSkipWhitespace(p);
      pArgs = p;
      if( *p!=')' ){
        p = fts5ConfigSkipArgs(p);
        if( p==0 ){
          rc = SQLITE_ERROR;
        }else{
          zRankArgs = sqlite3Fts5MallocZero(&rc, 1 + p - pArgs);
          if( zRankArgs ) memcpy(zRankArgs, pArgs, p-pArgs);
        }
      }
    }
  }

  if( rc!=SQLITE_OK ){
    sqlite3_free(zRank);
    assert( zRankArgs==0 );
  }else{
    *pzRank = zRank;
    *pzRankArgs = zRankArgs;
  }
  return rc;
}
