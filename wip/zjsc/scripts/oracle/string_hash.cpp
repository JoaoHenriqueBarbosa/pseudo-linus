#include "config.h"
#include <wtf/text/StringHasherInlines.h>
#include <cstdio>
int main(){
  unsigned char b[256]; for(int i=0;i<256;i++) b[i]=(unsigned char)(i*37+11);
  for(int n=0;n<=40;n++) printf("L %d %u\n", n, WTF::StringHasher::computeHashAndMaskTop8Bits<Latin1Character>(std::span<const Latin1Character>(b,n)));
  char16_t u[64]; for(int i=0;i<64;i++) u[i]=(char16_t)(0x100+i*977);
  for(int n=0;n<=40;n++) printf("U %d %u\n", n, WTF::StringHasher::computeHashAndMaskTop8Bits<char16_t>(std::span<const char16_t>(u,n)));
}
