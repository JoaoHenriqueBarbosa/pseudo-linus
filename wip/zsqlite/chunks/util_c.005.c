
/*
** High-resolution hardware timer used for debugging and testing only.
*/
#if defined(VDBE_PROFILE)  \
 || defined(SQLITE_PERFORMANCE_TRACE) \
 || defined(SQLITE_ENABLE_STMT_SCANSTATUS)
/************** Include hwtime.h in the middle of util.c *********************/
#endif

