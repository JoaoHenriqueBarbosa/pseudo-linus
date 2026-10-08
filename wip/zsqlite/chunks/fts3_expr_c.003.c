
/*
** This is the implementation of a scalar SQL function used to test the
** expression parser. It should be called as follows:
**
**   fts3_exprtest(<tokenizer>, <expr>, <column 1>, ...);
**
** The first argument, <tokenizer>, is the name of the fts3 tokenizer used
** to parse the query expression (see README.tokenizers). The second argument
** is the query expression to parse. Each subsequent argument is the name
** of a column of the fts3 table that the query expression may refer to.
** For example:
**
**   SELECT fts3_exprtest('simple', 'Bill col2:Bloggs', 'col1', 'col2');
*/
static void fts3ExprTestCommon(
  int bRebalance,
  sqlite3_context *context,
  int argc,
  sqlite3_value **argv
){
  sqlite3_tokenizer *pTokenizer = 0;
  int rc;
  char **azCol = 0;
  const char *zExpr;
  int nExpr;
  int nCol;
  int ii;
  Fts3Expr *pExpr;
  char *zBuf = 0;
  Fts3Hash *pHash = (Fts3Hash*)sqlite3_user_data(context);
  const char *zTokenizer = 0;
  char *zErr = 0;

  if( argc<3 ){
    sqlite3_result_error(context,
        "Usage: fts3_exprtest(tokenizer, expr, col1, ...", -1
    );
    return;
  }

  zTokenizer = (const char*)sqlite3_value_text(argv[0]);
  rc = sqlite3Fts3InitTokenizer(pHash, zTokenizer, &pTokenizer, &zErr);
  if( rc!=SQLITE_OK ){
    if( rc==SQLITE_NOMEM ){
      sqlite3_result_error_nomem(context);
    }else{
      sqlite3_result_error(context, zErr, -1);
    }
    sqlite3_free(zErr);
    return;
  }

  zExpr = (const char *)sqlite3_value_text(argv[1]);
  nExpr = sqlite3_value_bytes(argv[1]);
  nCol = argc-2;
  azCol = (char **)sqlite3_malloc64(nCol*sizeof(char *));
  if( !azCol ){
    sqlite3_result_error_nomem(context);
    goto exprtest_out;
  }
  for(ii=0; ii<nCol; ii++){
    azCol[ii] = (char *)sqlite3_value_text(argv[ii+2]);
  }

  if( bRebalance ){
    char *zDummy = 0;
    rc = sqlite3Fts3ExprParse(
        pTokenizer, 0, azCol, 0, nCol, nCol, zExpr, nExpr, &pExpr, &zDummy
    );
    assert( rc==SQLITE_OK || pExpr==0 );
    sqlite3_free(zDummy);
  }else{
    rc = fts3ExprParseUnbalanced(
        pTokenizer, 0, azCol, 0, nCol, nCol, zExpr, nExpr, &pExpr
    );
  }

  if( rc!=SQLITE_OK && rc!=SQLITE_NOMEM ){
    sqlite3Fts3ExprFree(pExpr);
    sqlite3_result_error(context, "Error parsing expression", -1);
  }else if( rc==SQLITE_NOMEM || !(zBuf = exprToString(pExpr, 0)) ){
    sqlite3_result_error_nomem(context);
  }else{
    sqlite3_result_text(context, zBuf, -1, SQLITE_TRANSIENT);
    sqlite3_free(zBuf);
  }

  sqlite3Fts3ExprFree(pExpr);

exprtest_out:
  if( pTokenizer ){
    rc = pTokenizer->pModule->xDestroy(pTokenizer);
  }
  sqlite3_free(azCol);
}

static void fts3ExprTest(
  sqlite3_context *context,
  int argc,
  sqlite3_value **argv
){
  fts3ExprTestCommon(0, context, argc, argv);
}
static void fts3ExprTestRebalance(
  sqlite3_context *context,
  int argc,
  sqlite3_value **argv
){
  fts3ExprTestCommon(1, context, argc, argv);
}

/*
** Register the query expression parser test function fts3_exprtest()
** with database connection db.
*/
SQLITE_PRIVATE int sqlite3Fts3ExprInitTestInterface(sqlite3 *db, Fts3Hash *pHash){
  int rc = sqlite3_create_function(
      db, "fts3_exprtest", -1, SQLITE_UTF8, (void*)pHash, fts3ExprTest, 0, 0
  );
  if( rc==SQLITE_OK ){
    rc = sqlite3_create_function(db, "fts3_exprtest_rebalance",
        -1, SQLITE_UTF8, (void*)pHash, fts3ExprTestRebalance, 0, 0
    );
  }
  return rc;
}

#endif
#endif /* !defined(SQLITE_CORE) || defined(SQLITE_ENABLE_FTS3) */

