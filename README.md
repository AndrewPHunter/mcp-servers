# MCP Servers

Rust workspace for Model Context Protocol (MCP) servers powering the AI Workbench.
The goal is to improve client productivity by giving agents direct access to trusted best-practice knowledge.

## Workspace Structure

```
crates/
  mcp-common/       Shared library (Redis, LanceDB, serialization utilities)
  cpp-guidelines/   C++ Core Guidelines MCP server
data/                Local data directory (not committed)
  cpp-guidelines/    Cloned C++ Core Guidelines repository
  lancedb/           LanceDB vector database files
  redis/             Redis persistence (AOF/RDB)
```

## Prerequisites

- Rust (stable toolchain)
- Protocol Buffers compiler (`brew install protobuf` on macOS)
- Docker and Docker Compose (for Redis)

## Setup

1. Clone the repository and create your local environment file:

```sh
cp .env.example .env
```

2. Create the data directories and start infrastructure:

```sh
mkdir -p data/lancedb data/redis
docker compose up -d
```

3. Clone the C++ Core Guidelines into the data directory:

```sh
git clone https://github.com/isocpp/CppCoreGuidelines.git data/cpp-guidelines
```

If `data/cpp-guidelines` already exists and is not empty, remove it first or update it in place:

```sh
rm -rf data/cpp-guidelines
git clone https://github.com/isocpp/CppCoreGuidelines.git data/cpp-guidelines
# OR, if it is already a clone:
git -C data/cpp-guidelines pull --ff-only
```

4. Build the workspace:

```sh
cargo build
```

## Development Workflow

- `cargo check` -- type-check the full workspace
- `cargo build` -- build all crates
- `cargo test` -- run all tests
- `cargo run -p cpp-guidelines` -- run the C++ Guidelines MCP server
- `docker compose up -d` -- start Redis
- `docker compose down` -- stop Redis

## License

MIT
