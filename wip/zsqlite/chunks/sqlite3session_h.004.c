** <dt>SQLITE_CHANGESET_CONSTRAINT<dd>
**   If any other constraint violation occurs while applying a change (i.e.
**   a UNIQUE, CHECK or NOT NULL constraint), the conflict handler is
**   invoked with CHANGESET_CONSTRAINT as the second argument.
**
**   There is no conflicting row in this case. The results of invoking the
**   sqlite3changeset_conflict() API are undefined.
**
** </dl>
*/
#define SQLITE_CHANGESET_DATA        1
#define SQLITE_CHANGESET_NOTFOUND    2
#define SQLITE_CHANGESET_CONFLICT    3
#define SQLITE_CHANGESET_CONSTRAINT  4
#define SQLITE_CHANGESET_FOREIGN_KEY 5

/*
** CAPI3REF: Constants Returned By The Conflict Handler
**
** A conflict handler callback must return one of the following three values.
**
** <dl>
** <dt>SQLITE_CHANGESET_OMIT<dd>
**   If a conflict handler returns this value no special action is taken. The
**   change that caused the conflict is not applied. The session module
**   continues to the next change in the changeset.
**
** <dt>SQLITE_CHANGESET_REPLACE<dd>
**   This value may only be returned if the second argument to the conflict
**   handler was SQLITE_CHANGESET_DATA or SQLITE_CHANGESET_CONFLICT. If this
**   is not the case, any changes applied so far are rolled back and the
**   call to sqlite3changeset_apply() returns SQLITE_MISUSE.
**
**   If CHANGESET_REPLACE is returned by an SQLITE_CHANGESET_DATA conflict
**   handler, then the conflicting row is either updated or deleted, depending
**   on the type of change.
**
**   If CHANGESET_REPLACE is returned by an SQLITE_CHANGESET_CONFLICT conflict
**   handler, then the conflicting row is removed from the database and a
**   second attempt to apply the change is made. If this second attempt fails,
**   the original row is restored to the database before continuing.
**
** <dt>SQLITE_CHANGESET_ABORT<dd>
**   If this value is returned, any changes applied so far are rolled back
**   and the call to sqlite3changeset_apply() returns SQLITE_ABORT.
** </dl>
*/
#define SQLITE_CHANGESET_OMIT       0
#define SQLITE_CHANGESET_REPLACE    1
#define SQLITE_CHANGESET_ABORT      2

