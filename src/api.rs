//! Uygulamanın tüm HTTP uç noktaları (API, e-kitap ve statik dosyalar).

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use crate::http::{Part, Request, Response};
use crate::json::Json;
use crate::sqlite::{Db, Table, Value};
use crate::util::{extension, fmt_date, now_millis, parse_date, pct_decode, pct_encode, sanitize_filename};

pub const VERSION: &str = "2.0.0";
pub const DEFAULT_LIB: &str = "Karakaya Kütüphanesi";
pub const STATUSES: [&str; 5] = ["Okunmadı", "Okunacak", "Okunuyor", "Okundu", "Yarım Bırakıldı"];
const NOTE_KINDS: [&str; 4] = ["Not", "Alıntı", "Soru", "Önemli"];

/// Formdan doğrudan metin olarak kaydedilen alanlar.
const FORM_TEXT: &[&str] = &[
    "title", "original_title", "author", "series_title", "publisher", "pub_place", "pub_year", "isbn", "language",
    "dimensions", "gmt", "content_type", "carrier_type", "category", "subject", "classification", "shelf_num",
    "inventory_code", "acquisition_date", "acquisition_type", "notes",
];

/// Eski sürüm veritabanlarına sonradan eklenen sütunlar.
const EXTRA_COLS: &[(&str, &str)] = &[
    ("original_title", "TEXT"),
    ("series_title", "TEXT"),
    ("publisher", "TEXT"),
    ("pub_place", "TEXT"),
    ("pub_year", "TEXT"),
    ("language", "TEXT"),
    ("page_count", "INTEGER DEFAULT 0"),
    ("dimensions", "TEXT"),
    ("gmt", "TEXT"),
    ("content_type", "TEXT"),
    ("carrier_type", "TEXT"),
    ("subject", "TEXT"),
    ("classification", "TEXT"),
    ("shelf_num", "TEXT"),
    ("inventory_code", "TEXT"),
    ("acquisition_date", "TEXT"),
    ("acquisition_type", "TEXT"),
    ("reading_status", "TEXT DEFAULT 'Okunmadı'"),
    ("current_page", "INTEGER DEFAULT 0"),
    ("notes", "TEXT"),
    ("library_source", "TEXT"),
    ("cover_image", "TEXT"),
    ("ebook_file", "TEXT"),
    // 2.0 (Rust sürümü) ile gelen okuma takibi alanları
    ("reading_start", "TEXT"),
    ("reading_end", "TEXT"),
    ("rating", "INTEGER DEFAULT 0"),
];

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS books (
    id INTEGER PRIMARY KEY AUTOINCREMENT, title TEXT NOT NULL, author TEXT NOT NULL,
    category TEXT, isbn TEXT, total_copies INTEGER DEFAULT 1, available_copies INTEGER DEFAULT 1);
CREATE TABLE IF NOT EXISTS members (
    id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL, phone TEXT, email TEXT, join_date TEXT);
CREATE TABLE IF NOT EXISTS loans (
    id INTEGER PRIMARY KEY AUTOINCREMENT, book_id INTEGER, member_id INTEGER,
    loan_date TEXT, due_date TEXT, return_date TEXT, status TEXT,
    FOREIGN KEY(book_id) REFERENCES books(id), FOREIGN KEY(member_id) REFERENCES members(id));
CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value TEXT);
CREATE TABLE IF NOT EXISTS reading_notes (
    id INTEGER PRIMARY KEY AUTOINCREMENT, book_id INTEGER NOT NULL, page INTEGER, kind TEXT DEFAULT 'Not',
    note TEXT NOT NULL, location TEXT, created_at TEXT, updated_at TEXT);
CREATE INDEX IF NOT EXISTS idx_reading_notes_book ON reading_notes(book_id);
CREATE TABLE IF NOT EXISTS reading_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT, book_id INTEGER NOT NULL, log_date TEXT NOT NULL, page INTEGER NOT NULL,
    UNIQUE(book_id, log_date));
";

pub struct App {
    db: Mutex<Db>,
    pub data_dir: PathBuf,
    ebook_dir: PathBuf,
    upload_dir: PathBuf,
}

type R = Result<Response, String>;

// ---------------- Okuma durumu kuralları ----------------

pub struct Reading {
    pub status: String,
    pub page: i64,
    pub total: i64,
    pub start: String,
    pub end: String,
}

impl Reading {
    pub fn normalize(&mut self, today: &str) {
        if !STATUSES.contains(&self.status.as_str()) {
            self.status = "Okunmadı".into();
        }
        if parse_date(&self.start).is_none() {
            self.start.clear();
        }
        if parse_date(&self.end).is_none() {
            self.end.clear();
        }
        self.start.truncate(10);
        self.end.truncate(10);
        self.total = self.total.max(0);
        self.page = self.page.max(0);
        if self.total > 0 && self.page > self.total {
            self.page = self.total;
        }
        match self.status.as_str() {
            "Okundu" => {
                if self.total > 0 {
                    self.page = self.total;
                }
                if self.end.is_empty() {
                    self.end = today.to_string();
                }
                if !self.start.is_empty() && self.start > self.end {
                    self.start = self.end.clone();
                }
            }
            "Okunuyor" => {
                if self.start.is_empty() {
                    self.start = today.to_string();
                }
                self.end.clear();
            }
            "Yarım Bırakıldı" => self.end.clear(),
            _ => {
                self.page = 0;
                self.start.clear();
                self.end.clear();
            }
        }
    }
    /// Günlük okuma geçmişine kayıt düşülmeli mi? (Geçmiş tarihte bitirilmiş kitaplar bugüne yazılmaz.)
    fn should_log(&self, today: &str) -> bool {
        self.page > 0
            && match self.status.as_str() {
                "Okunuyor" | "Yarım Bırakıldı" => true,
                "Okundu" => self.end == today,
                _ => false,
            }
    }
}

fn to_int(s: &str) -> i64 {
    s.trim().parse::<i64>().or_else(|_| s.trim().parse::<f64>().map(|f| f as i64)).unwrap_or(0)
}

pub fn mime_for(name: &str) -> &'static str {
    match extension(name).as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "xhtml" | "xht" => "application/xhtml+xml; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "json" => "application/json; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "avif" => "image/avif",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "pdf" => "application/pdf",
        "epub" => "application/epub+zip",
        "ncx" => "application/x-dtbncx+xml",
        "opf" => "application/oebps-package+xml",
        "xml" => "application/xml; charset=utf-8",
        "wasm" => "application/wasm",
        "txt" => "text/plain; charset=utf-8",
        "mp3" => "audio/mpeg",
        "mp4" => "video/mp4",
        _ => "application/octet-stream",
    }
}

