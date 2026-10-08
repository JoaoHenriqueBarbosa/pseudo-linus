** It is recommended that when a database filename actually does begin with
** a ":" character you should prefix the filename with a pathname such as
** "./" to avoid ambiguity.
**
** ^If the filename is an empty string, then a private, temporary
** on-disk database will be created.  ^This private database will be
** automatically deleted as soon as the database connection is closed.
**
** [[URI filenames in sqlite3_open()]] <h3>URI Filenames</h3>
**
** ^If [URI filename] interpretation is enabled, and the filename argument
** begins with "file:", then the filename is interpreted as a URI. ^URI
** filename interpretation is enabled if the [SQLITE_OPEN_URI] flag is
** set in the third argument to sqlite3_open_v2(), or if it has
** been enabled globally using the [SQLITE_CONFIG_URI] option with the
** [sqlite3_config()] method or by the [SQLITE_USE_URI] compile-time option.
** URI filename interpretation is turned off
** by default, but future releases of SQLite might enable URI filename
** interpretation by default.  See "[URI filenames]" for additional
** information.
**
** URI filenames are parsed according to RFC 3986. ^If the URI contains an
** authority, then it must be either an empty string or the string
** "localhost". ^If the authority is not an empty string or "localhost", an
** error is returned to the caller. ^The fragment component of a URI, if
** present, is ignored.
**
** ^SQLite uses the path component of the URI as the name of the disk file
** which contains the database. ^If the path begins with a '/' character,
** then it is interpreted as an absolute path. ^If the path does not begin
** with a '/' (meaning that the authority section is omitted from the URI)
** then the path is interpreted as a relative path.
** ^(On windows, the first component of an absolute path
** is a drive specification (e.g. "C:").)^
**
** [[core URI query parameters]]
** The query component of a URI may contain parameters that are interpreted
** either by SQLite itself, or by a [VFS | custom VFS implementation].
** SQLite and its built-in [VFSes] interpret the
** following query parameters:
**
** <ul>
**   <li> <b>vfs</b>: ^The "vfs" parameter may be used to specify the name of
**     a VFS object that provides the operating system interface that should
**     be used to access the database file on disk. ^If this option is set to
**     an empty string the default VFS object is used. ^Specifying an unknown
**     VFS is an error. ^If sqlite3_open_v2() is used and the vfs option is
**     present, then the VFS specified by the option takes precedence over
**     the value passed as the fourth parameter to sqlite3_open_v2().
**
**   <li> <b>mode</b>: ^(The mode parameter may be set to either "ro", "rw",
**     "rwc", or "memory". Attempting to set it to any other value is
**     an error)^.
**     ^If "ro" is specified, then the database is opened for read-only
**     access, just as if the [SQLITE_OPEN_READONLY] flag had been set in the
**     third argument to sqlite3_open_v2(). ^If the mode option is set to
**     "rw", then the database is opened for read-write (but not create)
**     access, as if SQLITE_OPEN_READWRITE (but not SQLITE_OPEN_CREATE) had
**     been set. ^Value "rwc" is equivalent to setting both
**     SQLITE_OPEN_READWRITE and SQLITE_OPEN_CREATE.  ^If the mode option is
**     set to "memory" then a pure [in-memory database] that never reads
**     or writes from disk is used. ^It is an error to specify a value for
**     the mode parameter that is less restrictive than that specified by
**     the flags passed in the third parameter to sqlite3_open_v2().
**
**   <li> <b>cache</b>: ^The cache parameter may be set to either "shared" or
**     "private". ^Setting it to "shared" is equivalent to setting the
**     SQLITE_OPEN_SHAREDCACHE bit in the flags argument passed to
**     sqlite3_open_v2(). ^Setting the cache parameter to "private" is
**     equivalent to setting the SQLITE_OPEN_PRIVATECACHE bit.
**     ^If sqlite3_open_v2() is used and the "cache" parameter is present in
**     a URI filename, its value overrides any behavior requested by setting
**     SQLITE_OPEN_PRIVATECACHE or SQLITE_OPEN_SHAREDCACHE flag.
**
**  <li> <b>psow</b>: ^The psow parameter indicates whether or not the
**     [powersafe overwrite] property does or does not apply to the
**     storage media on which the database file resides.
**
**  <li> <b>nolock</b>: ^The nolock parameter is a boolean query parameter
**     which if set disables file locking in rollback journal modes.  This
**     is useful for accessing a database on a filesystem that does not
**     support locking.  Caution:  Database corruption might result if two
**     or more processes write to the same database and any one of those
**     processes uses nolock=1.
**
**  <li> <b>immutable</b>: ^The immutable parameter is a boolean query
**     parameter that indicates that the database file is stored on
**     read-only media.  ^When immutable is set, SQLite assumes that the
**     database file cannot be changed, even by a process with higher
**     privilege, and so the database is opened read-only and all locking
**     and change detection is disabled.  Caution: Setting the immutable
**     property on a database file that does in fact change can result
**     in incorrect query results and/or [SQLITE_CORRUPT] errors.
**     See also: [SQLITE_IOCAP_IMMUTABLE].
**
** </ul>
**
** ^Specifying an unknown parameter in the query component of a URI is not an
** error.  Future versions of SQLite might understand additional query
** parameters.  See "[query parameters with special meaning to SQLite]" for
** additional information.
**
** [[URI filename examples]] <h3>URI filename examples</h3>
**
** <table border="1" align=center cellpadding=5>
** <tr><th> URI filenames <th> Results
** <tr><td> file:data.db <td>
**          Open the file "data.db" in the current directory.
** <tr><td> file:/home/fred/data.db<br>
**          file:///home/fred/data.db <br>
**          file://localhost/home/fred/data.db <br> <td>
**          Open the database file "/home/fred/data.db".
** <tr><td> file://darkstar/home/fred/data.db <td>
**          An error. "darkstar" is not a recognized authority.
** <tr><td style="white-space:nowrap">
**          file:///C:/Documents%20and%20Settings/fred/Desktop/data.db
**     <td> Windows only: Open the file "data.db" on fred's desktop on drive
**          C:. Note that the %20 escaping in this example is not strictly
**          necessary - space characters can be used literally
**          in URI filenames.
** <tr><td> file:data.db?mode=ro&cache=private <td>
**          Open file "data.db" in the current directory for read-only access.
**          Regardless of whether or not shared-cache mode is enabled by
**          default, use a private cache.
** <tr><td> file:/home/fred/data.db?vfs=unix-dotfile <td>
**          Open file "/home/fred/data.db". Use the special VFS "unix-dotfile"
**          that uses dot-files in place of posix advisory locking.
** <tr><td> file:data.db?mode=readonly <td>
**          An error. "readonly" is not a valid option for the "mode" parameter.
**          Use "ro" instead:  "file:data.db?mode=ro".
** </table>
**
** ^URI hexadecimal escape sequences (%HH) are supported within the path and
** query components of a URI. A hexadecimal escape sequence consists of a
** percent sign - "%" - followed by exactly two hexadecimal digits
** specifying an octet value. ^Before the path or query components of a
** URI filename are interpreted, they are encoded using UTF-8 and all
** hexadecimal escape sequences replaced by a single byte containing the
** corresponding octet. If this process generates an invalid UTF-8 encoding,
** the results are undefined.
**
** <b>Note to Windows users:</b>  The encoding used for the filename argument
** of sqlite3_open() and sqlite3_open_v2() must be UTF-8, not whatever
** codepage is currently defined.  Filenames containing international
** characters must be converted to UTF-8 prior to passing them into
** sqlite3_open() or sqlite3_open_v2().
**
** <b>Note to Windows Runtime users:</b>  The temporary directory must be set
** prior to calling sqlite3_open() or sqlite3_open_v2().  Otherwise, various
** features that require the use of temporary files may fail.
**
** See also: [sqlite3_temp_directory]
*/
SQLITE_API int sqlite3_open(
  const char *filename,   /* Database filename (UTF-8) */
  sqlite3 **ppDb          /* OUT: SQLite db handle */
);
SQLITE_API int sqlite3_open16(
  const void *filename,   /* Database filename (UTF-16) */
  sqlite3 **ppDb          /* OUT: SQLite db handle */
);
SQLITE_API int sqlite3_open_v2(
  const char *filename,   /* Database filename (UTF-8) */
  sqlite3 **ppDb,         /* OUT: SQLite db handle */
  int flags,              /* Flags */
  const char *zVfs        /* Name of VFS module to use */
);

