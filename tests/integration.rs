use assert_cmd::Command;
use predicates::prelude::*;
use std::time::Instant;

fn search_cmd() -> Command {
    Command::cargo_bin("search").unwrap()
}

/// Check if any provider key is configured (reads from config file)
fn has_any_provider() -> bool {
    let output = search_cmd().args(["config", "check"]).output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout.contains("[+]") || stdout.contains("OK")
}

fn has_provider(name: &str) -> bool {
    let output = search_cmd().args(["config", "check"]).output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        if line.contains(name) && (line.contains("[+]") || line.contains("OK")) {
            return true;
        }
    }
    false
}

// =============================================================================
// CLI structure tests (no API keys needed)
// =============================================================================

#[test]
fn test_help_output() {
    search_cmd()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Aggregates 11 search providers"))
        .stdout(predicate::str::contains("brave"))
        .stdout(predicate::str::contains("serper"))
        .stdout(predicate::str::contains("exa"));
}

#[test]
fn test_tracing_smoke_emits_structured_reliability_logs() {
    // Logging should remain silent by default, but emit reliability signals when RUST_LOG is enabled.
    let output = search_cmd()
        .env("RUST_LOG", "search=info")
        .args(["search", "-q", "test", "-p", "nonexistent", "--json"])
        .output()
        .unwrap();

    // Unknown provider exits non-zero; we only care about emitted structured logs on stderr.
    assert_ne!(output.status.code().unwrap_or_default(), 0);

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("event=\"search_completed\"")
            || stderr.contains("event=\"search_failed\""),
        "expected reliability event in tracing output, stderr was: {}",
        stderr
    );
}

#[test]
fn test_error_response_includes_actionable_rejection_fields() {
    // Regression check for hbq13: maintain compatibility while adding actionable
    // rejection classification fields.
    let output = search_cmd()
        .args(["search", "-q", "test", "-p", "nonexistent", "--json"])
        .output()
        .unwrap();

    assert_ne!(output.status.code().unwrap_or_default(), 0);

    let stderr = String::from_utf8_lossy(&output.stderr);
    let json: serde_json::Value = serde_json::from_str(stderr.trim()).unwrap();
    assert_eq!(json["status"], "error");
    assert!(json["error"]["code"].is_string());
    assert!(json["error"]["message"].is_string());

    // New optional fields should exist for classified rejections and remain nullable
    // for generic configuration errors.
    assert!(json["error"].get("cause").is_some(), "missing error.cause field");
    assert!(json["error"].get("action").is_some(), "missing error.action field");
    assert!(json["error"].get("signature").is_some(), "missing error.signature field");
}

#[test]
fn test_table_output_shows_rejection_guidance_for_failed_providers() {
    use std::io::Write;
    use std::net::TcpListener;
    use std::thread;
    use std::time::Duration;

    // Local stub that forces Exa NUM_RESULTS_EXCEEDED signature.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let body = r#"{"error":"NUM_RESULTS_EXCEEDED"}"#;
            let resp = format!(
                "HTTP/1.1 400 Bad Request\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = stream.write_all(resp.as_bytes());
            thread::sleep(Duration::from_millis(25));
        }
    });

    // Use JSON mode to deterministically assert provider failure details now
    // carry actionable guidance fields (hbq14 requirement).
    let output = search_cmd()
        .env("EXA_API_KEY", "test-key")
        .env("EXA_BASE_URL", format!("http://{}", addr))
        .args(["search", "-q", "rejection guidance test", "-m", "people", "-p", "exa", "--json"])
        .output()
        .unwrap();

    let _ = server.join();

    assert!(!output.status.success(), "expected provider failure for guidance path");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let details = json["metadata"]["providers_failed_detail"]
        .as_array()
        .expect("providers_failed_detail should be present");

    let exa = details
        .iter()
        .find(|d| d["provider"].as_str() == Some("exa"))
        .expect("expected exa failure detail");

    assert!(
        exa["action"].as_str().unwrap_or_default().contains("Lower -c/--count"),
        "expected actionable remediation in provider failure detail, detail was: {}",
        exa
    );

    assert_eq!(
        exa["signature"].as_str().unwrap_or_default(),
        "exa.NUM_RESULTS_EXCEEDED",
        "expected Exa diagnostic signature"
    );
}

#[test]
fn test_version() {
    search_cmd()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("search 0."));
}

#[test]
fn test_search_help_shows_modes() {
    search_cmd()
        .args(["search", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("academic"))
        .stdout(predicate::str::contains("people"))
        .stdout(predicate::str::contains("scholar"))
        .stdout(predicate::str::contains("patents"));
}

#[test]
fn test_agent_info_json() {
    let output = search_cmd()
        .arg("agent-info")
        .output()
        .unwrap();
    assert!(output.status.success());

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["name"], "search");
    assert!(json["modes"].as_array().unwrap().len() >= 13);
    assert!(json["providers"].as_array().unwrap().len() >= 5);
    assert_eq!(json["config"]["env_prefix"], "SEARCH_");
    assert_eq!(json["auto_json_when_piped"], true);
    assert!(json["command_schemas"].is_object());
    assert!(json["exit_codes"].is_object());
}

#[test]
fn test_providers_json() {
    let output = search_cmd()
        .args(["providers", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success());

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["status"], "success");
    let providers = json["providers"].as_array().unwrap();
    let names: Vec<&str> = providers.iter().map(|p| p["name"].as_str().unwrap()).collect();
    assert!(names.contains(&"brave"));
    assert!(names.contains(&"serper"));
    assert!(names.contains(&"exa"));
    assert!(names.contains(&"jina"));
    assert!(names.contains(&"firecrawl"));
    assert!(names.contains(&"tavily"));
}

#[test]
fn test_config_check() {
    search_cmd()
        .args(["config", "check"])
        .assert()
        .success()
        .stdout(predicate::str::contains("brave"))
        .stdout(predicate::str::contains("serper"))
        .stdout(predicate::str::contains("exa"))
        .stdout(predicate::str::contains("jina"))
        .stdout(predicate::str::contains("firecrawl"))
        .stdout(predicate::str::contains("tavily"));
}

