# Contributing

## Contributions are welcome

This is a small project maintained by one person in his spare time, and that is
exactly why an outside pair of eyes is worth a lot. **Finding a bug and writing
it down is a real contribution** — arguably the most useful one, because I only
ever use this on my own machine, with my own setup, and most of what is broken
is broken somewhere I never look.

Three ways to help, in the order of what they cost you:

### 1. Report something that is wrong

Open an issue with the **Bug report** template. It asks for what it does because
each field is something I would otherwise have to come back and ask for, which
costs us both a day.

What actually decides whether a report is useful:

- **What you expected, and what happened instead.** Both halves. "It does not
  work" is the one report I cannot act on.
- **The steps that get there.** If you can reproduce it, say how. If it only
  happened once, say that too — an intermittent bug is still worth knowing about,
  and "I could not reproduce it" is useful information rather than a
  disqualification.
- **Your setup**, as the template asks for it.

Do not polish it. A rough report today beats a perfect one that never gets
written. If in doubt whether something counts as a bug: open it. Deciding that
is my job, not yours.

### 2. Suggest something it should do

Open an issue with the **Feature request** template.

It asks what you are trying to *achieve* before what you want built, and that is
deliberate — not a hoop. Roughly half the time there turns out to be a simpler
answer than the one either of us had in mind, and it only surfaces if I know the
underlying situation.

A wish that gets declined is not a wasted issue. "Not now" and "not in this
project" are answers you will get quickly and with a reason.

### 3. Send a fix or a feature

Very welcome, and you do not need to ask permission for something small.

**For anything bigger than a few lines, open an issue first** — or comment on
the existing one — and say you are working on it. It costs you a sentence and
saves you the case where I fixed the same thing that evening, or where I would
have wanted it solved differently.

Because you cannot push to this repository, the route is through a fork:

```bash
# 1. Fork it on GitHub, then clone your fork
git clone https://github.com/<your-username>/Duplikat-Filesorter.git
cd Duplikat-Filesorter

# 2. A branch. Any name.
git switch -c fix/the-thing

# 3. Change what you came for, then run the checks below

# 4. Push to your fork and open the pull request
git push -u origin fix/the-thing
```

GitHub then offers you the pull request button. Fill in the template, and if it
closes an issue write `Fixes #12` so it closes itself on merge.

## What happens after you send it

1. **The pipeline runs** and posts a comment on your pull request with a table
   of what passed. It updates that same comment on every push, so there is one
   place to look rather than a growing pile.
2. **It labels the pull request** by size and type, and adds `ready-to-merge`
   once everything is green.
3. **On your very first contribution here, the checks wait for me to release
   them.** GitHub does that by default so that a stranger's code cannot use the
   runners unasked. If your pull request sits at "waiting for approval",
   **nothing is broken and you do not need to do anything** — I have to click
   once.
4. **I do the merging.** The default branch takes nothing that has not been
   through a pull request with green checks, and that holds for my own commits
   too.

If a check is red, the run log says which one and why. Ask in the pull request
if it is not obvious — a red pipeline is not a rejection, and quite often it is
the pipeline that is wrong rather than you.

I do this beside a job, so a reply can take a few days. It is not disinterest.

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

- Branch off `main` **in your fork** (see above). Any branch name is fine.
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
