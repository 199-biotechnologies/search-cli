# Reliability Hardening PR Readiness (search-cli-hbq)

Prepared for upstream submission to `paperfoot/search-cli` (base branch: `master`).

## 1) Upstream Conventions Check

- [x] Followed `CONTRIBUTING.md` flow (fork + branch from `master`)
- [x] PR body will include clear summary + why + test evidence
- [x] Final pre-PR matrix executed and attached (Step 11)
- [ ] Upstream PR opened and linked (Step 12)

## 2) Scope Summary (What Changed)

Reliability hardening across configuration typing, cache policy, timeout behavior, provider request normalization, structured failure diagnostics, extraction runtime safety, and user-facing rejection guidance.

### Files currently changed in working tree

- `README.md`
- `src/cache.rs`
- `src/config.rs`
- `src/engine.rs`
- `src/errors.rs`
- `src/logging.rs`
- `src/main.rs`
- `src/output/table.rs`
- `src/providers/brave.rs`
- `src/providers/browserless.rs`
- `src/providers/exa.rs`
- `src/providers/jina.rs`
- `src/providers/stealth.rs`
- `src/types.rs`
- `tests/integration.rs`

## 3) Motivation → Change Mapping (Beads Steps)

Closed implementation steps included in this PR scope:

- `search-cli-hbq.1` typed numeric config writes
- `search-cli-hbq.2` legacy quoted numeric migration
- `search-cli-hbq.3` cache skip policy for failed/degraded-empty outcomes
- `search-cli-hbq.4` structured provider failure taxonomy (`providers_failed_detail`) with backward compatibility
- `search-cli-hbq.5` timeout budget unification
- `search-cli-hbq.6` removal of special-mode timeout literals
- `search-cli-hbq.7` provider request count clamping
- `search-cli-hbq.8` tracing subscriber + structured reliability events
- `search-cli-hbq.9` `spawn_blocking` extraction offload
- `search-cli-hbq.13` actionable provider rejection classification
- `search-cli-hbq.14` informative rejection output in JSON/table modes
- `search-cli-hbq.15` provider-specific diagnostics + troubleshooting docs

Detailed close reasons and file-level blast-radius notes are captured in Step 10 Beads comments.

## 4) Behavioral Deltas (Reviewer-Facing)

1. **Config reliability:** `settings.timeout` / `settings.count` persist and load as numeric values (with narrow compatibility coercion for legacy quoted numerics).
2. **Cache correctness:** all-provider-failed and degraded-empty responses are not persisted to cache, preventing sticky replay of failure artifacts.
3. **Timeout semantics:** special-path and deep-path timeouts use shared policy-derived budgets rather than scattered literals.
4. **Provider normalization:** capped providers (e.g., Brave) receive clamped outbound count to avoid avoidable validation failures.
5. **Observability:** structured reliability events are emitted when tracing is enabled.
6. **Runtime robustness:** extraction parsing moved to blocking pool where appropriate to avoid async runtime blocking.
7. **Rejection UX:** machine-readable and table output now include actionable cause/action/signature diagnostics.

## 5) Compatibility / Risk Notes

- `providers_failed` remains preserved for compatibility.
- `providers_failed_detail` adds optional actionable fields (`cause`, `action`, `signature`).
- Rejection guidance avoids secrets; diagnostics are signature/cause/action oriented.
- Confirmed non-blocking warning baseline: unused `Provider::timeout` method in `src/providers/mod.rs`.

## 6) Verification Checklist (Step 11 Gate)

### Required matrix

- [x] `cargo check`
- [x] `cargo test`
- [x] `cargo test --test integration`
- [x] `cargo clippy --all-targets --all-features`

### Rejection UX contract checks

- [x] JSON actionable fields present for Exa count-limit classification
- [x] JSON actionable fields present for Jina Cloudflare-1010-style classification
- [x] JSON actionable fields present for Browserless auth-mode mismatch classification
- [x] Table output prints remediation guidance (`Try:`) and diagnostics signature (`diag:`)
- [x] Output review confirms no credential/token leakage

### Evidence capture

Recorded command + exit status + timestamp in Beads step comments (`search-cli-hbq.11`) at 2026-04-20T17:53:08Z.

### Step 11 result snapshot

- `cargo check`: PASS (0 errors)
- `cargo test`: PASS (48 passed)
- `cargo test --test integration`: PASS (36 passed)
- `cargo clippy --all-targets --all-features`: PASS (0 errors)
- Warning baseline: `Provider::timeout` dead_code in `src/providers/mod.rs`
- Execution note: during matrix run, fixed `backon` v1 incompatibility by replacing unsupported builder hook with retry-future `.notify(...)` in `src/providers/mod.rs`, then reran matrix to green.

## 7) PR Body Skeleton (for Step 12)

```md
## Summary
- Reliability hardening across config typing/migration, cache policy, timeout unification, provider count clamping, extraction runtime, and actionable rejection diagnostics.
- Preserves compatibility (`providers_failed`) while adding structured failure guidance (`providers_failed_detail` fields: cause/action/signature).
- Adds integration coverage and troubleshooting docs for common provider rejection classes.

## Why
- Prevent sticky failure replay, ambiguous timeout behavior, and opaque provider rejection output.

## Validation
- cargo check
- cargo test
- cargo test --test integration
- cargo clippy
- Additional rejection-UX contract checks (JSON + table guidance)

## Behavior impact
- More deterministic timeout/cache behavior and clearer remediation output for provider failures.

## Compatibility
- Existing `providers_failed` retained.
```

## 8) Beads Linkage

- Step 10 issue: `search-cli-hbq.10`
- Next gate: `search-cli-hbq.11`
- PR lifecycle gate: `search-cli-hbq.12`
