# Inert: vertical slice design

- **Date:** 2026-11-07
- **Status:** Implemented (night of 2026-11-07). This version is reconciled with the decisions made while building it; the changes are in the log below.
- **Scope:** The smallest end-to-end version of sub-projects 1–3 (protocol and core, server SDK, browser).
- **Context:** The project's overall decisions are in `CLAUDE.md`. This document only defines the vertical slice.

### Change log

| Date | Section | Change |
|---|---|---|
| 2026-11-07 | §2 | Servo and the normal web, a read-only assistant, "Sign in with Claude / GPT", Mullvad VPN and Shield brought into scope. |
| 2026-11-07 | §4 | Added `inert-client`, `inert-servo-ipc` and `inert-servo-host`. `inert-servo-host` is its own Cargo workspace. |
| 2026-11-07 | §11.1 | Drawing layer: `egui` + `eframe` (wgpu) instead of `vello` + `parley` + `taffy`, the alternative §17 allowed. |
| 2026-11-07 | §11.2 | Three process classes instead of one: the browser, the Servo process, WebKit's own processes. |
| 2026-11-07 | §11.3 | `https://` opens: engine selection rules. |
| 2026-11-07 | §16 | The memory criterion is defined and measured per process. |
| 2026-11-07 | §18.1 | Chrome-style tabs and omnibox, new tab search box, history timeline, Cookies extension, Mullvad sign-in and sign-out, import from Chrome. |
| 2026-11-11 | §18.4 | WebKit becomes the default web engine; Servo stays an option. |

---

## 1. Goal

Prove the Inert idea works end to end:

1. A small server sends a signed App Manifest and `Screen` data that conforms to it.
2. The Rust browser verifies the manifest signature against the pinned key, verifies every `Screen` against the manifest's template, and draws it on the GPU.
3. When the user triggers an action, a typed `Action` goes out, the server verifies it again, the handler runs and a new `Screen` comes back.
4. The attack scenarios the project promises to stop are rejected by automated tests.

## 2. Scope

**Out of scope:** the user-adaptation AI (fonts, contrast, simple layout), the agent toolkit and MCP client, key discovery through real DNS records, a transparency log, QUIC, sign-in screens and accounts inside `inert://` apps, the website, distribution packages and a signed `.app`.

**Brought into scope later (2026-11-07):** these were out of scope in the first draft.
- The normal web (§11.3).
- A read-only assistant panel. It reads `inert://` pages from the signed tree and web pages as untrusted text, and takes no actions.
- "Sign in with Claude / GPT".
- A Mullvad VPN panel and Shield ad and tracker blocking.
- Browser features inspired by Chrome: tab groups (Inbox, Homeserver, Developer), bookmarks, omnibox completion.

## 3. Terms

| Term | Meaning |
|---|---|
| **App Manifest** | The document signed at deploy time with an offline key: screen templates, action schemas, design tokens, static asset hashes. |
| **Template** | A screen's component tree. Its dynamic parts are typed **slots**. |
| **Slot** | A typed place in a template, filled at runtime by `Screen` data. |
| **Screen** | Runtime data that fills a template. Unsigned, it arrives over TLS and must conform to the template. |
| **Action** | A request the user triggers. It must conform to its schema. |
| **Pin** | The manifest signing key the browser accepts for a domain. |

## 4. Architecture

Two Cargo workspaces. The main one holds the crates below; `inert-servo-host` lives in its own workspace, so Servo's dependency tree and unified Cargo features never reach the browser.

| Crate | Role | Depends on |
|---|---|---|
| `inert-proto` | Message and manifest types, strict CBOR encode/decode, limits. | — |
| `inert-verify` | Signature checks (Ed25519), manifest rules, `Screen` ↔ template and `Action` ↔ schema verification, contrast checks. | `inert-proto` |
| `inert-transport` | The `Transport` trait, a TCP + `rustls` implementation, framing, timeouts. | `inert-proto` |
| `inert-server` | Server SDK, the `inert-sign` and `inert dev` commands. | proto, verify, transport |
| `inert-client` | The session shared by the browser and the headless test client: dial, pin, verify the signature and templates, fetch assets. | proto, verify, transport |
| `inert-browser` | Window, drawing, layout, the 10 components, browser UI, trust store. Does not link Servo. | client, servo-ipc |
| `inert-servo-ipc` | Messages between the browser and the Servo process (JSON lines). No `unsafe`. | — |
| `inert-servo-host` | Servo in its own process: draws normal web pages offscreen and writes frames to shared memory. Its own workspace. | servo-ipc, servo |
| `examples/{bank,shop,notes}` | Example apps. | `inert-server` |

**Rules:**
- `inert-proto` and `inert-verify` cannot depend on the network, the file system, UI or an async runtime. Both use `#![forbid(unsafe_code)]`.
- Verification logic lives only in `inert-verify`. The server and the browser call the same code.

## 5. Wire protocol

### 5.1 Framing

```
[ u32 length, big-endian ][ CBOR body ]
```

Each body is one message: `{ "t": <message type>, ...fields }`. If the length exceeds the limit for that message type (§12), `Fatal` is sent without reading the body and the connection closes.

