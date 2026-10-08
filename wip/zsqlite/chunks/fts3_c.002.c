
/*
** Store the current database page-size in bytes in p->nPgsz.
**
** If *pRc is non-zero when this function is called, it is a no-op.
** Otherwise, if an error occurs, an SQLite error code is stored in *pRc
** before returning.
*/
static void fts3DatabasePageSize(int *pRc, Fts3Table *p){
  if( *pRc==SQLITE_OK ){
    int rc;                       /* Return code */
    char *zSql;                   /* SQL text "PRAGMA %Q.page_size" */
    sqlite3_stmt *pStmt;          /* Compiled "PRAGMA %Q.page_size" statement */

    zSql = sqlite3_mprintf("PRAGMA %Q.page_size", p->zDb);
    if( !zSql ){
      rc = SQLITE_NOMEM;
    }else{
      rc = sqlite3_prepare(p->db, zSql, -1, &pStmt, 0);
      if( rc==SQLITE_OK ){
        sqlite3_step(pStmt);
        p->nPgsz = sqlite3_column_int(pStmt, 0);
        rc = sqlite3_finalize(pStmt);
      }else if( rc==SQLITE_AUTH ){
        p->nPgsz = 1024;
        rc = SQLITE_OK;
      }
    }
    assert( p->nPgsz>0 || rc!=SQLITE_OK );
    sqlite3_free(zSql);
    *pRc = rc;
  }
}

/*
** "Special" FTS4 arguments are column specifications of the following form:
**
**   <key> = <value>
**
** There may not be whitespace surrounding the "=" character. The <value>
** term may be quoted, but the <key> may not.
*/
static int fts3IsSpecialColumn(
  const char *z,
  int *pnKey,
  char **pzValue
){
  char *zValue;
  const char *zCsr = z;

  while( *zCsr!='=' ){
    if( *zCsr=='\0' ) return 0;
    zCsr++;
  }

  *pnKey = (int)(zCsr-z);
  zValue = sqlite3_mprintf("%s", &zCsr[1]);
  if( zValue ){
    sqlite3Fts3Dequote(zValue);
  }
  *pzValue = zValue;
  return 1;
}

/*
** Append the output of a printf() style formatting to an existing string.
*/
static void fts3Appendf(
  int *pRc,                       /* IN/OUT: Error code */
  char **pz,                      /* IN/OUT: Pointer to string buffer */
  const char *zFormat,            /* Printf format string to append */
  ...                             /* Arguments for printf format string */
){
  if( *pRc==SQLITE_OK ){
    va_list ap;
    char *z;
    va_start(ap, zFormat);
    z = sqlite3_vmprintf(zFormat, ap);
    va_end(ap);
    if( z && *pz ){
      char *z2 = sqlite3_mprintf("%s%s", *pz, z);
      sqlite3_free(z);
      z = z2;
    }
    if( z==0 ) *pRc = SQLITE_NOMEM;
    sqlite3_free(*pz);
    *pz = z;
  }
}

/*
** Return a copy of input string zInput enclosed in double-quotes (") and
** with all double quote characters escaped. For example:
**
**     fts3QuoteId("un \"zip\"")   ->    "un \"\"zip\"\""
**
** The pointer returned points to memory obtained from sqlite3_malloc(). It
** is the callers responsibility to call sqlite3_free() to release this
** memory.
*/
static char *fts3QuoteId(char const *zInput){
  sqlite3_int64 nRet;
  char *zRet;
  nRet = 2 + (int)strlen(zInput)*2 + 1;
  zRet = sqlite3_malloc64(nRet);
  if( zRet ){
    int i;
    char *z = zRet;
    *(z++) = '"';
    for(i=0; zInput[i]; i++){
      if( zInput[i]=='"' ) *(z++) = '"';
      *(z++) = zInput[i];
    }
    *(z++) = '"';
    *(z++) = '\0';
  }
  return zRet;
}

