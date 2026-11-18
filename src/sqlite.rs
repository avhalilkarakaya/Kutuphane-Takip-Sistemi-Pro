// Kütüphane Takip Sistemi Pro — Copyright (C) 2026 Halil Karakaya
// SPDX-License-Identifier: AGPL-3.0-only

//! SQLite C arayüzü için ince, güvenli sarmalayıcı.
//! Windows derlemesinde SQLite kaynak kodu `libsqlite3-sys` (bundled) ile exe'ye gömülür;
//! burada yalnızca ihtiyaç duyulan fonksiyonlar bildirilir.

use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::ptr;

use crate::json::Json;

#[allow(non_camel_case_types)]
type sqlite3 = c_void;
#[allow(non_camel_case_types)]
type sqlite3_stmt = c_void;

const SQLITE_OK: c_int = 0;
const SQLITE_ROW: c_int = 100;
const SQLITE_DONE: c_int = 101;
const SQLITE_OPEN_READWRITE: c_int = 0x0000_0002;
const SQLITE_OPEN_CREATE: c_int = 0x0000_0004;
const SQLITE_OPEN_FULLMUTEX: c_int = 0x0001_0000;
const SQLITE_TRANSIENT: isize = -1;

extern "C" {
    fn sqlite3_open_v2(filename: *const c_char, pp_db: *mut *mut sqlite3, flags: c_int, z_vfs: *const c_char) -> c_int;
    fn sqlite3_close(db: *mut sqlite3) -> c_int;
    fn sqlite3_errmsg(db: *mut sqlite3) -> *const c_char;
    fn sqlite3_busy_timeout(db: *mut sqlite3, ms: c_int) -> c_int;
    fn sqlite3_prepare_v2(
        db: *mut sqlite3,
        sql: *const c_char,
        n_byte: c_int,
        pp_stmt: *mut *mut sqlite3_stmt,
        pz_tail: *mut *const c_char,
    ) -> c_int;
    fn sqlite3_bind_int64(st: *mut sqlite3_stmt, idx: c_int, v: i64) -> c_int;
    fn sqlite3_bind_double(st: *mut sqlite3_stmt, idx: c_int, v: f64) -> c_int;
    fn sqlite3_bind_text(st: *mut sqlite3_stmt, idx: c_int, v: *const c_char, n: c_int, destructor: isize) -> c_int;
    fn sqlite3_bind_null(st: *mut sqlite3_stmt, idx: c_int) -> c_int;
    fn sqlite3_step(st: *mut sqlite3_stmt) -> c_int;
    fn sqlite3_finalize(st: *mut sqlite3_stmt) -> c_int;
    fn sqlite3_column_count(st: *mut sqlite3_stmt) -> c_int;
    fn sqlite3_column_name(st: *mut sqlite3_stmt, i: c_int) -> *const c_char;
    fn sqlite3_column_type(st: *mut sqlite3_stmt, i: c_int) -> c_int;
    fn sqlite3_column_int64(st: *mut sqlite3_stmt, i: c_int) -> i64;
    fn sqlite3_column_double(st: *mut sqlite3_stmt, i: c_int) -> f64;
    fn sqlite3_column_text(st: *mut sqlite3_stmt, i: c_int) -> *const u8;
    fn sqlite3_column_bytes(st: *mut sqlite3_stmt, i: c_int) -> c_int;
    fn sqlite3_changes(db: *mut sqlite3) -> c_int;
    fn sqlite3_last_insert_rowid(db: *mut sqlite3) -> i64;
}

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Int(i64),
    Real(f64),
    Text(String),
}

impl Value {
    pub fn to_json(&self) -> Json {
        match self {
            Value::Null => Json::Null,
            Value::Int(i) => Json::Int(*i),
            Value::Real(f) => Json::Num(*f),
            Value::Text(s) => Json::Str(s.clone()),
        }
    }
    pub fn as_i64(&self) -> i64 {
        match self {
            Value::Int(i) => *i,
            Value::Real(f) => *f as i64,
            Value::Text(s) => s.trim().parse().unwrap_or(0),
            Value::Null => 0,
        }
    }
    pub fn as_text(&self) -> String {
        match self {
            Value::Null => String::new(),
            Value::Int(i) => i.to_string(),
            Value::Real(f) => f.to_string(),
            Value::Text(s) => s.clone(),
        }
    }
}

impl From<i64> for Value {
    fn from(v: i64) -> Self {
        Value::Int(v)
    }
}
impl From<&str> for Value {
    fn from(v: &str) -> Self {
        Value::Text(v.to_string())
    }
}
impl From<String> for Value {
    fn from(v: String) -> Self {
        Value::Text(v)
    }
}
impl From<&String> for Value {
    fn from(v: &String) -> Self {
        Value::Text(v.clone())
    }
}
impl From<f64> for Value {
    fn from(v: f64) -> Self {
        Value::Real(v)
    }
}

