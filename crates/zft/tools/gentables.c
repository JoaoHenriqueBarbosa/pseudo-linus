/* Gera src/autofit/tables.rs a partir dos dados do próprio FreeType 2.13.3. */
#include <ft2build.h>
#include FT_FREETYPE_H
#include <stdio.h>
#include <string.h>

#define FT_LOCAL_ARRAY_DEF( x )  x
#include "aftypes.h"
#include "afblue.c"
#include "afranges.c"

#undef SCRIPT
#define SCRIPT( s, S, d, h, H, ss ) \
  dump_script( #s, af_##s##_uniranges, af_##s##_nonbase_uniranges, AF_##H, ss );

static void dump_ranges( const AF_Script_UniRangeRec* r ) {
  printf( "&[" );
  for ( ; r->first != 0; r++ ) printf( "(0x%lX, 0x%lX), ", r->first, r->last );
  printf( "]" );
}

static void print_escaped( const char* s ) {
  for ( const unsigned char* p = (const unsigned char*)s; *p; p++ ) {
    if ( *p < 0x80 ) {
      if ( *p == '"' || *p == '\\' ) putchar( '\\' );
      putchar( *p );
    } else {
      unsigned cp; int k;
      if ( *p >= 0xF0 ) { cp = *p & 7; k = 3; }
      else if ( *p >= 0xE0 ) { cp = *p & 15; k = 2; }
      else { cp = *p & 31; k = 1; }
      while ( k-- ) cp = ( cp << 6 ) | ( *++p & 63 );
      printf( "\\u{%X}", cp );
    }
  }
}

static void dump_script( const char* s, const AF_Script_UniRangeRec* u, const AF_Script_UniRangeRec* nb, int h,
                         const char* ss ) {
  printf( "    ScriptClass { name: \"%s\", ranges: ", s );
  dump_ranges( u );
  printf( ", nonbase: " );
  dump_ranges( nb );
  printf( ", top_to_bottom: %s, standard_chars: \"", h == AF_HINTING_TOP_TO_BOTTOM ? "true" : "false" );
  print_escaped( ss );
  printf( "\" },\n" );
}

int main( void ) {
  printf( "//! Tabelas do autohinter geradas a partir de `afscript.h`, `afstyles.h`, `afranges.c` e\n" );
  printf( "//! `afblue.c` do FreeType 2.13.3 (programa `gentables.c`, compilado contra o fonte).\n\n" );
  printf( "use super::{BlueString, ScriptClass, StyleClass};\n\n" );

  /* Os nomes dos enums, em ordem, viram índices. */
  printf( "pub static SCRIPTS: &[ScriptClass] = &[\n" );
#include "afscript.h"
  printf( "];\n\n" );

#undef STYLE
#define STYLE( s, S, d, ws, sc, ss, c ) \
  printf( "    StyleClass { name: \"%s\", writing_system: %d, script: %d, blue_stringset: %d, coverage: %d },\n", #s, ws, sc, ss, c );
  printf( "pub const COVERAGE_DEFAULT: u32 = %d;\n", AF_COVERAGE_DEFAULT );
  printf( "pub const WS_DUMMY: u32 = %d;\npub const WS_LATIN: u32 = %d;\npub const WS_CJK: u32 = %d;\npub const WS_INDIC: u32 = %d;\n",
          AF_WRITING_SYSTEM_DUMMY, AF_WRITING_SYSTEM_LATIN, AF_WRITING_SYSTEM_CJK, AF_WRITING_SYSTEM_INDIC );
  printf( "pub const SCRIPT_LATN: usize = %d;\npub const STYLE_NONE_DFLT: usize = %d;\npub const STYLE_LATN_DFLT: usize = %d;\n\n",
          AF_SCRIPT_LATN, AF_STYLE_NONE_DFLT, AF_STYLE_LATN_DFLT );
  printf( "pub static STYLES: &[StyleClass] = &[\n" );
#include "afstyles.h"
  printf( "];\n\n" );

  /* Strings azuis: tabela plana como `af_blue_stringsets`, com `text: ""` no lugar de */
  /* `AF_BLUE_STRING_MAX`; o `blue_stringset` dos estilos é o deslocamento nela.       */
  printf( "pub static BLUE_STRINGSETS: &[BlueString] = &[\n" );
  const AF_Blue_StringRec* r = af_blue_stringsets;
  int n = sizeof( af_blue_stringsets ) / sizeof( af_blue_stringsets[0] );
  for ( int i = 0; i < n; i++ ) {
    if ( r[i].string == AF_BLUE_STRING_MAX ) {
      printf( "    BlueString { text: \"\", properties: 0 },\n" );
      continue;
    }
    printf( "    BlueString { text: \"" );
    print_escaped( af_blue_strings + r[i].string );
    printf( "\", properties: %u },\n", r[i].properties );
  }
  printf( "];\n" );
  return 0;
}
