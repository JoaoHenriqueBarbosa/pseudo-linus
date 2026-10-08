
/*
** CAPI3REF: Record A Database Snapshot
** CONSTRUCTOR: sqlite3_snapshot
**
** ^The [sqlite3_snapshot_get(D,S,P)] interface attempts to make a
** new [sqlite3_snapshot] object that records the current state of
** schema S in database connection D.  ^On success, the
** [sqlite3_snapshot_get(D,S,P)] interface writes a pointer to the newly
** created [sqlite3_snapshot] object into *P and returns SQLITE_OK.
** If there is not already a read-transaction open on schema S when
** this function is called, one is opened automatically.
**
** The following must be true for this function to succeed. If any of
** the following statements are false when sqlite3_snapshot_get() is
** called, SQLITE_ERROR is returned. The final value of *P is undefined
** in this case.
**
** <ul>
**   <li> The database handle must not be in [autocommit mode].
**
**   <li> Schema S of [database connection] D must be a [WAL mode] database.
**
**   <li> There must not be a write transaction open on schema S of database
**        connection D.
**
**   <li> One or more transactions must have been written to the current wal
**        file since it was created on disk (by any connection). This means
**        that a snapshot cannot be taken on a wal mode database with no wal
**        file immediately after it is first opened. At least one transaction
**        must be written to it first.
** </ul>
**
** This function may also return SQLITE_NOMEM.  If it is called with the
** database handle in autocommit mode but fails for some other reason,
** whether or not a read transaction is opened on schema S is undefined.
**
** The [sqlite3_snapshot] object returned from a successful call to
** [sqlite3_snapshot_get()] must be freed using [sqlite3_snapshot_free()]
** to avoid a memory leak.
**
** The [sqlite3_snapshot_get()] interface is only available when the
** [SQLITE_ENABLE_SNAPSHOT] compile-time option is used.
*/
SQLITE_API SQLITE_EXPERIMENTAL int sqlite3_snapshot_get(
  sqlite3 *db,
  const char *zSchema,
  sqlite3_snapshot **ppSnapshot
);

/*
** CAPI3REF: Start a read transaction on an historical snapshot
** METHOD: sqlite3_snapshot
**
** ^The [sqlite3_snapshot_open(D,S,P)] interface either starts a new read
** transaction or upgrades an existing one for schema S of
** [database connection] D such that the read transaction refers to
** historical [snapshot] P, rather than the most recent change to the
** database. ^The [sqlite3_snapshot_open()] interface returns SQLITE_OK
** on success or an appropriate [error code] if it fails.
**
** ^In order to succeed, the database connection must not be in
** [autocommit mode] when [sqlite3_snapshot_open(D,S,P)] is called. If there
** is already a read transaction open on schema S, then the database handle
** must have no active statements (SELECT statements that have been passed
** to sqlite3_step() but not sqlite3_reset() or sqlite3_finalize()).
** SQLITE_ERROR is returned if either of these conditions is violated, or
** if schema S does not exist, or if the snapshot object is invalid.
**
** ^A call to sqlite3_snapshot_open() will fail to open if the specified
** snapshot has been overwritten by a [checkpoint]. In this case
** SQLITE_ERROR_SNAPSHOT is returned.
**
** If there is already a read transaction open when this function is
** invoked, then the same read transaction remains open (on the same
** database snapshot) if SQLITE_ERROR, SQLITE_BUSY or SQLITE_ERROR_SNAPSHOT
** is returned. If another error code - for example SQLITE_PROTOCOL or an
** SQLITE_IOERR error code - is returned, then the final state of the
** read transaction is undefined. If SQLITE_OK is returned, then the
** read transaction is now open on database snapshot P.
**
** ^(A call to [sqlite3_snapshot_open(D,S,P)] will fail if the
** database connection D does not know that the database file for
** schema S is in [WAL mode].  A database connection might not know
** that the database file is in [WAL mode] if there has been no prior
** I/O on that database connection, or if the database entered [WAL mode]
** after the most recent I/O on the database connection.)^
** (Hint: Run "[PRAGMA application_id]" against a newly opened
** database connection in order to make it ready to use snapshots.)
**
** The [sqlite3_snapshot_open()] interface is only available when the
** [SQLITE_ENABLE_SNAPSHOT] compile-time option is used.
*/
SQLITE_API SQLITE_EXPERIMENTAL int sqlite3_snapshot_open(
  sqlite3 *db,
  const char *zSchema,
  sqlite3_snapshot *pSnapshot
);

/*
** CAPI3REF: Destroy a snapshot
** DESTRUCTOR: sqlite3_snapshot
**
** ^The [sqlite3_snapshot_free(P)] interface destroys [sqlite3_snapshot] P.
** The application must eventually free every [sqlite3_snapshot] object
** using this routine to avoid a memory leak.
**
** The [sqlite3_snapshot_free()] interface is only available when the
** [SQLITE_ENABLE_SNAPSHOT] compile-time option is used.
*/
SQLITE_API SQLITE_EXPERIMENTAL void sqlite3_snapshot_free(sqlite3_snapshot*);

