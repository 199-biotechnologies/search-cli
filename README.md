<div align="center">

# Search CLI

**One binary, 11 providers, 14 modes. The web search tool your AI agent is missing.**

<br />

[![Star this repo](https://img.shields.io/github/stars/199-biotechnologies/search-cli?style=for-the-badge&logo=github&label=%E2%AD%90%20Star%20this%20repo&color=yellow)](https://github.com/199-biotechnologies/search-cli/stargazers)
&nbsp;&nbsp;
[![Follow @longevityboris](https://img.shields.io/badge/Follow_%40longevityboris-000000?style=for-the-badge&logo=x&logoColor=white)](https://x.com/longevityboris)

<br />

[![Crates.io](https://img.shields.io/crates/v/agent-search?style=for-the-badge&logo=rust&logoColor=white&label=crates.io)](https://crates.io/crates/agent-search)
[![Downloads](https://img.shields.io/crates/d/agent-search?style=for-the-badge&logo=rust&logoColor=white)](https://crates.io/crates/agent-search)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue?style=for-the-badge)](LICENSE)
[![Build](https://img.shields.io/github/actions/workflow/status/199-biotechnologies/search-cli/ci.yml?style=for-the-badge&logo=github)](https://github.com/199-biotechnologies/search-cli/actions)

---

A single Rust binary that aggregates Brave, Serper, Exa, Jina, Firecrawl, Tavily, SerpApi, Perplexity, xAI, and more into one unified search interface. Designed from day one for AI agents -- structured JSON output, semantic exit codes, auto-JSON when piped, and parallel fan-out across providers in under 2 seconds.

[Install](#install) | [How It Works](#how-it-works) | [Features](#features) | [Providers](#providers) | [Contributing](#contributing)

</div>

## Why This Exists

Every search API is good at something different. Brave has its own 35-billion page index. Serper gives you raw Google results plus Scholar, Patents, and Places. Exa does neural/semantic search. Perplexity gives AI-synthesized answers with citations. Jina reads any URL into clean markdown. Firecrawl renders JavaScript-heavy pages. xAI searches X/Twitter.

You shouldn't have to wire up each one separately, handle their different response formats, manage rate limits, or figure out which provider to use for which query type. `search` does all of that for you -- routes your query to the right combination automatically, fans out in parallel, deduplicates results, and gives you a single clean response.

```bash
search "CRISPR gene therapy breakthroughs"
```

That's it. Auto-detects your intent, picks the right providers, and returns results in under 2 seconds.

## Install

**Cargo (recommended):**
```bash
cargo install agent-search
```

**Homebrew:**
```bash
brew tap 199-biotechnologies/tap
brew install search-cli
```

**One-liner (macOS / Linux):**
```bash
curl -fsSL https://raw.githubusercontent.com/199-biotechnologies/search-cli/master/install.sh | sh
```

**From source:**
```bash
cargo install --git https://github.com/199-biotechnologies/search-cli
```

Binary size is ~6 MB. Startup is ~2 ms. Memory is ~5 MB. No Python, no Node, no Docker.

## Quick Start

```bash
# Set your API keys (any combination works -- even just one)
search config set keys.brave YOUR_BRAVE_KEY
search config set keys.serper YOUR_SERPER_KEY
search config set keys.exa YOUR_EXA_KEY

# Or use environment variables
export SEARCH_KEYS_BRAVE=YOUR_KEY
export SEARCH_KEYS_EXA=YOUR_KEY

# Search
search "your query here"
```

## How It Works

```
                          ┌─────────────┐
                          │  Your Query │
                          └──────┬──────┘
                                 │
                          ┌──────▼──────┐
                          │   Classify  │  regex-based intent detection
                          └──────┬──────┘
                                 │
                    ┌────────────┼────────────┐
                    ▼            ▼            ▼
              ┌──────────┐ ┌──────────┐ ┌──────────┐
              │  Brave   │ │  Serper  │ │   Exa    │  parallel fan-out
              └────┬─────┘ └────┬─────┘ └────┬─────┘  via tokio::JoinSet
                   │            │            │
                   └────────────┼────────────┘
                                │
                         ┌──────▼──────┐
                         │   Dedup &   │  URL normalization
                         │   Merge     │  across providers
                         └──────┬──────┘
                                │
                    ┌───────────┴───────────┐
                    ▼                       ▼
             ┌────────────┐         ┌────────────┐
             │    JSON    │         │   Table    │
             │  (piped)   │         │ (terminal) │
             └────────────┘         └────────────┘
```

1. **Parse** -- Clap parses your query, mode, provider filter, and output preferences
2. **Classify** -- If mode is `auto`, regex-based intent classifier picks the right mode
3. **Route** -- Mode determines which providers to query (or you override with `-p`)
4. **Fan out** -- `tokio::JoinSet` fires all providers in parallel with per-provider timeouts
5. **Collect** -- Results stream in as providers respond (no waiting for the slowest)
6. **Dedup** -- URL normalization removes duplicates across providers
7. **Render** -- JSON envelope when piped, colored terminal table when interactive

## Features

### 14 Search Modes

| Mode | What it does | Providers used |
|------|-------------|----------------|
| `auto` | Detects intent from your query | *varies* |
| `general` | Broad web search | Brave + Serper + Exa + Jina + Tavily + Perplexity |
| `news` | Breaking news, current events | Brave News + Serper News + Tavily + Perplexity |
| `academic` | Research papers, studies | Exa + Serper + Tavily + Perplexity |
| `people` | LinkedIn profiles, bios | Exa |
| `deep` | Maximum coverage | Brave (LLM Context) + Exa + Serper + Tavily + Perplexity + xAI |
| `scholar` | Google Scholar | Serper + SerpApi |
| `patents` | Patent search | Serper |
| `images` | Image search | Serper |
| `places` | Local businesses, maps | Serper |
| `extract` | Full text from a URL | Stealth -> Jina -> Firecrawl -> Browserless |
| `scrape` | Page scraping | Stealth -> Jina -> Firecrawl -> Browserless |
| `similar` | Find similar pages to a URL | Exa |
| `social` | X/Twitter social search | xAI (Grok) |

### Agent-First Design

Built for Claude Code, Codex CLI, Gemini CLI, [OpenClaw](https://github.com/openclaw), and any AI agent that can shell out to a command.

```bash
# Discover capabilities programmatically
search agent-info

# Structured JSON with metadata
search "query" --json
# {
#   "status": "success",
#   "query": "...",
#   "mode": "general",
#   "results": [...],
#   "metadata": {
#     "elapsed_ms": 1542,
#     "result_count": 10,
#     "providers_queried": ["brave", "serper", "exa"]
#   }
# }
```

**Auto-JSON:** Output is automatically JSON when piped to another program. Human-readable tables when you're in a terminal.

**Semantic exit codes:**

| Code | Meaning | Agent action |
|------|---------|-------------|
| 0 | Success | Process results |
| 1 | Runtime error | Retry might help |
| 2 | Config error | Fix configuration |
| 3 | Auth missing | Set API key |
| 4 | Rate limited | Back off and retry |

### Usage Examples

```bash
# Auto-detect mode (just type what you want)
search "quantum computing advances"
search "who is the CEO of Anthropic"
search "CRISPR research papers"

# Force a specific mode
search search -q "transformer architectures" -m academic
search search -q "Sam Altman" -m people
search search -q "AI startups 2026" -m news
search search -q "BRCA1 gene patent" -m patents

# Search X (Twitter) only
search --x "AI agents"

# Pick specific providers
search search -q "machine learning" -p exa
search search -q "rust programming" -p brave,serper

# Control output
search "query" --json | jq '.results[].url'
search "query" -c 20                   # 20 results
search "query" 2>/dev/null             # suppress diagnostics
```

## Providers

| Provider | What it does | Best for |
|----------|-------------|----------|
| **[Brave](https://brave.com/search/api/)** | Independent 35B-page index + LLM Context API | Web search, news, RAG-ready content |
| **[Serper](https://serper.dev/)** | Raw Google SERP + specialist endpoints | Scholar, patents, images, places |
| **[Exa](https://exa.ai/)** | Neural/semantic search, category filters | Research papers, people search, similar sites |
| **[Jina](https://jina.ai/)** | Fast URL-to-markdown, 500 RPM free tier | Reading article content, quick extraction |
| **[Firecrawl](https://firecrawl.dev/)** | JavaScript rendering, structured extraction | Dynamic pages, SPAs, data extraction |
| **[Tavily](https://tavily.com/)** | General + deep search, research-focused | Broad coverage, research queries |
| **[SerpApi](https://serpapi.com/)** | 80+ engines: Google, Bing, YouTube, Baidu | Scholar, multi-engine coverage |
| **[Perplexity](https://perplexity.ai/)** | AI-powered answers with citations (Sonar Pro) | Complex queries, synthesized answers |
| **Browserless** | Cloud browser for Cloudflare/JS-heavy pages | Anti-bot bypass, dynamic rendering |
| **Stealth** | Built-in anti-bot scraper | Protected pages, no API key needed |
| **[xAI](https://x.ai/)** | X/Twitter search via Grok AI | Tweets, trending topics, social sentiment |

## Configuration

Config file lives at `~/.config/search/config.toml` (Linux) or `~/Library/Application Support/search/config.toml` (macOS).

```bash
search config show       # View current config (keys masked)
search config check      # Health check all providers
search config set K V    # Set a value
```

Environment variables override the config file. Prefix with `SEARCH_KEYS_`:

```bash
export SEARCH_KEYS_BRAVE=your-key
export SEARCH_KEYS_SERPER=your-key
export SEARCH_KEYS_EXA=your-key
export SEARCH_KEYS_JINA=your-key
export SEARCH_KEYS_FIRECRAWL=your-key
export SEARCH_KEYS_TAVILY=your-key
export SEARCH_KEYS_SERPAPI=your-key
export SEARCH_KEYS_PERPLEXITY=your-key
export SEARCH_KEYS_BROWSERLESS=your-key
export SEARCH_KEYS_XAI=your-key
```

## Updating

```bash
search update             # Self-update from GitHub releases
search update --check     # Check without installing
```

## Building from Source

```bash
git clone https://github.com/199-biotechnologies/search-cli
cd search-cli
cargo build --release
# Binary at target/release/search
```

## Contributing

Contributions are welcome. See [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

## License

[MIT](LICENSE)

---

<div align="center">

Built by [Boris Djordjevic](https://github.com/longevityboris) at [199 Biotechnologies](https://github.com/199-biotechnologies) | [Paperfoot AI](https://paperfoot.ai)

<br />

**If this is useful to you:**

[![Star this repo](https://img.shields.io/github/stars/199-biotechnologies/search-cli?style=for-the-badge&logo=github&label=%E2%AD%90%20Star%20this%20repo&color=yellow)](https://github.com/199-biotechnologies/search-cli/stargazers)
&nbsp;&nbsp;
[![Follow @longevityboris](https://img.shields.io/badge/Follow_%40longevityboris-000000?style=for-the-badge&logo=x&logoColor=white)](https://x.com/longevityboris)

</div>
