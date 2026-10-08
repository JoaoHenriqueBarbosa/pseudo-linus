
/*
** Return a static string containing the name corresponding to the error code
** specified in the argument.
*/
#if defined(SQLITE_NEED_ERR_NAME)
SQLITE_PRIVATE const char *sqlite3ErrName(int rc){
  const char *zName = 0;
  int i, origRc = rc;
  for(i=0; i<2 && zName==0; i++, rc &= 0xff){
    switch( rc ){
      case SQLITE_OK:                 zName = "SQLITE_OK";                break;
      case SQLITE_ERROR:              zName = "SQLITE_ERROR";             break;
      case SQLITE_ERROR_SNAPSHOT:     zName = "SQLITE_ERROR_SNAPSHOT";    break;
      case SQLITE_INTERNAL:           zName = "SQLITE_INTERNAL";          break;
      case SQLITE_PERM:               zName = "SQLITE_PERM";              break;
      case SQLITE_ABORT:              zName = "SQLITE_ABORT";             break;
      case SQLITE_ABORT_ROLLBACK:     zName = "SQLITE_ABORT_ROLLBACK";    break;
      case SQLITE_BUSY:               zName = "SQLITE_BUSY";              break;
      case SQLITE_BUSY_RECOVERY:      zName = "SQLITE_BUSY_RECOVERY";     break;
      case SQLITE_BUSY_SNAPSHOT:      zName = "SQLITE_BUSY_SNAPSHOT";     break;
      case SQLITE_LOCKED:             zName = "SQLITE_LOCKED";            break;
      case SQLITE_LOCKED_SHAREDCACHE: zName = "SQLITE_LOCKED_SHAREDCACHE";break;
      case SQLITE_NOMEM:              zName = "SQLITE_NOMEM";             break;
      case SQLITE_READONLY:           zName = "SQLITE_READONLY";          break;
      case SQLITE_READONLY_RECOVERY:  zName = "SQLITE_READONLY_RECOVERY"; break;
      case SQLITE_READONLY_CANTINIT:  zName = "SQLITE_READONLY_CANTINIT"; break;
      case SQLITE_READONLY_ROLLBACK:  zName = "SQLITE_READONLY_ROLLBACK"; break;
      case SQLITE_READONLY_DBMOVED:   zName = "SQLITE_READONLY_DBMOVED";  break;
      case SQLITE_READONLY_DIRECTORY: zName = "SQLITE_READONLY_DIRECTORY";break;
      case SQLITE_INTERRUPT:          zName = "SQLITE_INTERRUPT";         break;
      case SQLITE_IOERR:              zName = "SQLITE_IOERR";             break;
      case SQLITE_IOERR_READ:         zName = "SQLITE_IOERR_READ";        break;
      case SQLITE_IOERR_SHORT_READ:   zName = "SQLITE_IOERR_SHORT_READ";  break;
      case SQLITE_IOERR_WRITE:        zName = "SQLITE_IOERR_WRITE";       break;
      case SQLITE_IOERR_FSYNC:        zName = "SQLITE_IOERR_FSYNC";       break;
      case SQLITE_IOERR_DIR_FSYNC:    zName = "SQLITE_IOERR_DIR_FSYNC";   break;
      case SQLITE_IOERR_TRUNCATE:     zName = "SQLITE_IOERR_TRUNCATE";    break;
      case SQLITE_IOERR_FSTAT:        zName = "SQLITE_IOERR_FSTAT";       break;
      case SQLITE_IOERR_UNLOCK:       zName = "SQLITE_IOERR_UNLOCK";      break;
      case SQLITE_IOERR_RDLOCK:       zName = "SQLITE_IOERR_RDLOCK";      break;
      case SQLITE_IOERR_DELETE:       zName = "SQLITE_IOERR_DELETE";      break;
      case SQLITE_IOERR_NOMEM:        zName = "SQLITE_IOERR_NOMEM";       break;
      case SQLITE_IOERR_ACCESS:       zName = "SQLITE_IOERR_ACCESS";      break;
      case SQLITE_IOERR_CHECKRESERVEDLOCK:
                                zName = "SQLITE_IOERR_CHECKRESERVEDLOCK"; break;
      case SQLITE_IOERR_LOCK:         zName = "SQLITE_IOERR_LOCK";        break;
      case SQLITE_IOERR_CLOSE:        zName = "SQLITE_IOERR_CLOSE";       break;
      case SQLITE_IOERR_DIR_CLOSE:    zName = "SQLITE_IOERR_DIR_CLOSE";   break;
      case SQLITE_IOERR_SHMOPEN:      zName = "SQLITE_IOERR_SHMOPEN";     break;
      case SQLITE_IOERR_SHMSIZE:      zName = "SQLITE_IOERR_SHMSIZE";     break;
      case SQLITE_IOERR_SHMLOCK:      zName = "SQLITE_IOERR_SHMLOCK";     break;
      case SQLITE_IOERR_SHMMAP:       zName = "SQLITE_IOERR_SHMMAP";      break;
      case SQLITE_IOERR_SEEK:         zName = "SQLITE_IOERR_SEEK";        break;
      case SQLITE_IOERR_DELETE_NOENT: zName = "SQLITE_IOERR_DELETE_NOENT";break;
      case SQLITE_IOERR_MMAP:         zName = "SQLITE_IOERR_MMAP";        break;
      case SQLITE_IOERR_GETTEMPPATH:  zName = "SQLITE_IOERR_GETTEMPPATH"; break;
      case SQLITE_IOERR_CONVPATH:     zName = "SQLITE_IOERR_CONVPATH";    break;
      case SQLITE_CORRUPT:            zName = "SQLITE_CORRUPT";           break;
      case SQLITE_CORRUPT_VTAB:       zName = "SQLITE_CORRUPT_VTAB";      break;
      case SQLITE_NOTFOUND:           zName = "SQLITE_NOTFOUND";          break;
      case SQLITE_FULL:               zName = "SQLITE_FULL";              break;
      case SQLITE_CANTOPEN:           zName = "SQLITE_CANTOPEN";          break;
      case SQLITE_CANTOPEN_NOTEMPDIR: zName = "SQLITE_CANTOPEN_NOTEMPDIR";break;
      case SQLITE_CANTOPEN_ISDIR:     zName = "SQLITE_CANTOPEN_ISDIR";    break;
      case SQLITE_CANTOPEN_FULLPATH:  zName = "SQLITE_CANTOPEN_FULLPATH"; break;
      case SQLITE_CANTOPEN_CONVPATH:  zName = "SQLITE_CANTOPEN_CONVPATH"; break;
      case SQLITE_CANTOPEN_SYMLINK:   zName = "SQLITE_CANTOPEN_SYMLINK";  break;
      case SQLITE_PROTOCOL:           zName = "SQLITE_PROTOCOL";          break;
      case SQLITE_EMPTY:              zName = "SQLITE_EMPTY";             break;
      case SQLITE_SCHEMA:             zName = "SQLITE_SCHEMA";            break;
      case SQLITE_TOOBIG:             zName = "SQLITE_TOOBIG";            break;
      case SQLITE_CONSTRAINT:         zName = "SQLITE_CONSTRAINT";        break;
      case SQLITE_CONSTRAINT_UNIQUE:  zName = "SQLITE_CONSTRAINT_UNIQUE"; break;
      case SQLITE_CONSTRAINT_TRIGGER: zName = "SQLITE_CONSTRAINT_TRIGGER";break;
      case SQLITE_CONSTRAINT_FOREIGNKEY:
                                zName = "SQLITE_CONSTRAINT_FOREIGNKEY";   break;
      case SQLITE_CONSTRAINT_CHECK:   zName = "SQLITE_CONSTRAINT_CHECK";  break;
      case SQLITE_CONSTRAINT_PRIMARYKEY:
                                zName = "SQLITE_CONSTRAINT_PRIMARYKEY";   break;
      case SQLITE_CONSTRAINT_NOTNULL: zName = "SQLITE_CONSTRAINT_NOTNULL";break;
      case SQLITE_CONSTRAINT_COMMITHOOK:
                                zName = "SQLITE_CONSTRAINT_COMMITHOOK";   break;
      case SQLITE_CONSTRAINT_VTAB:    zName = "SQLITE_CONSTRAINT_VTAB";   break;
      case SQLITE_CONSTRAINT_FUNCTION:
                                zName = "SQLITE_CONSTRAINT_FUNCTION";     break;
      case SQLITE_CONSTRAINT_ROWID:   zName = "SQLITE_CONSTRAINT_ROWID";  break;
      case SQLITE_MISMATCH:           zName = "SQLITE_MISMATCH";          break;
      case SQLITE_MISUSE:             zName = "SQLITE_MISUSE";            break;
      case SQLITE_NOLFS:              zName = "SQLITE_NOLFS";             break;
      case SQLITE_AUTH:               zName = "SQLITE_AUTH";              break;
      case SQLITE_FORMAT:             zName = "SQLITE_FORMAT";            break;
      case SQLITE_RANGE:              zName = "SQLITE_RANGE";             break;
      case SQLITE_NOTADB:             zName = "SQLITE_NOTADB";            break;
      case SQLITE_ROW:                zName = "SQLITE_ROW";               break;
      case SQLITE_NOTICE:             zName = "SQLITE_NOTICE";            break;
      case SQLITE_NOTICE_RECOVER_WAL: zName = "SQLITE_NOTICE_RECOVER_WAL";break;
      case SQLITE_NOTICE_RECOVER_ROLLBACK:
                                zName = "SQLITE_NOTICE_RECOVER_ROLLBACK"; break;
      case SQLITE_NOTICE_RBU:         zName = "SQLITE_NOTICE_RBU"; break;
      case SQLITE_WARNING:            zName = "SQLITE_WARNING";           break;
      case SQLITE_WARNING_AUTOINDEX:  zName = "SQLITE_WARNING_AUTOINDEX"; break;
      case SQLITE_DONE:               zName = "SQLITE_DONE";              break;
    }
  }
  if( zName==0 ){
    static char zBuf[50];
    sqlite3_snprintf(sizeof(zBuf), zBuf, "SQLITE_UNKNOWN(%d)", origRc);
    zName = zBuf;
  }
  return zName;
}
#endif

