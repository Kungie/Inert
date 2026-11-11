# Inert — Dikey Dilim Tasarımı

- **Tarih:** 2026-11-07
- **Durum:** Uygulandı (2026-11-07 gecesi). Bu sürüm, uygulama sırasında verilen kararlarla uzlaştırıldı. Değişiklikler aşağıdaki günlükte.
- **Kapsam:** Alt proje 1–3'ün en küçük uçtan uca hâli (protokol + çekirdek, server SDK, tarayıcı)
- **Bağlam:** Projenin genel kararları `CLAUDE.md` dosyasında. Bu belge sadece dikey dilimi tanımlar.

### Değişiklik günlüğü

| Tarih | Bölüm | Değişiklik |
|---|---|---|
| 2026-11-07 | §2 | Servo ve normal web, salt okunur asistan, "Sign in with Claude / GPT" ve Mullvad VPN + Shield, kullanıcının gece hedefiyle kapsama alındı. |
| 2026-11-07 | §4 | `inert-client`, `inert-servo-ipc` ve `inert-servo-host` eklendi. `inert-servo-host` ayrı bir Cargo workspace. |
| 2026-11-07 | §11.1 | Çizim katmanı `vello` + `parley` + `taffy` yerine `egui` + `eframe` (wgpu). §17'nin izin verdiği alternatif. |
| 2026-11-07 | §11.2 | Tek süreç yerine üç süreç sınıfı: tarayıcı, Servo süreci, WebKit'in kendi süreçleri. |
| 2026-11-07 | §11.3 | `https://` artık açılıyor: Servo varsayılan, Inbox sekmeleri WebKit, motor seçimi kuralları. |
| 2026-11-07 | §16 | Bellek kriteri süreç bazında tanımlandı ve ölçüldü. |
| 2026-11-07 | §18.1 | Sabah turu: Chrome tarzı sekme ve omnibox, yeni sekme arama kutusu, geçmiş zaman çizelgesi, Cookies eklentisi, Mullvad giriş/çıkış, Chrome'dan içe aktarma. 71 UI kontrolü. |

---

## 1. Amaç

Inert fikrinin uçtan uca çalıştığını kanıtlamak:

1. Küçük bir sunucu, imzalı bir App Manifest ve buna uyan `Screen` verisi gönderir.
2. Rust ile yazılmış tarayıcı, manifest imzasını pinlenmiş anahtara karşı doğrular, her `Screen`'i manifest'teki şablona karşı doğrular ve GPU ile çizer.
3. Kullanıcı bir aksiyon tetiklediğinde tipli bir `Action` gider, sunucu bunu tekrar doğrular, handler çalışır ve yeni bir `Screen` döner.
4. Projenin vaadi olan saldırı senaryoları otomatik testlerle reddedilir.

## 2. Kapsam

**Kapsam dışı:** Kullanıcı uyarlama AI'ı (font, kontrast, sade düzen), agent toolkit ve MCP istemcisi, gerçek DNS kaydı ile anahtar keşfi, şeffaflık kaydı, QUIC, `inert://` uygulamalarında giriş ekranı ve hesaplar, web sitesi, dağıtım paketleri ve imzalı `.app`.

**Sonradan kapsama alınanlar (2026-11-07, kullanıcının gece hedefi):** İlk taslakta kapsam dışıydılar.
- Normal web, Servo ile (§11.3).
- Asistan paneli, salt okunur. `inert://` sayfalarını imzalı ağaçtan, web sayfalarını güvenilmez metin olarak okur, hiçbir aksiyon almaz.
- "Sign in with Claude / GPT" girişi.
- Mullvad VPN paneli ve Shield reklam/izleyici engelleme.
- Chrome'dan esinlenen tarayıcı özellikleri: sekme grupları (Inbox, Homeserver, Developer), yer imleri, omnibox tamamlama.

## 3. Terimler

| Terim | Anlamı |
|---|---|
| **App Manifest** | Deploy zamanında çevrim dışı bir anahtarla imzalanan belge: ekran şablonları, aksiyon şemaları, tasarım token'ları, statik asset hash'leri. |
| **Şablon** | Bir ekranın bileşen ağacı. Dinamik kısımları tipli **slot**'lardır. |
| **Slot** | Şablondaki, çalışma zamanında `Screen` verisiyle doldurulan tipli yer. |
| **Screen** | Bir şablonu dolduran çalışma zamanı verisi. İmzasızdır, TLS ile gelir, şablona uyması zorunludur. |
| **Action** | Kullanıcının tetiklediği, şemaya uyması zorunlu istek. |
| **Pin** | Tarayıcının bir alan adı için kabul ettiği manifest imza anahtarı. |

## 4. Mimari

İki Cargo workspace. Ana workspace aşağıdaki crate'leri içerir, `inert-servo-host` ise kendi workspace'inde durur. Böylece Servo'nun bağımlılık ağacı ve birleşen Cargo özellikleri tarayıcıya hiç karışmaz.

| Crate | Görevi | Bağımlılık |
|---|---|---|
| `inert-proto` | Mesaj ve manifest tipleri, katı CBOR encode/decode, sınırlar. | — |
| `inert-verify` | İmza kontrolü (Ed25519), manifest kuralları, `Screen` ↔ şablon ve `Action` ↔ şema doğrulaması, kontrast kontrolü. | `inert-proto` |
| `inert-transport` | `Transport` trait'i, TCP + `rustls` implementasyonu, çerçeveleme, zaman aşımları. | `inert-proto` |
| `inert-server` | Server SDK, `inert-sign` ve `inert dev` komutları. | proto, verify, transport |
| `inert-client` | Tarayıcı ile ekransız test istemcisinin paylaştığı oturum: bağlan, pinle, imzayı ve şablonları doğrula, asset al. | proto, verify, transport |
| `inert-browser` | Pencere, çizim, layout, 10 bileşen, tarayıcı arayüzü, güven deposu. Servo'ya bağlanmaz. | client, servo-ipc |
| `inert-servo-ipc` | Tarayıcı ile Servo süreci arasındaki mesajlar (JSON satırları). `unsafe` yok. | — |
| `inert-servo-host` | Ayrı süreçte Servo: normal web sayfalarını ekran dışında çizer, kareleri paylaşılan belleğe yazar. Ayrı workspace. | servo-ipc, servo |
| `examples/{bank,shop,notes}` | Örnek uygulamalar. | `inert-server` |

**Kurallar:**
- `inert-proto` ve `inert-verify` ağ, dosya sistemi, UI ya da async runtime'a bağımlı olamaz. İkisi de `#![forbid(unsafe_code)]` kullanır.
- Doğrulama mantığı sadece `inert-verify`'da yaşar. Sunucu ve tarayıcı aynı kodu çağırır.

## 5. Kablo protokolü

### 5.1 Çerçeveleme

```
[ u32 uzunluk, big-endian ][ CBOR gövde ]
```

Her gövde tek bir mesajdır: `{ "t": <mesaj tipi>, ...alanlar }`. Uzunluk, mesaj tipine göre geçerli sınırı (§12) aşarsa okuma yapılmadan `Fatal` gönderilir ve bağlantı kapanır.

### 5.2 Katı CBOR kuralları

