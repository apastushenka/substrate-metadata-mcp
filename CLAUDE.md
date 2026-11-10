# Conventions

- Write `anyhow::Result` with its full path; never add `use anyhow::Result`.
  Only `Context` and the macros are imported from `anyhow`.
- A two-parameter `Result<T, E>` is `core::result::Result` from the prelude.
  A function that does not return `anyhow::Error` must not look like an anyhow function.
- An item used outside its module is `pub`, and private modules limit its reach; a parent
  re-exports with `pub use` what the rest of the crate needs. Reserve `pub(super)` and
  `pub(crate)` for what module privacy cannot hide: a helper method on a type that is or
  will be public.
- Format with `cargo +nightly fmt`. `rustfmt.toml` enables unstable options that
  stable rustfmt silently ignores, so formatting done on stable will not match.
- Do not comment what the code already says, and never write a doc comment that only
  restates the item's name. A comment earns its place by carrying what the code cannot:
  why a decision went this way, a constraint from outside the file, a trap.

# Tool schemas

`#[tool]` methods and the types they take and return become the tool and schema
descriptions that a model or a client reads, so documentation there is the contract rather
than commentary, and the rule above against restating a name does not apply to it. Which
of the two reads a given description depends on where it sits; the rule on the two halves
of the schema says which.

General MCP practice -- tool annotations, pagination metadata, naming, evaluations -- is
not repeated below. A rule about tool design belongs here only if general guidance does
not already cover it. What follows is specific to this codebase: how rmcp and schemars
render these types, and the conventions chosen where guidance leaves a choice.

- Every field carries a description, as does every named type reachable from it. Having
  nothing to say beyond a field's own name means the name is wrong, not that a restatement
  will do. On a request type keep them dense rather than short: they are cached at the
  front of the context, so length costs little, while a single wrong call they prevent
  costs a round trip. On a response type the reader is the client and the next maintainer.
- A root type is worse: rmcp strips `title` and `description` from a schema root, so a doc
  comment there reaches nobody, the client included. Put it on the `#[tool]` method
  instead, unless the text only repeats what the schema already shows. That is commentary,
  not contract: move it to a `//` comment when it warns about something, and delete it
  when it does not. A `#[serde(flatten)]` field loses its type's description the same way:
  its members are merged into the parent and the type itself is gone from the schema, for
  both structs and enums.
- The `#[tool]` doc and the field descriptions divide the work, but only one half of the
  schema reaches the model. The input schema does, so listing the parameters in the doc
  adds nothing. The output schema reaches the client, since rmcp fills it in from the
  `Json<T>` return and a test here keeps it that way, but not the model: the tool
  definition a client passes to a model API has room for a name, a description and the
  input schema, and no client copies the rest into the description. A response type's
  `description` is therefore written for the client and for the next reader of the source,
  and anything the model needs about a result belongs in the `#[tool]` doc as well: the
  shape of what comes back, what a listing leaves out, what only this tool reports.
- Prose carries what neither half of the schema can: what is absent (a listing that has no
  paging looks the same as one whose paging you missed), a fact about the whole response,
  and one tool's behaviour compared with another's.
- Naming another tool costs a rename and gains little: the tools are few and their names
  systematic, so the model already sees the one it needs. Name one where two tools overlap
  and the description has to say which of them ranks and truncates, where a string has to
  travel verbatim from one to the other, and in error messages, which fire exactly when
  the model has already guessed wrong. Saying when a tool does not apply is worth more
  than naming the one that does.
- Never serialise `null`: skip the field, so that a missing key means "not applicable".
  Empty collections stay put, being a different thing. That also settles the schema --
  schemars types an `Option` as `["string", "null"]`, which some MCP clients reject, and
  `#[schemars(extend("type" = "..."))]` pins the single type that is truthful once no
  `null` is ever sent.
- Check what the schema actually says, not what the source implies. schemars derives
  `required` from the type and from `#[serde(default)]` alone; it ignores
  `skip_serializing_if` entirely, so a skipped field that is neither an `Option` nor
  defaulted stays in `required` while the response omits it. It also emits `default` only
  for a type it can serialise, so two fields written identically diverge in the schema
  when one's enum lacks `Serialize`.
