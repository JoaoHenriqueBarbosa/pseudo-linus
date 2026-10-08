** routine is part of the [pointer passing interface] added for SQLite 3.20.0.
**
** ^(The sqlite3_value_type(V) interface returns the
** [SQLITE_INTEGER | datatype code] for the initial datatype of the
** [sqlite3_value] object V. The returned value is one of [SQLITE_INTEGER],
** [SQLITE_FLOAT], [SQLITE_TEXT], [SQLITE_BLOB], or [SQLITE_NULL].)^
** Other interfaces might change the datatype for an sqlite3_value object.
** For example, if the datatype is initially SQLITE_INTEGER and
** sqlite3_value_text(V) is called to extract a text value for that
** integer, then subsequent calls to sqlite3_value_type(V) might return
** SQLITE_TEXT.  Whether or not a persistent internal datatype conversion
** occurs is undefined and may change from one release of SQLite to the next.
**
** ^(The sqlite3_value_numeric_type() interface attempts to apply
** numeric affinity to the value.  This means that an attempt is
** made to convert the value to an integer or floating point.  If
** such a conversion is possible without loss of information (in other
** words, if the value is a string that looks like a number)
** then the conversion is performed.  Otherwise no conversion occurs.
** The [SQLITE_INTEGER | datatype] after conversion is returned.)^
**
** ^Within the [xUpdate] method of a [virtual table], the
** sqlite3_value_nochange(X) interface returns true if and only if
** the column corresponding to X is unchanged by the UPDATE operation
** that the xUpdate method call was invoked to implement and if
** and the prior [xColumn] method call that was invoked to extracted
** the value for that column returned without setting a result (probably
** because it queried [sqlite3_vtab_nochange()] and found that the column
** was unchanging).  ^Within an [xUpdate] method, any value for which
** sqlite3_value_nochange(X) is true will in all other respects appear
** to be a NULL value.  If sqlite3_value_nochange(X) is invoked anywhere other
** than within an [xUpdate] method call for an UPDATE statement, then
** the return value is arbitrary and meaningless.
**
** ^The sqlite3_value_frombind(X) interface returns non-zero if the
** value X originated from one of the [sqlite3_bind_int|sqlite3_bind()]
** interfaces.  ^If X comes from an SQL literal value, or a table column,
** or an expression, then sqlite3_value_frombind(X) returns zero.
**
** Please pay particular attention to the fact that the pointer returned
** from [sqlite3_value_blob()], [sqlite3_value_text()], or
** [sqlite3_value_text16()] can be invalidated by a subsequent call to
** [sqlite3_value_bytes()], [sqlite3_value_bytes16()], [sqlite3_value_text()],
** or [sqlite3_value_text16()].
**
** These routines must be called from the same thread as
** the SQL function that supplied the [sqlite3_value*] parameters.
**
** As long as the input parameter is correct, these routines can only
** fail if an out-of-memory error occurs during a format conversion.
** Only the following subset of interfaces are subject to out-of-memory
** errors:
**
** <ul>
** <li> sqlite3_value_blob()
** <li> sqlite3_value_text()
** <li> sqlite3_value_text16()
** <li> sqlite3_value_text16le()
** <li> sqlite3_value_text16be()
** <li> sqlite3_value_bytes()
** <li> sqlite3_value_bytes16()
** </ul>
**
** If an out-of-memory error occurs, then the return value from these
** routines is the same as if the column had contained an SQL NULL value.
** Valid SQL NULL returns can be distinguished from out-of-memory errors
** by invoking the [sqlite3_errcode()] immediately after the suspect
** return value is obtained and before any
** other SQLite interface is called on the same [database connection].
*/
SQLITE_API const void *sqlite3_value_blob(sqlite3_value*);
SQLITE_API double sqlite3_value_double(sqlite3_value*);
SQLITE_API int sqlite3_value_int(sqlite3_value*);
SQLITE_API sqlite3_int64 sqlite3_value_int64(sqlite3_value*);
SQLITE_API void *sqlite3_value_pointer(sqlite3_value*, const char*);
SQLITE_API const unsigned char *sqlite3_value_text(sqlite3_value*);
SQLITE_API const void *sqlite3_value_text16(sqlite3_value*);
SQLITE_API const void *sqlite3_value_text16le(sqlite3_value*);
SQLITE_API const void *sqlite3_value_text16be(sqlite3_value*);
SQLITE_API int sqlite3_value_bytes(sqlite3_value*);
SQLITE_API int sqlite3_value_bytes16(sqlite3_value*);
SQLITE_API int sqlite3_value_type(sqlite3_value*);
SQLITE_API int sqlite3_value_numeric_type(sqlite3_value*);
SQLITE_API int sqlite3_value_nochange(sqlite3_value*);
SQLITE_API int sqlite3_value_frombind(sqlite3_value*);

