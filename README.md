<p align="center">
  <img src="assets/logo.jpeg" alt="search-cli" width="500">
</p>

<h1 align="center">search</h1>

<p align="center">
  <strong>One binary. Eleven providers. Fourteen modes. Zero dependencies.</strong><br>
  <em>Built for <a href="https://github.com/openclaw">OpenClaw</a> agents, Claude Code, and any AI that needs to search the web.</em>
</p>

<p align="center">
  <a href="#install">Install</a> &middot;
  <a href="#quick-start">Quick Start</a> &middot;
  <a href="#modes">Modes</a> &middot;
  <a href="#agent-integration">Agent Integration</a> &middot;
  <a href="#providers">Providers</a>
</p>

---

A single Rust binary that aggregates 11 search providers into one unified search interface. Designed from day one for AI agents — structured JSON output, semantic exit codes, machine-readable capabilities, and auto-JSON when piped.

Works with [OpenClaw](https://github.com/openclaw), Claude Code, Codex CLI, Gemini CLI, or any agent framework that can shell out to a command.

```bash
search "CRISPR gene therapy breakthroughs"
```

That's it. Auto-detects your intent, fans out to the right providers in parallel, deduplicates results, and returns them in under 2 seconds.

---

## Why

Every search API is good at something different. Brave has its own 35-billion page index. Serper gives you raw Google results plus Scholar, Patents, and Places. Exa does neural/semantic search and finds LinkedIn profiles. Perplexity gives AI-synthesized answers with citations using Sonar Pro. Jina reads any URL into clean markdown. Firecrawl renders JavaScript-heavy pages. xAI searches X/Twitter via Grok.

**search** routes your query to the right combination automatically — or lets you pick exactly which providers to use.

## Install

**Cargo (recommended):**
```bash
cargo install agent-search
```

**One-liner (macOS / Linux):**
```bash
curl -fsSL https://raw.githubusercontent.com/199-biotechnologies/search-cli/master/install.sh | sh
```

**Homebrew:**
```bash
brew tap 199-biotechnologies/tap
brew install search-cli
```

**From source:**
```bash
cargo install --git https://github.com/199-biotechnologies/search-cli
```

**Binary size:** ~6MB. **Startup:** ~2ms. **Memory:** ~5MB. No Python, no Node, no Docker.

## Quick Start

```bash
# 1. Set your API keys (any combination works — even just one)
search config set keys.brave YOUR_BRAVE_KEY
search config set keys.serper YOUR_SERPER_KEY
search config set keys.exa YOUR_EXA_KEY
search config set keys.jina YOUR_JINA_KEY
search config set keys.firecrawl YOUR_FIRECRAWL_KEY
search config set keys.tavily YOUR_TAVILY_KEY
search config set keys.serpapi YOUR_SERPAPI_KEY
search config set keys.perplexity YOUR_PERPLEXITY_KEY
search config set keys.browserless YOUR_BROWSERLESS_KEY
search config set keys.xai YOUR_XAI_KEY

# Or use environment variables
export SEARCH_KEYS_BRAVE=YOUR_KEY
export SEARCH_KEYS_EXA=YOUR_KEY
export SEARCH_KEYS_XAI=YOUR_KEY

# 2. Search
search "your query here"
```

## Usage

```bash
# Auto-detect mode (recommended — just type what you want)
search "quantum computing advances"
search "who is the CEO of Anthropic"
search "latest AI news today"
search "CRISPR research papers"
search "trending on twitter AI"

# Force a specific mode
search search -q "transformer architectures" -m academic
search search -q "Sam Altman" -m people
search search -q "AI startups 2026" -m news
search search -q "BRCA1 gene patent" -m patents
search search -q "coffee shops near me" -m places
search search -q "https://example.com" -m extract
search search -q "what are people saying about Rust" -m social

# Search X (Twitter) only
search --x "AI agents"                                   # shorthand for -m social -p xai
search search -q "trending AI" -m social                 # explicit social mode

# Pick specific providers
search search -q "machine learning" -p exa              # Exa only
search search -q "rust programming" -p brave,serper      # Brave + Serper
search search -q "trending AI" -p xai                    # xAI X/Twitter only
search search -q "https://arxiv.org/..." -p jina         # Jina reader only

# Control output
search "query" --json                  # Force JSON output
search "query" --json | jq '.results[].url'   # Pipe to jq
search "query" -c 20                   # Get 20 results
search "query" 2>/dev/null             # Suppress diagnostics
```

**Auto-JSON:** Output is automatically JSON when piped to another program. Human-readable tables when you're in a terminal.

## Modes

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

## Providers

| Provider | Strength | Best for |
|----------|----------|----------|
| **Brave** | Independent 35B-page index + LLM Context API | Web search, news, RAG-ready content chunks |
| **Serper** | Raw Google SERP + specialist endpoints | Scholar, patents, images, places, fact-checking |
| **Exa** | Neural/semantic search, category filters | Research papers, LinkedIn people, finding similar sites |
| **Jina** | Fast URL-to-markdown, 500 RPM free tier | Reading article content, quick extraction |
| **Firecrawl** | JavaScript rendering, structured extraction | Dynamic pages, SPAs, data extraction |
| **Tavily** | General, news, academic, deep search | Broad coverage, research-oriented queries |
| **SerpApi** | 80+ engines: Google, Bing, YouTube, Baidu | Scholar, multi-engine coverage |
| **Perplexity** | AI-powered answers with citations (Sonar Pro) | Complex queries, synthesized answers with sources |
| **Browserless** | Cloud browser for Cloudflare/JS-heavy pages | Anti-bot bypass, dynamic page rendering |
| **Stealth** | Anti-bot stealth scraper | Extracting content from protected pages |
| **xAI** | X/Twitter search via Grok AI | Tweets, trending topics, social sentiment |

## Agent Integration

Built for AI agents from day one. Every command supports `--json` and structured error codes.

```bash
# Discover capabilities programmatically
search agent-info

# Structured JSON with metadata
search "query" --json
# {
#   "version": "1",
#   "status": "success",
#   "query": "...",
#   "mode": "general",
#   "results": [...],
#   "metadata": {
#     "elapsed_ms": 1542,
#     "result_count": 10,
#     "providers_queried": ["brave", "serper", "exa"],
#     "providers_failed": []
#   }
# }

# Errors are also structured JSON
search "query" --json 2>&1
# {
#   "status": "error",
#   "error": {
#     "code": "no_providers",
#     "message": "No providers configured for mode 'general'",
#     "suggestion": "Configure at least one provider API key"
#   }
# }
```

**Exit codes are semantic:**
| Code | Meaning | Agent action |
|------|---------|-------------|
| 0 | Success | Process results |
| 1 | Runtime error | Retry might help |
| 2 | Config error | Fix configuration |
| 3 | Auth missing | Set API key |
| 4 | Rate limited | Back off and retry |

## Configuration

Config file lives at `~/.config/search/config.toml` (Linux) or `~/Library/Application Support/search/config.toml` (macOS).

```bash
search config show       # View current config (keys masked)
search config check      # Health check all providers
search config set K V    # Set a value
```

Environment variables override the config file. Prefix: `SEARCH_KEYS_`:

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

## How It Works

1. **Parse** — Clap parses your query, mode, provider filter, and output preferences
2. **Classify** — If mode is `auto`, regex-based intent classifier picks the right mode
3. **Route** — Mode determines which providers to query (or you override with `-p`)
4. **Fan out** — `tokio::JoinSet` fires all providers in parallel with per-provider timeouts
5. **Collect** — Results stream in as providers respond (no waiting for the slowest)
6. **Dedup** — URL normalization removes duplicates across providers
7. **Log** — Every search is logged to `~/Library/Application Support/search/logs/` (JSONL)
8. **Render** — JSON envelope or colored terminal table, auto-detected from context

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

## License

MIT

---

Created by [Boris Djordjevic](https://github.com/borisdjordjevic) at [199 Biotechnologies](https://github.com/199-biotechnologies).
