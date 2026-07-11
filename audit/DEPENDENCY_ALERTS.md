# Dependency Vulnerability Triage

> Status of GitHub dependabot alerts as of 2026-07-11. Re-review this file
> whenever libp2p, Tauri, or ratatui ship new releases.

| # | Crate | Severity | Resolution |
|---|-------|----------|------------|
| 2 | `lru` 0.12.5 | Low | **Fixed** — dashboard upgraded ratatui 0.29 → 0.30; lock now resolves lru 0.18.1 (≥ 0.16.3 patch) |
| 3 | `yamux` 0.12.1 | High | **Dismissed (not used)** — see below |
| 9 | `hickory-proto` O(n²) encode | Medium | **Dismissed (not used)** — see below |
| 14 | `hickory-proto` NSEC3 loop | High | **Dismissed (not used)** — see below |
| 10 | `glib` 0.18.5 | Medium | **Dismissed (not used)** — see below |

## Dismissal rationale

### yamux 0.12.1 — remote panic via malformed frame (alert #3)

`libp2p-yamux` 0.47 (latest) links both yamux 0.12 and 0.13. RupeeCoin
constructs every muxer with `yamux::Config::default()`
(`core/network/src/lib.rs`), which uses the patched 0.13.10 code path. The
vulnerable 0.12 path activates only through the deprecated legacy
`WindowUpdateMode` configuration, which this codebase never touches. No
patched 0.12.x release exists. Clears when upstream libp2p drops the legacy
dependency.

### hickory-proto 0.25.2 — NSEC3 validation loop (alert #14)

Pulled in solely by `libp2p-mdns` for LAN multicast DNS discovery. The
vulnerable function is DNSSEC NSEC3 closest-encloser proof validation; mDNS
performs no DNSSEC validation, so the code is unreachable from RupeeCoin. No
patched release exists upstream at time of triage.

### hickory-proto 0.25.2 — O(n²) name compression (alert #9)

Reachable only through `libp2p-mdns` encoding of small, fixed-shape multicast
queries on the local network segment; the quadratic blowup requires encoding
attacker-controlled messages with many names, which the mDNS behaviour never
does. Patched in hickory-proto 0.26.1, but `libp2p-mdns` 0.48 (latest) pins
`^0.25`. Bump when upstream updates.

### glib 0.18.5 — VariantStrIter unsoundness (alert #10)

`desktop-wallet` only. glib 0.18 is pinned by the gtk3-rs bindings that Tauri
2's Linux windowing stack (`tao`/`muda`) requires; gtk3-rs 0.18 is the final
gtk3 release, so no glib 0.20-compatible upgrade path exists. The unsound
`VariantStrIter` API is not exercised by the wallet or the Tauri code paths it
links. Clears when Tauri moves its Linux backend off GTK3.
