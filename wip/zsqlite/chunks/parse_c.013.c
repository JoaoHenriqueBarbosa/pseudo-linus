
/*
** Return the fallback token corresponding to canonical token iToken, or
** 0 if iToken has no fallback.
*/
SQLITE_PRIVATE int sqlite3ParserFallback(int iToken){
#ifdef YYFALLBACK
  assert( iToken<(int)(sizeof(yyFallback)/sizeof(yyFallback[0])) );
  return yyFallback[iToken];
#else
  (void)iToken;
  return 0;
#endif
}

