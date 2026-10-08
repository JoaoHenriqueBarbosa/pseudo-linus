
/*
** Read a 64-bit variable-length integer from memory starting at p[0].
** Return the number of bytes read, or 0 on error.
** The value is stored in *v.
*/
SQLITE_PRIVATE int sqlite3Fts3GetVarint(const char *pBuf, sqlite_int64 *v){
  return sqlite3Fts3GetVarintU(pBuf, (sqlite3_uint64*)v);
}

/*
** Read a 64-bit variable-length integer from memory starting at p[0] and
** not extending past pEnd[-1].
** Return the number of bytes read, or 0 on error.
** The value is stored in *v.
*/
SQLITE_PRIVATE int sqlite3Fts3GetVarintBounded(
  const char *pBuf,
  const char *pEnd,
  sqlite_int64 *v
){
  const unsigned char *p = (const unsigned char*)pBuf;
  const unsigned char *pStart = p;
  const unsigned char *pX = (const unsigned char*)pEnd;
  u64 b = 0;
  int shift;
  for(shift=0; shift<=63; shift+=7){
    u64 c = p<pX ? *p : 0;
    p++;
    b += (c&0x7F) << shift;
    if( (c & 0x80)==0 ) break;
  }
  *v = b;
  return (int)(p - pStart);
}

/*
** Similar to sqlite3Fts3GetVarint(), except that the output is truncated to
** a non-negative 32-bit integer before it is returned.
*/
SQLITE_PRIVATE int sqlite3Fts3GetVarint32(const char *p, int *pi){
  const unsigned char *ptr = (const unsigned char*)p;
  u32 a;

#ifndef fts3GetVarint32
  GETVARINT_INIT(a, ptr, 0,  0x00,     0x80, *pi, 1);
#else
  a = (*ptr++);
  assert( a & 0x80 );
#endif

  GETVARINT_STEP(a, ptr, 7,  0x7F,     0x4000, *pi, 2);
  GETVARINT_STEP(a, ptr, 14, 0x3FFF,   0x200000, *pi, 3);
  GETVARINT_STEP(a, ptr, 21, 0x1FFFFF, 0x10000000, *pi, 4);
  a = (a & 0x0FFFFFFF );
  *pi = (int)(a | ((u32)(*ptr & 0x07) << 28));
  assert( 0==(a & 0x80000000) );
  assert( *pi>=0 );
  return 5;
}

