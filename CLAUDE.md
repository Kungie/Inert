# Inert

A code-free, verifiable application protocol. Address scheme: `inert://`
Tagline: "The web, behind glass." (You can see it; it can't touch you.)

Status: vertical slice implemented (2026-11-07): protocol crates, Rust browser, three demo sites. Details in README.md and spec §18.

## Working preferences
- Short answers. Conversation language: Turkish. Everything in the repo and the code is English.
- Commit often. Ask before outward-facing steps such as pushing or publishing.
- A new project, so the heavy path: design approval → written spec → plan → code.

## Problem
- Most holes found with AI aren't in cryptography. They're in code (C/C++ memory bugs), the browser/JS surface (XSS, supply chain) and app logic.
- No new crypto. Existing TLS 1.3 + post-quantum hybrid (X25519MLKEM768).
- The goal isn't to make holes "impossible" but to **remove whole classes of holes by design** and shrink the attack surface dramatically.

## Threat model: all three
- **Site owner:** code, business logic and APIs can't be seen or copied from the client.
- **User:** a malicious or hacked site can't harm the device.
- **Server:** exposes typed, schema'd actions, not arbitrary endpoints.
- Accepted limit: business logic bugs on the server shrink but don't disappear.

## Decisions
1. **A native protocol, not a browser technology.** No executable code goes to the client (no HTML/CSS/JS).
2. **A declarative component tree.** The server sends a "recipe" (screen, list, button, action). A trusted renderer inside the client draws it. A small (~30-50) but rich component set: layout, theme, animation presets, image/video. An isolated canvas component may come later.
3. **Design is made at design time.** The site owner's agent turns a style prompt into layout and theme. The output isn't code but a restricted format (Design Manifest). The verifier approves it and it's signed; everyone sees the same design.
4. **The client AI adapts only for the user:** font, contrast, simple layout, dark mode. Rule: the AI decides how things look, never what they are. Content and actions can't be added, hidden or changed (against prompt injection and AI phishing).
5. **Agents drive adoption.** Agents write apps or port existing sites, so the spec must be small, strict and machine-verifiable. Unsafe things shouldn't even be expressible.
6. **Two kinds of consumers:** people (through the renderer) and AI agents (reading the component tree and actions directly).
7. **Our own browser (2026-11-07).** Small, light, low-RAM, futuristic, minimal and AI-assisted, written in Rust. `inert://` sites show as **secured**, normal websites as **unsecured**. Normal web content is untrusted data: the browser's AI never takes text on those pages as instructions (prompt injection) and never acts there without approval. The normal web engine runs in an isolated process, apart from the Inert core and tokens. **Normal web engine: WebKit (WKWebView), the default (2026-11-11).** Servo was the default first; it fell short on everyday sites like Google and GitHub and became an option (Settings → Web engine). Servo runs in its own process (`inert-servo-host`, its own Cargo workspace) and draws offscreen, with frames in shared memory; the browser doesn't link Servo. Every page can be reopened in the other engine from the security panel. The demo profile and UI tests stay on Servo. Details in spec §18.
8. **Sign-in: users bring their own AI key.** "Sign in with Claude" and "Sign in with GPT". AI features (design generation, user adaptation, agent work) run on the user's own model. Neither provider offers OAuth sign-in to third-party apps, so an API key is the way in.

## Architecture
```
Site owner: Agent toolkit → Verifier (deploy) → Server (SDK)
                                        │ Protocol: Screen · Action · Result (QUIC/TLS1.3+PQ)
User:       Core (signature + verifier) → Renderer (+ adaptation AI) → Screen
                       └→ AI agent client (reads the tree directly)
```
- **Protocol spec:** `Screen` (component tree), `Action` (typed, schema'd request), `Result`. A strict binary format (CBOR). A message that doesn't fit the schema is rejected at parse time.
- **Two-layer signatures (2026-11-07):** the **App Manifest** signed at deploy time (screen templates, action schemas, Design Manifest) is signed with an offline key. Runtime `Screen` messages carry per-user data, so they can't be signed ahead of time. They come over TLS and the client checks every `Screen` against the signed manifest. Result: a compromised server can show wrong data, but it can't add a new input, button or fake password field.
- **Transport (2026-11-07):** in the slice, TCP + TLS 1.3 (`rustls`, PQ hybrid key exchange) + length-prefixed CBOR messages, behind a transport interface. The long-term target is QUIC (`quinn`). Carrying messages over HTTPS was rejected: it brings back the HTTP parser surface.
- **Trust model (2026-11-07): domain + pinning.** The connection is verified with a normal TLS certificate (WebPKI). The manifest signing key is published in DNS (`_inert.<domain>` TXT record). The browser pins the key on first visit and warns if it changes. Addresses stay familiar: `inert://bank.com`. The slice has no real DNS: in development mode the key sits in a local trust file, but the code follows this model. A transparency log (like Certificate Transparency) is deferred to the hardening phase.
- **Design Manifest:** the layout and theme the agent produced. Not code, signed.
- **Verifier:** one library, run in two places: at deploy and in the client before rendering.
- **Inert core:** Rust (connection, parsing, verifier, signatures, layout). No JS engine, no HTML engine, no plugins. Target size: a few tens of thousands of lines.
- **Browser: all Rust (2026-11-07).** `winit` (window) + `wgpu` (GPU), egui. The browser UI and the `inert://` renderer draw on the same GPU surface. Needed: accessibility (AccessKit), text input and IME (Turkish characters included).
- **Renderer:** `inert://` pages are drawn by our own renderer, not native widgets.
- **Server SDK + agent toolkit:** screen and action definitions. The spec and the verifier are offered to agents as tools (MCP).

## Layout
```
spec/          protocol vectors
crates/        Rust: proto, verify, transport, client, server, browser, Servo host
examples/      demo apps (bank, shop, notes)
fuzz/          fuzz targets
site/          the website
docs/          spec and notes
```

## Sub-projects, in order
1. Protocol and core (spec, parser, verifier, signatures)
2. Server SDK
3. Browser (shell, `inert://` renderer, Servo, AI panel)
4. Agent toolkit and MCP client
5. Website

Each goes through its own spec → plan → code cycle.

**The vertical slice spec:** `docs/superpowers/specs/2026-11-07-inert-vertical-slice-design.md`. Details are there; this file is a summary.

**The slice's component set (2026-11-07):** `Stack`, `Text`, `Button`, `Input`, `List`, `Divider`, `Image`, `Link`, `Toggle`, `Select`. Every new component is weighed before it enters the spec: "does it bring a class of holes?" Rules for four of them:
- `Image`: only memory-safe Rust decoders. Few formats (PNG, WebP), size and pixel limits. Decoding can move to an isolated process later.
- `Link`: moves between screens of the same site. A link to another site opens with an explicit warning; a link to the normal web is marked "unsecured".
- `Toggle` and `Select`: values and options are defined in the manifest. Only defined values can reach the server.

**The slice's example apps (2026-11-07),** all with fake data, none connected to a real service:
1. Bank dashboard: the main demo, the security story.
2. Shop and cart.
3. Notes and tasks.

## What it solves, what it doesn't
- Browser RCEs (V8/JIT, malicious JS): mostly solved; there's no JS on the client.
- PHP RCEs: partly. The path of arbitrary HTTP requests (headers, paths, uploads) closes. Dangerous logic inside the app (e.g. `unserialize`) stays.
- TLS library holes: not solved. Reduced with `rustls` and verified crypto.
- Linux kernel RCEs: not solved. The protocol runs on top of the OS.

## The browser today
- Glyph UI (blue glyph frame, sidebar, inset page card), egui + wgpu, the four Geist fonts built in.
- Sign-in: just the AI provider (Claude / GPT) + an optional API key (Keychain).
- Tabs, tab groups (Inbox, Homeserver, Developer + normal groups), bookmarks, history, omnibox (inline completion + suggestions), reopen closed tab, find in page, zoom, downloads, memory saver, native menu and shortcuts.
- `inert://` sites with a SECURED badge, the normal web UNSECURED / NOT ENCRYPTED.
- Shield: EasyList, EasyPrivacy and uBlock Origin lists. WebKit content blocker in WebKit tabs, request interception + Brave's adblock engine in Servo tabs.
- Private tabs, split view, Rewind, assistant (inert:// pages read from the signed tree, web pages as untrusted text), push-to-talk, watch-and-listen tools.
- UI tests: `scripts/ui-test.sh` (real input events). Build: `scripts/build.sh [--release]`. Bundle: `scripts/bundle.sh`.
- Memory (physical footprint): only the bank open, 73–74 MB (`scripts/memory-bank.sh`). `scripts/ui-test.sh` measures every run. If Servo crashes the browser stays open and restarts it.
- VPN: Mullvad (through its CLI).

## Deferred: for the hardening phase
- A secure runtime / hosting platform (defense in depth): `rustls` + verified crypto, every site in an isolated microVM (Firecracker, gVisor), long term a minimal or verified kernel (seL4).

## Visual identity
- **Typeface family: Geist, all four.** npm: `geist` (≥1.9.0).
  - **Geist Serif:** headlines and brand voice. Regular only for now: emphasis through size, not weight.
  - **Geist Sans:** body text, navigation, buttons.
  - **Geist Mono:** labels (capitals, wide tracking), protocol examples, code.
  - **Geist Pixel:** the wordmark (dark direction), step numbers, small accents.
- **Tone:** as minimal and futuristic as possible.
- **Website: Direction A (Stencil language, dark).** Code in `site/` (plain HTML).
- **Browser UI: Glyph language (2026-11-10).** Main theme glyphblue: a cobalt blue (`#1F4DFF`) window frame with a white ASCII glyph map (Geist Mono) that changes under the pointer. Layout and motion from Dia and Facet: tabs and groups in the left sidebar (Cmd+S hides it), the page in an inset rounded dark card, soft glass pill buttons, Geist Sans headings. Details in spec §18.2, code in `theme.rs`, `art.rs`, `chrome.rs`.
- **Logo: "Held caret"** (a text caret held still between two brackets), used in the browser, the app icon and the website.

## Open questions
- First development and test platform: macOS (the browser is cross-platform).
- Exact wording of the "unsecured" label.
- Brand and domain availability not checked.