fn ok_json(mut extra: Json, msg: &str) -> Response {
    extra.set("success", true.into());
    if !msg.is_empty() {
        extra.set("message", msg.into());
    }
    Response::json(&extra)
}

fn display_ebook_name(f: &str) -> String {
    // ekitap_<zaman>_<ad> biçiminden özgün adı çıkar
    if let Some(rest) = f.strip_prefix("ekitap_") {
        if let Some((num, name)) = rest.split_once('_') {
            if !num.is_empty() && num.chars().all(|c| c.is_ascii_digit()) {
                return name.to_string();
            }
        }
    }
    f.to_string()
}

fn safe_name(n: &str) -> bool {
    !n.is_empty() && !n.contains(['/', '\\']) && !n.contains("..") && !n.contains(':')
}

impl App {
    pub fn open(data_dir: PathBuf) -> Result<App, String> {
        let ebook_dir = data_dir.join("e_kitaplar");
        let upload_dir = data_dir.join("uploads");
        for d in [&data_dir, &ebook_dir, &upload_dir] {
            fs::create_dir_all(d).map_err(|e| format!("Klasör oluşturulamadı ({}): {}", d.display(), e))?;
        }
        let db = Db::open(&data_dir.join("kutuphane_pro.db"))?;
        db.exec_batch(SCHEMA)?;
        let cols = db.table_columns("main", "books")?;
        for (c, ty) in EXTRA_COLS {
            if !cols.iter().any(|x| x == c) {
                db.execute(&format!("ALTER TABLE books ADD COLUMN {} {}", c, ty), p![])?;
            }
        }
        let ncols = db.table_columns("main", "reading_notes")?;
        if !ncols.iter().any(|x| x == "location") {
            db.execute("ALTER TABLE reading_notes ADD COLUMN location TEXT", p![])?;
        }
        db.execute("INSERT OR IGNORE INTO settings (key, value) VALUES ('library_name', ?)", p![DEFAULT_LIB])?;
        Ok(App { db: Mutex::new(db), data_dir, ebook_dir, upload_dir })
    }

    fn db(&self) -> MutexGuard<'_, Db> {
        self.db.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn route(&self, req: Request) -> Response {
        match self.route_inner(&req) {
            Ok(r) => r,
            Err(e) => Response::fail(&format!("Hata oluştu: {}", e)),
        }
    }

    fn route_inner(&self, req: &Request) -> R {
        let segs: Vec<String> = req.path.trim_start_matches('/').split('/').map(|s| pct_decode(s, false)).collect();
        let seg: Vec<&str> = segs.iter().map(|s| s.as_str()).collect();
        let m = if req.method == "HEAD" { "GET" } else { req.method.as_str() };
        let id = |s: &str| s.parse::<i64>().map_err(|_| "Geçersiz kimlik".to_string());
        match (m, seg.as_slice()) {
            ("GET", [""]) | ("GET", ["index.html"]) => Ok(self.static_file("index.html")),
            ("GET", ["static", rest @ ..]) => Ok(self.static_file(&rest.join("/"))),
            ("GET", ["uploads", name]) => {
                if !safe_name(name) {
                    return Ok(Response::not_found());
                }
                Ok(Response::file(&self.upload_dir.join(name), mime_for(name)).header("Cache-Control", "max-age=86400"))
            }
            ("GET", ["ebook", i, "file"]) => self.ebook_file(id(i)?, req.q("dl").is_some()),
            ("GET", ["ebook", i, "epub", rest @ ..]) => self.epub_entry(id(i)?, &rest.join("/")),

            ("GET", ["api", "settings"]) => self.get_settings(),
            ("POST", ["api", "settings"]) => self.post_settings(req),
            ("GET", ["api", "stats"]) => self.stats(),
            ("GET", ["api", "books"]) => self.list_books(),
            ("POST", ["api", "books"]) => self.save_book(req, None),
            ("PUT", ["api", "books", i]) => self.save_book(req, Some(id(i)?)),
            ("DELETE", ["api", "books", i]) => self.delete_book(id(i)?),
            ("GET", ["api", "books", i, "toc"]) => self.toc(id(i)?),
            ("GET", ["api", "books", i, "search"]) => self.search(id(i)?, req.q("q").unwrap_or("")),
            ("POST", ["api", "books", i, "progress"]) => self.progress(id(i)?, req),
            ("GET", ["api", "books", i, "notes"]) => self.book_notes(id(i)?),
            ("POST", ["api", "books", i, "notes"]) => self.add_note(id(i)?, req),
            ("GET", ["api", "books", i, "log"]) => self.book_log(id(i)?),
            ("GET", ["api", "notes"]) => self.all_notes(),
            ("PUT", ["api", "notes", i]) => self.update_note(id(i)?, req),
            ("DELETE", ["api", "notes", i]) => self.delete_note(id(i)?),
            ("GET", ["api", "reading", "summary"]) => self.reading_summary(),

            ("GET", ["api", "members"]) => self.list_members(),
            ("POST", ["api", "members"]) => self.add_member(req),
            ("DELETE", ["api", "members", i]) => self.delete_member(id(i)?),
            ("GET", ["api", "loans"]) => self.list_loans(),
            ("POST", ["api", "loans"]) => self.add_loan(req),
            ("PUT", ["api", "loans", i, "return"]) => self.return_loan(id(i)?),

            ("GET", ["api", "backup"]) => self.backup(),
            ("GET", ["api", "export.csv"]) => self.export_csv(),
            ("POST", ["api", "merge-db"]) => self.merge_db(req),
            _ => Ok(Response::not_found()),
        }
    }

    fn static_file(&self, path: &str) -> Response {
        match crate::assets::get(path) {
            Some(data) => {
                let cache = if path.ends_with(".html") { "no-cache" } else { "max-age=3600" };
                Response::bytes(200, mime_for(path), data).header("Cache-Control", cache)
            }
            None => Response::not_found(),
        }
    }

    fn today(db: &Db) -> Result<String, String> {
        db.query_text("SELECT date('now','localtime')", p![])
    }

    fn setting(db: &Db, key: &str) -> String {
        db.query_text("SELECT value FROM settings WHERE key = ?", p![key]).unwrap_or_default()
    }

    fn lib_name(db: &Db) -> String {
        let n = Self::setting(db, "library_name");
        if n.trim().is_empty() {
            DEFAULT_LIB.to_string()
        } else {
            n
        }
    }