/*
** CAPI3REF: Report the internal text encoding state of an sqlite3_value object
** METHOD: sqlite3_value
**
** ^(The sqlite3_value_encoding(X) interface returns one of [SQLITE_UTF8],
** [SQLITE_UTF16BE], or [SQLITE_UTF16LE] according to the current text encoding
** of the value X, assuming that X has type TEXT.)^  If sqlite3_value_type(X)
** returns something other than SQLITE_TEXT, then the return value from
** sqlite3_value_encoding(X) is meaningless.  ^Calls to
** [sqlite3_value_text(X)], [sqlite3_value_text16(X)], [sqlite3_value_text16be(X)],
** [sqlite3_value_text16le(X)], [sqlite3_value_bytes(X)], or
** [sqlite3_value_bytes16(X)] might change the encoding of the value X and
** thus change the return from subsequent calls to sqlite3_value_encoding(X).
**
** This routine is intended for used by applications that test and validate
** the SQLite implementation.  This routine is inquiring about the opaque
** internal state of an [sqlite3_value] object.  Ordinary applications should
** not need to know what the internal state of an sqlite3_value object is and
** hence should not need to use this interface.
*/
SQLITE_API int sqlite3_value_encoding(sqlite3_value*);

/*
** CAPI3REF: Finding The Subtype Of SQL Values
** METHOD: sqlite3_value
**
** The sqlite3_value_subtype(V) function returns the subtype for
** an [application-defined SQL function] argument V.  The subtype
** information can be used to pass a limited amount of context from
** one SQL function to another.  Use the [sqlite3_result_subtype()]
** routine to set the subtype for the return value of an SQL function.
**
** Every [application-defined SQL function] that invoke this interface
** should include the [SQLITE_SUBTYPE] property in the text
** encoding argument when the function is [sqlite3_create_function|registered].
** If the [SQLITE_SUBTYPE] property is omitted, then sqlite3_value_subtype()
** might return zero instead of the upstream subtype in some corner cases.
*/
SQLITE_API unsigned int sqlite3_value_subtype(sqlite3_value*);

/*
** CAPI3REF: Copy And Free SQL Values
** METHOD: sqlite3_value
**
** ^The sqlite3_value_dup(V) interface makes a copy of the [sqlite3_value]
** object D and returns a pointer to that copy.  ^The [sqlite3_value] returned
** is a [protected sqlite3_value] object even if the input is not.
** ^The sqlite3_value_dup(V) interface returns NULL if V is NULL or if a
** memory allocation fails. ^If V is a [pointer value], then the result
** of sqlite3_value_dup(V) is a NULL value.
**
** ^The sqlite3_value_free(V) interface frees an [sqlite3_value] object
** previously obtained from [sqlite3_value_dup()].  ^If V is a NULL pointer
** then sqlite3_value_free(V) is a harmless no-op.
*/
SQLITE_API sqlite3_value *sqlite3_value_dup(const sqlite3_value*);
SQLITE_API void sqlite3_value_free(sqlite3_value*);

