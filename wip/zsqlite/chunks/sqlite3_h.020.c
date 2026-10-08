
/*
** CAPI3REF: Virtual Table Indexing Information
** KEYWORDS: sqlite3_index_info
**
** The sqlite3_index_info structure and its substructures is used as part
** of the [virtual table] interface to
** pass information into and receive the reply from the [xBestIndex]
** method of a [virtual table module].  The fields under **Inputs** are the
** inputs to xBestIndex and are read-only.  xBestIndex inserts its
** results into the **Outputs** fields.
**
** ^(The aConstraint[] array records WHERE clause constraints of the form:
**
** <blockquote>column OP expr</blockquote>
**
** where OP is =, &lt;, &lt;=, &gt;, or &gt;=.)^  ^(The particular operator is
** stored in aConstraint[].op using one of the
** [SQLITE_INDEX_CONSTRAINT_EQ | SQLITE_INDEX_CONSTRAINT_ values].)^
** ^(The index of the column is stored in
** aConstraint[].iColumn.)^  ^(aConstraint[].usable is TRUE if the
** expr on the right-hand side can be evaluated (and thus the constraint
** is usable) and false if it cannot.)^
**
** ^The optimizer automatically inverts terms of the form "expr OP column"
** and makes other simplifications to the WHERE clause in an attempt to
** get as many WHERE clause terms into the form shown above as possible.
** ^The aConstraint[] array only reports WHERE clause terms that are
** relevant to the particular virtual table being queried.
**
** ^Information about the ORDER BY clause is stored in aOrderBy[].
** ^Each term of aOrderBy records a column of the ORDER BY clause.
**
** The colUsed field indicates which columns of the virtual table may be
** required by the current scan. Virtual table columns are numbered from
** zero in the order in which they appear within the CREATE TABLE statement
** passed to sqlite3_declare_vtab(). For the first 63 columns (columns 0-62),
** the corresponding bit is set within the colUsed mask if the column may be
** required by SQLite. If the table has at least 64 columns and any column
** to the right of the first 63 is required, then bit 63 of colUsed is also
** set. In other words, column iCol may be required if the expression
** (colUsed & ((sqlite3_uint64)1 << (iCol>=63 ? 63 : iCol))) evaluates to
** non-zero.
**
** The [xBestIndex] method must fill aConstraintUsage[] with information
** about what parameters to pass to xFilter.  ^If argvIndex>0 then
** the right-hand side of the corresponding aConstraint[] is evaluated
** and becomes the argvIndex-th entry in argv.  ^(If aConstraintUsage[].omit
** is true, then the constraint is assumed to be fully handled by the
** virtual table and might not be checked again by the byte code.)^ ^(The
** aConstraintUsage[].omit flag is an optimization hint. When the omit flag
** is left in its default setting of false, the constraint will always be
** checked separately in byte code.  If the omit flag is change to true, then
** the constraint may or may not be checked in byte code.  In other words,
** when the omit flag is true there is no guarantee that the constraint will
** not be checked again using byte code.)^
**
** ^The idxNum and idxStr values are recorded and passed into the
** [xFilter] method.
** ^[sqlite3_free()] is used to free idxStr if and only if
** needToFreeIdxStr is true.
**
** ^The orderByConsumed means that output from [xFilter]/[xNext] will occur in
** the correct order to satisfy the ORDER BY clause so that no separate
** sorting step is required.
**
** ^The estimatedCost value is an estimate of the cost of a particular
** strategy. A cost of N indicates that the cost of the strategy is similar
** to a linear scan of an SQLite table with N rows. A cost of log(N)
** indicates that the expense of the operation is similar to that of a
** binary search on a unique indexed field of an SQLite table with N rows.
**
** ^The estimatedRows value is an estimate of the number of rows that
** will be returned by the strategy.
**
** The xBestIndex method may optionally populate the idxFlags field with a
** mask of SQLITE_INDEX_SCAN_* flags. Currently there is only one such flag -
** SQLITE_INDEX_SCAN_UNIQUE. If the xBestIndex method sets this flag, SQLite
** assumes that the strategy may visit at most one row.
**
** Additionally, if xBestIndex sets the SQLITE_INDEX_SCAN_UNIQUE flag, then
** SQLite also assumes that if a call to the xUpdate() method is made as
** part of the same statement to delete or update a virtual table row and the
** implementation returns SQLITE_CONSTRAINT, then there is no need to rollback
** any database changes. In other words, if the xUpdate() returns
** SQLITE_CONSTRAINT, the database contents must be exactly as they were
** before xUpdate was called. By contrast, if SQLITE_INDEX_SCAN_UNIQUE is not
** set and xUpdate returns SQLITE_CONSTRAINT, any database changes made by
** the xUpdate method are automatically rolled back by SQLite.
**
** IMPORTANT: The estimatedRows field was added to the sqlite3_index_info
** structure for SQLite [version 3.8.2] ([dateof:3.8.2]).
** If a virtual table extension is
** used with an SQLite version earlier than 3.8.2, the results of attempting
** to read or write the estimatedRows field are undefined (but are likely
** to include crashing the application). The estimatedRows field should
** therefore only be used if [sqlite3_libversion_number()] returns a
** value greater than or equal to 3008002. Similarly, the idxFlags field
** was added for [version 3.9.0] ([dateof:3.9.0]).
** It may therefore only be used if
** sqlite3_libversion_number() returns a value greater than or equal to
** 3009000.
*/
struct sqlite3_index_info {
  /* Inputs */
  int nConstraint;           /* Number of entries in aConstraint */
  struct sqlite3_index_constraint {
     int iColumn;              /* Column constrained.  -1 for ROWID */
     unsigned char op;         /* Constraint operator */
     unsigned char usable;     /* True if this constraint is usable */
     int iTermOffset;          /* Used internally - xBestIndex should ignore */
  } *aConstraint;            /* Table of WHERE clause constraints */
  int nOrderBy;              /* Number of terms in the ORDER BY clause */
  struct sqlite3_index_orderby {
     int iColumn;              /* Column number */
     unsigned char desc;       /* True for DESC.  False for ASC. */
  } *aOrderBy;               /* The ORDER BY clause */
  /* Outputs */
  struct sqlite3_index_constraint_usage {
    int argvIndex;           /* if >0, constraint is part of argv to xFilter */
    unsigned char omit;      /* Do not code a test for this constraint */
  } *aConstraintUsage;
  int idxNum;                /* Number used to identify the index */
  char *idxStr;              /* String, possibly obtained from sqlite3_malloc */
  int needToFreeIdxStr;      /* Free idxStr using sqlite3_free() if true */
  int orderByConsumed;       /* True if output is already ordered */
  double estimatedCost;           /* Estimated cost of using this index */
  /* Fields below are only available in SQLite 3.8.2 and later */
  sqlite3_int64 estimatedRows;    /* Estimated number of rows returned */
  /* Fields below are only available in SQLite 3.9.0 and later */
  int idxFlags;              /* Mask of SQLITE_INDEX_SCAN_* flags */
  /* Fields below are only available in SQLite 3.10.0 and later */
  sqlite3_uint64 colUsed;    /* Input: Mask of columns used by statement */
};

