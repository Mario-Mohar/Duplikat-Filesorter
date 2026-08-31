# Contributing

Thanks for taking the time. This is a small project, so the process is short.

## Getting set up

The application is a Tauri desktop app: a Rust core under `src-tauri/` and a
plain HTML and JavaScript front end under `src/`.

```bash
git clone https://github.com/Mario-Mohar/Duplikat-Filesorter.git
cd Duplikat-Filesorter
npm install

# Linux also needs Tauri's system libraries:
sudo apt-get install libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf

npm run tauri dev
```

## Running the checks

The pipeline runs exactly what you can run here:

```bash
cd src-tauri
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

CI runs the tests through `cargo nextest` rather than `cargo test`, for one
reason only: `cargo test` cannot write a JUnit report, and JUnit is the one
format every project here emits so the pull request comment can read a test
count. `cargo test` is the same suite and is fine locally.

Note that the Rust core links against webkit2gtk through Tauri, so even
`cargo test` needs the system libraries above.

## The rule this tool exists under

**It never deletes.** Duplicates are *moved* into a folder of their own, and
the original stays where it was until the move has succeeded. A change that
deletes a file — even one it is certain is a duplicate — is out of scope. The
whole promise of the tool is that a mistake is recoverable by dragging a folder
back.

Three pieces of the code hold that promise up, and each has a test:

**The copy is interruptible and cleans up after itself.** `copy_with_stop`
copies in blocks and checks the stop flag between them, because with `fs::copy`
the last chance to notice a cancellation on a 4 GB file would be minutes in the
past. If it is interrupted, the half-written destination is removed — otherwise
the duplicates folder would hold a corrupt file that looks like a rescued one.

**Only EXDEV falls back to copying.** A rename across filesystems fails with
EXDEV (18 on Unix, 17 on Windows) and that is the one case that should be
retried as copy-then-delete. A permission error must stay an error rather than
become a second, equally hopeless attempt.

**The scan remembers where it has been.** A directory symlink pointing at its
own ancestor would otherwise be an endless descent, and a second path to the
same directory would list a file twice — making it its own duplicate.

## Two things about hashing

The read buffer is 8 KB and the loop must consume the whole file. A loop
written wrongly hashes only the first block, and then every file that merely
*starts* the same counts as a duplicate. There is a test with two 100 KB files
differing in their last byte alone.

`calculate_md5` returns `None` for a file it cannot read, and the caller skips
it. Never substitute a placeholder hash: it would be a value two unreadable
files share, which makes them duplicates of each other.

## Pull requests

- Branch off `main`. Any branch name is fine.
- Commit messages follow `fix(scope):`, `feat(scope):`, `docs:`, `chore:`.
  The pipeline reads the pull request title's prefix to label it.
- The pipeline comments the result and updates that comment on every push.
  Green plus not-a-draft gets a `ready-to-merge` label.
- Maintainers can ask for a deeper look with `/claude review`.

`build.yml` still produces the Windows and Linux bundles. The pipeline here is
deliberately the fast half — formatting, lint and tests — so it is useful on a
pull request.

## Reporting something

Use the issue templates. A path that reproduces it matters more than anything
else; if the paths are private, the *shape* is usually enough — a symlink, a
mount point, two different filesystems, an unreadable file.

## Licence

MIT, same as the project. By contributing you agree your work ships under it.
