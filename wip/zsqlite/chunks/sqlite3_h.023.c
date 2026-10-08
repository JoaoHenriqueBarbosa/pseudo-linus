**
** ^The [sqlite3_str_length(X)] method returns the current length, in bytes,
** of the dynamic string under construction in [sqlite3_str] object X.
** ^The length returned by [sqlite3_str_length(X)] does not include the
** zero-termination byte.
**
** ^The [sqlite3_str_value(X)] method returns a pointer to the current
** content of the dynamic string under construction in X.  The value
** returned by [sqlite3_str_value(X)] is managed by the sqlite3_str object X
** and might be freed or altered by any subsequent method on the same
** [sqlite3_str] object.  Applications must not used the pointer returned
** [sqlite3_str_value(X)] after any subsequent method call on the same
** object.  ^Applications may change the content of the string returned
** by [sqlite3_str_value(X)] as long as they do not write into any bytes
** outside the range of 0 to [sqlite3_str_length(X)] and do not read or
** write any byte after any subsequent sqlite3_str method call.
*/
SQLITE_API int sqlite3_str_errcode(sqlite3_str*);
SQLITE_API int sqlite3_str_length(sqlite3_str*);
SQLITE_API char *sqlite3_str_value(sqlite3_str*);

/*
** CAPI3REF: SQLite Runtime Status
**
** ^These interfaces are used to retrieve runtime status information
** about the performance of SQLite, and optionally to reset various
** highwater marks.  ^The first argument is an integer code for
** the specific parameter to measure.  ^(Recognized integer codes
** are of the form [status parameters | SQLITE_STATUS_...].)^
** ^The current value of the parameter is returned into *pCurrent.
** ^The highest recorded value is returned in *pHighwater.  ^If the
** resetFlag is true, then the highest record value is reset after
** *pHighwater is written.  ^(Some parameters do not record the highest
** value.  For those parameters
** nothing is written into *pHighwater and the resetFlag is ignored.)^
** ^(Other parameters record only the highwater mark and not the current
** value.  For these latter parameters nothing is written into *pCurrent.)^
**
** ^The sqlite3_status() and sqlite3_status64() routines return
** SQLITE_OK on success and a non-zero [error code] on failure.
**
** If either the current value or the highwater mark is too large to
** be represented by a 32-bit integer, then the values returned by
** sqlite3_status() are undefined.
**
** See also: [sqlite3_db_status()]
*/
SQLITE_API int sqlite3_status(int op, int *pCurrent, int *pHighwater, int resetFlag);
SQLITE_API int sqlite3_status64(
  int op,
  sqlite3_int64 *pCurrent,
  sqlite3_int64 *pHighwater,
  int resetFlag
);


