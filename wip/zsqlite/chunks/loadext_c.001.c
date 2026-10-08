
/* True if x is the directory separator character
*/
#if SQLITE_OS_WIN
# define DirSep(X)  ((X)=='/'||(X)=='\\')
#else
# define DirSep(X)  ((X)=='/')
#endif

/*
** Attempt to load an SQLite extension library contained in the file
** zFile.  The entry point is zProc.  zProc may be 0 in which case a
** default entry point name (sqlite3_extension_init) is used.  Use
** of the default name is recommended.
**
** Return SQLITE_OK on success and SQLITE_ERROR if something goes wrong.
**
** If an error occurs and pzErrMsg is not 0, then fill *pzErrMsg with
** error message text.  The calling function should free this memory
** by calling sqlite3DbFree(db, ).
*/
static int sqlite3LoadExtension(
  sqlite3 *db,          /* Load the extension into this database connection */
  const char *zFile,    /* Name of the shared library containing extension */
  const char *zProc,    /* Entry point.  Use "sqlite3_extension_init" if 0 */
  char **pzErrMsg       /* Put error message here if not 0 */
){
  sqlite3_vfs *pVfs = db->pVfs;
  void *handle;
  sqlite3_loadext_entry xInit;
  char *zErrmsg = 0;
  const char *zEntry;
  char *zAltEntry = 0;
  void **aHandle;
  u64 nMsg = strlen(zFile);
  int ii;
  int rc;

  /* Shared library endings to try if zFile cannot be loaded as written */
  static const char *azEndings[] = {
#if SQLITE_OS_WIN
     "dll"
#elif defined(__APPLE__)
     "dylib"
#else
     "so"
#endif
  };


  if( pzErrMsg ) *pzErrMsg = 0;

  /* Ticket #1863.  To avoid a creating security problems for older
  ** applications that relink against newer versions of SQLite, the
  ** ability to run load_extension is turned off by default.  One
  ** must call either sqlite3_enable_load_extension(db) or
  ** sqlite3_db_config(db, SQLITE_DBCONFIG_ENABLE_LOAD_EXTENSION, 1, 0)
  ** to turn on extension loading.
  */
  if( (db->flags & SQLITE_LoadExtension)==0 ){
    if( pzErrMsg ){
      *pzErrMsg = sqlite3_mprintf("not authorized");
    }
    return SQLITE_ERROR;
  }

  zEntry = zProc ? zProc : "sqlite3_extension_init";

  /* tag-20210611-1.  Some dlopen() implementations will segfault if given
  ** an oversize filename.  Most filesystems have a pathname limit of 4K,
  ** so limit the extension filename length to about twice that.
  ** https://sqlite.org/forum/forumpost/08a0d6d9bf
  **
  ** Later (2023-03-25): Save an extra 6 bytes for the filename suffix.
  ** See https://sqlite.org/forum/forumpost/24083b579d.
  */
  if( nMsg>SQLITE_MAX_PATHLEN ) goto extension_not_found;

  /* Do not allow sqlite3_load_extension() to link to a copy of the
  ** running application, by passing in an empty filename. */
  if( nMsg==0 ) goto extension_not_found;

  handle = sqlite3OsDlOpen(pVfs, zFile);
#if SQLITE_OS_UNIX || SQLITE_OS_WIN
  for(ii=0; ii<ArraySize(azEndings) && handle==0; ii++){
    char *zAltFile = sqlite3_mprintf("%s.%s", zFile, azEndings[ii]);
    if( zAltFile==0 ) return SQLITE_NOMEM_BKPT;
    if( nMsg+strlen(azEndings[ii])+1<=SQLITE_MAX_PATHLEN ){
      handle = sqlite3OsDlOpen(pVfs, zAltFile);
    }
    sqlite3_free(zAltFile);
  }
#endif
  if( handle==0 ) goto extension_not_found;
  xInit = (sqlite3_loadext_entry)sqlite3OsDlSym(pVfs, handle, zEntry);

  /* If no entry point was specified and the default legacy
  ** entry point name "sqlite3_extension_init" was not found, then
  ** construct an entry point name "sqlite3_X_init" where the X is
  ** replaced by the lowercase value of every ASCII alphabetic
  ** character in the filename after the last "/" upto the first ".",
  ** and eliding the first three characters if they are "lib".
  ** Examples:
  **
  **    /usr/local/lib/libExample5.4.3.so ==>  sqlite3_example_init
  **    C:/lib/mathfuncs.dll              ==>  sqlite3_mathfuncs_init
  */
  if( xInit==0 && zProc==0 ){
    int iFile, iEntry, c;
    int ncFile = sqlite3Strlen30(zFile);
    zAltEntry = sqlite3_malloc64(ncFile+30);
    if( zAltEntry==0 ){
      sqlite3OsDlClose(pVfs, handle);
      return SQLITE_NOMEM_BKPT;
    }
    memcpy(zAltEntry, "sqlite3_", 8);
    for(iFile=ncFile-1; iFile>=0 && !DirSep(zFile[iFile]); iFile--){}
    iFile++;
    if( sqlite3_strnicmp(zFile+iFile, "lib", 3)==0 ) iFile += 3;
    for(iEntry=8; (c = zFile[iFile])!=0 && c!='.'; iFile++){
      if( sqlite3Isalpha(c) ){
        zAltEntry[iEntry++] = (char)sqlite3UpperToLower[(unsigned)c];
      }
    }
    memcpy(zAltEntry+iEntry, "_init", 6);
    zEntry = zAltEntry;
    xInit = (sqlite3_loadext_entry)sqlite3OsDlSym(pVfs, handle, zEntry);
  }
  if( xInit==0 ){
    if( pzErrMsg ){
      nMsg += strlen(zEntry) + 300;
      *pzErrMsg = zErrmsg = sqlite3_malloc64(nMsg);
      if( zErrmsg ){
        assert( nMsg<0x7fffffff );  /* zErrmsg would be NULL if not so */
        sqlite3_snprintf((int)nMsg, zErrmsg,
            "no entry point [%s] in shared library [%s]", zEntry, zFile);
        sqlite3OsDlError(pVfs, nMsg-1, zErrmsg);
      }
    }
    sqlite3OsDlClose(pVfs, handle);
    sqlite3_free(zAltEntry);
    return SQLITE_ERROR;
  }
  sqlite3_free(zAltEntry);
  rc = xInit(db, &zErrmsg, &sqlite3Apis);
  if( rc ){
    if( rc==SQLITE_OK_LOAD_PERMANENTLY ) return SQLITE_OK;
    if( pzErrMsg ){
      *pzErrMsg = sqlite3_mprintf("error during initialization: %s", zErrmsg);
    }
    sqlite3_free(zErrmsg);
    sqlite3OsDlClose(pVfs, handle);
    return SQLITE_ERROR;
  }

  /* Append the new shared library handle to the db->aExtension array. */
  aHandle = sqlite3DbMallocZero(db, sizeof(handle)*(db->nExtension+1));
  if( aHandle==0 ){
    return SQLITE_NOMEM_BKPT;
  }
  if( db->nExtension>0 ){
    memcpy(aHandle, db->aExtension, sizeof(handle)*db->nExtension);
  }
  sqlite3DbFree(db, db->aExtension);
  db->aExtension = aHandle;

  db->aExtension[db->nExtension++] = handle;
  return SQLITE_OK;

extension_not_found:
  if( pzErrMsg ){
    nMsg += 300;
    *pzErrMsg = zErrmsg = sqlite3_malloc64(nMsg);
    if( zErrmsg ){
      assert( nMsg<0x7fffffff );  /* zErrmsg would be NULL if not so */
      sqlite3_snprintf((int)nMsg, zErrmsg,
          "unable to open shared library [%.*s]", SQLITE_MAX_PATHLEN, zFile);
      sqlite3OsDlError(pVfs, nMsg-1, zErrmsg);
    }
  }
  return SQLITE_ERROR;
}
SQLITE_API int sqlite3_load_extension(
  sqlite3 *db,          /* Load the extension into this database connection */
  const char *zFile,    /* Name of the shared library containing extension */
  const char *zProc,    /* Entry point.  Use "sqlite3_extension_init" if 0 */
  char **pzErrMsg       /* Put error message here if not 0 */
){
  int rc;
  sqlite3_mutex_enter(db->mutex);
  rc = sqlite3LoadExtension(db, zFile, zProc, pzErrMsg);
  rc = sqlite3ApiExit(db, rc);
  sqlite3_mutex_leave(db->mutex);
  return rc;
}

