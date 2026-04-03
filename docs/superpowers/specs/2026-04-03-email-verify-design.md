# Email Verification — Design Spec

## Goal

Add a `search verify <email>` subcommand that checks if email addresses exist via SMTP handshake without sending mail. Zero cost, no API key, LLM-optimized JSON output.

## Architecture

Native Rust SMTP verification over async TCP. Flow: syntax check → MX lookup → catch-all probe → RCPT TO probe → verdict. Single new file `src/verify.rs` (~200 lines) + CLI/output integration.

## CLI Surface

```bash
search verify user@example.com                    # single
search verify alice@stripe.com bob@gucci.com      # multiple args
search verify -f emails.txt                       # file (one per line)
echo "a@b.com" | search verify -f -               # stdin pipe
search verify user@example.com --json             # explicit JSON
```

New `Verify` variant in `Commands` enum. Sits alongside `Search`, `Config`, `AgentInfo`, `Providers`, `Update`.

## SMTP Engine

1. **Syntax check** — RFC 5321 basic validation (contains @, valid domain chars)
2. **MX lookup** — Use `hickory-resolver` async DNS to get MX records, sorted by priority
3. **Catch-all probe** — `RCPT TO:<{uuid}@domain>` — if 250, domain is catch-all
4. **Real probe** — `EHLO` → `MAIL FROM:<>` → `RCPT TO:<target>` → `QUIT`
5. **Greylist retry** — On 450/451/452, wait 5s, retry once
6. **Timeout** — 10s per connection

No email is ever sent (we QUIT before DATA).

## Verdicts (exhaustive enum)

| Verdict | SMTP Codes | Meaning |
|---------|-----------|---------|
| `valid` | 250 on strict domain | Mailbox confirmed |
| `invalid` | 550/551/553 | Mailbox rejected |
| `catch_all` | 250 but catch-all detected | Domain accepts everything |
| `unreachable` | Connection refused / no MX | Server down |
| `timeout` | No response in 10s | Server didn't respond |
| `syntax_error` | N/A | Not a valid email format |

## JSON Output

```json
{
  "version": "1",
  "status": "success",
  "results": [
    {
      "email": "user@example.com",
      "verdict": "valid",
      "smtp_code": 250,
      "mx_host": "mx.example.com",
      "is_catch_all": false,
      "is_disposable": false,
      "suggestion": "Mailbox exists and accepts mail."
    }
  ],
  "metadata": {
    "elapsed_ms": 1200,
    "verified_count": 1,
    "valid_count": 1,
    "invalid_count": 0,
    "catch_all_count": 0
  }
}
```

The `suggestion` field is plain English for LLMs that can't interpret SMTP codes.

## Table Output

Colored terminal table with verdict, email, MX host. Similar style to search results.

## Agent Info Update

Add `verify` section to `agent-info` JSON output with description, usage, verdicts array, examples, and notes.

## Dependencies

- `hickory-resolver` — async MX record lookup (only new dep)
- `tokio` — add `net` and `io-util` features (for TcpStream + AsyncBufRead)
- `uuid` or `rand` — for catch-all probe address (or use a hardcoded test string)

## Disposable Email Detection

Hardcoded list of ~50 common disposable email domains (mailinator.com, guerrillamail.com, etc.). Check domain against list. No external API needed.

## No Config Required

This feature requires zero API keys. Direct SMTP to the target MX server.