/*
** Return a static string that describes the kind of error specified in the
** argument.
*/
SQLITE_PRIVATE const char *sqlite3ErrStr(int rc){
  static const char* const aMsg[] = {
    /* SQLITE_OK          */ "not an error",
    /* SQLITE_ERROR       */ "SQL logic error",
    /* SQLITE_INTERNAL    */ 0,
    /* SQLITE_PERM        */ "access permission denied",
    /* SQLITE_ABORT       */ "query aborted",
    /* SQLITE_BUSY        */ "database is locked",
    /* SQLITE_LOCKED      */ "database table is locked",
    /* SQLITE_NOMEM       */ "out of memory",
    /* SQLITE_READONLY    */ "attempt to write a readonly database",
    /* SQLITE_INTERRUPT   */ "interrupted",
    /* SQLITE_IOERR       */ "disk I/O error",
    /* SQLITE_CORRUPT     */ "database disk image is malformed",
    /* SQLITE_NOTFOUND    */ "unknown operation",
    /* SQLITE_FULL        */ "database or disk is full",
    /* SQLITE_CANTOPEN    */ "unable to open database file",
    /* SQLITE_PROTOCOL    */ "locking protocol",
    /* SQLITE_EMPTY       */ 0,
    /* SQLITE_SCHEMA      */ "database schema has changed",
    /* SQLITE_TOOBIG      */ "string or blob too big",
    /* SQLITE_CONSTRAINT  */ "constraint failed",
    /* SQLITE_MISMATCH    */ "datatype mismatch",
    /* SQLITE_MISUSE      */ "bad parameter or other API misuse",
#ifdef SQLITE_DISABLE_LFS
    /* SQLITE_NOLFS       */ "large file support is disabled",
#else
    /* SQLITE_NOLFS       */ 0,
#endif
    /* SQLITE_AUTH        */ "authorization denied",
    /* SQLITE_FORMAT      */ 0,
    /* SQLITE_RANGE       */ "column index out of range",
    /* SQLITE_NOTADB      */ "file is not a database",
    /* SQLITE_NOTICE      */ "notification message",
    /* SQLITE_WARNING     */ "warning message",
  };
  const char *zErr = "unknown error";
  switch( rc ){
    case SQLITE_ABORT_ROLLBACK: {
      zErr = "abort due to ROLLBACK";
      break;
    }
    case SQLITE_ROW: {
      zErr = "another row available";
      break;
    }
    case SQLITE_DONE: {
      zErr = "no more rows available";
      break;
    }
    default: {
      rc &= 0xff;
      if( ALWAYS(rc>=0) && rc<ArraySize(aMsg) && aMsg[rc]!=0 ){
        zErr = aMsg[rc];
      }
      break;
    }
  }
  return zErr;
}