/*
** Call this routine when the database connection is closing in order
** to clean up loaded extensions
*/
SQLITE_PRIVATE void sqlite3CloseExtensions(sqlite3 *db){
  int i;
  assert( sqlite3_mutex_held(db->mutex) );
  for(i=0; i<db->nExtension; i++){
    sqlite3OsDlClose(db->pVfs, db->aExtension[i]);
  }
  sqlite3DbFree(db, db->aExtension);
}

/*
** Enable or disable extension loading.  Extension loading is disabled by
** default so as not to open security holes in older applications.
*/
SQLITE_API int sqlite3_enable_load_extension(sqlite3 *db, int onoff){
#ifdef SQLITE_ENABLE_API_ARMOR
  if( !sqlite3SafetyCheckOk(db) ) return SQLITE_MISUSE_BKPT;
#endif
  sqlite3_mutex_enter(db->mutex);
  if( onoff ){
    db->flags |= SQLITE_LoadExtension|SQLITE_LoadExtFunc;
  }else{
    db->flags &= ~(u64)(SQLITE_LoadExtension|SQLITE_LoadExtFunc);
  }
  sqlite3_mutex_leave(db->mutex);
  return SQLITE_OK;
}

#endif /* !defined(SQLITE_OMIT_LOAD_EXTENSION) */

