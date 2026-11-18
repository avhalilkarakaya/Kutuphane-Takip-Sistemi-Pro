//! Yardımcı işlevler: Türkçe arama katlaması, metin/HTML işlemleri, tarih hesapları.

use std::time::{SystemTime, UNIX_EPOCH};

/// Aramada kullanılan karakter katlaması: büyük/küçük harf ve Türkçe karakter duyarsız.
/// Her karakter tek bir karaktere eşlenir (konum hesapları bozulmasın diye).
/// JavaScript tarafındaki `fold()` ile birebir aynıdır.
pub fn fold_char(c: char) -> char {
    let c = match c {
        'İ' | 'I' | 'ı' => return 'i',
        _ => {
            let mut l = c.to_lowercase();
            match (l.next(), l.next()) {
                (Some(x), None) => x,
                _ => c,
            }
        }
    };
    match c {
        'ş' => 's',
        'ç' => 'c',
        'ğ' => 'g',
        'ö' | 'ô' | 'ò' | 'ó' | 'õ' => 'o',
        'ü' | 'û' | 'ù' | 'ú' => 'u',
        'â' | 'à' | 'á' | 'ä' | 'ã' | 'å' => 'a',
        'î' | 'ì' | 'í' | 'ï' => 'i',
        'é' | 'è' | 'ê' | 'ë' => 'e',
        'ñ' => 'n',
        '\u{00a0}' | '\t' | '\n' | '\r' => ' ',
        '’' | '‘' | 'ʼ' => '\'',
        x => x,
    }
}

pub fn fold(s: &str) -> String {
    s.chars().map(fold_char).collect()
}

pub fn now_millis() -> u128 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0)
}

/// Dosya adından Windows'ta geçersiz karakterleri temizler.
pub fn sanitize_filename(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let mut s: String = base
        .chars()
        .map(|c| if c.is_control() || "/\\:*?\"<>|".contains(c) { '_' } else { c })
        .collect();
    s = s.trim().trim_matches('.').to_string();
    if s.chars().count() > 120 {
        let ext = extension(&s);
        let stem: String = s.chars().take(100).collect();
        s = if ext.is_empty() { stem } else { format!("{}.{}", stem, ext) };
    }
    if s.is_empty() {
        "dosya".into()
    } else {
        s
    }
}

pub fn extension(name: &str) -> String {
    match name.rsplit_once('.') {
        Some((_, e)) if !e.is_empty() && e.len() <= 8 && !e.contains(['/', '\\']) => e.to_ascii_lowercase(),
        _ => String::new(),
    }
}

/// URL / başlık için yüzde kodlama (harf, rakam ve -._~ dışında her şey).
pub fn pct_encode(s: &str, keep_slash: bool) -> String {
    let mut o = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) || (keep_slash && b == b'/') {
            o.push(b as char);
        } else {
            o.push_str(&format!("%{:02X}", b));
        }
    }
    o
}

pub fn pct_decode(s: &str, plus_space: bool) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' if i + 2 < b.len() => {
                let h = std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok());
                match h {
                    Some(v) => {
                        out.push(v);
                        i += 3;
                    }
                    None => {
                        out.push(b'%');
                        i += 1;
                    }
                }
            }
            b'+' if plus_space => {
                out.push(b' ');
                i += 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(pos) = rest.find('&') {
        out.push_str(&rest[..pos]);
        rest = &rest[pos..];
        let end = rest[..rest.len().min(12)].find(';');
        let mut done = false;
        if let Some(e) = end {
            let ent = &rest[1..e];
            let ch = match ent {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                "nbsp" => Some('\u{00a0}'),
                "shy" => Some('\u{00ad}'),
                "mdash" => Some('—'),
                "ndash" => Some('–'),
                "hellip" => Some('…'),
                "laquo" => Some('«'),
                "raquo" => Some('»'),
                "rsquo" => Some('’'),
                "lsquo" => Some('‘'),
                "rdquo" => Some('”'),
                "ldquo" => Some('“'),
                _ if ent.starts_with("#x") || ent.starts_with("#X") => {
                    u32::from_str_radix(&ent[2..], 16).ok().and_then(char::from_u32)
                }
                _ if ent.starts_with('#') => ent[1..].parse::<u32>().ok().and_then(char::from_u32),
                _ => None,
            };
            if let Some(c) = ch {
                out.push(c);
                rest = &rest[e + 1..];
                done = true;
            }
        }
        if !done {
            out.push('&');
            rest = &rest[1..];
        }
    }
    out.push_str(rest);
    out
}

// ---------------- Basit XML/HTML belirteçleyici ----------------

