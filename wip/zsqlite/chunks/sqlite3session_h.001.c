** a DELETE and an INSERT.
**
** When a session object is disabled (see the [sqlite3session_enable()] API),
** it does not accumulate records when rows are inserted, updated or deleted.
** This may appear to have some counter-intuitive effects if a single row
** is written to more than once during a session. For example, if a row
** is inserted while a session object is enabled, then later deleted while
** the same session object is disabled, no INSERT record will appear in the
** changeset, even though the delete took place while the session was disabled.
** Or, if one field of a row is updated while a session is disabled, and
** another field of the same row is updated while the session is enabled, the
** resulting changeset will contain an UPDATE change that updates both fields.
*/
SQLITE_API int sqlite3session_changeset(
  sqlite3_session *pSession,      /* Session object */
  int *pnChangeset,               /* OUT: Size of buffer at *ppChangeset */
  void **ppChangeset              /* OUT: Buffer containing changeset */
);

/*
** CAPI3REF: Return An Upper-limit For The Size Of The Changeset
** METHOD: sqlite3_session
**
** By default, this function always returns 0. For it to return
** a useful result, the sqlite3_session object must have been configured
** to enable this API using sqlite3session_object_config() with the
** SQLITE_SESSION_OBJCONFIG_SIZE verb.
**
** When enabled, this function returns an upper limit, in bytes, for the size
** of the changeset that might be produced if sqlite3session_changeset() were
** called. The final changeset size might be equal to or smaller than the
** size in bytes returned by this function.
*/
SQLITE_API sqlite3_int64 sqlite3session_changeset_size(sqlite3_session *pSession);

/*
** CAPI3REF: Load The Difference Between Tables Into A Session
** METHOD: sqlite3_session
**
** If it is not already attached to the session object passed as the first
** argument, this function attaches table zTbl in the same manner as the
** [sqlite3session_attach()] function. If zTbl does not exist, or if it
** does not have a primary key, this function is a no-op (but does not return
** an error).
**
** Argument zFromDb must be the name of a database ("main", "temp" etc.)
** attached to the same database handle as the session object that contains
** a table compatible with the table attached to the session by this function.
** A table is considered compatible if it:
**
** <ul>
**   <li> Has the same name,
**   <li> Has the same set of columns declared in the same order, and
**   <li> Has the same PRIMARY KEY definition.
** </ul>
**
** If the tables are not compatible, SQLITE_SCHEMA is returned. If the tables
** are compatible but do not have any PRIMARY KEY columns, it is not an error
** but no changes are added to the session object. As with other session
** APIs, tables without PRIMARY KEYs are simply ignored.
**
** This function adds a set of changes to the session object that could be
** used to update the table in database zFrom (call this the "from-table")
** so that its content is the same as the table attached to the session
** object (call this the "to-table"). Specifically:
**
** <ul>
**   <li> For each row (primary key) that exists in the to-table but not in
**     the from-table, an INSERT record is added to the session object.
**
**   <li> For each row (primary key) that exists in the to-table but not in
**     the from-table, a DELETE record is added to the session object.
**
**   <li> For each row (primary key) that exists in both tables, but features
**     different non-PK values in each, an UPDATE record is added to the
**     session.
** </ul>
**
** To clarify, if this function is called and then a changeset constructed
** using [sqlite3session_changeset()], then after applying that changeset to
** database zFrom the contents of the two compatible tables would be
** identical.
**
** It an error if database zFrom does not exist or does not contain the
** required compatible table.
**
** If the operation is successful, SQLITE_OK is returned. Otherwise, an SQLite
** error code. In this case, if argument pzErrMsg is not NULL, *pzErrMsg
** may be set to point to a buffer containing an English language error
** message. It is the responsibility of the caller to free this buffer using
** sqlite3_free().
*/
SQLITE_API int sqlite3session_diff(
  sqlite3_session *pSession,
  const char *zFromDb,
  const char *zTbl,
  char **pzErrMsg
);


/*
** CAPI3REF: Generate A Patchset From A Session Object
** METHOD: sqlite3_session
**
** The differences between a patchset and a changeset are that:
**
** <ul>
**   <li> DELETE records consist of the primary key fields only. The
**        original values of other fields are omitted.
**   <li> The original values of any modified fields are omitted from
**        UPDATE records.
** </ul>
**
** A patchset blob may be used with up to date versions of all
** sqlite3changeset_xxx API functions except for sqlite3changeset_invert(),
** which returns SQLITE_CORRUPT if it is passed a patchset. Similarly,
** attempting to use a patchset blob with old versions of the
** sqlite3changeset_xxx APIs also provokes an SQLITE_CORRUPT error.
**
** Because the non-primary key "old.*" fields are omitted, no
** SQLITE_CHANGESET_DATA conflicts can be detected or reported if a patchset
** is passed to the sqlite3changeset_apply() API. Other conflict types work
** in the same way as for changesets.
**
** Changes within a patchset are ordered in the same way as for changesets
** generated by the sqlite3session_changeset() function (i.e. all changes for
** a single table are grouped together, tables appear in the order in which
** they were attached to the session object).
*/
SQLITE_API int sqlite3session_patchset(
  sqlite3_session *pSession,      /* Session object */
  int *pnPatchset,                /* OUT: Size of buffer at *ppPatchset */
  void **ppPatchset               /* OUT: Buffer containing patchset */
);