/*
** CAPI3REF: Compare the ages of two snapshot handles.
** METHOD: sqlite3_snapshot
**
** The sqlite3_snapshot_cmp(P1, P2) interface is used to compare the ages
** of two valid snapshot handles.
**
** If the two snapshot handles are not associated with the same database
** file, the result of the comparison is undefined.
**
** Additionally, the result of the comparison is only valid if both of the
** snapshot handles were obtained by calling sqlite3_snapshot_get() since the
** last time the wal file was deleted. The wal file is deleted when the
** database is changed back to rollback mode or when the number of database
** clients drops to zero. If either snapshot handle was obtained before the
** wal file was last deleted, the value returned by this function
** is undefined.
**
** Otherwise, this API returns a negative value if P1 refers to an older
** snapshot than P2, zero if the two handles refer to the same database
** snapshot, and a positive value if P1 is a newer snapshot than P2.
**
** This interface is only available if SQLite is compiled with the
** [SQLITE_ENABLE_SNAPSHOT] option.
*/
SQLITE_API SQLITE_EXPERIMENTAL int sqlite3_snapshot_cmp(
  sqlite3_snapshot *p1,
  sqlite3_snapshot *p2
);

/*
** CAPI3REF: Recover snapshots from a wal file
** METHOD: sqlite3_snapshot
**
** If a [WAL file] remains on disk after all database connections close
** (either through the use of the [SQLITE_FCNTL_PERSIST_WAL] [file control]
** or because the last process to have the database opened exited without
** calling [sqlite3_close()]) and a new connection is subsequently opened
** on that database and [WAL file], the [sqlite3_snapshot_open()] interface
** will only be able to open the last transaction added to the WAL file
** even though the WAL file contains other valid transactions.
**
** This function attempts to scan the WAL file associated with database zDb
** of database handle db and make all valid snapshots available to
** sqlite3_snapshot_open(). It is an error if there is already a read
** transaction open on the database, or if the database is not a WAL mode
** database.
**
** SQLITE_OK is returned if successful, or an SQLite error code otherwise.
**
** This interface is only available if SQLite is compiled with the
** [SQLITE_ENABLE_SNAPSHOT] option.
*/
SQLITE_API SQLITE_EXPERIMENTAL int sqlite3_snapshot_recover(sqlite3 *db, const char *zDb);

/*
** CAPI3REF: Serialize a database
**
** The sqlite3_serialize(D,S,P,F) interface returns a pointer to memory
** that is a serialization of the S database on [database connection] D.
** If P is not a NULL pointer, then the size of the database in bytes
** is written into *P.
**
** For an ordinary on-disk database file, the serialization is just a
** copy of the disk file.  For an in-memory database or a "TEMP" database,
** the serialization is the same sequence of bytes which would be written
** to disk if that database where backed up to disk.
**
** The usual case is that sqlite3_serialize() copies the serialization of
** the database into memory obtained from [sqlite3_malloc64()] and returns
** a pointer to that memory.  The caller is responsible for freeing the
** returned value to avoid a memory leak.  However, if the F argument
** contains the SQLITE_SERIALIZE_NOCOPY bit, then no memory allocations
** are made, and the sqlite3_serialize() function will return a pointer
** to the contiguous memory representation of the database that SQLite
** is currently using for that database, or NULL if the no such contiguous
** memory representation of the database exists.  A contiguous memory
** representation of the database will usually only exist if there has
** been a prior call to [sqlite3_deserialize(D,S,...)] with the same
** values of D and S.
** The size of the database is written into *P even if the
** SQLITE_SERIALIZE_NOCOPY bit is set but no contiguous copy
** of the database exists.
**
** After the call, if the SQLITE_SERIALIZE_NOCOPY bit had been set,
** the returned buffer content will remain accessible and unchanged
** until either the next write operation on the connection or when
** the connection is closed, and applications must not modify the
** buffer. If the bit had been clear, the returned buffer will not
** be accessed by SQLite after the call.
**
** A call to sqlite3_serialize(D,S,P,F) might return NULL even if the
** SQLITE_SERIALIZE_NOCOPY bit is omitted from argument F if a memory
** allocation error occurs.
**
** This interface is omitted if SQLite is compiled with the
** [SQLITE_OMIT_DESERIALIZE] option.
*/
SQLITE_API unsigned char *sqlite3_serialize(
  sqlite3 *db,           /* The database connection */
  const char *zSchema,   /* Which DB to serialize. ex: "main", "temp", ... */
  sqlite3_int64 *piSize, /* Write size of the DB here, if not NULL */
  unsigned int mFlags    /* Zero or more SQLITE_SERIALIZE_* flags */
);

/*
** CAPI3REF: Flags for sqlite3_serialize
**
** Zero or more of the following constants can be OR-ed together for
** the F argument to [sqlite3_serialize(D,S,P,F)].
**
** SQLITE_SERIALIZE_NOCOPY means that [sqlite3_serialize()] will return
** a pointer to contiguous in-memory database that it is currently using,
** without making a copy of the database.  If SQLite is not currently using
** a contiguous in-memory database, then this option causes
** [sqlite3_serialize()] to return a NULL pointer.  SQLite will only be
** using a contiguous in-memory database if it has been initialized by a
** prior call to [sqlite3_deserialize()].
*/
#define SQLITE_SERIALIZE_NOCOPY 0x001   /* Do no memory allocations */

