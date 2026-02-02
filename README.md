# MCP Servers

Rust workspace for Model Context Protocol (MCP) servers powering the AI Workbench.
The goal is to improve client productivity by giving agents direct access to trusted best-practice knowledge.

## Workspace Structure

```
crates/
  mcp-common/       Shared library (Redis, LanceDB, serialization utilities)
  cpp-guidelines/   C++ Core Guidelines MCP server
  rust-api-guidelines/ Rust API Guidelines MCP server
  llm-proxy/        Local OpenAI-compatible proxy MCP server
  nodejs-guidelines/ Node.js Best Practices MCP server
data/                Local data directory (not committed)
  cpp-guidelines/    Cloned C++ Core Guidelines repository
  rust-api-guidelines/ Cloned rust-lang/api-guidelines repository
  nodejs-guidelines/  Cloned nodebestpractices repository
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

3. Clone the guideline repositories into the data directory:

```sh
git clone https://github.com/isocpp/CppCoreGuidelines.git data/cpp-guidelines
git clone https://github.com/rust-lang/api-guidelines.git data/rust-api-guidelines
git clone https://github.com/goldbergyoni/nodebestpractices.git data/nodejs-guidelines
```

If a target directory already exists and is not empty, remove it first or update it in place:

```sh
rm -rf data/cpp-guidelines
git clone https://github.com/isocpp/CppCoreGuidelines.git data/cpp-guidelines
# OR, if it is already a clone:
git -C data/cpp-guidelines pull --ff-only

rm -rf data/rust-api-guidelines
git clone https://github.com/rust-lang/api-guidelines.git data/rust-api-guidelines
# OR, if it is already a clone:
git -C data/rust-api-guidelines pull --ff-only
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
- `cargo run -p rust-api-guidelines` -- run the Rust API Guidelines MCP server
- `cargo run -p llm-proxy` -- run the local LLM proxy MCP server
- `cargo run -p nodejs-guidelines` -- run the Node.js Best Practices MCP server
- `docker compose up -d` -- start Redis
- `docker compose down` -- stop Redis

## Rust API Guidelines MCP Tools

The `rust-api-guidelines` server exposes the following MCP tools.

- `search_guidelines`
  - Input: `{ "query": string, "limit"?: number }` (`limit` defaults to 10, max 50)
  - Output: JSON object `{ results: [{ id, title, category, score, summary }] }`
- `get_guideline`
  - Input: `{ "guideline_id": string }` (for example `C-CASE`)
  - Output: JSON object `{ id, anchor, title, category, source_file, raw_markdown }`
- `list_category`
  - Input: `{ "category": string }` (for example `Naming`, `Documentation`)
  - Output: JSON object `{ category: { key, display_name, guideline_count }, guidelines: [{ id, title }] }`
- `update_guidelines`
  - Input: none
  - Output: JSON object `{ updated, commit, guideline_count }`

## LLM Proxy MCP Tools

The `llm-proxy` server exposes tools for a coordinator model to discover available local models
and delegate requests to them via an OpenAI-compatible API host.

- `list_models`
  - Input: none
  - Output: JSON object `{ object?, data: [{ id, object?, created?, owned_by? }] }`
- `ask_model`
  - Input: `{ "model": string, "prompt": string }`
  - Output: JSON object `{ text: string }`
- `chat_model`
  - Input: `{ "model": string, "messages": [{ "role": string, "content": string }] }`
  - Output: JSON object `{ text: string }`
- `generate_code`
  - Input: `{ "model": string, "language": string, "specification": string }`
  - Output: JSON object `{ text: string }` (typically code-only)
- `start_conversation`
  - Input: none
  - Output: JSON object `{ conversation_id: string }`
- `continue_conversation`
  - Input: `{ "conversation_id": string, "model": string, "prompt": string }`
  - Output: JSON object `{ text: string }`
- `end_conversation`
  - Input: `{ "conversation_id": string }`
  - Output: JSON object `{ ok: bool }`
- `get_usage_stats`
  - Input: none
  - Output: JSON object `{ redis_available: bool, models: [{ model, requests, total_tokens?, token_counted_requests, token_unknown_requests }] }`

## Node.js Best Practices MCP Tools

The `nodejs-guidelines` server exposes the following MCP tools.

- `search_guidelines`
  - Input: `{ "query": string, "limit"?: number }` (`limit` defaults to 10, max 50)
  - Output: JSON object `{ results: [{ id, title, category, score, summary }] }`
- `get_guideline`
  - Input: `{ "guideline_id": string }` (for example `1.1`)
  - Output: JSON object `{ id, anchor, title, category, source_file, raw_markdown }`
- `list_category`
  - Input: `{ "category": string }` (for example `1`, `2`, `3`)
  - Output: JSON object `{ category: { key, display_name, guideline_count }, guidelines: [{ id, title }] }`
- `update_guidelines`
  - Input: none
  - Output: JSON object `{ updated, commit, guideline_count }`

## License

MIT
