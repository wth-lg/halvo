#include <fcntl.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/param.h>
#include <sys/stat.h>
#include <unistd.h>

#define INTERPOSE(replacement, original)                                      \
  __attribute__((used)) static struct {                                       \
    const void *r;                                                            \
    const void *o;                                                            \
  } interpose_##original __attribute__((section("__DATA,__interpose"))) = {   \
      (const void *)(unsigned long)&replacement,                              \
      (const void *)(unsigned long)&original}

static int inited = 0;
static int armed = 0;
static long count = 0;
static long crash_at = 0;
static const char *dir_mark = NULL;
static int trace_fd = -1;

static void init(void) {
  if (inited) return;
  inited = 1;
  dir_mark = getenv("HALVO_IT_DIR");
  const char *at = getenv("HALVO_IT_CRASH_AT");
  crash_at = at ? atol(at) : 0;
  const char *t = getenv("HALVO_IT_TRACE");
  if (t) trace_fd = open(t, O_WRONLY | O_CREAT | O_APPEND, 0644);
}

static const char *base(const char *p) {
  const char *s = strrchr(p, '/');
  return s ? s + 1 : p;
}

static void point(const char *op, const char *path, long bytes) {
  init();
  if (!dir_mark || !path) return;
  const char *at = strstr(path, dir_mark);
  if (!at) return;
  if (!armed) {
    if (strcmp(base(path), "ARM") == 0) armed = 1;
    return;
  }
  count++;
  if (crash_at > 0 && count == crash_at) _exit(77);
  if (trace_fd >= 0) {
    char line[MAXPATHLEN + 96];
    const char *rel = at + strlen(dir_mark);
    int n = snprintf(line, sizeof line, "%ld %s %s %ld\n", count, op,
                     *rel ? rel : "/", bytes);
    if (n > 0) write(trace_fd, line, (size_t)n);
  }
}

static void point_fd(const char *op, int fd, long bytes) {
  char path[MAXPATHLEN];
  if (fcntl(fd, F_GETPATH, path) == -1) return;
  point(op, path, bytes);
}

ssize_t it_write(int fd, const void *buf, size_t n) {
  point_fd("write", fd, (long)n);
  return write(fd, buf, n);
}

ssize_t it_pwrite(int fd, const void *buf, size_t n, off_t off) {
  point_fd("pwrite", fd, (long)n);
  return pwrite(fd, buf, n, off);
}

int it_ftruncate(int fd, off_t len) {
  point_fd("ftruncate", fd, (long)len);
  return ftruncate(fd, len);
}

int it_fsync(int fd) {
  point_fd("fsync", fd, 0);
  return fsync(fd);
}

int it_fcntl(int fd, int cmd, ...) {
  va_list ap;
  va_start(ap, cmd);
  long arg = va_arg(ap, long);
  va_end(ap);
  if (cmd == F_FULLFSYNC) point_fd("F_FULLFSYNC", fd, 0);
  else if (cmd == F_BARRIERFSYNC) point_fd("F_BARRIERFSYNC", fd, 0);
  return fcntl(fd, cmd, arg);
}

int it_rename(const char *a, const char *b) {
  point("rename", b, 0);
  return rename(a, b);
}

int it_unlink(const char *p) {
  point("unlink", p, 0);
  return unlink(p);
}

int it_unlinkat(int dfd, const char *p, int flag) {
  char dir[MAXPATHLEN];
  char full[MAXPATHLEN * 2];
  if (p && p[0] != '/' && dfd != AT_FDCWD && fcntl(dfd, F_GETPATH, dir) != -1) {
    snprintf(full, sizeof full, "%s/%s", dir, p);
    point("unlinkat", full, 0);
  } else {
    point("unlinkat", p, 0);
  }
  return unlinkat(dfd, p, flag);
}

static void point_at(const char *op, int dfd, const char *p) {
  char dir[MAXPATHLEN];
  char full[MAXPATHLEN * 2];
  if (p && p[0] != '/' && dfd != AT_FDCWD && fcntl(dfd, F_GETPATH, dir) != -1) {
    snprintf(full, sizeof full, "%s/%s", dir, p);
    point(op, full, 0);
  } else {
    point(op, p, 0);
  }
}

int it_renameat(int fdfrom, const char *from, int fdto, const char *to) {
  point_at("renameat", fdto, to);
  return renameat(fdfrom, from, fdto, to);
}

int it_linkat(int fdfrom, const char *from, int fdto, const char *to, int flag) {
  point_at("linkat", fdto, to);
  return linkat(fdfrom, from, fdto, to, flag);
}

int it_mkdirat(int dfd, const char *p, mode_t m) {
  point_at("mkdirat", dfd, p);
  return mkdirat(dfd, p, m);
}

int it_fchmod(int fd, mode_t m) {
  point_fd("fchmod", fd, 0);
  return fchmod(fd, m);
}

int it_rmdir(const char *p) {
  point("rmdir", p, 0);
  return rmdir(p);
}

int it_mkdir(const char *p, mode_t m) {
  point("mkdir", p, 0);
  return mkdir(p, m);
}

INTERPOSE(it_write, write);
INTERPOSE(it_pwrite, pwrite);
INTERPOSE(it_ftruncate, ftruncate);
INTERPOSE(it_fsync, fsync);
INTERPOSE(it_fcntl, fcntl);
INTERPOSE(it_rename, rename);
INTERPOSE(it_unlink, unlink);
INTERPOSE(it_unlinkat, unlinkat);
INTERPOSE(it_rmdir, rmdir);
INTERPOSE(it_mkdir, mkdir);
INTERPOSE(it_renameat, renameat);
INTERPOSE(it_linkat, linkat);
INTERPOSE(it_mkdirat, mkdirat);
INTERPOSE(it_fchmod, fchmod);
