## What this changes

<!-- What does this do, and why? -->

## Type of change

- [ ] Bug fix
- [ ] New feature
- [ ] Behaviour change that is not backwards compatible
- [ ] Refactor, tests or tooling
- [ ] Documentation

## Related issues

<!-- "Fixes #12". Leave blank if there is none. -->

## Checklist

- [ ] `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` and `cargo test` all pass
- [ ] Nothing in this change deletes a file
- [ ] An interrupted or failed move leaves no half-written file behind
- [ ] Any new fallback on an IO error is as narrow as the EXDEV one
- [ ] Changes to hashing or to the scan come with a test