/*
** Return the number of bytes required to encode v as a varint
*/
SQLITE_PRIVATE int sqlite3Fts3VarintLen(sqlite3_uint64 v){
  int i = 0;
  do{
    i++;
    v >>= 7;
  }while( v!=0 );
  return i;
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
**
*/
SQLITE_PRIVATE void sqlite3Fts3Dequote(char *z){
  char quote;                     /* Quote character (if any ) */

  quote = z[0];
  if( quote=='[' || quote=='\'' || quote=='"' || quote=='`' ){
    int iIn = 1;                  /* Index of next byte to read from input */
    int iOut = 0;                 /* Index of next byte to write to output */

    /* If the first byte was a '[', then the close-quote character is a ']' */
    if( quote=='[' ) quote = ']';

    while( z[iIn] ){
      if( z[iIn]==quote ){
        if( z[iIn+1]!=quote ) break;
        z[iOut++] = quote;
        iIn += 2;
      }else{
        z[iOut++] = z[iIn++];
      }
    }
    z[iOut] = '\0';
  }
}

/*
** Read a single varint from the doclist at *pp and advance *pp to point
** to the first byte past the end of the varint.  Add the value of the varint
** to *pVal.
*/
static void fts3GetDeltaVarint(char **pp, sqlite3_int64 *pVal){
  sqlite3_int64 iVal;
  *pp += sqlite3Fts3GetVarint(*pp, &iVal);
  *pVal += iVal;
}

/*
** When this function is called, *pp points to the first byte following a
** varint that is part of a doclist (or position-list, or any other list
** of varints). This function moves *pp to point to the start of that varint,
** and sets *pVal by the varint value.
**
** Argument pStart points to the first byte of the doclist that the
** varint is part of.
*/
static void fts3GetReverseVarint(
  char **pp,
  char *pStart,
  sqlite3_int64 *pVal
){
  sqlite3_int64 iVal;
  char *p;

  /* Pointer p now points at the first byte past the varint we are
  ** interested in. So, unless the doclist is corrupt, the 0x80 bit is
  ** clear on character p[-1]. */
  for(p = (*pp)-2; p>=pStart && *p&0x80; p--);
  p++;
  *pp = p;

  sqlite3Fts3GetVarint(p, &iVal);
  *pVal = iVal;
}

/*
** The xDisconnect() virtual table method.
*/
static int fts3DisconnectMethod(sqlite3_vtab *pVtab){
  Fts3Table *p = (Fts3Table *)pVtab;
  int i;

  assert( p->nPendingData==0 );
  assert( p->pSegments==0 );

  /* Free any prepared statements held */
  sqlite3_finalize(p->pSeekStmt);
  for(i=0; i<SizeofArray(p->aStmt); i++){
    sqlite3_finalize(p->aStmt[i]);
  }
  sqlite3_free(p->zSegmentsTbl);
  sqlite3_free(p->zReadExprlist);
  sqlite3_free(p->zWriteExprlist);
  sqlite3_free(p->zContentTbl);
  sqlite3_free(p->zLanguageid);

  /* Invoke the tokenizer destructor to free the tokenizer. */
  p->pTokenizer->pModule->xDestroy(p->pTokenizer);

  sqlite3_free(p);
  return SQLITE_OK;
}

/*
** Write an error message into *pzErr
*/
SQLITE_PRIVATE void sqlite3Fts3ErrMsg(char **pzErr, const char *zFormat, ...){
  va_list ap;
  sqlite3_free(*pzErr);
  va_start(ap, zFormat);
  *pzErr = sqlite3_vmprintf(zFormat, ap);
  va_end(ap);
}

/*
** Construct one or more SQL statements from the format string given
** and then evaluate those statements. The success code is written
** into *pRc.
**
** If *pRc is initially non-zero then this routine is a no-op.
*/
static void fts3DbExec(
  int *pRc,              /* Success code */
  sqlite3 *db,           /* Database in which to run SQL */
  const char *zFormat,   /* Format string for SQL */
  ...                    /* Arguments to the format string */
){
  va_list ap;
  char *zSql;
  if( *pRc ) return;
  va_start(ap, zFormat);
  zSql = sqlite3_vmprintf(zFormat, ap);
  va_end(ap);
  if( zSql==0 ){
    *pRc = SQLITE_NOMEM;
  }else{
    *pRc = sqlite3_exec(db, zSql, 0, 0, 0);
    sqlite3_free(zSql);
  }
}

/*
** The xDestroy() virtual table method.
*/
static int fts3DestroyMethod(sqlite3_vtab *pVtab){
  Fts3Table *p = (Fts3Table *)pVtab;
  int rc = SQLITE_OK;              /* Return code */
  const char *zDb = p->zDb;        /* Name of database (e.g. "main", "temp") */
  sqlite3 *db = p->db;             /* Database handle */

  /* Drop the shadow tables */
  fts3DbExec(&rc, db,
    "DROP TABLE IF EXISTS %Q.'%q_segments';"
    "DROP TABLE IF EXISTS %Q.'%q_segdir';"
    "DROP TABLE IF EXISTS %Q.'%q_docsize';"
    "DROP TABLE IF EXISTS %Q.'%q_stat';"
    "%s DROP TABLE IF EXISTS %Q.'%q_content';",
    zDb, p->zName,
    zDb, p->zName,
    zDb, p->zName,
    zDb, p->zName,
    (p->zContentTbl ? "--" : ""), zDb,p->zName
  );

  /* If everything has worked, invoke fts3DisconnectMethod() to free the
  ** memory associated with the Fts3Table structure and return SQLITE_OK.
  ** Otherwise, return an SQLite error code.
  */
  return (rc==SQLITE_OK ? fts3DisconnectMethod(pVtab) : rc);
}


/*
** Invoke sqlite3_declare_vtab() to declare the schema for the FTS3 table
** passed as the first argument. This is done as part of the xConnect()
** and xCreate() methods.
**
** If *pRc is non-zero when this function is called, it is a no-op.
** Otherwise, if an error occurs, an SQLite error code is stored in *pRc
** before returning.
*/
static void fts3DeclareVtab(int *pRc, Fts3Table *p){
  if( *pRc==SQLITE_OK ){
    int i;                        /* Iterator variable */
    int rc;                       /* Return code */
    char *zSql;                   /* SQL statement passed to declare_vtab() */
    char *zCols;                  /* List of user defined columns */
    const char *zLanguageid;

    zLanguageid = (p->zLanguageid ? p->zLanguageid : "__langid");
    sqlite3_vtab_config(p->db, SQLITE_VTAB_CONSTRAINT_SUPPORT, 1);
    sqlite3_vtab_config(p->db, SQLITE_VTAB_INNOCUOUS);

    /* Create a list of user columns for the virtual table */
    zCols = sqlite3_mprintf("%Q, ", p->azColumn[0]);
    for(i=1; zCols && i<p->nColumn; i++){
      zCols = sqlite3_mprintf("%z%Q, ", zCols, p->azColumn[i]);
    }

    /* Create the whole "CREATE TABLE" statement to pass to SQLite */
    zSql = sqlite3_mprintf(
        "CREATE TABLE x(%s %Q HIDDEN, docid HIDDEN, %Q HIDDEN)",
        zCols, p->zName, zLanguageid
    );
    if( !zCols || !zSql ){
      rc = SQLITE_NOMEM;
    }else{
      rc = sqlite3_declare_vtab(p->db, zSql);
    }

    sqlite3_free(zSql);
    sqlite3_free(zCols);
    *pRc = rc;
  }
}

/*
** Create the %_stat table if it does not already exist.
*/
SQLITE_PRIVATE void sqlite3Fts3CreateStatTable(int *pRc, Fts3Table *p){
  fts3DbExec(pRc, p->db,
      "CREATE TABLE IF NOT EXISTS %Q.'%q_stat'"
          "(id INTEGER PRIMARY KEY, value BLOB);",
      p->zDb, p->zName
  );
  if( (*pRc)==SQLITE_OK ) p->bHasStat = 1;
}

/*
** Create the backing store tables (%_content, %_segments and %_segdir)
** required by the FTS3 table passed as the only argument. This is done
** as part of the vtab xCreate() method.
**
** If the p->bHasDocsize boolean is true (indicating that this is an
** FTS4 table, not an FTS3 table) then also create the %_docsize and
** %_stat tables required by FTS4.
*/
static int fts3CreateTables(Fts3Table *p){
  int rc = SQLITE_OK;             /* Return code */
  int i;                          /* Iterator variable */
  sqlite3 *db = p->db;            /* The database connection */

  if( p->zContentTbl==0 ){
    const char *zLanguageid = p->zLanguageid;
    char *zContentCols;           /* Columns of %_content table */

    /* Create a list of user columns for the content table */
    zContentCols = sqlite3_mprintf("docid INTEGER PRIMARY KEY");
    for(i=0; zContentCols && i<p->nColumn; i++){
      char *z = p->azColumn[i];
      zContentCols = sqlite3_mprintf("%z, 'c%d%q'", zContentCols, i, z);
    }
    if( zLanguageid && zContentCols ){
      zContentCols = sqlite3_mprintf("%z, langid", zContentCols, zLanguageid);
    }
    if( zContentCols==0 ) rc = SQLITE_NOMEM;

    /* Create the content table */
    fts3DbExec(&rc, db,
       "CREATE TABLE %Q.'%q_content'(%s)",
       p->zDb, p->zName, zContentCols
    );
    sqlite3_free(zContentCols);
  }

  /* Create other tables */
  fts3DbExec(&rc, db,
      "CREATE TABLE %Q.'%q_segments'(blockid INTEGER PRIMARY KEY, block BLOB);",
      p->zDb, p->zName
  );
  fts3DbExec(&rc, db,
      "CREATE TABLE %Q.'%q_segdir'("
        "level INTEGER,"
        "idx INTEGER,"
        "start_block INTEGER,"
        "leaves_end_block INTEGER,"
        "end_block INTEGER,"
        "root BLOB,"
        "PRIMARY KEY(level, idx)"
      ");",
      p->zDb, p->zName
  );
  if( p->bHasDocsize ){
    fts3DbExec(&rc, db,
        "CREATE TABLE %Q.'%q_docsize'(docid INTEGER PRIMARY KEY, size BLOB);",
        p->zDb, p->zName
    );
  }
  assert( p->bHasStat==p->bFts4 );
  if( p->bHasStat ){
    sqlite3Fts3CreateStatTable(&rc, p);
  }
  return rc;
}
