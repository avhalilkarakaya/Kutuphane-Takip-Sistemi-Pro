Kütüphane Takip Sistemi Pro **ücretsiz** ve açık kaynaklıdır (GNU AGPL-3.0). Kişisel ve kurumsal kütüphaneler için katalog, üye ve ödünç takibi, PDF/EPUB okuyucu (belge içinde arama ile), okuma takibi ve okuma notları sunar. İnternet bağlantısı gerektirmez.

## İndirme

Aşağıdaki **Assets** bölümünden bilgisayarınıza uygun dosyayı indirin:

| Dosya | Kimin için |
|---|---|
| `Kutuphane_Takip_Sistemi_Pro_64bit.exe` | Çoğu bilgisayar (Windows 10/11, 64 bit) |
| `Kutuphane_Takip_Sistemi_Pro_32bit.exe` | Yalnızca 32 bit Windows 10 |

Kurulum gerekmez. Exe dosyasına çift tıklamanız yeterlidir.

## İlk çalıştırmadan önce

1. **Exe'yi kendine ait bir klasöre taşıyın** (ör. `Belgeler\Kütüphane`). Uygulama verilerini (veritabanı, e-kitaplar, kapak resimleri) exe'nin bulunduğu klasörde tutar. İndirilenler klasöründen çalıştırırsanız veriler de oraya yazılır.
2. Windows **"Windows bilgisayarınızı korudu"** uyarısı gösterebilir. Uygulama dijital olarak imzalı olmadığı için çıkan bir uyarıdır. **Ek bilgi > Yine de çalıştır** ile açabilirsiniz.
3. Uygulama Microsoft Edge **WebView2** bileşenini kullanır. Windows 11'de ve güncel Windows 10'da zaten kuruludur. Kurulu değilse uygulama bunu bildirir.

## Önceki sürümden geçiş

Yeni exe'yi eski uygulamanın bulunduğu klasöre koyun. Mevcut `kutuphane_pro.db`, `e_kitaplar` ve `uploads` aynen kullanılır.

## Sistem gereksinimi

Windows 10 veya 11. Windows XP ve 7 desteklenmez.

## Doğrulama

`SHA256SUMS.txt` dosyası, indirdiğiniz exe'nin değişmediğini doğrulamak için sağlama toplamlarını içerir.
Windows'ta: `certutil -hashfile Kutuphane_Takip_Sistemi_Pro_64bit.exe SHA256`

---
© 2026 Halil Karakaya. GNU AGPL-3.0 ile lisanslanmıştır ve hiçbir garanti olmaksızın dağıtılır.