    fn book_row(db: &Db, id: i64) -> Result<Option<Json>, String> {
        let t = db.query(
            "SELECT b.*, (SELECT COUNT(*) FROM reading_notes n WHERE n.book_id = b.id) AS note_count,
                    (SELECT MAX(l.log_date) FROM reading_log l WHERE l.book_id = b.id) AS last_read
             FROM books b WHERE b.id = ?",
            p![id],
        )?;
        Ok(t.to_json().into_iter().next())
    }

    // ---------------- Ayarlar & istatistikler ----------------

    fn get_settings(&self) -> R {
        let db = self.db();
        let t = db.query("SELECT key, value FROM settings", p![])?;
        let mut o = Json::Obj(Vec::new());
        for r in &t.rows {
            o.set(&r[0].as_text(), Json::Str(r[1].as_text()));
        }
        o.set("library_name", Self::lib_name(&db).into());
        o.set("data_dir", self.data_dir.display().to_string().into());
        o.set("version", VERSION.into());
        Ok(Response::json(&o))
    }

    fn post_settings(&self, req: &Request) -> R {
        let j = req.json();
        let db = self.db();
        let Json::Obj(pairs) = j else { return Ok(Response::fail("Form okunamadı.")) };
        for (k, v) in &pairs {
            let val = v.text().trim().to_string();
            if k == "library_name" {
                if val.is_empty() {
                    return Ok(Response::fail("Kütüphane ismi boş olamaz."));
                }
            } else if !allowed_pref(k) {
                continue;
            }
            db.execute("INSERT OR REPLACE INTO settings (key, value) VALUES (?, ?)", p![k.as_str(), val])?;
        }
        Ok(Response::ok("Kaydedildi."))
    }

    fn stats(&self) -> R {
        let db = self.db();
        let q = |s: &str| db.query_i64(s, p![]);
        let o = Json::obj([
            ("total_titles", q("SELECT COUNT(*) FROM books")?.into()),
            ("total_books", q("SELECT COALESCE(SUM(total_copies),0) FROM books")?.into()),
            ("ebooks", q("SELECT COUNT(*) FROM books WHERE COALESCE(ebook_file,'') <> ''")?.into()),
            ("total_members", q("SELECT COUNT(*) FROM members")?.into()),
            ("active_loans", q("SELECT COUNT(*) FROM loans WHERE status='Aktif'")?.into()),
            (
                "overdue_loans",
                q("SELECT COUNT(*) FROM loans WHERE status='Aktif' AND due_date < date('now','localtime')")?.into(),
            ),
            ("reading_now", q("SELECT COUNT(*) FROM books WHERE reading_status='Okunuyor'")?.into()),
            (
                "finished_year",
                q("SELECT COUNT(*) FROM books WHERE reading_status='Okundu' AND substr(COALESCE(reading_end,''),1,4) = strftime('%Y','now','localtime')")?
                    .into(),
            ),
        ]);
        Ok(Response::json(&o))
    }

    // ---------------- Kitaplar ----------------

    fn list_books(&self) -> R {
        let db = self.db();
        let t = db.query(
            "SELECT b.*, (SELECT COUNT(*) FROM reading_notes n WHERE n.book_id = b.id) AS note_count,
                    (SELECT MAX(l.log_date) FROM reading_log l WHERE l.book_id = b.id) AS last_read
             FROM books b ORDER BY b.id DESC",
            p![],
        )?;
        Ok(Response::json(&Json::Arr(t.to_json())))
    }

    fn save_book(&self, req: &Request, id: Option<i64>) -> R {
        let parts = req.multipart();
        if parts.is_empty() {
            return Ok(Response::fail("Form okunamadı."));
        }
        let field_opt = |k: &str| {
            parts.iter().find(|p| p.name == k && p.filename.is_none()).map(|p| p.text().trim().to_string())
        };
        let field = |k: &str| field_opt(k).unwrap_or_default();
        let file = |k: &str| {
            parts.iter().find(|p: &&Part| {
                p.name == k && p.filename.as_deref().map(|f| !f.is_empty()).unwrap_or(false) && !p.data.is_empty()
            })
        };
        let title = field("title");
        let author = field("author");
        if title.is_empty() || author.is_empty() {
            return Ok(Response::fail("Kitap adı ve yazar zorunludur."));
        }
        let copies = to_int(&field("copies")).max(1);

        let db = self.db();
        let today = Self::today(&db)?;
        let old = match id {
            Some(i) => match Self::book_row(&db, i)? {
                Some(b) => Some(b),
                None => return Ok(Response::fail("Kitap bulunamadı.")),
            },
            None => None,
        };
        let old_text = |k: &str| old.as_ref().map(|b| b.field_text(k)).unwrap_or_default();
        let pick = |k: &str| field_opt(k).unwrap_or_else(|| old_text(k));

        let mut r = Reading {
            status: pick("reading_status"),
            page: to_int(&pick("current_page")),
            total: to_int(&pick("page_count")),
            start: pick("reading_start"),
            end: pick("reading_end"),
        };
        r.normalize(&today);
        let rating = to_int(&pick("rating")).clamp(0, 5);

        // --- dosyalar ---
        let stamp = now_millis();
        let mut cover = old_text("cover_image");
        let old_cover = cover.clone();
        if let Some(p) = file("cover_file") {
            let fname = sanitize_filename(p.filename.as_deref().unwrap_or("kapak"));
            let ext = extension(&fname);
            if !["jpg", "jpeg", "png", "gif", "webp", "bmp", "avif", "svg"].contains(&ext.as_str()) {
                return Ok(Response::fail("Kapak için yalnızca resim dosyası (JPG, PNG, WEBP, GIF) seçilebilir."));
            }
            let name = format!("kapak_{}_{}", stamp, fname);
            fs::write(self.upload_dir.join(&name), &p.data).map_err(|e| format!("Dosya kaydedilemedi: {}", e))?;
            cover = format!("/uploads/{}", name);
        }
        let mut ebook = old_text("ebook_file");
        let mut remove_files: Vec<PathBuf> = Vec::new();
        if field("remove_ebook") == "1" && !ebook.is_empty() {
            if safe_name(&ebook) {
                remove_files.push(self.ebook_dir.join(&ebook));
            }
            ebook.clear();
        }
        if let Some(p) = file("ebook_file") {
            let fname = sanitize_filename(p.filename.as_deref().unwrap_or("ekitap.pdf"));
            let ext = extension(&fname);
            if ext != "pdf" && ext != "epub" {
                return Ok(Response::fail("E-kitap olarak yalnızca PDF veya EPUB dosyası eklenebilir."));
            }
            let name = format!("ekitap_{}_{}", stamp, fname);
            fs::write(self.ebook_dir.join(&name), &p.data).map_err(|e| format!("Dosya kaydedilemedi: {}", e))?;
            if !ebook.is_empty() && safe_name(&ebook) {
                remove_files.push(self.ebook_dir.join(&ebook));
            }
            ebook = name;
        }

        // --- sütunlar ---
        let mut cols: Vec<(&str, Value)> = Vec::new();
        for k in FORM_TEXT {
            cols.push((*k, Value::Text(pick(k))));
        }
        cols.push(("page_count", Value::Int(r.total)));
        cols.push(("current_page", Value::Int(r.page)));
        cols.push(("reading_status", Value::Text(r.status.clone())));
        cols.push(("reading_start", Value::Text(r.start.clone())));
        cols.push(("reading_end", Value::Text(r.end.clone())));
        cols.push(("rating", Value::Int(rating)));
        cols.push(("cover_image", Value::Text(cover.clone())));
        cols.push(("ebook_file", Value::Text(ebook.clone())));

        let (book_id, msg) = match id {
            None => {
                cols.push(("total_copies", Value::Int(copies)));
                cols.push(("available_copies", Value::Int(copies)));
                cols.push(("library_source", Value::Text(Self::lib_name(&db))));
                let names: Vec<&str> = cols.iter().map(|c| c.0).collect();
                let marks = vec!["?"; cols.len()].join(", ");
                let vals: Vec<Value> = cols.into_iter().map(|c| c.1).collect();
                db.execute(&format!("INSERT INTO books ({}) VALUES ({})", names.join(", "), marks), &vals)?;
                (db.last_id(), "Eser kaydedildi.")
            }
            Some(i) => {
                let active = db.query_i64("SELECT COUNT(*) FROM loans WHERE book_id = ? AND status = 'Aktif'", p![i])?;
                if copies < active {
                    return Ok(Response::fail(&format!(
                        "Bu eserin {} kopyası şu anda ödünçte. Toplam kopya bundan az olamaz.",
                        active
                    )));
                }
                cols.push(("total_copies", Value::Int(copies)));
                cols.push(("available_copies", Value::Int(copies - active)));
                let sets: Vec<String> = cols.iter().map(|c| format!("{} = ?", c.0)).collect();
                let mut vals: Vec<Value> = cols.into_iter().map(|c| c.1).collect();
                vals.push(Value::Int(i));
                db.execute(&format!("UPDATE books SET {} WHERE id = ?", sets.join(", ")), &vals)?;
                (i, "Eser güncellendi.")
            }
        };
        let old_page = to_int(&old_text("current_page"));
        if r.should_log(&today) && (id.is_none() || r.page != old_page) {
            Self::log_progress(&db, book_id, old_page, r.page, &r.start, &today)?;
        }
        for f in remove_files {
            let _ = fs::remove_file(f);
        }
        if cover != old_cover {
            self.cleanup_cover(&db, &old_cover);
        }
        Ok(ok_json(Json::obj([("id", book_id.into())]), msg))
    }

