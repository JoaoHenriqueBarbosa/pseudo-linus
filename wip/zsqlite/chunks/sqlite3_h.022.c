
/*
** CAPI3REF: Mutex Verification Routines
**
** The sqlite3_mutex_held() and sqlite3_mutex_notheld() routines
** are intended for use inside assert() statements.  The SQLite core
** never uses these routines except inside an assert() and applications
** are advised to follow the lead of the core.  The SQLite core only
** provides implementations for these routines when it is compiled
** with the SQLITE_DEBUG flag.  External mutex implementations
** are only required to provide these routines if SQLITE_DEBUG is
** defined and if NDEBUG is not defined.
**
** These routines should return true if the mutex in their argument
** is held or not held, respectively, by the calling thread.
**
** The implementation is not required to provide versions of these
** routines that actually work. If the implementation does not provide working
** versions of these routines, it should at least provide stubs that always
** return true so that one does not get spurious assertion failures.
**
** If the argument to sqlite3_mutex_held() is a NULL pointer then
** the routine should return 1.   This seems counter-intuitive since
** clearly the mutex cannot be held if it does not exist.  But
** the reason the mutex does not exist is because the build is not
** using mutexes.  And we do not want the assert() containing the
** call to sqlite3_mutex_held() to fail, so a non-zero return is
** the appropriate thing to do.  The sqlite3_mutex_notheld()
** interface should also return 1 when given a NULL pointer.
*/
#ifndef NDEBUG
SQLITE_API int sqlite3_mutex_held(sqlite3_mutex*);
SQLITE_API int sqlite3_mutex_notheld(sqlite3_mutex*);
#endif

/*
** CAPI3REF: Mutex Types
**
** The [sqlite3_mutex_alloc()] interface takes a single argument
** which is one of these integer constants.
**
** The set of static mutexes may change from one SQLite release to the
** next.  Applications that override the built-in mutex logic must be
** prepared to accommodate additional static mutexes.
*/
#define SQLITE_MUTEX_FAST             0
#define SQLITE_MUTEX_RECURSIVE        1
#define SQLITE_MUTEX_STATIC_MAIN      2
#define SQLITE_MUTEX_STATIC_MEM       3  /* sqlite3_malloc() */
#define SQLITE_MUTEX_STATIC_MEM2      4  /* NOT USED */
#define SQLITE_MUTEX_STATIC_OPEN      4  /* sqlite3BtreeOpen() */
#define SQLITE_MUTEX_STATIC_PRNG      5  /* sqlite3_randomness() */
#define SQLITE_MUTEX_STATIC_LRU       6  /* lru page list */
#define SQLITE_MUTEX_STATIC_LRU2      7  /* NOT USED */
#define SQLITE_MUTEX_STATIC_PMEM      7  /* sqlite3PageMalloc() */
#define SQLITE_MUTEX_STATIC_APP1      8  /* For use by application */
#define SQLITE_MUTEX_STATIC_APP2      9  /* For use by application */
#define SQLITE_MUTEX_STATIC_APP3     10  /* For use by application */
#define SQLITE_MUTEX_STATIC_VFS1     11  /* For use by built-in VFS */
#define SQLITE_MUTEX_STATIC_VFS2     12  /* For use by extension VFS */
#define SQLITE_MUTEX_STATIC_VFS3     13  /* For use by application VFS */

/* Legacy compatibility: */
#define SQLITE_MUTEX_STATIC_MASTER    2


/*
** CAPI3REF: Retrieve the mutex for a database connection
** METHOD: sqlite3
**
** ^This interface returns a pointer the [sqlite3_mutex] object that
** serializes access to the [database connection] given in the argument
** when the [threading mode] is Serialized.
** ^If the [threading mode] is Single-thread or Multi-thread then this
** routine returns a NULL pointer.
*/
SQLITE_API sqlite3_mutex *sqlite3_db_mutex(sqlite3*);

