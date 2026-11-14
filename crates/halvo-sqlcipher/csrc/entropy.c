#include <stdlib.h>
#include <tomcrypt.h>

int sqlcipher_extra_init(const char *arg);

#if defined(_WIN32)
#include <windows.h>
#include <bcrypt.h>
static int os_entropy(unsigned char *out, unsigned long len) {
  return BCryptGenRandom(NULL, out, (ULONG)len, BCRYPT_USE_SYSTEM_PREFERRED_RNG) == 0;
}
#else
#include <sys/random.h>
#define GETENTROPY_MAX 256
static int os_entropy(unsigned char *out, unsigned long len) {
  while (len > 0) {
    unsigned long n = len < GETENTROPY_MAX ? len : GETENTROPY_MAX;
    if (getentropy(out, n) != 0) return 0;
    out += n;
    len -= n;
  }
  return 1;
}
#endif

static unsigned long halvo_ltc_rng(unsigned char *out, unsigned long len, void (*callback)(void)) {
  (void)callback;
  if (!os_entropy(out, len)) abort();
  return len;
}

int halvo_sqlcipher_extra_init(const char *arg) {
  ltc_rng = halvo_ltc_rng;
  return sqlcipher_extra_init(arg);
}

int halvo_sqlcipher_os_entropy_hook_installed(void) {
  return ltc_rng == halvo_ltc_rng;
}
