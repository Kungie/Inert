# Inert

Kodsuz, doğrulanabilir bir uygulama protokolü. Adres şeması: `inert://`
Slogan taslağı: "The web, behind glass." (Görürsün, dokunamazsın.)

Durum: dikey dilim implement edildi (2026-11-07 gecesi): protokol crate'leri, Rust tarayıcı, üç demo site. Ayrıntılar: README.md ve spec §18.

## Çalışma tercihleri
- Kısa cevaplar. Konuşma dili Türkçe.
- Sık commit at. Push ya da publish gibi dışa dönük işlemlerden önce sor.
- Yeni bir proje olduğu için ağır yoldan gidilir: tasarım onayı → yazılı spec → plan → kod.

## Problem
- AI ile bulunan açıkların çoğu şifrelemede değil. Kod (C/C++ bellek hataları), tarayıcı/JS yüzeyi (XSS, supply chain) ve uygulama mantığında çıkıyor.
- Yeni kripto icat edilmez. Mevcut TLS 1.3 + post-quantum hibrit (X25519MLKEM768) kullanılır.
- Hedef açık ihtimalini "sıfırlamak" değil, **açık sınıflarını tasarım gereği yok etmek** ve saldırı yüzeyini dramatik şekilde küçültmek.

## Tehdit modeli: üçü birden
- **Site sahibi:** Kodu, iş mantığı ve API'leri istemciden görülmez, kopyalanamaz.
- **Kullanıcı:** Kötü niyetli ya da hacklenmiş bir site cihaza zarar veremez.
- **Sunucu:** Dışarıya keyfi endpoint değil, tipli ve şemalı aksiyonlar açılır.
- Kabul edilen sınır: sunucudaki iş mantığı hataları daralır ama tamamen yok olmaz.

## Verilen kararlar
1. **Native protokol, browser değil.** İstemciye çalıştırılabilir kod gitmez (HTML/CSS/JS yok).
2. **Deklaratif bileşen ağacı.** Sunucu bir "tarif" gönderir (ekran, liste, buton, aksiyon). Çizimi istemcinin içindeki güvenilir renderer yapar (WhatsApp mesaj baloncuğu benzetmesi). Bileşen seti küçük (~30-50) ama zengin: layout, tema, animasyon presetleri, görsel/video. İleride izole bir canvas bileşeni düşünülebilir.
3. **Tasarım, tasarım zamanında üretilir.** Site sahibinin agent'ı bir stil prompt'undan layout ve tema üretir. Çıktı kod değil, kısıtlı bir formattır (Design Manifest). Verifier onaylar ve imzalanır, herkes aynı tasarımı görür.
4. **İstemci AI'ı yalnızca kullanıcı için uyarlar:** font, kontrast, sade düzen, koyu mod. Kural: AI neyin nasıl göründüğüne karar verir, ne olduğuna asla karar vermez. İçerik ve aksiyon eklenemez, gizlenemez, değiştirilemez (prompt injection ve AI phishing'e karşı).
5. **Adoption'ı agent'lar yapar.** Uygulamaları agent'lar yazar ya da mevcut sitelerden port eder. Bu yüzden spec küçük, katı ve makine tarafından doğrulanabilir olmalı. Güvensiz olan şey ifade bile edilememeli.
6. **İki tür tüketici:** insanlar (renderer ile) ve AI agent'ları (bileşen ağacını ve aksiyonları doğrudan okur).
7. **Kendi tarayıcımız da olacak (2026-11-07).** Rust ile yazılmış, küçük, hafif, az RAM kullanan, futuristik, minimal ve AI destekli bir tarayıcı. `inert://` siteleri **secured**, normal web siteleri **unsecured** olarak gösterilir. Normal web içeriği güvenilmez veri sayılır: tarayıcının AI'ı bu sayfalardaki metni talimat olarak almaz (prompt injection), bu sayfalarda onaysız aksiyon almaz. Normal web motoru Inert çekirdeğinden ve token'lardan ayrı, izole bir süreçte çalışır. Normal web motoru olarak Servo seçilmişti. İlk implementasyonda Gmail ve WhatsApp Web uyumluluğu için **sistem WebKit'i (WKWebView, `wry`)** kullanıldı. Servo hedef olarak kalıyor ve `web.rs` sınırının arkasına eklenebilir (spec §18). Eren'e sabah danışılacak.
8. **Kimlik doğrulama: kullanıcı kendi AI token'ını getirir (BYO token).** "Sign in with GPT" ve "Sign in with Claude". AI özellikleri (tasarım üretimi, kullanıcı uyarlaması, agent işlemleri) kullanıcının kendi modeliyle çalışır. Doğrulanmadı: OpenAI ve Anthropic'in üçüncü taraf uygulamalar için OAuth girişi sunup sunmadığı ve koşulları. Sunmuyorsa API key girişi yedek yol olur.

