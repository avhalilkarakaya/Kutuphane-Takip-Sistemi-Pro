//! Kütüphane Takip Sistemi Pro — Rust sürümü.
//!
//! Uygulama, yerel (127.0.0.1) bir sunucu başlatır ve arayüzü tarayıcı yerine
//! kendi penceresinde (Windows'ta Microsoft Edge WebView2) açar.

// Sürüm derlemesinde arkada siyah konsol penceresi açılmasın
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

#[macro_use]
mod sqlite;
mod api;
mod assets;
mod epub;
mod http;
mod json;
mod util;
mod zipread;

#[cfg(not(localtest))]
extern crate libsqlite3_sys; // SQLite'ı exe'nin içine gömer

use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::Arc;

pub const APP_TITLE: &str = "Kütüphane Takip Sistemi Pro";

/// Veri klasörü: exe'nin bulunduğu klasör (eski sürümlerle aynı yer).
/// Oraya yazılamıyorsa (ör. Program Files) kullanıcının AppData klasörü kullanılır.
fn data_dir() -> PathBuf {
    if let Ok(d) = std::env::var("KYS_DATA_DIR") {
        return PathBuf::from(d);
    }
    let exe_dir = std::env::current_exe().ok().and_then(|p| p.parent().map(|p| p.to_path_buf()));
    if let Some(dir) = exe_dir {
        let probe = dir.join(".yazma_testi");
        if std::fs::write(&probe, b"ok").is_ok() {
            let _ = std::fs::remove_file(&probe);
            return dir;
        }
    }
    let base = std::env::var("APPDATA")
        .or_else(|_| std::env::var("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."));
    base.join("KutuphaneTakipPro")
}

/// Sunucuyu başlatır, adresini döndürür.
fn start_server() -> Result<(PathBuf, String), String> {
    let dir = data_dir();
    let app = Arc::new(api::App::open(dir.clone())?);
    let port_req: u16 = std::env::var("KYS_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(0);
    let listener = TcpListener::bind(("127.0.0.1", port_req)).map_err(|e| format!("Yerel sunucu başlatılamadı: {}", e))?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let handler: Arc<http::Handler> = Arc::new(move |req| app.route(req));
    std::thread::spawn(move || http::serve(listener, port, handler));
    Ok((dir, format!("http://127.0.0.1:{}/", port)))
}

fn main() {
    if let Err(e) = run() {
        show_error(&e);
        std::process::exit(1);
    }
}

#[cfg(localtest)]
fn run() -> Result<(), String> {
    let (dir, url) = start_server()?;
    println!("Veri klasörü: {}", dir.display());
    println!("Adres: {}", url);
    loop {
        std::thread::park();
    }
}

#[cfg(not(localtest))]
mod gui {
    use tao::dpi::LogicalSize;
    use tao::event::{Event, WindowEvent};
    use tao::event_loop::{ControlFlow, EventLoopBuilder};
    use tao::window::{Icon, WindowBuilder};
    use wry::WebViewBuilder;

    enum UserEvent {
        SetTitle(String),
    }

    fn icon() -> Option<Icon> {
        let raw = include_bytes!("../assets/icon64.rgba");
        Icon::from_rgba(raw.to_vec(), 64, 64).ok()
    }

    pub fn open_window(data_dir: &std::path::Path, url: &str) -> Result<(), String> {
        // WebView2 önbelleğini veri klasöründe tut (exe yanında dağınıklık olmasın)
        std::env::set_var("WEBVIEW2_USER_DATA_FOLDER", data_dir.join(".webview2"));

        let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
        let proxy = event_loop.create_proxy();
        let window = WindowBuilder::new()
            .with_title(super::APP_TITLE)
            .with_inner_size(LogicalSize::new(1366.0, 860.0))
            .with_min_inner_size(LogicalSize::new(820.0, 560.0))
            .with_maximized(true)
            .with_window_icon(icon())
            .build(&event_loop)
            .map_err(|e| e.to_string())?;

        let builder = WebViewBuilder::new()
            .with_url(url)
            .with_devtools(cfg!(debug_assertions))
            .with_ipc_handler(move |req: wry::http::Request<String>| {
                if let Some(t) = req.body().strip_prefix("title:") {
                    let _ = proxy.send_event(UserEvent::SetTitle(t.to_string()));
                }
            });

        #[cfg(not(target_os = "linux"))]
        let webview = builder.build(&window).map_err(webview_error)?;
        #[cfg(target_os = "linux")]
        let webview = {
            use tao::platform::unix::WindowExtUnix;
            use wry::WebViewBuilderExtUnix;
            builder.build_gtk(window.default_vbox().ok_or("GTK kutusu yok")?).map_err(webview_error)?
        };

        event_loop.run(move |event, _, control_flow| {
            *control_flow = ControlFlow::Wait;
            let _keep_alive = &webview;
            match event {
                Event::WindowEvent { event: WindowEvent::CloseRequested, .. } => *control_flow = ControlFlow::Exit,
                Event::UserEvent(UserEvent::SetTitle(t)) => window.set_title(&t),
                _ => {}
            }
        })
    }

    fn webview_error(e: wry::Error) -> String {
        format!(
            "Uygulama penceresi açılamadı: {}\n\nBu bilgisayarda Microsoft Edge WebView2 Runtime kurulu olmayabilir. \
             https://developer.microsoft.com/microsoft-edge/webview2/ adresinden \"Evergreen\" sürümünü kurup tekrar deneyin.",
            e
        )
    }
}

#[cfg(not(localtest))]
fn run() -> Result<(), String> {
    let (dir, url) = start_server()?;
    gui::open_window(&dir, &url)
}

#[cfg(windows)]
fn show_error(msg: &str) {
    #[link(name = "user32")]
    extern "system" {
        fn MessageBoxW(hwnd: *mut std::ffi::c_void, text: *const u16, caption: *const u16, utype: u32) -> i32;
    }
    let w = |s: &str| s.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
    let (t, c) = (w(msg), w(APP_TITLE));
    unsafe {
        MessageBoxW(std::ptr::null_mut(), t.as_ptr(), c.as_ptr(), 0x10);
    }
}

#[cfg(not(windows))]
fn show_error(msg: &str) {
    eprintln!("{}", msg);
}
