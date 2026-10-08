** Routines to read or write a two- and four-byte big-endian integer values.
*/
#define get2byte(x)   ((x)[0]<<8 | (x)[1])
#define put2byte(p,v) ((p)[0] = (u8)((v)>>8), (p)[1] = (u8)(v))
#define get4byte sqlite3Get4byte
#define put4byte sqlite3Put4byte

/*
** get2byteAligned(), unlike get2byte(), requires that its argument point to a
** two-byte aligned address.  get2byteAligned() is only used for accessing the
** cell addresses in a btree header.
*/
#if SQLITE_BYTEORDER==4321
# define get2byteAligned(x)  (*(u16*)(x))
#elif SQLITE_BYTEORDER==1234 && GCC_VERSION>=4008000
# define get2byteAligned(x)  __builtin_bswap16(*(u16*)(x))
#elif SQLITE_BYTEORDER==1234 && MSVC_VERSION>=1300
# define get2byteAligned(x)  _byteswap_ushort(*(u16*)(x))
#else
# define get2byteAligned(x)  ((x)[0]<<8 | (x)[1])
#endif

