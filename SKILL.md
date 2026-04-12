---
name: search
description: >
  Multi-provider search CLI with 14 modes. Run `search agent-info` for full
  capabilities, flags, and exit codes.
---

## search

Agent-friendly multi-provider search CLI. Run `search agent-info` for the
machine-readable capability manifest.

Quick examples:
- `search "rust error handling"` — auto-detect mode
- `search search -q "CRISPR" -m academic` — academic papers
- `search search -q "AI news" -m news --json` — JSON output
- `search verify alice@stripe.com --json` — email verification
- `search --x "trending AI"` — X/Twitter search

## Not suited for (use these instead)

- **GitHub repos/code/issues/PRs** → use `gh` CLI (GitHub's own search API):
  - `gh search repos "query" --language=rust --sort=stars --json fullName,description,stargazersCount,url`
  - `gh search code "query" --language=go --json path,repository`
  - `gh search issues "query" --state=open --json title,url,state`
