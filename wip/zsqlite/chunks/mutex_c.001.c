SQLITE_API int sqlite3_mutex_notheld(sqlite3_mutex *p){
  assert( p==0 || sqlite3GlobalConfig.mutex.xMutexNotheld );
  return p==0 || sqlite3GlobalConfig.mutex.xMutexNotheld(p);
}
#endif

#endif /* !defined(SQLITE_MUTEX_OMIT) */

