**
** ^These functions (collectively known as "function creation routines")
** are used to add SQL functions or aggregates or to redefine the behavior
** of existing SQL functions or aggregates. The only differences between
** the three "sqlite3_create_function*" routines are the text encoding
** expected for the second parameter (the name of the function being
** created) and the presence or absence of a destructor callback for
** the application data pointer. Function sqlite3_create_window_function()
** is similar, but allows the user to supply the extra callback functions
** needed by [aggregate window functions].
**
** ^The first parameter is the [database connection] to which the SQL
** function is to be added.  ^If an application uses more than one database
** connection then application-defined SQL functions must be added
** to each database connection separately.
**
** ^The second parameter is the name of the SQL function to be created or
** redefined.  ^The length of the name is limited to 255 bytes in a UTF-8
** representation, exclusive of the zero-terminator.  ^Note that the name
** length limit is in UTF-8 bytes, not characters nor UTF-16 bytes.
** ^Any attempt to create a function with a longer name
** will result in [SQLITE_MISUSE] being returned.
**
** ^The third parameter (nArg)
** is the number of arguments that the SQL function or
** aggregate takes. ^If this parameter is -1, then the SQL function or
** aggregate may take any number of arguments between 0 and the limit
** set by [sqlite3_limit]([SQLITE_LIMIT_FUNCTION_ARG]).  If the third
** parameter is less than -1 or greater than 127 then the behavior is
** undefined.
**
** ^The fourth parameter, eTextRep, specifies what
** [SQLITE_UTF8 | text encoding] this SQL function prefers for
** its parameters.  The application should set this parameter to
** [SQLITE_UTF16LE] if the function implementation invokes
** [sqlite3_value_text16le()] on an input, or [SQLITE_UTF16BE] if the
** implementation invokes [sqlite3_value_text16be()] on an input, or
** [SQLITE_UTF16] if [sqlite3_value_text16()] is used, or [SQLITE_UTF8]
** otherwise.  ^The same SQL function may be registered multiple times using
** different preferred text encodings, with different implementations for
** each encoding.
** ^When multiple implementations of the same function are available, SQLite
** will pick the one that involves the least amount of data conversion.
**
** ^The fourth parameter may optionally be ORed with [SQLITE_DETERMINISTIC]
** to signal that the function will always return the same result given
** the same inputs within a single SQL statement.  Most SQL functions are
** deterministic.  The built-in [random()] SQL function is an example of a
** function that is not deterministic.  The SQLite query planner is able to
** perform additional optimizations on deterministic functions, so use
** of the [SQLITE_DETERMINISTIC] flag is recommended where possible.
**
** ^The fourth parameter may also optionally include the [SQLITE_DIRECTONLY]
** flag, which if present prevents the function from being invoked from
** within VIEWs, TRIGGERs, CHECK constraints, generated column expressions,
** index expressions, or the WHERE clause of partial indexes.
**
** For best security, the [SQLITE_DIRECTONLY] flag is recommended for
** all application-defined SQL functions that do not need to be
** used inside of triggers, view, CHECK constraints, or other elements of
** the database schema.  This flags is especially recommended for SQL
** functions that have side effects or reveal internal application state.
** Without this flag, an attacker might be able to modify the schema of
** a database file to include invocations of the function with parameters
** chosen by the attacker, which the application will then execute when
** the database file is opened and read.
**
** ^(The fifth parameter is an arbitrary pointer.  The implementation of the
** function can gain access to this pointer using [sqlite3_user_data()].)^
**
** ^The sixth, seventh and eighth parameters passed to the three
** "sqlite3_create_function*" functions, xFunc, xStep and xFinal, are
** pointers to C-language functions that implement the SQL function or
** aggregate. ^A scalar SQL function requires an implementation of the xFunc
** callback only; NULL pointers must be passed as the xStep and xFinal
** parameters. ^An aggregate SQL function requires an implementation of xStep
** and xFinal and NULL pointer must be passed for xFunc. ^To delete an existing
** SQL function or aggregate, pass NULL pointers for all three function
** callbacks.
**
** ^The sixth, seventh, eighth and ninth parameters (xStep, xFinal, xValue
** and xInverse) passed to sqlite3_create_window_function are pointers to
** C-language callbacks that implement the new function. xStep and xFinal
** must both be non-NULL. xValue and xInverse may either both be NULL, in
** which case a regular aggregate function is created, or must both be
** non-NULL, in which case the new function may be used as either an aggregate
** or aggregate window function. More details regarding the implementation
** of aggregate window functions are
** [user-defined window functions|available here].
**
** ^(If the final parameter to sqlite3_create_function_v2() or
** sqlite3_create_window_function() is not NULL, then it is destructor for
** the application data pointer. The destructor is invoked when the function
** is deleted, either by being overloaded or when the database connection
** closes.)^ ^The destructor is also invoked if the call to
** sqlite3_create_function_v2() fails.  ^When the destructor callback is
** invoked, it is passed a single argument which is a copy of the application
** data pointer which was the fifth parameter to sqlite3_create_function_v2().
**
** ^It is permitted to register multiple implementations of the same
** functions with the same name but with either differing numbers of
** arguments or differing preferred text encodings.  ^SQLite will use
** the implementation that most closely matches the way in which the
** SQL function is used.  ^A function implementation with a non-negative
** nArg parameter is a better match than a function implementation with
** a negative nArg.  ^A function where the preferred text encoding
** matches the database encoding is a better
** match than a function where the encoding is different.
** ^A function where the encoding difference is between UTF16le and UTF16be
** is a closer match than a function where the encoding difference is
** between UTF8 and UTF16.
**
** ^Built-in functions may be overloaded by new application-defined functions.
**
** ^An application-defined function is permitted to call other
** SQLite interfaces.  However, such calls must not
** close the database connection nor finalize or reset the prepared
** statement in which the function is running.
*/
SQLITE_API int sqlite3_create_function(
  sqlite3 *db,
  const char *zFunctionName,
  int nArg,
  int eTextRep,
  void *pApp,
  void (*xFunc)(sqlite3_context*,int,sqlite3_value**),
  void (*xStep)(sqlite3_context*,int,sqlite3_value**),
  void (*xFinal)(sqlite3_context*)
);
SQLITE_API int sqlite3_create_function16(
  sqlite3 *db,
  const void *zFunctionName,
  int nArg,
  int eTextRep,
  void *pApp,
  void (*xFunc)(sqlite3_context*,int,sqlite3_value**),
  void (*xStep)(sqlite3_context*,int,sqlite3_value**),
  void (*xFinal)(sqlite3_context*)
);
SQLITE_API int sqlite3_create_function_v2(
  sqlite3 *db,
  const char *zFunctionName,
  int nArg,
  int eTextRep,
  void *pApp,
  void (*xFunc)(sqlite3_context*,int,sqlite3_value**),
  void (*xStep)(sqlite3_context*,int,sqlite3_value**),
  void (*xFinal)(sqlite3_context*),
  void(*xDestroy)(void*)
);
SQLITE_API int sqlite3_create_window_function(
  sqlite3 *db,
  const char *zFunctionName,
  int nArg,
  int eTextRep,
  void *pApp,
  void (*xStep)(sqlite3_context*,int,sqlite3_value**),
  void (*xFinal)(sqlite3_context*),
  void (*xValue)(sqlite3_context*),
  void (*xInverse)(sqlite3_context*,int,sqlite3_value**),
  void(*xDestroy)(void*)
);