/*
** CAPI3REF: Virtual Table Scan Flags
**
** Virtual table implementations are allowed to set the
** [sqlite3_index_info].idxFlags field to some combination of
** these bits.
*/
#define SQLITE_INDEX_SCAN_UNIQUE      1     /* Scan visits at most 1 row */

/*
** CAPI3REF: Virtual Table Constraint Operator Codes
**
** These macros define the allowed values for the
** [sqlite3_index_info].aConstraint[].op field.  Each value represents
** an operator that is part of a constraint term in the WHERE clause of
** a query that uses a [virtual table].
**
** ^The left-hand operand of the operator is given by the corresponding
** aConstraint[].iColumn field.  ^An iColumn of -1 indicates the left-hand
** operand is the rowid.
** The SQLITE_INDEX_CONSTRAINT_LIMIT and SQLITE_INDEX_CONSTRAINT_OFFSET
** operators have no left-hand operand, and so for those operators the
** corresponding aConstraint[].iColumn is meaningless and should not be
** used.
**
** All operator values from SQLITE_INDEX_CONSTRAINT_FUNCTION through
** value 255 are reserved to represent functions that are overloaded
** by the [xFindFunction|xFindFunction method] of the virtual table
** implementation.
**
** The right-hand operands for each constraint might be accessible using
** the [sqlite3_vtab_rhs_value()] interface.  Usually the right-hand
** operand is only available if it appears as a single constant literal
** in the input SQL.  If the right-hand operand is another column or an
** expression (even a constant expression) or a parameter, then the
** sqlite3_vtab_rhs_value() probably will not be able to extract it.
** ^The SQLITE_INDEX_CONSTRAINT_ISNULL and
** SQLITE_INDEX_CONSTRAINT_ISNOTNULL operators have no right-hand operand
** and hence calls to sqlite3_vtab_rhs_value() for those operators will
** always return SQLITE_NOTFOUND.
**
** The collating sequence to be used for comparison can be found using
** the [sqlite3_vtab_collation()] interface.  For most real-world virtual
** tables, the collating sequence of constraints does not matter (for example
** because the constraints are numeric) and so the sqlite3_vtab_collation()
** interface is not commonly needed.
*/
#define SQLITE_INDEX_CONSTRAINT_EQ          2
#define SQLITE_INDEX_CONSTRAINT_GT          4
#define SQLITE_INDEX_CONSTRAINT_LE          8
#define SQLITE_INDEX_CONSTRAINT_LT         16
#define SQLITE_INDEX_CONSTRAINT_GE         32
#define SQLITE_INDEX_CONSTRAINT_MATCH      64
#define SQLITE_INDEX_CONSTRAINT_LIKE       65
#define SQLITE_INDEX_CONSTRAINT_GLOB       66
#define SQLITE_INDEX_CONSTRAINT_REGEXP     67
#define SQLITE_INDEX_CONSTRAINT_NE         68
#define SQLITE_INDEX_CONSTRAINT_ISNOT      69
#define SQLITE_INDEX_CONSTRAINT_ISNOTNULL  70
#define SQLITE_INDEX_CONSTRAINT_ISNULL     71
#define SQLITE_INDEX_CONSTRAINT_IS         72
#define SQLITE_INDEX_CONSTRAINT_LIMIT      73
#define SQLITE_INDEX_CONSTRAINT_OFFSET     74
#define SQLITE_INDEX_CONSTRAINT_FUNCTION  150