### 5.2 Strict CBOR rules

The decoder rejects:
- Any encoding other than RFC 8949 "core deterministic encoding" (shortest integer form, sorted map keys).
- Indefinite-length items.
- Repeated map keys.
- Every CBOR tag.
- Floats (numbers are integers or the decimal form in §6.4).
- Any field the type doesn't define.
- Anything beyond the depth, length and size limits in §12.

The encoder only produces deterministic encoding. `encode(decode(x)) == x` holds for every valid message.

### 5.3 Messages

| Direction | `t` | Fields |
|---|---|---|
| B → S | `hello` | `v: uint` (protocol version, `1` in the slice) |
| S → B | `welcome` | `v: uint`, `manifest: bstr` (the manifest's deterministic CBOR), `sig: bstr(64)`, `key_id: tstr` |
| B → S | `open` | `screen: tstr`, `params: map` |
| S → B | `screen` | `screen: tstr`, `data: map` |
| B → S | `action` | `id: bstr(16)`, `screen: tstr`, `action: tstr`, `fields: map` |
| S → B | `result` | `id: bstr(16)`, `ok: bool`, `error?: { code: uint, field?: tstr }`, `next?: screen` |
| B → S | `get_asset` | `hash: bstr(32)` |
| S → B | `asset` | `hash: bstr(32)`, `mime: tstr`, `bytes: bstr` |
| both | `fatal` | `code: uint` |

### 5.4 Session flow

1. TCP connection + TLS 1.3 handshake.
2. The browser sends `hello`. If the version isn't supported, the server answers `fatal`.
3. The server sends `welcome`. The browser verifies the signature and pin (§8), then the manifest rules (§7.1).
4. The browser sends `open` for the manifest's `entry` screen. The server answers `screen`.
5. User actions go through `action` → `result`. `result.next`, when present, is the new screen.
6. Moving to another screen of the same site with a `Link` is a new `open`.
7. Images are fetched on demand with `get_asset` → `asset` and cached for the session.

One TCP connection is one session. Session state lives in server memory. The slice has no sign-in; each session gets a fake user.

## 6. App Manifest

### 6.1 Structure

```
Manifest {
  app:      tstr            // domain, e.g. "bank.example"
  version:  uint            // app version, increases with every signature
  entry:    tstr            // first screen
  design:   Design
  assets:   map<hash, AssetMeta>     // static images
  screens:  map<tstr, ScreenDef>
  actions:  map<tstr, ActionDef>
}

ScreenDef { params: map<tstr, FieldType>, slots: map<tstr, SlotType>, root: Node }
ActionDef { fields: map<tstr, FieldType> }
AssetMeta { mime: tstr, bytes: uint, width: uint, height: uint }
```

Developers and agents write the manifest as JSON (`app.inert.json`), with a published JSON Schema. JSON is only the source format; the wire always carries deterministic CBOR.

### 6.2 Components

Every `Node` is one of the following. Text and image values are either literals or bound to a slot (`bind`).

| Component | Properties |
|---|---|
| `Stack` | `dir: v \| h`, `gap: spacing token`, `align: start \| center \| end \| stretch`, `children: [Node]` |
| `Text` | `style: display \| title \| body \| mono \| label`, `text: literal \| bind` |
| `Button` | `label: literal`, `action: tstr`, `args?: map<field, literal \| bind>`, `variant: primary \| ghost` |
| `Input` | `action`, `field`, `label`, `kind: text \| number \| multiline`, `placeholder?` |
| `Select` | `action`, `field`, `label` (options come from the field's type) |
| `Toggle` | `action`, `field`, `label` |
| `List` | `bind: slot`, `item: Node` (`item.<field>` can be bound inside an item) |
| `Divider` | — |
| `Image` | `src: asset hash \| bind`, `alt: literal` (required), `width`, `height` |
| `Link` | `label`, `to: screen name` or `href: inert://… \| https://…`, `params?` |

**Form model:** `Input`, `Select` and `Toggle` bind to one field of an action. When a `Button` on the same screen triggers that action, the values of the fields bound to it and the button's `args` together become `action.fields`. The combined field set must match the action's schema exactly.

### 6.3 Slot types

`string { max }`, `int { min, max }`, `decimal { scale, min, max }`, `bool`, `image` (a dynamic image hash), `list { max, item: map<tstr, SlotType> }`.

### 6.4 Field types (action and `open` parameters)

| Type | Rule |
|---|---|
| `string { max }` | UTF-8, no control characters, length ≤ `max`. No regex in the slice. |
| `int { min, max }` | Within the i64 range. |
| `decimal { scale, min, max }` | `[mantissa: int, scale: uint]` on the wire. `scale` can't exceed the type's. |
| `bool` | — |
| `enum { options: [{ value, label }] }` | The value must be one of the manifest's options. |
| `choice { from: slot }` | `from` must be a `list` slot whose items have `id: string` and `label: string`. The value must be one of that list's `id`s in the current `Screen` data (for example the user's own accounts). The server also checks ownership. |

### 6.5 Design tokens

```
Design {
  colors:  { bg, fg, muted, accent, on_accent, line }    // sRGB hex
  fonts:   { display, title, body, mono, label } → "geist-serif" | "geist-sans" | "geist-mono" | "geist-pixel"
  sizes:   { display, title, body, mono, label }          // px
  spacing: { xs, s, m, l, xl }                            // px
  radius:  uint
}
```

Sites can't send font files. They can only choose among the four Geist fonts built into the browser.

### 6.6 Images

- **Static:** their hashes are in `assets`, with bytes and size written in the manifest.
- **Dynamic:** only for `image` slots. The value is a hash and the image is fetched with `get_asset`. They aren't in the manifest.
- Either way the browser compares the SHA-256 of the bytes with the hash, then decodes within the limits of §12.
- Formats: `image/png` and `image/webp`, decoded by memory-safe Rust crates.

## 7. Verification rules (`inert-verify`)

### 7.1 Manifest (in `inert-sign build` and in the browser)

- The signature is valid and belongs to `key_id`'s key.
- The `entry` screen exists. Every `Button`, `Input`, `Select` and `Toggle` is bound to an existing action and field. Every `Link.to` points to an existing screen.
- Every `bind` points to a slot defined on the screen, with a type that fits the component (for example `Text` → `string`, `int` or `decimal`; `Image` → `image`).
- An action's field set matches, exactly, the form fields + `args` on every screen that triggers it.
- Every static image hash is in `assets`.
- Every `Image` has an `alt`.
- **Contrast:** the WCAG contrast ratio of `fg/bg`, `muted/bg`, `accent/bg` and `on_accent/accent` is ≥ 4.5:1. This is for accessibility, and against prompt injection through invisible text.
- All numeric limits are within §12.

### 7.2 `Screen` (in the browser, and in the server before sending)

- The `screen` name exists in the manifest.
- `data` holds exactly the screen's slot set: nothing missing, nothing extra.
- Every value fits its slot type and limits. Lists don't exceed `max` and every item fits the item type.

### 7.3 `Action` (in the browser before sending, in the server before the handler)

- The `action` name exists in the manifest and can be triggered from the screen named `screen`.
- `fields` holds exactly the action's field set and every value fits its type.
- `choice` fields are checked against the `Screen` data the session last sent for that screen. The server keeps that data in the session.
- `id` is 16 bytes. If it was seen before in the same session the action isn't processed and the previous `result` is sent again.

### 7.4 `Open` (in the server before the handler)

- The `screen` name exists in the manifest.
- `params` holds exactly the screen's `params` set and every value fits its field type (§6.4).

## 8. Trust model

- **Channel:** TLS 1.3 with `rustls`, post-quantum hybrid key exchange on. WebPKI certificate verification in production.
- **Manifest key:** Ed25519. In production the key is published in a `_inert.<domain>` TXT record and pinned on first visit. **The slice has no DNS**; this path is only defined as an interface.
- **Trust store:** `~/.config/inert/trust.toml`

  ```toml
  [pins."bank.example"]
  key = "ed25519:<base64>"
  pinned_at = "2026-11-07T00:00:00Z"
  ```

- **Development mode:** the file given with `inert-browser --dev-trust <file>` holds both the (self-signed) TLS certificate and the manifest key. This mode is marked `DEV` in the address bar.
- **Key change:** a manifest that doesn't match the pinned key doesn't open; a full-screen warning shows. The new key isn't pinned without the user's explicit approval.
- **Rollback protection:** the browser stores the highest `manifest.version` it has seen for each domain and rejects lower ones.

## 9. Transport

- The `Transport` trait: connect, send frame, receive frame, close. The slice's only implementation is TCP + `rustls`. QUIC will come later behind the same trait.
- Async runtime: `tokio`, only in `inert-transport`, `inert-server` and the browser's network thread.
- Timeouts are in §12.

## 10. Server side

### 10.1 `inert-sign`

- `inert-sign keygen --out <file>`: makes an Ed25519 key pair. The key file stays outside the repo.
- `inert-sign build app.inert.json --key <file> --out manifest.signed.cbor`: reads the JSON, applies the rules of §7.1, produces deterministic CBOR and signs it. If any rule fails, there is no output.

### 10.2 Server SDK

```rust
let mut app = App::load("manifest.signed.cbor")?;

app.screen("overview", |session, params| {
    Ok(ScreenData::new()
        .set("balance", session.user.balance())
        .set("transactions", session.user.transactions()))
});

app.action("transfer", |session, action| {
    let amount = action.decimal("amount");   // already verified
    let from = action.choice("from");
    session.user.transfer(from, amount)?;
    Ok(Next::screen("overview"))
});

app.serve("0.0.0.0:7443", tls_config).await
```

- **The server never sees the signing key**; it only loads `manifest.signed.cbor`.
- **Startup check:** every screen and action in the manifest must have exactly one handler. If any is missing or extra, `serve` doesn't start.
- Only actions that passed §7.3 reach a handler. Every `Screen` a handler returns is checked against §7.2 before it's sent. If that fails the client gets an `internal` error and the details are logged.
- Handler panics are caught and turned into `internal` errors.

### 10.3 `inert dev`

One command: makes a self-signed TLS certificate and a development key (if missing), signs the manifest, starts the server, writes the `--dev-trust` file, and re-signs and reloads when `app.inert.json` changes. Development keys are in `.gitignore`.

## 11. Browser (`inert-browser`)

### 11.1 Layers

| Job | Used |
|---|---|
| Window, input, IME | `winit` (through `eframe`) |
| GPU | `wgpu` |
| 2D drawing, text, layout | `egui` 0.37 (instead of `vello` + `parley` + `taffy`, §17) |
| Accessibility | `accesskit` (egui's own integration) |
| Image decoding | Memory-safe `png` and `image-webp` |
| Normal web | WebKit (`wry`, WKWebView) and Servo 0.7 (in its own process) |

### 11.2 Process and thread model

- **Browser process:** the `inert://` core, the renderer and the UI. Network and protocol run on their own thread (`tokio`); only verified screens reach the main thread, over a channel. Drawing and input are on the main thread. Servo isn't linked into this process; AI keys and pins live only here.
- **Servo process (`inert-servo-host`):** started when the first Servo tab opens. The browser writes JSON-line commands to its stdin (create, load, show, resize, input, back/forward, zoom, script, Shield). The process writes events to stdout (load, URL, title, history, cursor, blocked request, `inert://` hand-off). The active tab's frames go to a two-slot memory-mapped file that the browser maps read-only and copies from. If the process dies its tabs are dropped and it restarts at most three times, then tabs fall back to WebKit. When the browser exits stdin closes and the process exits on its own.
- **WebKit processes:** WKWebView runs its own WebContent and Networking processes in the macOS sandbox.

### 11.3 Browser UI

The address bar and the trust indicator are drawn with the same renderer. Indicator states:

| State | Indicator |
|---|---|
| Signature and pin valid | `● SECURED` + key fingerprint (first and last 4 characters) |
| Development mode | `● SECURED · DEV` |
| Key changed | Full-screen warning, the page isn't drawn |
| Protocol error | Error screen + code + "reconnect" |

If `Link.href` goes to another `inert://` domain, a confirmation shows. `https://` and `http://` pages open with an `UNSECURED` badge (`NOT ENCRYPTED` without TLS).

**Engine selection:**
1. The default engine is WebKit (Servo can be chosen in Settings).
2. Every page can be reopened in the other engine from the site security panel. The choice is per tab.
3. If the Servo process can't start or keeps crashing, tabs open in WebKit.
4. When a page in Servo follows an `inert://` link, Servo refuses the navigation and the link opens in the secured engine.

### 11.4 Renderer

- Template + `Screen` data → node tree → layout → drawing.
- Fonts are built in: Geist Serif, Sans, Mono and Pixel (SIL OFL).
- Numbers aren't localized in the slice: `int` as is, `decimal` with as many decimal places as its `scale` and a dot separator.
- The accessibility tree is built from the same node tree. Every `Image` has an `alt` and every form field a `label`.
- The browser checks every `Action` against §7.3 before sending it and shows errors under the field.

## 12. Limits (starting values)

| Limit | Value |
|---|---|
| Frame size: `welcome`, `asset` | 4 MiB |
| Frame size: other messages | 256 KiB |
| CBOR nesting depth | 32 |
| Template tree depth | 16 |
| Screens / actions | 256 / 256 |
| Absolute maximum for `string` | 4096 characters |
| Absolute maximum for `list` | 1000 items |
| Image size | 2 MiB, at most 4096 × 4096 and 16 megapixels |
| Handshake timeout | 10 s |
| Single message read timeout | 30 s |
| Idle timeout | 300 s |
| `action.id`s remembered per session | last 1024 |

A manifest can set its own limits below these absolute ones, never above.

## 13. Error handling

### 13.1 Classes

| Class | What happens |
|---|---|
| **Fatal** (protocol violation) | `fatal` is sent, the connection closes, the browser shows an error screen. No partial drawing. |
| **Recoverable** (action error) | `result { ok: false, error }`. The browser shows the error under the field. |
| **Trust** | Full-screen warning. |
| **Network** | "Reconnect". Actions are never retried automatically. |

### 13.2 Error codes

| Code | Name | Class |
|---|---|---|
| 1 | `unsupported_version` | fatal |
| 2 | `frame_too_large` | fatal |
| 3 | `malformed_cbor` | fatal |
| 4 | `unknown_message` | fatal |
| 5 | `bad_signature` | fatal |
| 6 | `manifest_invalid` | fatal |
| 7 | `screen_invalid` | fatal |
| 8 | `asset_invalid` | fatal |
| 9 | `timeout` | fatal |
| 100 | `field_invalid` | recoverable |
| 101 | `action_unknown` | recoverable |
| 102 | `rejected` | recoverable (a business rule in the handler, for example insufficient funds) |
| 103 | `internal` | recoverable |

## 14. Example apps

| App | Screens | Notable components |
|---|---|---|
| `bank` | Overview, Transfer, Settings | `List` (transactions), `Select` + `choice` (account), `Input` (amount), `Toggle` (notifications), `Image` (static logo) |
| `shop` | Catalog, Product, Cart, Checkout | `Image` (dynamic product image), `Select` + `enum` (size), `Input` (quantity), `Link` |
| `notes` | List, Note | `Input` (title, text), `Toggle` (done), `Select` (category), `Button` + `args` inside a `List` (delete) |

All data is fake and in memory. Nothing connects to a real service. Product images are placeholders generated in the repo.

## 15. Test strategy

| Layer | Tests |
|---|---|
| `inert-proto` | `proptest` encode/decode round trips. Tests that non-deterministic CBOR, tags, floats, repeated keys and extra fields are rejected. `cargo-fuzz` target: the decoder. |
| `inert-verify` | Unit tests for every rule. `cargo-fuzz` targets: manifest, `Screen` and `Action` verification. |
| Conformance vectors | Valid and invalid `manifest`, `screen` and `action` samples under `spec/vectors/`, each with its expected result and error code. Language-independent. |
| `inert-transport` | Integration tests over real TLS on localhost, timeouts included. |
| End to end | A headless test client (`inert-proto` + `inert-verify` + `inert-transport`) drives all three examples. |
| `inert-browser` | A UI driver that injects mouse and keyboard events into the real browser (`drive.rs`). |

### 15.1 Attack scenarios (each an automated test)

1. The server sends a `Screen` with a password field that isn't in the template → `screen_invalid`.
2. The `Screen` data has an extra key → `screen_invalid`.
3. One byte of the manifest is changed → `bad_signature`.
4. The server presents a manifest signed with a different key → trust warning, nothing drawn.
5. A manifest with an older version → rejected.
6. Low-contrast tokens → `inert-sign build` fails.
7. An image over the limits or with a wrong hash → `asset_invalid`.
8. The same `action.id` sent twice → the handler runs once.
9. A `choice` field set to an account id not on the screen → `field_invalid`.
10. A frame over the limit → `frame_too_large`, the body isn't read.

## 16. Success criteria

- The three example apps, run with `inert dev`, open in `inert-browser` on macOS and every screen flow works.
- All ten attack scenarios of §15.1 pass.
- Every fuzz target runs locally for 30 minutes without a crash.
- `inert-proto` and `inert-verify` contain no `unsafe` code and have no I/O dependencies.
- The browser's memory with the bank example open is measured and reported. The target for the **browser process** is under 150 MB. The normal web engines' processes (Servo, WebKit) are measured and reported separately. This target is a first measurement, not an acceptance condition (results in §18).

## 17. Open points for planning

- ~~Whether current `vello` and `parley` are enough for the slice.~~ Decision: `egui` (§11.1).
- The CBOR crate (`minicbor` or similar) and how much custom code strict mode needs.
- Maturity of a memory-safe WebP decoder. If not enough, PNG only in the slice.

---

## 18. Implementation notes

Deliberate departures from the spec and additions. Each sits behind a single boundary in the code and can be reverted.

**Protocol additions (small, backward-compatible fields):**
- `Manifest.name`: the app name shown in the browser.
- `ScreenDef.title`: the screen name for the tab title and accessibility.
- Optional `init` on `Input` / `Select` / `Toggle`: the form field's initial value (a literal, or a bind to a top-level slot). The verifier checks that `init` fits the field type.
- The dev trust file (`~/Library/Application Support/Inert/dev-trust/<host>.toml`) carries the TLS dev CA, the manifest key and the address to dial together.

**Crate layout:** a sixth crate, `inert-client`. The browser and the headless test client share the same session code (pin, signature, template checks, asset hash checks).

**Error behavior:** an invalid `open` ends with `fatal screen_invalid` on the server, a failing screen handler with `fatal internal`. The server catches a screen that doesn't fit its template before sending it and returns `internal`.

**Drawing layer:** `egui` + `eframe` (wgpu) instead of `vello` + `parley` + `taffy` (§17 allowed it). Accessibility (AccessKit), IME, text editing and screen reader support come built in. The browser's own UI and the `inert://` renderer are drawn with the same layer.

**Normal web engine:** WebKit (WKWebView) by default since 2026-11-11 (§18.4); Servo 0.7 runs in its own process, `inert-servo-host` (§11.2). Servo draws offscreen with `SoftwareRenderingContext` and its frames are shown in egui as a texture from shared memory. Mouse, wheel and keyboard input are forwarded to it; `inert://` links are handed from Servo to the secured engine. Every web page can be reopened in the other engine from the site security panel. Shield blocks requests with Brave's `adblock` engine in Servo tabs and uses WebKit's own content blocker in WebKit tabs.

**epaint patch (removed):** with Servo linked into the browser, `vello_cpu`'s multithreading feature leaked into egui through Cargo feature unification and crashed glyph rasterization. A temporary `vendor/epaint` patch was used. Moving Servo into its own process and workspace separated the dependency trees and the patch was deleted.

**Sign-in:** "Sign in with Claude / GPT" only asks for the provider. Those providers don't offer OAuth sign-in to third-party apps, so the model is reached with an API key, stored in the macOS Keychain. The key can be skipped.

**VPN:** the Mullvad app's own WireGuard tunnel, controlled through the official `mullvad` CLI. The tunnel applies to the whole Mac, not just the browser, and the UI says so. The account number is never read back into the UI.

**Ad blocking:** filter lists are converted to WebKit content-blocker rules with Brave's `adblock` crate, compiled in WebKit once and reused from the cache on later launches.

**Measurements (release, macOS, Servo in its own process):**

The main figure is the physical footprint (`footprint`), the "Memory" column in Activity Monitor. RSS is given too, since macOS counts shared framework pages in it. `scripts/ui-test.sh` samples both processes continuously while it runs.

| State | Median | Max |
|---|---|---|
| **Only the bank open**, window redrawn continuously, 30 s (`scripts/memory-bank.sh`, §16) | **73–74 MB** | **74 MB** |
| UI driver run, browser process, footprint | **80–95 MB** | **98–105 MB** |
| Browser process, RSS | 126 MB | 140 MB |
| Servo process, footprint | 131 MB | 177 MB |
| Browser with the test's screenshots on (`SHOTS=1`) | 139 MB | 160 MB |

- §16: the browser process stays under the 150 MB target through the whole run. The normal web engine's memory is in its own process.
- **Where it came from:** in the first measurement the browser's median was 187 MB. Three causes:
  1. Servo was in the same process. It moved out.
  2. The macOS allocator kept large freed blocks (page frames, decoded images) dirty in its own cache, about 40 MB. That cache can only be turned off at process start, so the browser re-executes itself once with `MallocLargeCache=0` on first launch (same PID, a few ms). The Servo process inherits the setting.
  3. The test's in-app screenshots held about 36 MB of GPU memory in egui-wgpu's capture texture and readback buffers. That's the test's own overhead; screenshots are now optional (`SHOTS=1`).
- An earlier "70 MB with the bank open" figure was invalid: it was taken with the screen locked, when a normally opened window isn't drawn at all. The bank measurement now runs under the UI driver, which redraws about 33 times a second.
- **Crash isolation:** when the Servo process is killed with `kill -9`, the browser stays open, Servo tabs drop and a new Servo process starts.

**UI tests:** `crates/inert-browser/src/drive.rs` injects real mouse and keyboard events into egui's input (`App::raw_input_hook`), covering sign-in, omnibox completion, bank transfers (including an over-limit amount), the shop cart, notes, bookmarks, tab groups, the context menu, shortcuts, clicking and typing in Servo, switching engines, VPN, Shield, the assistant, history and more.

**Fuzzing:** four targets (`cbor_decode`, `message_decode`, `manifest_verify`, `screen_action_verify`) ran in parallel for 30 minutes each (§16): about 213 million inputs, no crashes or artifacts.

### 18.1 Chrome-style browser features (2026-11-07)

| # | Feature | What was done |
|---|---|---|
| 1 | Tabs and address bar | Chrome-shaped tabs with grow and shrink animations; a pill omnibox with a focus ring; dropdowns and panels slide and fade in. |
| 2 | Address bar text | The text field is one line tall and centered in the pill. Out of focus the host is white and the rest dimmed. |
| 3 | Tab groups | A group's tabs open and close in 0.24 s. Pill labels, filled when collapsed. |
| 4 | New tab search box | A real field with its own inline completion, suggestions, arrow keys and Enter. The address bar is left alone. |
| 5 | Dark DuckDuckGo | Searches and duckduckgo.com open with `kae=d`. WebKit pages see `prefers-color-scheme: dark`; Servo views are told `Theme::Dark`. |
| 6 | History | Every visit is recorded (up to 20,000). Day cards, times, search, row deletion, "Recently closed", "Clear browsing data" by time range (optionally with cookies). |
| 7 | Cookies extension | A built-in "Cookies" extension: the current site's cookies (values never shown), deletion one by one or per site, a "keep no cookies for this site" list, automatic rejection of cookie banners (OneTrust, Cookiebot, Didomi… and "Reject all / Only necessary" by text). `WKHTTPCookieStore` in WebKit, `SiteDataManager` in the Servo process. |
| 8 | Mullvad | Setup steps, sign-in with just the account number (16 digits checked, the number is kept neither in the UI nor in the profile), confirmed sign-out. |
| 9 | Moving from Chrome | Step 2 of the first run (skippable) and `about:import`. Finds Chrome, Brave, Edge, Arc, Vivaldi, Chromium and Dia profiles. Bookmarks, history and visits, saved passwords (into the macOS Keychain, filled from the key in the address bar), cookies (sessions stay signed in), search engine, bookmarks bar, startup pages. SQLite files are copied and read by the system's `sqlite3` in a separate process. The password and cookie key is "Chrome Safe Storage"; macOS asks the user first. |

The importer was tested end to end against a real Chrome run with a throwaway profile and `--use-mock-keychain` (`scripts/real-chrome-import.sh`): Chrome itself wrote visits, a cookie, a bookmark and a password, and the importer read them all back. With a wrong key, no passwords or cookies come out.

### 18.2 Glyph interface (2026-11-10)

The browser UI was redesigned, drawing on glyphblue (main theme), Dia and Facet (layout, buttons, motion).

| Part | Decision |
|---|---|
| Frame | The whole window is cobalt blue (`#1F4DFF`), with an ASCII glyph map drawn in Geist Mono; it rises and changes letters under the pointer. No textures: rows are drawn as text (`art.rs`). |
| Tabs | A left sidebar. Pinned tabs are glass tiles, groups folder rows with colored chips, tabs rows. Cmd+S hides it. VPN pill, history, settings and the assistant at the bottom. |
| Page | An inset dark card with rounded corners (`#090B12`). WebKit views are rounded with a layer radius, egui content with corner masks. |
| Address bar | A dark glass pill on the blue frame. A light runs along its bottom edge while a page loads. |
| Buttons | Plain-text pills (`theme::cta`). Mono capitals only for small labels. |
| New tab | A glyph card (hero) with the search box overlapping its bottom: field, search engine chip, send button. |
| Sign-in | The whole window is the glyph card; steps that need typing sit in a dark panel. |
| Motion | Hover and press transitions, tab rows opening and folding, group arrow rotation, the sidebar sliding, panels fading and sliding in, the page fading in on tab change. |
| macOS | The traffic lights are moved to the toolbar's line (`web::mac::traffic_lights`). |

- Sidebar rows carry stable ids. Automatic ids shifted when rows above folded and bound an open context menu to the wrong tab.
- A panel that opens upward (VPN) is laid out again in the same frame when its content changes (`request_discard`), so it never jumps.

### 18.3 Setup, assistant and page tools (2026-11-11)

| Part | Decision |
|---|---|
| Setup | Steps with a progress bar: model (API key) → look (Blue/Dark, color, sidebar) → permissions and import. An "Unlock your data" screen while passwords are read. |
| Look | `Settings.dark`, `accent` (8 colors, `theme::ACCENTS`). In the dark look the frame is near black and the glyphs take the chosen color. |
| Logo | "Held caret": a text caret held still between two brackets. The browser draws the app icon itself (`--write-icon`); `scripts/bundle.sh` builds Inert.app. |
| Chrome import | `Bookmarks` and `AccountBookmarks` are read together; site icons come from Chrome's `Favicons` database. If macOS protects the data folder, Full Disk Access steps are shown. Host-only cookies are written without a Domain attribute (otherwise `__Host-` cookies drop and GitHub sees half a session). |
| Site icons | `favicon.rs`: one PNG per host in the profile folder, from Chrome's cache and fetched after a visit. Bookmarks never start a network request on their own. |
| Page menu | Servo's context menu is drawn by the browser: copy, cut, paste, select all, link and image rows, translate the page, inspect with WebKit, add to the assistant. Cmd+C/V/X/A/Z go through our own Edit menu items to egui fields, Servo and WebKit. |
| Assistant | Replies are rendered as Markdown (`md.rs`). The panel sits at the side, floats over the page or has its own tab (`about:chat`). The clip tool attaches a region of the page as an image, "Attach to assistant" the selected text; both go to the model as data. On a new tab, open tabs and recent pages (titles and addresses only) are context. Alt+Enter asks the assistant what's in the address bar. |
| Shield | uBlock Origin's default lists: EasyList, EasyPrivacy and the uBlock filters. WebKit is limited to 140,000 rules in one list; Servo uses them all. |
| Push-to-talk | While Option+Space is held, `inert-dictate` (Swift, Apple Speech, on device when the language supports it) listens; on release the text is typed where the keyboard is. |

Not done: importing Chrome extensions (Inert runs no extensions), app connections such as Gmail, passkeys (§18.7).

### 18.4 Default web engine: WebKit (2026-11-11)

Servo wasn't enough for everyday use: Google sends its basic version, GitHub sessions get stuck, and some web features sites expect are missing. WebKit became the default; Servo stays an option in Settings and any page can be opened in Servo from the security panel. Profiles created while Servo was the default switch to WebKit once (`engine_v2`). The demo profile and UI tests stay on Servo.

Known gaps in WebKit pages: the clip tool and the floating assistant can't draw over the native view, and the context menu is WebKit's own ("Attach to assistant" and translation are in the ⋮ menu).

### 18.5 Private tabs, page tools and assistant additions (2026-11-11)

| Feature | How it works | Limit |
|---|---|---|
| Private tab (Shift+Cmd+N) | A WebKit view with its own temporary data store, never written to disk or shared with another tab. Not in history, Rewind or the session; no site icon is fetched. | WebKit only: Servo has a single cookie jar. |
| Every tab sealed off | Each private tab has its own store. | Normal tabs share one store. |
| Erases itself | Private tabs close after an idle time (never, 5, 15, 30, 60 min). | Idle time counts from the last input in the browser. |
| Downloads vanish | A file downloaded in a private tab is deleted when the tab closes, unless kept in Downloads. | — |
| Clipboard cleared | The pasteboard's change count is noted when the first private tab opens; if it changed, the pasteboard is cleared when the last one closes. | It can't tell which app copied. |
| Panic key (Ctrl+Opt+Cmd+H) | Closes every private tab and minimizes the window. | — |
| Hide my location | A page script answers every location request with "denied". | The location your network address reveals is the VPN's job. |
| Camera and microphone | The wry permission handler returns "ask"; WebKit asks every time. | — |
| Always-private sites | A listed site (eTLD+1) never opens in a normal tab; it goes to a private one. | — |
| Secret favorites | Kept in the Keychain, shown on the bookmarks page after Touch ID, opened in private tabs. Lock again after 5 minutes. | — |
| Touch ID lock | When on, private tabs are covered while Inert is in the background; Touch ID opens them. While a private tab shows, the window is hidden from screenshots and screen sharing (`sharingType`). | — |
| Split view | The active page on the left, a second page on the right; swap and close in its header. | The right pane shows WebKit and inert:// pages; Servo and Inert's own pages stay on the left. |
| Rewind | The text of visited pages (6000 characters, at most 1500 pages) is kept in `rewind.json` on this Mac; history search looks inside pages too. | Private tabs aren't remembered. |
| Ask all tabs | "All tabs" in the assistant, or the prompts on a new tab: every open page's text is gathered (4 s limit) and sent with one question. | Sleeping tabs join with title and address only. |
| Summarize | Menu and context menu: the page text goes to the model. | No video summaries. |
| Translate in place | The page's text nodes (at most 260 pieces, 14,000 characters) are translated with the user's model and written back. Without a key, a Google Translate tab opens. | Only the first part of long pages. |
| Instant answers | Sums are worked out in the address bar; short facts come from DuckDuckGo's instant answer service when web suggestions are on. | — |
| Read aloud | The selection, or else the article text, read with macOS `say`; press again to stop. | System voice and language. |
| Picture in picture | WebKit's PiP for the largest video on the page. | — |
| Captions | Turns on the video's own caption tracks (and YouTube's caption button). | Not live speech recognition. |
| Calm mode | A style that hides Shorts, feeds and recommendations on YouTube, X, Instagram and TikTok. | Depends on the sites' markup. |

Also: the security indicator in the address bar is just a colored lock (green: verified inert://, yellow: local development key or still verifying, orange: an ordinary HTTPS page, red open lock: unencrypted or failed verification); details open on click. The gap when scrolling past a page's edge is closed with `overscroll-behavior: none`. Without bookmarks the bookmarks bar takes no space.

Import and permissions: without a stable signature macOS forgets the Full Disk Access permission on every new build. If the Keychain has a code-signing certificate named "Inert Dev", `scripts/bundle.sh` uses it and the permission sticks.

### 18.6 Logo, layout settings and real-site checks (2026-11-11)

- **Settings in categories:** General, AI, Search & pages, Privacy, Shield & cookies, Passwords & import, VPN, About.
- **Sidebar width** is set by dragging its edge (200–440 pt, double-click for the default).
- **One layout, two states:** with the sidebar open, tabs are in it; closed (Cmd+S), they're in a strip on top. The address bar runs from the back/forward/reload buttons to the tools in both.
- **Menus over the page:** a WebKit view is native, so Inert's drawing can't go above it. When a menu opens, a snapshot of the page (`takeSnapshotWithConfiguration`) is drawn in its place while the view hides.
- **Shield:** a real ad script (`pagead2.googlesyndication.com`) is blocked in WebKit (measured). YouTube video ads come from the same servers as videos and can't be blocked by network rules; with Shield on they're skipped as they start and ad slots are hidden (`page_js::YT_ADS`).
- **Real-site run** (`INERT_DRIVE_SCENARIO=real-web`, a throwaway profile through `INERT_PROFILE_DIR`, WebKit): page loads, overscroll gap, hidden location, Rewind, history, translate in place, read-aloud text, site icons, GitHub, page snapshot under menus, Shield, calm mode on YouTube, picture in picture, captions, private tabs, always-private sites, split view with two real pages, instant answers, the panic key.
- **Known:** when the screen locks macOS treats the window as occluded and stops producing frames; tests run under `caffeinate -dimu`. WebKit sends no "loaded" event for a directly opened video or image; if the active tab stays loading, the page's `document.readyState` is asked and loading ends.

### 18.7 Gmail, Google sign-in, passkeys (2026-11-11)

- **Empty Gmail messages:** uBlock Origin's generic hiding rule `##.ads` hid Gmail's message container (`div.adn.ads`). uBO applies that rule with site exceptions, which are lost in WebKit's rule format. Generic hiding rules (`css-display-none` without a site) are no longer compiled; network blocking and site-specific hiding stay (`inert-adblock-v3`).
- **Blank Google sign-in:** the popup opened as a tab and lost its link to the opener (`window.opener`). Sign-in providers and popups opened with a size now open as real windows (wry `NewWindowResponse::Allow`).
- **Passkeys:** WebKit's passkey and security key support needs the `com.apple.developer.web-browser.public-key-credential` entitlement, which Apple grants only to approved browsers, and a Developer ID signature. Until then the menu has "Open in Safari (passkeys)".

### 18.8 Cursors and bug hunt (2026-11-11)

- The pointing hand shows over everything that is only clickable; text fields show the I-beam. WebKit pages show their own cursor: the window's cursor rects (egui's cursor over the whole view) are off while the mouse is over a web view.
- A bug hunt over every feature found 13 bugs; all are fixed. Notes in `docs/bug-hunt-2026-11-11.md`.
