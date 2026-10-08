
/*
** The first character of the string pointed to by argument z is guaranteed
** to be an open-quote character (see function fts5_isopenquote()).
**
** This function searches for the corresponding close-quote character within
** the string and, if found, dequotes the string in place and adds a new
** nul-terminator byte.
**
** If the close-quote is found, the value returned is the byte offset of
** the character immediately following it. Or, if the close-quote is not
** found, -1 is returned. If -1 is returned, the buffer is left in an
** undefined state.
*/
static int fts5Dequote(char *z){
  char q;
  int iIn = 1;
  int iOut = 0;
  q = z[0];

  /* Set stack variable q to the close-quote character */
  assert( q=='[' || q=='\'' || q=='"' || q=='`' );
  if( q=='[' ) q = ']';

  while( z[iIn] ){
    if( z[iIn]==q ){
      if( z[iIn+1]!=q ){
        /* Character iIn was the close quote. */
        iIn++;
        break;
      }else{
        /* Character iIn and iIn+1 form an escaped quote character. Skip
        ** the input cursor past both and copy a single quote character
        ** to the output buffer. */
        iIn += 2;
        z[iOut++] = q;
      }
    }else{
      z[iOut++] = z[iIn++];
    }
  }

  z[iOut] = '\0';
  return iIn;
}

/*
** Convert an SQL-style quoted string into a normal string by removing
** the quote characters.  The conversion is done in-place.  If the
** input does not begin with a quote character, then this routine
** is a no-op.
**
** Examples:
**
**     "abc"   becomes   abc
**     'xyz'   becomes   xyz
**     [pqr]   becomes   pqr
**     `mno`   becomes   mno
*/
static void sqlite3Fts5Dequote(char *z){
  char quote;                     /* Quote character (if any ) */

  assert( 0==fts5_iswhitespace(z[0]) );
  quote = z[0];
  if( quote=='[' || quote=='\'' || quote=='"' || quote=='`' ){
    fts5Dequote(z);
  }
}


struct Fts5Enum {
  const char *zName;
  int eVal;
};
typedef struct Fts5Enum Fts5Enum;

static int fts5ConfigSetEnum(
  const Fts5Enum *aEnum,
  const char *zEnum,
  int *peVal
){
  int nEnum = (int)strlen(zEnum);
  int i;
  int iVal = -1;

  for(i=0; aEnum[i].zName; i++){
    if( sqlite3_strnicmp(aEnum[i].zName, zEnum, nEnum)==0 ){
      if( iVal>=0 ) return SQLITE_ERROR;
      iVal = aEnum[i].eVal;
    }
  }

  *peVal = iVal;
  return iVal<0 ? SQLITE_ERROR : SQLITE_OK;
}