/*
** This routine implements a busy callback that sleeps and tries
** again until a timeout value is reached.  The timeout value is
** an integer number of milliseconds passed in as the first
** argument.
**
** Return non-zero to retry the lock.  Return zero to stop trying
** and cause SQLite to return SQLITE_BUSY.
*/
static int sqliteDefaultBusyCallback(
  void *ptr,               /* Database connection */
  int count                /* Number of times table has been busy */
){
#if SQLITE_OS_WIN || !defined(HAVE_NANOSLEEP) || HAVE_NANOSLEEP
  /* This case is for systems that have support for sleeping for fractions of
  ** a second.  Examples:  All windows systems, unix systems with nanosleep() */
  static const u8 delays[] =
     { 1, 2, 5, 10, 15, 20, 25, 25,  25,  50,  50, 100 };
  static const u8 totals[] =
     { 0, 1, 3,  8, 18, 33, 53, 78, 103, 128, 178, 228 };
# define NDELAY ArraySize(delays)
  sqlite3 *db = (sqlite3 *)ptr;
  int tmout = db->busyTimeout;
  int delay, prior;

  assert( count>=0 );
  if( count < NDELAY ){
    delay = delays[count];
    prior = totals[count];
  }else{
    delay = delays[NDELAY-1];
    prior = totals[NDELAY-1] + delay*(count-(NDELAY-1));
  }
  if( prior + delay > tmout ){
    delay = tmout - prior;
    if( delay<=0 ) return 0;
  }
  sqlite3OsSleep(db->pVfs, delay*1000);
  return 1;
#else
  /* This case for unix systems that lack usleep() support.  Sleeping
  ** must be done in increments of whole seconds */
  sqlite3 *db = (sqlite3 *)ptr;
  int tmout = ((sqlite3 *)ptr)->busyTimeout;
  if( (count+1)*1000 > tmout ){
    return 0;
  }
  sqlite3OsSleep(db->pVfs, 1000000);
  return 1;
#endif
}