/// Sorgu parametreleri: `p![1, "metin", isim]`
#[macro_export]
macro_rules! p {
    () => { &[] as &[$crate::sqlite::Value] };
    ($($x:expr),+ $(,)?) => { &[$($crate::sqlite::Value::from($x)),+] as &[$crate::sqlite::Value] };
}

pub struct Table {
    pub cols: Vec<String>,
    pub rows: Vec<Vec<Value>>,
}

impl Table {
    pub fn col(&self, name: &str) -> Option<usize> {
        self.cols.iter().position(|c| c == name)
    }
    /// Satırları JSON nesneleri olarak döndürür.
    pub fn to_json(&self) -> Vec<Json> {
        self.rows
            .iter()
            .map(|r| Json::Obj(self.cols.iter().cloned().zip(r.iter().map(|v| v.to_json())).collect()))
            .collect()
    }
}

pub struct Db {
    db: *mut sqlite3,
}

// Bağlantı her zaman bir Mutex arkasında kullanılır.
unsafe impl Send for Db {}

impl Drop for Db {
    fn drop(&mut self) {
        unsafe {
            sqlite3_close(self.db);
        }
    }
}

struct Stmt(*mut sqlite3_stmt);
impl Drop for Stmt {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                sqlite3_finalize(self.0);
            }
        }
    }
}

impl Db {
    pub fn open(path: &std::path::Path) -> Result<Db, String> {
        let c = CString::new(path.to_string_lossy().as_bytes()).map_err(|e| e.to_string())?;
        let mut db = ptr::null_mut();
        let rc = unsafe {
            sqlite3_open_v2(
                c.as_ptr(),
                &mut db,
                SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE | SQLITE_OPEN_FULLMUTEX,
                ptr::null(),
            )
        };
        let conn = Db { db };
        if rc != SQLITE_OK {
            return Err(conn.err());
        }
        unsafe {
            sqlite3_busy_timeout(db, 5000);
        }
        Ok(conn)
    }

    fn err(&self) -> String {
        if self.db.is_null() {
            return "Veritabanı açılamadı".into();
        }
        unsafe { CStr::from_ptr(sqlite3_errmsg(self.db)).to_string_lossy().into_owned() }
    }

    fn prepare(&self, sql: &str, params: &[Value]) -> Result<Stmt, String> {
        let c = CString::new(sql).map_err(|e| e.to_string())?;
        let mut st = ptr::null_mut();
        let rc = unsafe { sqlite3_prepare_v2(self.db, c.as_ptr(), -1, &mut st, ptr::null_mut()) };
        if rc != SQLITE_OK {
            return Err(format!("{} [{}]", self.err(), sql.trim()));
        }
        let st = Stmt(st);
        for (i, p) in params.iter().enumerate() {
            let idx = (i + 1) as c_int;
            let rc = unsafe {
                match p {
                    Value::Null => sqlite3_bind_null(st.0, idx),
                    Value::Int(v) => sqlite3_bind_int64(st.0, idx, *v),
                    Value::Real(v) => sqlite3_bind_double(st.0, idx, *v),
                    Value::Text(s) => {
                        sqlite3_bind_text(st.0, idx, s.as_ptr() as *const c_char, s.len() as c_int, SQLITE_TRANSIENT)
                    }
                }
            };
            if rc != SQLITE_OK {
                return Err(self.err());
            }
        }
        Ok(st)
    }

    /// Satır döndürmeyen komut; etkilenen satır sayısını verir.
    pub fn execute(&self, sql: &str, params: &[Value]) -> Result<usize, String> {
        let st = self.prepare(sql, params)?;
        if st.0.is_null() {
            return Ok(0);
        }
        loop {
            match unsafe { sqlite3_step(st.0) } {
                SQLITE_ROW => continue,
                SQLITE_DONE => break,
                _ => return Err(self.err()),
            }
        }
        Ok(unsafe { sqlite3_changes(self.db) } as usize)
    }

    /// Noktalı virgülle ayrılmış birden çok komutu (parametresiz) çalıştırır.
    pub fn exec_batch(&self, sql: &str) -> Result<(), String> {
        for part in split_sql(sql) {
            self.execute(&part, &[])?;
        }
        Ok(())
    }