/*
** CAPI3REF: Low-Level Control Of Database Files
** METHOD: sqlite3
** KEYWORDS: {file control}
**
** ^The [sqlite3_file_control()] interface makes a direct call to the
** xFileControl method for the [sqlite3_io_methods] object associated
** with a particular database identified by the second argument. ^The
** name of the database is "main" for the main database or "temp" for the
** TEMP database, or the name that appears after the AS keyword for
** databases that are added using the [ATTACH] SQL command.
** ^A NULL pointer can be used in place of "main" to refer to the
** main database file.
** ^The third and fourth parameters to this routine
** are passed directly through to the second and third parameters of
** the xFileControl method.  ^The return value of the xFileControl
** method becomes the return value of this routine.
**
** A few opcodes for [sqlite3_file_control()] are handled directly
** by the SQLite core and never invoke the
** sqlite3_io_methods.xFileControl method.
** ^The [SQLITE_FCNTL_FILE_POINTER] value for the op parameter causes
** a pointer to the underlying [sqlite3_file] object to be written into
** the space pointed to by the 4th parameter.  The
** [SQLITE_FCNTL_JOURNAL_POINTER] works similarly except that it returns
** the [sqlite3_file] object associated with the journal file instead of
** the main database.  The [SQLITE_FCNTL_VFS_POINTER] opcode returns
** a pointer to the underlying [sqlite3_vfs] object for the file.
** The [SQLITE_FCNTL_DATA_VERSION] returns the data version counter
** from the pager.
**
** ^If the second parameter (zDbName) does not match the name of any
** open database file, then SQLITE_ERROR is returned.  ^This error
** code is not remembered and will not be recalled by [sqlite3_errcode()]
** or [sqlite3_errmsg()].  The underlying xFileControl method might
** also return SQLITE_ERROR.  There is no way to distinguish between
** an incorrect zDbName and an SQLITE_ERROR return from the underlying
** xFileControl method.
**
** See also: [file control opcodes]
*/
SQLITE_API int sqlite3_file_control(sqlite3*, const char *zDbName, int op, void*);

/*
** CAPI3REF: Testing Interface
**
** ^The sqlite3_test_control() interface is used to read out internal
** state of SQLite and to inject faults into SQLite for testing
** purposes.  ^The first parameter is an operation code that determines
** the number, meaning, and operation of all subsequent parameters.
**
** This interface is not for use by applications.  It exists solely
** for verifying the correct operation of the SQLite library.  Depending
** on how the SQLite library is compiled, this interface might not exist.
**
** The details of the operation codes, their meanings, the parameters
** they take, and what they do are all subject to change without notice.
** Unlike most of the SQLite API, this function is not guaranteed to
** operate consistently from one release to the next.
*/
SQLITE_API int sqlite3_test_control(int op, ...);

/*
** CAPI3REF: Testing Interface Operation Codes
**
** These constants are the valid operation code parameters used
** as the first argument to [sqlite3_test_control()].
**
** These parameters and their meanings are subject to change
** without notice.  These values are for testing purposes only.
** Applications should not use any of these parameters or the
** [sqlite3_test_control()] interface.
*/
#define SQLITE_TESTCTRL_FIRST                    5
#define SQLITE_TESTCTRL_PRNG_SAVE                5
#define SQLITE_TESTCTRL_PRNG_RESTORE             6
#define SQLITE_TESTCTRL_PRNG_RESET               7  /* NOT USED */
#define SQLITE_TESTCTRL_FK_NO_ACTION             7
#define SQLITE_TESTCTRL_BITVEC_TEST              8
#define SQLITE_TESTCTRL_FAULT_INSTALL            9
#define SQLITE_TESTCTRL_BENIGN_MALLOC_HOOKS     10
#define SQLITE_TESTCTRL_PENDING_BYTE            11
#define SQLITE_TESTCTRL_ASSERT                  12
#define SQLITE_TESTCTRL_ALWAYS                  13
#define SQLITE_TESTCTRL_RESERVE                 14  /* NOT USED */
#define SQLITE_TESTCTRL_JSON_SELFCHECK          14
#define SQLITE_TESTCTRL_OPTIMIZATIONS           15
#define SQLITE_TESTCTRL_ISKEYWORD               16  /* NOT USED */
#define SQLITE_TESTCTRL_SCRATCHMALLOC           17  /* NOT USED */
#define SQLITE_TESTCTRL_INTERNAL_FUNCTIONS      17
#define SQLITE_TESTCTRL_LOCALTIME_FAULT         18
#define SQLITE_TESTCTRL_EXPLAIN_STMT            19  /* NOT USED */
#define SQLITE_TESTCTRL_ONCE_RESET_THRESHOLD    19
#define SQLITE_TESTCTRL_NEVER_CORRUPT           20
#define SQLITE_TESTCTRL_VDBE_COVERAGE           21
#define SQLITE_TESTCTRL_BYTEORDER               22
#define SQLITE_TESTCTRL_ISINIT                  23
#define SQLITE_TESTCTRL_SORTER_MMAP             24
#define SQLITE_TESTCTRL_IMPOSTER                25
#define SQLITE_TESTCTRL_PARSER_COVERAGE         26
#define SQLITE_TESTCTRL_RESULT_INTREAL          27
#define SQLITE_TESTCTRL_PRNG_SEED               28
#define SQLITE_TESTCTRL_EXTRA_SCHEMA_CHECKS     29
#define SQLITE_TESTCTRL_SEEK_COUNT              30
#define SQLITE_TESTCTRL_TRACEFLAGS              31
#define SQLITE_TESTCTRL_TUNE                    32
#define SQLITE_TESTCTRL_LOGEST                  33
#define SQLITE_TESTCTRL_USELONGDOUBLE           34
#define SQLITE_TESTCTRL_LAST                    34  /* Largest TESTCTRL */

