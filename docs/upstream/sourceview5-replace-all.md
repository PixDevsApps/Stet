# Upstream issue draft: `SearchContext::replace_all` in sourceview5

Project: Stet, a fast, keyboard-first text and code editor for Omarchy

Synced: 2026-10-01

**Status: draft, not filed.** Filing it is the user's action, with their own GNOME GitLab account; agents do not file it. Spike S2 found the bug ([S2-search.md, binding defects](../spikes/S2-search.md#binding-defects)). The decision and our workaround are in the [ADR-005 amendment](../DECISIONS.md#adr-005-amendment--our-own-replace-engine-2026-09-30).

---

## Facts checked on 2026-09-30

- **Crate:** `sourceview5` 0.11.2 from crates.io, the Rust binding of GtkSourceView 5. Upstream is [World/Rust/sourceview5-rs](https://gitlab.gnome.org/World/Rust/sourceview5-rs) on GNOME GitLab.
- **Where:** `SearchContext::replace_all` is written by hand, not generated, in upstream `sourceview5/src/search_context.rs`. It comes from commit `be3b3c22`, "manually implement SearchContext::replace_all" (2022-01-02), and was unchanged on `main` when checked on 2026-09-30.
- **The C function returns a count.** The installed `gtksourcesearchcontext.h` (GtkSourceView 5.20.0) declares `guint gtk_source_search_context_replace_all (GtkSourceSearchContext *search, const gchar *replace, gint replace_length, GError **error)`, and the installed GIR documents the return value as "the number of replaced matches."
- **The binding treats it as a success flag.** The published 0.11.2 crate (`src/search_context.rs`) has `assert_eq!(is_ok == 0, !error.is_null());`, and the method returns `Result<(), glib::Error>`.
- **Observed:** in S2 (headless under GTK Broadway, gtk4 0.11.5, GtkSourceView 5.20.0, rustc 1.98.1), a replace with zero matches made the C call return 0 and the safe wrapper panic with `assertion left == right failed`.
- **Reproduced:** the snippet in the draft was compiled and run headless (GTK Broadway) on 2026-09-30, as a debug and as a release build. Both panic at `sourceview5-0.11.2/src/search_context.rs:16:13` with exactly the message under *Actual result*. With three matches the call replaces all three and returns `Ok(())`, so the count is lost. The first version of the snippet did not compile (`set_search_text` needs `sourceview5::prelude::*`); the draft below has the corrected import.
- **`SearchContext::replace` too:** the generated binding (`src/auto/search_context.rs:362`) has `debug_assert_eq!(is_ok == glib::ffi::GFALSE, !error.is_null())`, but the C function returns `FALSE` without an error when the range is not a match. Run on 2026-09-30: a debug build panics with the same message; a release build returns `Ok(())` and replaces nothing. A real match returns `Ok(())` and replaces correctly. The draft includes it.
- **Existing reports:** no upstream issue or merge request matched "replace_all" (issues and MRs of sourceview5-rs searched on 2026-09-30).

## Draft issue

Copy the title and the body below into a new issue.

**Title:** `SearchContext::replace_all` panics when nothing is replaced and discards the count; `replace` asserts on a non-match

**Body:**

````markdown
### Summary

`SearchContext::replace_all` panics when the search has no matches, and it drops the number of replacements.

The C function returns the number of replaced matches, not a boolean:

```c
guint gtk_source_search_context_replace_all (GtkSourceSearchContext  *search,
                                             const gchar             *replace,
                                             gint                     replace_length,
                                             GError                 **error);
```

The hand-written binding in `sourceview5/src/search_context.rs` (from be3b3c22, "manually implement SearchContext::replace_all") treats that count as a success flag:

```rust
let is_ok = ffi::gtk_source_search_context_replace_all(
    self.to_glib_none().0,
    replace.to_glib_none().0,
    replace_length,
    &mut error,
);
assert_eq!(is_ok == 0, !error.is_null());
```

- With 0 replacements and no error, `is_ok == 0` is `true` and `!error.is_null()` is `false`, so the assertion panics.
- With n > 0 replacements the count is lost, because the method returns `Result<(), glib::Error>`.

### Steps to reproduce

`Cargo.toml`: `gtk = { package = "gtk4", version = "0.11" }`, `sourceview5 = "0.11.2"`. Run it with any display (a GDK backend is needed for `gtk::init`).

```rust
use sourceview5::prelude::*; // also re-exports gtk's prelude

fn main() {
    gtk::init().unwrap();
    let buffer = sourceview5::Buffer::new(None);
    buffer.set_text("abc");
    let settings = sourceview5::SearchSettings::new();
    settings.set_search_text(Some("zzz"));
    let context = sourceview5::SearchContext::new(&buffer, Some(&settings));
    let result = context.replace_all("x");
    println!("{result:?}");
}
```

### Actual result

```
assertion `left == right` failed
  left: true
 right: false
```

### Expected result

No panic. With the count returned: `Ok(0)`.

### Suggested fix

Return the count, and decide success by the error pointer only:

```rust
pub fn replace_all(&self, replace: &str) -> Result<u32, glib::Error> {
    unsafe {
        let mut error = std::ptr::null_mut();
        let count = ffi::gtk_source_search_context_replace_all(
            self.to_glib_none().0,
            replace.to_glib_none().0,
            replace.len() as i32,
            &mut error,
        );
        if error.is_null() {
            Ok(count)
        } else {
            Err(from_glib_full(error))
        }
    }
}
```

Changing the return type is an API break, so it may need a new method (with the assertion also removed from the existing one) or a semver-major release.

### Related: `SearchContext::replace`

The generated `replace` (in `src/auto/search_context.rs`) has the same pattern:

```rust
debug_assert_eq!(is_ok == glib::ffi::GFALSE, !error.is_null());
```

`gtk_source_search_context_replace` returns `FALSE` *without* setting an error when `match_start`/`match_end` do not correspond to a search match. With the buffer `"abc abc"`, search text `"abc"`, and the range 1..2:

- debug build: it panics with the same `left == right` assertion message (`left: true`, `right: false`);
- release build: `Ok(())`, and nothing is replaced, so the caller cannot tell.

It should report whether a replacement happened, for example `Result<bool, glib::Error>`, without the assertion.

### Versions

sourceview5 0.11.2, gtk4 0.11.5, GtkSourceView 5.20.0, GTK 4.22.4, rustc 1.98.1, Arch Linux. The code is unchanged on `main` as of 2026-09-30.
````

## How to file

1. Sign in to [gitlab.gnome.org](https://gitlab.gnome.org) with your own GNOME GitLab account.
2. Search the [sourceview5-rs issues](https://gitlab.gnome.org/World/Rust/sourceview5-rs/-/issues) and merge requests for "replace_all" again, in case someone reported it since 2026-09-30.
3. If a sourceview5 release newer than 0.11.2 exists by then, re-run the repro against it. It was compiled and verified against 0.11.2 on 2026-09-30 (see *Facts* above).
4. Open a new issue with the title and body above.
5. Record the issue link and date here and in [PLAN.md](../PLAN.md).

## Our workaround

- Production code does not use `SearchContext::replace_all` or `SearchContext::replace`. Every replace runs through our own `pcre2` engine ([ADR-005 amendment](../DECISIONS.md#adr-005-amendment--our-own-replace-engine-2026-09-30)).
- If a replace through SearchContext is ever needed, call `sourceview5::ffi::gtk_source_search_context_replace_all` through a small wrapper that returns `Result<u32, glib::Error>`, as the S2 spike's `replace_all_counted` does.

## Same class of defect in the async search (found 2026-10-01)

**Status: facts recorded, not yet reduced to a standalone repro, not filed.** Add it to the issue above, or file it separately, once a minimal repro has been run.

- **Where:** the generated `SearchContext::forward_async` and `backward_async` in sourceview5 0.11.2 (`src/auto/search_context.rs`), and so `forward_future` and `backward_future`.
- **What:** the trampolines call `gtk_source_search_context_forward_finish` (and `backward_finish`), which return a `gboolean` "a match was found", but they ignore it and check only the `GError`. When nothing matches, the finish function returns `FALSE` without an error and without filling the iterators, and the binding returns `Ok((match_start, match_end, has_wrapped_around))` built from `TextIter::uninitialized()`.
- **Observed** in Stet's M1 find self-test (headless under GTK Broadway, gtk4 0.11.5, GtkSourceView 5.20.0, rustc 1.98.1): after `forward_future` resolved for a search text that does not occur, `TextBuffer::select_range` on the returned iterators logged `Gtk-CRITICAL **: real_set_mark: assertion '_gtk_text_iter_get_btree (where) == tree' failed` twice, and the process aborted (core dumped).
- **Suggested fix:** return `Ok(None)` (or `Option<(TextIter, TextIter, bool)>`) when the finish function returns `FALSE`, as the synchronous `forward`, which checks the return value, already does.
- **Our workaround:** `app/src/search.rs` calls the C async functions itself and honours the return value ([ADR-005 amendment, 2026-10-01](../DECISIONS.md#adr-005-amendment--find-next-through-our-own-async-wrapper-2026-10-01)).
