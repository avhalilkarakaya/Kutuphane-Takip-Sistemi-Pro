//! Arayüz dosyaları. Normal derlemede `static/` klasörü exe'nin içine gömülür
//! (build.rs bir dosya tablosu üretir); yerel testte diskten okunur.

#[cfg(not(localtest))]
include!(concat!(env!("OUT_DIR"), "/static_files.rs"));

#[cfg(not(localtest))]
pub fn get(path: &str) -> Option<Vec<u8>> {
    STATIC_FILES.iter().find(|(p, _)| *p == path).map(|(_, d)| d.to_vec())
}

#[cfg(localtest)]
pub fn get(path: &str) -> Option<Vec<u8>> {
    if path.contains("..") {
        return None;
    }
    let base = std::env::var("KYS_STATIC").unwrap_or_else(|_| "static".into());
    std::fs::read(std::path::Path::new(&base).join(path)).ok()
}
