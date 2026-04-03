# Email Verify Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `search verify <email>` subcommand for SMTP-based email verification with LLM-optimized output.

**Architecture:** Single `src/verify.rs` module (~200 lines) handles SMTP probing. CLI integration via new `Verify` variant in `Commands`. Output reuses existing `output::json::render_value` and a new table renderer.

**Tech Stack:** `hickory-resolver` for MX DNS, `tokio::net::TcpStream` for SMTP, existing `serde_json` for output.

---

### Task 1: Add dependencies

**Files:**
- Modify: `Cargo.toml`

- [ ] **Step 1: Add hickory-resolver and update tokio features**

In `Cargo.toml`, change the tokio line and add hickory-resolver:

```toml
tokio = { version = "1", features = ["rt-multi-thread", "macros", "time", "net", "io-util"] }
```

Add after the `rquest-util` line:

```toml
hickory-resolver = { version = "0.25", features = ["tokio-runtime"] }
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check 2>&1 | tail -3`
Expected: `Finished` with no errors

- [ ] **Step 3: Commit**

```bash
git add Cargo.toml Cargo.lock
git commit -m "deps: add hickory-resolver for MX lookup, enable tokio net+io-util"
```

---

### Task 2: Add verify types and CLI subcommand

**Files:**
- Modify: `src/cli.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Add Verify variant to Commands enum**

In `src/cli.rs`, add after the `Update` variant inside `Commands`:

```rust
    /// Verify if email addresses exist via SMTP (no API key needed)
    Verify(VerifyArgs),
```

Add the `VerifyArgs` struct after `ConfigAction`:

```rust
#[derive(Parser)]
pub struct VerifyArgs {
    /// Email addresses to verify
    pub emails: Vec<String>,

    /// Read emails from file (one per line, use - for stdin)
    #[arg(short, long)]
    pub file: Option<String>,
}
```

- [ ] **Step 2: Add stub handler in main.rs**

In `src/main.rs`, add the `mod verify;` declaration at the top (after `mod types;`):

```rust
mod verify;
```

In the `match command` block (after the `Commands::Update` arm), add:

```rust
        Commands::Verify(args) => {
            // Collect emails from args + file
            let mut emails: Vec<String> = args.emails;
            if let Some(ref path) = args.file {
                let content = if path == "-" {
                    use std::io::Read;
                    let mut buf = String::new();
                    std::io::stdin().read_to_string(&mut buf)?;
                    buf
                } else {
                    std::fs::read_to_string(path)?
                };
                emails.extend(
                    content.lines()
                        .map(|l| l.trim().to_string())
                        .filter(|l| !l.is_empty() && l.contains('@'))
                );
            }

            if emails.is_empty() {
                let err = errors::SearchError::Config("No email addresses provided. Usage: search verify user@example.com".into());
                match *format {
                    OutputFormat::Json => output::json::render_error(&err),
                    OutputFormat::Table => eprintln!("Error: {err}"),
                }
                return Ok(2);
            }

            let start = std::time::Instant::now();
            let results = verify::verify_emails(&emails).await;
            let elapsed = start.elapsed().as_millis();

            let valid_count = results.iter().filter(|r| r.verdict == "valid").count();
            let invalid_count = results.iter().filter(|r| r.verdict == "invalid").count();
            let catch_all_count = results.iter().filter(|r| r.verdict == "catch_all").count();

            let response = serde_json::json!({
                "version": "1",
                "status": "success",
                "results": results,
                "metadata": {
                    "elapsed_ms": elapsed,
                    "verified_count": results.len(),
                    "valid_count": valid_count,
                    "invalid_count": invalid_count,
                    "catch_all_count": catch_all_count,
                }
            });

            match *format {
                OutputFormat::Json => output::json::render_value(&response),
                OutputFormat::Table => verify::render_table(&results),
            }

            Ok(0)
        }
