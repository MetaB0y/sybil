# `fuzz`

Standalone `cargo-fuzz` workspace for API parsing and settlement boundaries.

- A target must exercise production parsing/arithmetic rather than reproduce
  the implementation in the harness.
- Crashes become deterministic regression tests in the owning crate before the
  corpus is treated as fixed.
- Keep generated values within the target's documented semantic domain; do not
  hide panics by broadly discarding inputs.
- Run targets through `cargo fuzz run <target>` from this directory.
