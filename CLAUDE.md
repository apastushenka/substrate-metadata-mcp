# Conventions

- Write `anyhow::Result` with its full path; never add `use anyhow::Result`.
  Only `Context` and the `anyhow!` macro are imported from `anyhow`.
- A two-parameter `Result<T, E>` is `core::result::Result` from the prelude.
  A function that does not return `anyhow::Error` must not look like an anyhow function.
- Format with `cargo +nightly fmt`. `rustfmt.toml` enables unstable options that
  stable rustfmt silently ignores, so formatting done on stable will not match.