```

- [ ] **Step 3: Create stub verify.rs that compiles**

Create `src/verify.rs` with a stub:

```rust
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct VerifyResult {
    pub email: String,
    pub verdict: String,
    pub smtp_code: u16,
    pub mx_host: String,
    pub is_catch_all: bool,
    pub is_disposable: bool,
    pub suggestion: String,
}

pub async fn verify_emails(emails: &[String]) -> Vec<VerifyResult> {
    let mut results = Vec::new();
    for email in emails {
        results.push(VerifyResult {
            email: email.clone(),
            verdict: "valid".to_string(),
            smtp_code: 250,
            mx_host: "stub".to_string(),
            is_catch_all: false,
            is_disposable: false,
            suggestion: "Stub — not yet implemented.".to_string(),
        });
    }
    results
}

pub fn render_table(results: &[VerifyResult]) {
    for r in results {
        eprintln!("{} → {}", r.email, r.verdict);
    }
}
```

- [ ] **Step 4: Verify it compiles and runs**

Run: `cargo check 2>&1 | tail -3`
Run: `cargo run -- verify test@example.com --json 2>&1 | head -10`
Expected: JSON with stub "valid" result

- [ ] **Step 5: Commit**

```bash
git add src/cli.rs src/main.rs src/verify.rs
git commit -m "feat: add verify subcommand scaffold with stub SMTP engine"
```

---

### Task 3: Implement SMTP verification engine

**Files:**
- Modify: `src/verify.rs`

- [ ] **Step 1: Replace verify.rs with full implementation**

Replace `src/verify.rs` with the complete SMTP engine:

```rust
use hickory_resolver::config::{ResolverConfig, ResolverOpts};
use hickory_resolver::TokioAsyncResolver;
use owo_colors::OwoColorize;
use serde::Serialize;
use std::io::IsTerminal;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::time::{timeout, Duration};

const SMTP_TIMEOUT: Duration = Duration::from_secs(10);
const GREYLIST_DELAY: Duration = Duration::from_secs(5);

const DISPOSABLE_DOMAINS: &[&str] = &[
    "mailinator.com", "guerrillamail.com", "tempmail.com", "throwaway.email",
    "yopmail.com", "sharklasers.com", "guerrillamailblock.com", "grr.la",
    "dispostable.com", "trashmail.com", "mailnesia.com", "maildrop.cc",
    "discard.email", "tempail.com", "fakeinbox.com", "mailcatch.com",
    "temp-mail.org", "10minutemail.com", "mohmal.com", "burnermail.io",
    "inboxkitten.com", "emailondeck.com", "getnada.com", "tempr.email",
    "tmail.ws", "tmpmail.net", "tmpmail.org", "harakirimail.com",
    "mailsac.com", "spamgourmet.com", "jetable.org", "trash-mail.com",
    "mytemp.email", "boun.cr", "filzmail.com", "mailexpire.com",
    "tempinbox.com", "spamfree24.org", "mailforspam.com", "safetymail.info",
    "trashymail.com", "mailtemp.info", "temporarymail.com", "tempomail.fr",
    "mintemail.com", "discardmail.com", "mailnull.com", "spamhereplease.com",
];

#[derive(Debug, Clone, Serialize)]
pub struct VerifyResult {
    pub email: String,
    pub verdict: String,
    pub smtp_code: u16,
    pub mx_host: String,
    pub is_catch_all: bool,
    pub is_disposable: bool,
    pub suggestion: String,
}

pub async fn verify_emails(emails: &[String]) -> Vec<VerifyResult> {
    let resolver = TokioAsyncResolver::tokio(ResolverConfig::default(), ResolverOpts::default());

    let mut results = Vec::with_capacity(emails.len());
    for email in emails {
        results.push(verify_one(&resolver, email).await);
    }
    results
}