    fn cleanup_cover(&self, db: &Db, cover: &str) {
        if let Some(name) = cover.strip_prefix("/uploads/") {
            if safe_name(name) {
                let used = db.query_i64("SELECT COUNT(*) FROM books WHERE cover_image = ?", p![cover]).unwrap_or(1);
                if used == 0 {
                    let _ = fs::remove_file(self.upload_dir.join(name));
                }
            }
        }
    }

    /// Günlük okuma geçmişine kayıt ekler. Kitabın ilk kaydında, takipten önce okunmuş sayfalar
    /// bugüne yazılmasın diye başlangıç tarihine (yoksa düne) bir başlangıç kaydı düşülür.
    fn log_progress(db: &Db, book_id: i64, old_page: i64, page: i64, start: &str, today: &str) -> Result<(), String> {
        if db.query_i64("SELECT COUNT(*) FROM reading_log WHERE book_id = ?", p![book_id])? == 0 {
            let started_before = !start.is_empty() && start < today;
            let base = if old_page > 0 { old_page } else if started_before { page } else { 0 };
            if base > 0 {
                let date = if started_before {
                    start.to_string()
                } else {
                    parse_date(today).map(|d| fmt_date(d - 1)).unwrap_or_else(|| today.to_string())
                };
                db.execute("INSERT OR IGNORE INTO reading_log (book_id, log_date, page) VALUES (?, ?, ?)", p![book_id, date, base])?;
            }
        }
        db.execute(
            "INSERT INTO reading_log (book_id, log_date, page) VALUES (?, ?, ?)
             ON CONFLICT(book_id, log_date) DO UPDATE SET page = excluded.page",
            p![book_id, today, page],
        )?;
        Ok(())
    }

    fn delete_book(&self, id: i64) -> R {
        let db = self.db();
        let active = db.query_i64("SELECT COUNT(*) FROM loans WHERE book_id = ? AND status = 'Aktif'", p![id])?;
        if active > 0 {
            return Ok(Response::fail("Bu kitap şu anda ödünçte olduğu için silinemez. Önce iade alınmalıdır."));
        }
        let Some(b) = Self::book_row(&db, id)? else { return Ok(Response::fail("Kitap bulunamadı.")) };
        db.transaction(|d| {
            d.execute("DELETE FROM books WHERE id = ?", p![id])?;
            d.execute("DELETE FROM loans WHERE book_id = ?", p![id])?;
            d.execute("DELETE FROM reading_notes WHERE book_id = ?", p![id])?;
            d.execute("DELETE FROM reading_log WHERE book_id = ?", p![id])?;
            Ok(())
        })?;
        let eb = b.field_text("ebook_file");
        if safe_name(&eb) {
            let _ = fs::remove_file(self.ebook_dir.join(&eb));
        }
        self.cleanup_cover(&db, &b.field_text("cover_image"));
        Ok(Response::ok("Silindi."))
    }

    fn ebook_path(&self, id: i64) -> Result<Option<PathBuf>, String> {
        let f = self.db().query_text("SELECT COALESCE(ebook_file,'') FROM books WHERE id = ?", p![id])?;
        if !safe_name(&f) {
            return Ok(None);
        }
        let p = self.ebook_dir.join(&f);
        Ok(if p.exists() { Some(p) } else { None })
    }

    fn ebook_file(&self, id: i64, download: bool) -> R {
        let Some(path) = self.ebook_path(id)? else { return Ok(Response::not_found()) };
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let mut r = Response::file(&path, mime_for(&name));
        if download {
            r = r.attachment(&display_ebook_name(&name));
        }
        Ok(r)
    }

