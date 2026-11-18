// Kütüphane Takip Sistemi Pro — Copyright (C) 2026 Halil Karakaya
// SPDX-License-Identifier: AGPL-3.0-only

//! Arayüz dosyaları. Normal derlemede `static/` klasörü exe'nin içine gömülür
//! (build.rs bir dosya tablosu üretir); yerel testte diskten okunur.

#[cfg(not(localtest))]
include!(concat!(env!("OUT_DIR"), "/static_files.rs"));

#[cfg(not(localtest))]
pub fn get(path: &str) -> Option<Vec<u8>> {
    STATIC_FILES.iter().find(|(p, _)| *p == path).map(|(_, d)| d.to_vec())
}

/// Gömülü tüm dosyaların yolları.
#[cfg(not(localtest))]
pub fn list() -> Vec<String> {
    STATIC_FILES.iter().map(|(p, _)| p.to_string()).collect()
}

#[cfg(localtest)]
pub fn list() -> Vec<String> {
    fn walk(dir: &std::path::Path, base: &std::path::Path, out: &mut Vec<String>) {
        for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, base, out);
            } else if let Ok(r) = p.strip_prefix(base) {
                out.push(r.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    let base = std::path::PathBuf::from(std::env::var("KYS_STATIC").unwrap_or_else(|_| "static".into()));
    let mut out = Vec::new();
    walk(&base, &base, &mut out);
    out
}

#[cfg(localtest)]
pub fn get(path: &str) -> Option<Vec<u8>> {
    if path.contains("..") {
        return None;
    }
    let base = std::env::var("KYS_STATIC").unwrap_or_else(|_| "static".into());
    std::fs::read(std::path::Path::new(&base).join(path)).ok()
}
