#endif /* SQLITE_ENABLE_SNAPSHOT */

#ifndef SQLITE_OMIT_COMPILEOPTION_DIAGS
/*
** Given the name of a compile-time option, return true if that option
** was used and false if not.
**
** The name can optionally begin with "SQLITE_" but the "SQLITE_" prefix
** is not required for a match.
*/
SQLITE_API int sqlite3_compileoption_used(const char *zOptName){
  int i, n;
  int nOpt;
  const char **azCompileOpt;

#ifdef SQLITE_ENABLE_API_ARMOR
  if( zOptName==0 ){
    (void)SQLITE_MISUSE_BKPT;
    return 0;
  }
#endif

  azCompileOpt = sqlite3CompileOptions(&nOpt);

  if( sqlite3StrNICmp(zOptName, "SQLITE_", 7)==0 ) zOptName += 7;
  n = sqlite3Strlen30(zOptName);

  /* Since nOpt is normally in single digits, a linear search is
  ** adequate. No need for a binary search. */
  for(i=0; i<nOpt; i++){
    if( sqlite3StrNICmp(zOptName, azCompileOpt[i], n)==0
     && sqlite3IsIdChar((unsigned char)azCompileOpt[i][n])==0
    ){
      return 1;
    }
  }
  return 0;
}

/*
** Return the N-th compile-time option string.  If N is out of range,
** return a NULL pointer.
*/
SQLITE_API const char *sqlite3_compileoption_get(int N){
  int nOpt;
  const char **azCompileOpt;
  azCompileOpt = sqlite3CompileOptions(&nOpt);
  if( N>=0 && N<nOpt ){
    return azCompileOpt[N];
  }
  return 0;
}
#endif /* SQLITE_OMIT_COMPILEOPTION_DIAGS */