/*
** CAPI3REF: Text Encodings
**
** These constant define integer codes that represent the various
** text encodings supported by SQLite.
*/
#define SQLITE_UTF8           1    /* IMP: R-37514-35566 */
#define SQLITE_UTF16LE        2    /* IMP: R-03371-37637 */
#define SQLITE_UTF16BE        3    /* IMP: R-51971-34154 */
#define SQLITE_UTF16          4    /* Use native byte order */
#define SQLITE_ANY            5    /* Deprecated */
#define SQLITE_UTF16_ALIGNED  8    /* sqlite3_create_collation only */

/*
** CAPI3REF: Function Flags
**
** These constants may be ORed together with the
** [SQLITE_UTF8 | preferred text encoding] as the fourth argument
** to [sqlite3_create_function()], [sqlite3_create_function16()], or
** [sqlite3_create_function_v2()].
**
** <dl>
** [[SQLITE_DETERMINISTIC]] <dt>SQLITE_DETERMINISTIC</dt><dd>
** The SQLITE_DETERMINISTIC flag means that the new function always gives
** the same output when the input parameters are the same.
** The [abs|abs() function] is deterministic, for example, but
** [randomblob|randomblob()] is not.  Functions must
** be deterministic in order to be used in certain contexts such as
** with the WHERE clause of [partial indexes] or in [generated columns].
** SQLite might also optimize deterministic functions by factoring them
** out of inner loops.
** </dd>
**
** [[SQLITE_DIRECTONLY]] <dt>SQLITE_DIRECTONLY</dt><dd>
** The SQLITE_DIRECTONLY flag means that the function may only be invoked
** from top-level SQL, and cannot be used in VIEWs or TRIGGERs nor in
** schema structures such as [CHECK constraints], [DEFAULT clauses],
** [expression indexes], [partial indexes], or [generated columns].
** <p>
** The SQLITE_DIRECTONLY flag is recommended for any
** [application-defined SQL function]
** that has side-effects or that could potentially leak sensitive information.
** This will prevent attacks in which an application is tricked
** into using a database file that has had its schema surreptitiously
** modified to invoke the application-defined function in ways that are
** harmful.
** <p>
** Some people say it is good practice to set SQLITE_DIRECTONLY on all
** [application-defined SQL functions], regardless of whether or not they
** are security sensitive, as doing so prevents those functions from being used
** inside of the database schema, and thus ensures that the database
** can be inspected and modified using generic tools (such as the [CLI])
** that do not have access to the application-defined functions.
** </dd>
**
** [[SQLITE_INNOCUOUS]] <dt>SQLITE_INNOCUOUS</dt><dd>
** The SQLITE_INNOCUOUS flag means that the function is unlikely
** to cause problems even if misused.  An innocuous function should have
** no side effects and should not depend on any values other than its
** input parameters. The [abs|abs() function] is an example of an
** innocuous function.
** The [load_extension() SQL function] is not innocuous because of its
** side effects.
** <p> SQLITE_INNOCUOUS is similar to SQLITE_DETERMINISTIC, but is not
** exactly the same.  The [random|random() function] is an example of a
** function that is innocuous but not deterministic.
** <p>Some heightened security settings
** ([SQLITE_DBCONFIG_TRUSTED_SCHEMA] and [PRAGMA trusted_schema=OFF])
** disable the use of SQL functions inside views and triggers and in
** schema structures such as [CHECK constraints], [DEFAULT clauses],
** [expression indexes], [partial indexes], and [generated columns] unless
** the function is tagged with SQLITE_INNOCUOUS.  Most built-in functions
** are innocuous.  Developers are advised to avoid using the
** SQLITE_INNOCUOUS flag for application-defined functions unless the
** function has been carefully audited and found to be free of potentially
** security-adverse side-effects and information-leaks.
** </dd>
**
** [[SQLITE_SUBTYPE]] <dt>SQLITE_SUBTYPE</dt><dd>
** The SQLITE_SUBTYPE flag indicates to SQLite that a function might call
** [sqlite3_value_subtype()] to inspect the sub-types of its arguments.
** This flag instructs SQLite to omit some corner-case optimizations that
** might disrupt the operation of the [sqlite3_value_subtype()] function,
** causing it to return zero rather than the correct subtype().
** SQL functions that invokes [sqlite3_value_subtype()] should have this
** property.  If the SQLITE_SUBTYPE property is omitted, then the return
** value from [sqlite3_value_subtype()] might sometimes be zero even though
** a non-zero subtype was specified by the function argument expression.
**
** [[SQLITE_RESULT_SUBTYPE]] <dt>SQLITE_RESULT_SUBTYPE</dt><dd>
** The SQLITE_RESULT_SUBTYPE flag indicates to SQLite that a function might call
** [sqlite3_result_subtype()] to cause a sub-type to be associated with its
** result.
** Every function that invokes [sqlite3_result_subtype()] should have this
** property.  If it does not, then the call to [sqlite3_result_subtype()]
** might become a no-op if the function is used as term in an
** [expression index].  On the other hand, SQL functions that never invoke
** [sqlite3_result_subtype()] should avoid setting this property, as the
** purpose of this property is to disable certain optimizations that are
** incompatible with subtypes.
** </dd>
** </dl>
*/
#define SQLITE_DETERMINISTIC    0x000000800
#define SQLITE_DIRECTONLY       0x000080000
#define SQLITE_SUBTYPE          0x000100000
#define SQLITE_INNOCUOUS        0x000200000
#define SQLITE_RESULT_SUBTYPE   0x001000000