async fn verify_one(resolver: &TokioAsyncResolver, email: &str) -> VerifyResult {
    let email = email.trim().to_lowercase();

    // Step 1: Syntax check
    let parts: Vec<&str> = email.splitn(2, '@').collect();
    if parts.len() != 2 || parts[0].is_empty() || parts[1].is_empty() || !parts[1].contains('.') {
        return make_result(&email, "syntax_error", 0, "", false, false,
            "Invalid email format.");
    }
    let domain = parts[1];

    // Disposable check
    let is_disposable = DISPOSABLE_DOMAINS.contains(&domain);

    // Step 2: MX lookup
    let mx_host = match resolve_mx(resolver, domain).await {
        Some(host) => host,
        None => {
            return make_result(&email, "unreachable", 0, "", false, is_disposable,
                &format!("No MX records found for domain '{domain}'."));
        }
    };

    // Step 3: Catch-all probe
    let catch_all_probe = format!("verify-test-{}@{}", &email[..email.find('@').unwrap_or(0)].len(), domain);
    let is_catch_all = match smtp_probe(&mx_host, &catch_all_probe).await {
        SmtpResult::Accepted(_) => true,
        _ => false,
    };

    // Step 4: Real probe
    let result = smtp_probe(&mx_host, &email).await;

    // Step 5: Interpret
    match result {
        SmtpResult::Accepted(code) => {
            if is_catch_all {
                make_result(&email, "catch_all", code, &mx_host, true, is_disposable,
                    "Domain accepts all addresses. Email format likely valid but unverifiable.")
            } else {
                make_result(&email, "valid", code, &mx_host, false, is_disposable,
                    "Mailbox exists and accepts mail.")
            }
        }
        SmtpResult::Rejected(code) => {
            make_result(&email, "invalid", code, &mx_host, is_catch_all, is_disposable,
                "Mailbox does not exist.")
        }
        SmtpResult::Greylisted(code) => {
            // Retry once after delay
            tokio::time::sleep(GREYLIST_DELAY).await;
            match smtp_probe(&mx_host, &email).await {
                SmtpResult::Accepted(code2) => {
                    if is_catch_all {
                        make_result(&email, "catch_all", code2, &mx_host, true, is_disposable,
                            "Domain accepts all addresses. Email format likely valid but unverifiable.")
                    } else {
                        make_result(&email, "valid", code2, &mx_host, false, is_disposable,
                            "Mailbox exists and accepts mail (passed greylist).")
                    }
                }
                SmtpResult::Rejected(code2) => {
                    make_result(&email, "invalid", code2, &mx_host, is_catch_all, is_disposable,
                        "Mailbox does not exist.")
                }
                _ => {
                    make_result(&email, "unreachable", code, &mx_host, is_catch_all, is_disposable,
                        "Server greylisted the request and did not respond on retry.")
                }
            }
        }
        SmtpResult::Timeout => {
            make_result(&email, "timeout", 0, &mx_host, is_catch_all, is_disposable,
                "SMTP server did not respond within timeout.")
        }
        SmtpResult::Error(msg) => {
            make_result(&email, "unreachable", 0, &mx_host, is_catch_all, is_disposable,
                &format!("Connection failed: {msg}"))
        }
    }
}

async fn resolve_mx(resolver: &TokioAsyncResolver, domain: &str) -> Option<String> {
    match resolver.mx_lookup(domain).await {
        Ok(mx) => {
            // Get lowest priority (highest preference) MX record
            mx.into_iter()
                .min_by_key(|r| r.preference())
                .map(|r| r.exchange().to_string().trim_end_matches('.').to_string())
        }
        Err(_) => None,
    }
}

enum SmtpResult {
    Accepted(u16),
    Rejected(u16),
    Greylisted(u16),
    Timeout,
    Error(String),
}