/*
** CAPI3REF: Test if a changeset has recorded any changes.
**
** Return non-zero if no changes to attached tables have been recorded by
** the session object passed as the first argument. Otherwise, if one or
** more changes have been recorded, return zero.
**
** Even if this function returns zero, it is possible that calling
** [sqlite3session_changeset()] on the session handle may still return a
** changeset that contains no changes. This can happen when a row in
** an attached table is modified and then later on the original values
** are restored. However, if this function returns non-zero, then it is
** guaranteed that a call to sqlite3session_changeset() will return a
** changeset containing zero changes.
*/
SQLITE_API int sqlite3session_isempty(sqlite3_session *pSession);

/*
** CAPI3REF: Query for the amount of heap memory used by a session object.
**
** This API returns the total amount of heap memory in bytes currently
** used by the session object passed as the only argument.
*/
SQLITE_API sqlite3_int64 sqlite3session_memory_used(sqlite3_session *pSession);

/*
** CAPI3REF: Create An Iterator To Traverse A Changeset
** CONSTRUCTOR: sqlite3_changeset_iter
**
** Create an iterator used to iterate through the contents of a changeset.
** If successful, *pp is set to point to the iterator handle and SQLITE_OK
** is returned. Otherwise, if an error occurs, *pp is set to zero and an
** SQLite error code is returned.
**
** The following functions can be used to advance and query a changeset
** iterator created by this function:
**
** <ul>
**   <li> [sqlite3changeset_next()]
**   <li> [sqlite3changeset_op()]
**   <li> [sqlite3changeset_new()]
**   <li> [sqlite3changeset_old()]
** </ul>
**
** It is the responsibility of the caller to eventually destroy the iterator
** by passing it to [sqlite3changeset_finalize()]. The buffer containing the
** changeset (pChangeset) must remain valid until after the iterator is
** destroyed.
**
** Assuming the changeset blob was created by one of the
** [sqlite3session_changeset()], [sqlite3changeset_concat()] or
** [sqlite3changeset_invert()] functions, all changes within the changeset
** that apply to a single table are grouped together. This means that when
** an application iterates through a changeset using an iterator created by
** this function, all changes that relate to a single table are visited
** consecutively. There is no chance that the iterator will visit a change
** the applies to table X, then one for table Y, and then later on visit
** another change for table X.
**
** The behavior of sqlite3changeset_start_v2() and its streaming equivalent
** may be modified by passing a combination of
** [SQLITE_CHANGESETSTART_INVERT | supported flags] as the 4th parameter.
**
** Note that the sqlite3changeset_start_v2() API is still <b>experimental</b>
** and therefore subject to change.
*/
SQLITE_API int sqlite3changeset_start(
  sqlite3_changeset_iter **pp,    /* OUT: New changeset iterator handle */
  int nChangeset,                 /* Size of changeset blob in bytes */
  void *pChangeset                /* Pointer to blob containing changeset */
);
SQLITE_API int sqlite3changeset_start_v2(
  sqlite3_changeset_iter **pp,    /* OUT: New changeset iterator handle */
  int nChangeset,                 /* Size of changeset blob in bytes */
  void *pChangeset,               /* Pointer to blob containing changeset */
  int flags                       /* SESSION_CHANGESETSTART_* flags */
);

/*
** CAPI3REF: Flags for sqlite3changeset_start_v2
**
** The following flags may passed via the 4th parameter to
** [sqlite3changeset_start_v2] and [sqlite3changeset_start_v2_strm]:
**
** <dt>SQLITE_CHANGESETAPPLY_INVERT <dd>
**   Invert the changeset while iterating through it. This is equivalent to
**   inverting a changeset using sqlite3changeset_invert() before applying it.
**   It is an error to specify this flag with a patchset.
*/
#define SQLITE_CHANGESETSTART_INVERT        0x0002


/*
** CAPI3REF: Advance A Changeset Iterator
** METHOD: sqlite3_changeset_iter
**
** This function may only be used with iterators created by the function
** [sqlite3changeset_start()]. If it is called on an iterator passed to
** a conflict-handler callback by [sqlite3changeset_apply()], SQLITE_MISUSE
** is returned and the call has no effect.
**
** Immediately after an iterator is created by sqlite3changeset_start(), it
** does not point to any change in the changeset. Assuming the changeset
** is not empty, the first call to this function advances the iterator to
** point to the first change in the changeset. Each subsequent call advances
** the iterator to point to the next change in the changeset (if any). If
** no error occurs and the iterator points to a valid change after a call
** to sqlite3changeset_next() has advanced it, SQLITE_ROW is returned.
** Otherwise, if all changes in the changeset have already been visited,
** SQLITE_DONE is returned.
**
** If an error occurs, an SQLite error code is returned. Possible error
** codes include SQLITE_CORRUPT (if the changeset buffer is corrupt) or
** SQLITE_NOMEM.
*/
SQLITE_API int sqlite3changeset_next(sqlite3_changeset_iter *pIter);