    pub fn query(&self, sql: &str, params: &[Value]) -> Result<Table, String> {
        let st = self.prepare(sql, params)?;
        let mut t = Table { cols: Vec::new(), rows: Vec::new() };
        if st.0.is_null() {
            return Ok(t);
        }
        let n = unsafe { sqlite3_column_count(st.0) };
        for i in 0..n {
            let name = unsafe { CStr::from_ptr(sqlite3_column_name(st.0, i)) };
            t.cols.push(name.to_string_lossy().into_owned());
        }
        loop {
            match unsafe { sqlite3_step(st.0) } {
                SQLITE_ROW => {
                    let mut row = Vec::with_capacity(n as usize);
                    for i in 0..n {
                        let v = unsafe {
                            match sqlite3_column_type(st.0, i) {
                                1 => Value::Int(sqlite3_column_int64(st.0, i)),
                                2 => Value::Real(sqlite3_column_double(st.0, i)),
                                3 => {
                                    let p = sqlite3_column_text(st.0, i);
                                    let len = sqlite3_column_bytes(st.0, i) as usize;
                                    if p.is_null() {
                                        Value::Text(String::new())
                                    } else {
                                        let b = std::slice::from_raw_parts(p, len);
                                        Value::Text(String::from_utf8_lossy(b).into_owned())
                                    }
                                }
                                _ => Value::Null,
                            }
                        };
                        row.push(v);
                    }
                    t.rows.push(row);
                }
                SQLITE_DONE => break,
                _ => return Err(self.err()),
            }
        }
        Ok(t)
    }

    pub fn query_value(&self, sql: &str, params: &[Value]) -> Result<Option<Value>, String> {
        let t = self.query(sql, params)?;
        Ok(t.rows.into_iter().next().and_then(|r| r.into_iter().next()))
    }

    pub fn query_i64(&self, sql: &str, params: &[Value]) -> Result<i64, String> {
        Ok(self.query_value(sql, params)?.map(|v| v.as_i64()).unwrap_or(0))
    }

    pub fn query_text(&self, sql: &str, params: &[Value]) -> Result<String, String> {
        Ok(self.query_value(sql, params)?.map(|v| v.as_text()).unwrap_or_default())
    }

    pub fn last_id(&self) -> i64 {
        unsafe { sqlite3_last_insert_rowid(self.db) }
    }

    /// İşlem (transaction) içinde çalıştırır; hata olursa geri alır.
    pub fn transaction<T>(&self, f: impl FnOnce(&Db) -> Result<T, String>) -> Result<T, String> {
        self.execute("BEGIN", &[])?;
        match f(self) {
            Ok(v) => {
                self.execute("COMMIT", &[])?;
                Ok(v)
            }
            Err(e) => {
                let _ = self.execute("ROLLBACK", &[]);
                Err(e)
            }
        }
    }

    pub fn table_columns(&self, schema: &str, table: &str) -> Result<Vec<String>, String> {
        let t = self.query(&format!("PRAGMA {}.table_info({})", schema, table), &[])?;
        let idx = t.col("name").unwrap_or(1);
        Ok(t.rows.iter().map(|r| r[idx].as_text()).collect())
    }
}

/// Basit SQL bölücü: tırnak içindeki noktalı virgülleri dikkate alır.
fn split_sql(sql: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    for c in sql.chars() {
        match quote {
            Some(q) => {
                cur.push(c);
                if c == q {
                    quote = None;
                }
            }
            None => {
                if c == ';' {
                    if !cur.trim().is_empty() {
                        out.push(cur.trim().to_string());
                    }
                    cur.clear();
                } else {
                    if c == '\'' || c == '"' {
                        quote = Some(c);
                    }
                    cur.push(c);
                }
            }
        }
    }
    if !cur.trim().is_empty() {
        out.push(cur.trim().to_string());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn basic() {
        let p = std::env::temp_dir().join(format!("kys_test_{}.db", std::process::id()));
        let _ = std::fs::remove_file(&p);
        let db = Db::open(&p).unwrap();
        db.exec_batch("CREATE TABLE t (id INTEGER PRIMARY KEY, a TEXT, b INTEGER); INSERT INTO t(a,b) VALUES ('x;y', 3);")
            .unwrap();
        db.execute("INSERT INTO t(a,b) VALUES (?, ?)", p!["Şüheda ğ", 5]).unwrap();
        assert_eq!(db.last_id(), 2);
        let t = db.query("SELECT * FROM t ORDER BY id", p![]).unwrap();
        assert_eq!(t.rows.len(), 2);
        assert_eq!(t.rows[0][1], Value::Text("x;y".into()));
        assert_eq!(t.rows[1][1], Value::Text("Şüheda ğ".into()));
        assert_eq!(db.query_i64("SELECT SUM(b) FROM t", p![]).unwrap(), 8);
        let r: Result<(), String> = db.transaction(|d| {
            d.execute("DELETE FROM t", p![])?;
            Err("iptal".into())
        });
        assert!(r.is_err());
        assert_eq!(db.query_i64("SELECT COUNT(*) FROM t", p![]).unwrap(), 2);
        assert!(db.execute("SELECT * FROM yok", p![]).is_err());
        drop(db);
        let _ = std::fs::remove_file(&p);
    }
}