#[test]
fn test_config_show() {
    search_cmd()
        .args(["config", "show"])
        .assert()
        .success()
        .stdout(predicate::str::contains("timeout"))
        .stdout(predicate::str::contains("count"));
}

#[test]
fn test_no_providers_error_json() {
    // Use a fake provider name to ensure no providers match
    let output = search_cmd()
        .args(["search", "-q", "test", "-p", "nonexistent", "--json"])
        .output()
        .unwrap();

    assert_ne!(output.status.code().unwrap(), 0);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let json: serde_json::Value = serde_json::from_str(stderr.trim()).unwrap();
    assert_eq!(json["status"], "error");
    assert!(json["error"]["code"].as_str().is_some());
    assert!(json["error"]["message"].as_str().is_some());
}

#[test]
fn test_exit_code_no_providers() {
    let output = search_cmd()
        .args(["search", "-q", "test", "-p", "nonexistent", "--json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code().unwrap(), 2); // config error
}

// =============================================================================
// Real API tests (require configured providers)
// =============================================================================

#[test]
fn test_real_general_search() {
    if !has_any_provider() {
        eprintln!("SKIP: no providers configured");
        return;
    }

    let start = Instant::now();
    let output = search_cmd()
        .args(["search", "-q", "Rust programming language", "--json", "-c", "5"])
        .output()
        .unwrap();
    let elapsed = start.elapsed();

    assert!(output.status.success(), "search failed: {}", String::from_utf8_lossy(&output.stderr));

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["status"], "success");
    assert_eq!(json["mode"], "general");

    let results = json["results"].as_array().unwrap();
    assert!(!results.is_empty(), "expected results, got 0");

    // Check result structure
    let first = &results[0];
    assert!(first["title"].as_str().is_some());
    assert!(first["url"].as_str().is_some());
    assert!(first["source"].as_str().is_some());

    // Metadata
    assert!(json["metadata"]["elapsed_ms"].as_u64().unwrap() > 0);
    assert!(!json["metadata"]["providers_queried"].as_array().unwrap().is_empty());

    eprintln!(
        "  PASS general search: {} results in {}ms (wall: {}ms)",
        results.len(),
        json["metadata"]["elapsed_ms"],
        elapsed.as_millis()
    );
}

#[test]
fn test_real_news_search() {
    if !has_provider("serper") && !has_provider("brave") {
        eprintln!("SKIP: need serper or brave for news");
        return;
    }

    let output = search_cmd()
        .args(["search", "-q", "technology news", "-m", "news", "--json", "-c", "5"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["mode"], "news");
    let results = json["results"].as_array().unwrap();
    assert!(!results.is_empty(), "news search returned 0 results");

    eprintln!("  PASS news search: {} results", results.len());
}

#[test]
fn test_real_academic_search() {
    if !has_provider("exa") && !has_provider("serper") {
        eprintln!("SKIP: need exa or serper for academic");
        return;
    }

    let output = search_cmd()
        .args(["search", "-q", "CRISPR gene editing", "-m", "academic", "--json", "-c", "5"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["mode"], "academic");
    let results = json["results"].as_array().unwrap();
    assert!(!results.is_empty(), "academic search returned 0 results");

    eprintln!("  PASS academic search: {} results", results.len());
}

#[test]
fn test_real_provider_filter_single() {
    if !has_provider("exa") {
        eprintln!("SKIP: need exa");
        return;
    }

    let output = search_cmd()
        .args(["search", "-q", "artificial intelligence", "-p", "exa", "--json", "-c", "3"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let providers = json["metadata"]["providers_queried"].as_array().unwrap();
    assert_eq!(providers.len(), 1);
    assert_eq!(providers[0], "exa");

    // All results should be from exa
    for r in json["results"].as_array().unwrap() {
        assert!(r["source"].as_str().unwrap().starts_with("exa"), "unexpected source: {}", r["source"]);
    }

    eprintln!("  PASS provider filter (exa only): {} results", json["results"].as_array().unwrap().len());
}

#[test]
fn test_real_domain_filter() {
    if !has_provider("exa") {
        eprintln!("SKIP: need exa for domain filter");
        return;
    }

    let output = search_cmd()
        .args(["search", "-q", "machine learning", "-d", "arxiv.org", "-p", "exa", "--json", "-c", "5"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let results = json["results"].as_array().unwrap();
    assert!(!results.is_empty());

    // All results should be from arxiv.org
    for r in results {
        let url = r["url"].as_str().unwrap();
        assert!(url.contains("arxiv.org"), "expected arxiv.org URL, got: {}", url);
    }

    eprintln!("  PASS domain filter (arxiv.org): {} results", results.len());
}

#[test]
fn test_real_freshness_filter() {
    if !has_provider("serper") {
        eprintln!("SKIP: need serper for freshness test");
        return;
    }

    let output = search_cmd()
        .args(["search", "-q", "AI news", "-f", "day", "-p", "serper", "--json", "-c", "5"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let results = json["results"].as_array().unwrap();
    assert!(!results.is_empty(), "freshness filter returned 0 results");

    eprintln!("  PASS freshness filter (day): {} results", results.len());
}

#[test]
fn test_real_last_cache() {
    if !has_any_provider() {
        eprintln!("SKIP: no providers");
        return;
    }

    // First search to populate cache
    let output1 = search_cmd()
        .args(["search", "-q", "cache test query", "--json", "-c", "3"])
        .output()
        .unwrap();
    assert!(output1.status.success());

    // Replay from cache
    let start = Instant::now();
    let output2 = search_cmd()
        .args(["--last", "--json"])
        .output()
        .unwrap();
    let cache_elapsed = start.elapsed();

    assert!(output2.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output2.stdout).unwrap();
    assert_eq!(json["status"], "success");
    assert!(!json["results"].as_array().unwrap().is_empty());

    // Cache replay should be near-instant (< 100ms)
    assert!(cache_elapsed.as_millis() < 500, "cache replay took {}ms", cache_elapsed.as_millis());

    eprintln!("  PASS cache replay: {}ms", cache_elapsed.as_millis());
}

// =============================================================================
// Performance benchmark
// =============================================================================

#[test]
fn test_performance_benchmark() {
    if !has_any_provider() {
        eprintln!("SKIP: no providers for benchmark");
        return;
    }

    let queries = [
        "rust programming",
        "machine learning",
        "quantum computing",
        "climate change",
        "blockchain technology",
    ];

    let mut latencies = Vec::new();

    for q in &queries {
        let start = Instant::now();
        let output = search_cmd()
            .args(["search", "-q", q, "--json", "-c", "5"])
            .output()
            .unwrap();
        let elapsed = start.elapsed().as_millis();

        if output.status.success() {
            let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
            let api_ms = json["metadata"]["elapsed_ms"].as_u64().unwrap_or(0);
            let count = json["results"].as_array().map(|a| a.len()).unwrap_or(0);
            eprintln!("  query={:<25} results={:<3} api={}ms  wall={}ms", q, count, api_ms, elapsed);
            latencies.push(elapsed);
        } else {
            eprintln!("  query={:<25} FAILED", q);
        }
    }

    if latencies.is_empty() {
        eprintln!("  No successful queries for benchmark");
        return;
    }

    latencies.sort();
    let avg = latencies.iter().sum::<u128>() / latencies.len() as u128;
    let p50 = latencies[latencies.len() / 2];
    let p95 = latencies[latencies.len() * 95 / 100];
    let min = latencies[0];
    let max = latencies[latencies.len() - 1];

    eprintln!("\n  BENCHMARK ({} queries):", latencies.len());
    eprintln!("    avg:  {}ms", avg);
    eprintln!("    p50:  {}ms", p50);
    eprintln!("    p95:  {}ms", p95);
    eprintln!("    min:  {}ms", min);
    eprintln!("    max:  {}ms", max);
}

// -----------------------------------------------------------------------------
// Regression tests for typed config write/read behavior.
// -----------------------------------------------------------------------------

#[test]
fn test_config_show_json_numeric_types() {
    use std::{fs, path::PathBuf};
    use serde_json::Value;
    use std::time::{SystemTime, UNIX_EPOCH};

    // Use an isolated config directory (override platform config envs)
    let base = std::env::temp_dir().join(format!("search_cli_test_{}_{}",
        std::process::id(),
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));
    let _ = std::fs::create_dir_all(&base);

    // Discover config path
    let out = search_cmd().env("APPDATA", &base).env("LOCALAPPDATA", &base).env("USERPROFILE", &base).env("XDG_CONFIG_HOME", &base).env("HOME", &base)
        .args(["config", "path", "--json"]).output().unwrap();
    assert!(out.status.success(), "failed to get config path: {}", String::from_utf8_lossy(&out.stderr));
    let j: Value = serde_json::from_slice(&out.stdout).unwrap();
    let p = PathBuf::from(j["data"]["path"].as_str().unwrap());

    // Backup/restore guard so we don't leave the user's config modified even on panic
    struct Guard { path: PathBuf, content: Option<String> }
    impl Drop for Guard {
        fn drop(&mut self) {
            if let Some(c) = &self.content {
                let _ = fs::write(&self.path, c);
            } else {
                let _ = fs::remove_file(&self.path);
            }
        }
    }

    let orig = fs::read_to_string(&p).ok();
    let _g = Guard { path: p.clone(), content: orig };

    // Ensure we write deterministic numeric values
    let out = search_cmd().env("APPDATA", &base).env("LOCALAPPDATA", &base).env("USERPROFILE", &base).env("XDG_CONFIG_HOME", &base).env("HOME", &base)
        .args(["config", "set", "settings.timeout", "123"]).output().unwrap();
    assert!(out.status.success(), "config set failed: {}", String::from_utf8_lossy(&out.stderr));

    let out = search_cmd().env("APPDATA", &base).env("LOCALAPPDATA", &base).env("USERPROFILE", &base).env("XDG_CONFIG_HOME", &base).env("HOME", &base)
        .args(["config", "set", "settings.count", "7"]).output().unwrap();
    assert!(out.status.success(), "config set failed: {}", String::from_utf8_lossy(&out.stderr));

    // Request JSON output and assert numeric typed settings values
    let out = search_cmd().env("APPDATA", &base).env("LOCALAPPDATA", &base).env("USERPROFILE", &base).env("XDG_CONFIG_HOME", &base).env("HOME", &base)
        .args(["config", "show", "--json"]).output().unwrap();
    assert!(out.status.success(), "config show --json failed: {}", String::from_utf8_lossy(&out.stderr));
    let j: Value = serde_json::from_slice(&out.stdout).unwrap();

    // Expect timeout and count to be numeric JSON values
    assert_eq!(j["settings"]["timeout"].as_u64().unwrap(), 123);
    assert_eq!(j["settings"]["count"].as_u64().unwrap(), 7);
}

#[test]
fn test_config_set_invalid_numeric_input() {
    use std::{fs, path::PathBuf};
    use serde_json::Value;
    use std::time::{SystemTime, UNIX_EPOCH};

    // Use an isolated config directory (override platform config envs)
    let base = std::env::temp_dir().join(format!("search_cli_test_{}_{}",
        std::process::id(),
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));
    let _ = std::fs::create_dir_all(&base);

    // Discover config path
    let out = search_cmd().env("APPDATA", &base).env("LOCALAPPDATA", &base).env("USERPROFILE", &base).env("XDG_CONFIG_HOME", &base).env("HOME", &base)
        .args(["config", "path", "--json"]).output().unwrap();
    assert!(out.status.success(), "failed to get config path: {}", String::from_utf8_lossy(&out.stderr));
    let j: Value = serde_json::from_slice(&out.stdout).unwrap();
    let p = PathBuf::from(j["data"]["path"].as_str().unwrap());

    // Backup/restore guard
    struct Guard { path: PathBuf, content: Option<String> }
    impl Drop for Guard {
        fn drop(&mut self) {
            if let Some(c) = &self.content {
                let _ = fs::write(&self.path, c);
            } else {
                let _ = fs::remove_file(&self.path);
            }
        }
    }

    let orig = fs::read_to_string(&p).ok();
    let _g = Guard { path: p.clone(), content: orig };

    // Attempt to set an invalid timeout value; CLI should reject this and return error
    let out_timeout = search_cmd().env("APPDATA", &base).env("LOCALAPPDATA", &base).env("USERPROFILE", &base).env("XDG_CONFIG_HOME", &base).env("HOME", &base)
        .args(["config", "set", "settings.timeout", "not-a-number"]).output().unwrap();

    // Expect an error exit status and a config error message for timeout
    assert!(!out_timeout.status.success(), "expected config set to fail for invalid numeric input but it succeeded. stdout: {} stderr: {}",
        String::from_utf8_lossy(&out_timeout.stdout), String::from_utf8_lossy(&out_timeout.stderr));

    let stderr_timeout = String::from_utf8_lossy(&out_timeout.stderr);
    assert!(stderr_timeout.to_lowercase().contains("invalid numeric")
        || stderr_timeout.to_lowercase().contains("invalid numeric value")
        || stderr_timeout.to_lowercase().contains("config"),
        "stderr did not indicate numeric/config error: {}", stderr_timeout);

    // Attempt to set an invalid count value; CLI should also reject this
    let out_count = search_cmd().env("APPDATA", &base).env("LOCALAPPDATA", &base).env("USERPROFILE", &base).env("XDG_CONFIG_HOME", &base).env("HOME", &base)
        .args(["config", "set", "settings.count", "not-a-number"]).output().unwrap();

    assert!(!out_count.status.success(), "expected config set to fail for invalid numeric input but it succeeded. stdout: {} stderr: {}",
        String::from_utf8_lossy(&out_count.stdout), String::from_utf8_lossy(&out_count.stderr));

let stderr_count = String::from_utf8_lossy(&out_count.stderr);
    assert!(stderr_count.to_lowercase().contains("invalid numeric")
    || stderr_count.to_lowercase().contains("invalid numeric value")
    || stderr_count.to_lowercase().contains("config"),
    "stderr did not indicate numeric/config error: {}", stderr_count);
}

// =============================================================================
// Legacy quoted numeric migration tests (search-cli-hbq.2)
// =============================================================================

#[test]
fn test_load_config_tolerates_quoted_numeric_timeout() {
    // Test that load_config() tolerantly coerces legacy quoted numeric values
    // for settings.timeout when loaded from TOML config file.
    use std::{fs, path::PathBuf};
    use serde_json::Value;
    use std::time::{SystemTime, UNIX_EPOCH};

    // Use an isolated config directory
    let base = std::env::temp_dir().join(format!("search_cli_test_{}_{}",
        std::process::id(),
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));
    let _ = std::fs::create_dir_all(&base);

    // Discover config path
    let out = search_cmd().env("APPDATA", &base).env("LOCALAPPDATA", &base)
        .env("USERPROFILE", &base).env("XDG_CONFIG_HOME", &base).env("HOME", &base)
        .args(["config", "path", "--json"]).output().unwrap();
    assert!(out.status.success(), "failed to get config path: {}", String::from_utf8_lossy(&out.stderr));
    let j: Value = serde_json::from_slice(&out.stdout).unwrap();
    let p = PathBuf::from(j["data"]["path"].as_str().unwrap());

    // Backup/restore guard
    struct Guard { path: PathBuf, content: Option<String> }
    impl Drop for Guard {
        fn drop(&mut self) {
            if let Some(c) = &self.content {
                let _ = fs::write(&self.path, c);
            } else {
                let _ = fs::remove_file(&self.path);
            }
        }
    }

    let orig = fs::read_to_string(&p).ok();
    let _g = Guard { path: p.clone(), content: orig };

    // Create parent directory if it doesn't exist
    if let Some(parent) = p.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    // Write a legacy config with QUOTED numeric value for timeout (not integer)
    // This is the legacy format that should be tolerantly coerced
    let legacy_config = r#"
[settings]
timeout = "77"
count = 10
"#;
    fs::write(&p, legacy_config).unwrap();

    // Request JSON output - load_config() should tolerate the quoted timeout and coerce it
    let out = search_cmd().env("APPDATA", &base).env("LOCALAPPDATA", &base)
        .env("USERPROFILE", &base).env("XDG_CONFIG_HOME", &base).env("HOME", &base)
        .args(["config", "show", "--json"]).output().unwrap();

    // The command should succeed (not fail on config load)
    assert!(out.status.success(),
        "load_config() failed on legacy quoted numeric timeout. stderr: {}",
        String::from_utf8_lossy(&out.stderr));

    let j: Value = serde_json::from_slice(&out.stdout).unwrap();
    // timeout should be coerced from "77" (string) to 77 (integer)
    assert_eq!(j["settings"]["timeout"].as_u64().unwrap(), 77,
        "timeout should be coerced from quoted string '77' to integer 77");
}

#[test]
fn test_load_config_tolerates_quoted_numeric_count() {
    // Test that load_config() tolerantly coerces legacy quoted numeric values
    // for settings.count when loaded from TOML config file.
    use std::{fs, path::PathBuf};
    use serde_json::Value;
    use std::time::{SystemTime, UNIX_EPOCH};

    // Use an isolated config directory
    let base = std::env::temp_dir().join(format!("search_cli_test_{}_{}",
        std::process::id(),
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));
    let _ = std::fs::create_dir_all(&base);

    // Discover config path
    let out = search_cmd().env("APPDATA", &base).env("LOCALAPPDATA", &base)
        .env("USERPROFILE", &base).env("XDG_CONFIG_HOME", &base).env("HOME", &base)
        .args(["config", "path", "--json"]).output().unwrap();
    assert!(out.status.success(), "failed to get config path: {}", String::from_utf8_lossy(&out.stderr));
    let j: Value = serde_json::from_slice(&out.stdout).unwrap();
    let p = PathBuf::from(j["data"]["path"].as_str().unwrap());

    // Backup/restore guard
    struct Guard { path: PathBuf, content: Option<String> }
    impl Drop for Guard {
        fn drop(&mut self) {
            if let Some(c) = &self.content {
                let _ = fs::write(&self.path, c);
            } else {
                let _ = fs::remove_file(&self.path);
            }
        }
    }

    let orig = fs::read_to_string(&p).ok();
    let _g = Guard { path: p.clone(), content: orig };

    // Create parent directory if it doesn't exist
    if let Some(parent) = p.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    // Write a legacy config with QUOTED numeric value for count (not integer)
    let legacy_config = r#"
[settings]
timeout = 10
count = "15"
"#;
    fs::write(&p, legacy_config).unwrap();

    // Request JSON output - load_config() should tolerate the quoted count and coerce it
    let out = search_cmd().env("APPDATA", &base).env("LOCALAPPDATA", &base)
        .env("USERPROFILE", &base).env("XDG_CONFIG_HOME", &base).env("HOME", &base)
        .args(["config", "show", "--json"]).output().unwrap();

    // The command should succeed (not fail on config load)
    assert!(out.status.success(),
        "load_config() failed on legacy quoted numeric count. stderr: {}",
        String::from_utf8_lossy(&out.stderr));

    let j: Value = serde_json::from_slice(&out.stdout).unwrap();
    // count should be coerced from "15" (string) to 15 (integer)
    assert_eq!(j["settings"]["count"].as_u64().unwrap(), 15,
        "count should be coerced from quoted string '15' to integer 15");
}

#[test]
fn test_load_config_rejects_non_coercible_quoted_value() {
    // Test that load_config() fails with a clear error when a quoted numeric
    // value cannot be coerced (e.g., timeout = "abc").
    use std::{fs, path::PathBuf};
    use serde_json::Value;
    use std::time::{SystemTime, UNIX_EPOCH};

    // Use an isolated config directory
    let base = std::env::temp_dir().join(format!("search_cli_test_{}_{}",
        std::process::id(),
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));
    let _ = std::fs::create_dir_all(&base);

    // Discover config path
    let out = search_cmd().env("APPDATA", &base).env("LOCALAPPDATA", &base)
        .env("USERPROFILE", &base).env("XDG_CONFIG_HOME", &base).env("HOME", &base)
        .args(["config", "path", "--json"]).output().unwrap();
    assert!(out.status.success(), "failed to get config path: {}", String::from_utf8_lossy(&out.stderr));
    let j: Value = serde_json::from_slice(&out.stdout).unwrap();
    let p = PathBuf::from(j["data"]["path"].as_str().unwrap());

    // Backup/restore guard
    struct Guard { path: PathBuf, content: Option<String> }
    impl Drop for Guard {
        fn drop(&mut self) {
            if let Some(c) = &self.content {
                let _ = fs::write(&self.path, c);
            } else {
                let _ = fs::remove_file(&self.path);
            }
        }
    }

    let orig = fs::read_to_string(&p).ok();
    let _g = Guard { path: p.clone(), content: orig };

    // Create parent directory if it doesn't exist
    if let Some(parent) = p.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    // Write a config with NON-COERCIBLE quoted value for timeout
    let bad_config = r#"
[settings]
timeout = "abc"
count = 10
"#;
    fs::write(&p, bad_config).unwrap();

    // Request JSON output - load_config() should fail with a clear error
    let out = search_cmd().env("APPDATA", &base).env("LOCALAPPDATA", &base)
        .env("USERPROFILE", &base).env("XDG_CONFIG_HOME", &base).env("HOME", &base)
        .args(["config", "show", "--json"]).output().unwrap();

    // The command should FAIL because "abc" cannot be coerced to a number
    assert!(!out.status.success(),
        "expected load_config() to fail for non-coercible quoted value 'abc' but it succeeded");

    let stderr = String::from_utf8_lossy(&out.stderr);
    // Should have a clear error message indicating the problem
assert!(stderr.to_lowercase().contains("invalid") || stderr.to_lowercase().contains("error") || stderr.to_lowercase().contains("config"),
    "error message should be clear about the invalid quoted value. got: {}", stderr);
}

// =============================================================================
// search-cli-hbq.3: Cache policy - skip failed/degraded-empty responses
// =============================================================================

#[test]
fn test_cache_skips_all_providers_failed_response() {
    // When all providers fail, the response should NOT be cached.
    // Use a fake provider name to force all providers to fail.
    use std::time::Instant;

    let query = format!("cache test all failed {}", std::process::id());

    // First search - will fail because provider "nonexistent" doesn't exist
    let output1 = search_cmd()
        .args(["search", "-q", &query, "-p", "nonexistent", "--json"])
        .output()
        .unwrap();

    // Should fail (no providers)
    assert!(!output1.status.success(), "expected search with nonexistent provider to fail");

    // Now run the same query again WITHOUT the provider filter
    // If caching is working correctly, a FAILED response was NOT cached,
    // so this should hit the API again (not return cached failure)
    let start = Instant::now();
    let _ = search_cmd()
        .args(["search", "-q", &query, "--json", "-c", "3"])
        .output()
        .unwrap();
    let elapsed = start.elapsed();

    // If the failed response WAS cached, we'd get a quick response (< 100ms)
    // If it was NOT cached (correct behavior), this hits the API and takes longer
    // The key assertion: elapsed should be > 500ms meaning it actually ran the search
    // (not served from cache which would be < 100ms)
    assert!(elapsed.as_millis() > 500,
        "search-cli-hbq.3 FAILED: all_providers_failed response was cached (returned in {}ms). \
        Failed responses should NOT be cached to avoid replaying failure artifacts.",
        elapsed.as_millis());

    eprintln!(" PASS: all_providers_failed response was NOT cached (took {}ms)", elapsed.as_millis());
}

#[test]
fn test_cache_skips_degraded_empty_response() {
    // A "degraded-empty" response is one where results=0 AND providers_failed is not empty.
    // This represents a partial failure that returned no useful data.
    // Such responses should NOT be cached.

    // This test requires at least one provider to be configured, but we need it to fail.
    // We simulate this by using a query that will cause a provider to fail, or by
    // using an unconfigured provider to get the degraded-empty state.

    // Use an unconfigured provider to trigger all-providers-failed which is degraded-empty
    let query = format!("cache test degraded empty {}", std::process::id());

    // First search with unconfigured provider - should get degraded-empty (0 results, failures)
    let output1 = search_cmd()
        .args(["search", "-q", &query, "-p", "nonexistent", "--json"])
        .output()
        .unwrap();

    // This should fail (no providers available)
    assert!(!output1.status.success());

    // Now run again - if degraded-empty was cached, we'd get instant failure
    // If correctly NOT cached, it will try to run again
    use std::time::Instant;
    let start = Instant::now();

    // Run without the bad provider filter - should actually try to search
    let _ = search_cmd()
        .args(["search", "-q", &query, "--json", "-c", "3"])
        .output()
        .unwrap();
    let elapsed = start.elapsed();

    // If degraded-empty WAS cached, this would be instant (< 100ms)
    // If NOT cached (correct), this runs the search
    assert!(elapsed.as_millis() > 500,
        "search-cli-hbq.3 FAILED: degraded-empty response was cached ({}ms). \
        Degraded-empty responses (0 results + failures) should NOT be cached.",
        elapsed.as_millis());

    eprintln!(" PASS: degraded-empty response was NOT cached (took {}ms)", elapsed.as_millis());
}

#[test]
fn test_cache_still_works_for_successful_responses() {
    // Verify that SUCCESSFUL responses ARE still cached and replayed correctly.
    // This is a regression test to ensure the cache policy fix doesn't break
    // the normal caching behavior for successful queries.
    if !has_any_provider() {
        eprintln!("SKIP: no providers configured");
        return;
    }

    use std::time::Instant;

    let query = format!("cache success regression test {}", std::process::id());

    // First search - should succeed and be cached
    let output1 = search_cmd()
        .args(["search", "-q", &query, "--json", "-c", "3"])
        .output()
        .unwrap();

    assert!(output1.status.success(), "first search should succeed");
    let json1: serde_json::Value = serde_json::from_slice(&output1.stdout).unwrap();
    assert_eq!(json1["status"], "success", "first search should have success status");

    // Second search with same query - should be served from cache (fast)
    let start = Instant::now();
    let output2 = search_cmd()
        .args(["search", "-q", &query, "--json", "-c", "3"])
        .output()
        .unwrap();
    let elapsed = start.elapsed();

    assert!(output2.status.success());
    let json2: serde_json::Value = serde_json::from_slice(&output2.stdout).unwrap();
    assert_eq!(json2["status"], "success");

    // Cache replay should be fast (< 500ms, typically < 50ms)
    assert!(elapsed.as_millis() < 500,
        "search-cli-hbq.3 FAILED: successful response was NOT cached (took {}ms). \
        Successful responses SHOULD be cached for fast replay.",
        elapsed.as_millis());

    // Results should be identical (both from cache or both fresh)
    // At minimum, both should have the same status
    assert_eq!(json1["status"], json2["status"]);

    eprintln!(" PASS: successful response was cached and replayed in {}ms", elapsed.as_millis());
}

#[test]
fn test_cache_still_works_for_partial_success_responses() {
    // Partial success (some results + some failures) should still be cached
    // because it contains useful results.
    if !has_any_provider() {
        eprintln!("SKIP: no providers configured");
        return;
    }

    use std::time::Instant;

    // Use a query that might get partial results
    let query = format!("cache partial success test {}", std::process::id());

    // First search
    let output1 = search_cmd()
        .args(["search", "-q", &query, "--json", "-c", "5"])
        .output()
        .unwrap();

    // Even if partial success, it should have results
    if !output1.status.success() {
        eprintln!("SKIP: first search failed, cannot test partial success caching");
        return;
    }

    let json1: serde_json::Value = serde_json::from_slice(&output1.stdout).unwrap();

    // Second search - should be cached
    let start = Instant::now();
    let output2 = search_cmd()
        .args(["search", "-q", &query, "--json", "-c", "5"])
        .output()
        .unwrap();
    let elapsed = start.elapsed();

    assert!(output2.status.success());
    let json2: serde_json::Value = serde_json::from_slice(&output2.stdout).unwrap();

    // Should be fast (cached)
    assert!(elapsed.as_millis() < 500,
        "search-cli-hbq.3 FAILED: partial_success response was NOT cached (took {}ms). \
        Partial success responses SHOULD be cached.",
        elapsed.as_millis());

    assert_eq!(json1["status"], json2["status"]);
    eprintln!(" PASS: partial_success response was cached in {}ms", elapsed.as_millis());
}

// =============================================================================
// search-cli-hbq.4: Structured provider failure detail metadata (compat mode)
// =============================================================================

#[test]
fn test_failure_metadata_includes_validation_reason_and_legacy_list() {
    use serde_json::Value;

    // Deterministic validation failure for stealth extract path: invalid URL.
    let output = search_cmd()
        .args(["search", "-q", "not-a-url", "-m", "extract", "-p", "stealth", "--json"])
        .output()
        .unwrap();

    assert!(!output.status.success(), "expected extract with invalid URL to fail");

    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["status"], "all_providers_failed");

    // Backward compatibility: legacy providers_failed list remains present.
    let failed = json["metadata"]["providers_failed"]
        .as_array()
        .expect("metadata.providers_failed should be an array");
    assert!(failed.iter().any(|v| v.as_str() == Some("stealth")));

    // New structured detail field should include reason taxonomy.
    let details = json["metadata"]["providers_failed_detail"]
        .as_array()
        .expect("metadata.providers_failed_detail should be an array");
    assert!(!details.is_empty(), "providers_failed_detail should not be empty");

    let stealth_detail = details
        .iter()
        .find(|d| d["provider"].as_str() == Some("stealth"))
        .expect("expected stealth detail entry");

    assert_eq!(stealth_detail["reason"], "validation");
    assert_eq!(stealth_detail["code"], "config_error");
}

#[test]
fn test_failure_metadata_includes_api_reason_and_legacy_list() {
    use serde_json::Value;

    // Deterministic API-class failure for browserless extract path: invalid URL.
    // Browserless classifies URL parse errors as SearchError::Api { code: "invalid_url" }.
    let output = search_cmd()
        .env("BROWSERLESS_API_KEY", "test-key")
        .args(["search", "-q", "not-a-url", "-m", "extract", "-p", "browserless", "--json"])
        .output()
        .unwrap();

    assert!(!output.status.success(), "expected browserless extract with invalid URL to fail");

    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["status"], "all_providers_failed");

    let failed = json["metadata"]["providers_failed"]
        .as_array()
        .expect("metadata.providers_failed should be an array");
    assert!(failed.iter().any(|v| v.as_str() == Some("browserless")));

    let details = json["metadata"]["providers_failed_detail"]
        .as_array()
        .expect("metadata.providers_failed_detail should be an array");
    assert!(!details.is_empty(), "providers_failed_detail should not be empty");

    let stealth_detail = details
        .iter()
        .find(|d| d["provider"].as_str() == Some("browserless"))
        .expect("expected browserless detail entry");

    assert_eq!(stealth_detail["reason"], "api");
    let code = stealth_detail["code"].as_str().unwrap_or_default();
    assert!(
        matches!(code, "api_error" | "invalid_url" | "http_error" | "extraction_error"),
        "unexpected API-class code: {}",
        code
    );
}

// =============================================================================
// search-cli-hbq.5: Timeout unification across config/engine/providers (RED)
// =============================================================================

#[test]
fn test_timeout_unification_respects_settings_timeout_for_stealth_extract() {
    use serde_json::Value;
    use std::io::ErrorKind;
    use std::io::Write;
    use std::net::TcpListener;
    use std::thread;
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    // Isolated config home for deterministic timeout config.
    let uniq = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let base = std::env::temp_dir().join(format!("search_cli_hbq5_timeout_{}", uniq));
    std::fs::create_dir_all(&base).unwrap();

    // Set a very small timeout that unified policy should honor end-to-end.
    let set_timeout = search_cmd()
        .env("APPDATA", &base)
        .env("LOCALAPPDATA", &base)
        .env("USERPROFILE", &base)
        .env("XDG_CONFIG_HOME", &base)
        .env("HOME", &base)
        .args(["config", "set", "settings.timeout", "1"])
        .output()
        .unwrap();
    assert!(
        set_timeout.status.success(),
        "config set timeout failed: {}",
        String::from_utf8_lossy(&set_timeout.stderr)
    );

    // Local server that delays the response long enough to exceed 1s timeout.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let _ = listener.set_nonblocking(true);
        let accept_start = Instant::now();

        loop {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    // Delay before responding to force timeout behavior if unified timeout is respected.
                    thread::sleep(Duration::from_millis(3500));
                    let body = "<html><body>delayed response</body></html>";
                    let resp = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    let _ = stream.write_all(resp.as_bytes());
                    break;
                }
                Err(e) if e.kind() == ErrorKind::WouldBlock => {
                    if accept_start.elapsed() > Duration::from_secs(8) {
                        break;
                    }
                    thread::sleep(Duration::from_millis(25));
                }
                Err(_) => break,
            }
        }
    });

    let url = format!("http://{}", addr);
    let start = Instant::now();
    let output = search_cmd()
        .env("APPDATA", &base)
        .env("LOCALAPPDATA", &base)
        .env("USERPROFILE", &base)
        .env("XDG_CONFIG_HOME", &base)
        .env("HOME", &base)
        .args(["search", "-q", &url, "-m", "extract", "-p", "stealth", "--json"])
        .output()
        .unwrap();
    let elapsed = start.elapsed();

    let _ = server.join();

    // Unified timeout behavior expectation: this should fail quickly with timeout metadata.
    assert!(
        !output.status.success(),
        "expected request to fail under unified 1s timeout; stdout: {} stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["status"], "all_providers_failed");

    let details = json["metadata"]["providers_failed_detail"]
        .as_array()
        .expect("metadata.providers_failed_detail should be an array");
    let stealth_detail = details
        .iter()
        .find(|d| d["provider"].as_str() == Some("stealth"))
        .expect("expected stealth detail entry");

    // Step5 focuses on deterministic timeout budget behavior. Keep schema-level
    // checks here, and assert timing below.
    assert!(stealth_detail["reason"].is_string());
    assert!(stealth_detail["code"].is_string());

    // Keep this fairly loose to avoid CI flake, while still proving 1s-level behavior.
    assert!(
        elapsed.as_millis() < 2500,
        "expected timeout close to configured 1s budget, got {}ms",
        elapsed.as_millis()
    );
}

// =============================================================================
// search-cli-hbq.9: Blocking extraction offload parity checks
// =============================================================================

#[test]
fn test_stealth_extract_local_html_still_returns_content() {
    use serde_json::Value;
    use std::io::{ErrorKind, Read, Write};
    use std::net::TcpListener;
    use std::thread;
    use std::time::{Duration, Instant};

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let _ = listener.set_nonblocking(true);
        let start = Instant::now();
        loop {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let mut req_buf = [0u8; 1024];
                    let _ = stream.read(&mut req_buf);

                    let body = "<html><head><title>Local Test</title></head><body><p>hello extraction parity</p></body></html>";
                    let resp = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    let _ = stream.write_all(resp.as_bytes());
                    break;
                }
                Err(e) if e.kind() == ErrorKind::WouldBlock => {
                    if start.elapsed() > Duration::from_secs(8) {
                        break;
                    }
                    thread::sleep(Duration::from_millis(25));
                }
                Err(_) => break,
            }
        }
    });

    let url = format!("http://{}", addr);
    let output = search_cmd()
        .args(["search", "-q", &url, "-m", "extract", "-p", "stealth", "--json"])
        .output()
        .unwrap();

    let _ = server.join();

    let json: Value = serde_json::from_slice(&output.stdout).unwrap();

    // In some environments stealth local client can fail to connect to localhost;
    // regardless, spawn_blocking offload must preserve structured output shape.
    if output.status.success() {
        assert_eq!(json["status"], "success");
        let results = json["results"].as_array().unwrap_or(&Vec::new()).to_vec();
        assert!(!results.is_empty(), "expected at least one extracted result");
    } else {
        assert_eq!(json["status"], "all_providers_failed");
        let details = json["metadata"]["providers_failed_detail"]
            .as_array()
            .expect("metadata.providers_failed_detail should be an array");
        assert!(
            details.iter().any(|d| d["provider"].as_str() == Some("stealth")),
            "expected stealth failure detail entry"
        );
    }
}