#[derive(Debug)]
pub enum Tok {
    Start { name: String, attrs: Vec<(String, String)>, self_closing: bool },
    End { name: String },
    Text(String),
}

fn local_lower(name: &str) -> String {
    name.rsplit(':').next().unwrap_or(name).to_ascii_lowercase()
}

/// Hatalara dayanıklı belirteçleyici (EPUB OPF/NCX/XHTML için yeterli).
pub fn tokenize(s: &str) -> Vec<Tok> {
    let mut toks = Vec::new();
    let mut i = 0;
    let b = s.as_bytes();
    while i < b.len() {
        if b[i] == b'<' {
            if s[i..].starts_with("<!--") {
                i = s[i..].find("-->").map(|p| i + p + 3).unwrap_or(b.len());
                continue;
            }
            if s[i..].starts_with("<![CDATA[") {
                let end = s[i..].find("]]>").map(|p| i + p).unwrap_or(b.len());
                toks.push(Tok::Text(s[i + 9..end].to_string()));
                i = (end + 3).min(b.len());
                continue;
            }
            if s[i..].starts_with("<?") || s[i..].starts_with("<!") {
                i = s[i..].find('>').map(|p| i + p + 1).unwrap_or(b.len());
                continue;
            }
            // etiket sonunu bul (tırnaklara dikkat ederek)
            let mut j = i + 1;
            let mut q: u8 = 0;
            while j < b.len() {
                let c = b[j];
                if q != 0 {
                    if c == q {
                        q = 0;
                    }
                } else if c == b'"' || c == b'\'' {
                    q = c;
                } else if c == b'>' {
                    break;
                }
                j += 1;
            }
            let inner = &s[i + 1..j.min(b.len())];
            i = (j + 1).min(b.len());
            if let Some(rest) = inner.strip_prefix('/') {
                toks.push(Tok::End { name: local_lower(rest.trim()) });
                continue;
            }
            let self_closing = inner.trim_end().ends_with('/');
            let inner = inner.trim_end().trim_end_matches('/');
            let name_end = inner.find(|c: char| c.is_whitespace()).unwrap_or(inner.len());
            let name = local_lower(&inner[..name_end]);
            if name.is_empty() {
                toks.push(Tok::Text("<".into()));
                continue;
            }
            toks.push(Tok::Start { name, attrs: parse_attrs(&inner[name_end..]), self_closing });
        } else {
            let end = s[i..].find('<').map(|p| i + p).unwrap_or(b.len());
            toks.push(Tok::Text(decode_entities(&s[i..end])));
            i = end;
        }
    }
    toks
}

fn parse_attrs(s: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let c: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < c.len() {
        while i < c.len() && c[i].is_whitespace() {
            i += 1;
        }
        let st = i;
        while i < c.len() && !c[i].is_whitespace() && c[i] != '=' {
            i += 1;
        }
        if st == i {
            i += 1;
            continue;
        }
        let name: String = c[st..i].iter().collect();
        while i < c.len() && c[i].is_whitespace() {
            i += 1;
        }
        let mut val = String::new();
        if i < c.len() && c[i] == '=' {
            i += 1;
            while i < c.len() && c[i].is_whitespace() {
                i += 1;
            }
            if i < c.len() && (c[i] == '"' || c[i] == '\'') {
                let q = c[i];
                i += 1;
                let vs = i;
                while i < c.len() && c[i] != q {
                    i += 1;
                }
                val = c[vs..i].iter().collect();
                i += 1;
            } else {
                let vs = i;
                while i < c.len() && !c[i].is_whitespace() {
                    i += 1;
                }
                val = c[vs..i].iter().collect();
            }
        }
        out.push((local_lower(&name), decode_entities(&val)));
    }
    out
}

pub fn attr<'a>(attrs: &'a [(String, String)], name: &str) -> Option<&'a str> {
    attrs.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
}

const BLOCK_TAGS: &[&str] = &[
    "p", "div", "br", "h1", "h2", "h3", "h4", "h5", "h6", "li", "ul", "ol", "tr", "td", "th", "table", "section",
    "article", "blockquote", "pre", "hr", "dt", "dd", "figcaption", "aside", "header", "footer", "body",
];

