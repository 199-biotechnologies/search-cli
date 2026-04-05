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
