# substrate-metadata-mcp

Read-only MCP server that indexes a Substrate runtime's metadata. Loads it once,
from a node RPC endpoint or a file, then lists, searches, and reads pallets,
storage, calls, events, errors, constants, view functions, and runtime APIs —
without touching chain state.

Supports metadata v14–v16. v14 has no runtime APIs or view functions; v15 has
no view functions.

## Install

```bash
cargo install substrate-metadata-mcp
```

Building from source needs Rust 1.88 or newer.

## Usage

The one required argument says where the metadata comes from: a node endpoint
(ws, wss, http, or https) or a file.

```bash
substrate-metadata-mcp --url wss://rpc.polkadot.io
substrate-metadata-mcp --file polkadot.scale
```

The file holds SCALE-encoded metadata, the default output of
[`subxt metadata`](https://crates.io/crates/subxt-cli). A project that generates
code with subxt usually keeps one already; otherwise:

```bash
cargo install subxt-cli
subxt metadata --url wss://rpc.polkadot.io -o polkadot.scale
```

A node's metadata is fetched once in the background at startup, and retried on
tool use if that fetch failed: an unreachable node breaks the tool calls, not
the server's startup. A file is read once before the server starts, and one
that cannot be read or decoded stops it with an error. Either way the metadata
and its source last for the process lifetime: a runtime upgrade on the node or
a change to the file is not picked up, and no tool argument can change the
source.

## Usage with an MCP client

Add this to the client's MCP configuration, replacing the URL with the chain you
want indexed:

```json
{
  "mcpServers": {
    "substrate-metadata": {
      "command": "substrate-metadata-mcp",
      "args": ["--url", "wss://rpc.polkadot.io"]
    }
  }
}
```

- Claude Code: `.mcp.json` in the project root
- Cursor: `.cursor/mcp.json` in the project, or `~/.cursor/mcp.json` globally

For a file, the arguments are `["--file", "/absolute/path/to/polkadot.scale"]`:
clients start the server in different working directories, so a relative path
may not resolve.

Add another entry under `mcpServers` with a different name and `--url` or
`--file` for a second chain.

## Tools

| Tool | Role |
| --- | --- |
| `get_runtime_info` | Spec name, versions, SS58 prefix, metadata format version |
| `list_pallets` | Every pallet with index and per-kind item counts |
| `list_pallet_items` | Paged listing of one pallet's items (optionally by kind) |
| `get_pallet_item` | Full docs and kind-specific fields for one pallet item |
| `list_runtime_apis` | Every runtime API (empty under metadata v14) |
| `list_runtime_api_methods` | Every method of one runtime API |
| `get_runtime_api_method` | Full docs and signature for one runtime API method |
| `search` | Ranked name/docs search across the whole metadata |

Typical flow: `search` (or `list_pallets` → `list_pallet_items`) to locate an
item, then `get_pallet_item` / `get_runtime_api_method` for the full record.
Listings are complete, paged if long; `search` ranks and truncates.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.
