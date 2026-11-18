// Kütüphane Takip Sistemi Pro — Copyright (C) 2026 Halil Karakaya
// SPDX-License-Identifier: AGPL-3.0-only

//! Yalnızca 127.0.0.1'e bağlanan, bağımlılıksız küçük HTTP/1.1 sunucusu.

use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::time::Duration;

use crate::json::Json;
use crate::util::{pct_decode, pct_encode};

const MAX_BODY: usize = 1024 * 1024 * 1024; // 1 GB

pub struct Request {
    pub method: String,
    pub path: String,
    pub query: Vec<(String, String)>,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
    pub fn q(&self, name: &str) -> Option<&str> {
        self.query.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }
    pub fn json(&self) -> Json {
        Json::parse(&String::from_utf8_lossy(&self.body)).unwrap_or(Json::Obj(Vec::new()))
    }
    pub fn multipart(&self) -> Vec<Part> {
        let ct = self.header("content-type").unwrap_or("");
        match boundary_of(ct) {
            Some(b) => parse_multipart(&self.body, &b),
            None => Vec::new(),
        }
    }
}

pub enum Body {
    Bytes(Vec<u8>),
    File(File, u64),
}

pub struct Response {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Body,
}

impl Response {
    pub fn bytes(status: u16, ctype: &str, data: Vec<u8>) -> Response {
        Response { status, headers: vec![("Content-Type".into(), ctype.into())], body: Body::Bytes(data) }
    }
    pub fn json(v: &Json) -> Response {
        Response::bytes(200, "application/json; charset=utf-8", v.to_string().into_bytes())
            .header("Cache-Control", "no-store")
    }
    pub fn ok(msg: &str) -> Response {
        Response::json(&Json::obj([("success", true.into()), ("message", msg.into())]))
    }
    pub fn fail(msg: &str) -> Response {
        Response::json(&Json::obj([("success", false.into()), ("message", msg.into())]))
    }
    pub fn text(status: u16, msg: &str) -> Response {
        Response::bytes(status, "text/plain; charset=utf-8", msg.as_bytes().to_vec())
    }
    pub fn not_found() -> Response {
        Response::text(404, "404 sayfa bulunamadı")
    }
    pub fn file(path: &std::path::Path, ctype: &str) -> Response {
        match File::open(path) {
            Ok(f) => {
                let len = f.metadata().map(|m| m.len()).unwrap_or(0);
                Response { status: 200, headers: vec![("Content-Type".into(), ctype.into())], body: Body::File(f, len) }
            }
            Err(_) => Response::not_found(),
        }
    }
    pub fn header(mut self, k: &str, v: &str) -> Response {
        self.headers.push((k.into(), v.into()));
        self
    }
    /// Türkçe karakterli dosya adları için RFC 5987 uyumlu indirme başlığı.
    pub fn attachment(self, filename: &str) -> Response {
        let ascii: String = filename.chars().map(|c| if c.is_ascii() && c != '"' && c != '\\' { c } else { '_' }).collect();
        let v = format!("attachment; filename=\"{}\"; filename*=UTF-8''{}", ascii, pct_encode(filename, false));
        self.header("Content-Disposition", &v)
    }
}

pub type Handler = dyn Fn(Request) -> Response + Send + Sync;

pub fn serve(listener: TcpListener, port: u16, handler: Arc<Handler>) {
    for stream in listener.incoming() {
        if let Ok(s) = stream {
            let h = handler.clone();
            std::thread::spawn(move || {
                let _ = handle_conn(s, port, &*h);
            });
        }
    }
}

fn handle_conn(mut stream: TcpStream, port: u16, h: &Handler) -> io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(120)))?;
    let _ = stream.set_nodelay(true);
    let mut reader = BufReader::new(stream.try_clone()?);
    loop {
        let req = match read_request(&mut reader) {
            Ok(Some(r)) => r,
            Ok(None) => return Ok(()),
            Err(e) => {
                let _ = write_response(&mut stream, Response::text(400, &e.to_string()), false, false);
                return Ok(());
            }
        };
        let keep = !req.header("connection").map(|v| v.eq_ignore_ascii_case("close")).unwrap_or(false);
        let head_only = req.method == "HEAD";
        // DNS yeniden bağlama saldırılarına karşı yalnızca yerel Host başlıklarını kabul et
        let host_ok = match req.header("host") {
            Some(hst) => hst == format!("127.0.0.1:{}", port) || hst == format!("localhost:{}", port),
            None => false,
        };
        let resp = if !host_ok {
            Response::text(403, "Yasak")
        } else {
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| h(req))) {
                Ok(r) => r,
                Err(_) => Response::fail("Beklenmeyen bir hata oluştu."),
            }
        };
        write_response(&mut stream, resp, keep, head_only)?;
        if !keep {
            return Ok(());
        }
    }
}