/*
** CAPI3REF: Deserialize a database
**
** The sqlite3_deserialize(D,S,P,N,M,F) interface causes the
** [database connection] D to disconnect from database S and then
** reopen S as an in-memory database based on the serialization contained
** in P.  The serialized database P is N bytes in size.  M is the size of
** the buffer P, which might be larger than N.  If M is larger than N, and
** the SQLITE_DESERIALIZE_READONLY bit is not set in F, then SQLite is
** permitted to add content to the in-memory database as long as the total
** size does not exceed M bytes.
**
** If the SQLITE_DESERIALIZE_FREEONCLOSE bit is set in F, then SQLite will
** invoke sqlite3_free() on the serialization buffer when the database
** connection closes.  If the SQLITE_DESERIALIZE_RESIZEABLE bit is set, then
** SQLite will try to increase the buffer size using sqlite3_realloc64()
** if writes on the database cause it to grow larger than M bytes.
**
** Applications must not modify the buffer P or invalidate it before
** the database connection D is closed.
**
** The sqlite3_deserialize() interface will fail with SQLITE_BUSY if the
** database is currently in a read transaction or is involved in a backup
** operation.
**
** It is not possible to deserialized into the TEMP database.  If the
** S argument to sqlite3_deserialize(D,S,P,N,M,F) is "temp" then the
** function returns SQLITE_ERROR.
**
** The deserialized database should not be in [WAL mode].  If the database
** is in WAL mode, then any attempt to use the database file will result
** in an [SQLITE_CANTOPEN] error.  The application can set the
** [file format version numbers] (bytes 18 and 19) of the input database P
** to 0x01 prior to invoking sqlite3_deserialize(D,S,P,N,M,F) to force the
** database file into rollback mode and work around this limitation.
**
** If sqlite3_deserialize(D,S,P,N,M,F) fails for any reason and if the
** SQLITE_DESERIALIZE_FREEONCLOSE bit is set in argument F, then
** [sqlite3_free()] is invoked on argument P prior to returning.
**
** This interface is omitted if SQLite is compiled with the
** [SQLITE_OMIT_DESERIALIZE] option.
*/
SQLITE_API int sqlite3_deserialize(
  sqlite3 *db,            /* The database connection */
  const char *zSchema,    /* Which DB to reopen with the deserialization */
  unsigned char *pData,   /* The serialized database content */
  sqlite3_int64 szDb,     /* Number bytes in the deserialization */
  sqlite3_int64 szBuf,    /* Total size of buffer pData[] */
  unsigned mFlags         /* Zero or more SQLITE_DESERIALIZE_* flags */
);

/*
** CAPI3REF: Flags for sqlite3_deserialize()
**
** The following are allowed values for 6th argument (the F argument) to
** the [sqlite3_deserialize(D,S,P,N,M,F)] interface.
**
** The SQLITE_DESERIALIZE_FREEONCLOSE means that the database serialization
** in the P argument is held in memory obtained from [sqlite3_malloc64()]
** and that SQLite should take ownership of this memory and automatically
** free it when it has finished using it.  Without this flag, the caller
** is responsible for freeing any dynamically allocated memory.
**
** The SQLITE_DESERIALIZE_RESIZEABLE flag means that SQLite is allowed to
** grow the size of the database using calls to [sqlite3_realloc64()].  This
** flag should only be used if SQLITE_DESERIALIZE_FREEONCLOSE is also used.
** Without this flag, the deserialized database cannot increase in size beyond
** the number of bytes specified by the M parameter.
**
** The SQLITE_DESERIALIZE_READONLY flag means that the deserialized database
** should be treated as read-only.
*/
#define SQLITE_DESERIALIZE_FREEONCLOSE 1 /* Call sqlite3_free() on close */
#define SQLITE_DESERIALIZE_RESIZEABLE  2 /* Resize using sqlite3_realloc64() */
#define SQLITE_DESERIALIZE_READONLY    4 /* Database is read-only */

/*
** Undo the hack that converts floating point types to integer for
** builds on processors without floating point support.
*/
#ifdef SQLITE_OMIT_FLOATING_POINT
# undef double
#endif

#if defined(__wasi__)
# undef SQLITE_WASI
# define SQLITE_WASI 1
# undef SQLITE_OMIT_WAL
# define SQLITE_OMIT_WAL 1/* because it requires shared memory APIs */
# ifndef SQLITE_OMIT_LOAD_EXTENSION
#  define SQLITE_OMIT_LOAD_EXTENSION
# endif
# ifndef SQLITE_THREADSAFE
#  define SQLITE_THREADSAFE 0
# endif
#endif

#if 0
}  /* End of the 'extern "C"' block */
#endif
#endif /* SQLITE3_H */