/*
** The following object holds the list of automatically loaded
** extensions.
**
** This list is shared across threads.  The SQLITE_MUTEX_STATIC_MAIN
** mutex must be held while accessing this list.
*/
typedef struct sqlite3AutoExtList sqlite3AutoExtList;
static SQLITE_WSD struct sqlite3AutoExtList {
  u32 nExt;              /* Number of entries in aExt[] */
  void (**aExt)(void);   /* Pointers to the extension init functions */
} sqlite3Autoext = { 0, 0 };

/* The "wsdAutoext" macro will resolve to the autoextension
** state vector.  If writable static data is unsupported on the target,
** we have to locate the state vector at run-time.  In the more common
** case where writable static data is supported, wsdStat can refer directly
** to the "sqlite3Autoext" state vector declared above.
*/
#ifdef SQLITE_OMIT_WSD
# define wsdAutoextInit \
  sqlite3AutoExtList *x = &GLOBAL(sqlite3AutoExtList,sqlite3Autoext)
# define wsdAutoext x[0]
#else
# define wsdAutoextInit
# define wsdAutoext sqlite3Autoext
#endif


/*
** Register a statically linked extension that is automatically
** loaded by every new database connection.
*/
SQLITE_API int sqlite3_auto_extension(
  void (*xInit)(void)
){
  int rc = SQLITE_OK;
#ifdef SQLITE_ENABLE_API_ARMOR
  if( xInit==0 ) return SQLITE_MISUSE_BKPT;
#endif
#ifndef SQLITE_OMIT_AUTOINIT
  rc = sqlite3_initialize();
  if( rc ){
    return rc;
  }else
#endif
  {
    u32 i;
#if SQLITE_THREADSAFE
    sqlite3_mutex *mutex = sqlite3MutexAlloc(SQLITE_MUTEX_STATIC_MAIN);
#endif
    wsdAutoextInit;
    sqlite3_mutex_enter(mutex);
    for(i=0; i<wsdAutoext.nExt; i++){
      if( wsdAutoext.aExt[i]==xInit ) break;
    }
    if( i==wsdAutoext.nExt ){
      u64 nByte = (wsdAutoext.nExt+1)*sizeof(wsdAutoext.aExt[0]);
      void (**aNew)(void);
      aNew = sqlite3_realloc64(wsdAutoext.aExt, nByte);
      if( aNew==0 ){
        rc = SQLITE_NOMEM_BKPT;
      }else{
        wsdAutoext.aExt = aNew;
        wsdAutoext.aExt[wsdAutoext.nExt] = xInit;
        wsdAutoext.nExt++;
      }
    }
    sqlite3_mutex_leave(mutex);
    assert( (rc&0xff)==rc );
    return rc;
  }
}

/*
** Cancel a prior call to sqlite3_auto_extension.  Remove xInit from the
** set of routines that is invoked for each new database connection, if it
** is currently on the list.  If xInit is not on the list, then this
** routine is a no-op.
**
** Return 1 if xInit was found on the list and removed.  Return 0 if xInit
** was not on the list.
*/
SQLITE_API int sqlite3_cancel_auto_extension(
  void (*xInit)(void)
){
#if SQLITE_THREADSAFE
  sqlite3_mutex *mutex = sqlite3MutexAlloc(SQLITE_MUTEX_STATIC_MAIN);
#endif
  int i;
  int n = 0;
  wsdAutoextInit;
#ifdef SQLITE_ENABLE_API_ARMOR
  if( xInit==0 ) return 0;
#endif
  sqlite3_mutex_enter(mutex);
  for(i=(int)wsdAutoext.nExt-1; i>=0; i--){
    if( wsdAutoext.aExt[i]==xInit ){
      wsdAutoext.nExt--;
      wsdAutoext.aExt[i] = wsdAutoext.aExt[wsdAutoext.nExt];
      n++;
      break;
    }
  }
  sqlite3_mutex_leave(mutex);
  return n;
}

/*
** Reset the automatic extension loading mechanism.
*/
SQLITE_API void sqlite3_reset_auto_extension(void){
#ifndef SQLITE_OMIT_AUTOINIT
  if( sqlite3_initialize()==SQLITE_OK )
#endif
  {
#if SQLITE_THREADSAFE
    sqlite3_mutex *mutex = sqlite3MutexAlloc(SQLITE_MUTEX_STATIC_MAIN);
#endif
    wsdAutoextInit;
    sqlite3_mutex_enter(mutex);
    sqlite3_free(wsdAutoext.aExt);
    wsdAutoext.aExt = 0;
    wsdAutoext.nExt = 0;
    sqlite3_mutex_leave(mutex);
  }
}
