# Inert — proje notları

Kodsuz, doğrulanabilir uygulama protokolü. Adres şeması: `inert://`

_Son güncelleme: 2026-11-07_

## Verilen kararlar
1. Native protokol: browser değil. İstemciye çalıştırılabilir kod gitmez.
2. Hedef: site sahibi, kullanıcı ve sunucu birlikte korunur. Açık sınıfları tasarım gereği yok edilir.
3. Ekran deklaratif bir bileşen ağacıyla tarif edilir. Çizimi istemcinin içindeki güvenilir renderer yapar.
4. Tasarım, tasarım zamanında site sahibinin agent'ı tarafından üretilir. Verifier kontrol eder, imzalanır.
5. İstemcideki AI yalnızca kullanıcıya özel uyarlama yapar (font, kontrast, düzen). İçerik ve aksiyonlara dokunamaz.
6. Adoption ve porting işini agent'lar yapar. Spec küçük, katı ve makine tarafından doğrulanabilir olmalı.
7. Protokolü hem insanlar hem AI agent'ları kullanabilir.
8. Şifreleme olarak mevcut TLS 1.3 + PQ hibrit kullanılır, yeni kripto yok.

## Mimari (5 parça)
- spec/: protokol tanımı (Screen / Action / Result)
- core/: Rust parser, verifier, imza, layout
- server-sdk/: şemalı aksiyonlar
- client-macos/ + client-agent/ (MCP)
- agent-toolkit/: agent'lara spec ve verifier'ı tool olarak sunar

## Açık sorular
- İlk hedef platform (öneri: macOS uygulaması + agent istemcisi)
- Marka ve domain müsaitliği kontrol edilmedi

## Ertelendi: sonra düşünülecek
- **Güvenli çalışma ortamı / barındırma platformu (defense in depth).**
  Protokol PHP, TLS ve Linux kernel RCE'lerini tamamen çözmüyor. Fikirler:
  - rustls + formal doğrulanmış kripto (OpenSSL yerine)
  - Her site izole bir microVM'de (Firecracker, gVisor)
  - Uzun vade: minimal ya da doğrulanmış kernel (seL4)
