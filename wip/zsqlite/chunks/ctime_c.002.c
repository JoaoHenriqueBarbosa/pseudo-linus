
SQLITE_PRIVATE const char **sqlite3CompileOptions(int *pnOpt){
  *pnOpt = sizeof(sqlite3azCompileOpt) / sizeof(sqlite3azCompileOpt[0]);
  return (const char**)sqlite3azCompileOpt;
}

#endif /* SQLITE_OMIT_COMPILEOPTION_DIAGS */

