//! EPUB içindekiler tablosu, bölüm listesi ve kitap içi arama.

use std::collections::HashMap;
use std::path::Path;

use crate::util::{attr, find_all, fold, html_to_text, pct_decode, snippet, Tok};
use crate::zipread::Archive;

pub struct Chapter {
    pub title: String,
    /// zip içindeki tam yol
    pub path: String,
}

fn dir_of(p: &str) -> String {
    match p.rfind('/') {
        Some(i) => p[..i + 1].to_string(),
        None => String::new(),
    }
}

/// `base` klasörüne göre göreli yolu çözer (../ ve ./ desteği).
pub fn resolve(base_dir: &str, href: &str) -> String {
    let href = href.split('#').next().unwrap_or("");
    let href = pct_decode(href, false);
    let joined = if href.starts_with('/') { href.trim_start_matches('/').to_string() } else { format!("{}{}", base_dir, href) };
    let mut out: Vec<&str> = Vec::new();
    for seg in joined.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                out.pop();
            }
            s => out.push(s),
        }
    }
    out.join("/")
}

fn text_of(bytes: &[u8]) -> String {
    let s = String::from_utf8_lossy(bytes);
    s.trim_start_matches('\u{feff}').to_string()
}

pub fn chapters(file: &Path) -> Result<Vec<Chapter>, String> {
    let mut ar = Archive::open(file).map_err(|e| format!("EPUB dosyası açılamadı: {}", e))?;
    let container = ar
        .read_any("META-INF/container.xml")
        .ok_or("Geçerli bir EPUB dosyası değil (container.xml okunamadı).")?;
    let mut opf_path = String::new();
    for t in crate::util::tokenize(&text_of(&container)) {
        if let Tok::Start { name, attrs, .. } = t {
            if name == "rootfile" {
                if let Some(p) = attr(&attrs, "full-path") {
                    opf_path = p.to_string();
                    break;
                }
            }
        }
    }
    if opf_path.is_empty() {
        return Err("EPUB paket dosyası (OPF) bulunamadı.".into());
    }
    let opf = ar.read_any(&opf_path).ok_or("EPUB paket dosyası (OPF) okunamadı.")?;
    let base = dir_of(&opf_path);

    let mut manifest: HashMap<String, (String, String, String)> = HashMap::new(); // id -> (yol, tür, özellikler)
    let mut spine: Vec<String> = Vec::new();
    let mut ncx_id = String::new();
    for t in crate::util::tokenize(&text_of(&opf)) {
        if let Tok::Start { name, attrs, .. } = t {
            match name.as_str() {
                "item" => {
                    let id = attr(&attrs, "id").unwrap_or("").to_string();
                    let href = attr(&attrs, "href").unwrap_or("");
                    manifest.insert(
                        id,
                        (
                            resolve(&base, href),
                            attr(&attrs, "media-type").unwrap_or("").to_string(),
                            attr(&attrs, "properties").unwrap_or("").to_string(),
                        ),
                    );
                }
                "itemref" => {
                    spine.push(attr(&attrs, "idref").unwrap_or("").to_string());
                }
                "spine" => ncx_id = attr(&attrs, "toc").unwrap_or("").to_string(),
                _ => {}
            }
        }
    }

    // İçindekiler başlıkları: önce EPUB3 nav, yoksa NCX
    let mut titles: HashMap<String, String> = HashMap::new();
    let nav = manifest.values().find(|(_, _, props)| props.split_whitespace().any(|p| p == "nav")).map(|v| v.0.clone());
    if let Some(nav_path) = nav {
        if let Some(b) = ar.read_any(&nav_path) {
            let nav_base = dir_of(&nav_path);
            let mut cur_href: Option<String> = None;
            let mut buf = String::new();
            for t in crate::util::tokenize(&text_of(&b)) {
                match t {
                    Tok::Start { name, attrs, .. } if name == "a" => {
                        cur_href = attr(&attrs, "href").map(|h| resolve(&nav_base, h));
                        buf.clear();
                    }
                    Tok::Text(x) if cur_href.is_some() => buf.push_str(&x),
                    Tok::End { name } if name == "a" => {
                        if let Some(h) = cur_href.take() {
                            let title = buf.split_whitespace().collect::<Vec<_>>().join(" ");
                            if !title.is_empty() {
                                titles.entry(h).or_insert(title);
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    if titles.is_empty() {
        let ncx = manifest
            .get(&ncx_id)
            .map(|v| v.0.clone())
            .or_else(|| manifest.values().find(|v| v.1 == "application/x-dtbncx+xml").map(|v| v.0.clone()));
        if let Some(ncx_path) = ncx {
            if let Some(b) = ar.read_any(&ncx_path) {
                let ncx_base = dir_of(&ncx_path);
                let mut in_label = false;
                let mut in_text = false;
                let mut label = String::new();
                for t in crate::util::tokenize(&text_of(&b)) {
                    match t {
                        Tok::Start { name, .. } if name == "navlabel" => {
                            in_label = true;
                            label.clear();
                        }
                        Tok::Start { name, .. } if name == "text" && in_label => in_text = true,
                        Tok::End { name } if name == "text" => in_text = false,
                        Tok::End { name } if name == "navlabel" => in_label = false,
                        Tok::Text(x) if in_text => label.push_str(&x),
                        Tok::Start { name, attrs, .. } if name == "content" => {
                            if let Some(src) = attr(&attrs, "src") {
                                let title = label.split_whitespace().collect::<Vec<_>>().join(" ");
                                if !title.is_empty() {
                                    titles.entry(resolve(&ncx_base, src)).or_insert(title);
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    let mut out = Vec::new();
    for idref in spine {
        if let Some((path, _mt, _)) = manifest.get(&idref) {
            if path.is_empty() {
                continue;
            }
            let n = out.len() + 1;
            let title = titles.get(path).cloned().unwrap_or_else(|| format!("Bölüm {}", n));
            out.push(Chapter { title, path: path.clone() });
        }
    }
    if out.is_empty() {
        return Err("EPUB içinde okunabilir bölüm bulunamadı.".into());
    }
    Ok(out)
}

pub struct Hit {
    pub chapter: usize,
    pub title: String,
    pub count: usize,
    /// (önce, eşleşme, sonra, bölüm içindeki sıra)
    pub snippets: Vec<(String, String, String, usize)>,
}

/// Kitabın tüm bölümlerinde arama yapar.
pub fn search(file: &Path, query: &str, max_snippets: usize) -> Result<(Vec<Hit>, usize), String> {
    let chs = chapters(file)?;
    let mut ar = Archive::open(file)?;
    let q: Vec<char> = fold(query.trim()).chars().collect();
    let mut hits = Vec::new();
    let mut total = 0;
    if q.is_empty() {
        return Ok((hits, 0));
    }
    for (i, ch) in chs.iter().enumerate() {
        let Some(bytes) = ar.read_any(&ch.path) else { continue };
        let text = html_to_text(&text_of(&bytes));
        let chars: Vec<char> = text.chars().collect();
        let folded: Vec<char> = chars.iter().map(|&c| crate::util::fold_char(c)).collect();
        let pos = find_all(&folded, &q);
        if pos.is_empty() {
            continue;
        }
        total += pos.len();
        let snippets = pos
            .iter()
            .enumerate()
            .take(max_snippets)
            .map(|(n, &p)| {
                let (a, b, c) = snippet(&chars, p, q.len(), 60);
                (a, b, c, n)
            })
            .collect();
        hits.push(Hit { chapter: i, title: ch.title.clone(), count: pos.len(), snippets });
    }
    Ok((hits, total))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn resolving() {
        assert_eq!(resolve("OEBPS/", "Text/ch%201.xhtml#x"), "OEBPS/Text/ch 1.xhtml");
        assert_eq!(resolve("OEBPS/Text/", "../Images/a.jpg"), "OEBPS/Images/a.jpg");
        assert_eq!(resolve("", "./a.xhtml"), "a.xhtml");
    }
}