Decoder aşağıdakileri reddeder:
- RFC 8949 "core deterministic encoding" dışındaki her kodlama (en kısa tamsayı biçimi, sıralı map anahtarları).
- Belirsiz uzunluklu (indefinite-length) öğeler.
- Tekrarlanan map anahtarları.
- Her CBOR tag'i.
- Float değerler (sayılar tamsayı ya da §6.4'teki decimal biçimiyle taşınır).
- Tipte tanımlı olmayan her alan.
- §12'deki derinlik, uzunluk ve boyut sınırlarını aşan her şey.

Encoder sadece deterministik kodlama üretir. `encode(decode(x)) == x` her geçerli mesaj için doğrudur.

### 5.3 Mesajlar

| Yön | `t` | Alanlar |
|---|---|---|
| T → S | `hello` | `v: uint` (protokol sürümü, dilimde `1`) |
| S → T | `welcome` | `v: uint`, `manifest: bstr` (manifest'in deterministik CBOR'u), `sig: bstr(64)`, `key_id: tstr` |
| T → S | `open` | `screen: tstr`, `params: map` |
| S → T | `screen` | `screen: tstr`, `data: map` |
| T → S | `action` | `id: bstr(16)`, `screen: tstr`, `action: tstr`, `fields: map` |
| S → T | `result` | `id: bstr(16)`, `ok: bool`, `error?: { code: uint, field?: tstr }`, `next?: screen` |
| T → S | `get_asset` | `hash: bstr(32)` |
| S → T | `asset` | `hash: bstr(32)`, `mime: tstr`, `bytes: bstr` |
| her iki yön | `fatal` | `code: uint` |

### 5.4 Oturum akışı

1. TCP bağlantısı + TLS 1.3 el sıkışması.
2. Tarayıcı `hello` gönderir. Sürüm desteklenmiyorsa sunucu `fatal` döner.
3. Sunucu `welcome` gönderir. Tarayıcı imzayı ve pin'i doğrular (§8), ardından manifest kurallarını (§7.1) kontrol eder.
4. Tarayıcı manifest'teki `entry` ekranı için `open` gönderir. Sunucu `screen` döner.
5. Kullanıcı aksiyonları `action` → `result` döngüsüyle ilerler. `result.next`, varsa yeni ekrandır.
6. `Link` ile aynı site içinde başka bir ekrana geçmek yeni bir `open` mesajıdır.
7. Görseller ihtiyaç anında `get_asset` → `asset` ile alınır ve oturum boyunca önbellekte tutulur.

Bir TCP bağlantısı bir oturumdur. Oturum durumu sunucu belleğinde tutulur. Dilimde giriş yoktur, her oturuma sahte bir kullanıcı atanır.

## 6. App Manifest

### 6.1 Yapı

```
Manifest {
  app:      tstr            // alan adı, ör. "bank.example"
  version:  uint            // uygulama sürümü, her imzada artar
  entry:    tstr            // açılış ekranı
  design:   Design
  assets:   map<hash, AssetMeta>     // statik görseller
  screens:  map<tstr, ScreenDef>
  actions:  map<tstr, ActionDef>
}

ScreenDef { params: map<tstr, FieldType>, slots: map<tstr, SlotType>, root: Node }
ActionDef { fields: map<tstr, FieldType> }
AssetMeta { mime: tstr, bytes: uint, width: uint, height: uint }
```

Geliştiriciler ve agent'lar manifest'i JSON olarak yazar (`app.inert.json`). Bunun için bir JSON Schema yayınlanır. JSON sadece kaynak formattır, kabloda her zaman deterministik CBOR gider.

### 6.2 Bileşenler

Her `Node` aşağıdakilerden biridir. Metin ve görsel değerleri ya sabit (literal) olur ya da bir slot'a bağlanır (`bind`).

| Bileşen | Özellikler |
|---|---|
| `Stack` | `dir: v \| h`, `gap: spacing token`, `align: start \| center \| end \| stretch`, `children: [Node]` |
| `Text` | `style: display \| title \| body \| mono \| label`, `text: literal \| bind` |
| `Button` | `label: literal`, `action: tstr`, `args?: map<field, literal \| bind>`, `variant: primary \| ghost` |
| `Input` | `action`, `field`, `label`, `kind: text \| number \| multiline`, `placeholder?` |
| `Select` | `action`, `field`, `label` (seçenekler alanın tipinden gelir) |
| `Toggle` | `action`, `field`, `label` |
| `List` | `bind: slot`, `item: Node` (öğe içinde `item.<alan>` bağlanabilir) |
| `Divider` | — |
| `Image` | `src: asset hash \| bind`, `alt: literal` (zorunlu), `width`, `height` |
| `Link` | `label`, `to: ekran adı` ya da `href: inert://… \| https://…`, `params?` |

**Form modeli:** `Input`, `Select` ve `Toggle`, bir aksiyonun bir alanına bağlanır. Aynı ekrandaki bir `Button` o aksiyonu tetiklediğinde, o aksiyona bağlı alanların değerleri ve butonun `args`'ı birleşip `action.fields` olur. Birleşik alan seti aksiyonun şemasıyla birebir eşleşmek zorundadır.

### 6.3 Slot tipleri

`string { max }`, `int { min, max }`, `decimal { scale, min, max }`, `bool`, `image` (dinamik görsel hash'i), `list { max, item: map<tstr, SlotType> }`.

### 6.4 Alan tipleri (aksiyon ve `open` parametreleri)

| Tip | Kural |
|---|---|
| `string { max }` | UTF-8, kontrol karakterleri yok, uzunluk ≤ `max`. Dilimde regex yok. |
| `int { min, max }` | i64 aralığı içinde. |
| `decimal { scale, min, max }` | Kabloda `[mantissa: int, scale: uint]`. `scale` tipteki değeri aşamaz. |
| `bool` | — |
| `enum { options: [{ value, label }] }` | Değer, manifest'teki seçeneklerden biri olmak zorunda. |
| `choice { from: slot }` | `from`, öğelerinde `id: string` ve `label: string` alanları olan bir `list` slot'u olmak zorunda. Değer, mevcut `Screen` verisinde o listedeki `id`'lerden biri olmalı (ör. kullanıcıya özel hesaplar). Sunucu ayrıca sahipliği kontrol eder. |

### 6.5 Tasarım token'ları

```
Design {
  colors:  { bg, fg, muted, accent, on_accent, line }    // sRGB hex
  fonts:   { display, title, body, mono, label } → "geist-serif" | "geist-sans" | "geist-mono" | "geist-pixel"
  sizes:   { display, title, body, mono, label }          // px
  spacing: { xs, s, m, l, xl }                            // px
  radius:  uint
}
```

Siteler font dosyası gönderemez. Sadece tarayıcıya gömülü dört Geist fontundan seçebilir.

### 6.6 Görseller

- **Statik:** Hash'leri `assets` içinde durur. Bayt ve boyutları manifest'te yazılıdır.
- **Dinamik:** Sadece `image` tipindeki slot'lar için geçerlidir. Değer bir hash'tir, görsel `get_asset` ile alınır. Manifest'te olmamaları normaldir.
- Her iki durumda da tarayıcı gelen baytların SHA-256'sını hash ile karşılaştırır, ardından §12 sınırları içinde decode eder.
- Formatlar: `image/png` ve `image/webp`. Decoder'lar bellek güvenli Rust crate'leridir.

## 7. Doğrulama kuralları (`inert-verify`)

### 7.1 Manifest (hem `inert-sign build` sırasında hem tarayıcıda)

- İmza geçerli ve `key_id`'nin anahtarına ait.
- `entry` ekranı var. Her `Button`, `Input`, `Select` ve `Toggle` var olan bir aksiyona ve alana bağlı. Her `Link.to` var olan bir ekrana bağlı.
- Her `bind` ekranda tanımlı bir slot'a, tipi de bileşenle uyumlu (ör. `Text` → `string`, `int` ya da `decimal`; `Image` → `image`).
- Bir aksiyonun alan seti, onu tetikleyen her ekrandaki form alanları + `args` ile birebir eşleşiyor.
- Her statik görsel hash'i `assets` içinde.
- Her `Image` için `alt` dolu.
- **Kontrast:** `fg/bg`, `muted/bg`, `accent/bg` ve `on_accent/accent` çiftlerinin WCAG kontrast oranı ≥ 4.5:1. Bu, hem erişilebilirlik hem de "görünmez metin" ile prompt injection'a karşı bir kuraldır.
- Tüm sayısal sınırlar §12 içinde.

### 7.2 `Screen` (tarayıcıda ve sunucuda gönderimden önce)

- `screen` adı manifest'te var.
- `data` tam olarak ekranın slot setini içeriyor: eksik yok, fazla yok.
- Her değer slot tipine ve sınırlarına uyuyor. Listeler `max`'ı aşmıyor ve her öğe öğe tipine uyuyor.

### 7.3 `Action` (tarayıcıda göndermeden önce, sunucuda handler'dan önce)

- `action` adı manifest'te var ve `screen` adlı ekrandan tetiklenebiliyor.
- `fields` tam olarak aksiyonun alan setini içeriyor ve her değer kendi tipine uyuyor.
- `choice` alanları, oturumun o ekran için en son gönderdiği `Screen` verisine göre kontrol edilir. Sunucu bu veriyi oturumda tutar.
- `id` 16 bayt. Aynı oturumda daha önce görülmüşse aksiyon işlenmez, önceki `result` tekrar gönderilir.

### 7.4 `Open` (sunucuda handler'dan önce)

- `screen` adı manifest'te var.
- `params` tam olarak ekranın `params` setini içeriyor ve her değer kendi alan tipine uyuyor (§6.4).

## 8. Güven modeli

- **Kanal:** TLS 1.3, `rustls`, post-quantum hibrit anahtar değişimi açık. Üretimde WebPKI sertifika doğrulaması yapılır.
- **Manifest anahtarı:** Ed25519. Üretim modelinde anahtar `_inert.<alan>` TXT kaydında yayınlanır ve ilk ziyarette pinlenir. **Dilimde DNS yok**, bu yol sadece arayüz olarak tanımlanır.
- **Güven deposu:** `~/.config/inert/trust.toml`

  ```toml
  [pins."bank.example"]
  key = "ed25519:<base64>"
  pinned_at = "2026-11-07T00:00:00Z"
  ```

- **Geliştirme modu:** `inert-browser --dev-trust <dosya>` ile verilen dosya, hem TLS sertifikasını (kendinden imzalı) hem manifest anahtarını içerir. Bu mod adres çubuğunda açıkça `DEV` olarak işaretlenir.
- **Anahtar değişimi:** Pinlenmiş anahtarla eşleşmeyen bir manifest gelirse sayfa açılmaz, tam ekran uyarı gösterilir. Kullanıcı açıkça onaylamadan yeni anahtar pinlenmez.
- **Sürüm geri alma koruması:** Tarayıcı her alan adı için gördüğü en yüksek `manifest.version`'ı saklar ve daha düşük sürümü reddeder.

## 9. Taşıma

- `Transport` trait'i: bağlan, çerçeve gönder, çerçeve al, kapat. Dilimdeki tek implementasyon TCP + `rustls`. QUIC ileride aynı trait'in arkasına gelecek.
- Async runtime: `tokio`, sadece `inert-transport`, `inert-server` ve tarayıcının ağ thread'inde.
- Zaman aşımları §12'de.

## 10. Sunucu tarafı

### 10.1 `inert-sign`

- `inert-sign keygen --out <dosya>`: Ed25519 anahtar çifti üretir. Anahtar dosyası repo dışında tutulur.
- `inert-sign build app.inert.json --key <dosya> --out manifest.signed.cbor`: JSON'u okur, §7.1 kurallarını uygular, deterministik CBOR üretir ve imzalar. Kurallardan biri bile başarısız olursa çıktı üretilmez.

### 10.2 Server SDK

```rust
let mut app = App::load("manifest.signed.cbor")?;

app.screen("overview", |session, params| {
    Ok(ScreenData::new()
        .set("balance", session.user.balance())
        .set("transactions", session.user.transactions()))
});

app.action("transfer", |session, action| {
    let amount = action.decimal("amount");   // zaten doğrulanmış
    let from = action.choice("from");
    session.user.transfer(from, amount)?;
    Ok(Next::screen("overview"))
});

app.serve("0.0.0.0:7443", tls_config).await
```

- **Sunucu imza anahtarını görmez**, sadece `manifest.signed.cbor`'u yükler.
- **Başlangıç kontrolü:** Manifest'teki her ekranın ve aksiyonun tam olarak bir handler'ı olmalı. Eksik ya da fazla varsa `serve` başlamaz.
- Handler'a sadece §7.3'ten geçmiş aksiyonlar ulaşır. Handler'ın döndürdüğü her `Screen` gönderilmeden önce §7.2'ye karşı kontrol edilir. Başarısız olursa istemciye `internal` hatası gider, ayrıntı loglanır.
- Handler'daki panikler yakalanır ve `internal` hatasına çevrilir.

### 10.3 `inert dev`

Tek komutla şunları yapar: kendinden imzalı bir TLS sertifikası ve geliştirme anahtarı üretir (yoksa), manifest'i imzalar, sunucuyu başlatır, `--dev-trust` dosyasını yazar ve `app.inert.json` değiştiğinde yeniden imzalayıp yükler. Geliştirme anahtarları `.gitignore`'dadır.

## 11. Tarayıcı (`inert-browser`)

### 11.1 Katmanlar

| Görev | Kullanılan |
|---|---|
| Pencere, girdi, IME | `winit` (`eframe` üzerinden) |
| GPU | `wgpu` |
| 2B çizim, metin, layout | `egui` 0.37 (`vello` + `parley` + `taffy` yerine, §17) |
| Erişilebilirlik | `accesskit` (egui'nin kendi entegrasyonu) |
| Görsel decode | Bellek güvenli `png` ve `image-webp` |
| Normal web | Servo 0.7 (ayrı süreç) ve WebKit (`wry`, WKWebView) |

### 11.2 Süreç ve thread modeli

- **Tarayıcı süreci:** `inert://` çekirdeği, renderer ve arayüz. Ağ ve protokol ayrı bir thread'de (`tokio`) çalışır, kanal üzerinden ana thread'e sadece doğrulanmış ekranlar iletilir. Çizim ve girdi ana thread'dedir. Servo bu sürece bağlanmaz, AI anahtarları ve pinler sadece buradadır.
- **Servo süreci (`inert-servo-host`):** İlk Servo sekmesi açıldığında başlatılır. Tarayıcı, stdin'e JSON satırı komutlar yazar (oluştur, yükle, göster, boyutlandır, girdi, geri/ileri, zoom, script, Shield). Süreç stdout'a olaylar yazar (yükleme, URL, başlık, geçmiş, imleç, engellenen istek, `inert://` yönlendirmesi). Aktif sekmenin kareleri iki yuvalı, bellek eşlemli bir dosyaya yazılır. Tarayıcı dosyayı salt okunur eşler ve kareyi kopyalar. Süreç ölürse sekmeleri düşürülür ve en fazla üç kez yeniden başlatılır, sonra WebKit'e geçilir. Tarayıcı kapanınca stdin kapanır ve süreç kendiliğinden çıkar.
- **WebKit süreçleri:** WKWebView kendi WebContent ve Networking süreçlerini macOS'un sandbox'ında açar.

### 11.3 Tarayıcı arayüzü

Adres çubuğu ve güven göstergesi aynı bileşen setiyle ve aynı renderer ile çizilir (Yön A dili, koyu). Gösterge durumları:

| Durum | Gösterge |
|---|---|
| İmza ve pin geçerli | `● SECURED` + anahtar parmak izi (ilk ve son 4 karakter) |
| Geliştirme modu | `● SECURED · DEV` |
| Anahtar değişmiş | Tam ekran uyarı, sayfa çizilmez |
| Protokol hatası | Hata ekranı + kod + "yeniden bağlan" |

`Link.href` başka bir `inert://` alanına gidiyorsa bir onay gösterilir. `https://` ve `http://` sayfaları `UNSECURED` (şifresiz bağlantıda `NOT ENCRYPTED`) rozetiyle açılır.

**Motor seçimi:**
1. Varsayılan motor Servo'dur (ayarlardan WebKit seçilebilir).
2. Inbox grubundaki sekmeler, "Inbox sekmeleri WebKit kullansın" ayarı açıkken (varsayılan) WebKit ile açılır, çünkü Gmail ve WhatsApp Web Servo'da tam çalışmıyor.
3. Her sayfa, site güvenliği panelinden diğer motorla yeniden açılabilir. Seçim sekme bazındadır.
4. Servo süreci başlatılamazsa ya da tekrar tekrar çökerse sekmeler WebKit ile açılır.
5. Servo'daki bir sayfa `inert://` linkine gittiğinde Servo gezinmeyi reddeder ve link güvenli motorda açılır.

### 11.4 Renderer

- Şablon + `Screen` verisi → `taffy` düğüm ağacı → layout → `vello` sahnesi.
- Fontlar gömülüdür: Geist Serif, Sans, Mono ve Pixel (SIL OFL).
- Sayılar dilimde yerelleştirme olmadan gösterilir: `int` olduğu gibi, `decimal` kendi `scale`'i kadar ondalık basamakla ve nokta ayırıcıyla.
- Erişilebilirlik ağacı aynı düğüm ağacından üretilir. Her `Image`'ın `alt`'ı, her form alanının `label`'ı vardır.
- Tarayıcı, gönderilmeden önce her `Action`'ı §7.3'e karşı kontrol eder ve hataları ilgili alanın altında gösterir.

## 12. Sınırlar (başlangıç değerleri)

| Sınır | Değer |
|---|---|
| Çerçeve boyutu: `welcome`, `asset` | 4 MiB |
| Çerçeve boyutu: diğer mesajlar | 256 KiB |
| CBOR iç içe geçme derinliği | 32 |
| Şablon ağacı derinliği | 16 |
| Ekran sayısı / aksiyon sayısı | 256 / 256 |
| `string` için mutlak üst sınır | 4096 karakter |
| `list` için mutlak üst sınır | 1000 öğe |
| Görsel boyutu | 2 MiB, en fazla 4096 × 4096 ve 16 megapiksel |
| El sıkışma zaman aşımı | 10 sn |
| Tek mesaj okuma zaman aşımı | 30 sn |
| Boşta bekleme zaman aşımı | 300 sn |
| Oturumda hatırlanan `action.id` | son 1024 |

Manifest bu mutlak sınırların altında kendi sınırlarını belirleyebilir, üstüne çıkamaz.

## 13. Hata yönetimi

### 13.1 Sınıflar

| Sınıf | Ne olur |
|---|---|
| **Ölümcül** (protokol ihlali) | `fatal` gönderilir, bağlantı kapanır, tarayıcı hata ekranı gösterir. Kısmi çizim yok. |
| **Düzeltilebilir** (aksiyon hatası) | `result { ok: false, error }` döner. Tarayıcı hatayı ilgili alanın altında gösterir. |
| **Güven** | Tam ekran uyarı. |
| **Ağ** | "Yeniden bağlan". Aksiyonlar otomatik tekrarlanmaz. |

### 13.2 Hata kodları

| Kod | Ad | Sınıf |
|---|---|---|
| 1 | `unsupported_version` | ölümcül |
| 2 | `frame_too_large` | ölümcül |
| 3 | `malformed_cbor` | ölümcül |
| 4 | `unknown_message` | ölümcül |
| 5 | `bad_signature` | ölümcül |
| 6 | `manifest_invalid` | ölümcül |
| 7 | `screen_invalid` | ölümcül |
| 8 | `asset_invalid` | ölümcül |
| 9 | `timeout` | ölümcül |
| 100 | `field_invalid` | düzeltilebilir |
| 101 | `action_unknown` | düzeltilebilir |
| 102 | `rejected` | düzeltilebilir (handler'ın iş kuralı reddi, ör. yetersiz bakiye) |
| 103 | `internal` | düzeltilebilir |

## 14. Örnek uygulamalar

| Uygulama | Ekranlar | Öne çıkan bileşenler |
|---|---|---|
| `bank` | Özet, Transfer, Ayarlar | `List` (işlemler), `Select` + `choice` (hesap), `Input` (tutar), `Toggle` (bildirim), `Image` (statik logo) |
| `shop` | Katalog, Ürün, Sepet, Ödeme | `Image` (dinamik ürün görseli), `Select` + `enum` (beden), `Input` (adet), `Link` |
| `notes` | Liste, Not | `Input` (başlık, metin), `Toggle` (tamamlandı), `Select` (kategori), `List` içinde `Button` + `args` (sil) |

Tüm veriler sahte ve bellekte. Gerçek bir servise bağlantı yok. Ürün görselleri repoda üretilmiş yer tutucu görsellerdir.

## 15. Test stratejisi

| Katman | Testler |
|---|---|
| `inert-proto` | `proptest` ile encode/decode gidiş-dönüş testleri. Deterministik olmayan CBOR, tag, float, tekrarlanan anahtar ve fazladan alan içeren girdilerin reddedildiğini gösteren testler. `cargo-fuzz` hedefi: decoder. |
| `inert-verify` | Her kural için unit testler. `cargo-fuzz` hedefleri: manifest doğrulama, `Screen` doğrulama, `Action` doğrulama. |
| Uyumluluk vektörleri | `spec/vectors/` altında geçerli ve geçersiz `manifest`, `screen` ve `action` örnekleri, her biri beklenen sonuç ve hata koduyla. Dilden bağımsızdır. |
| `inert-transport` | Localhost'ta gerçek TLS ile entegrasyon testleri, zaman aşımları dahil. |
| Uçtan uca | Ekransız bir test istemcisi (`inert-proto` + `inert-verify` + `inert-transport`) üç örneği de sürer. |
| `inert-browser` | Her bileşen için ekran dışı çizim + referans PNG karşılaştırması. Erişilebilirlik ağacı snapshot'ı. |

### 15.1 Saldırı senaryoları (her biri otomatik test)

1. Sunucu, şablonda olmayan bir şifre alanı içeren bir `Screen` gönderiyor → `screen_invalid`.
2. `Screen` verisinde fazladan bir anahtar var → `screen_invalid`.
3. Manifest'in tek bir baytı değiştirilmiş → `bad_signature`.
4. Sunucu farklı bir anahtarla imzalanmış manifest sunuyor → güven uyarısı, sayfa çizilmez.
5. Daha eski sürümlü bir manifest sunuluyor → reddedilir.
6. Düşük kontrastlı token'lar → `inert-sign build` başarısız olur.
7. Sınırı aşan ya da hash'i tutmayan görsel → `asset_invalid`.
8. Aynı `action.id` iki kez gönderiliyor → handler bir kez çalışır.
9. `choice` alanına ekranda olmayan bir hesap id'si yazılıyor → `field_invalid`.
10. Sınırı aşan çerçeve → `frame_too_large`, gövde okunmaz.

## 16. Başarı kriterleri

- `inert dev` ile çalışan üç örnek uygulama, `inert-browser` ile macOS'ta açılıyor ve tüm ekran akışları çalışıyor.
- §15.1'deki on saldırı senaryosunun hepsi geçiyor.
- Her fuzz hedefi yerelde 30 dakika çöküş olmadan çalışıyor.
- `inert-proto` ve `inert-verify` `unsafe` kod içermiyor ve I/O bağımlılığı yok.
- Banka örneği açıkken tarayıcının bellek kullanımı ölçülüp raporlanıyor. Hedef **tarayıcı süreci** için 150 MB'ın altı. Normal web motorlarının süreçleri (Servo süreci, WebKit süreçleri) ayrı ölçülür ve raporlanır. Bu hedef bir kabul şartı değil, ilk ölçüm (sonuçlar §18'de).

## 17. Plan aşamasında netleşecek noktalar

- ~~`vello` ile `parley`'in mevcut sürümlerinin dilim için yeterli olup olmadığı.~~ Karar: `egui` (§11.1).
- CBOR crate'inin seçimi (`minicbor` ya da benzeri) ve katı mod için ne kadar özel kod gerekeceği.
- WebP decode için bellek güvenli bir crate'in olgunluğu. Yetersizse dilimde sadece PNG.

---

## 18. Uygulama notları (2026-11-07, ilk implementasyon)

Spec'ten bilinçli sapmalar ve eklemeler. Her biri kodda tek bir sınırın arkasında duruyor, geri dönmek mümkün.

**Protokol eklemeleri (geriye uyumlu küçük alanlar):**
- `Manifest.name`: tarayıcıda gösterilen uygulama adı.
- `ScreenDef.title`: sekme başlığı ve erişilebilirlik için ekran adı.
- `Input` / `Select` / `Toggle` üzerinde isteğe bağlı `init`: form alanının başlangıç değeri (literal ya da üst seviye bir slot'a bind). Verifier, `init`'in alan tipine uyduğunu kontrol eder.
- Dev trust dosyası (`~/Library/Application Support/Inert/dev-trust/<host>.toml`) TLS dev CA'sını, manifest anahtarını ve bağlanılacak adresi birlikte taşır.

**Crate yapısı:** `inert-client` adında altıncı bir crate eklendi. Tarayıcı ile ekransız test istemcisi aynı oturum kodunu (pin, imza, şablon kontrolü, asset hash kontrolü) paylaşıyor.

**Hata davranışı:** Geçersiz bir `open` mesajı sunucuda `fatal screen_invalid` ile, ekran handler'ının hatası `fatal internal` ile sonlanır. Sunucu, şablona uymayan bir ekranı göndermeden önce kendisi yakalar ve `internal` döner.

**Tarayıcı çizim katmanı:** §11.1'deki `vello` + `parley` + `taffy` yerine `egui` + `eframe` (wgpu) kullanıldı (§17'nin izin verdiği alternatif). Erişilebilirlik (AccessKit), IME, metin düzenleme ve ekran okuyucu desteği kutudan çıkıyor. Tarayıcının kendi arayüzü ve `inert://` renderer'ı aynı katmanla çiziliyor.

**Normal web motoru (güncellendi, 2026-11-07 sabah): Servo varsayılan, WebKit seçenek.** Servo 0.7, `inert-servo-host` adında ayrı bir süreçte çalışıyor (§11.2). `SoftwareRenderingContext` ile ekran dışında çiziyor, kareler paylaşılan bellekten egui'de doku olarak gösteriliyor. Fare, tekerlek ve klavye girdisi Servo'ya iletiliyor. `inert://` linkleri Servo'dan güvenli motora devrediliyor. Inbox grubundaki sekmeler (Gmail, WhatsApp) uyumluluk için varsayılan olarak WebKit (WKWebView) ile açılıyor; bu bir ayar. Her web sayfası, site güvenliği panelinden diğer motorla yeniden açılabilir. Shield, Servo sekmelerinde Brave'in `adblock` motoruyla istekleri yakalayıp engelliyor; WebKit sekmelerinde WebKit'in kendi içerik engelleyicisi kullanılıyor.

**epaint yaması (kaldırıldı):** Servo tarayıcıya gömülüyken `vello_cpu`'nun çok iş parçacıklı özelliği Cargo'nun özellik birleştirmesiyle egui'ye de geçiyor ve glif rasterleştirmesini çökertiyordu. Geçici olarak `vendor/epaint` yaması kullanıldı. Servo ayrı bir süreç ve ayrı bir workspace'e taşınınca bağımlılık ağaçları ayrıldı ve yama silindi.

**Giriş:** "Sign in with Claude / GPT" ekranı sadece sağlayıcıyı soruyor. Üçüncü taraf uygulamalar için bu sağlayıcıların OAuth girişi kullanılmadığı için anahtar bir API key olarak alınıyor ve macOS Keychain'de saklanıyor. Anahtar atlanabilir.

**VPN:** Mullvad uygulamasının kendi WireGuard tüneli, resmi `mullvad` CLI'ı üzerinden yönetiliyor. Tünel tarayıcıya değil, bütün Mac'e uygulanıyor ve arayüzde de bu şekilde anlatılıyor. Hesap numarası hiçbir zaman arayüze okunmuyor.

**Reklam engelleme:** EasyList ve EasyPrivacy, Brave'in `adblock` crate'i ile WebKit content-blocker kurallarına çevriliyor (yaklaşık 131 bin kural), WebKit'te bir kez derleniyor ve sonraki açılışlarda önbellekten kullanılıyor.

**Ölçümler (release, macOS, 2026-11-07 sabah, Servo ayrı süreçte):**

Ana ölçü fiziksel ayak izi (`footprint`), yani Activity Monitor'daki "Bellek". RSS ayrıca veriliyor, çünkü macOS'un paylaşılan framework sayfalarını da sayıyor. `scripts/ui-test.sh` 49 kontrolü koşarken iki süreci de sürekli örnekliyor (`docs/test-results/memory-ui-driver.csv`).

| Durum | Medyan | En fazla |
|---|---|---|
| **Sadece banka açık**, pencere sürekli çiziliyor, 30 sn (`scripts/memory-bank.sh`, §16 kriteri) | **73–74 MB** | **74 MB** |
| 49 kontrollük UI turu, tarayıcı süreci, ayak izi (4 koşu) | **80–95 MB** | **98–105 MB** |
| Tarayıcı süreci, RSS | 126 MB | 140 MB |
| Servo süreci, ayak izi | 131 MB | 177 MB |
| Tarayıcı, testin ekran görüntüleri açıkken (`SHOTS=1`) | 139 MB | 160 MB |

- §16: Tarayıcı süreci tüm tur boyunca 150 MB hedefinin altında. Normal web motorunun belleği kendi sürecinde.
- Binary: `inert-browser` 19 MB, `inert-servo-host` 95 MB.
- **Nereden geldi:** İlk ölçümde tarayıcı tur boyunca medyan 187 MB'tı. Üç neden vardı:
  1. Servo aynı süreçteydi. Ayrı sürece taşındı.
  2. macOS'un bellek ayırıcısı, serbest bırakılan büyük blokları (sayfa kareleri, çözülen görseller) kendi önbelleğinde kirli tutuyordu. Bu yaklaşık 40 MB ediyordu. Önbellek sadece süreç başlarken kapatılabildiği için tarayıcı ilk açılışta `MallocLargeCache=0` ile kendini bir kez yeniden çalıştırıyor (aynı PID, birkaç ms). Servo süreci ayarı devralıyor.
  3. Testin uygulama içi ekran görüntüleri, egui-wgpu'nun yakalama dokusu ve okuma tamponlarıyla yaklaşık 36 MB GPU belleği tutuyordu. Bu testin kendi yükü, tarayıcıya ait değil. Ekran görüntüleri artık isteğe bağlı (`SHOTS=1`).
- **Geçersiz ölçüm, değiştirildi:** Önceki "banka açıkken 70 MB" ölçümü ekran kilitliyken yapıldı. Ekran kilitliyken normal açılan bir pencere hiç çizilmiyor (10 saniyede 0 kare), yani o ölçüm çizim belleğini içermiyordu. Yeni banka ölçümü UI sürücüsüyle yapılıyor. Sürücü pencereyi saniyede yaklaşık 33 kez çizdiriyor, kare sayacı da bunu doğruluyor: 30 saniyelik beklemede 1563'ten 2427'ye çıktı.
- Önceki ölçüm (Servo tarayıcıya gömülüyken, RSS): tur boyunca medyan 167, en fazla 281 MB, binary 108 MB.
- **Çökme izolasyonu:** Servo süreci `kill -9` ile öldürüldüğünde tarayıcı açık kalıyor, Servo sekmeleri düşüyor ve yeni bir Servo süreci başlıyor (elle denendi, 2026-11-07).

**Tasarım kontrolü (Yön A):** `theme.rs` token'ları Claude Design'daki Yön A dosyasıyla birebir aynı:
- Zemin #000000, metin #EDEDED, başlık #FFFFFF, ikincil #9A9A9A ve #8A8A8A.
- Çizgiler #222222 ve #3A3A3A. Vurgular #B48CFF, #7F9CFF, #B9C6FF.
- Etiketler Geist Mono, büyük harf, geniş harf aralığı. Köşeler 2 px.

Eklenenler:
- Yükseltilmiş yüzeyler: #080808, #101010, #181818.
- Durum renkleri ve grup renkleri.

`scripts/design-check.py` ekran görüntülerinin piksellerini bu palete karşı sayıyor. Tarayıcı arayüzündeki düz renklerin %85–99'u paletten. Gerisi grup renklerinin soluk tonları ve dither çizimi (`docs/test-results/design-check.json`).

**UI testleri:** `crates/inert-browser/src/drive.rs`, egui'nin girdi akışına gerçek fare ve klavye olayları enjekte ediyor (`App::raw_input_hook`). 49 kontrol var: giriş, omnibox tamamlama, banka transferi (limit dışı tutar dahil), mağaza sepeti, not ekleme ve kaydetme, yer imleri, sekme grupları, sağ tık menüsü, kısayollar, Servo'da tıklama ve yazma, motor değiştirme, Inbox'ın WebKit'te açılması, VPN, Shield, asistan, geçmiş. Hepsi geçiyor (`docs/test-results/ui-driver.json`), Servo ayrı süreçteyken tekrar koşuldu. Bu testler 4 gerçek hata buldu ve düzeltildi.

**Fuzzing (2026-11-07):** Dört hedef (`cbor_decode`, `message_decode`, `manifest_verify`, `screen_action_verify`) paralel olarak her biri 30 dakika çalıştı (§16). Toplam yaklaşık 213 milyon girdi denendi, hiç çökme ya da artifact yok. İlk 10 dakikalık koşu (78 milyon girdi) da temizdi.

### 18.1 Sabah turu (2026-11-07)

Kullanıcının dokuz maddelik listesi. Her madde UI sürücüsünde bir kontrolle doğrulanıyor (71/71, `docs/test-results/ui-driver.json`).

| # | İstek | Ne yapıldı |
|---|---|---|
| 1 | Sekme ve arama çubuğu yayvan, animasyon, Chrome gibi | Sekmeler 36 px, Chrome'un şekli (yuvarlak üst köşe, alt kıvrımlar, aktif sekme araç çubuğuna akıyor). Yeni sekme genişleyerek açılıyor, kapanan sekme yerinde daralıyor, hover ve genişlik değişimleri yumuşak. Omnibox hap biçiminde, odakta vurgu halkası. Açılır liste ve paneller kayarak ve solarak açılıyor. |
| 2 | Linkler adres çubuğunda ortada değil, üstte | Metin alanı tam bir satır yüksekliğinde ve iç boşluksuz, hapın tam ortasında. Odak dışında host beyaz, gerisi soluk. |
| 3 | Grup açılıp kapanırken efekt, daha yuvarlak | Grubun sekmeleri 0,24 sn'de açılıp kapanıyor. Etiket hap biçiminde, kapalıyken dolu (Chrome gibi). Genel köşe yarıçapları 8 (bileşen), 12 (menü), 14 (panel). |
| 4 | Yeni sekme arama kutusu üstteki bara yazıyor | Kutu artık gerçek bir alan: kendi satır içi tamamlaması, kendi önerileri, ok tuşları ve Enter. Üstteki bar dokunulmadan kalıyor. |
| 5 | DuckDuckGo varsayılan karanlık | Arama ve duckduckgo.com adresleri `kae=d` ile açılıyor. Pencere koyu, WebKit sayfaları `prefers-color-scheme: dark` görüyor, Servo görünümlerine `Theme::Dark` bildiriliyor. |
| 6 | Geçmiş | Her ziyaret kaydediliyor (en fazla 20 000). Gün gün kartlar, saatler, arama, satır silme, "Recently closed", zaman aralıklı "Clear browsing data" (isteğe bağlı çerezlerle). |
| 7 | Çerez yönetimi eklentisi | Yerleşik "Cookies" eklentisi. Bulunulan sitenin çerezleri (değerler gösterilmez), tek tek veya site bazında silme, "bu sitede çerez tutma" listesi, çerez bantlarını otomatik reddetme (OneTrust, Cookiebot, Didomi, … ve metinden "Reject all / Only necessary"). WebKit'te `WKHTTPCookieStore`, Servo'da yardımcı süreçteki `SiteDataManager`. Ayarlarda tüm siteler. |
| 8 | Mullvad: başlıktaki tik, çıkış, kurulum | Simge etiketle aynı satırda. Kurulum adımları, sadece hesap numarasıyla giriş (16 hane kontrolü, numara arayüzde ve profilde tutulmuyor), onaylı çıkış. Kullanıcının isteğiyle gerçek hesaptan çıkış yapıldı, panel gerçek "Sign in to Mullvad" durumunu gösteriyor. Kurulum ekranı gerçek modda (demo değil), gerçek algılama koduyla da gösterildi: `INERT_MULLVAD_BIN` var olmayan bir yolu gösterince tarayıcı Mullvad'ı kurulu saymıyor (`docs/screenshots/43`); Mullvad kaldırılmadı. Kurulum akışının dört adımı `docs/screenshots/37…40` (`INERT_DRIVE_SCENARIO=vpn`, `INERT_DEMO_VPN=not-installed` / `logged-out`). |
| 9 | Chrome'dan geçişte yer imleri, hesaplar, alışkanlıklar | Girişten sonra 2. adım (atlanabilir) ve `about:import`. Chrome, Brave, Edge, Arc, Vivaldi, Chromium profilleri bulunuyor. Yer imleri, geçmiş ve ziyaretler, kayıtlı şifreler (macOS Keychain'e, adres çubuğundaki anahtarla doldurma), çerezler (oturumlar açık kalıyor), arama motoru, yer imi çubuğu, açılış sayfaları. SQLite dosyaları kopyalanıp sistemin `sqlite3` aracıyla ayrı süreçte okunuyor. Şifre ve çerez anahtarı "Chrome Safe Storage"; macOS önce kullanıcıya soruyor. |

- Bu Mac'te Chrome kurulu değil. İçe aktarıcı, Chrome'un dosya biçimlerinde (aynı tablolar, aynı `v10` şifreleme, sürüm 24 çerez öneki) sahte bir profille test ediliyor (`import.rs` birim testi ve demo profili). Gerçek bir Chrome profiliyle denenmedi. Çerez sorgusu bu Mac'teki beş gerçek Chromium çerez veritabanında (VS Code, Obsidian, Linear, Arduino IDE, Logi Options+; şema sürümleri 24 ve 21) `LIMIT 0` ile koşuldu ve geçti, hiçbir değer okunmadı (`import::tests::real_chromium_cookie_schema`, `INERT_REAL_COOKIES` ile, varsayılan olarak kapalı). **Gerçek Chrome ile:** Kurulu Google Chrome 156, geçici bir profil ve `--use-mock-keychain` ile çalıştırıldı (kullanıcının Keychain'i ve verisi kullanılmadı). Chrome kendisi iki sayfa ziyaretini, bir çerezi, `chrome://bookmarks` üzerinden bir yer imini ve `chrome://password-manager` üzerinden bir şifreyi yazdı. İçe aktarıcı hepsini aldı: yer imi, geçmiş (ziyaret sayılarıyla), 3 ziyaret, şifre ve çerez (çözülmüş, sürüm 24 öneki atılmış). Yanlış anahtarla şifre ve çerez çıkmıyor. Aynı profil tarayıcının giriş ekranından da içe aktarıldı (7/7). Tekrar: `scripts/real-chrome-import.sh`. Kullanıcının kendi Chrome profiliyle (gerçek Keychain izni dahil) henüz denenmedi.
- Mullvad girişi gerçek bir hesapla denenmedi (numara kullanıcıda). Çıkış ve oturum kapalı durumun okunması gerçek CLI ile doğrulandı.
- Bellek, 71 kontrollük turda: tarayıcı medyan 97 MB, en fazla 109 MB; Servo süreci medyan 56 MB, en fazla 185 MB.

### 18.2 Glyph arayüzü (2026-11-10)

Tarayıcı arayüzü baştan tasarlandı. Kaynaklar: glyphblue (ana tema), Dia ve Facet (düzen, butonlar, hareket).

| Parça | Karar |
|---|---|
| Çerçeve | Pencerenin tamamı kobalt mavi (`#1F4DFF`). Üzerinde Geist Mono ile çizilen ASCII glif haritası; imlecin altında yükselir ve harfleri değişir. Doku tutulmaz, satırlar metin olarak çizilir (`art.rs`). |
| Sekmeler | Sol kenar çubuğu (252 pt). Sabitlenen sekmeler cam karolar, gruplar renkli çipli klasör satırları, sekmeler satır. Cmd+S ile gizlenir. Altta VPN hapı, geçmiş, ayarlar, asistan. |
| Sayfa | İçe gömülü, 14 pt yuvarlak koyu kart (`#090B12`). WebKit görünümleri katman yarıçapıyla, egui içeriği köşe maskeleriyle yuvarlanır. |
| Adres çubuğu | Mavi çerçevede lacivert cam hap, ortalanmış. Sayfa yüklenirken alt kenarında ışık gezer. |
| Butonlar | Düz yazılı, hap biçimli (`theme::cta`). Mono büyük harf yalnızca küçük etiketlerde. |
| Yeni sekme | Glif kartı (kahraman) ve altına taşan arama kutusu: alan, arama motoru çipi, gönder düğmesi. |
| Giriş | Tüm pencere glif kartı. Yazı gerektiren adımlar koyu panelde. |
| Hareket | Üzerine gelme ve basma geçişleri, sekme satırlarının açılıp katlanması, grup oku dönüşü, kenar çubuğu kayması, açılır panellerde belirme ve kayma, sekme değişiminde sayfa belirmesi. |
| macOS | Trafik ışıkları araç çubuğu hizasına taşınır (`web::mac::traffic_lights`). |

Notlar:
- Kenar çubuğu satırları sabit kimlik taşır. Otomatik kimlikler, üstteki satırlar katlanınca kayıyor ve açık sağ tık menüsünü başka sekmeye bağlıyordu.
- Yukarı açılan panel (VPN) içeriği değişince aynı kare yeniden yerleştirilir (`request_discard`), panel zıplamaz.
- Demo siteleri yeni dile uyduruldu (yarıçap 14, sans başlıklar); her site kendi vurgu rengini korur.
- Bellek: yalnız banka açıkken ayak izi 134 MB ölçüldü (önceki sürüm aynı ekranda 60 MB). Glif haritası bunun ~4 MB'ı; kalan artışın nedeni bulunmadı. Açık iş.
- Doğrulama: son tam UI turu 68/75 (kalan 7 hata VPN çıkış adımı ve ona bağlı adımlar; düzeltmesi yazıldı, tam turla doğrulanmadı). `scripts/design-check.py` hâlâ Yön A paletini sayıyor, güncellenmedi.
- Ekran görüntüleri: `docs/screenshots/44…49`.

### 18.3 Kurulum, asistan ve sayfa araçları (2026-11-11)

| Parça | Karar |
|---|---|
| Kurulum | Üç adım, ilerleme çubuğuyla: görünüm (Mavi/Koyu, renk, sekmeler kenarda ya da üstte) → model (API key) → Chrome'dan aktarım. Şifreler okunurken "Unlock your data" bekleme ekranı. |
| Görünüm | `Settings.dark`, `accent` (8 renk, `theme::ACCENTS`), `tabs_top`. Koyu görünümde çerçeve siyaha yakın, glifler seçilen renkte. |
| Logo | Halka içinde ortalanmış I, marka mavisi üzerinde beyaz glifler. Uygulama simgesini tarayıcı kendisi üretir (`--write-icon`). `scripts/bundle.sh` Inert.app'i kurar. |
| Modeller | Varsayılan `claude-sonnet-5-5` ve `gpt-6.1-sol`. Giriş API key ile: Anthropic üçüncü taraf uygulamalara claude.ai girişi ya da abonelik limiti sunma izni vermiyor. |
| Chrome aktarımı | `Bookmarks` ve `AccountBookmarks` birlikte okunur; site simgeleri Chrome'un `Favicons` veritabanından gelir. macOS veri klasörünü koruyorsa Tam Disk Erişimi adımları gösterilir. Yalnız-host çerezleri Domain özniteliği olmadan yazılır (aksi `__Host-` çerezlerini düşürüyor, GitHub yarım oturum görüyordu). |
| Site simgeleri | `favicon.rs`: host başına PNG, profil klasöründe. Kaynak: Chrome önbelleği ve ziyaret sonrası indirme. Yer imleri kendi başına ağ isteği başlatmaz. |
| Sayfa menüsü | Servo'nun bağlam menüsü tarayıcıda çizilir: kopyala, kes, yapıştır, tümünü seç, bağlantı ve görsel satırları, sayfayı çevir (Google Translate), WebKit ile incele, asistana ekle. Cmd+C/V/X/A/Z kendi Edit menü öğelerimizle egui alanlarına, Servo'ya ve WebKit'e yönlenir. |
| Asistan | Yanıtlar Markdown olarak çizilir (`md.rs`). Panel kenarda, sayfanın üstünde yüzer ya da kendi sekmesinde (`about:chat`). Kırpma aracı sayfadan seçilen bölgeyi görsel olarak, "Attach to assistant" seçili metni ekler; ikisi de modele veri olarak gider. Yeni sekmede açık sekmeler ve son sayfalar (yalnız başlık ve adres) bağlam olur. Alt+Enter adres çubuğundaki metni asistana sorar. |
| Shield | uBlock Origin'in varsayılan listeleri: EasyList, EasyPrivacy ve uBlock filtreleri. WebKit tek kural listesinde 140 bin kuralla sınırlı; Servo tarafı tümünü kullanır. |
| Bas-konuş | Option+Space basılıyken `inert-dictate` (Swift, Apple Speech; dil destekliyorsa cihaz üzerinde) dinler, bırakınca metin klavyenin olduğu yere yazılır. |

Yapılmayanlar: Chrome eklentilerini içe aktarma (Inert eklenti çalıştırmaz), Gmail gibi uygulama bağlantıları, passkey (WebKit'te Apple'ın tarayıcılara verdiği yetki gerekir; Servo'da WebAuthn yok), WebKit sayfalarında kırpma ve yüzen asistan. Bu turdaki değişikliklerden sonra tam UI testi koşturulmadı; kısa görsel tur 2/2.

### 18.4 Varsayılan web motoru: WebKit (2026-11-11)

Gündelik kullanımda Servo yetmedi: Google basit sürümünü gönderiyor, GitHub oturumu takılıyor, sitelerin beklediği bazı web özellikleri yok. Varsayılan WebKit oldu; Servo Ayarlar'da seçenek olarak duruyor ve her sayfa güvenlik panelinden Servo ile açılabiliyor. Servo varsayılanken oluşturulan profiller bir kez WebKit'e geçer (`engine_v2`). Demo profili ve UI testleri Servo'da kalır.

WebKit sayfalarında bilinen eksikler: kırpma aracı ve yüzen asistan çalışmaz (yerel görünümün üstüne çizilemiyor), sağ tık menüsü WebKit'in kendi menüsüdür ("Attach to assistant" ve çeviri ⋮ menüsünde). Passkey iki motorda da yok.

Aynı gün: Servo'ya ekran ölçeği iletiliyor (Retina'da sayfalar yarı boyutta çiziliyordu); Shield sol altta VPN'in yanında, "bu sitede" ve "her yerde" anahtarlarıyla (`adblock_off`); sekmeler üstteyken adres çubuğu başlık boyunca uzanır; adı silinen grup yalnız ikonuyla görünür; kurulum sırası açılış → model → görünüm → izinler ve aktarım.

### 18.5 Gizli sekmeler, sayfa araçları ve asistan eklemeleri (2026-11-11)

| Özellik | Nasıl çalışıyor | Sınırı |
|---|---|---|
| Gizli sekme (Shift+Cmd+N) | WebKit görünümü, kendi geçici veri deposuyla; diske yazılmaz, başka sekmeyle paylaşılmaz. Geçmişe, Rewind'e, oturuma girmez; site simgesi istenmez. | Yalnız WebKit. Servo tek çerez kavanozu kullandığı için gizli sekmede kullanılmaz. |
| Her sekme ayrı | Her gizli sekmenin deposu ayrıdır. | Normal sekmeler ortak depoyu kullanır. |
| Kendini siler | Boşta kalma süresi dolunca gizli sekmeler kapanır (ayar: asla, 5, 15, 30, 60 dk). | Süre, tarayıcıdaki son girdiye göre ölçülür. |
| İndirmeler kaybolur | Gizli sekmeden inen dosya, sekme kapanınca silinir; İndirilenler'de "Keep" ile tutulur. | — |
| Pano temizlenir | İlk gizli sekme açılırken panonun sayacı alınır; son gizli sekme kapanırken değişmişse pano boşaltılır. | Kopyalamanın hangi uygulamada yapıldığına bakılmaz. |
| Panik tuşu (Ctrl+Opt+Cmd+H) | Tüm gizli sekmeleri kapatır, pencereyi simge durumuna alır. | — |
| Konumumu gizle | Sayfa betiği: konum isteği her zaman "reddedildi" yanıtı alır. | Ağ adresinden çıkan konumu VPN gizler. Betik düzeyinde bir engel. |
| Kamera ve mikrofon | wry izin işleyicisi "sor" döndürür; WebKit her seferinde sorar. | Gerçek bir sitede denenmedi. |
| Her zaman gizli siteler | Listedeki site (eTLD+1) normal sekmede açılmaz, gizli sekmeye yönlenir. Ayarlar'dan ya da kilit panelinden eklenir. | — |
| Gizli favoriler | Anahtar Zinciri'nde tutulur, yer imleri sayfasında Touch ID'den sonra görünür, gizli sekmede açılır. 5 dakikada yeniden kilitlenir. | Touch ID gerçek cihazda denenmedi (demo profili onay verir). |
| Touch ID kilidi | Ayar açıksa Inert arka plana geçince gizli sekmeler örtülür; Touch ID açar. Gizli sekme görünürken pencere ekran görüntüsüne ve paylaşımına kapatılır (`sharingType`). | Aynı. |
| Bölünmüş görünüm | Etkin sayfa solda, ikinci sayfa sağda; başlıkta yer değiştir ve kapat. | Sağ bölme WebKit ve inert:// sayfalarını gösterir; Servo ve Inert'in kendi sayfaları solda kalır. |
| Rewind | Ziyaret edilen sayfanın metni (6000 karakter, en çok 1500 sayfa) `rewind.json` içinde bu Mac'te tutulur; Geçmiş'teki arama sayfa içlerinde de arar. | Gizli sekmeler hatırlanmaz. |
| Tüm sekmelere sor | Asistan kutusundaki "All tabs" ya da yeni sekmedeki hazır sorular: açık her sayfanın metni toplanır (4 sn sınır), tek soruyla modele gider. | Uyuyan sekmeler yalnız başlık ve adresle katılır. |
| Özetle | Menü ve sağ tık: sayfa metni modele gider. | Video özeti yok. |
| Yerinde çeviri | Sayfanın metin düğümleri (en çok 260 parça, 14 bin karakter) kullanıcının modeliyle çevrilir ve yerine yazılır. Anahtar yoksa Google Translate sekmesi açılır. | Uzun sayfalarda yalnız ilk kısım çevrilir. |
| Anında yanıt | Adres çubuğunda toplama-çarpma burada hesaplanır; kısa bilgiler, web önerileri açıksa DuckDuckGo'nun anında yanıt servisinden gelir. | — |
| Sesli oku | Seçim, yoksa makale metni macOS `say` ile okunur; tekrar basınca durur. | Sistem sesi ve dili. |
| Resim içinde resim | Sayfadaki en büyük video için WebKit'in PiP kipi. | Gerçek sitede denenmedi. |
| Altyazı | Videonun kendi altyazı izlerini (ve YouTube'un altyazı düğmesini) açar. | Canlı konuşma tanıma değil: altyazısı olmayan videoda bir şey yapmaz. |
| Sakin kip | YouTube, X, Instagram ve TikTok'ta Shorts, akış ve öneri bölümlerini gizleyen stil. | Sitelerin kendi işaretlemesine bağlı; değişirse güncellenmeli. |

Aynı tur: adres çubuğundaki güvenlik göstergesi yalnız renkli kilit (yeşil: doğrulanmış inert://, sarı: yerel geliştirme anahtarı ya da doğrulanıyor, turuncu: sıradan HTTPS sayfası, kırmızı açık kilit: şifresiz ya da doğrulama başarısız); ayrıntı tıklayınca açılır. Sayfa kaydırma sınırını aşınca açılan boşluk `overscroll-behavior: none` ile kapatıldı. Yer imi yokken yer imi çubuğu yer kaplamaz. İçe aktarma kaynaklarına Dia eklendi.

İçe aktarma ve izinler: uygulama kalıcı bir imzayla imzalanmadığı için macOS, her yeni derlemede Tam Disk Erişimi iznini unutuyor. Anahtar Zinciri'nde "Inert Dev" adlı bir kod imzalama sertifikası varsa `scripts/bundle.sh` onu kullanır ve izin kalıcı olur.

Doğrulama: UI testi 80/80 (demo profili, Servo). Yeni özelliklerden gizli sekme, anında yanıt, bölünmüş görünüm, gizli favoriler ve Shield paneli testte; gerisi gerçek sitelerde elle denenmedi.

### 18.6 Logo, düzen ayarları ve gerçek site doğrulaması (2026-11-11)

- **Logo:** "Held caret" seçildi: iki köşeli parantez arasında duran metin imleci (çalışmayan kod). `art::mark`, `art::logo` ve uygulama simgesi (`art::icon_image`) bunu çiziyor.
- **Ayarlar kategorili:** General, AI, Search & pages, Privacy, Shield & cookies, Passwords & import, VPN, About. Bir seferde bir kategori.
- **Kenar çubuğu genişliği** kenarından sürüklenerek ayarlanır (200–440 pt, çift tıkla varsayılan), `sidebar_w`.
- **Sekmeler üstteyken** trafik ışıklarından sonra 14 pt fazladan boşluk.
- **Menü açılınca sayfanın kararması:** WebKit görünümü yerel olduğu için Inert'in kendi çizimi üstüne çıkamıyor; menü açılırken sayfanın görüntüsü alınıp (`takeSnapshotWithConfiguration`) görünüm gizlenince yerine çiziliyor.
- **Shield:** gerçek bir reklam betiği (`pagead2.googlesyndication.com`) WebKit'te engelleniyor (ölçüldü). YouTube video reklamları videolarla aynı sunuculardan geldiği için ağ kuralıyla engellenemiyor; Shield açıkken YouTube'da reklamlar başlar başlamaz atlanıyor ve reklam alanları gizleniyor (`page_js::YT_ADS`).
- **Chrome aktarımı teşhisi** (`--import-check`, yalnız sayılar): Chrome klasörünü macOS engelliyor; Dia okunabiliyor (32 yer imi, 4989 geçmiş satırı). Aktarma ekranı artık engellenen tarayıcıyı, okunabilenlerin yanında Tam Disk Erişimi adımlarıyla gösteriyor.
- **Gerçek site turu** (`INERT_DRIVE_SCENARIO=real-web`, `INERT_PROFILE_DIR` ile geçici profil, demo değil, WebKit): sayfa yükleme, kaydırma boşluğu, konum gizleme, Rewind, geçmiş, yerinde çeviri (topla/geri yaz), sesli oku metni, site simgesi, GitHub, menü altında sayfa görüntüsü, Shield, YouTube'da sakin kip, resim içinde resim, altyazı, gizli sekmeler (geçmiş yok, birbirinden ve normal sekmelerden ayrı depo, boşta silinme, indirme silinmesi), her zaman gizli site, iki gerçek sayfayla bölünmüş görünüm, web'den anında yanıt, panik tuşu. Bu tur bir hata buldu: gizli sekmenin başlığı geçmişe yazılıyordu (düzeltildi).
- **Bilinen:** gece ekran kilitlenince macOS pencereyi örtülü sayıp kare üretmeyi durduruyor; testler `caffeinate -dimu` ile koşuyor. Doğrudan açılan bir video ya da görsel için WebKit "yüklendi" olayı göndermiyor; etkin sekme bir süre yükleniyor kalırsa sayfanın `document.readyState` değeri sorulup yükleme kapatılıyor (gerçek site turunda doğrulandı, 39/39).
- **Uygulamanın kendi izinleriyle uçtan uca denetim** (`open -a Inert.app --args …`, yalnız sayılar yazılır): `--touchid-check` bu Mac'te Touch ID'nin kurulu ve sorulabilir olduğunu gösterdi (istem gösterilmedi; parmakla onay kullanıcıda). `--import-run` okunabilen ilk tarayıcıdan (Dia) geçici bir profile 31 yer imi ve 4989 geçmiş satırı aktardı, kaydedilen profilde 38 yer imi (7 hazır yer imiyle). Geçici profil silindi. Chrome, macOS Tam Disk Erişimi verilene kadar okunamıyor.