/*
** Parse a "special" CREATE VIRTUAL TABLE directive and update
** configuration object pConfig as appropriate.
**
** If successful, object pConfig is updated and SQLITE_OK returned. If
** an error occurs, an SQLite error code is returned and an error message
** may be left in *pzErr. It is the responsibility of the caller to
** eventually free any such error message using sqlite3_free().
*/
static int fts5ConfigParseSpecial(
  Fts5Global *pGlobal,
  Fts5Config *pConfig,            /* Configuration object to update */
  const char *zCmd,               /* Special command to parse */
  const char *zArg,               /* Argument to parse */
  char **pzErr                    /* OUT: Error message */
){
  int rc = SQLITE_OK;
  int nCmd = (int)strlen(zCmd);
  if( sqlite3_strnicmp("prefix", zCmd, nCmd)==0 ){
    const int nByte = sizeof(int) * FTS5_MAX_PREFIX_INDEXES;
    const char *p;
    int bFirst = 1;
    if( pConfig->aPrefix==0 ){
      pConfig->aPrefix = sqlite3Fts5MallocZero(&rc, nByte);
      if( rc ) return rc;
    }

    p = zArg;
    while( 1 ){
      int nPre = 0;

      while( p[0]==' ' ) p++;
      if( bFirst==0 && p[0]==',' ){
        p++;
        while( p[0]==' ' ) p++;
      }else if( p[0]=='\0' ){
        break;
      }
      if( p[0]<'0' || p[0]>'9' ){
        *pzErr = sqlite3_mprintf("malformed prefix=... directive");
        rc = SQLITE_ERROR;
        break;
      }

      if( pConfig->nPrefix==FTS5_MAX_PREFIX_INDEXES ){
        *pzErr = sqlite3_mprintf(
            "too many prefix indexes (max %d)", FTS5_MAX_PREFIX_INDEXES
        );
        rc = SQLITE_ERROR;
        break;
      }

      while( p[0]>='0' && p[0]<='9' && nPre<1000 ){
        nPre = nPre*10 + (p[0] - '0');
        p++;
      }

      if( nPre<=0 || nPre>=1000 ){
        *pzErr = sqlite3_mprintf("prefix length out of range (max 999)");
        rc = SQLITE_ERROR;
        break;
      }

      pConfig->aPrefix[pConfig->nPrefix] = nPre;
      pConfig->nPrefix++;
      bFirst = 0;
    }
    assert( pConfig->nPrefix<=FTS5_MAX_PREFIX_INDEXES );
    return rc;
  }

  if( sqlite3_strnicmp("tokenize", zCmd, nCmd)==0 ){
    const char *p = (const char*)zArg;
    sqlite3_int64 nArg = strlen(zArg) + 1;
    char **azArg = sqlite3Fts5MallocZero(&rc, sizeof(char*) * nArg);
    char *pDel = sqlite3Fts5MallocZero(&rc, nArg * 2);
    char *pSpace = pDel;

    if( azArg && pSpace ){
      if( pConfig->pTok ){
        *pzErr = sqlite3_mprintf("multiple tokenize=... directives");
        rc = SQLITE_ERROR;
      }else{
        for(nArg=0; p && *p; nArg++){
          const char *p2 = fts5ConfigSkipWhitespace(p);
          if( *p2=='\'' ){
            p = fts5ConfigSkipLiteral(p2);
          }else{
            p = fts5ConfigSkipBareword(p2);
          }
          if( p ){
            memcpy(pSpace, p2, p-p2);
            azArg[nArg] = pSpace;
            sqlite3Fts5Dequote(pSpace);
            pSpace += (p - p2) + 1;
            p = fts5ConfigSkipWhitespace(p);
          }
        }
        if( p==0 ){
          *pzErr = sqlite3_mprintf("parse error in tokenize directive");
          rc = SQLITE_ERROR;
        }else{
          rc = sqlite3Fts5GetTokenizer(pGlobal,
              (const char**)azArg, (int)nArg, pConfig,
              pzErr
          );
        }
      }
    }

    sqlite3_free(azArg);
    sqlite3_free(pDel);
    return rc;
  }

  if( sqlite3_strnicmp("content", zCmd, nCmd)==0 ){
    if( pConfig->eContent!=FTS5_CONTENT_NORMAL ){
      *pzErr = sqlite3_mprintf("multiple content=... directives");
      rc = SQLITE_ERROR;
    }else{
      if( zArg[0] ){
        pConfig->eContent = FTS5_CONTENT_EXTERNAL;
        pConfig->zContent = sqlite3Fts5Mprintf(&rc, "%Q.%Q", pConfig->zDb,zArg);
      }else{
        pConfig->eContent = FTS5_CONTENT_NONE;
      }
    }
    return rc;
  }

  if( sqlite3_strnicmp("contentless_delete", zCmd, nCmd)==0 ){
    if( (zArg[0]!='0' && zArg[0]!='1') || zArg[1]!='\0' ){
      *pzErr = sqlite3_mprintf("malformed contentless_delete=... directive");
      rc = SQLITE_ERROR;
    }else{
      pConfig->bContentlessDelete = (zArg[0]=='1');
    }
    return rc;
  }

  if( sqlite3_strnicmp("content_rowid", zCmd, nCmd)==0 ){
    if( pConfig->zContentRowid ){
      *pzErr = sqlite3_mprintf("multiple content_rowid=... directives");
      rc = SQLITE_ERROR;
    }else{
      pConfig->zContentRowid = sqlite3Fts5Strndup(&rc, zArg, -1);
    }
    return rc;
  }

  if( sqlite3_strnicmp("columnsize", zCmd, nCmd)==0 ){
    if( (zArg[0]!='0' && zArg[0]!='1') || zArg[1]!='\0' ){
      *pzErr = sqlite3_mprintf("malformed columnsize=... directive");
      rc = SQLITE_ERROR;
    }else{
      pConfig->bColumnsize = (zArg[0]=='1');
    }
    return rc;
  }

  if( sqlite3_strnicmp("detail", zCmd, nCmd)==0 ){
    const Fts5Enum aDetail[] = {
      { "none", FTS5_DETAIL_NONE },
      { "full", FTS5_DETAIL_FULL },
      { "columns", FTS5_DETAIL_COLUMNS },
      { 0, 0 }
    };

    if( (rc = fts5ConfigSetEnum(aDetail, zArg, &pConfig->eDetail)) ){
      *pzErr = sqlite3_mprintf("malformed detail=... directive");
    }
    return rc;
  }

  if( sqlite3_strnicmp("tokendata", zCmd, nCmd)==0 ){
    if( (zArg[0]!='0' && zArg[0]!='1') || zArg[1]!='\0' ){
      *pzErr = sqlite3_mprintf("malformed tokendata=... directive");
      rc = SQLITE_ERROR;
    }else{
      pConfig->bTokendata = (zArg[0]=='1');
    }
    return rc;
  }

  *pzErr = sqlite3_mprintf("unrecognized option: \"%.*s\"", nCmd, zCmd);
  return SQLITE_ERROR;
}