## Mimari
```
Site sahibi: Agent toolkit → Verifier (deploy) → Sunucu (SDK)
                                        │ Protokol: Screen · Action · Result (QUIC/TLS1.3+PQ)
Kullanıcı:   Çekirdek (imza + verifier) → Renderer (+ uyarlama AI) → Ekran
                       └→ AI agent istemcisi (ağacı doğrudan okur)
```
- **Protokol spec'i:** `Screen` (bileşen ağacı), `Action` (tipli, şemalı istek), `Result`. Binary ve katı format (CBOR). Şemaya uymayan mesaj parse aşamasında reddedilir.
- **İmza iki katmanlı (netleştirildi 2026-11-07):** Deploy zamanında imzalanan **App Manifest** (ekran şablonları, aksiyon şemaları, Design Manifest) çevrim dışı bir anahtarla imzalanır. Çalışma zamanındaki `Screen` mesajları kullanıcıya özel dinamik veri taşıdığı için önceden imzalanamaz. TLS kanalıyla gelir ve istemci her `Screen`'in imzalı manifest'e uyduğunu doğrular. Sonuç: ele geçirilmiş bir sunucu yanlış veri gösterebilir, ama yeni input, buton ya da sahte şifre alanı ekleyemez.
- **Taşıma (2026-11-07):** Dikey dilimde TCP + TLS 1.3 (`rustls`, PQ hibrit anahtar değişimi) + uzunluk önekli CBOR mesajlar. Taşıma katmanı bir arayüzün arkasında durur. Uzun vadeli hedef QUIC (`quinn`). Mesajları HTTPS üzerinden taşımak reddedildi, HTTP parser yüzeyini geri getirir.
- **Güven modeli (2026-11-07): alan adı + pinleme.** Bağlantı normal TLS sertifikasıyla (WebPKI) doğrulanır. Manifest imza anahtarı DNS'te yayınlanır (`_inert.<alan>` TXT kaydı). Tarayıcı anahtarı ilk ziyarette pinler, değişirse uyarır. Adresler tanıdık kalır: `inert://banka.com`. Dikey dilimde gerçek DNS yok: geliştirme modunda anahtar tarayıcıdaki yerel bir güven dosyasında durur, ama kod bu modele göre yazılır. Şeffaflık kaydı (Certificate Transparency benzeri) güvenliği artırma aşamasına ertelendi.
- **Design Manifest:** Agent'ın ürettiği layout ve tema. Kod değil, imzalı.
- **Verifier:** Tek kütüphane, iki yerde çalışır: deploy'da ve istemcide render öncesinde.
- **Inert çekirdeği:** Rust (bağlantı, parse, verifier, imza, layout). JS motoru yok, HTML motoru yok, eklenti yok. Hedef boyut birkaç on bin satır.
- **Tarayıcı: tamamen Rust (2026-11-07).** `winit` (pencere) + `wgpu` (GPU). Tarayıcı arayüzü, Servo (unsecured web) ve `inert://` renderer'ı aynı GPU yüzeyine çizer. Tek kod tabanı, tüm platformlar. Servo ayrı ve izole bir süreçte çalışır. Gereken ekstra iş: erişilebilirlik (AccessKit desteği olan bir UI kütüphanesi), metin girişi ve IME (Türkçe karakterler dahil).
- **Renderer:** `inert://` sayfaları native widget'larla değil, kendi `wgpu` renderer'ımızla çizilir. Yön A'nın görsel dili sayfaların içinde de korunur. (Önceki "SwiftUI native eşleme" kararının yerini aldı.)
- **Server SDK + Agent toolkit:** Ekran ve aksiyon tanımlama. Spec ve verifier agent'lara tool (MCP) olarak sunulur.

