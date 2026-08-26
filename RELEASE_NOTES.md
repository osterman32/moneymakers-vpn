# MoneyMakers VPN — Release Notes

## v0.2.2 — 2026-08-22

- Server-side heartbeat now carries a device-fingerprint beacon on every
  60s ping: OS name + version, CPU arch + count, RAM, screen resolution,
  system timezone, locale, LAN IP + adapter name, battery presence, app +
  system uptime, and a persisted install-UUID (first-run generated, stored
  at `<app_config_dir>/install-id`).
- Enables cross-account correlation + device dossier drilldown in the
  biolink admin panel. Server side is backward-compatible — pre-v0.2.2
  clients continue to work with the same heartbeat shape.
- No new capabilities grants; auto-updater pipeline unchanged. New Rust
  crates: sysinfo, uuid, iana-time-zone, sys-locale, local-ip-address,
  if-addrs, battery (all pure-Rust, no C deps).

## v0.2.1

- macOS TUN inbound + Windows close-handler fix.

## v0.2.0

- macOS full-tunnel routing via sing-box TUN.

## v0.1.1

- Auto-updater plugin + DoH tunnel DNS.
