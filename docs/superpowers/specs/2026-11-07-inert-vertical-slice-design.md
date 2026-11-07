# Inert — Dikey Dilim Tasarımı

- **Tarih:** 2026-11-07
- **Durum:** Taslak, onay bekliyor
- **Kapsam:** Alt proje 1–3'ün en küçük uçtan uca hâli (protokol + çekirdek, server SDK, tarayıcı)
- **Bağlam:** Projenin genel kararları `CLAUDE.md` dosyasında. Bu belge sadece dikey dilimi tanımlar.

---

## 1. Amaç

Inert fikrinin uçtan uca çalıştığını kanıtlamak:

1. Küçük bir sunucu, imzalı bir App Manifest ve buna uyan `Screen` verisi gönderir.
2. Rust ile yazılmış tarayıcı, manifest imzasını pinlenmiş anahtara karşı doğrular, her `Screen`'i manifest'teki şablona karşı doğrular ve GPU ile çizer.
3. Kullanıcı bir aksiyon tetiklediğinde tipli bir `Action` gider, sunucu bunu tekrar doğrular, handler çalışır ve yeni bir `Screen` döner.
4. Projenin vaadi olan saldırı senaryoları otomatik testlerle reddedilir.

## 2. Kapsam dışı

Servo ve normal web, AI paneli ve kullanıcı uyarlama AI'ı, agent toolkit ve MCP istemcisi, "Sign in with GPT / Claude", gerçek DNS kaydı ile anahtar keşfi, şeffaflık kaydı, QUIC, uygulama içi giriş ekranı ve hesaplar, web sitesi, dağıtım paketleri.

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

Tek bir Cargo workspace:

| Crate | Görevi | Bağımlılık |
|---|---|---|
| `inert-proto` | Mesaj ve manifest tipleri, katı CBOR encode/decode, sınırlar. | — |
| `inert-verify` | İmza kontrolü (Ed25519), manifest kuralları, `Screen` ↔ şablon ve `Action` ↔ şema doğrulaması, kontrast kontrolü. | `inert-proto` |
| `inert-transport` | `Transport` trait'i, TCP + `rustls` implementasyonu, çerçeveleme, zaman aşımları. | `inert-proto` |
| `inert-server` | Server SDK, `inert-sign` ve `inert dev` komutları. | proto, verify, transport |
| `inert-browser` | Pencere, çizim, layout, 10 bileşen, tarayıcı arayüzü, güven deposu. | proto, verify, transport |
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

| Görev | Aday |
|---|---|
| Pencere, girdi, IME | `winit` |
| GPU | `wgpu` |
| 2B çizim | `vello` (1.0 öncesi, bir arayüzün arkasında) |
| Metin yerleşimi ve düzenleme | `parley` |
| Layout | `taffy` |
| Erişilebilirlik | `accesskit` + `accesskit_winit` |
| Görsel decode | Bellek güvenli `png` ve WebP crate'leri |

### 11.2 Süreç ve thread modeli

Tek süreç. Ağ ve protokol ayrı bir thread'de (`tokio`) çalışır, kanal üzerinden ana thread'e sadece doğrulanmış ekranlar iletilir. Çizim ve girdi ana thread'dedir.

### 11.3 Tarayıcı arayüzü

Adres çubuğu ve güven göstergesi aynı bileşen setiyle ve aynı renderer ile çizilir (Yön A dili, koyu). Gösterge durumları:

| Durum | Gösterge |
|---|---|
| İmza ve pin geçerli | `● SECURED` + anahtar parmak izi (ilk ve son 4 karakter) |
| Geliştirme modu | `● SECURED · DEV` |
| Anahtar değişmiş | Tam ekran uyarı, sayfa çizilmez |
| Protokol hatası | Hata ekranı + kod + "yeniden bağlan" |

`Link.href` başka bir `inert://` alanına gidiyorsa bir onay gösterilir. `https://` için dilimde sadece "unsecured, desteklenmiyor" mesajı çıkar (Servo dilimde yok).

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
- Banka örneği açıkken tarayıcının bellek kullanımı ölçülüp raporlanıyor. Hedef 150 MB'ın altı, ama bu bir kabul şartı değil, ilk ölçüm.

## 17. Plan aşamasında netleşecek noktalar

- `vello` ile `parley`'in mevcut sürümlerinin dilim için yeterli olup olmadığı. Değilse aynı arayüzün arkasında alternatif bir renderer.
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

**Normal web motoru: Servo yerine sistem WebKit'i (WKWebView, `wry` ile).** Gerekçe: Inbox grubunun amacı Gmail ve WhatsApp Web gibi uygulamaları çalıştırmak, bunlar Servo 0.7'de çalışmıyor. Servo'yu derlemek de SpiderMonkey yüzünden ağır. Web içeriği WebKit'in kendi sandbox'lı süreçlerinde çalışıyor ve Inert çekirdeğine, pin'lere ya da AI anahtarlarına erişemiyor. Motor `web.rs`'in arkasında. Servo, uyumluluğu yeterli olduğunda aynı arayüzün arkasına eklenebilir.

**Giriş:** "Sign in with Claude / GPT" ekranı sadece sağlayıcıyı soruyor. Üçüncü taraf uygulamalar için bu sağlayıcıların OAuth girişi kullanılmadığı için anahtar bir API key olarak alınıyor ve macOS Keychain'de saklanıyor. Anahtar atlanabilir.

**VPN:** Mullvad uygulamasının kendi WireGuard tüneli, resmi `mullvad` CLI'ı üzerinden yönetiliyor. Tünel tarayıcıya değil, bütün Mac'e uygulanıyor ve arayüzde de bu şekilde anlatılıyor. Hesap numarası hiçbir zaman arayüze okunmuyor.

**Reklam engelleme:** EasyList ve EasyPrivacy, Brave'in `adblock` crate'i ile WebKit content-blocker kurallarına çevriliyor (yaklaşık 131 bin kural), WebKit'te bir kez derleniyor ve sonraki açılışlarda önbellekten kullanılıyor.

**Ölçümler (release, macOS, 1100×880 pencere):**
- Tarayıcı süreci açılışta en fazla 136 MB, kullanımda 64–95 MB. WebKit web süreçleri bunun dışında.
- `inert-browser` release binary'si 19 MB (dört gömülü font dahil).

**Fuzzing (2026-11-07):** Dört hedef (`cbor_decode`, `message_decode`, `manifest_verify`, `screen_action_verify`) paralel olarak her biri 10 dakika çalıştı. Toplamda yaklaşık 78 milyon girdi denendi, hiç çökme yok. §16'daki 30 dakikalık hedefin üçte biri; daha uzun bir koşu bir sonraki adım.