/*
** CAPI3REF: Register A Virtual Table Implementation
** METHOD: sqlite3
**
** ^These routines are used to register a new [virtual table module] name.
** ^Module names must be registered before
** creating a new [virtual table] using the module and before using a
** preexisting [virtual table] for the module.
**
** ^The module name is registered on the [database connection] specified
** by the first parameter.  ^The name of the module is given by the
** second parameter.  ^The third parameter is a pointer to
** the implementation of the [virtual table module].   ^The fourth
** parameter is an arbitrary client data pointer that is passed through
** into the [xCreate] and [xConnect] methods of the virtual table module
** when a new virtual table is be being created or reinitialized.
**
** ^The sqlite3_create_module_v2() interface has a fifth parameter which
** is a pointer to a destructor for the pClientData.  ^SQLite will
** invoke the destructor function (if it is not NULL) when SQLite
** no longer needs the pClientData pointer.  ^The destructor will also
** be invoked if the call to sqlite3_create_module_v2() fails.
** ^The sqlite3_create_module()
** interface is equivalent to sqlite3_create_module_v2() with a NULL
** destructor.
**
** ^If the third parameter (the pointer to the sqlite3_module object) is
** NULL then no new module is created and any existing modules with the
** same name are dropped.
**
** See also: [sqlite3_drop_modules()]
*/
SQLITE_API int sqlite3_create_module(
  sqlite3 *db,               /* SQLite connection to register module with */
  const char *zName,         /* Name of the module */
  const sqlite3_module *p,   /* Methods for the module */
  void *pClientData          /* Client data for xCreate/xConnect */
);
SQLITE_API int sqlite3_create_module_v2(
  sqlite3 *db,               /* SQLite connection to register module with */
  const char *zName,         /* Name of the module */
  const sqlite3_module *p,   /* Methods for the module */
  void *pClientData,         /* Client data for xCreate/xConnect */
  void(*xDestroy)(void*)     /* Module destructor function */
);