    fn epub_entry(&self, id: i64, inner: &str) -> R {
        let Some(path) = self.ebook_path(id)? else { return Ok(Response::not_found()) };
        match crate::zipread::read_one(&path, inner) {
            Some(data) => Ok(Response::bytes(200, mime_for(inner), data)
                .header("Content-Security-Policy", "script-src 'none'")
                .header("Cache-Control", "max-age=600")),
            None => Ok(Response::not_found()),
        }
    }

    fn toc(&self, id: i64) -> R {
        let Some(path) = self.ebook_path(id)? else { return Ok(Response::fail("E-kitap dosyası bulunamadı.")) };
        match crate::epub::chapters(&path) {
            Ok(chs) => {
                let list: Vec<Json> = chs
                    .iter()
                    .map(|c| {
                        Json::obj([
                            ("title", c.title.as_str().into()),
                            ("href", format!("/ebook/{}/epub/{}", id, pct_encode(&c.path, true)).into()),
                        ])
                    })
                    .collect();
                Ok(ok_json(Json::obj([("chapters", Json::Arr(list))]), ""))
            }
            Err(e) => Ok(Response::fail(&e)),
        }
    }

    fn search(&self, id: i64, q: &str) -> R {
        let Some(path) = self.ebook_path(id)? else { return Ok(Response::fail("E-kitap dosyası bulunamadı.")) };
        if q.trim().chars().count() < 2 {
            return Ok(Response::fail("Arama için en az 2 karakter yazın."));
        }
        let (hits, total) = match crate::epub::search(&path, q, 6) {
            Ok(v) => v,
            Err(e) => return Ok(Response::fail(&e)),
        };
        let results: Vec<Json> = hits
            .into_iter()
            .map(|h| {
                Json::obj([
                    ("chapter", h.chapter.into()),
                    ("title", h.title.into()),
                    ("count", h.count.into()),
                    (
                        "snippets",
                        Json::Arr(
                            h.snippets
                                .into_iter()
                                .map(|(a, b, c, n)| {
                                    Json::obj([("pre", a.into()), ("hit", b.into()), ("post", c.into()), ("n", n.into())])
                                })
                                .collect(),
                        ),
                    ),
                ])
            })
            .collect();
        Ok(ok_json(Json::obj([("total", total.into()), ("results", Json::Arr(results))]), ""))
    }

    // ---------------- Okuma takibi ----------------

    fn progress(&self, id: i64, req: &Request) -> R {
        let j = req.json();
        let db = self.db();
        let Some(b) = Self::book_row(&db, id)? else { return Ok(Response::fail("Kitap bulunamadı.")) };
        let today = Self::today(&db)?;
        let mut r = Reading {
            status: b.field_text("reading_status"),
            page: to_int(&b.field_text("current_page")),
            total: to_int(&b.field_text("page_count")),
            start: b.field_text("reading_start"),
            end: b.field_text("reading_end"),
        };
        let old_page = r.page;
        if let Some(t) = j.get("page_count").and_then(|v| v.as_i64()) {
            if t >= 0 {
                r.total = t;
            }
        }
        let status_given = j.get("status").map(|v| v.text()).filter(|s| !s.is_empty());
        if let Some(s) = &status_given {
            r.status = s.clone();
        }
        if let Some(p) = j.get("page").and_then(|v| v.as_i64()) {
            r.page = p.max(0);
            if p > 0 && status_given.is_none() && matches!(r.status.as_str(), "Okunmadı" | "Okunacak" | "Yarım Bırakıldı") {
                r.status = "Okunuyor".into();
            }
        }
        if let Some(v) = j.get("reading_start") {
            r.start = v.text();
        }
        if let Some(v) = j.get("reading_end") {
            r.end = v.text();
        }
        r.normalize(&today);
        let mut rating = to_int(&b.field_text("rating"));
        if let Some(v) = j.get("rating").and_then(|v| v.as_i64()) {
            rating = v.clamp(0, 5);
        }
        db.execute(
            "UPDATE books SET reading_status = ?, current_page = ?, page_count = ?, reading_start = ?, reading_end = ?, rating = ? WHERE id = ?",
            p![r.status.clone(), r.page, r.total, r.start.clone(), r.end.clone(), rating, id],
        )?;
        if r.should_log(&today) && r.page != old_page {
            Self::log_progress(&db, id, old_page, r.page, &r.start, &today)?;
        }
        let book = Self::book_row(&db, id)?.unwrap_or(Json::Null);
        Ok(ok_json(Json::obj([("book", book)]), "Okuma durumu güncellendi."))
    }

    fn book_notes(&self, id: i64) -> R {
        let t = self.db().query(
            "SELECT * FROM reading_notes WHERE book_id = ?
             ORDER BY CASE WHEN COALESCE(page,0) = 0 THEN 1 ELSE 0 END, page, id",
            p![id],
        )?;
        Ok(Response::json(&Json::Arr(t.to_json())))
    }

    fn note_fields(j: &Json) -> (String, Value, String, String) {
        let note = j.field_text("note").trim().to_string();
        let page = j.get("page").and_then(|v| v.as_i64()).filter(|p| *p > 0).map(Value::Int).unwrap_or(Value::Null);
        let mut kind = j.field_text("kind");
        if !NOTE_KINDS.contains(&kind.as_str()) {
            kind = "Not".into();
        }
        let loc: String = j.field_text("location").chars().take(100).collect();
        (note, page, kind, loc)
    }

    fn add_note(&self, id: i64, req: &Request) -> R {
        let (note, page, kind, loc) = Self::note_fields(&req.json());
        if note.is_empty() {
            return Ok(Response::fail("Not metni boş olamaz."));
        }
        let db = self.db();
        if db.query_i64("SELECT COUNT(*) FROM books WHERE id = ?", p![id])? == 0 {
            return Ok(Response::fail("Kitap bulunamadı."));
        }
        db.execute(
            "INSERT INTO reading_notes (book_id, page, kind, note, location, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, datetime('now','localtime'), datetime('now','localtime'))",
            &[Value::Int(id), page, Value::Text(kind), Value::Text(note), Value::Text(loc)],
        )?;
        Ok(ok_json(Json::obj([("id", db.last_id().into())]), "Not eklendi."))
    }

