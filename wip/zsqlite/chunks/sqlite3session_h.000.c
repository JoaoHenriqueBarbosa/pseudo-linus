
#if !defined(__SQLITESESSION_H_) && defined(SQLITE_ENABLE_SESSION)
#define __SQLITESESSION_H_ 1

/*
** Make sure we can call this stuff from C++.
*/
#if 0
extern "C" {
#endif


/*
** CAPI3REF: Session Object Handle
**
** An instance of this object is a [session] that can be used to
** record changes to a database.
*/
typedef struct sqlite3_session sqlite3_session;

/*
** CAPI3REF: Changeset Iterator Handle
**
** An instance of this object acts as a cursor for iterating
** over the elements of a [changeset] or [patchset].
*/
typedef struct sqlite3_changeset_iter sqlite3_changeset_iter;

/*
** CAPI3REF: Create A New Session Object
** CONSTRUCTOR: sqlite3_session
**
** Create a new session object attached to database handle db. If successful,
** a pointer to the new object is written to *ppSession and SQLITE_OK is
** returned. If an error occurs, *ppSession is set to NULL and an SQLite
** error code (e.g. SQLITE_NOMEM) is returned.
**
** It is possible to create multiple session objects attached to a single
** database handle.
**
** Session objects created using this function should be deleted using the
** [sqlite3session_delete()] function before the database handle that they
** are attached to is itself closed. If the database handle is closed before
** the session object is deleted, then the results of calling any session
** module function, including [sqlite3session_delete()] on the session object
** are undefined.
**
** Because the session module uses the [sqlite3_preupdate_hook()] API, it
** is not possible for an application to register a pre-update hook on a
** database handle that has one or more session objects attached. Nor is
** it possible to create a session object attached to a database handle for
** which a pre-update hook is already defined. The results of attempting
** either of these things are undefined.
**
** The session object will be used to create changesets for tables in
** database zDb, where zDb is either "main", or "temp", or the name of an
** attached database. It is not an error if database zDb is not attached
** to the database when the session object is created.
*/
SQLITE_API int sqlite3session_create(
  sqlite3 *db,                    /* Database handle */
  const char *zDb,                /* Name of db (e.g. "main") */
  sqlite3_session **ppSession     /* OUT: New session object */
);

/*
** CAPI3REF: Delete A Session Object
** DESTRUCTOR: sqlite3_session
**
** Delete a session object previously allocated using
** [sqlite3session_create()]. Once a session object has been deleted, the
** results of attempting to use pSession with any other session module
** function are undefined.
**
** Session objects must be deleted before the database handle to which they
** are attached is closed. Refer to the documentation for
** [sqlite3session_create()] for details.
*/
SQLITE_API void sqlite3session_delete(sqlite3_session *pSession);

/*
** CAPI3REF: Configure a Session Object
** METHOD: sqlite3_session
**
** This method is used to configure a session object after it has been
** created. At present the only valid values for the second parameter are
** [SQLITE_SESSION_OBJCONFIG_SIZE] and [SQLITE_SESSION_OBJCONFIG_ROWID].
**
*/
SQLITE_API int sqlite3session_object_config(sqlite3_session*, int op, void *pArg);

/*
** CAPI3REF: Options for sqlite3session_object_config
**
** The following values may passed as the the 2nd parameter to
** sqlite3session_object_config().
**
** <dt>SQLITE_SESSION_OBJCONFIG_SIZE <dd>
**   This option is used to set, clear or query the flag that enables
**   the [sqlite3session_changeset_size()] API. Because it imposes some
**   computational overhead, this API is disabled by default. Argument
**   pArg must point to a value of type (int). If the value is initially
**   0, then the sqlite3session_changeset_size() API is disabled. If it
**   is greater than 0, then the same API is enabled. Or, if the initial
**   value is less than zero, no change is made. In all cases the (int)
**   variable is set to 1 if the sqlite3session_changeset_size() API is
**   enabled following the current call, or 0 otherwise.
**
**   It is an error (SQLITE_MISUSE) to attempt to modify this setting after
**   the first table has been attached to the session object.
**
** <dt>SQLITE_SESSION_OBJCONFIG_ROWID <dd>
**   This option is used to set, clear or query the flag that enables
**   collection of data for tables with no explicit PRIMARY KEY.
**
**   Normally, tables with no explicit PRIMARY KEY are simply ignored
**   by the sessions module. However, if this flag is set, it behaves
**   as if such tables have a column "_rowid_ INTEGER PRIMARY KEY" inserted
**   as their leftmost columns.
**
**   It is an error (SQLITE_MISUSE) to attempt to modify this setting after
**   the first table has been attached to the session object.
*/
#define SQLITE_SESSION_OBJCONFIG_SIZE  1
#define SQLITE_SESSION_OBJCONFIG_ROWID 2

/*
** CAPI3REF: Enable Or Disable A Session Object
** METHOD: sqlite3_session
**
** Enable or disable the recording of changes by a session object. When
** enabled, a session object records changes made to the database. When
** disabled - it does not. A newly created session object is enabled.
** Refer to the documentation for [sqlite3session_changeset()] for further
** details regarding how enabling and disabling a session object affects
** the eventual changesets.
**
** Passing zero to this function disables the session. Passing a value
** greater than zero enables it. Passing a value less than zero is a
** no-op, and may be used to query the current state of the session.
**
** The return value indicates the final state of the session object: 0 if
** the session is disabled, or 1 if it is enabled.
*/
SQLITE_API int sqlite3session_enable(sqlite3_session *pSession, int bEnable);

/*
** CAPI3REF: Set Or Clear the Indirect Change Flag
** METHOD: sqlite3_session
**
** Each change recorded by a session object is marked as either direct or
** indirect. A change is marked as indirect if either:
**
** <ul>
**   <li> The session object "indirect" flag is set when the change is
**        made, or
**   <li> The change is made by an SQL trigger or foreign key action
**        instead of directly as a result of a users SQL statement.
** </ul>
**
** If a single row is affected by more than one operation within a session,
** then the change is considered indirect if all operations meet the criteria
** for an indirect change above, or direct otherwise.
**
** This function is used to set, clear or query the session object indirect
** flag.  If the second argument passed to this function is zero, then the
** indirect flag is cleared. If it is greater than zero, the indirect flag
** is set. Passing a value less than zero does not modify the current value
** of the indirect flag, and may be used to query the current state of the
** indirect flag for the specified session object.
**
** The return value indicates the final state of the indirect flag: 0 if
** it is clear, or 1 if it is set.
*/
SQLITE_API int sqlite3session_indirect(sqlite3_session *pSession, int bIndirect);

/*
** CAPI3REF: Attach A Table To A Session Object
** METHOD: sqlite3_session
**
** If argument zTab is not NULL, then it is the name of a table to attach
** to the session object passed as the first argument. All subsequent changes
** made to the table while the session object is enabled will be recorded. See
** documentation for [sqlite3session_changeset()] for further details.
**
** Or, if argument zTab is NULL, then changes are recorded for all tables
** in the database. If additional tables are added to the database (by
** executing "CREATE TABLE" statements) after this call is made, changes for
** the new tables are also recorded.
**
** Changes can only be recorded for tables that have a PRIMARY KEY explicitly
** defined as part of their CREATE TABLE statement. It does not matter if the
** PRIMARY KEY is an "INTEGER PRIMARY KEY" (rowid alias) or not. The PRIMARY
** KEY may consist of a single column, or may be a composite key.
**
** It is not an error if the named table does not exist in the database. Nor
** is it an error if the named table does not have a PRIMARY KEY. However,
** no changes will be recorded in either of these scenarios.
**
** Changes are not recorded for individual rows that have NULL values stored
** in one or more of their PRIMARY KEY columns.
**
** SQLITE_OK is returned if the call completes without error. Or, if an error
** occurs, an SQLite error code (e.g. SQLITE_NOMEM) is returned.
**
** <h3>Special sqlite_stat1 Handling</h3>
**
** As of SQLite version 3.22.0, the "sqlite_stat1" table is an exception to
** some of the rules above. In SQLite, the schema of sqlite_stat1 is:
**  <pre>
**  &nbsp;     CREATE TABLE sqlite_stat1(tbl,idx,stat)
**  </pre>
**
** Even though sqlite_stat1 does not have a PRIMARY KEY, changes are
** recorded for it as if the PRIMARY KEY is (tbl,idx). Additionally, changes
** are recorded for rows for which (idx IS NULL) is true. However, for such
** rows a zero-length blob (SQL value X'') is stored in the changeset or
** patchset instead of a NULL value. This allows such changesets to be
** manipulated by legacy implementations of sqlite3changeset_invert(),
** concat() and similar.
**
** The sqlite3changeset_apply() function automatically converts the
** zero-length blob back to a NULL value when updating the sqlite_stat1
** table. However, if the application calls sqlite3changeset_new(),
** sqlite3changeset_old() or sqlite3changeset_conflict on a changeset
** iterator directly (including on a changeset iterator passed to a
** conflict-handler callback) then the X'' value is returned. The application
** must translate X'' to NULL itself if required.
**
** Legacy (older than 3.22.0) versions of the sessions module cannot capture
** changes made to the sqlite_stat1 table. Legacy versions of the
** sqlite3changeset_apply() function silently ignore any modifications to the
** sqlite_stat1 table that are part of a changeset or patchset.
*/
SQLITE_API int sqlite3session_attach(
  sqlite3_session *pSession,      /* Session object */
  const char *zTab                /* Table name */
);

/*
** CAPI3REF: Set a table filter on a Session Object.
** METHOD: sqlite3_session
**
** The second argument (xFilter) is the "filter callback". For changes to rows
** in tables that are not attached to the Session object, the filter is called
** to determine whether changes to the table's rows should be tracked or not.
** If xFilter returns 0, changes are not tracked. Note that once a table is
** attached, xFilter will not be called again.
*/
SQLITE_API void sqlite3session_table_filter(
  sqlite3_session *pSession,      /* Session object */
  int(*xFilter)(
    void *pCtx,                   /* Copy of third arg to _filter_table() */
    const char *zTab              /* Table name */
  ),
  void *pCtx                      /* First argument passed to xFilter */
);

/*
** CAPI3REF: Generate A Changeset From A Session Object
** METHOD: sqlite3_session
**
** Obtain a changeset containing changes to the tables attached to the
** session object passed as the first argument. If successful,
** set *ppChangeset to point to a buffer containing the changeset
** and *pnChangeset to the size of the changeset in bytes before returning
** SQLITE_OK. If an error occurs, set both *ppChangeset and *pnChangeset to
** zero and return an SQLite error code.
**
** A changeset consists of zero or more INSERT, UPDATE and/or DELETE changes,
** each representing a change to a single row of an attached table. An INSERT
** change contains the values of each field of a new database row. A DELETE
** contains the original values of each field of a deleted database row. An
** UPDATE change contains the original values of each field of an updated
** database row along with the updated values for each updated non-primary-key
** column. It is not possible for an UPDATE change to represent a change that
** modifies the values of primary key columns. If such a change is made, it
** is represented in a changeset as a DELETE followed by an INSERT.
**
** Changes are not recorded for rows that have NULL values stored in one or
** more of their PRIMARY KEY columns. If such a row is inserted or deleted,
** no corresponding change is present in the changesets returned by this
** function. If an existing row with one or more NULL values stored in
** PRIMARY KEY columns is updated so that all PRIMARY KEY columns are non-NULL,
** only an INSERT is appears in the changeset. Similarly, if an existing row
** with non-NULL PRIMARY KEY values is updated so that one or more of its
** PRIMARY KEY columns are set to NULL, the resulting changeset contains a
** DELETE change only.
**
** The contents of a changeset may be traversed using an iterator created
** using the [sqlite3changeset_start()] API. A changeset may be applied to
** a database with a compatible schema using the [sqlite3changeset_apply()]
** API.
**
** Within a changeset generated by this function, all changes related to a
** single table are grouped together. In other words, when iterating through
** a changeset or when applying a changeset to a database, all changes related
** to a single table are processed before moving on to the next table. Tables
** are sorted in the same order in which they were attached (or auto-attached)
** to the sqlite3_session object. The order in which the changes related to
** a single table are stored is undefined.
**
** Following a successful call to this function, it is the responsibility of
** the caller to eventually free the buffer that *ppChangeset points to using
** [sqlite3_free()].
**
** <h3>Changeset Generation</h3>
**
** Once a table has been attached to a session object, the session object
** records the primary key values of all new rows inserted into the table.
** It also records the original primary key and other column values of any
** deleted or updated rows. For each unique primary key value, data is only
** recorded once - the first time a row with said primary key is inserted,
** updated or deleted in the lifetime of the session.
**
** There is one exception to the previous paragraph: when a row is inserted,
** updated or deleted, if one or more of its primary key columns contain a
** NULL value, no record of the change is made.
**
** The session object therefore accumulates two types of records - those
** that consist of primary key values only (created when the user inserts
** a new record) and those that consist of the primary key values and the
** original values of other table columns (created when the users deletes
** or updates a record).
**
** When this function is called, the requested changeset is created using
** both the accumulated records and the current contents of the database
** file. Specifically:
**
** <ul>
**   <li> For each record generated by an insert, the database is queried
**        for a row with a matching primary key. If one is found, an INSERT
**        change is added to the changeset. If no such row is found, no change
**        is added to the changeset.
**
**   <li> For each record generated by an update or delete, the database is
**        queried for a row with a matching primary key. If such a row is
**        found and one or more of the non-primary key fields have been
**        modified from their original values, an UPDATE change is added to
**        the changeset. Or, if no such row is found in the table, a DELETE
**        change is added to the changeset. If there is a row with a matching
**        primary key in the database, but all fields contain their original
**        values, no change is added to the changeset.
** </ul>
**
** This means, amongst other things, that if a row is inserted and then later
** deleted while a session object is active, neither the insert nor the delete
** will be present in the changeset. Or if a row is deleted and then later a
** row with the same primary key values inserted while a session object is
** active, the resulting changeset will contain an UPDATE change instead of
