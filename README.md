# Kütüphane Takip Sistemi Pro 2.0 (Rust sürümü)

Bu sürüm, önceki Go tabanlı uygulamanın Rust ile yeniden yazılmış hâlidir. Uygulama artık tarayıcıda açılmaz. Kendi penceresinde çalışan bağımsız bir Windows programıdır (Microsoft Edge WebView2 altyapısı). İnternet bağlantısı gerektirmez.

## Yenilikler

### E-kitap okuyucu
- **Belge içinde arama** (PDF ve EPUB):
  - Üst çubuktaki arama kutusu ya da `Ctrl+F` ile açılır. `Enter` sonraki, `Shift+Enter` önceki sonuca gider (`F3` / `Shift+F3` de çalışır).
  - Büyük/küçük harf ve Türkçe karakter ayrımı yapmaz. "sukru" yazınca "Şükrü" de bulunur.
  - Bulunan tüm sonuçlar yan panelde sayfa (PDF) veya bölüm (EPUB) bazında, çevresindeki metinle birlikte listelenir. Bir sonuca tıklayınca o konuma gidilir ve sonuç vurgulanır.
- Yeni PDF görüntüleyici (pdf.js) eklendi: sayfa numarasıyla gezinme, yakınlaştırma (`Ctrl` + fare tekerleği) ve sığdırma modları (otomatik / sayfa genişliği / tam sayfa).
- PDF'te kaldığınız sayfa hatırlanır. "İlerlemeyi okuma durumuna işle" seçeneği açıkken, ilerledikçe kitabın "Kaçıncı Sayfa" bilgisi kendiliğinden güncellenir.
- EPUB'da kaldığınız bölüm ve konum hatırlanır. Yazı boyutu ve okuma teması (açık / sepya / koyu) ayarlanabilir.
- **Okurken not alma:** Yan paneldeki "Notlar" sekmesinden sayfa numarasıyla not eklenebilir. Kitapta bir metni seçip "Seçili metni al" düğmesine basınca o metin alıntı olarak eklenir. Bir nota tıklayınca kitapta o yere gidilir.

### Okuma durumu
- **İlerleme çubuğu:** Toplam sayfa sayısına göre yüzde ve "sayfa / toplam" bilgisi gösterilir. Katalog listesinde, kayıt ayrıntısında, düzenleme formunda ve okuma takibinde görünür.
- Yeni durumlar: **Okunacak** (okuma listesi) ve **Yarım Bırakıldı**. Eskiden olan Okunmadı / Okunuyor / Okundu durumları da korundu.
- Okumaya başlama ve bitirme tarihleri tutulur. Durum değişince bu tarihler kendiliğinden doldurulur.
- Kitaba 1–5 yıldız arasında puan verilebilir.
- Günlük ortalama okunan sayfa ve tahmini bitiş tarihi hesaplanır.
- Son sayfaya gelindiğinde uygulama, kitabın "Okundu" olarak işaretlenmesini önerir.

### Okuma notları
- Her kitap için sınırsız not eklenebilir. Not türleri: Not, Alıntı, Soru, Önemli. Notlara isteğe bağlı sayfa numarası verilebilir.
- Notlar düzenlenebilir ve silinebilir. Kitap bazında ya da tüm kitaplarda aranabilir.

