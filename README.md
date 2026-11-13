# substrate-metadata-mcp

An MCP server that indexes the metadata of a Substrate runtime. It loads the
metadata once, from a node or from a file. Its tools list, search and read the
pallets with their calls, storage, events, errors, constants and view
functions, and the runtime APIs with their methods. The server is read-only and
never touches chain state.

It supports metadata versions 14 to 16. Version 14 has no runtime APIs or view
functions, and version 15 has no view functions.

## Install

```bash
cargo install --locked substrate-metadata-mcp
```

This builds the server from source and needs Rust 1.88 or newer.

## Usage

Point the server at a node endpoint (ws, wss, http or https) or at a file.
Exactly one of the two is required.

```bash
substrate-metadata-mcp --url wss://rpc.polkadot.io
substrate-metadata-mcp --file polkadot.scale
```

The file holds SCALE-encoded metadata, which is what
[`subxt metadata`](https://crates.io/crates/subxt-cli) writes by default.
Projects that generate code with subxt usually have such a file already. If
yours doesn't, create one:

```bash
cargo install --locked subxt-cli
subxt metadata --url wss://rpc.polkadot.io -o polkadot.scale
```

With `--url`, the server fetches the metadata in the background at startup. If
the node can't be reached, the server still starts, and each tool call tries
the fetch again. With `--file`, the server reads the file before it starts, and
exits with an error if the file can't be read or decoded.

The metadata is loaded only once. A runtime upgrade on the node or a new
version of the file takes effect after a restart, and no tool can switch the
server to another node or file.

## Usage with an MCP client

Add the server to your client's MCP configuration, with the URL of the chain
you want to index:

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

Claude Code reads this from `.mcp.json` in the project root. Cursor reads it
from `.cursor/mcp.json` in the project, or from `~/.cursor/mcp.json` for all
projects.

To use a file, pass `["--file", "/absolute/path/to/polkadot.scale"]` as the
arguments. Give an absolute path, because each client starts the server in a
working directory of its own.

To index a second chain, add another entry with a different name.

## Tools

| Tool | What it does |
| --- | --- |
| `get_runtime_info` | Spec name, versions, SS58 prefix and metadata version |
| `list_pallets` | Every pallet with its index and the number of items of each kind |
| `list_pallet_items` | The items of one pallet, page by page, optionally filtered by kind |
| `get_pallet_item` | One pallet item in full, with its docs and the fields of its kind |
| `list_runtime_apis` | Every runtime API (none under metadata version 14) |
| `list_runtime_api_methods` | Every method of one runtime API |
| `get_runtime_api_method` | One runtime API method in full, with its docs and signature |
| `search` | Names and docs across the whole metadata, best matches first |

To find an item, use `search`, or browse with `list_pallets` and
`list_pallet_items`. Then read it in full with `get_pallet_item` or
`get_runtime_api_method`. The listings show everything, split into pages when
long, while `search` returns only the best matches.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.