/*
** CAPI3REF: Obtain Aggregate Function Context
** METHOD: sqlite3_context
**
** Implementations of aggregate SQL functions use this
** routine to allocate memory for storing their state.
**
** ^The first time the sqlite3_aggregate_context(C,N) routine is called
** for a particular aggregate function, SQLite allocates
** N bytes of memory, zeroes out that memory, and returns a pointer
** to the new memory. ^On second and subsequent calls to
** sqlite3_aggregate_context() for the same aggregate function instance,
** the same buffer is returned.  Sqlite3_aggregate_context() is normally
** called once for each invocation of the xStep callback and then one
** last time when the xFinal callback is invoked.  ^(When no rows match
** an aggregate query, the xStep() callback of the aggregate function
** implementation is never called and xFinal() is called exactly once.
** In those cases, sqlite3_aggregate_context() might be called for the
** first time from within xFinal().)^
**
** ^The sqlite3_aggregate_context(C,N) routine returns a NULL pointer
** when first called if N is less than or equal to zero or if a memory
** allocation error occurs.
**
** ^(The amount of space allocated by sqlite3_aggregate_context(C,N) is
** determined by the N parameter on first successful call.  Changing the
** value of N in any subsequent call to sqlite3_aggregate_context() within
** the same aggregate function instance will not resize the memory
** allocation.)^  Within the xFinal callback, it is customary to set
** N=0 in calls to sqlite3_aggregate_context(C,N) so that no
** pointless memory allocations occur.
**
** ^SQLite automatically frees the memory allocated by
** sqlite3_aggregate_context() when the aggregate query concludes.
**
** The first parameter must be a copy of the
** [sqlite3_context | SQL function context] that is the first parameter
** to the xStep or xFinal callback routine that implements the aggregate
** function.
**
** This routine must be called from the same thread in which
** the aggregate SQL function is running.
*/
SQLITE_API void *sqlite3_aggregate_context(sqlite3_context*, int nBytes);

/*
** CAPI3REF: User Data For Functions
** METHOD: sqlite3_context
**
** ^The sqlite3_user_data() interface returns a copy of
** the pointer that was the pUserData parameter (the 5th parameter)
** of the [sqlite3_create_function()]
** and [sqlite3_create_function16()] routines that originally
** registered the application defined function.
**
** This routine must be called from the same thread in which
** the application-defined function is running.
*/
SQLITE_API void *sqlite3_user_data(sqlite3_context*);

/*
** CAPI3REF: Database Connection For Functions
** METHOD: sqlite3_context
**
** ^The sqlite3_context_db_handle() interface returns a copy of
** the pointer to the [database connection] (the 1st parameter)
** of the [sqlite3_create_function()]
** and [sqlite3_create_function16()] routines that originally
** registered the application defined function.
*/
SQLITE_API sqlite3 *sqlite3_context_db_handle(sqlite3_context*);

/*
** CAPI3REF: Function Auxiliary Data
** METHOD: sqlite3_context
**
** These functions may be used by (non-aggregate) SQL functions to
** associate auxiliary data with argument values. If the same argument
** value is passed to multiple invocations of the same SQL function during
** query execution, under some circumstances the associated auxiliary data
** might be preserved.  An example of where this might be useful is in a
** regular-expression matching function. The compiled version of the regular
** expression can be stored as auxiliary data associated with the pattern string.
** Then as long as the pattern string remains the same,
** the compiled regular expression can be reused on multiple
** invocations of the same function.
**
** ^The sqlite3_get_auxdata(C,N) interface returns a pointer to the auxiliary data
** associated by the sqlite3_set_auxdata(C,N,P,X) function with the Nth argument
** value to the application-defined function.  ^N is zero for the left-most
** function argument.  ^If there is no auxiliary data
** associated with the function argument, the sqlite3_get_auxdata(C,N) interface
** returns a NULL pointer.
**
** ^The sqlite3_set_auxdata(C,N,P,X) interface saves P as auxiliary data for the
** N-th argument of the application-defined function.  ^Subsequent
** calls to sqlite3_get_auxdata(C,N) return P from the most recent
** sqlite3_set_auxdata(C,N,P,X) call if the auxiliary data is still valid or
** NULL if the auxiliary data has been discarded.
** ^After each call to sqlite3_set_auxdata(C,N,P,X) where X is not NULL,
** SQLite will invoke the destructor function X with parameter P exactly
** once, when the auxiliary data is discarded.
** SQLite is free to discard the auxiliary data at any time, including: <ul>
** <li> ^(when the corresponding function parameter changes)^, or
** <li> ^(when [sqlite3_reset()] or [sqlite3_finalize()] is called for the
**      SQL statement)^, or
** <li> ^(when sqlite3_set_auxdata() is invoked again on the same
**       parameter)^, or
** <li> ^(during the original sqlite3_set_auxdata() call when a memory
**      allocation error occurs.)^
** <li> ^(during the original sqlite3_set_auxdata() call if the function
**      is evaluated during query planning instead of during query execution,
**      as sometimes happens with [SQLITE_ENABLE_STAT4].)^ </ul>
**
** Note the last two bullets in particular.  The destructor X in
** sqlite3_set_auxdata(C,N,P,X) might be called immediately, before the
** sqlite3_set_auxdata() interface even returns.  Hence sqlite3_set_auxdata()
** should be called near the end of the function implementation and the
** function implementation should not make any use of P after
** sqlite3_set_auxdata() has been called.  Furthermore, a call to
** sqlite3_get_auxdata() that occurs immediately after a corresponding call
** to sqlite3_set_auxdata() might still return NULL if an out-of-memory
** condition occurred during the sqlite3_set_auxdata() call or if the
** function is being evaluated during query planning rather than during
** query execution.
**
** ^(In practice, auxiliary data is preserved between function calls for
** function parameters that are compile-time constants, including literal
** values and [parameters] and expressions composed from the same.)^
**
** The value of the N parameter to these interfaces should be non-negative.
** Future enhancements may make use of negative N values to define new
** kinds of function caching behavior.
**
** These routines must be called from the same thread in which
** the SQL function is running.
**
** See also: [sqlite3_get_clientdata()] and [sqlite3_set_clientdata()].
*/
SQLITE_API void *sqlite3_get_auxdata(sqlite3_context*, int N);
SQLITE_API void sqlite3_set_auxdata(sqlite3_context*, int N, void*, void (*)(void*));