## Planlanan dizin yapısı
```
spec/          protokol tanımı
core/          Rust: parser, verifier, imza, layout
server-sdk/    ekran/aksiyon tanımı, aksiyon doğrulama
browser/       Rust tarayıcı: winit + wgpu kabuk, inert:// renderer, Servo entegrasyonu, AI paneli
client-agent/  MCP sunucusu (agent'lar için istemci)
agent-toolkit/ agent'lara spec + verifier, porting
examples/      örnek uygulama (ör. basit banka paneli)
```

## Alt projeler ve sıra
1. Protokol ve çekirdek (spec, parser, verifier, imza)
2. Server SDK
3. Tarayıcı (kabuk, `inert://` renderer, Servo, AI paneli)
4. Agent toolkit ve MCP istemcisi
5. Web sitesi (Yön A)

Her biri kendi spec → plan → kod döngüsüyle ilerler.

**Dikey dilimin spec'i:** `docs/superpowers/specs/2026-11-07-inert-vertical-slice-design.md`. Ayrıntılar orada, bu dosya sadece özet.

**Şu an: dikey dilim (2026-11-07).** 1, 2 ve 3'ün en küçük hâlleri uçtan uca: küçük bir sunucu imzalı bir `Screen` gönderir, tarayıcı doğrulayıp çizer, butona basılınca `Action` gider ve ekran güncellenir. Kapsam dışı: Servo, AI, agent'lar, web sitesi.

**Dilimin bileşen seti: geniş 10 (2026-11-07).** `Stack`, `Text`, `Button`, `Input`, `List`, `Divider`, `Image`, `Link`, `Toggle`, `Select`. Her yeni bileşen spec'e girmeden önce "bir açık sınıfı getiriyor mu?" sorusuyla değerlendirilir. Bu dördü için kurallar:
- `Image`: Sadece bellek güvenli Rust decoder'ları. Az sayıda format (ör. PNG, WebP), boyut ve piksel sınırı. Decode sonradan izole bir sürece taşınabilir.
- `Link`: Aynı site içinde ekranlar arası geçiş. Başka bir siteye giden link açık bir uyarıyla açılır, normal web'e giden link "unsecured" olarak işaretlenir.
- `Toggle` ve `Select`: Değerler ve seçenekler manifest'te önceden tanımlıdır. Sunucuya sadece tanımlı değerler gidebilir.

**Dilimin örnek uygulamaları: üçü birden (2026-11-07).** Hepsi sahte veriyle çalışır, gerçek bir servise bağlanmaz.
1. Banka paneli: ana demo, güvenlik hikâyesi.
2. Mağaza ve sepet.
3. Not ve görev uygulaması.

## Neyi çözer, neyi çözmez
- Tarayıcı RCE'leri (V8/JIT, kötü niyetli JS): büyük ölçüde çözer, istemcide JS yok.
- PHP RCE'leri: kısmen. Keyfi HTTP isteği (header, path, upload) yolu kapanır. Uygulama içi tehlikeli mantık (ör. `unserialize`) kalır.
- TLS kütüphane açıkları: çözmez. `rustls` ve doğrulanmış kripto ile azaltılır.
- Linux kernel RCE'leri: çözmez. Protokol OS'un üstünde çalışır.

Gece raporu (görsel özet): https://claude.ai/artifact/BERyRh213X8xgAk2CSKnMD