### Yeni "Okuma Takibi" bölümü
- Şu an okunan kitaplar, ilerlemeleri ve hızlı sayfa güncelleme (+10, +25, "Bitirdim").
- Yıllık okuma hedefi ve hedefin ne kadarının tamamlandığı (hedef Ayarlar'dan girilir).
- Bu yıl okunan sayfa sayısı, okuma serisi (art arda okunan gün sayısı) ve son 14 günde okunan sayfaları gösteren grafik.
- Okuma listesi (Okunacak), yarım bırakılanlar, son bitirilenler ve aylara göre bitirilen kitaplar.
- Tüm okuma notlarının tek bir listede toplanması.
- Her kitap için ayrı bir okuma penceresi: durum, puan, tarihler, notlar ve günlük okuma geçmişi.

### Diğer
- Tarayıcının uyarı kutuları yerine uygulamanın kendi onay pencereleri ve kısa bildirimleri kullanılıyor.
- Katalog aramasında Türkçe karakter ayrımı yok.
- CSV dışa aktarmaya okuma alanları (ilerleme %, tarihler, puan, not sayısı) eklendi. Dosya Excel'in Türkçe ayarlarıyla doğrudan açılır.

## Veri uyumluluğu
- Aynı veri yapısı kullanılır: `kutuphane_pro.db`, `e_kitaplar\` ve `uploads\`. Yeni exe'yi eski uygulamanın bulunduğu klasöre koymanız yeterlidir. Mevcut kitaplar, üyeler, ödünç kayıtları, kapaklar ve e-kitaplar aynen açılır.
- Yeni alanlar veritabanına ilk açılışta otomatik eklenir. Eski (Go) sürüm bu veritabanını açmaya devam edebilir. Yeni alanları görmez ama bozmaz.
- "Veritabanlarını Birleştir" özelliği normal ve Pro sürümlerin veritabanlarıyla çalışır.
- Exe'nin bulunduğu klasöre yazılamıyorsa (ör. `Program Files`), veriler `%APPDATA%\KutuphaneTakipPro` klasöründe tutulur. Ayarlar sayfasında hangi klasörün kullanıldığı gösterilir.

## Sistem gereksinimi
- Windows 10 veya 11 (64 ya da 32 bit).
- Microsoft Edge WebView2 Runtime. Windows 11'de ve güncel Windows 10'da zaten kuruludur. Kurulu değilse uygulama bunu bildirir. Ücretsiz "Evergreen" sürüm Microsoft'tan indirilebilir.
- **Windows XP / 7 desteklenmez.** WebView2 ve güncel Rust derleyicisi bu sistemleri desteklemiyor. Bu bilgisayarlarda eski Go sürümünü kullanmaya devam edebilirsiniz. Veritabanları ortaktır.

## Derleme (exe oluşturma)

### Yol 1: Kendi bilgisayarınızda
1. https://rustup.rs adresinden `rustup-init.exe` dosyasını indirip kurun. Kurulum "Visual Studio C++ Build Tools" isterse kurulmasına izin verin.
2. Bu klasördeki **`derle.bat`** dosyasına çift tıklayın.
3. Exe dosyası `dist\Kutuphane_Takip_Sistemi_Pro.exe` olarak oluşur.
   - 32 bit sürüm için komut isteminde `derle.bat 32` yazın.

İlk derleme gerekli paketleri internetten indirir ve 5–10 dakika sürebilir. Sonraki derlemeler daha hızlıdır. Oluşan exe tek dosyadır. Arayüz, PDF okuyucu ve SQLite exe'nin içindedir.

### Yol 2: GitHub üzerinde (bilgisayara bir şey kurmadan)
1. Bu klasörü (özel/private) bir GitHub deposuna yükleyin.
2. **Actions** sekmesinde "Windows exe derle" iş akışı kendiliğinden çalışır.
3. İş bitince sayfanın altındaki **Artifacts** bölümünden 64 bit ve 32 bit exe dosyalarını indirin.

## Okuyucu kısayolları
| Kısayol | İşlev |
|---|---|
| `Ctrl+F` | Kitap içinde ara |
| `Enter` / `Shift+Enter` | Sonraki / önceki sonuç |
| `F3` / `Shift+F3` | Sonraki / önceki sonuç |
| `←` / `→` | EPUB'da önceki / sonraki bölüm |
| `Ctrl` + fare tekerleği | PDF yakınlaştır / uzaklaştır |
| `Esc` | Aramayı temizle / okuyucuyu kapat |

## Proje yapısı
```
Cargo.toml, build.rs        Derleme ayarları (arayüz dosyaları ve simge exe'ye gömülür)
src/main.rs                 Başlangıç ve uygulama penceresi (tao + wry / WebView2)
src/api.rs                  Tüm işlevler: kitaplar, üyeler, ödünç, okuma takibi, notlar, yedek, birleştirme
src/http.rs                 Yalnızca bu bilgisayardan (127.0.0.1) erişilebilen küçük yerel sunucu
src/sqlite.rs               SQLite bağlantısı
src/epub.rs, zipread.rs     EPUB içindekiler ve kitap içi arama
src/json.rs, util.rs        Yardımcılar (JSON, Türkçe arama, tarih)
static/                     Arayüz (index.html, pdf-reader.html, stiller, pdf.js)
assets/                     Uygulama simgesi
```

© 2026 Halil Karakaya. Tüm hakları saklıdır.
Üçüncü taraf bileşenler: pdf.js (Apache 2.0), Font Awesome Free (OFL/MIT), SQLite (kamu malı), wry/tao (Apache 2.0/MIT).