/*
** CAPI3REF: SQL Keyword Checking
**
** These routines provide access to the set of SQL language keywords
** recognized by SQLite.  Applications can uses these routines to determine
** whether or not a specific identifier needs to be escaped (for example,
** by enclosing in double-quotes) so as not to confuse the parser.
**
** The sqlite3_keyword_count() interface returns the number of distinct
** keywords understood by SQLite.
**
** The sqlite3_keyword_name(N,Z,L) interface finds the 0-based N-th keyword and
** makes *Z point to that keyword expressed as UTF8 and writes the number
** of bytes in the keyword into *L.  The string that *Z points to is not
** zero-terminated.  The sqlite3_keyword_name(N,Z,L) routine returns
** SQLITE_OK if N is within bounds and SQLITE_ERROR if not. If either Z
** or L are NULL or invalid pointers then calls to
** sqlite3_keyword_name(N,Z,L) result in undefined behavior.
**
** The sqlite3_keyword_check(Z,L) interface checks to see whether or not
** the L-byte UTF8 identifier that Z points to is a keyword, returning non-zero
** if it is and zero if not.
**
** The parser used by SQLite is forgiving.  It is often possible to use
** a keyword as an identifier as long as such use does not result in a
** parsing ambiguity.  For example, the statement
** "CREATE TABLE BEGIN(REPLACE,PRAGMA,END);" is accepted by SQLite, and
** creates a new table named "BEGIN" with three columns named
** "REPLACE", "PRAGMA", and "END".  Nevertheless, best practice is to avoid
** using keywords as identifiers.  Common techniques used to avoid keyword
** name collisions include:
** <ul>
** <li> Put all identifier names inside double-quotes.  This is the official
**      SQL way to escape identifier names.
** <li> Put identifier names inside &#91;...&#93;.  This is not standard SQL,
**      but it is what SQL Server does and so lots of programmers use this
**      technique.
** <li> Begin every identifier with the letter "Z" as no SQL keywords start
**      with "Z".
** <li> Include a digit somewhere in every identifier name.
** </ul>
**
** Note that the number of keywords understood by SQLite can depend on
** compile-time options.  For example, "VACUUM" is not a keyword if
** SQLite is compiled with the [-DSQLITE_OMIT_VACUUM] option.  Also,
** new keywords may be added to future releases of SQLite.
*/
SQLITE_API int sqlite3_keyword_count(void);
SQLITE_API int sqlite3_keyword_name(int,const char**,int*);
SQLITE_API int sqlite3_keyword_check(const char*,int);

/*
** CAPI3REF: Dynamic String Object
** KEYWORDS: {dynamic string}
**
** An instance of the sqlite3_str object contains a dynamically-sized
** string under construction.
**
** The lifecycle of an sqlite3_str object is as follows:
** <ol>
** <li> ^The sqlite3_str object is created using [sqlite3_str_new()].
** <li> ^Text is appended to the sqlite3_str object using various
** methods, such as [sqlite3_str_appendf()].
** <li> ^The sqlite3_str object is destroyed and the string it created
** is returned using the [sqlite3_str_finish()] interface.
** </ol>
*/
typedef struct sqlite3_str sqlite3_str;

