# Inert

**The web, behind glass.** Inert is an application protocol where sites send
*descriptions*, never code — and a small Rust browser that draws them.

- `inert://` sites ship a signed **App Manifest** (screens, actions, design).
  The browser pins the site's key, verifies the signature, then checks every
  screen against the signed template before drawing it. A hacked server can
  show wrong data, but it cannot add a password field.
- Everything else on the web opens in the same browser as **unsecured** tabs
  (system WebKit, sandboxed), with a native ad & tracker blocker.

![Inert Bank in the Inert browser](docs/screenshots/04-bank-overview.png)

## What's in the repo

| Path | What |
|---|---|
| `crates/inert-proto` | Message and manifest types, strict deterministic CBOR codec, limits. No I/O, no `unsafe`. |
| `crates/inert-verify` | Every verification rule: signatures, manifest rules, screen/action/open conformance, contrast. |
| `crates/inert-transport` | Length-prefixed frames over TCP + TLS 1.3 (rustls, X25519MLKEM768 preferred), per-direction limits, timeouts, dev trust files. |
| `crates/inert-client` | Client session shared by the browser and tests: dial, pin, verify, fetch assets. |
| `crates/inert-server` | Server SDK, `inert-sign` (keygen / build / check) and the dev runner. |
| `crates/inert-browser` | The browser (egui + wgpu, WebKit web views, Mullvad VPN, Shield, assistant). |
| `examples/{bank,shop,notes}` | Three demo sites with fake data. |
| `spec/vectors` | Language-independent conformance vectors. |
| `fuzz` | cargo-fuzz targets for the decoder and verifier. |
| `docs/superpowers/specs` | The design spec. |

## Run it

```bash
scripts/demo-sites.sh
```

```bash
cargo run --release -p inert-browser
```

The first launch asks for one thing: your AI provider (Claude or GPT) and,
optionally, your API key (stored in the macOS Keychain).

`cargo run -p inert-browser -- --demo` starts with a throwaway demo profile.
`scripts/walkthrough.sh <dir>` runs the scripted feature tour and screenshots it.

## Tests

```bash
cargo test --workspace
```

Includes end-to-end tests over real TLS for all ten attack scenarios in the
spec (injected password field, extra data, tampered manifest, swapped key,
rollback, invisible text, bad asset, replayed action, unoffered choice,
oversized frame).

```bash
cd fuzz && cargo +nightly fuzz run cbor_decode
```

## Browser features

Tab groups with three special groups — **Inbox** (mail & chat, never
unloaded), **Homeserver** (self-hosted dashboards) and **Developer** (dev
servers, docs, developer tools) — plus bookmarks, history, omnibox with inline
completion and search suggestions, reopen closed tab, find in page, zoom,
downloads, memory saver, native menu with Chrome-style shortcuts, a site
security panel and a manifest inspector, **Shield** (EasyList + EasyPrivacy
compiled into WebKit's content blocker), a **Mullvad VPN** panel, and an
**assistant** that reads inert:// pages from their signed structure and web
pages as untrusted text.

Typefaces: Geist Serif, Sans, Mono and Pixel by Vercel (SIL OFL).