fn read_line_limited(r: &mut BufReader<TcpStream>, limit: usize) -> io::Result<Option<String>> {
    let mut buf = Vec::new();
    let n = r.by_ref().take(limit as u64).read_until(b'\n', &mut buf)?;
    if n == 0 {
        return Ok(None);
    }
    if !buf.ends_with(b"\n") {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "Başlık satırı çok uzun"));
    }
    while buf.last() == Some(&b'\n') || buf.last() == Some(&b'\r') {
        buf.pop();
    }
    Ok(Some(String::from_utf8_lossy(&buf).into_owned()))
}

fn read_request(r: &mut BufReader<TcpStream>) -> io::Result<Option<Request>> {
    let line = loop {
        match read_line_limited(r, 16 * 1024)? {
            None => return Ok(None),
            Some(l) if l.is_empty() => continue,
            Some(l) => break l,
        }
    };
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or("").to_ascii_uppercase();
    let target = parts.next().unwrap_or("/").to_string();
    let mut headers = Vec::new();
    loop {
        let l = read_line_limited(r, 16 * 1024)?
            .ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "Eksik istek"))?;
        if l.is_empty() {
            break;
        }
        if let Some((k, v)) = l.split_once(':') {
            headers.push((k.trim().to_string(), v.trim().to_string()));
        }
        if headers.len() > 200 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "Çok fazla başlık"));
        }
    }
    let hdr = |n: &str| headers.iter().find(|(k, _): &&(String, String)| k.eq_ignore_ascii_case(n)).map(|(_, v)| v.clone());
    let mut body = Vec::new();
    if hdr("transfer-encoding").map(|v| v.to_ascii_lowercase().contains("chunked")).unwrap_or(false) {
        loop {
            let size_line = read_line_limited(r, 1024)?.unwrap_or_default();
            let size = usize::from_str_radix(size_line.split(';').next().unwrap_or("0").trim(), 16).unwrap_or(0);
            if size == 0 {
                // son satırları tüket
                while let Some(l) = read_line_limited(r, 16 * 1024)? {
                    if l.is_empty() {
                        break;
                    }
                }
                break;
            }
            if body.len() + size > MAX_BODY {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "İstek çok büyük"));
            }
            let st = body.len();
            body.resize(st + size, 0);
            r.read_exact(&mut body[st..])?;
            let _ = read_line_limited(r, 16)?;
        }
    } else if let Some(cl) = hdr("content-length") {
        let n: usize = cl.trim().parse().map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "Hatalı uzunluk"))?;
        if n > MAX_BODY {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "İstek çok büyük"));
        }
        body.resize(n, 0);
        r.read_exact(&mut body)?;
    }
    let (p, qs) = match target.split_once('?') {
        Some((a, b)) => (a.to_string(), b.to_string()),
        None => (target.clone(), String::new()),
    };
    let query = qs
        .split('&')
        .filter(|s| !s.is_empty())
        .map(|kv| match kv.split_once('=') {
            Some((k, v)) => (pct_decode(k, true), pct_decode(v, true)),
            None => (pct_decode(kv, true), String::new()),
        })
        .collect();
    Ok(Some(Request { method, path: p, query, headers, body }))
}

fn reason(code: u16) -> &'static str {
    match code {
        200 => "OK",
        206 => "Partial Content",
        304 => "Not Modified",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        _ => "Internal Server Error",
    }
}

fn write_response(s: &mut TcpStream, resp: Response, keep: bool, head_only: bool) -> io::Result<()> {
    let len = match &resp.body {
        Body::Bytes(b) => b.len() as u64,
        Body::File(_, l) => *l,
    };
    let mut head = format!("HTTP/1.1 {} {}\r\n", resp.status, reason(resp.status));
    for (k, v) in &resp.headers {
        head.push_str(&format!("{}: {}\r\n", k, v));
    }
    head.push_str(&format!(
        "Content-Length: {}\r\nX-Content-Type-Options: nosniff\r\nConnection: {}\r\n\r\n",
        len,
        if keep { "keep-alive" } else { "close" }
    ));
    s.write_all(head.as_bytes())?;
    if !head_only {
        match resp.body {
            Body::Bytes(b) => s.write_all(&b)?,
            Body::File(f, l) => {
                io::copy(&mut f.take(l), s)?;
            }
        }
    }
    s.flush()
}

// ---------------- multipart/form-data ----------------

#[derive(Debug)]
pub struct Part {
    pub name: String,
    pub filename: Option<String>,
    pub data: Vec<u8>,
}

impl Part {
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.data).into_owned()
    }
}