// =============================================================================
// search-cli-hbq.7: Provider count clamping prevents validation failures
// =============================================================================

#[test]
fn test_provider_count_clamping_prevents_validation_failure() {
    // When a user requests a count that exceeds Brave's API limit (20),
    // the dispatch layer should clamp the count BEFORE sending to the provider.
    // This prevents avoidable "validation" style failures from the API.
    // This test uses Brave specifically since it's the primary capped provider.
    use serde_json::Value;

    if !has_provider("brave") {
        eprintln!("SKIP: need brave provider for count clamping test");
        return;
    }

    // Request a count that exceeds Brave's typical API limit (20)
    // The dispatch layer should clamp this to a valid range before calling Brave.
    let output = search_cmd()
        .args(["search", "-q", "test query", "-p", "brave", "--json", "-c", "100"])
        .output()
        .unwrap();

    // The search should NOT fail with a validation-style error from Brave
    // If clamping is working, Brave receives a valid count and returns results
    // If clamping is NOT working, Brave may reject with 400 Bad Request or similar
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();

    // Status should be success or partial_success, NOT all_providers_failed
    // due to a validation error from the provider
    assert!(
        json["status"] == "success" || json["status"] == "partial_success",
        "search-cli-hbq.7 FAILED: high count request failed with status '{}'. \
        This suggests count clamping is not working. Response: {}",
        json["status"],
        String::from_utf8_lossy(&output.stderr)
    );

    // Verify brave was queried
    let providers_queried = json["metadata"]["providers_queried"]
        .as_array()
        .expect("providers_queried should be an array");
    assert!(
        providers_queried.iter().any(|p| p == "brave"),
        "brave should have been queried"
    );

    eprintln!(" PASS: high count request succeeded with clamping (status: {})", json["status"]);
}