/*
** CAPI3REF: Deprecated Functions
** DEPRECATED
**
** These functions are [deprecated].  In order to maintain
** backwards compatibility with older code, these functions continue
** to be supported.  However, new applications should avoid
** the use of these functions.  To encourage programmers to avoid
** these functions, we will not explain what they do.
*/
#ifndef SQLITE_OMIT_DEPRECATED
SQLITE_API SQLITE_DEPRECATED int sqlite3_aggregate_count(sqlite3_context*);
SQLITE_API SQLITE_DEPRECATED int sqlite3_expired(sqlite3_stmt*);
SQLITE_API SQLITE_DEPRECATED int sqlite3_transfer_bindings(sqlite3_stmt*, sqlite3_stmt*);
SQLITE_API SQLITE_DEPRECATED int sqlite3_global_recover(void);
SQLITE_API SQLITE_DEPRECATED void sqlite3_thread_cleanup(void);
SQLITE_API SQLITE_DEPRECATED int sqlite3_memory_alarm(void(*)(void*,sqlite3_int64,int),
                      void*,sqlite3_int64);
#endif

/*
** CAPI3REF: Obtaining SQL Values
** METHOD: sqlite3_value
**
** <b>Summary:</b>
** <blockquote><table border=0 cellpadding=0 cellspacing=0>
** <tr><td><b>sqlite3_value_blob</b><td>&rarr;<td>BLOB value
** <tr><td><b>sqlite3_value_double</b><td>&rarr;<td>REAL value
** <tr><td><b>sqlite3_value_int</b><td>&rarr;<td>32-bit INTEGER value
** <tr><td><b>sqlite3_value_int64</b><td>&rarr;<td>64-bit INTEGER value
** <tr><td><b>sqlite3_value_pointer</b><td>&rarr;<td>Pointer value
** <tr><td><b>sqlite3_value_text</b><td>&rarr;<td>UTF-8 TEXT value
** <tr><td><b>sqlite3_value_text16</b><td>&rarr;<td>UTF-16 TEXT value in
** the native byteorder
** <tr><td><b>sqlite3_value_text16be</b><td>&rarr;<td>UTF-16be TEXT value
** <tr><td><b>sqlite3_value_text16le</b><td>&rarr;<td>UTF-16le TEXT value
** <tr><td>&nbsp;<td>&nbsp;<td>&nbsp;
** <tr><td><b>sqlite3_value_bytes</b><td>&rarr;<td>Size of a BLOB
** or a UTF-8 TEXT in bytes
** <tr><td><b>sqlite3_value_bytes16&nbsp;&nbsp;</b>
** <td>&rarr;&nbsp;&nbsp;<td>Size of UTF-16
** TEXT in bytes
** <tr><td><b>sqlite3_value_type</b><td>&rarr;<td>Default
** datatype of the value
** <tr><td><b>sqlite3_value_numeric_type&nbsp;&nbsp;</b>
** <td>&rarr;&nbsp;&nbsp;<td>Best numeric datatype of the value
** <tr><td><b>sqlite3_value_nochange&nbsp;&nbsp;</b>
** <td>&rarr;&nbsp;&nbsp;<td>True if the column is unchanged in an UPDATE
** against a virtual table.
** <tr><td><b>sqlite3_value_frombind&nbsp;&nbsp;</b>
** <td>&rarr;&nbsp;&nbsp;<td>True if value originated from a [bound parameter]
** </table></blockquote>
**
** <b>Details:</b>
**
** These routines extract type, size, and content information from
** [protected sqlite3_value] objects.  Protected sqlite3_value objects
** are used to pass parameter information into the functions that
** implement [application-defined SQL functions] and [virtual tables].
**
** These routines work only with [protected sqlite3_value] objects.
** Any attempt to use these routines on an [unprotected sqlite3_value]
** is not threadsafe.
**
** ^These routines work just like the corresponding [column access functions]
** except that these routines take a single [protected sqlite3_value] object
** pointer instead of a [sqlite3_stmt*] pointer and an integer column number.
**
** ^The sqlite3_value_text16() interface extracts a UTF-16 string
** in the native byte-order of the host machine.  ^The
** sqlite3_value_text16be() and sqlite3_value_text16le() interfaces
** extract UTF-16 strings as big-endian and little-endian respectively.
**
** ^If [sqlite3_value] object V was initialized
** using [sqlite3_bind_pointer(S,I,P,X,D)] or [sqlite3_result_pointer(C,P,X,D)]
** and if X and Y are strings that compare equal according to strcmp(X,Y),
** then sqlite3_value_pointer(V,Y) will return the pointer P.  ^Otherwise,
** sqlite3_value_pointer(V,Y) returns a NULL. The sqlite3_bind_pointer()
