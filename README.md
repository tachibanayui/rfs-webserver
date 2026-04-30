# rfs-webserver

`rfs-webserver` is a small Rust webserver built with [axum](https://docs.rs/axum) that serves a randomly generated virtual filesystem.

The filesystem is generated on demand from a seed and the current request path. That means the server does not keep a full tree in memory, which keeps RAM usage low even for large virtual directory structures.

## Features

- Deterministic virtual filesystem generation from a seed.
- Optional seed fallback to the current system time.
- HTML directory listings with simple browser-like navigation.
- Plain text file responses for generated file paths.
- Clap-based command-line configuration with sensible defaults.
- Optional real-path overlay sampling from an on-disk directory tree.
- Optional TOML dictionary to override default Linux + e-commerce naming.

## Requirements

- Rust toolchain with Cargo

## Run

Start the server with the default configuration:

```bash
cargo run
```

By default the server listens on `127.0.0.1:3000`.

## CLI Options

```text
--host <HOST>         Bind address, default: 127.0.0.1
-p, --port <PORT>     TCP port, default: 3000
--seed <SEED>         Optional base seed for deterministic generation
--depth <DEPTH>       Maximum directory depth, default: 10
--min-files <N>       Minimum files per directory, default: 10
--max-files <N>       Maximum files per directory, default: 100
--min-dirs <N>        Minimum subdirectories per directory, default: 0
--max-dirs <N>        Maximum subdirectories per directory, default: 100
--real-path <PATH>    Optional on-disk directory used as a source of real entries
--real-path-chance <P> Probability in the range 0..1 for including a real entry, default: 0
--dictionary <PATH>   Optional TOML dictionary to override the default naming lists
```

If `--seed` is not provided, the server uses the current system time as the effective seed.

The min/max values are validated at startup:

- `min-files` must not be greater than `max-files`
- `min-dirs` must not be greater than `max-dirs`
- `real-path-chance` must be between `0` and `1`
- `real-path`, when provided, must point to an existing directory

## Examples

Run with the default settings:

```bash
cargo run
```

Run with a fixed seed and smaller tree:

```bash
cargo run -- --seed 42 --depth 4 --min-files 1 --max-files 5 --min-dirs 0 --max-dirs 3
```

Mix in entries from a real directory tree:

```bash
cargo run -- --real-path C:\data\sample-vfs --real-path-chance 0.25
```

Bind to another port and host:

```bash
cargo run -- --host 0.0.0.0 --port 8080
```

Use a custom dictionary (TOML) for naming:

```bash
cargo run -- --dictionary .\dictionary.toml
```

Example dictionary:

```toml
[anchors]
# Preferred root-level directories for Linux-style layouts.
roots = ["etc", "var", "srv", "opt", "home", "data", "logs"]

[dirs]
# Frequent directory names used at most depths.
common = ["orders", "users", "invoices", "billing", "payments", "exports", "archive"]
# Extra directory names that appear more at deeper levels (optional).
deep = ["2026", "2025", "04", "05", "daily", "monthly", "regional"]

[files]
# Base names for files; an ID is appended.
stems = ["order", "invoice", "user", "receipt", "export", "report"]
# File extensions, without dots.
extensions = ["json", "csv", "pdf", "txt", "log"]

[ids]
# Allowed ID formats for suffixes.
formats = ["uuid", "numeric", "date", "invoice_code"]

[weights]
# Relative selection weights; higher values make that group more likely (optional).
anchors = 4
dirs_common = 5
dirs_deep = 2
```

## Browser Usage

Open the server root in your browser:

```text
http://127.0.0.1:3000/
```

Directory requests render an HTML listing page with links to child directories and files. File requests return plain text content.

## Behavior

- Directory listings are generated from the effective seed and the request path.
- The same seed and configuration produce the same paths and file content.
- Directories are rendered as HTML pages with a simple mirror/index style layout.
- Files are generated lazily when requested, so the server does not need to store the entire tree.
- When `--real-path` is set, some directory entries may come from the configured on-disk tree.
- Real directories recurse and real files return their actual file contents.
- When `--dictionary` is set, the default Linux + e-commerce naming lists are replaced by the
  contents of the TOML file.

## Project Layout

- `src/main.rs` - application entrypoint and server startup
- `src/cli.rs` - clap argument parsing and configuration validation
- `src/routes.rs` - axum routes and HTML rendering
- `src/vfs/node.rs` - on-demand virtual filesystem generation and lookup logic
- `src/vfs/generator.rs` - lightweight wrapper that builds the runtime filesystem state

## Development

Run the test suite:

```bash
cargo test
```

Check the project:

```bash
cargo check
```