/*
** CAPI3REF: Create A New Dynamic String Object
** CONSTRUCTOR: sqlite3_str
**
** ^The [sqlite3_str_new(D)] interface allocates and initializes
** a new [sqlite3_str] object.  To avoid memory leaks, the object returned by
** [sqlite3_str_new()] must be freed by a subsequent call to
** [sqlite3_str_finish(X)].
**
** ^The [sqlite3_str_new(D)] interface always returns a pointer to a
** valid [sqlite3_str] object, though in the event of an out-of-memory
** error the returned object might be a special singleton that will
** silently reject new text, always return SQLITE_NOMEM from
** [sqlite3_str_errcode()], always return 0 for
** [sqlite3_str_length()], and always return NULL from
** [sqlite3_str_finish(X)].  It is always safe to use the value
** returned by [sqlite3_str_new(D)] as the sqlite3_str parameter
** to any of the other [sqlite3_str] methods.
**
** The D parameter to [sqlite3_str_new(D)] may be NULL.  If the
** D parameter in [sqlite3_str_new(D)] is not NULL, then the maximum
** length of the string contained in the [sqlite3_str] object will be
** the value set for [sqlite3_limit](D,[SQLITE_LIMIT_LENGTH]) instead
** of [SQLITE_MAX_LENGTH].
*/
SQLITE_API sqlite3_str *sqlite3_str_new(sqlite3*);

/*
** CAPI3REF: Finalize A Dynamic String
** DESTRUCTOR: sqlite3_str
**
** ^The [sqlite3_str_finish(X)] interface destroys the sqlite3_str object X
** and returns a pointer to a memory buffer obtained from [sqlite3_malloc64()]
** that contains the constructed string.  The calling application should
** pass the returned value to [sqlite3_free()] to avoid a memory leak.
** ^The [sqlite3_str_finish(X)] interface may return a NULL pointer if any
** errors were encountered during construction of the string.  ^The
** [sqlite3_str_finish(X)] interface will also return a NULL pointer if the
** string in [sqlite3_str] object X is zero bytes long.
*/
SQLITE_API char *sqlite3_str_finish(sqlite3_str*);

/*
** CAPI3REF: Add Content To A Dynamic String
** METHOD: sqlite3_str
**
** These interfaces add content to an sqlite3_str object previously obtained
** from [sqlite3_str_new()].
**
** ^The [sqlite3_str_appendf(X,F,...)] and
** [sqlite3_str_vappendf(X,F,V)] interfaces uses the [built-in printf]
** functionality of SQLite to append formatted text onto the end of
** [sqlite3_str] object X.
**
** ^The [sqlite3_str_append(X,S,N)] method appends exactly N bytes from string S
** onto the end of the [sqlite3_str] object X.  N must be non-negative.
** S must contain at least N non-zero bytes of content.  To append a
** zero-terminated string in its entirety, use the [sqlite3_str_appendall()]
** method instead.
**
** ^The [sqlite3_str_appendall(X,S)] method appends the complete content of
** zero-terminated string S onto the end of [sqlite3_str] object X.
**
** ^The [sqlite3_str_appendchar(X,N,C)] method appends N copies of the
** single-byte character C onto the end of [sqlite3_str] object X.
** ^This method can be used, for example, to add whitespace indentation.
**
** ^The [sqlite3_str_reset(X)] method resets the string under construction
** inside [sqlite3_str] object X back to zero bytes in length.
**
** These methods do not return a result code.  ^If an error occurs, that fact
** is recorded in the [sqlite3_str] object and can be recovered by a
** subsequent call to [sqlite3_str_errcode(X)].
*/
SQLITE_API void sqlite3_str_appendf(sqlite3_str*, const char *zFormat, ...);
SQLITE_API void sqlite3_str_vappendf(sqlite3_str*, const char *zFormat, va_list);
SQLITE_API void sqlite3_str_append(sqlite3_str*, const char *zIn, int N);
SQLITE_API void sqlite3_str_appendall(sqlite3_str*, const char *zIn);
SQLITE_API void sqlite3_str_appendchar(sqlite3_str*, int N, char C);
SQLITE_API void sqlite3_str_reset(sqlite3_str*);

/*
** CAPI3REF: Status Of A Dynamic String
** METHOD: sqlite3_str
**
** These interfaces return the current status of an [sqlite3_str] object.
**
** ^If any prior errors have occurred while constructing the dynamic string
** in sqlite3_str X, then the [sqlite3_str_errcode(X)] method will return
** an appropriate error code.  ^The [sqlite3_str_errcode(X)] method returns
** [SQLITE_NOMEM] following any out-of-memory error, or
** [SQLITE_TOOBIG] if the size of the dynamic string exceeds
** [SQLITE_MAX_LENGTH], or [SQLITE_OK] if there have been no errors.