async fn smtp_probe(mx_host: &str, email: &str) -> SmtpResult {
    let addr = format!("{mx_host}:25");

    let stream = match timeout(SMTP_TIMEOUT, TcpStream::connect(&addr)).await {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => return SmtpResult::Error(e.to_string()),
        Err(_) => return SmtpResult::Timeout,
    };

    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);
    let mut line = String::new();

    // Read greeting
    if read_line(&mut reader, &mut line).await.is_err() {
        return SmtpResult::Error("No greeting".into());
    }

    // EHLO
    if send_cmd(&mut writer, &mut reader, &mut line, "EHLO verify.local\r\n").await.is_err() {
        return SmtpResult::Error("EHLO failed".into());
    }

    // MAIL FROM
    if send_cmd(&mut writer, &mut reader, &mut line, "MAIL FROM:<>\r\n").await.is_err() {
        return SmtpResult::Error("MAIL FROM failed".into());
    }

    // RCPT TO — this is the probe
    let rcpt = format!("RCPT TO:<{email}>\r\n");
    if let Err(_) = timeout(SMTP_TIMEOUT, writer.write_all(rcpt.as_bytes())).await {
        return SmtpResult::Timeout;
    }
    line.clear();
    match timeout(SMTP_TIMEOUT, reader.read_line(&mut line)).await {
        Ok(Ok(_)) => {}
        _ => return SmtpResult::Timeout,
    }

    let code = parse_code(&line);

    // Always QUIT
    let _ = timeout(Duration::from_secs(2), writer.write_all(b"QUIT\r\n")).await;

    match code {
        250 | 251 => SmtpResult::Accepted(code),
        550 | 551 | 552 | 553 | 554 => SmtpResult::Rejected(code),
        450 | 451 | 452 | 421 => SmtpResult::Greylisted(code),
        _ => SmtpResult::Rejected(code),
    }
}

async fn read_line(reader: &mut BufReader<tokio::net::tcp::OwnedReadHalf>, line: &mut String) -> Result<(), ()> {
    line.clear();
    match timeout(SMTP_TIMEOUT, reader.read_line(line)).await {
        Ok(Ok(n)) if n > 0 => {
            // Read continuation lines (250-...)
            while line.len() >= 4 && line.as_bytes().get(3) == Some(&b'-') {
                let mut cont = String::new();
                match timeout(SMTP_TIMEOUT, reader.read_line(&mut cont)).await {
                    Ok(Ok(n)) if n > 0 => line.push_str(&cont),
                    _ => break,
                }
            }
            Ok(())
        }
        _ => Err(()),
    }
}

async fn send_cmd(
    writer: &mut tokio::net::tcp::OwnedWriteHalf,
    reader: &mut BufReader<tokio::net::tcp::OwnedReadHalf>,
    line: &mut String,
    cmd: &str,
) -> Result<u16, ()> {
    match timeout(SMTP_TIMEOUT, writer.write_all(cmd.as_bytes())).await {
        Ok(Ok(_)) => {}
        _ => return Err(()),
    }
    read_line(reader, line).await?;
    let code = parse_code(line);
    if code >= 400 { Err(()) } else { Ok(code) }
}

fn parse_code(line: &str) -> u16 {
    line.get(..3)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}

fn make_result(email: &str, verdict: &str, smtp_code: u16, mx_host: &str,
               is_catch_all: bool, is_disposable: bool, suggestion: &str) -> VerifyResult {
    VerifyResult {
        email: email.to_string(),
        verdict: verdict.to_string(),
        smtp_code,
        mx_host: mx_host.to_string(),
        is_catch_all,
        is_disposable,
        suggestion: suggestion.to_string(),
    }
}

