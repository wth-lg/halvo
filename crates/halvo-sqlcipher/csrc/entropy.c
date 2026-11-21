#include <stdlib.h>
#include <tomcrypt.h>
#include "sqlite3.h"

#if !defined(LTC_CLEAN_STACK)
#error "halvo-sqlcipher: the build stops: LTC_CLEAN_STACK is not defined, so LibTomCrypt leaves its stack and its HMAC state unwiped"
#endif

int sqlcipher_extra_init(const char *arg);
int halvo_sqlcipher_page_gap_mend(void);

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

static void halvo_page_gap_mend_sql(sqlite3_context *context, int argc, sqlite3_value **argv) {
  (void)argc;
  (void)argv;
  sqlite3_result_int(context, halvo_sqlcipher_page_gap_mend());
}

static int halvo_register_page_gap_mend(sqlite3 *db, char **error, const sqlite3_api_routines *api) {
  (void)error;
  (void)api;
  return sqlite3_create_function_v2(db, "halvo_sqlcipher_page_gap_mend", 0,
                                    SQLITE_UTF8 | SQLITE_DETERMINISTIC | SQLITE_INNOCUOUS, NULL,
                                    halvo_page_gap_mend_sql, NULL, NULL, NULL);
}

int halvo_sqlcipher_extra_init(const char *arg) {
  int rc;
  ltc_rng = halvo_ltc_rng;
  rc = sqlcipher_extra_init(arg);
  if (rc != SQLITE_OK) return rc;
  return sqlite3_auto_extension((void (*)(void))halvo_register_page_gap_mend);
}

int halvo_sqlcipher_os_entropy_hook_installed(void) {
  return ltc_rng == halvo_ltc_rng;
}
