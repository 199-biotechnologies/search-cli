use hickory_resolver::Resolver;
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
    let resolver = Resolver::builder_tokio()
        .expect("failed to create DNS resolver")
        .build();

    let mut results = Vec::with_capacity(emails.len());
    for email in emails {
        results.push(verify_one(&resolver, email).await);
    }
    results
}

async fn verify_one(resolver: &hickory_resolver::TokioResolver, email: &str) -> VerifyResult {
    let email = email.trim().to_lowercase();

    // Step 1: Syntax check
    let parts: Vec<&str> = email.splitn(2, '@').collect();
    if parts.len() != 2 || parts[0].is_empty() || parts[1].is_empty() || !parts[1].contains('.') {
        return make_result(&email, "syntax_error", 0, "", false, false,
            "Invalid email format.");
    }
    let domain = parts[1];

    let is_disposable = DISPOSABLE_DOMAINS.contains(&domain);

    // Step 2: MX lookup
    let mx_host = match resolve_mx(resolver, domain).await {
        Some(host) => host,
        None => {
            return make_result(&email, "unreachable", 0, "", false, is_disposable,
                &format!("No MX records found for domain '{domain}'."));
        }
    };

    // Step 3: Catch-all probe — use a clearly-fake local part
    let catch_all_addr = format!("xvfy-probe-7f3a9b@{domain}");
    let is_catch_all = matches!(smtp_probe(&mx_host, &catch_all_addr).await, SmtpResult::Accepted(_));

    // Step 4: Real probe
    let result = smtp_probe(&mx_host, &email).await;

    // Step 5: Interpret result
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
                SmtpResult::Accepted(c) => {
                    if is_catch_all {
                        make_result(&email, "catch_all", c, &mx_host, true, is_disposable,
                            "Domain accepts all addresses. Email format likely valid but unverifiable.")
                    } else {
                        make_result(&email, "valid", c, &mx_host, false, is_disposable,
                            "Mailbox exists and accepts mail (passed greylist).")
                    }
                }
                SmtpResult::Rejected(c) => {
                    make_result(&email, "invalid", c, &mx_host, is_catch_all, is_disposable,
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

async fn resolve_mx(resolver: &hickory_resolver::TokioResolver, domain: &str) -> Option<String> {
    match resolver.mx_lookup(domain).await {
        Ok(mx) => {
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

    // RCPT TO — this is the actual probe
    let rcpt = format!("RCPT TO:<{email}>\r\n");
    if timeout(SMTP_TIMEOUT, writer.write_all(rcpt.as_bytes())).await.is_err() {
        return SmtpResult::Timeout;
    }
    line.clear();
    match timeout(SMTP_TIMEOUT, reader.read_line(&mut line)).await {
        Ok(Ok(_)) => {}
        _ => return SmtpResult::Timeout,
    }

    let code = parse_code(&line);

    // Always QUIT cleanly
    let _ = timeout(Duration::from_secs(2), writer.write_all(b"QUIT\r\n")).await;

    match code {
        250 | 251 => SmtpResult::Accepted(code),
        550..=554 => SmtpResult::Rejected(code),
        450 | 451 | 452 | 421 => SmtpResult::Greylisted(code),
        _ => SmtpResult::Rejected(code),
    }
}

async fn read_line(
    reader: &mut BufReader<tokio::net::tcp::OwnedReadHalf>,
    line: &mut String,
) -> Result<(), ()> {
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
    line.get(..3).and_then(|s| s.parse().ok()).unwrap_or(0)
}

fn make_result(
    email: &str, verdict: &str, smtp_code: u16, mx_host: &str,
    is_catch_all: bool, is_disposable: bool, suggestion: &str,
) -> VerifyResult {
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

        println!("  {} -> {}", email_display, verdict_display);
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