/*
** CAPI3REF: Status Parameters
** KEYWORDS: {status parameters}
**
** These integer constants designate various run-time status parameters
** that can be returned by [sqlite3_status()].
**
** <dl>
** [[SQLITE_STATUS_MEMORY_USED]] ^(<dt>SQLITE_STATUS_MEMORY_USED</dt>
** <dd>This parameter is the current amount of memory checked out
** using [sqlite3_malloc()], either directly or indirectly.  The
** figure includes calls made to [sqlite3_malloc()] by the application
** and internal memory usage by the SQLite library.  Auxiliary page-cache
** memory controlled by [SQLITE_CONFIG_PAGECACHE] is not included in
** this parameter.  The amount returned is the sum of the allocation
** sizes as reported by the xSize method in [sqlite3_mem_methods].</dd>)^
**
** [[SQLITE_STATUS_MALLOC_SIZE]] ^(<dt>SQLITE_STATUS_MALLOC_SIZE</dt>
** <dd>This parameter records the largest memory allocation request
** handed to [sqlite3_malloc()] or [sqlite3_realloc()] (or their
** internal equivalents).  Only the value returned in the
** *pHighwater parameter to [sqlite3_status()] is of interest.
** The value written into the *pCurrent parameter is undefined.</dd>)^
**
** [[SQLITE_STATUS_MALLOC_COUNT]] ^(<dt>SQLITE_STATUS_MALLOC_COUNT</dt>
** <dd>This parameter records the number of separate memory allocations
** currently checked out.</dd>)^
**
** [[SQLITE_STATUS_PAGECACHE_USED]] ^(<dt>SQLITE_STATUS_PAGECACHE_USED</dt>
** <dd>This parameter returns the number of pages used out of the
** [pagecache memory allocator] that was configured using
** [SQLITE_CONFIG_PAGECACHE].  The
** value returned is in pages, not in bytes.</dd>)^
**
** [[SQLITE_STATUS_PAGECACHE_OVERFLOW]]
** ^(<dt>SQLITE_STATUS_PAGECACHE_OVERFLOW</dt>
** <dd>This parameter returns the number of bytes of page cache
** allocation which could not be satisfied by the [SQLITE_CONFIG_PAGECACHE]
** buffer and where forced to overflow to [sqlite3_malloc()].  The
** returned value includes allocations that overflowed because they
** where too large (they were larger than the "sz" parameter to
** [SQLITE_CONFIG_PAGECACHE]) and allocations that overflowed because
** no space was left in the page cache.</dd>)^
**
** [[SQLITE_STATUS_PAGECACHE_SIZE]] ^(<dt>SQLITE_STATUS_PAGECACHE_SIZE</dt>
** <dd>This parameter records the largest memory allocation request
** handed to the [pagecache memory allocator].  Only the value returned in the
** *pHighwater parameter to [sqlite3_status()] is of interest.
** The value written into the *pCurrent parameter is undefined.</dd>)^
**
** [[SQLITE_STATUS_SCRATCH_USED]] <dt>SQLITE_STATUS_SCRATCH_USED</dt>
** <dd>No longer used.</dd>
**
** [[SQLITE_STATUS_SCRATCH_OVERFLOW]] ^(<dt>SQLITE_STATUS_SCRATCH_OVERFLOW</dt>
** <dd>No longer used.</dd>
**
** [[SQLITE_STATUS_SCRATCH_SIZE]] <dt>SQLITE_STATUS_SCRATCH_SIZE</dt>
** <dd>No longer used.</dd>
**
** [[SQLITE_STATUS_PARSER_STACK]] ^(<dt>SQLITE_STATUS_PARSER_STACK</dt>
** <dd>The *pHighwater parameter records the deepest parser stack.
** The *pCurrent value is undefined.  The *pHighwater value is only
** meaningful if SQLite is compiled with [YYTRACKMAXSTACKDEPTH].</dd>)^
** </dl>
**
** New status parameters may be added from time to time.
*/
#define SQLITE_STATUS_MEMORY_USED          0
#define SQLITE_STATUS_PAGECACHE_USED       1
#define SQLITE_STATUS_PAGECACHE_OVERFLOW   2
#define SQLITE_STATUS_SCRATCH_USED         3  /* NOT USED */
#define SQLITE_STATUS_SCRATCH_OVERFLOW     4  /* NOT USED */
#define SQLITE_STATUS_MALLOC_SIZE          5
#define SQLITE_STATUS_PARSER_STACK         6
#define SQLITE_STATUS_PAGECACHE_SIZE       7
#define SQLITE_STATUS_SCRATCH_SIZE         8  /* NOT USED */
#define SQLITE_STATUS_MALLOC_COUNT         9

/*
** CAPI3REF: Database Connection Status
** METHOD: sqlite3
**
** ^This interface is used to retrieve runtime status information
** about a single [database connection].  ^The first argument is the
** database connection object to be interrogated.  ^The second argument
** is an integer constant, taken from the set of
** [SQLITE_DBSTATUS options], that
** determines the parameter to interrogate.  The set of
** [SQLITE_DBSTATUS options] is likely
** to grow in future releases of SQLite.
**
** ^The current value of the requested parameter is written into *pCur
** and the highest instantaneous value is written into *pHiwtr.  ^If
** the resetFlg is true, then the highest instantaneous value is
** reset back down to the current value.
**
** ^The sqlite3_db_status() routine returns SQLITE_OK on success and a
** non-zero [error code] on failure.
**
** See also: [sqlite3_status()] and [sqlite3_stmt_status()].
*/
SQLITE_API int sqlite3_db_status(sqlite3*, int op, int *pCur, int *pHiwtr, int resetFlg);

