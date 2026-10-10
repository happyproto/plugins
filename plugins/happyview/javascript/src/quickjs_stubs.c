/* What keeps this module's WASI imports to the three the host grants it:
 * `clock_time_get`, `random_get` and `fd_write`.
 *
 * Nothing here is reached by a script. QuickJS-ng compiles without its
 * `quickjs-libc` half — no `std`, no `os`, no file or process access — but
 * two routes still reach libc at link time, and each brings imports the host
 * refuses.
 *
 * Rust's own panic machinery asks the environment for `RUST_BACKTRACE`, even
 * under `panic = "abort"`. wasi-libc answers `getenv` by copying the whole
 * environment in on first use, which links `environ_get` and
 * `environ_sizes_get`, and exits the process through `_Exit` if that copy
 * cannot be allocated, which links `proc_exit`.
 *
 * Every `FILE` carries a seek and close backend, and the standard streams
 * carry them as data — so the descriptor imports survive however little of
 * stdio anything reaches. QuickJS's own diagnostics print through `fprintf`,
 * and `stdout`'s first write asks whether the descriptor is a terminal, which
 * links `fd_fdstat_get`; `__stdio_seek` and `__stdio_close` link `fd_seek` and
 * `fd_close`.
 *
 * Defining those entry points here keeps libc's own out. An archive member is
 * extracted only to satisfy a symbol still undefined when the linker reaches
 * it, so a definition that arrives first displaces the member along with
 * everything its translation unit would have dragged in. `build.rs` links
 * this object whole and ahead of both QuickJS and libc, which is what "first"
 * rests on. `tests/module_interface.rs` pins the result, so a QuickJS or a
 * Rust that reaches a new libc symbol fails there, naming the import. */

#include <errno.h>
#include <stddef.h>
#include <stdio.h>
#include <sys/types.h>

/* --- the environment, which is empty ------------------------------------- */

/* The host instantiates the module with no environment at all, so libc's
 * `getenv` could only ever have answered NULL after copying in nothing. Rust
 * then takes its default backtrace style, which `panic = "abort"` never
 * prints anyway. */
char *getenv(const char *name) {
  (void)name;
  return NULL;
}

/* --- what a stream does besides write ------------------------------------ */

/* Nothing open is seekable, closing a standard stream is never asked for by
 * anything that runs, and no descriptor is a terminal — which leaves `stdout`
 * fully buffered rather than line buffered, a difference nothing here can
 * observe because nothing here writes to it. Writing itself is untouched:
 * `__stdio_write` is still libc's, and `fd_write` is what `wasi:stdio` grants.
 *
 * The signatures are musl's, which wasi-libc keeps in a private header; they
 * must match exactly, because the three are called through a `FILE`'s
 * function pointers and wasm-ld refuses an indirect call of the wrong type. */

off_t __stdio_seek(FILE *stream, off_t offset, int whence) {
  (void)stream;
  (void)offset;
  (void)whence;
  errno = ESPIPE;
  return -1;
}

int __stdio_close(FILE *stream) {
  (void)stream;
  return 0;
}

int __isatty(int fd) {
  (void)fd;
  errno = ENOTTY;
  return 0;
}