## Tarayıcıda olanlar (2026-11-07)
- Stencil tarzı koyu arayüz, egui + wgpu, dört Geist fontu gömülü.
- Giriş: sadece AI sağlayıcı (Claude / GPT) + isteğe bağlı API key (Keychain).
- Sekmeler, sekme grupları (Inbox, Homeserver, Developer + normal gruplar), yer imleri, geçmiş, omnibox (satır içi tamamlama + arama önerileri), kapatılan sekmeyi geri açma, sayfada bul, zoom, indirmeler, memory saver, native menü ve kısayollar.
- `inert://` siteleri SECURED, normal web UNSECURED / NOT ENCRYPTED rozetiyle.
- Shield: EasyList + EasyPrivacy, WebKit content blocker.
- VPN: Mullvad (CLI üzerinden), Stencil temalı panel.
- Asistan: inert:// sayfaları imzalı ağaçtan, web sayfaları güvenilmez metin olarak okunur. Salt okunur.
- `--demo` profili ve `scripts/walkthrough.sh` ile otomatik ekran görüntüsü turu.

## Ertelendi: güvenliği artırmaya gelince düşünülecek
- Güvenli çalışma ortamı / barındırma platformu (defense in depth): `rustls` + doğrulanmış kripto, her site izole bir microVM'de (Firecracker, gVisor), uzun vade minimal ya da doğrulanmış kernel (seL4).

## Görsel kimlik (marka ve web sitesi)
- **Font ailesi: Geist, dört çeşidiyle.** Hangisi nereye uyuyorsa o kullanılır. npm: `geist` (≥1.9.0, Serif 1.9.0 ile geldi). Web dosyaları `dist/fonts/` altında.
  - **Geist Serif:** başlıklar ve marka sesi. Şimdilik sadece Regular var, vurgu ağırlıkla değil boyutla yapılır.
  - **Geist Sans:** gövde metni, navigasyon, butonlar.
  - **Geist Mono:** etiketler (büyük harf, geniş harf aralığı), protokol örnekleri, kod.
  - **Geist Pixel:** wordmark (koyu yön), adım numaraları ve küçük vurgular. Açık zeminde wordmark için fazla baskın.
- **Ton:** olabildiğince minimal ve futuristik.
- **Referanslar:**
  - stencil.so: saf siyah zemin, `#EDEDED` metin, dither/noktalı (stipple) mor-mavi gezegen yayı ve yıldızlar, küçük harf aralıklı mono büyük harf etiketler, ince kenarlıklı küçük butonlar ve `→` okları, neredeyse boş kompozisyon, logo sol altta.
  - stoaexchange.com: sıcak kırık beyaz zemin, sıcak gri tonlar (`#272625` metin, `#706C6A` buton), Inter, hap (pill) butonlar, 24px regular başlıklar ve hafif negatif harf aralığı, arka planda mono ASCII ticker metni, dikey çizgili ve dolu sütun grafikleri, küçük mono etiketler.
- **Seçilen yön: Yön A (Stencil dili, koyu).** Web sitesi ve tarayıcı arayüzü bu dille yapılır.
- **Tasarım dosyaları (Claude Design, 2026-11-07):**
  - Yön A, Stencil dili (koyu, dither cam küre): https://claude.ai/artifact/FeE2baRHUwY1tdfxLf6cbz
  - Yön B, Stoa dili (açık, sütunlu galeri, protokol ticker'ı): https://claude.ai/artifact/7Phi2q4Cashnwoo1Xnmen2
  - Her ikisinde de bir landing sayfası ve dört Geist çeşidinin rollerini gösteren bir font sayfası var.

## Açık sorular
- İlk geliştirme ve test platformu macOS (tarayıcı çapraz platform olsa da).
- Tarayıcı arayüzü için hangi Rust UI kütüphanesi (AccessKit desteği şart).
- "Unsecured" etiketinin tam metni (ör. "Web · Unverified").
- Marka ve domain müsaitliği kontrol edilmedi.
- GPT ve Claude ile giriş için OAuth desteği doğrulanmalı.
