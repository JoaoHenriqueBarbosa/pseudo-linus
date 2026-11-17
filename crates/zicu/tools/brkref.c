// Oráculo dos testes do zicu: roda o RuleBasedBreakIterator do ICU4C do sistema sobre as mesmas
// tabelas binárias (`ubrk_openBinaryRules`) e imprime, para cada linha da entrada, as fronteiras
// para a frente com o status da regra, e following, preceding e isBoundary em toda posição.
//
// A entrada tem um texto por linha, em UTF-8; `\uXXXX` vira a unidade UTF-16 dada (serve para
// surrogates soltos), `\n` vira LF e `\\` vira barra.
//
// O segundo argumento, opcional, é um pacote de dados do ICU (`icudtl.dat`) que substitui o do
// sistema antes de qualquer uso: os dicionários vêm dele. Sem ele, valem os do sistema.
//
//   cc -O2 -o brkref tools/brkref.c $(pkg-config --cflags --libs icu-uc)
//   ./brkref tests/data/line_normal.brk icudtl.dat < tests/data/texts.txt > tests/data/line_normal.golden
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unicode/ubrk.h>
#include <unicode/udata.h>
#include <unicode/ustring.h>

static int decode(const char *line, UChar *out, int cap) {
    UChar tmp[8192];
    int n = 0;
    char buf[8192];
    int b = 0;
    for (const char *p = line; *p && *p != '\n';) {
        if (p[0] == '\\' && p[1] == 'u') {
            buf[b] = 0;
            int32_t len = 0;
            UErrorCode st = U_ZERO_ERROR;
            u_strFromUTF8(tmp, 8192, &len, buf, b, &st);
            memcpy(out + n, tmp, len * sizeof(UChar));
            n += len;
            b = 0;
            char hex[5] = {p[2], p[3], p[4], p[5], 0};
            out[n++] = (UChar)strtol(hex, NULL, 16);
            p += 6;
        } else if (p[0] == '\\' && p[1] == 'n') {
            buf[b++] = '\n';
            p += 2;
        } else if (p[0] == '\\' && p[1] == '\\') {
            buf[b++] = '\\';
            p += 2;
        } else {
            buf[b++] = *p++;
        }
    }
    buf[b] = 0;
    int32_t len = 0;
    UErrorCode st = U_ZERO_ERROR;
    u_strFromUTF8(tmp, 8192, &len, buf, b, &st);
    memcpy(out + n, tmp, len * sizeof(UChar));
    n += len;
    (void)cap;
    return n;
}

int main(int argc, char **argv) {
    if (argc != 2 && argc != 3) {
        fprintf(stderr, "uso: brkref arquivo.brk [icudtl.dat] < textos\n");
        return 2;
    }
    if (argc == 3) {
        FILE *c = fopen(argv[2], "rb");
        fseek(c, 0, SEEK_END);
        long csize = ftell(c);
        fseek(c, 0, SEEK_SET);
        void *common = malloc(csize);
        fread(common, 1, csize, c);
        fclose(c);
        UErrorCode cst = U_ZERO_ERROR;
        udata_setCommonData(common, &cst);
        if (U_FAILURE(cst)) {
            fprintf(stderr, "udata_setCommonData: %s\n", u_errorName(cst));
            return 1;
        }
    }
    FILE *f = fopen(argv[1], "rb");
    fseek(f, 0, SEEK_END);
    long size = ftell(f);
    fseek(f, 0, SEEK_SET);
    unsigned char *data = malloc(size);
    fread(data, 1, size, f);
    fclose(f);
    // O arquivo do pacote começa pelo cabeçalho do udata; as tabelas vêm depois dele.
    int header = data[0] | (data[1] << 8);
    UErrorCode st = U_ZERO_ERROR;
    UBreakIterator *bi = ubrk_openBinaryRules(data + header, (int32_t)(size - header), NULL, 0, &st);
    if (U_FAILURE(st)) {
        fprintf(stderr, "ubrk_openBinaryRules: %s\n", u_errorName(st));
        return 1;
    }
    static char line[65536];
    static UChar text[65536];
    while (fgets(line, sizeof line, stdin)) {
        int n = decode(line, text, 65536);
        st = U_ZERO_ERROR;
        ubrk_setText(bi, text, n, &st);
        printf("N");
        for (int32_t p = ubrk_first(bi); p != UBRK_DONE; p = ubrk_next(bi)) {
            printf(" %d:%d", p, ubrk_getRuleStatus(bi));
        }
        printf("\nF");
        for (int i = -1; i <= n + 1; i++) {
            printf(" %d", ubrk_following(bi, i));
        }
        printf("\nP");
        for (int i = -1; i <= n + 1; i++) {
            printf(" %d", ubrk_preceding(bi, i));
        }
        printf("\nI");
        for (int i = -1; i <= n + 1; i++) {
            printf(" %d", ubrk_isBoundary(bi, i));
        }
        printf("\n");
    }
    ubrk_close(bi);
    return 0;
}