/*
** CAPI3REF: Obtain Values For URI Parameters
**
** These are utility routines, useful to [VFS|custom VFS implementations],
** that check if a database file was a URI that contained a specific query
** parameter, and if so obtains the value of that query parameter.
**
** The first parameter to these interfaces (hereafter referred to
** as F) must be one of:
** <ul>
** <li> A database filename pointer created by the SQLite core and
** passed into the xOpen() method of a VFS implementation, or
** <li> A filename obtained from [sqlite3_db_filename()], or
** <li> A new filename constructed using [sqlite3_create_filename()].
** </ul>
** If the F parameter is not one of the above, then the behavior is
** undefined and probably undesirable.  Older versions of SQLite were
** more tolerant of invalid F parameters than newer versions.
**
** If F is a suitable filename (as described in the previous paragraph)
** and if P is the name of the query parameter, then
** sqlite3_uri_parameter(F,P) returns the value of the P
** parameter if it exists or a NULL pointer if P does not appear as a
** query parameter on F.  If P is a query parameter of F and it
** has no explicit value, then sqlite3_uri_parameter(F,P) returns
** a pointer to an empty string.
**
** The sqlite3_uri_boolean(F,P,B) routine assumes that P is a boolean
** parameter and returns true (1) or false (0) according to the value
** of P.  The sqlite3_uri_boolean(F,P,B) routine returns true (1) if the
** value of query parameter P is one of "yes", "true", or "on" in any
** case or if the value begins with a non-zero number.  The
** sqlite3_uri_boolean(F,P,B) routines returns false (0) if the value of
** query parameter P is one of "no", "false", or "off" in any case or
** if the value begins with a numeric zero.  If P is not a query
** parameter on F or if the value of P does not match any of the
** above, then sqlite3_uri_boolean(F,P,B) returns (B!=0).
**
** The sqlite3_uri_int64(F,P,D) routine converts the value of P into a
** 64-bit signed integer and returns that integer, or D if P does not
** exist.  If the value of P is something other than an integer, then
** zero is returned.
**
** The sqlite3_uri_key(F,N) returns a pointer to the name (not
** the value) of the N-th query parameter for filename F, or a NULL
** pointer if N is less than zero or greater than the number of query
** parameters minus 1.  The N value is zero-based so N should be 0 to obtain
** the name of the first query parameter, 1 for the second parameter, and
** so forth.
**
** If F is a NULL pointer, then sqlite3_uri_parameter(F,P) returns NULL and
** sqlite3_uri_boolean(F,P,B) returns B.  If F is not a NULL pointer and
** is not a database file pathname pointer that the SQLite core passed
** into the xOpen VFS method, then the behavior of this routine is undefined
** and probably undesirable.
**
** Beginning with SQLite [version 3.31.0] ([dateof:3.31.0]) the input F
** parameter can also be the name of a rollback journal file or WAL file
** in addition to the main database file.  Prior to version 3.31.0, these
** routines would only work if F was the name of the main database file.
** When the F parameter is the name of the rollback journal or WAL file,
** it has access to all the same query parameters as were found on the
** main database file.
**
** See the [URI filename] documentation for additional information.
*/
SQLITE_API const char *sqlite3_uri_parameter(sqlite3_filename z, const char *zParam);
SQLITE_API int sqlite3_uri_boolean(sqlite3_filename z, const char *zParam, int bDefault);
SQLITE_API sqlite3_int64 sqlite3_uri_int64(sqlite3_filename, const char*, sqlite3_int64);
SQLITE_API const char *sqlite3_uri_key(sqlite3_filename z, int N);