pub fn render_table(results: &[VerifyResult]) {
    let use_color = std::io::stdout().is_terminal();

    if use_color {
        eprintln!("\n{}  Email Verification\n", "search".bold().cyan());
    }

    for r in results {
        let verdict_display = if use_color {
            match r.verdict.as_str() {
                "valid" => format!("{}", "VALID".green().bold()),
                "invalid" => format!("{}", "INVALID".red().bold()),
                "catch_all" => format!("{}", "CATCH-ALL".yellow().bold()),
                "unreachable" => format!("{}", "UNREACHABLE".red()),
                "timeout" => format!("{}", "TIMEOUT".yellow()),
                "syntax_error" => format!("{}", "SYNTAX ERROR".red()),
                _ => r.verdict.clone(),
            }
        } else {
            r.verdict.to_uppercase()
        };

        let email_display = if use_color {
            r.email.bold().to_string()
        } else {
            r.email.clone()
        };

        println!("  {} → {}", email_display, verdict_display);
        if !r.mx_host.is_empty() {
            if use_color {
                println!("    {} {}", "MX:".dimmed(), r.mx_host.dimmed());
            } else {
                println!("    MX: {}", r.mx_host);
            }
        }
        if use_color {
            println!("    {}", r.suggestion.dimmed());
        } else {
            println!("    {}", r.suggestion);
        }
        println!();
    }

    let valid = results.iter().filter(|r| r.verdict == "valid").count();
    let total = results.len();
    if use_color {
        eprintln!("  {}/{} verified as valid", valid.to_string().bold(), total);
    } else {
        eprintln!("  {}/{} verified as valid", valid, total);
    }
    eprintln!();
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check 2>&1 | tail -3`
Expected: `Finished` with no errors

- [ ] **Step 3: Test with real emails**

Run: `cargo run -- verify test@gmail.com totally.fake.nonexistent@gmail.com not-an-email --json 2>&1 | head -40`
Expected: JSON with mixed verdicts

- [ ] **Step 4: Commit**

```bash
git add src/verify.rs
git commit -m "feat: implement native SMTP email verification engine"
```

---

### Task 4: Update agent-info and bump version

**Files:**
- Modify: `src/main.rs` (agent-info section)
- Modify: `Cargo.toml` (version bump)

- [ ] **Step 1: Update agent-info output**

In `src/main.rs`, in the `Commands::AgentInfo` arm, update the `info` JSON to add verify:

Change the `"commands"` line to:
```rust
"commands": ["search", "verify", "config show", "config set", "config check", "agent-info", "providers", "update"],
```

Add after `"auto_json_when_piped": true,`:
```rust
"verify": {
    "description": "Check if email addresses exist via SMTP without sending mail. No API key required.",
    "usage": "search verify <email> [<email>...] [-f <file>] [--json]",
    "verdicts": ["valid", "invalid", "catch_all", "unreachable", "timeout", "syntax_error"],
    "examples": [
        "search verify alice@stripe.com",
        "search verify alice@stripe.com bob@gucci.com --json",
        "search verify -f emails.txt"
    ],
    "notes": "No API key required. Uses direct SMTP. catch_all means domain accepts all addresses — email format likely correct but unverifiable. is_disposable flags throwaway email services."
},
```

- [ ] **Step 2: Bump version to 0.5.0**

In `Cargo.toml`, change:
```toml
version = "0.5.0"
```

Update description:
```toml
description = "Unified multi-provider search CLI for AI agents — 12 providers, 14 modes, email verification, one binary"
```

- [ ] **Step 3: Verify agent-info output**

Run: `cargo run -- agent-info 2>&1 | python3 -c "import json,sys; d=json.load(sys.stdin); print('verify' in d, 'verify' in d.get('commands',[]))"`
Expected: `True True`

- [ ] **Step 4: Commit**

```bash
git add src/main.rs Cargo.toml
git commit -m "v0.5.0: Add email verification subcommand — native SMTP, zero-config, LLM-optimized"
```

---

### Task 5: Test, push, publish

- [ ] **Step 1: Full integration test**

```bash
cargo run -- verify real@gmail.com fake12345678@gmail.com not-valid --json
```

- [ ] **Step 2: Push to GitHub**

```bash
git push origin master
```

- [ ] **Step 3: Install locally**

```bash
cargo install --path .
cp ~/.cargo/bin/search /usr/local/bin/search
search --version
search verify test@gmail.com
```

- [ ] **Step 4: Publish to crates.io**

```bash
cargo publish
```