/*
** CAPI3REF: Rebasing changesets
** EXPERIMENTAL
**
** Suppose there is a site hosting a database in state S0. And that
** modifications are made that move that database to state S1 and a
** changeset recorded (the "local" changeset). Then, a changeset based
** on S0 is received from another site (the "remote" changeset) and
** applied to the database. The database is then in state
** (S1+"remote"), where the exact state depends on any conflict
** resolution decisions (OMIT or REPLACE) made while applying "remote".
** Rebasing a changeset is to update it to take those conflict
** resolution decisions into account, so that the same conflicts
** do not have to be resolved elsewhere in the network.
**
** For example, if both the local and remote changesets contain an
** INSERT of the same key on "CREATE TABLE t1(a PRIMARY KEY, b)":
**
**   local:  INSERT INTO t1 VALUES(1, 'v1');
**   remote: INSERT INTO t1 VALUES(1, 'v2');
**
** and the conflict resolution is REPLACE, then the INSERT change is
** removed from the local changeset (it was overridden). Or, if the
** conflict resolution was "OMIT", then the local changeset is modified
** to instead contain:
**
**           UPDATE t1 SET b = 'v2' WHERE a=1;
**
** Changes within the local changeset are rebased as follows:
**
** <dl>
** <dt>Local INSERT<dd>
**   This may only conflict with a remote INSERT. If the conflict
**   resolution was OMIT, then add an UPDATE change to the rebased
**   changeset. Or, if the conflict resolution was REPLACE, add
**   nothing to the rebased changeset.
**
** <dt>Local DELETE<dd>
**   This may conflict with a remote UPDATE or DELETE. In both cases the
**   only possible resolution is OMIT. If the remote operation was a
**   DELETE, then add no change to the rebased changeset. If the remote
**   operation was an UPDATE, then the old.* fields of change are updated
**   to reflect the new.* values in the UPDATE.
**
** <dt>Local UPDATE<dd>
**   This may conflict with a remote UPDATE or DELETE. If it conflicts
**   with a DELETE, and the conflict resolution was OMIT, then the update
**   is changed into an INSERT. Any undefined values in the new.* record
**   from the update change are filled in using the old.* values from
**   the conflicting DELETE. Or, if the conflict resolution was REPLACE,
**   the UPDATE change is simply omitted from the rebased changeset.
**
**   If conflict is with a remote UPDATE and the resolution is OMIT, then
**   the old.* values are rebased using the new.* values in the remote
**   change. Or, if the resolution is REPLACE, then the change is copied
**   into the rebased changeset with updates to columns also updated by
**   the conflicting remote UPDATE removed. If this means no columns would
**   be updated, the change is omitted.
** </dl>
**
** A local change may be rebased against multiple remote changes
** simultaneously. If a single key is modified by multiple remote
** changesets, they are combined as follows before the local changeset
** is rebased:
**
** <ul>
**    <li> If there has been one or more REPLACE resolutions on a
**         key, it is rebased according to a REPLACE.
**
**    <li> If there have been no REPLACE resolutions on a key, then
**         the local changeset is rebased according to the most recent
**         of the OMIT resolutions.
** </ul>
**
** Note that conflict resolutions from multiple remote changesets are
** combined on a per-field basis, not per-row. This means that in the
** case of multiple remote UPDATE operations, some fields of a single
** local change may be rebased for REPLACE while others are rebased for
** OMIT.
**
** In order to rebase a local changeset, the remote changeset must first
** be applied to the local database using sqlite3changeset_apply_v2() and
** the buffer of rebase information captured. Then:
**
** <ol>
**   <li> An sqlite3_rebaser object is created by calling
**        sqlite3rebaser_create().
**   <li> The new object is configured with the rebase buffer obtained from
**        sqlite3changeset_apply_v2() by calling sqlite3rebaser_configure().
**        If the local changeset is to be rebased against multiple remote
**        changesets, then sqlite3rebaser_configure() should be called
**        multiple times, in the same order that the multiple
**        sqlite3changeset_apply_v2() calls were made.
**   <li> Each local changeset is rebased by calling sqlite3rebaser_rebase().
**   <li> The sqlite3_rebaser object is deleted by calling
**        sqlite3rebaser_delete().
** </ol>
*/
typedef struct sqlite3_rebaser sqlite3_rebaser;

/*
** CAPI3REF: Create a changeset rebaser object.
** EXPERIMENTAL
**
** Allocate a new changeset rebaser object. If successful, set (*ppNew) to
** point to the new object and return SQLITE_OK. Otherwise, if an error
** occurs, return an SQLite error code (e.g. SQLITE_NOMEM) and set (*ppNew)
** to NULL.
*/
SQLITE_API int sqlite3rebaser_create(sqlite3_rebaser **ppNew);

/*
** CAPI3REF: Configure a changeset rebaser object.
** EXPERIMENTAL
**
** Configure the changeset rebaser object to rebase changesets according
** to the conflict resolutions described by buffer pRebase (size nRebase
** bytes), which must have been obtained from a previous call to
** sqlite3changeset_apply_v2().
*/
SQLITE_API int sqlite3rebaser_configure(
  sqlite3_rebaser*,
  int nRebase, const void *pRebase
);

/*
** CAPI3REF: Rebase a changeset
** EXPERIMENTAL
**
** Argument pIn must point to a buffer containing a changeset nIn bytes
** in size. This function allocates and populates a buffer with a copy
** of the changeset rebased according to the configuration of the
** rebaser object passed as the first argument. If successful, (*ppOut)
** is set to point to the new buffer containing the rebased changeset and
** (*pnOut) to its size in bytes and SQLITE_OK returned. It is the
** responsibility of the caller to eventually free the new buffer using
** sqlite3_free(). Otherwise, if an error occurs, (*ppOut) and (*pnOut)
** are set to zero and an SQLite error code returned.
*/
SQLITE_API int sqlite3rebaser_rebase(
  sqlite3_rebaser*,
  int nIn, const void *pIn,
  int *pnOut, void **ppOut
);

/*
** CAPI3REF: Delete a changeset rebaser object.
** EXPERIMENTAL
**
** Delete the changeset rebaser object and all associated resources. There
** should be one call to this function for each successful invocation
** of sqlite3rebaser_create().
*/
SQLITE_API void sqlite3rebaser_delete(sqlite3_rebaser *p);

