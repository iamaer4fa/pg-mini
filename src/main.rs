pub mod app;
pub mod activity;
pub mod config;
pub mod db;
pub mod models;
pub mod sql_ddl;
pub mod ui;

use app::PgMiniApp;

#[cfg(target_os = "windows")]
fn select_renderer() -> eframe::Renderer {
    if windows_opengl_2_available() {
        eprintln!("Using OpenGL renderer");
        eframe::Renderer::Glow
    } else {
        eprintln!("OpenGL 2.0 is unavailable; using WGPU renderer");
        eframe::Renderer::Wgpu
    }
}

#[cfg(not(target_os = "windows"))]
fn select_renderer() -> eframe::Renderer {
    eframe::Renderer::Glow
}

#[cfg(any(target_os = "windows", test))]
fn supports_opengl_2(version: &str) -> bool {
    version
        .split('.')
        .next()
        .and_then(|major| major.parse::<u32>().ok())
        .is_some_and(|major| major >= 2)
}

#[cfg(target_os = "windows")]
fn windows_opengl_2_available() -> bool {
    use std::ffi::CStr;
    use std::mem::size_of;
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::Graphics::Gdi::{GetDC, ReleaseDC};
    use windows_sys::Win32::Graphics::OpenGL::{
        ChoosePixelFormat, GL_VERSION, PFD_DOUBLEBUFFER, PFD_DRAW_TO_WINDOW, PFD_MAIN_PLANE,
        PFD_SUPPORT_OPENGL, PFD_TYPE_RGBA, PIXELFORMATDESCRIPTOR, SetPixelFormat, glGetString,
        wglCreateContext, wglDeleteContext, wglMakeCurrent,
    };
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{CreateWindowExW, DestroyWindow, WS_POPUP};

    // The hidden WGL context tests the active session's actual driver, including RDP sessions.
    unsafe {
        let class_name = [
            b'S' as u16,
            b'T' as u16,
            b'A' as u16,
            b'T' as u16,
            b'I' as u16,
            b'C' as u16,
            0,
        ];
        let window_name = [0u16];
        let window = CreateWindowExW(
            0,
            class_name.as_ptr(),
            window_name.as_ptr(),
            WS_POPUP,
            0,
            0,
            1,
            1,
            null_mut(),
            null_mut(),
            GetModuleHandleW(null()),
            null(),
        );
        if window.is_null() {
            return false;
        }

        let device_context = GetDC(window);
        if device_context.is_null() {
            DestroyWindow(window);
            return false;
        }

        let mut pixel_format_descriptor = PIXELFORMATDESCRIPTOR::default();
        pixel_format_descriptor.nSize = size_of::<PIXELFORMATDESCRIPTOR>() as u16;
        pixel_format_descriptor.nVersion = 1;
        pixel_format_descriptor.dwFlags =
            PFD_DRAW_TO_WINDOW | PFD_SUPPORT_OPENGL | PFD_DOUBLEBUFFER;
        pixel_format_descriptor.iPixelType = PFD_TYPE_RGBA;
        pixel_format_descriptor.cColorBits = 24;
        pixel_format_descriptor.cDepthBits = 16;
        pixel_format_descriptor.iLayerType = PFD_MAIN_PLANE as u8;

        let format = ChoosePixelFormat(device_context, &pixel_format_descriptor);
        if format == 0 || SetPixelFormat(device_context, format, &pixel_format_descriptor) == 0 {
            ReleaseDC(window, device_context);
            DestroyWindow(window);
            return false;
        }

        let context = wglCreateContext(device_context);
        if context.is_null() {
            ReleaseDC(window, device_context);
            DestroyWindow(window);
            return false;
        }

        let supports_opengl_2 = if wglMakeCurrent(device_context, context) != 0 {
            let version = glGetString(GL_VERSION);
            if version.is_null() {
                false
            } else {
                supports_opengl_2(&CStr::from_ptr(version.cast()).to_string_lossy())
            }
        } else {
            false
        };

        wglMakeCurrent(null_mut(), null_mut());
        wglDeleteContext(context);
        ReleaseDC(window, device_context);
        DestroyWindow(window);
        supports_opengl_2
    }
}

fn main() -> eframe::Result<()> {
    let icon_image = image::load_from_memory(include_bytes!("../assets/pg-mini.png"))
        .expect("embedded pg-mini icon must be a valid PNG")
        .into_rgba8();
    let (icon_width, icon_height) = icon_image.dimensions();
    let icon = egui::IconData {
        rgba: icon_image.into_raw(),
        width: icon_width,
        height: icon_height,
    };

    let native_options = eframe::NativeOptions {
        renderer: select_renderer(),
        viewport: egui::ViewportBuilder::default()
            .with_title("pg-mini - Lightweight PostgreSQL Manager")
            .with_inner_size([1100.0, 720.0])
            .with_min_inner_size([700.0, 480.0])
            .with_icon(icon)
            .with_active(true),
        ..Default::default()
    };

    eframe::run_native(
        "pg-mini",
        native_options,
        Box::new(|cc| Ok(Box::new(PgMiniApp::new(cc)))),
    )
}

#[cfg(test)]
mod renderer_tests {
    use super::supports_opengl_2;

    #[test]
    fn opengl_renderer_requires_version_two_or_newer() {
        assert!(!supports_opengl_2("1.1.0"));
        assert!(supports_opengl_2("2.0"));
        assert!(supports_opengl_2("4.6.0 NVIDIA 555.12"));
        assert!(!supports_opengl_2("unknown"));
    }
}
