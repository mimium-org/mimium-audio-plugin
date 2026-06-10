use clack_extensions::gui::{GuiSize, Window};
use clack_plugin::prelude::PluginError;
use raw_window_handle::{HandleError, HasWindowHandle, RawWindowHandle, WindowHandle};
use serde_json::Value;
use std::process::Command;
use url::Url;
use wry::{NewWindowResponse, WebView, WebViewBuilder};

const MAX_IPC_BODY_BYTES: usize = 1024 * 1024;

#[cfg(not(debug_assertions))]
const DISABLE_CONTEXT_MENU_SCRIPT: &str = r#"
window.addEventListener(
    'contextmenu',
    (event) => {
        event.preventDefault();
    },
    { capture: true }
);
"#;


pub struct EmbeddedWebviewConfig {
    pub width: u32,
    pub height: u32,
    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    pub debug_url: &'static str,
    #[cfg_attr(debug_assertions, allow(dead_code))]
    pub release_html: &'static str,
}

pub struct EmbeddedWebviewGui {
    webview: WebView,
}

#[derive(Clone, Debug, Default)]
struct NavigationPolicy {
    internal_origin: Option<UrlOrigin>,
}

#[derive(Clone, Debug)]
struct UrlOrigin {
    scheme: String,
    host: String,
    port: Option<u16>,
}

impl NavigationPolicy {
    fn from_config(config: &EmbeddedWebviewConfig) -> Self {
        #[cfg(debug_assertions)]
        {
            Self {
                internal_origin: UrlOrigin::parse(config.debug_url),
            }
        }

        #[cfg(not(debug_assertions))]
        {
            let _ = config;
            Self::default()
        }
    }

    fn should_allow_embedded_navigation(&self, url: &Url) -> bool {
        self.internal_origin
            .as_ref()
            .is_some_and(|origin| origin.matches(url))
    }
}

impl UrlOrigin {
    #[cfg(debug_assertions)]
    fn parse(url: &str) -> Option<Self> {
        let parsed = Url::parse(url).ok()?;
        Some(Self {
            scheme: parsed.scheme().to_string(),
            host: parsed.host_str()?.to_string(),
            port: parsed.port_or_known_default(),
        })
    }

    fn matches(&self, url: &Url) -> bool {
        url.scheme() == self.scheme
            && url
                .host_str()
                .is_some_and(|host| host.eq_ignore_ascii_case(&self.host))
            && url.port_or_known_default() == self.port
    }
}

impl EmbeddedWebviewGui {
    pub fn new<F>(
        parent: Window<'_>,
        config: &EmbeddedWebviewConfig,
        ipc_handler: F,
    ) -> Result<Self, PluginError>
    where
        F: Fn(Value) + 'static,
    {
        let parent_handle = PluginParentWindow::from_clap_window(&parent)
            .ok_or(PluginError::Message("Unsupported window type"))?;
        let navigation_policy = NavigationPolicy::from_config(config);

        let mut builder = WebViewBuilder::new()
            .with_ipc_handler(move |request| {
                let body = request.body();
                if body.len() > MAX_IPC_BODY_BYTES {
                    eprintln!(
                        "[mimium] dropped oversized IPC body: {} bytes (limit {})",
                        body.len(),
                        MAX_IPC_BODY_BYTES
                    );
                    return;
                }
                if let Ok(message) = serde_json::from_str::<Value>(body) {
                    ipc_handler(message);
                }
            })
            .with_navigation_handler({
                let navigation_policy = navigation_policy.clone();
                move |url| handle_navigation_request(&url, &navigation_policy)
            })
            .with_new_window_req_handler(|url, _features| handle_new_window_request(&url))
            .with_bounds(wry::Rect {
                position: wry::dpi::LogicalPosition::new(0.0, 0.0).into(),
                size: wry::dpi::LogicalSize::new(config.width as f64, config.height as f64).into(),
            });

        #[cfg(debug_assertions)]
        {
            builder = builder.with_url(config.debug_url);
        }
        #[cfg(not(debug_assertions))]
        {
            builder = builder
                .with_initialization_script(DISABLE_CONTEXT_MENU_SCRIPT)
                .with_html(config.release_html);
        }

        let webview = builder
            .with_accept_first_mouse(true)
            .build_as_child(&parent_handle)
            .map_err(|_| PluginError::Message("Failed to create webview"))?;

        Ok(Self { webview })
    }

    pub fn resize(&self, size: GuiSize) {
        let _ = self.webview.set_bounds(wry::Rect {
            position: wry::dpi::LogicalPosition::new(0.0, 0.0).into(),
            size: wry::dpi::LogicalSize::new(size.width as f64, size.height as f64).into(),
        });
    }

    pub fn evaluate_script(&self, script: &str) -> Result<(), String> {
        self.webview
            .evaluate_script(script)
            .map_err(|error| error.to_string())
    }
}

fn handle_navigation_request(url: &str, navigation_policy: &NavigationPolicy) -> bool {
    let Ok(parsed) = Url::parse(url) else {
        return true;
    };

    if !matches!(parsed.scheme(), "http" | "https") {
        return true;
    }

    if navigation_policy.should_allow_embedded_navigation(&parsed) {
        return true;
    }

    open_external_url(parsed.as_str());
    false
}

fn handle_new_window_request(url: &str) -> NewWindowResponse {
    if let Ok(parsed) = Url::parse(url) {
        if matches!(parsed.scheme(), "http" | "https") {
            open_external_url(parsed.as_str());
        }
    }

    NewWindowResponse::Deny
}

fn open_external_url(url: &str) {
    if let Err(error) = spawn_external_browser(url) {
        eprintln!("failed to open external url {url}: {error}");
    }
}

fn spawn_external_browser(url: &str) -> std::io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        Command::new("/usr/bin/open").arg(url).spawn().map(|_| ())
    }

    #[cfg(target_os = "windows")]
    {
        Command::new("cmd")
            .args(["/C", "start", "", url])
            .spawn()
            .map(|_| ())
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        Command::new("xdg-open").arg(url).spawn().map(|_| ())
    }
}

struct PluginParentWindow(RawWindowHandle);

impl HasWindowHandle for PluginParentWindow {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        Ok(unsafe { WindowHandle::borrow_raw(self.0) })
    }
}

impl PluginParentWindow {
    fn from_clap_window(window: &Window<'_>) -> Option<Self> {
        #[cfg(target_os = "macos")]
        {
            use raw_window_handle::AppKitWindowHandle;
            use std::ptr::NonNull;

            let parent_view = NonNull::new(window.as_cocoa_nsview()? as *mut std::ffi::c_void)?;
            let handle = AppKitWindowHandle::new(parent_view);
            Some(Self(RawWindowHandle::AppKit(handle)))
        }

        #[cfg(target_os = "windows")]
        {
            use raw_window_handle::Win32WindowHandle;
            use std::num::NonZeroIsize;

            let hwnd = window.as_win32_hwnd()?;
            let handle = Win32WindowHandle::new(NonZeroIsize::new(hwnd as isize)?);
            Some(Self(RawWindowHandle::Win32(handle)))
        }

        #[cfg(all(unix, not(target_os = "macos")))]
        {
            use raw_window_handle::XlibWindowHandle;

            let x11 = window.as_x11_handle()?;
            Some(Self(RawWindowHandle::Xlib(XlibWindowHandle::new(x11))))
        }
    }
}