    fn update_note(&self, id: i64, req: &Request) -> R {
        let (note, page, kind, _) = Self::note_fields(&req.json());
        if note.is_empty() {
            return Ok(Response::fail("Not metni boş olamaz."));
        }
        let n = self.db().execute(
            "UPDATE reading_notes SET note = ?, page = ?, kind = ?, updated_at = datetime('now','localtime') WHERE id = ?",
            &[Value::Text(note), page, Value::Text(kind), Value::Int(id)],
        )?;
        Ok(if n > 0 { Response::ok("Not güncellendi.") } else { Response::fail("Not bulunamadı.") })
    }

    fn delete_note(&self, id: i64) -> R {
        self.db().execute("DELETE FROM reading_notes WHERE id = ?", p![id])?;
        Ok(Response::ok("Not silindi."))
    }

    fn all_notes(&self) -> R {
        let t = self.db().query(
            "SELECT n.*, b.title AS book_title, b.author AS book_author
             FROM reading_notes n JOIN books b ON b.id = n.book_id
             ORDER BY n.created_at DESC, n.id DESC LIMIT 2000",
            p![],
        )?;
        Ok(Response::json(&Json::Arr(t.to_json())))
    }

    fn book_log(&self, id: i64) -> R {
        let t = self.db().query("SELECT log_date, page FROM reading_log WHERE book_id = ? ORDER BY log_date", p![id])?;
        Ok(Response::json(&Json::Arr(t.to_json())))
    }

