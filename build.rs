// Derleme betiği:
//  1) static/ klasöründeki tüm arayüz dosyalarını exe'nin içine gömmek için bir tablo üretir.
//  2) Windows'ta exe'ye simge ve sürüm bilgisi ekler.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn walk(dir: &Path, base: &Path, out: &mut Vec<(String, PathBuf)>) {
    let mut entries: Vec<PathBuf> = fs::read_dir(dir).expect("static klasörü okunamadı").filter_map(|e| e.ok().map(|e| e.path())).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            walk(&p, base, out);
        } else {
            let rel = p.strip_prefix(base).unwrap().to_string_lossy().replace('\\', "/");
            out.push((rel, p));
        }
    }
}

fn main() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let static_dir = manifest.join("static");
    println!("cargo:rerun-if-changed=static");
    println!("cargo:rerun-if-changed=assets");

    let mut files = Vec::new();
    walk(&static_dir, &static_dir, &mut files);
    let mut code = String::from("pub static STATIC_FILES: &[(&str, &[u8])] = &[\n");
    for (rel, path) in &files {
        code.push_str(&format!("    ({:?}, include_bytes!({:?})),\n", rel, path.to_string_lossy()));
    }
    code.push_str("];\n");
    let out = PathBuf::from(env::var("OUT_DIR").unwrap()).join("static_files.rs");
    fs::write(out, code).unwrap();

    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        // WebView2 yükleyicisi (webview2-com-sys) kayıt defteri ve olay günlüğü işlevleri için
        // advapi32 kütüphanesine ihtiyaç duyar; bazı sürümler bunu kendisi eklemiyor.
        println!("cargo:rustc-link-lib=advapi32");

        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        res.set("FileDescription", "Kütüphane Takip Sistemi Pro");
        res.set("ProductName", "Kütüphane Takip Sistemi Pro");
        res.set("CompanyName", "Halil Karakaya");
        res.set("LegalCopyright", "© 2026 Halil Karakaya. Tüm hakları saklıdır.");
        if let Err(e) = res.compile() {
            println!("cargo:warning=Exe simgesi eklenemedi: {}", e);
        }
    }
}
