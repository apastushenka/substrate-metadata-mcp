# Conventions

- Write `anyhow::Result` with its full path; never add `use anyhow::Result`.
  Only `Context` and the macros are imported from `anyhow`.
- A two-parameter `Result<T, E>` is `core::result::Result` from the prelude.
  A function that does not return `anyhow::Error` must not look like an anyhow function.
- Format with `cargo +nightly fmt`. `rustfmt.toml` enables unstable options that
  stable rustfmt silently ignores, so formatting done on stable will not match.
- Do not comment what the code already says, and never write a doc comment that only
  restates the item's name. A comment earns its place by carrying what the code cannot:
  why a decision went this way, a constraint from outside the file, a trap.

# Tool schemas

`#[tool]` methods and the types they return become the tool and schema descriptions a
model reads, so documentation there is the contract rather than commentary, and the rule
above against restating a name does not apply to it.

- Every field carries a description, as does every named type reachable from it. Having
  nothing to say beyond a field's own name means the name is wrong, not that a restatement
  will do. Keep them dense rather than short: they are cached at the front of the context,
  so brevity buys little, while one mis-call they prevent costs a whole round trip.
- Root request and response types are the exception: rmcp strips `title` and `description`
  from a schema root, so a doc comment there reaches nobody. Put it on the `#[tool]`
  method instead.
- Never serialise `null`: skip the field, so that a missing key means "not applicable".
  Empty collections stay put, being a different thing. That also settles the schema --
  schemars types an `Option` as `["string", "null"]`, which some MCP clients reject, and
  `#[schemars(extend("type" = "..."))]` pins the single type that is truthful once no
  `null` is ever sent.
- Check what the schema actually says, not what the source implies. schemars derives
  `required` from the type and from `#[serde(default)]` alone; it ignores
  `skip_serializing_if` entirely, so a skipped field that is neither an `Option` nor
  defaulted stays in `required` while the response omits it.