pub fn boundary_of(ct: &str) -> Option<String> {
    if !ct.to_ascii_lowercase().starts_with("multipart/") {
        return None;
    }
    for p in ct.split(';') {
        let p = p.trim();
        if p.len() > 9 && p[..9].eq_ignore_ascii_case("boundary=") {
            return Some(p[9..].trim_matches('"').to_string());
        }
    }
    None
}

fn find(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if needle.is_empty() || from >= hay.len() {
        return None;
    }
    let first = needle[0];
    let mut i = from;
    while i + needle.len() <= hay.len() {
        match hay[i..hay.len() - needle.len() + 1].iter().position(|&b| b == first) {
            None => return None,
            Some(off) => {
                i += off;
                if &hay[i..i + needle.len()] == needle {
                    return Some(i);
                }
                i += 1;
            }
        }
    }
    None
}

/// Content-Disposition başlığındaki parametreleri tırnakları dikkate alarak ayrıştırır.
fn disposition_params(v: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let chars: Vec<char> = v.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        // anahtar
        while i < chars.len() && (chars[i] == ';' || chars[i].is_whitespace()) {
            i += 1;
        }
        let ks = i;
        while i < chars.len() && chars[i] != '=' && chars[i] != ';' {
            i += 1;
        }
        let key: String = chars[ks..i].iter().collect::<String>().trim().to_ascii_lowercase();
        let mut val = String::new();
        if i < chars.len() && chars[i] == '=' {
            i += 1;
            if i < chars.len() && chars[i] == '"' {
                i += 1;
                while i < chars.len() && chars[i] != '"' {
                    if chars[i] == '\\' && i + 1 < chars.len() {
                        i += 1;
                    }
                    val.push(chars[i]);
                    i += 1;
                }
                i += 1;
            } else {
                while i < chars.len() && chars[i] != ';' {
                    val.push(chars[i]);
                    i += 1;
                }
                val = val.trim().to_string();
            }
        }
        if !key.is_empty() {
            out.push((key, val));
        }
    }
    out
}

pub fn parse_multipart(body: &[u8], boundary: &str) -> Vec<Part> {
    let mut parts = Vec::new();
    let delim = format!("--{}", boundary).into_bytes();
    let mut pos = match find(body, &delim, 0) {
        Some(p) => p + delim.len(),
        None => return parts,
    };
    let crlf_delim = [b"\r\n".as_slice(), &delim].concat();
    loop {
        if body[pos..].starts_with(b"--") {
            break;
        }
        if body[pos..].starts_with(b"\r\n") {
            pos += 2;
        }
        let next = match find(body, &crlf_delim, pos) {
            Some(n) => n,
            None => break,
        };
        let seg = &body[pos..next];
        if let Some(h) = find(seg, b"\r\n\r\n", 0) {
            let head = String::from_utf8_lossy(&seg[..h]).into_owned();
            let data = seg[h + 4..].to_vec();
            let mut name = String::new();
            let mut filename = None;
            for line in head.split("\r\n") {
                if let Some((k, v)) = line.split_once(':') {
                    if k.trim().eq_ignore_ascii_case("content-disposition") {
                        for (pk, pv) in disposition_params(v) {
                            match pk.as_str() {
                                "name" => name = pv,
                                "filename" => filename = Some(pct_decode_quotes(&pv)),
                                _ => {}
                            }
                        }
                    }
                }
            }
            parts.push(Part { name, filename, data });
        }
        pos = next + crlf_delim.len();
        if pos >= body.len() {
            break;
        }
    }
    parts
}

/// Chromium, dosya adındaki " karakterini %22 olarak gönderir.
fn pct_decode_quotes(s: &str) -> String {
    s.replace("%22", "\"").replace("%0D", "\r").replace("%0A", "\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn multipart() {
        let body = b"------B\r\nContent-Disposition: form-data; name=\"title\"\r\n\r\nK\xc3\xbcrk Mantolu\r\n------B\r\nContent-Disposition: form-data; name=\"ebook_file\"; filename=\"a;b \xc3\xa7.pdf\"\r\nContent-Type: application/pdf\r\n\r\n%PDF\r\n--x\r\n\r\n------B\r\nContent-Disposition: form-data; name=\"empty\"\r\n\r\n\r\n------B--\r\n";
        let parts = parse_multipart(body, "----B");
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[0].name, "title");
        assert_eq!(parts[0].text(), "Kürk Mantolu");
        assert_eq!(parts[1].filename.as_deref(), Some("a;b ç.pdf"));
        assert_eq!(parts[1].data, b"%PDF\r\n--x\r\n");
        assert_eq!(parts[2].name, "empty");
        assert_eq!(parts[2].data, b"");
        assert_eq!(boundary_of("multipart/form-data; boundary=----B").as_deref(), Some("----B"));
    }
}
