// Gera src/sets.rs: os conjuntos de caracteres dos motores de dicionário (`dictbe.cpp`), avaliados
// pelo ICU4C do sistema (78, Unicode 17), em faixas fechadas.
//
//   cc -O2 -o gensets tools/gensets.c $(pkg-config --cflags --libs icu-uc) && ./gensets > src/sets.rs
#include <stdio.h>
#include <unicode/uset.h>
#include <unicode/ustring.h>

static void emit(const char *name, const char *pattern) {
    UChar buf[256];
    u_uastrcpy(buf, pattern);
    UErrorCode st = U_ZERO_ERROR;
    USet *set = uset_openPattern(buf, -1, &st);
    if (U_FAILURE(st)) {
        fprintf(stderr, "%s: %s\n", pattern, u_errorName(st));
        return;
    }
    printf("/// `%s`.\npub const %s: &[(u32, u32)] = &[", pattern, name);
    int32_t n = uset_getRangeCount(set);
    for (int32_t i = 0; i < n; i++) {
        UChar32 start, end;
        uset_getItem(set, i, &start, &end, NULL, 0, &st);
        printf("%s(0x%04X, 0x%04X)", i ? ", " : "", start, end);
    }
    printf("];\n\n");
    uset_close(set);
}

int main(void) {
    printf("//! Gerado por `tools/gensets.c` a partir do ICU4C do sistema; não editar à mão.\n\n");
    emit("THAI_WORD", "[[:Thai:]&[:LineBreak=SA:]]");
    emit("THAI_MARK", "[[:Thai:]&[:LineBreak=SA:]&[:M:]]");
    emit("LAO_WORD", "[[:Laoo:]&[:LineBreak=SA:]]");
    emit("LAO_MARK", "[[:Laoo:]&[:LineBreak=SA:]&[:M:]]");
    emit("BURMESE_WORD", "[[:Mymr:]&[:LineBreak=SA:]]");
    emit("BURMESE_MARK", "[[:Mymr:]&[:LineBreak=SA:]&[:M:]]");
    emit("KHMER_WORD", "[[:Khmr:]&[:LineBreak=SA:]]");
    emit("KHMER_MARK", "[[:Khmr:]&[:LineBreak=SA:]&[:M:]]");
    return 0;
}