/*
** CAPI3REF: Streaming Versions of API functions.
**
** The six streaming API xxx_strm() functions serve similar purposes to the
** corresponding non-streaming API functions:
**
** <table border=1 style="margin-left:8ex;margin-right:8ex">
**   <tr><th>Streaming function<th>Non-streaming equivalent</th>
**   <tr><td>sqlite3changeset_apply_strm<td>[sqlite3changeset_apply]
**   <tr><td>sqlite3changeset_apply_strm_v2<td>[sqlite3changeset_apply_v2]
**   <tr><td>sqlite3changeset_concat_strm<td>[sqlite3changeset_concat]
**   <tr><td>sqlite3changeset_invert_strm<td>[sqlite3changeset_invert]
**   <tr><td>sqlite3changeset_start_strm<td>[sqlite3changeset_start]
**   <tr><td>sqlite3session_changeset_strm<td>[sqlite3session_changeset]
**   <tr><td>sqlite3session_patchset_strm<td>[sqlite3session_patchset]
** </table>
**
** Non-streaming functions that accept changesets (or patchsets) as input
** require that the entire changeset be stored in a single buffer in memory.
** Similarly, those that return a changeset or patchset do so by returning
** a pointer to a single large buffer allocated using sqlite3_malloc().
** Normally this is convenient. However, if an application running in a
** low-memory environment is required to handle very large changesets, the
** large contiguous memory allocations required can become onerous.
**
** In order to avoid this problem, instead of a single large buffer, input
** is passed to a streaming API functions by way of a callback function that
** the sessions module invokes to incrementally request input data as it is
** required. In all cases, a pair of API function parameters such as
**
**  <pre>
**  &nbsp;     int nChangeset,
**  &nbsp;     void *pChangeset,
**  </pre>
**
** Is replaced by:
**
**  <pre>
**  &nbsp;     int (*xInput)(void *pIn, void *pData, int *pnData),
**  &nbsp;     void *pIn,
**  </pre>
**
** Each time the xInput callback is invoked by the sessions module, the first
** argument passed is a copy of the supplied pIn context pointer. The second
** argument, pData, points to a buffer (*pnData) bytes in size. Assuming no
** error occurs the xInput method should copy up to (*pnData) bytes of data
** into the buffer and set (*pnData) to the actual number of bytes copied
** before returning SQLITE_OK. If the input is completely exhausted, (*pnData)
** should be set to zero to indicate this. Or, if an error occurs, an SQLite
** error code should be returned. In all cases, if an xInput callback returns
** an error, all processing is abandoned and the streaming API function
** returns a copy of the error code to the caller.
**
** In the case of sqlite3changeset_start_strm(), the xInput callback may be
** invoked by the sessions module at any point during the lifetime of the
** iterator. If such an xInput callback returns an error, the iterator enters
** an error state, whereby all subsequent calls to iterator functions
** immediately fail with the same error code as returned by xInput.
**
** Similarly, streaming API functions that return changesets (or patchsets)
** return them in chunks by way of a callback function instead of via a
** pointer to a single large buffer. In this case, a pair of parameters such
** as:
**
**  <pre>
**  &nbsp;     int *pnChangeset,
**  &nbsp;     void **ppChangeset,
**  </pre>
**
** Is replaced by:
**
**  <pre>
**  &nbsp;     int (*xOutput)(void *pOut, const void *pData, int nData),
**  &nbsp;     void *pOut
**  </pre>
**
** The xOutput callback is invoked zero or more times to return data to
** the application. The first parameter passed to each call is a copy of the
** pOut pointer supplied by the application. The second parameter, pData,
** points to a buffer nData bytes in size containing the chunk of output
** data being returned. If the xOutput callback successfully processes the
** supplied data, it should return SQLITE_OK to indicate success. Otherwise,
** it should return some other SQLite error code. In this case processing
** is immediately abandoned and the streaming API function returns a copy
** of the xOutput error code to the application.
**
** The sessions module never invokes an xOutput callback with the third
** parameter set to a value less than or equal to zero. Other than this,
** no guarantees are made as to the size of the chunks of data returned.
*/
SQLITE_API int sqlite3changeset_apply_strm(
  sqlite3 *db,                    /* Apply change to "main" db of this handle */
  int (*xInput)(void *pIn, void *pData, int *pnData), /* Input function */
  void *pIn,                                          /* First arg for xInput */
  int(*xFilter)(
    void *pCtx,                   /* Copy of sixth arg to _apply() */
    const char *zTab              /* Table name */
  ),
  int(*xConflict)(
    void *pCtx,                   /* Copy of sixth arg to _apply() */
    int eConflict,                /* DATA, MISSING, CONFLICT, CONSTRAINT */
    sqlite3_changeset_iter *p     /* Handle describing change and conflict */
  ),
  void *pCtx                      /* First argument passed to xConflict */
);
SQLITE_API int sqlite3changeset_apply_v2_strm(
  sqlite3 *db,                    /* Apply change to "main" db of this handle */
  int (*xInput)(void *pIn, void *pData, int *pnData), /* Input function */
  void *pIn,                                          /* First arg for xInput */
  int(*xFilter)(
    void *pCtx,                   /* Copy of sixth arg to _apply() */
    const char *zTab              /* Table name */
  ),
  int(*xConflict)(
    void *pCtx,                   /* Copy of sixth arg to _apply() */
    int eConflict,                /* DATA, MISSING, CONFLICT, CONSTRAINT */
    sqlite3_changeset_iter *p     /* Handle describing change and conflict */
  ),
  void *pCtx,                     /* First argument passed to xConflict */
  void **ppRebase, int *pnRebase,
  int flags
);
SQLITE_API int sqlite3changeset_concat_strm(
  int (*xInputA)(void *pIn, void *pData, int *pnData),
  void *pInA,
  int (*xInputB)(void *pIn, void *pData, int *pnData),
  void *pInB,
  int (*xOutput)(void *pOut, const void *pData, int nData),
  void *pOut
);
SQLITE_API int sqlite3changeset_invert_strm(
  int (*xInput)(void *pIn, void *pData, int *pnData),
  void *pIn,
  int (*xOutput)(void *pOut, const void *pData, int nData),
  void *pOut
);
SQLITE_API int sqlite3changeset_start_strm(
  sqlite3_changeset_iter **pp,
  int (*xInput)(void *pIn, void *pData, int *pnData),
  void *pIn
);
SQLITE_API int sqlite3changeset_start_v2_strm(
  sqlite3_changeset_iter **pp,
  int (*xInput)(void *pIn, void *pData, int *pnData),
  void *pIn,
  int flags
);
SQLITE_API int sqlite3session_changeset_strm(
  sqlite3_session *pSession,
  int (*xOutput)(void *pOut, const void *pData, int nData),
  void *pOut
);
SQLITE_API int sqlite3session_patchset_strm(
  sqlite3_session *pSession,
  int (*xOutput)(void *pOut, const void *pData, int nData),
  void *pOut
);
SQLITE_API int sqlite3changegroup_add_strm(sqlite3_changegroup*,
    int (*xInput)(void *pIn, void *pData, int *pnData),
    void *pIn
);
SQLITE_API int sqlite3changegroup_output_strm(sqlite3_changegroup*,
    int (*xOutput)(void *pOut, const void *pData, int nData),
    void *pOut
);
SQLITE_API int sqlite3rebaser_rebase_strm(
  sqlite3_rebaser *pRebaser,
  int (*xInput)(void *pIn, void *pData, int *pnData),
  void *pIn,
  int (*xOutput)(void *pOut, const void *pData, int nData),
  void *pOut
);