/*
** CAPI3REF: Remove Unnecessary Virtual Table Implementations
** METHOD: sqlite3
**
** ^The sqlite3_drop_modules(D,L) interface removes all virtual
** table modules from database connection D except those named on list L.
** The L parameter must be either NULL or a pointer to an array of pointers
** to strings where the array is terminated by a single NULL pointer.
** ^If the L parameter is NULL, then all virtual table modules are removed.
**
** See also: [sqlite3_create_module()]
*/
SQLITE_API int sqlite3_drop_modules(
  sqlite3 *db,                /* Remove modules from this connection */
  const char **azKeep         /* Except, do not remove the ones named here */
);

/*
** CAPI3REF: Virtual Table Instance Object
** KEYWORDS: sqlite3_vtab
**
** Every [virtual table module] implementation uses a subclass
** of this object to describe a particular instance
** of the [virtual table].  Each subclass will
** be tailored to the specific needs of the module implementation.
** The purpose of this superclass is to define certain fields that are
** common to all module implementations.
**
** ^Virtual tables methods can set an error message by assigning a
** string obtained from [sqlite3_mprintf()] to zErrMsg.  The method should
** take care that any prior string is freed by a call to [sqlite3_free()]
** prior to assigning a new string to zErrMsg.  ^After the error message
** is delivered up to the client application, the string will be automatically
** freed by sqlite3_free() and the zErrMsg field will be zeroed.
*/
struct sqlite3_vtab {
  const sqlite3_module *pModule;  /* The module for this virtual table */
  int nRef;                       /* Number of open cursors */
  char *zErrMsg;                  /* Error message from sqlite3_mprintf() */
  /* Virtual table implementations will typically add additional fields */
};

/*
** CAPI3REF: Virtual Table Cursor Object
** KEYWORDS: sqlite3_vtab_cursor {virtual table cursor}
**
** Every [virtual table module] implementation uses a subclass of the
** following structure to describe cursors that point into the
** [virtual table] and are used
** to loop through the virtual table.  Cursors are created using the
** [sqlite3_module.xOpen | xOpen] method of the module and are destroyed
** by the [sqlite3_module.xClose | xClose] method.  Cursors are used
** by the [xFilter], [xNext], [xEof], [xColumn], and [xRowid] methods
** of the module.  Each module implementation will define
** the content of a cursor structure to suit its own needs.
**
** This superclass exists in order to define fields of the cursor that
** are common to all implementations.
*/
struct sqlite3_vtab_cursor {
  sqlite3_vtab *pVtab;      /* Virtual table of this cursor */
  /* Virtual table implementations will typically add additional fields */
};

/*
** CAPI3REF: Declare The Schema Of A Virtual Table
**
** ^The [xCreate] and [xConnect] methods of a
** [virtual table module] call this interface
** to declare the format (the names and datatypes of the columns) of
** the virtual tables they implement.
*/
SQLITE_API int sqlite3_declare_vtab(sqlite3*, const char *zSQL);

/*
** CAPI3REF: Overload A Function For A Virtual Table
** METHOD: sqlite3
**
** ^(Virtual tables can provide alternative implementations of functions
** using the [xFindFunction] method of the [virtual table module].
** But global versions of those functions
** must exist in order to be overloaded.)^
**
** ^(This API makes sure a global version of a function with a particular
** name and number of parameters exists.  If no such function exists
** before this API is called, a new function is created.)^  ^The implementation
** of the new function always causes an exception to be thrown.  So
** the new function is not good for anything by itself.  Its only
** purpose is to be a placeholder function that can be overloaded
** by a [virtual table].
*/
SQLITE_API int sqlite3_overload_function(sqlite3*, const char *zFuncName, int nArg);

/*
** CAPI3REF: A Handle To An Open BLOB
** KEYWORDS: {BLOB handle} {BLOB handles}
**
** An instance of this object represents an open BLOB on which
** [sqlite3_blob_open | incremental BLOB I/O] can be performed.
** ^Objects of this type are created by [sqlite3_blob_open()]
** and destroyed by [sqlite3_blob_close()].
** ^The [sqlite3_blob_read()] and [sqlite3_blob_write()] interfaces
** can be used to read or write small subsections of the BLOB.
** ^The [sqlite3_blob_bytes()] interface returns the size of the BLOB in bytes.