/*
** Return a list of comma separated SQL expressions and a FROM clause that
** could be used in a SELECT statement such as the following:
**
**     SELECT <list of expressions> FROM %_content AS x ...
**
** to return the docid, followed by each column of text data in order
** from left to write. If parameter zFunc is not NULL, then instead of
** being returned directly each column of text data is passed to an SQL
** function named zFunc first. For example, if zFunc is "unzip" and the
** table has the three user-defined columns "a", "b", and "c", the following
** string is returned:
**
**     "docid, unzip(x.'a'), unzip(x.'b'), unzip(x.'c') FROM %_content AS x"
**
** The pointer returned points to a buffer allocated by sqlite3_malloc(). It
** is the responsibility of the caller to eventually free it.
**
** If *pRc is not SQLITE_OK when this function is called, it is a no-op (and
** a NULL pointer is returned). Otherwise, if an OOM error is encountered
** by this function, NULL is returned and *pRc is set to SQLITE_NOMEM. If
** no error occurs, *pRc is left unmodified.
*/
static char *fts3ReadExprList(Fts3Table *p, const char *zFunc, int *pRc){
  char *zRet = 0;
  char *zFree = 0;
  char *zFunction;
  int i;

  if( p->zContentTbl==0 ){
    if( !zFunc ){
      zFunction = "";
    }else{
      zFree = zFunction = fts3QuoteId(zFunc);
    }
    fts3Appendf(pRc, &zRet, "docid");
    for(i=0; i<p->nColumn; i++){
      fts3Appendf(pRc, &zRet, ",%s(x.'c%d%q')", zFunction, i, p->azColumn[i]);
    }
    if( p->zLanguageid ){
      fts3Appendf(pRc, &zRet, ", x.%Q", "langid");
    }
    sqlite3_free(zFree);
  }else{
    fts3Appendf(pRc, &zRet, "rowid");
    for(i=0; i<p->nColumn; i++){
      fts3Appendf(pRc, &zRet, ", x.'%q'", p->azColumn[i]);
    }
    if( p->zLanguageid ){
      fts3Appendf(pRc, &zRet, ", x.%Q", p->zLanguageid);
    }
  }
  fts3Appendf(pRc, &zRet, " FROM '%q'.'%q%s' AS x",
      p->zDb,
      (p->zContentTbl ? p->zContentTbl : p->zName),
      (p->zContentTbl ? "" : "_content")
  );
  return zRet;
}

/*
** Return a list of N comma separated question marks, where N is the number
** of columns in the %_content table (one for the docid plus one for each
** user-defined text column).
**
** If argument zFunc is not NULL, then all but the first question mark
** is preceded by zFunc and an open bracket, and followed by a closed
** bracket. For example, if zFunc is "zip" and the FTS3 table has three
** user-defined text columns, the following string is returned:
**
**     "?, zip(?), zip(?), zip(?)"
**
** The pointer returned points to a buffer allocated by sqlite3_malloc(). It
** is the responsibility of the caller to eventually free it.
**
** If *pRc is not SQLITE_OK when this function is called, it is a no-op (and
** a NULL pointer is returned). Otherwise, if an OOM error is encountered
** by this function, NULL is returned and *pRc is set to SQLITE_NOMEM. If
** no error occurs, *pRc is left unmodified.
*/
static char *fts3WriteExprList(Fts3Table *p, const char *zFunc, int *pRc){
  char *zRet = 0;
  char *zFree = 0;
  char *zFunction;
  int i;

  if( !zFunc ){
    zFunction = "";
  }else{
    zFree = zFunction = fts3QuoteId(zFunc);
  }
  fts3Appendf(pRc, &zRet, "?");
  for(i=0; i<p->nColumn; i++){
    fts3Appendf(pRc, &zRet, ",%s(?)", zFunction);
  }
  if( p->zLanguageid ){
    fts3Appendf(pRc, &zRet, ", ?");
  }
  sqlite3_free(zFree);
  return zRet;
}

/*
** Buffer z contains a positive integer value encoded as utf-8 text.
** Decode this value and store it in *pnOut, returning the number of bytes
** consumed. If an overflow error occurs return a negative value.
*/
SQLITE_PRIVATE int sqlite3Fts3ReadInt(const char *z, int *pnOut){
  u64 iVal = 0;
  int i;
  for(i=0; z[i]>='0' && z[i]<='9'; i++){
    iVal = iVal*10 + (z[i] - '0');
    if( iVal>0x7FFFFFFF ) return -1;
  }
  *pnOut = (int)iVal;
  return i;
}

/*
** This function interprets the string at (*pp) as a non-negative integer
** value. It reads the integer and sets *pnOut to the value read, then
** sets *pp to point to the byte immediately following the last byte of
** the integer value.
**
** Only decimal digits ('0'..'9') may be part of an integer value.
**
** If *pp does not being with a decimal digit SQLITE_ERROR is returned and
** the output value undefined. Otherwise SQLITE_OK is returned.
**
** This function is used when parsing the "prefix=" FTS4 parameter.
*/
static int fts3GobbleInt(const char **pp, int *pnOut){
  const int MAX_NPREFIX = 10000000;
  int nInt = 0;                   /* Output value */
  int nByte;
  nByte = sqlite3Fts3ReadInt(*pp, &nInt);
  if( nInt>MAX_NPREFIX ){
    nInt = 0;
  }
  if( nByte==0 ){
    return SQLITE_ERROR;
  }
  *pnOut = nInt;
  *pp += nByte;
  return SQLITE_OK;
}