/*
** CAPI3REF: Configure global parameters
**
** The sqlite3session_config() interface is used to make global configuration
** changes to the sessions module in order to tune it to the specific needs
** of the application.
**
** The sqlite3session_config() interface is not threadsafe. If it is invoked
** while any other thread is inside any other sessions method then the
** results are undefined. Furthermore, if it is invoked after any sessions
** related objects have been created, the results are also undefined.
**
** The first argument to the sqlite3session_config() function must be one
** of the SQLITE_SESSION_CONFIG_XXX constants defined below. The
** interpretation of the (void*) value passed as the second parameter and
** the effect of calling this function depends on the value of the first
** parameter.
**
** <dl>
** <dt>SQLITE_SESSION_CONFIG_STRMSIZE<dd>
**    By default, the sessions module streaming interfaces attempt to input
**    and output data in approximately 1 KiB chunks. This operand may be used
**    to set and query the value of this configuration setting. The pointer
**    passed as the second argument must point to a value of type (int).
**    If this value is greater than 0, it is used as the new streaming data
**    chunk size for both input and output. Before returning, the (int) value
**    pointed to by pArg is set to the final value of the streaming interface
**    chunk size.
** </dl>
**
** This function returns SQLITE_OK if successful, or an SQLite error code
** otherwise.
*/
SQLITE_API int sqlite3session_config(int op, void *pArg);

/*
** CAPI3REF: Values for sqlite3session_config().
*/
#define SQLITE_SESSION_CONFIG_STRMSIZE 1

/*
** Make sure we can call this stuff from C++.
*/
#if 0
}
