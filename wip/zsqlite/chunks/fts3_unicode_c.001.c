
/*
** Set *ppModule to a pointer to the sqlite3_tokenizer_module
** structure for the unicode tokenizer.
*/
SQLITE_PRIVATE void sqlite3Fts3UnicodeTokenizer(sqlite3_tokenizer_module const **ppModule){
  static const sqlite3_tokenizer_module module = {
    0,
    unicodeCreate,
    unicodeDestroy,
    unicodeOpen,
    unicodeClose,
    unicodeNext,
    0,
  };
  *ppModule = &module;
}

#endif /* !defined(SQLITE_CORE) || defined(SQLITE_ENABLE_FTS3) */
#endif /* ifndef SQLITE_DISABLE_FTS3_UNICODE */

