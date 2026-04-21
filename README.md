# MoneyMakers VPN

Cross-platform desktop client (Windows + macOS) for the biolink-managed
Outline/Shadowsocks network. Pairs with `moneymakers.inc` — the app uses a
share token from biolink's admin to fetch the user's server list, then
(milestone 2) tunnels system traffic through the selected server.

## Milestones

- **M1 (done, this scaffold):** Tauri 2 app, web UI, fetches server list from
  `https://moneymakers.inc/api/vpn/app/<token>/servers`, stores token in
  `localStorage`. Connect/Disconnect commands exist but return a "not
  implemented" error.
- **M2:** Bundle `sing-box`, wire up actual Connect → TUN adapter → SS
  tunnel, with UAC (Windows) / authorization (macOS) elevation.
- **M3:** Code-signing decisions, auto-update, polish.

## One-time setup before first GitHub Actions build

You'll push this repo to GitHub and release tags trigger the cross-platform
build. Before that:

1. **Put a 1024×1024 PNG at `src-tauri/icons/source.png`.**  Any square
   PNG — it becomes the app icon on every platform. The CI workflow will
   generate all the required sizes/formats from it.
2. Push this directory to a new GitHub repo:
   ```bash
   cd /root/moneymakers-vpn-app
   git init -b main
   git add .
   git commit -m "initial scaffold"
   # create repo on github.com first, then:
   git remote add origin git@github.com:<you>/moneymakers-vpn.git
   git push -u origin main
   ```

## Cutting a release

Each time you want new Windows + Mac binaries:

```bash
# bump version in package.json + src-tauri/tauri.conf.json + Cargo.toml
git commit -am "v0.0.2"
git tag v0.0.2
git push --tags
```

GitHub Actions will build for both platforms and create a **draft release**
with the artifacts attached. Edit the release notes and publish to hand the
download URL to friends.

Artifacts:
- `MoneyMakers VPN_<version>_x64-setup.nsis.exe` (Windows installer)
- `MoneyMakers VPN_<version>_x64_en-US.msi` (alternate Windows installer)
- `MoneyMakers VPN_<version>_universal.dmg` (macOS, Intel + Apple Silicon)

## Developing locally (Windows only — no Mac on hand)

Install prerequisites once:

- [Rust](https://rustup.rs/)
- Node 20+
- [Tauri prerequisites for Windows](https://tauri.app/start/prerequisites/)

Then:

```bash
npm install
npm run dev      # launches the app with hot-reload
npm run build    # produces a Windows installer in src-tauri/target/release/bundle/
```

Mac builds happen only on GitHub Actions (macOS runner) since you don't have
a Mac — CI will sort that out.

## Architecture

### Token flow

1. Zain creates a VPN user in biolink admin → gets a share link
   (`moneymakers.inc/v/<token>`).
2. Friend installs this app, pastes the link on first launch.
3. App calls `GET https://moneymakers.inc/api/vpn/app/<token>/servers` and
   gets back `{ user: { name }, servers: [{ id, name, ssUrl }, ...] }`.
4. Token is stored in `localStorage`. On next launch, app auto-fetches.

### Connect flow (milestone 2)

Planned:
- Bundle `sing-box` binary (~30 MB) alongside the Tauri exe.
- "Connect" elevates via UAC / macOS authorization and spawns `sing-box`
  with a generated config pointing at the selected server's `ss://` URL
  and a TUN inbound (`wintun` on Windows, `utun` on macOS).
- Sing-box takes over system routing; all traffic tunnels through the VPS.
- Disconnect kills the sing-box process — TUN goes away, routes revert.

### Distribution

- Build artifacts are posted to GitHub Releases by the CI workflow.
- Link friends directly to the release asset for their platform.
- When you add new servers in biolink admin, the app picks them up on next
  fetch — no rebuild needed.

## Unsigned-app caveats

- **Windows:** SmartScreen will warn on first run ("Windows protected your
  PC"). Friend clicks **More info → Run anyway**. Paying ~$100/yr for a
  code-signing cert removes the warning later.
- **macOS:** Right-click the app → **Open** on first launch (the Gatekeeper
  "can't be opened" dialog on double-click has an "Open Anyway" button in
  System Settings → Privacy & Security). To avoid this, enrol in the Apple
  Developer Program ($99/yr) and notarize builds — do this later if friends
  complain.

## Troubleshooting

- **"invalid token"** in the app → your share token was revoked, or biolink
  is down. Check `moneymakers.inc/admin` → VPN → your user.
- **No servers shown** → no active servers registered in biolink admin.
- **Build fails with "missing icons"** → drop `src-tauri/icons/source.png`
  (1024×1024 PNG) and re-run.
