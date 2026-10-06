/* What keeps this module's WASI imports inside the set the host allows.
 *
 * PUC Lua's C is compiled whole, and the parts the sandbox never opens still
 * reach libc at link time. Two routes do it. `luaopen_io`, `luaopen_os`,
 * `luaopen_package` and `luaopen_debug` are all named in mlua's stdlib
 * loader, which chooses between them at run time, so each is linked whatever
 * the VM opens; `lauxlib.c`'s `luaL_loadfilex` reaches `fopen` on behalf of
 * the `dofile` and `loadfile` the sandbox removes; and `loslib.c`, which *is*
 * opened for `os.time` and `os.date`, carries `os.remove` and `os.rename`
 * whether or not the sandbox keeps them. Between them they bring in
 * `path_open`, `path_rename`, `path_unlink_file`, `path_remove_directory`,
 * `fd_read`, `fd_seek`, `fd_close`, `fd_renumber` and
 * `fd_fdstat_set_flags`, every one of which the host refuses.
 *
 * Defining those entry points here keeps libc's own out. An archive member is
 * extracted only to satisfy a symbol still undefined when the linker reaches
 * it, so a definition that arrives first displaces the member along with
 * everything its translation unit would have dragged in. `build.rs` links
 * this object whole and ahead of both Lua and libc, which is what "first"
 * rests on.
 *
 * Lua's own `print` and number formatting are untouched: buffered stdio stays
 * libc's, writes out through `fd_write`, and only the three backends below —
 * the ones a `FILE` reaches the filesystem through — answer from here. */

#include <errno.h>
#include <stddef.h>
#include <stdio.h>
#include <sys/types.h>

#include "lauxlib.h"
#include "lua.h"

/* --- the libraries this interpreter does not open ------------------------ */

/* An empty table is `luaL_requiref`'s contract. None of these is reached: the
 * VM is built with a library set that names none of them. */
static int empty_module(lua_State *L) {
  lua_newtable(L);
  return 1;
}

int luaopen_io(lua_State *L) { return empty_module(L); }
int luaopen_package(lua_State *L) { return empty_module(L); }
int luaopen_debug(lua_State *L) { return empty_module(L); }

/* --- touching the filesystem, which refuses ------------------------------ */

/* `os.remove` and `os.rename` are cut from the sandbox's `os` table, but
 * `loslib.c` is compiled and linked for the three functions that are not, so
 * the two names are still resolved from here. */
int remove(const char *path) {
  (void)path;
  errno = EACCES;
  return -1;
}

int rename(const char *from, const char *to) {
  (void)from;
  (void)to;
  errno = EACCES;
  return -1;
}

FILE *fopen(const char *path, const char *mode) {
  (void)path;
  (void)mode;
  errno = EACCES;
  return NULL;
}

FILE *freopen(const char *path, const char *mode, FILE *stream) {
  (void)path;
  (void)mode;
  (void)stream;
  errno = EACCES;
  return NULL;
}

/* --- what a stream does besides write ------------------------------------ */

/* Every `FILE` carries a read, seek and close backend, and the standard
 * streams carry them as data — so the descriptor imports survive however
 * little of stdio a script reaches, and libc's exit path calls through them
 * unconditionally. Answering here leaves writing alone: `__stdio_write` is
 * still libc's, and `fd_write` is what `wasi:stdio` grants.
 *
 * Nothing open is seekable and no stream is readable, which is what these
 * say. The signatures are musl's, which wasi-libc keeps in a private header. */

off_t __stdio_seek(FILE *stream, off_t offset, int whence) {
  (void)stream;
  (void)offset;
  (void)whence;
  errno = ESPIPE;
  return -1;
}

size_t __stdio_read(FILE *stream, unsigned char *buf, size_t len) {
  (void)stream;
  (void)buf;
  (void)len;
  return 0;
}

int __stdio_close(FILE *stream) {
  (void)stream;
  return 0;
}