/*
** CAPI3REF: Status Parameters for database connections
** KEYWORDS: {SQLITE_DBSTATUS options}
**
** These constants are the available integer "verbs" that can be passed as
** the second argument to the [sqlite3_db_status()] interface.
**
** New verbs may be added in future releases of SQLite. Existing verbs
** might be discontinued. Applications should check the return code from
** [sqlite3_db_status()] to make sure that the call worked.
** The [sqlite3_db_status()] interface will return a non-zero error code
** if a discontinued or unsupported verb is invoked.
**
** <dl>
** [[SQLITE_DBSTATUS_LOOKASIDE_USED]] ^(<dt>SQLITE_DBSTATUS_LOOKASIDE_USED</dt>
** <dd>This parameter returns the number of lookaside memory slots currently
** checked out.</dd>)^
**
** [[SQLITE_DBSTATUS_LOOKASIDE_HIT]] ^(<dt>SQLITE_DBSTATUS_LOOKASIDE_HIT</dt>
** <dd>This parameter returns the number of malloc attempts that were
** satisfied using lookaside memory. Only the high-water value is meaningful;
** the current value is always zero.)^
**
** [[SQLITE_DBSTATUS_LOOKASIDE_MISS_SIZE]]
** ^(<dt>SQLITE_DBSTATUS_LOOKASIDE_MISS_SIZE</dt>
** <dd>This parameter returns the number malloc attempts that might have
** been satisfied using lookaside memory but failed due to the amount of
** memory requested being larger than the lookaside slot size.
** Only the high-water value is meaningful;
** the current value is always zero.)^
**
** [[SQLITE_DBSTATUS_LOOKASIDE_MISS_FULL]]
** ^(<dt>SQLITE_DBSTATUS_LOOKASIDE_MISS_FULL</dt>
** <dd>This parameter returns the number malloc attempts that might have
** been satisfied using lookaside memory but failed due to all lookaside
** memory already being in use.
** Only the high-water value is meaningful;
** the current value is always zero.)^
**
** [[SQLITE_DBSTATUS_CACHE_USED]] ^(<dt>SQLITE_DBSTATUS_CACHE_USED</dt>
** <dd>This parameter returns the approximate number of bytes of heap
** memory used by all pager caches associated with the database connection.)^
** ^The highwater mark associated with SQLITE_DBSTATUS_CACHE_USED is always 0.
**
** [[SQLITE_DBSTATUS_CACHE_USED_SHARED]]
** ^(<dt>SQLITE_DBSTATUS_CACHE_USED_SHARED</dt>
** <dd>This parameter is similar to DBSTATUS_CACHE_USED, except that if a
** pager cache is shared between two or more connections the bytes of heap
** memory used by that pager cache is divided evenly between the attached
** connections.)^  In other words, if none of the pager caches associated
** with the database connection are shared, this request returns the same
** value as DBSTATUS_CACHE_USED. Or, if one or more or the pager caches are
** shared, the value returned by this call will be smaller than that returned
** by DBSTATUS_CACHE_USED. ^The highwater mark associated with
** SQLITE_DBSTATUS_CACHE_USED_SHARED is always 0.
**
** [[SQLITE_DBSTATUS_SCHEMA_USED]] ^(<dt>SQLITE_DBSTATUS_SCHEMA_USED</dt>
** <dd>This parameter returns the approximate number of bytes of heap
** memory used to store the schema for all databases associated
** with the connection - main, temp, and any [ATTACH]-ed databases.)^
** ^The full amount of memory used by the schemas is reported, even if the
** schema memory is shared with other database connections due to
** [shared cache mode] being enabled.
** ^The highwater mark associated with SQLITE_DBSTATUS_SCHEMA_USED is always 0.
**
** [[SQLITE_DBSTATUS_STMT_USED]] ^(<dt>SQLITE_DBSTATUS_STMT_USED</dt>
** <dd>This parameter returns the approximate number of bytes of heap
** and lookaside memory used by all prepared statements associated with
** the database connection.)^
** ^The highwater mark associated with SQLITE_DBSTATUS_STMT_USED is always 0.
** </dd>
**
** [[SQLITE_DBSTATUS_CACHE_HIT]] ^(<dt>SQLITE_DBSTATUS_CACHE_HIT</dt>
** <dd>This parameter returns the number of pager cache hits that have
** occurred.)^ ^The highwater mark associated with SQLITE_DBSTATUS_CACHE_HIT
** is always 0.
** </dd>
**
** [[SQLITE_DBSTATUS_CACHE_MISS]] ^(<dt>SQLITE_DBSTATUS_CACHE_MISS</dt>
** <dd>This parameter returns the number of pager cache misses that have
** occurred.)^ ^The highwater mark associated with SQLITE_DBSTATUS_CACHE_MISS
** is always 0.
** </dd>
**
** [[SQLITE_DBSTATUS_CACHE_WRITE]] ^(<dt>SQLITE_DBSTATUS_CACHE_WRITE</dt>
** <dd>This parameter returns the number of dirty cache entries that have
** been written to disk. Specifically, the number of pages written to the
** wal file in wal mode databases, or the number of pages written to the
** database file in rollback mode databases. Any pages written as part of
** transaction rollback or database recovery operations are not included.
** If an IO or other error occurs while writing a page to disk, the effect
** on subsequent SQLITE_DBSTATUS_CACHE_WRITE requests is undefined.)^ ^The
** highwater mark associated with SQLITE_DBSTATUS_CACHE_WRITE is always 0.
** </dd>
**
** [[SQLITE_DBSTATUS_CACHE_SPILL]] ^(<dt>SQLITE_DBSTATUS_CACHE_SPILL</dt>
** <dd>This parameter returns the number of dirty cache entries that have
** been written to disk in the middle of a transaction due to the page
** cache overflowing. Transactions are more efficient if they are written
** to disk all at once. When pages spill mid-transaction, that introduces
** additional overhead. This parameter can be used help identify
** inefficiencies that can be resolved by increasing the cache size.
** </dd>
**
** [[SQLITE_DBSTATUS_DEFERRED_FKS]] ^(<dt>SQLITE_DBSTATUS_DEFERRED_FKS</dt>
** <dd>This parameter returns zero for the current value if and only if
** all foreign key constraints (deferred or immediate) have been
** resolved.)^  ^The highwater mark is always 0.
** </dd>
** </dl>
*/
#define SQLITE_DBSTATUS_LOOKASIDE_USED       0
#define SQLITE_DBSTATUS_CACHE_USED           1
#define SQLITE_DBSTATUS_SCHEMA_USED          2
#define SQLITE_DBSTATUS_STMT_USED            3
#define SQLITE_DBSTATUS_LOOKASIDE_HIT        4
#define SQLITE_DBSTATUS_LOOKASIDE_MISS_SIZE  5
#define SQLITE_DBSTATUS_LOOKASIDE_MISS_FULL  6
#define SQLITE_DBSTATUS_CACHE_HIT            7
#define SQLITE_DBSTATUS_CACHE_MISS           8
#define SQLITE_DBSTATUS_CACHE_WRITE          9
#define SQLITE_DBSTATUS_DEFERRED_FKS        10
#define SQLITE_DBSTATUS_CACHE_USED_SHARED   11
#define SQLITE_DBSTATUS_CACHE_SPILL         12
#define SQLITE_DBSTATUS_MAX                 12   /* Largest defined DBSTATUS */