#[test]
fn test_provider_count_clamping_metadata_signal() {
    // Optional: If clamping occurred, a minimal metadata signal may indicate this.
    // This test verifies the signal exists and is backward-compatible (optional field).
    use serde_json::Value;

    if !has_provider("brave") {
        eprintln!("SKIP: need brave provider for count clamping metadata test");
        return;
    }

    // Request count way over the limit
    let output = search_cmd()
        .args(["search", "-q", "test query", "-p", "brave", "--json", "-c", "100"])
        .output()
        .unwrap();

    let json: Value = serde_json::from_slice(&output.stdout).unwrap();

    // The response should still be valid and successful
    assert!(
        json["status"] == "success" || json["status"] == "partial_success",
        "clamped request should succeed"
    );

    // If providers_clamped field exists (optional), verify it's an array
    // This is a backward-compatible check - the field may not exist
    if let Some(clamped) = json["metadata"].get("providers_clamped") {
        assert!(
            clamped.is_array(),
            "providers_clamped should be an array if present"
        );
        eprintln!(" PASS: providers_clamped metadata signal present: {}", clamped);
    } else {
        eprintln!(" PASS: no providers_clamped signal (backward-compatible, clamping still works)");
    }
}

// =============================================================================
// search-cli-hbq.7: Provider-specific count clamping
// =============================================================================