    fn reading_summary(&self) -> R {
        let db = self.db();
        let today = Self::today(&db)?;
        let td = parse_date(&today).unwrap_or(0);
        let year = today.get(..4).unwrap_or("").to_string();
        let goal = to_int(&Self::setting(&db, "reading_goal"));

        // günlük okunan sayfa = aynı kitabın bir önceki kaydına göre fark
        let logs = db.query("SELECT book_id, log_date, page FROM reading_log ORDER BY book_id, log_date", p![])?;
        let mut per_day: HashMap<i64, i64> = HashMap::new();
        let mut prev: HashMap<i64, i64> = HashMap::new();
        for r in &logs.rows {
            let (b, d, p) = (r[0].as_i64(), parse_date(&r[1].as_text()), r[2].as_i64());
            let before = prev.insert(b, p).unwrap_or(0);
            if let Some(d) = d {
                let delta = (p - before).max(0);
                *per_day.entry(d).or_insert(0) += delta;
            }
        }
        let pages_year: i64 =
            per_day.iter().filter(|(d, _)| fmt_date(**d).starts_with(&year)).map(|(_, v)| *v).sum();
        let days: Vec<Json> = (0..14)
            .rev()
            .map(|k| {
                let d = td - k;
                Json::obj([("date", fmt_date(d).into()), ("pages", (*per_day.get(&d).unwrap_or(&0)).into())])
            })
            .collect();
        let mut streak: i64 = 0;
        let mut d = if per_day.get(&td).copied().unwrap_or(0) > 0 { td } else { td - 1 };
        while per_day.get(&d).copied().unwrap_or(0) > 0 {
            streak += 1;
            d -= 1;
        }
        let q = |s: &str| db.query_i64(s, p![year.as_str()]);
        let finished_year = q("SELECT COUNT(*) FROM books WHERE reading_status='Okundu' AND substr(COALESCE(reading_end,''),1,4) = ?")?;
        let months_t = db.query(
            "SELECT CAST(substr(reading_end,6,2) AS INTEGER) AS m, COUNT(*) FROM books
             WHERE reading_status='Okundu' AND substr(COALESCE(reading_end,''),1,4) = ? GROUP BY m",
            p![year.as_str()],
        )?;
        let mut months = vec![0i64; 12];
        for r in &months_t.rows {
            let m = r[0].as_i64();
            if (1..=12).contains(&m) {
                months[(m - 1) as usize] = r[1].as_i64();
            }
        }
        let c = |s: &str| db.query_i64(s, p![]);
        let o = Json::obj([
            ("today", today.clone().into()),
            ("year", year.clone().into()),
            ("goal", goal.into()),
            ("finished_year", finished_year.into()),
            ("finished_total", c("SELECT COUNT(*) FROM books WHERE reading_status='Okundu'")?.into()),
            ("reading_now", c("SELECT COUNT(*) FROM books WHERE reading_status='Okunuyor'")?.into()),
            ("to_read", c("SELECT COUNT(*) FROM books WHERE reading_status='Okunacak'")?.into()),
            ("paused", c("SELECT COUNT(*) FROM books WHERE reading_status='Yarım Bırakıldı'")?.into()),
            ("notes_total", c("SELECT COUNT(*) FROM reading_notes")?.into()),
            (
                "pages_total",
                c("SELECT COALESCE(SUM(CASE WHEN reading_status='Okundu' THEN CAST(page_count AS INTEGER)
                                          WHEN reading_status IN ('Okunuyor','Yarım Bırakıldı') THEN CAST(current_page AS INTEGER)
                                          ELSE 0 END),0) FROM books")?
                .into(),
            ),
            ("pages_year", pages_year.into()),
            ("streak", streak.into()),
            ("days", Json::Arr(days)),
            ("months", Json::Arr(months.into_iter().map(Json::Int).collect())),
        ]);
        Ok(Response::json(&o))
    }

    // ---------------- Üyeler & ödünç ----------------

    fn list_members(&self) -> R {
        let t = self.db().query("SELECT * FROM members ORDER BY id DESC", p![])?;
        Ok(Response::json(&Json::Arr(t.to_json())))
    }

    fn add_member(&self, req: &Request) -> R {
        let j = req.json();
        let name = j.field_text("name").trim().to_string();
        if name.is_empty() {
            return Ok(Response::fail("Ad Soyad zorunludur."));
        }
        self.db().execute(
            "INSERT INTO members (name, phone, email, join_date) VALUES (?, ?, ?, date('now','localtime'))",
            p![name, j.field_text("phone").trim(), j.field_text("email").trim()],
        )?;
        Ok(Response::ok("Kaydedildi."))
    }

    fn delete_member(&self, id: i64) -> R {
        let db = self.db();
        let active = db.query_i64("SELECT COUNT(*) FROM loans WHERE member_id = ? AND status = 'Aktif'", p![id])?;
        if active > 0 {
            return Ok(Response::fail("Bu üyenin iade etmediği kitap var. Önce iade alınmalıdır."));
        }
        db.transaction(|d| {
            d.execute("DELETE FROM members WHERE id = ?", p![id])?;
            d.execute("DELETE FROM loans WHERE member_id = ?", p![id])?;
            Ok(())
        })?;
        Ok(Response::ok("Silindi."))
    }

    fn list_loans(&self) -> R {
        let t = self.db().query(
            "SELECT loans.id, books.title AS book_title, members.name AS member_name, loans.loan_date,
                    loans.due_date, loans.return_date, loans.status
             FROM loans JOIN books ON loans.book_id = books.id JOIN members ON loans.member_id = members.id
             ORDER BY CASE WHEN loans.status = 'Aktif' THEN 0 ELSE 1 END, loans.id DESC",
            p![],
        )?;
        Ok(Response::json(&Json::Arr(t.to_json())))
    }

    fn add_loan(&self, req: &Request) -> R {
        let j = req.json();
        let member = j.get("member_id").and_then(|v| v.as_i64()).unwrap_or(0);
        let book = j.get("book_id").and_then(|v| v.as_i64()).unwrap_or(0);
        let days = j.get("days").and_then(|v| v.as_i64()).unwrap_or(15).clamp(1, 3650);
        let db = self.db();
        if db.query_i64("SELECT COUNT(*) FROM members WHERE id = ?", p![member])? == 0 {
            return Ok(Response::fail("Lütfen bir üye seçin."));
        }
        let avail = db.query_value("SELECT available_copies FROM books WHERE id = ?", p![book])?;
        let Some(avail) = avail else { return Ok(Response::fail("Kitap bulunamadı.")) };
        if avail.as_i64() <= 0 {
            return Ok(Response::fail("Bu kitaptan stokta kalmamış."));
        }
        db.transaction(|d| {
            d.execute(
                "INSERT INTO loans (book_id, member_id, loan_date, due_date, status)
                 VALUES (?, ?, date('now','localtime'), date('now','localtime', ?), 'Aktif')",
                p![book, member, format!("+{} days", days)],
            )?;
            d.execute("UPDATE books SET available_copies = available_copies - 1 WHERE id = ?", p![book])?;
            Ok(())
        })?;
        Ok(Response::ok("Ödünç verildi."))
    }

    fn return_loan(&self, id: i64) -> R {
        let db = self.db();
        let book = db.query_value("SELECT book_id FROM loans WHERE id = ? AND status = 'Aktif'", p![id])?;
        let Some(book) = book else { return Ok(Response::fail("Aktif ödünç kaydı bulunamadı.")) };
        db.transaction(|d| {
            d.execute(
                "UPDATE loans SET status = 'İade Edildi', return_date = date('now','localtime') WHERE id = ?",
                p![id],
            )?;
            d.execute(
                "UPDATE books SET available_copies = MIN(available_copies + 1, total_copies) WHERE id = ?",
                p![book.as_i64()],
            )?;
            Ok(())
        })?;
        Ok(Response::ok("İade alındı."))
    }

    // ---------------- Yedek, dışa aktarma, birleştirme ----------------

    fn backup(&self) -> R {
        let db = self.db();
        let tmp = self.data_dir.join(format!("kutuphane_yedek_{}.tmp", now_millis()));
        let res = db.execute("VACUUM INTO ?", p![tmp.to_string_lossy().to_string()]);
        if let Err(e) = res {
            let _ = fs::remove_file(&tmp);
            return Ok(Response::fail(&format!("Yedek alınamadı: {}", e)));
        }
        let data = fs::read(&tmp).map_err(|e| format!("Yedek alınamadı: {}", e));
        let _ = fs::remove_file(&tmp);
        let today = Self::today(&db)?;
        Ok(Response::bytes(200, "application/octet-stream", data?)
            .attachment(&format!("kutuphane_yedek_{}.db", today)))
    }

    fn export_csv(&self) -> R {
        const COLS: &[(&str, &str)] = &[
            ("id", "ID"),
            ("title", "Kitap Adı"),
            ("original_title", "Özgün Ad"),
            ("author", "Yazar"),
            ("series_title", "Seri Başlığı"),
            ("publisher", "Yayınevi"),
            ("pub_place", "Yayım Yeri"),
            ("pub_year", "Yayım Yılı"),
            ("isbn", "ISBN"),
            ("language", "Eser Dili"),
            ("page_count", "Sayfa Sayısı"),
            ("dimensions", "Boyutlar"),
            ("gmt", "GMT"),
            ("content_type", "İçerik Tipi"),
            ("carrier_type", "Taşıyıcı Tipi"),
            ("category", "Kategori"),
            ("subject", "Konu"),
            ("classification", "Sınıflandırma"),
            ("shelf_num", "Raf Numarası"),
            ("inventory_code", "Envanter Kodu"),
            ("acquisition_date", "Alım Tarihi"),
            ("acquisition_type", "Alım Türü"),
            ("total_copies", "Toplam Kopya"),
            ("available_copies", "Mevcut Kopya"),
            ("library_source", "Kütüphane / Kaynak"),
            ("reading_status", "Okuma Durumu"),
            ("current_page", "Kaçıncı Sayfa"),
            ("progress", "İlerleme (%)"),
            ("reading_start", "Okumaya Başlama"),
            ("reading_end", "Bitirme Tarihi"),
            ("rating", "Puan"),
            ("note_count", "Okuma Notu Sayısı"),
            ("notes", "Açıklamalar"),
            ("ebook_file", "E-Kitap Dosyası"),
        ];
        let db = self.db();
        let t: Table = db.query(
            "SELECT b.*, (SELECT COUNT(*) FROM reading_notes n WHERE n.book_id = b.id) AS note_count FROM books b ORDER BY b.id",
            p![],
        )?;
        let cell = |s: &str| format!("\"{}\"", s.replace('"', "\"\""));
        let mut out = String::from("\u{feff}");
        out.push_str(&COLS.iter().map(|c| cell(c.1)).collect::<Vec<_>>().join(";"));
        out.push_str("\r\n");
        for row in &t.rows {
            let get = |k: &str| t.col(k).map(|i| row[i].as_text()).unwrap_or_default();
            let line: Vec<String> = COLS
                .iter()
                .map(|(k, _)| {
                    let v = match *k {
                        "progress" => {
                            let total = to_int(&get("page_count"));
                            let page = to_int(&get("current_page"));
                            if total > 0 {
                                ((page.min(total) * 100) / total).to_string()
                            } else {
                                String::new()
                            }
                        }
                        "ebook_file" => display_ebook_name(&get("ebook_file")),
                        "rating" => {
                            let r = to_int(&get("rating"));
                            if r > 0 {
                                r.to_string()
                            } else {
                                String::new()
                            }
                        }
                        _ => get(*k),
                    };
                    cell(&v)
                })
                .collect();
            out.push_str(&line.join(";"));
            out.push_str("\r\n");
        }
        let today = Self::today(&db)?;
        Ok(Response::bytes(200, "text/csv; charset=utf-8", out.into_bytes()).attachment(&format!("katalog_{}.csv", today)))
    }

    fn merge_db(&self, req: &Request) -> R {
        let parts = req.multipart();
        let Some(part) = parts.iter().find(|p| p.name == "db_file" && !p.data.is_empty()) else {
            return Ok(Response::fail("Dosya alınamadı."));
        };
        if !part.data.starts_with(b"SQLite format 3\0") {
            return Ok(Response::fail("Seçilen dosya geçerli bir SQLite veritabanı değil."));
        }
        let tmp = self.data_dir.join(format!("kutuphane_merge_{}.db", now_millis()));
        fs::write(&tmp, &part.data).map_err(|e| format!("Dosya kaydedilemedi: {}", e))?;
        let db = self.db();
        let res = self.merge_inner(&db, &tmp);
        let _ = db.execute("DETACH DATABASE ext", p![]);
        let _ = fs::remove_file(&tmp);
        Ok(match res {
            Ok(msg) => Response::ok(&msg),
            Err(e) => Response::fail(&format!("Veritabanı okunamadı: {}", e)),
        })
    }

    fn merge_inner(&self, db: &Db, tmp: &Path) -> Result<String, String> {
        db.execute("ATTACH DATABASE ? AS ext", p![tmp.to_string_lossy().to_string()])?;
        let ext_cols = db.table_columns("ext", "books")?;
        if ext_cols.is_empty() {
            return Err("Dosyada kitap tablosu bulunamadı.".into());
        }
        let main_cols = db.table_columns("main", "books")?;
        let has_settings =
            db.query_i64("SELECT COUNT(*) FROM ext.sqlite_master WHERE type='table' AND name='settings'", p![])? > 0;
        let ext_name = if has_settings {
            db.query_text("SELECT value FROM ext.settings WHERE key='library_name'", p![]).unwrap_or_default()
        } else {
            String::new()
        };
        let src = if ext_name.trim().is_empty() { "Harici Kütüphane".to_string() } else { ext_name.trim().to_string() };
        let src_expr = if ext_cols.iter().any(|c| c == "library_source") {
            "COALESCE(NULLIF(TRIM(e.library_source),''), ?1)"
        } else {
            "?1"
        };
        let mut ins = Vec::new();
        let mut sel = Vec::new();
        for c in &main_cols {
            match c.as_str() {
                "id" => {}
                "library_source" => {
                    ins.push(c.clone());
                    sel.push(src_expr.to_string());
                }
                "available_copies" => {
                    if ext_cols.iter().any(|x| x == "total_copies") {
                        ins.push(c.clone());
                        sel.push("COALESCE(e.total_copies, 1)".to_string());
                    }
                }
                _ if ext_cols.iter().any(|x| x == c) => {
                    ins.push(c.clone());
                    sel.push(format!("e.\"{}\"", c));
                }
                _ => {}
            }
        }
        let total_ext = db.query_i64("SELECT COUNT(*) FROM ext.books", p![])?;
        let max_before = db.query_i64("SELECT COALESCE(MAX(id),0) FROM main.books", p![])?;
        let sql = format!(
            "INSERT INTO main.books ({}) SELECT {} FROM ext.books e
             WHERE NOT EXISTS (SELECT 1 FROM main.books m WHERE lower(m.title) = lower(e.title)
                   AND lower(m.author) = lower(e.author) AND COALESCE(m.library_source,'') = {})
             ORDER BY e.id",
            ins.iter().map(|c| format!("\"{}\"", c)).collect::<Vec<_>>().join(", "),
            sel.join(", "),
            src_expr
        );
        let inserted = db.transaction(|d| {
            let n = d.execute(&sql, p![src.as_str()])?;
            // dosyası bu bilgisayarda olmayan kapak / e-kitap bağlantılarını temizle
            let t = d.query(
                "SELECT id, COALESCE(cover_image,''), COALESCE(ebook_file,'') FROM books WHERE id > ?",
                p![max_before],
            )?;
            for r in &t.rows {
                let (bid, cover, eb) = (r[0].as_i64(), r[1].as_text(), r[2].as_text());
                if !cover.is_empty() {
                    let ok = cover.strip_prefix("/uploads/").map(|n| safe_name(n) && self.upload_dir.join(n).exists());
                    if ok != Some(true) {
                        d.execute("UPDATE books SET cover_image = '' WHERE id = ?", p![bid])?;
                    }
                }
                if !eb.is_empty() && !(safe_name(&eb) && self.ebook_dir.join(&eb).exists()) {
                    d.execute("UPDATE books SET ebook_file = '' WHERE id = ?", p![bid])?;
                }
            }
            Ok(n)
        })?;
        let skipped = (total_ext - inserted as i64).max(0);
        let mut msg = format!("{} eser \"{}\" kaynağından aktarıldı.", inserted, src);
        if skipped > 0 {
            msg.push_str(&format!(" {} eser zaten kayıtlı olduğu için atlandı.", skipped));
        }
        Ok(msg)
    }
}