/*
** This function is called to allocate an array of Fts3Index structures
** representing the indexes maintained by the current FTS table. FTS tables
** always maintain the main "terms" index, but may also maintain one or
** more "prefix" indexes, depending on the value of the "prefix=" parameter
** (if any) specified as part of the CREATE VIRTUAL TABLE statement.
**
** Argument zParam is passed the value of the "prefix=" option if one was
** specified, or NULL otherwise.
**
** If no error occurs, SQLITE_OK is returned and *apIndex set to point to
** the allocated array. *pnIndex is set to the number of elements in the
** array. If an error does occur, an SQLite error code is returned.
**
** Regardless of whether or not an error is returned, it is the responsibility
** of the caller to call sqlite3_free() on the output array to free it.
*/
static int fts3PrefixParameter(
  const char *zParam,             /* ABC in prefix=ABC parameter to parse */
  int *pnIndex,                   /* OUT: size of *apIndex[] array */
  struct Fts3Index **apIndex      /* OUT: Array of indexes for this table */
){
  struct Fts3Index *aIndex;       /* Allocated array */
  int nIndex = 1;                 /* Number of entries in array */

  if( zParam && zParam[0] ){
    const char *p;
    nIndex++;
    for(p=zParam; *p; p++){
      if( *p==',' ) nIndex++;
    }
  }

  aIndex = sqlite3_malloc64(sizeof(struct Fts3Index) * nIndex);
  *apIndex = aIndex;
  if( !aIndex ){
    return SQLITE_NOMEM;
  }

  memset(aIndex, 0, sizeof(struct Fts3Index) * nIndex);
  if( zParam ){
    const char *p = zParam;
    int i;
    for(i=1; i<nIndex; i++){
      int nPrefix = 0;
      if( fts3GobbleInt(&p, &nPrefix) ) return SQLITE_ERROR;
      assert( nPrefix>=0 );
      if( nPrefix==0 ){
        nIndex--;
        i--;
      }else{
        aIndex[i].nPrefix = nPrefix;
      }
      p++;
    }
  }

  *pnIndex = nIndex;
  return SQLITE_OK;
}

/*
** This function is called when initializing an FTS4 table that uses the
** content=xxx option. It determines the number of and names of the columns
** of the new FTS4 table.
**
** The third argument passed to this function is the value passed to the
** config=xxx option (i.e. "xxx"). This function queries the database for
** a table of that name. If found, the output variables are populated
** as follows:
**
**   *pnCol:   Set to the number of columns table xxx has,
**
**   *pnStr:   Set to the total amount of space required to store a copy
**             of each columns name, including the nul-terminator.
**
**   *pazCol:  Set to point to an array of *pnCol strings. Each string is
**             the name of the corresponding column in table xxx. The array
**             and its contents are allocated using a single allocation. It
**             is the responsibility of the caller to free this allocation
**             by eventually passing the *pazCol value to sqlite3_free().
**
** If the table cannot be found, an error code is returned and the output
** variables are undefined. Or, if an OOM is encountered, SQLITE_NOMEM is
** returned (and the output variables are undefined).
*/
static int fts3ContentColumns(
  sqlite3 *db,                    /* Database handle */
  const char *zDb,                /* Name of db (i.e. "main", "temp" etc.) */
  const char *zTbl,               /* Name of content table */
  const char ***pazCol,           /* OUT: Malloc'd array of column names */
  int *pnCol,                     /* OUT: Size of array *pazCol */
  int *pnStr,                     /* OUT: Bytes of string content */
  char **pzErr                    /* OUT: error message */
){
  int rc = SQLITE_OK;             /* Return code */
  char *zSql;                     /* "SELECT *" statement on zTbl */
  sqlite3_stmt *pStmt = 0;        /* Compiled version of zSql */

  zSql = sqlite3_mprintf("SELECT * FROM %Q.%Q", zDb, zTbl);
  if( !zSql ){
    rc = SQLITE_NOMEM;
  }else{
    rc = sqlite3_prepare(db, zSql, -1, &pStmt, 0);
    if( rc!=SQLITE_OK ){
      sqlite3Fts3ErrMsg(pzErr, "%s", sqlite3_errmsg(db));
    }
  }
  sqlite3_free(zSql);

  if( rc==SQLITE_OK ){
    const char **azCol;           /* Output array */
    sqlite3_int64 nStr = 0;       /* Size of all column names (incl. 0x00) */
    int nCol;                     /* Number of table columns */
    int i;                        /* Used to iterate through columns */

    /* Loop through the returned columns. Set nStr to the number of bytes of
    ** space required to store a copy of each column name, including the
    ** nul-terminator byte.  */
    nCol = sqlite3_column_count(pStmt);
    for(i=0; i<nCol; i++){
      const char *zCol = sqlite3_column_name(pStmt, i);
      nStr += strlen(zCol) + 1;
    }

    /* Allocate and populate the array to return. */
    azCol = (const char **)sqlite3_malloc64(sizeof(char *) * nCol + nStr);
    if( azCol==0 ){
      rc = SQLITE_NOMEM;
    }else{
      char *p = (char *)&azCol[nCol];
      for(i=0; i<nCol; i++){
        const char *zCol = sqlite3_column_name(pStmt, i);
        int n = (int)strlen(zCol)+1;
        memcpy(p, zCol, n);
        azCol[i] = p;
        p += n;
      }
    }
    sqlite3_finalize(pStmt);

    /* Set the output variables. */
    *pnCol = nCol;
    *pnStr = nStr;
    *pazCol = azCol;
  }

  return rc;
}
