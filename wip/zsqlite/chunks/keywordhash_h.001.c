SQLITE_PRIVATE int sqlite3KeywordCode(const unsigned char *z, int n){
  int id = TK_ID;
  if( n>=2 ) keywordCode((char*)z, n, &id);
  return id;
}
#define SQLITE_N_KEYWORD 147
SQLITE_API int sqlite3_keyword_name(int i,const char **pzName,int *pnName){
  if( i<0 || i>=SQLITE_N_KEYWORD ) return SQLITE_ERROR;
  i++;
  *pzName = zKWText + aKWOffset[i];
  *pnName = aKWLen[i];
  return SQLITE_OK;
}
SQLITE_API int sqlite3_keyword_count(void){ return SQLITE_N_KEYWORD; }
SQLITE_API int sqlite3_keyword_check(const char *zName, int nName){
  return TK_ID!=sqlite3KeywordCode((const u8*)zName, nName);
}