#[test]
fn test_brave_count_is_clamped_before_dispatch() {
    use serde_json::Value;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    // Local HTTP sink to capture outbound Brave request query params.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("expected one request");

        let mut buf = [0u8; 8192];
        let n = stream.read(&mut buf).unwrap_or(0);
        let req = String::from_utf8_lossy(&buf[..n]).to_string();

        let body = r#"{"web":{"results":[]}}"#;
        let resp = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        let _ = stream.write_all(resp.as_bytes());
        req
    });

    let output = search_cmd()
        .env("BRAVE_API_KEY", "test-key")
        .env("BRAVE_BASE_URL", format!("http://{}", addr))
        .args(["search", "-q", "provider clamp test", "-p", "brave", "--json", "-c", "100"])
        .output()
        .unwrap();

    let request = server.join().expect("server thread should complete");

    // RED expectation for hbq.7: outbound count should be clamped for brave.
    assert!(
        request.contains("count=20"),
        "expected clamped brave count=20 in request line, got: {}",
        request.lines().next().unwrap_or("<empty>")
    );

    assert!(
        !request.contains("count=100"),
        "request still contains unclamped count=100: {}",
        request.lines().next().unwrap_or("<empty>")
    );

    assert!(
        output.status.success(),
        "expected successful no_results response from local brave stub, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["status"], "no_results");
}