fn allowed_pref(k: &str) -> bool {
    const KEYS: [&str; 5] = ["reading_goal", "reader_fs", "pdf_auto", "reader_theme", "reader_width"];
    if KEYS.contains(&k) {
        return true;
    }
    for pre in ["epub_pos_", "pdf_pos_"] {
        if let Some(rest) = k.strip_prefix(pre) {
            return !rest.is_empty() && rest.len() < 12 && rest.chars().all(|c| c.is_ascii_digit());
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reading_rules() {
        let mut r = Reading { status: "Okundu".into(), page: 10, total: 300, start: "".into(), end: "".into() };
        r.normalize("2026-11-05");
        assert_eq!((r.page, r.end.as_str()), (300, "2026-11-05"));
        let mut r = Reading { status: "Okunuyor".into(), page: 500, total: 300, start: "x".into(), end: "2026-01-01".into() };
        r.normalize("2026-11-05");
        assert_eq!((r.page, r.start.as_str(), r.end.as_str()), (300, "2026-11-05", ""));
        let mut r = Reading { status: "Okunacak".into(), page: 50, total: 300, start: "2026-01-01".into(), end: "".into() };
        r.normalize("2026-11-05");
        assert_eq!((r.page, r.start.as_str()), (0, ""));
        assert_eq!(display_ebook_name("ekitap_1730000_Kürk Mantolu.pdf"), "Kürk Mantolu.pdf");
        assert!(allowed_pref("epub_pos_12") && !allowed_pref("epub_pos_x") && !allowed_pref("library"));
    }
}