/*
** CAPI3REF: Database Connection Client Data
** METHOD: sqlite3
**
** These functions are used to associate one or more named pointers
** with a [database connection].
** A call to sqlite3_set_clientdata(D,N,P,X) causes the pointer P
** to be attached to [database connection] D using name N.  Subsequent
** calls to sqlite3_get_clientdata(D,N) will return a copy of pointer P
** or a NULL pointer if there were no prior calls to
** sqlite3_set_clientdata() with the same values of D and N.
** Names are compared using strcmp() and are thus case sensitive.
**
** If P and X are both non-NULL, then the destructor X is invoked with
** argument P on the first of the following occurrences:
** <ul>
** <li> An out-of-memory error occurs during the call to
**      sqlite3_set_clientdata() which attempts to register pointer P.
** <li> A subsequent call to sqlite3_set_clientdata(D,N,P,X) is made
**      with the same D and N parameters.
** <li> The database connection closes.  SQLite does not make any guarantees
**      about the order in which destructors are called, only that all
**      destructors will be called exactly once at some point during the
**      database connection closing process.
** </ul>
**
** SQLite does not do anything with client data other than invoke
** destructors on the client data at the appropriate time.  The intended
** use for client data is to provide a mechanism for wrapper libraries
** to store additional information about an SQLite database connection.
**
** There is no limit (other than available memory) on the number of different
** client data pointers (with different names) that can be attached to a
** single database connection.  However, the implementation is optimized
** for the case of having only one or two different client data names.
** Applications and wrapper libraries are discouraged from using more than
** one client data name each.
**
** There is no way to enumerate the client data pointers
** associated with a database connection.  The N parameter can be thought
** of as a secret key such that only code that knows the secret key is able
** to access the associated data.
**
** Security Warning:  These interfaces should not be exposed in scripting
** languages or in other circumstances where it might be possible for an
** an attacker to invoke them.  Any agent that can invoke these interfaces
** can probably also take control of the process.
**
** Database connection client data is only available for SQLite
** version 3.44.0 ([dateof:3.44.0]) and later.
**
** See also: [sqlite3_set_auxdata()] and [sqlite3_get_auxdata()].
*/
SQLITE_API void *sqlite3_get_clientdata(sqlite3*,const char*);
SQLITE_API int sqlite3_set_clientdata(sqlite3*, const char*, void*, void(*)(void*));

/*
** CAPI3REF: Constants Defining Special Destructor Behavior
**
** These are special values for the destructor that is passed in as the
** final argument to routines like [sqlite3_result_blob()].  ^If the destructor
** argument is SQLITE_STATIC, it means that the content pointer is constant
** and will never change.  It does not need to be destroyed.  ^The
** SQLITE_TRANSIENT value means that the content will likely change in