/*
** CAPI3REF:  Translate filenames
**
** These routines are available to [VFS|custom VFS implementations] for
** translating filenames between the main database file, the journal file,
** and the WAL file.
**
** If F is the name of an sqlite database file, journal file, or WAL file
** passed by the SQLite core into the VFS, then sqlite3_filename_database(F)
** returns the name of the corresponding database file.
**
** If F is the name of an sqlite database file, journal file, or WAL file
** passed by the SQLite core into the VFS, or if F is a database filename
** obtained from [sqlite3_db_filename()], then sqlite3_filename_journal(F)
** returns the name of the corresponding rollback journal file.
**
** If F is the name of an sqlite database file, journal file, or WAL file
** that was passed by the SQLite core into the VFS, or if F is a database
** filename obtained from [sqlite3_db_filename()], then
** sqlite3_filename_wal(F) returns the name of the corresponding
** WAL file.
**
** In all of the above, if F is not the name of a database, journal or WAL
** filename passed into the VFS from the SQLite core and F is not the
** return value from [sqlite3_db_filename()], then the result is
** undefined and is likely a memory access violation.
*/
SQLITE_API const char *sqlite3_filename_database(sqlite3_filename);
SQLITE_API const char *sqlite3_filename_journal(sqlite3_filename);
SQLITE_API const char *sqlite3_filename_wal(sqlite3_filename);