/*
** CAPI3REF: Prepared Statement Status
** METHOD: sqlite3_stmt
**
** ^(Each prepared statement maintains various
** [SQLITE_STMTSTATUS counters] that measure the number
** of times it has performed specific operations.)^  These counters can
** be used to monitor the performance characteristics of the prepared
** statements.  For example, if the number of table steps greatly exceeds
** the number of table searches or result rows, that would tend to indicate
** that the prepared statement is using a full table scan rather than
** an index.
**
** ^(This interface is used to retrieve and reset counter values from
** a [prepared statement].  The first argument is the prepared statement
** object to be interrogated.  The second argument
** is an integer code for a specific [SQLITE_STMTSTATUS counter]
** to be interrogated.)^
** ^The current value of the requested counter is returned.
** ^If the resetFlg is true, then the counter is reset to zero after this
** interface call returns.
**
** See also: [sqlite3_status()] and [sqlite3_db_status()].
*/
SQLITE_API int sqlite3_stmt_status(sqlite3_stmt*, int op,int resetFlg);

/*
** CAPI3REF: Status Parameters for prepared statements
** KEYWORDS: {SQLITE_STMTSTATUS counter} {SQLITE_STMTSTATUS counters}
**
** These preprocessor macros define integer codes that name counter
** values associated with the [sqlite3_stmt_status()] interface.
** The meanings of the various counters are as follows:
**
** <dl>
** [[SQLITE_STMTSTATUS_FULLSCAN_STEP]] <dt>SQLITE_STMTSTATUS_FULLSCAN_STEP</dt>
** <dd>^This is the number of times that SQLite has stepped forward in
** a table as part of a full table scan.  Large numbers for this counter
** may indicate opportunities for performance improvement through
** careful use of indices.</dd>
**
** [[SQLITE_STMTSTATUS_SORT]] <dt>SQLITE_STMTSTATUS_SORT</dt>
** <dd>^This is the number of sort operations that have occurred.
** A non-zero value in this counter may indicate an opportunity to
** improvement performance through careful use of indices.</dd>
**
** [[SQLITE_STMTSTATUS_AUTOINDEX]] <dt>SQLITE_STMTSTATUS_AUTOINDEX</dt>
** <dd>^This is the number of rows inserted into transient indices that
** were created automatically in order to help joins run faster.
** A non-zero value in this counter may indicate an opportunity to
** improvement performance by adding permanent indices that do not
** need to be reinitialized each time the statement is run.</dd>
**
** [[SQLITE_STMTSTATUS_VM_STEP]] <dt>SQLITE_STMTSTATUS_VM_STEP</dt>
** <dd>^This is the number of virtual machine operations executed
** by the prepared statement if that number is less than or equal
** to 2147483647.  The number of virtual machine operations can be
** used as a proxy for the total work done by the prepared statement.
** If the number of virtual machine operations exceeds 2147483647
** then the value returned by this statement status code is undefined.
**
** [[SQLITE_STMTSTATUS_REPREPARE]] <dt>SQLITE_STMTSTATUS_REPREPARE</dt>
** <dd>^This is the number of times that the prepare statement has been
** automatically regenerated due to schema changes or changes to
** [bound parameters] that might affect the query plan.
**
** [[SQLITE_STMTSTATUS_RUN]] <dt>SQLITE_STMTSTATUS_RUN</dt>
** <dd>^This is the number of times that the prepared statement has
** been run.  A single "run" for the purposes of this counter is one
** or more calls to [sqlite3_step()] followed by a call to [sqlite3_reset()].
** The counter is incremented on the first [sqlite3_step()] call of each
** cycle.
**
** [[SQLITE_STMTSTATUS_FILTER_MISS]]
** [[SQLITE_STMTSTATUS_FILTER HIT]]
** <dt>SQLITE_STMTSTATUS_FILTER_HIT<br>
** SQLITE_STMTSTATUS_FILTER_MISS</dt>
** <dd>^SQLITE_STMTSTATUS_FILTER_HIT is the number of times that a join
** step was bypassed because a Bloom filter returned not-found.  The
** corresponding SQLITE_STMTSTATUS_FILTER_MISS value is the number of
** times that the Bloom filter returned a find, and thus the join step
** had to be processed as normal.
**
** [[SQLITE_STMTSTATUS_MEMUSED]] <dt>SQLITE_STMTSTATUS_MEMUSED</dt>
** <dd>^This is the approximate number of bytes of heap memory
** used to store the prepared statement.  ^This value is not actually
** a counter, and so the resetFlg parameter to sqlite3_stmt_status()
** is ignored when the opcode is SQLITE_STMTSTATUS_MEMUSED.
** </dd>
** </dl>
*/
#define SQLITE_STMTSTATUS_FULLSCAN_STEP     1
#define SQLITE_STMTSTATUS_SORT              2
#define SQLITE_STMTSTATUS_AUTOINDEX         3
#define SQLITE_STMTSTATUS_VM_STEP           4
#define SQLITE_STMTSTATUS_REPREPARE         5
#define SQLITE_STMTSTATUS_RUN               6
#define SQLITE_STMTSTATUS_FILTER_MISS       7
#define SQLITE_STMTSTATUS_FILTER_HIT        8
#define SQLITE_STMTSTATUS_MEMUSED           99

/*
** CAPI3REF: Custom Page Cache Object
**
** The sqlite3_pcache type is opaque.  It is implemented by
** the pluggable module.  The SQLite core has no knowledge of
** its size or internal structure and never deals with the
** sqlite3_pcache object except by holding and passing pointers
** to the object.
**
** See [sqlite3_pcache_methods2] for additional information.
*/
typedef struct sqlite3_pcache sqlite3_pcache;

/*
** CAPI3REF: Custom Page Cache Object
**
** The sqlite3_pcache_page object represents a single page in the
** page cache.  The page cache will allocate instances of this
** object.  Various methods of the page cache use pointers to instances
** of this object as parameters or as their return value.
**
** See [sqlite3_pcache_methods2] for additional information.
*/
typedef struct sqlite3_pcache_page sqlite3_pcache_page;
struct sqlite3_pcache_page {
  void *pBuf;        /* The content of the page */
  void *pExtra;      /* Extra information associated with the page */
};