/*
** Invoke the given busy handler.
**
** This routine is called when an operation failed to acquire a
** lock on VFS file pFile.
**
** If this routine returns non-zero, the lock is retried.  If it
** returns 0, the operation aborts with an SQLITE_BUSY error.
*/
SQLITE_PRIVATE int sqlite3InvokeBusyHandler(BusyHandler *p){
  int rc;
  if( p->xBusyHandler==0 || p->nBusy<0 ) return 0;
  rc = p->xBusyHandler(p->pBusyArg, p->nBusy);
  if( rc==0 ){
    p->nBusy = -1;
  }else{
    p->nBusy++;
  }
  return rc;
}

/*
** This routine sets the busy callback for an Sqlite database to the
** given callback function with the given argument.
*/
SQLITE_API int sqlite3_busy_handler(
  sqlite3 *db,
  int (*xBusy)(void*,int),
  void *pArg
){
#ifdef SQLITE_ENABLE_API_ARMOR
  if( !sqlite3SafetyCheckOk(db) ) return SQLITE_MISUSE_BKPT;
#endif
  sqlite3_mutex_enter(db->mutex);
  db->busyHandler.xBusyHandler = xBusy;
  db->busyHandler.pBusyArg = pArg;
  db->busyHandler.nBusy = 0;
  db->busyTimeout = 0;
  sqlite3_mutex_leave(db->mutex);
  return SQLITE_OK;
}

#ifndef SQLITE_OMIT_PROGRESS_CALLBACK
/*
** This routine sets the progress callback for an Sqlite database to the
** given callback function with the given argument. The progress callback will
** be invoked every nOps opcodes.
*/
SQLITE_API void sqlite3_progress_handler(
  sqlite3 *db,
  int nOps,
  int (*xProgress)(void*),
  void *pArg
){
#ifdef SQLITE_ENABLE_API_ARMOR
  if( !sqlite3SafetyCheckOk(db) ){
    (void)SQLITE_MISUSE_BKPT;
    return;
  }
#endif
  sqlite3_mutex_enter(db->mutex);
  if( nOps>0 ){
    db->xProgress = xProgress;
    db->nProgressOps = (unsigned)nOps;
    db->pProgressArg = pArg;
  }else{
    db->xProgress = 0;
    db->nProgressOps = 0;
    db->pProgressArg = 0;
  }
  sqlite3_mutex_leave(db->mutex);
}
#endif


/*
** This routine installs a default busy handler that waits for the
** specified number of milliseconds before returning 0.
*/
SQLITE_API int sqlite3_busy_timeout(sqlite3 *db, int ms){
#ifdef SQLITE_ENABLE_API_ARMOR
  if( !sqlite3SafetyCheckOk(db) ) return SQLITE_MISUSE_BKPT;
#endif
  if( ms>0 ){
    sqlite3_busy_handler(db, (int(*)(void*,int))sqliteDefaultBusyCallback,
                             (void*)db);
    db->busyTimeout = ms;
  }else{
    sqlite3_busy_handler(db, 0, 0);
  }
  return SQLITE_OK;
}

/*
** Cause any pending operation to stop at its earliest opportunity.
*/
SQLITE_API void sqlite3_interrupt(sqlite3 *db){
#ifdef SQLITE_ENABLE_API_ARMOR
  if( !sqlite3SafetyCheckOk(db)
   && (db==0 || db->eOpenState!=SQLITE_STATE_ZOMBIE)
  ){
    (void)SQLITE_MISUSE_BKPT;
    return;
  }
#endif
  AtomicStore(&db->u1.isInterrupted, 1);
}

/*
** Return true or false depending on whether or not an interrupt is
** pending on connection db.
*/
SQLITE_API int sqlite3_is_interrupted(sqlite3 *db){
#ifdef SQLITE_ENABLE_API_ARMOR
  if( !sqlite3SafetyCheckOk(db)
   && (db==0 || db->eOpenState!=SQLITE_STATE_ZOMBIE)
  ){
    (void)SQLITE_MISUSE_BKPT;
    return 0;
  }
#endif
  return AtomicLoad(&db->u1.isInterrupted)!=0;
}