/*
** CAPI3REF: Obtain The Current Operation From A Changeset Iterator
** METHOD: sqlite3_changeset_iter
**
** The pIter argument passed to this function may either be an iterator
** passed to a conflict-handler by [sqlite3changeset_apply()], or an iterator
** created by [sqlite3changeset_start()]. In the latter case, the most recent
** call to [sqlite3changeset_next()] must have returned [SQLITE_ROW]. If this
** is not the case, this function returns [SQLITE_MISUSE].
**
** Arguments pOp, pnCol and pzTab may not be NULL. Upon return, three
** outputs are set through these pointers:
**
** *pOp is set to one of [SQLITE_INSERT], [SQLITE_DELETE] or [SQLITE_UPDATE],
** depending on the type of change that the iterator currently points to;
**
** *pnCol is set to the number of columns in the table affected by the change; and
**
** *pzTab is set to point to a nul-terminated utf-8 encoded string containing
** the name of the table affected by the current change. The buffer remains
** valid until either sqlite3changeset_next() is called on the iterator
** or until the conflict-handler function returns.
**
** If pbIndirect is not NULL, then *pbIndirect is set to true (1) if the change
** is an indirect change, or false (0) otherwise. See the documentation for
** [sqlite3session_indirect()] for a description of direct and indirect
** changes.
**
** If no error occurs, SQLITE_OK is returned. If an error does occur, an
** SQLite error code is returned. The values of the output variables may not
** be trusted in this case.
*/
SQLITE_API int sqlite3changeset_op(
  sqlite3_changeset_iter *pIter,  /* Iterator object */
  const char **pzTab,             /* OUT: Pointer to table name */
  int *pnCol,                     /* OUT: Number of columns in table */
  int *pOp,                       /* OUT: SQLITE_INSERT, DELETE or UPDATE */
  int *pbIndirect                 /* OUT: True for an 'indirect' change */
);

/*
** CAPI3REF: Obtain The Primary Key Definition Of A Table
** METHOD: sqlite3_changeset_iter
**
** For each modified table, a changeset includes the following:
**
** <ul>
**   <li> The number of columns in the table, and
**   <li> Which of those columns make up the tables PRIMARY KEY.
** </ul>
**
** This function is used to find which columns comprise the PRIMARY KEY of
** the table modified by the change that iterator pIter currently points to.
** If successful, *pabPK is set to point to an array of nCol entries, where
** nCol is the number of columns in the table. Elements of *pabPK are set to
** 0x01 if the corresponding column is part of the tables primary key, or
** 0x00 if it is not.
**
** If argument pnCol is not NULL, then *pnCol is set to the number of columns
** in the table.
**
** If this function is called when the iterator does not point to a valid
** entry, SQLITE_MISUSE is returned and the output variables zeroed. Otherwise,
** SQLITE_OK is returned and the output variables populated as described
** above.
*/
SQLITE_API int sqlite3changeset_pk(
  sqlite3_changeset_iter *pIter,  /* Iterator object */
  unsigned char **pabPK,          /* OUT: Array of boolean - true for PK cols */
  int *pnCol                      /* OUT: Number of entries in output array */
);

/*
** CAPI3REF: Obtain old.* Values From A Changeset Iterator
** METHOD: sqlite3_changeset_iter
**
** The pIter argument passed to this function may either be an iterator
** passed to a conflict-handler by [sqlite3changeset_apply()], or an iterator
** created by [sqlite3changeset_start()]. In the latter case, the most recent
** call to [sqlite3changeset_next()] must have returned SQLITE_ROW.
** Furthermore, it may only be called if the type of change that the iterator
** currently points to is either [SQLITE_DELETE] or [SQLITE_UPDATE]. Otherwise,
** this function returns [SQLITE_MISUSE] and sets *ppValue to NULL.
**
** Argument iVal must be greater than or equal to 0, and less than the number
** of columns in the table affected by the current change. Otherwise,
** [SQLITE_RANGE] is returned and *ppValue is set to NULL.
**
** If successful, this function sets *ppValue to point to a protected
** sqlite3_value object containing the iVal'th value from the vector of
** original row values stored as part of the UPDATE or DELETE change and
** returns SQLITE_OK. The name of the function comes from the fact that this
** is similar to the "old.*" columns available to update or delete triggers.
**
** If some other error occurs (e.g. an OOM condition), an SQLite error code
** is returned and *ppValue is set to NULL.
*/
SQLITE_API int sqlite3changeset_old(
