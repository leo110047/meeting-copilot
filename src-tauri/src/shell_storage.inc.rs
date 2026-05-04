fn install_tray(app: &tauri::AppHandle) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "Open Meeting Copilot", true, None::<&str>)?;
    let stop = MenuItem::with_id(app, "stop", "Stop Listening", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &stop, &quit])?;
    TrayIconBuilder::with_id("meeting-copilot")
        .icon(TRAY_ICON)
        .tooltip("Meeting Copilot")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show" => show_main_window(app),
            "stop" => {
                stop_all_native_transcribers(app);
                show_main_window(app);
                emit_native_transcription_error(
                    app,
                    "Native transcription stopped from tray",
                    "tray",
                );
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| match event {
            TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            }
            | TrayIconEvent::DoubleClick {
                button: MouseButton::Left,
                ..
            } => show_main_window(tray.app_handle()),
            _ => {}
        })
        .build(app)?;
    Ok(())
}

fn show_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

fn set_listening_window_mode(app: &tauri::AppHandle, enabled: bool) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.set_always_on_top(enabled);
    }
}

#[cfg(target_os = "macos")]
fn set_native_window_opacity(app: &tauri::AppHandle, opacity: f64) -> Result<(), String> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "main window not found".to_string())?;
    let ns_window = window.ns_window().map_err(|error| error.to_string())?;
    unsafe {
        // Tauri exposes the raw NSWindow pointer; objc2 0.3's NSWindow ABI is
        // pinned in Cargo.toml, so keep this cast local and easy to audit.
        let ns_window = &*(ns_window.cast::<objc2_app_kit::NSWindow>());
        ns_window.setOpaque(false);
        ns_window.setBackgroundColor(Some(&objc2_app_kit::NSColor::clearColor()));
        ns_window.setAlphaValue(opacity.clamp(0.1, 1.0));
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn set_native_window_opacity(app: &tauri::AppHandle, opacity: f64) -> Result<(), String> {
    use windows::Win32::Foundation::COLORREF;
    use windows::Win32::UI::WindowsAndMessaging::{
        GWL_EXSTYLE, GetWindowLongW, LWA_ALPHA, SetLayeredWindowAttributes, SetWindowLongW,
        WS_EX_LAYERED,
    };

    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "main window not found".to_string())?;
    let hwnd = window.hwnd().map_err(|error| error.to_string())?;
    let alpha = (opacity.clamp(0.1, 1.0) * 255.0).round() as u8;
    unsafe {
        let style = GetWindowLongW(hwnd, GWL_EXSTYLE);
        SetWindowLongW(hwnd, GWL_EXSTYLE, style | WS_EX_LAYERED.0 as i32);
        SetLayeredWindowAttributes(hwnd, COLORREF(0), alpha, LWA_ALPHA)
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn set_native_window_opacity(_app: &tauri::AppHandle, opacity: f64) -> Result<(), String> {
    let _ = opacity;
    Err("native window opacity is not implemented for this platform yet".to_string())
}

fn open_db(db_path: &PathBuf) -> Result<Connection, String> {
    if let Some(parent) = db_path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let conn = Connection::open(db_path).map_err(|error| error.to_string())?;
    conn.execute_batch(SCHEMA_SQL)
        .map_err(|error| error.to_string())?;
    Ok(conn)
}

fn app_db_path() -> Result<PathBuf, String> {
    let base = app_data_dir()?;
    Ok(base.join("meeting-copilot-native.db"))
}

#[cfg(target_os = "macos")]
fn app_data_dir() -> Result<PathBuf, String> {
    let home = std::env::var("HOME").map_err(|_| "HOME is not set".to_string())?;
    Ok(PathBuf::from(home)
        .join("Library")
        .join("Application Support")
        .join("Meeting Copilot"))
}

fn read_dropped_context_file(path: PathBuf) -> DroppedContextFile {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("dropped-file")
        .to_string();
    let allowed = path
        .extension()
        .and_then(|value| value.to_str())
        .map(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "txt" | "md" | "markdown" | "csv" | "json" | "log" | "srt" | "vtt"
            )
        })
        .unwrap_or(false);
    if !allowed {
        return DroppedContextFile {
            name,
            text: String::new(),
            truncated: false,
            error: Some("只支援文字檔：txt、md、csv、json、log、srt、vtt".to_string()),
        };
    }
    let metadata = match fs::metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) => {
            return DroppedContextFile {
                name,
                text: String::new(),
                truncated: false,
                error: Some(error.to_string()),
            };
        }
    };
    if !metadata.is_file() {
        return DroppedContextFile {
            name,
            text: String::new(),
            truncated: false,
            error: Some("拖拉項目不是檔案".to_string()),
        };
    }
    const MAX_CONTEXT_BYTES: usize = 256 * 1024;
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) => {
            return DroppedContextFile {
                name,
                text: String::new(),
                truncated: false,
                error: Some(error.to_string()),
            };
        }
    };
    let truncated = bytes.len() > MAX_CONTEXT_BYTES;
    let bytes = if truncated {
        &bytes[..MAX_CONTEXT_BYTES]
    } else {
        bytes.as_slice()
    };
    match String::from_utf8(bytes.to_vec()) {
        Ok(text) => DroppedContextFile {
            name,
            text,
            truncated,
            error: None,
        },
        Err(error) => DroppedContextFile {
            name,
            text: String::new(),
            truncated,
            error: Some(format!("不是 UTF-8 文字檔：{error}")),
        },
    }
}