/*
** Allocate an instance of the default tokenizer ("simple") at
** Fts5Config.pTokenizer. Return SQLITE_OK if successful, or an SQLite error
** code if an error occurs.
*/
static int fts5ConfigDefaultTokenizer(Fts5Global *pGlobal, Fts5Config *pConfig){
  assert( pConfig->pTok==0 && pConfig->pTokApi==0 );
  return sqlite3Fts5GetTokenizer(pGlobal, 0, 0, pConfig, 0);
}

/*
** Gobble up the first bareword or quoted word from the input buffer zIn.
** Return a pointer to the character immediately following the last in
** the gobbled word if successful, or a NULL pointer otherwise (failed
** to find close-quote character).
**
** Before returning, set pzOut to point to a new buffer containing a
** nul-terminated, dequoted copy of the gobbled word. If the word was
** quoted, *pbQuoted is also set to 1 before returning.
**
** If *pRc is other than SQLITE_OK when this function is called, it is
** a no-op (NULL is returned). Otherwise, if an OOM occurs within this
** function, *pRc is set to SQLITE_NOMEM before returning. *pRc is *not*
** set if a parse error (failed to find close quote) occurs.
*/
static const char *fts5ConfigGobbleWord(
  int *pRc,                       /* IN/OUT: Error code */
  const char *zIn,                /* Buffer to gobble string/bareword from */
  char **pzOut,                   /* OUT: malloc'd buffer containing str/bw */
  int *pbQuoted                   /* OUT: Set to true if dequoting required */
){
  const char *zRet = 0;

  sqlite3_int64 nIn = strlen(zIn);
  char *zOut = sqlite3_malloc64(nIn+1);

  assert( *pRc==SQLITE_OK );
  *pbQuoted = 0;
  *pzOut = 0;

  if( zOut==0 ){
    *pRc = SQLITE_NOMEM;
  }else{
    memcpy(zOut, zIn, (size_t)(nIn+1));
    if( fts5_isopenquote(zOut[0]) ){
      int ii = fts5Dequote(zOut);
      zRet = &zIn[ii];
      *pbQuoted = 1;
    }else{
      zRet = fts5ConfigSkipBareword(zIn);
      if( zRet ){
        zOut[zRet-zIn] = '\0';
      }
    }
  }

  if( zRet==0 ){
    sqlite3_free(zOut);
  }else{
    *pzOut = zOut;
  }

  return zRet;
}

static int fts5ConfigParseColumn(
  Fts5Config *p,
  char *zCol,
  char *zArg,
  char **pzErr
){
  int rc = SQLITE_OK;
  if( 0==sqlite3_stricmp(zCol, FTS5_RANK_NAME)
   || 0==sqlite3_stricmp(zCol, FTS5_ROWID_NAME)
  ){
    *pzErr = sqlite3_mprintf("reserved fts5 column name: %s", zCol);
    rc = SQLITE_ERROR;
  }else if( zArg ){
    if( 0==sqlite3_stricmp(zArg, "unindexed") ){
      p->abUnindexed[p->nCol] = 1;
    }else{
      *pzErr = sqlite3_mprintf("unrecognized column option: %s", zArg);
      rc = SQLITE_ERROR;
    }
  }

  p->azCol[p->nCol++] = zCol;
  return rc;
}