/*
** CAPI3REF:  Database File Corresponding To A Journal
**
** ^If X is the name of a rollback or WAL-mode journal file that is
** passed into the xOpen method of [sqlite3_vfs], then
** sqlite3_database_file_object(X) returns a pointer to the [sqlite3_file]
** object that represents the main database file.
**
** This routine is intended for use in custom [VFS] implementations
** only.  It is not a general-purpose interface.
** The argument sqlite3_file_object(X) must be a filename pointer that
** has been passed into [sqlite3_vfs].xOpen method where the
** flags parameter to xOpen contains one of the bits
** [SQLITE_OPEN_MAIN_JOURNAL] or [SQLITE_OPEN_WAL].  Any other use
** of this routine results in undefined and probably undesirable
** behavior.
*/
SQLITE_API sqlite3_file *sqlite3_database_file_object(const char*);

/*
** CAPI3REF: Create and Destroy VFS Filenames
**
** These interfaces are provided for use by [VFS shim] implementations and
** are not useful outside of that context.
**
** The sqlite3_create_filename(D,J,W,N,P) allocates memory to hold a version of
** database filename D with corresponding journal file J and WAL file W and
** with N URI parameters key/values pairs in the array P.  The result from
** sqlite3_create_filename(D,J,W,N,P) is a pointer to a database filename that
** is safe to pass to routines like:
** <ul>
** <li> [sqlite3_uri_parameter()],
** <li> [sqlite3_uri_boolean()],
** <li> [sqlite3_uri_int64()],
** <li> [sqlite3_uri_key()],
** <li> [sqlite3_filename_database()],
** <li> [sqlite3_filename_journal()], or
** <li> [sqlite3_filename_wal()].
** </ul>
** If a memory allocation error occurs, sqlite3_create_filename() might
** return a NULL pointer.  The memory obtained from sqlite3_create_filename(X)
** must be released by a corresponding call to sqlite3_free_filename(Y).
**
** The P parameter in sqlite3_create_filename(D,J,W,N,P) should be an array
** of 2*N pointers to strings.  Each pair of pointers in this array corresponds
** to a key and value for a query parameter.  The P parameter may be a NULL
** pointer if N is zero.  None of the 2*N pointers in the P array may be
** NULL pointers and key pointers should not be empty strings.
** None of the D, J, or W parameters to sqlite3_create_filename(D,J,W,N,P) may
** be NULL pointers, though they can be empty strings.
**
** The sqlite3_free_filename(Y) routine releases a memory allocation
** previously obtained from sqlite3_create_filename().  Invoking
** sqlite3_free_filename(Y) where Y is a NULL pointer is a harmless no-op.
**
** If the Y parameter to sqlite3_free_filename(Y) is anything other
** than a NULL pointer or a pointer previously acquired from
** sqlite3_create_filename(), then bad things such as heap
** corruption or segfaults may occur. The value Y should not be
** used again after sqlite3_free_filename(Y) has been called.  This means
** that if the [sqlite3_vfs.xOpen()] method of a VFS has been called using Y,
** then the corresponding [sqlite3_module.xClose() method should also be
** invoked prior to calling sqlite3_free_filename(Y).
*/
SQLITE_API sqlite3_filename sqlite3_create_filename(
  const char *zDatabase,
  const char *zJournal,
  const char *zWal,
  int nParam,
  const char **azParam
);
SQLITE_API void sqlite3_free_filename(sqlite3_filename);

/*
** CAPI3REF: Error Codes And Messages
** METHOD: sqlite3
**
** ^If the most recent sqlite3_* API call associated with
** [database connection] D failed, then the sqlite3_errcode(D) interface
** returns the numeric [result code] or [extended result code] for that
