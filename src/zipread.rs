//! EPUB (zip) içinden dosya okuma.

use std::path::Path;

#[cfg(not(localtest))]
mod imp {
    use std::fs::File;
    use std::io::Read;
    use std::path::Path;

    pub struct Archive(zip::ZipArchive<File>);

    impl Archive {
        pub fn open(path: &Path) -> Result<Archive, String> {
            let f = File::open(path).map_err(|e| e.to_string())?;
            zip::ZipArchive::new(f).map(Archive).map_err(|e| e.to_string())
        }
        pub fn names(&self) -> Vec<String> {
            self.0.file_names().map(|s| s.to_string()).collect()
        }
        pub fn read(&mut self, name: &str) -> Option<Vec<u8>> {
            let mut buf = Vec::new();
            let ok = match self.0.by_name(name) {
                Ok(mut e) => e.read_to_end(&mut buf).is_ok(),
                Err(_) => false,
            };
            if ok {
                Some(buf)
            } else {
                None
            }
        }
    }
}

/// Yerel test derlemesi: sistemdeki `unzip` aracını kullanır.
#[cfg(localtest)]
mod imp {
    use std::path::{Path, PathBuf};
    use std::process::Command;

    pub struct Archive(PathBuf);

    impl Archive {
        pub fn open(path: &Path) -> Result<Archive, String> {
            if path.exists() {
                Ok(Archive(path.to_path_buf()))
            } else {
                Err("dosya yok".into())
            }
        }
        pub fn names(&self) -> Vec<String> {
            Command::new("unzip")
                .arg("-Z1")
                .arg(&self.0)
                .output()
                .map(|o| String::from_utf8_lossy(&o.stdout).lines().map(|s| s.to_string()).collect())
                .unwrap_or_default()
        }
        pub fn read(&mut self, name: &str) -> Option<Vec<u8>> {
            let o = Command::new("unzip").arg("-p").arg(&self.0).arg(name).output().ok()?;
            if o.status.success() {
                Some(o.stdout)
            } else {
                None
            }
        }
    }
}

pub use imp::Archive;

impl Archive {
    /// Büyük/küçük harf farkını tolere ederek okur.
    pub fn read_any(&mut self, name: &str) -> Option<Vec<u8>> {
        if let Some(v) = self.read(name) {
            return Some(v);
        }
        let alt = self.names().into_iter().find(|n| n.eq_ignore_ascii_case(name))?;
        self.read(&alt)
    }
}

pub fn read_one(path: &Path, name: &str) -> Option<Vec<u8>> {
    Archive::open(path).ok()?.read_any(name)
}