/// XHTML belgesinden düz metin çıkarır (head/script/style hariç).
pub fn html_to_text(html: &str) -> String {
    let mut out = String::new();
    let mut skip = 0usize;
    for t in tokenize(html) {
        match t {
            Tok::Start { name, self_closing, .. } => {
                if matches!(name.as_str(), "head" | "script" | "style") && !self_closing {
                    skip += 1;
                } else if BLOCK_TAGS.contains(&name.as_str()) {
                    out.push(' ');
                }
            }
            Tok::End { name } => {
                if matches!(name.as_str(), "head" | "script" | "style") {
                    skip = skip.saturating_sub(1);
                } else if BLOCK_TAGS.contains(&name.as_str()) {
                    out.push(' ');
                }
            }
            Tok::Text(x) if skip == 0 => out.push_str(&x),
            _ => {}
        }
    }
    // boşlukları sadeleştir
    let mut res = String::with_capacity(out.len());
    let mut sp = true;
    for ch in out.chars() {
        if ch.is_whitespace() || ch == '\u{00ad}' {
            if ch != '\u{00ad}' && !sp {
                res.push(' ');
                sp = true;
            }
        } else {
            res.push(ch);
            sp = false;
        }
    }
    res.trim_end().to_string()
}

/// Metin içinde (katlanmış) sorgunun tüm geçtiği yerleri karakter konumu olarak bulur.
pub fn find_all(text_chars_folded: &[char], q: &[char]) -> Vec<usize> {
    let mut v = Vec::new();
    if q.is_empty() || q.len() > text_chars_folded.len() {
        return v;
    }
    let mut i = 0;
    while i + q.len() <= text_chars_folded.len() {
        if text_chars_folded[i] == q[0] && &text_chars_folded[i..i + q.len()] == q {
            v.push(i);
            i += q.len();
        } else {
            i += 1;
        }
    }
    v
}

pub fn snippet(chars: &[char], pos: usize, len: usize, ctx: usize) -> (String, String, String) {
    let st = pos.saturating_sub(ctx);
    let en = (pos + len + ctx).min(chars.len());
    let mut pre: String = chars[st..pos].iter().collect();
    let hit: String = chars[pos..pos + len].iter().collect();
    let mut post: String = chars[pos + len..en].iter().collect();
    if st > 0 {
        pre = format!("…{}", pre.trim_start());
    }
    if en < chars.len() {
        post = format!("{}…", post.trim_end());
    }
    (pre, hit, post)
}

// ---------------- Tarih hesapları (YYYY-MM-DD) ----------------

pub fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

pub fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

pub fn parse_date(s: &str) -> Option<i64> {
    let s = s.get(..10)?;
    let mut it = s.split('-');
    let y = it.next()?.parse().ok()?;
    let m = it.next()?.parse().ok()?;
    let d = it.next()?.parse().ok()?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    Some(days_from_civil(y, m, d))
}

pub fn fmt_date(days: i64) -> String {
    let (y, m, d) = civil_from_days(days);
    format!("{:04}-{:02}-{:02}", y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn folding() {
        assert_eq!(fold("İSTANBUL Işık ŞÜKRÜ Çağ"), "istanbul isik sukru cag");
        assert_eq!(fold("Sözleşme").chars().count(), "Sözleşme".chars().count());
    }
    #[test]
    fn pct() {
        assert_eq!(pct_decode("a%20b%C3%A7+c", true), "a bç c");
        assert_eq!(pct_decode("100%", false), "100%");
        assert_eq!(pct_decode("%4", false), "%4");
        assert_eq!(pct_encode("Ödev 1.pdf", false), "%C3%96dev%201.pdf");
    }
    #[test]
    fn html() {
        let t = html_to_text("<html><head><title>X</title><style>p{}</style></head><body><p>Merhaba&nbsp;<b>dün</b>ya</p><p>İkinci &amp; son</p><script>x()</script></body></html>");
        assert_eq!(t.trim(), "Merhaba dünya İkinci & son");
        let toks = tokenize(r#"<opf:item id="a" href="Text/b%20c.xhtml" media-type='application/xhtml+xml'/>"#);
        match &toks[0] {
            Tok::Start { name, attrs, self_closing } => {
                assert_eq!(name, "item");
                assert!(self_closing);
                assert_eq!(attr(attrs, "href"), Some("Text/b%20c.xhtml"));
                assert_eq!(attr(attrs, "media-type"), Some("application/xhtml+xml"));
            }
            _ => panic!(),
        }
    }
    #[test]
    fn dates() {
        let d = parse_date("2026-03-01").unwrap();
        assert_eq!(fmt_date(d - 1), "2026-02-28");
        assert_eq!(fmt_date(parse_date("2024-12-31").unwrap() + 1), "2025-01-01");
        assert_eq!(fmt_date(days_from_civil(1970, 1, 1)), "1970-01-01");
    }
    #[test]
    fn finds() {
        let t: Vec<char> = fold("Kitap, kitaplık ve KİTAP").chars().collect();
        let q: Vec<char> = fold("kitap").chars().collect();
        assert_eq!(find_all(&t, &q).len(), 3);
        assert_eq!(sanitize_filename("C:\\x\\a:b?.pdf"), "a_b_.pdf");
    }
}
