use color_eyre::eyre::anyhow;
use color_eyre::eyre::Result;
use crossbeam_utils::atomic::AtomicConsume;
use komorebi_core::Rect;
use lazy_static::lazy_static;
use parking_lot::Mutex;
use std::{
    ops::Deref,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Instant,
};
use windows::Win32::Graphics::Gdi::ReleaseDC;
use windows::Win32::{
    Foundation::{COLORREF, HWND, LPARAM, LRESULT, WPARAM},
    Graphics::Gdi::{
        CreatePen, CreateSolidBrush, DrawTextW, GetDC, SelectObject, SetBkMode, SetTextColor,
        DT_CENTER, DT_END_ELLIPSIS, DT_SINGLELINE, DT_VCENTER, HFONT, PS_SOLID, TRANSPARENT,
    },
    UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, GetSystemMetrics,
        PostQuitMessage, SetLayeredWindowAttributes, TranslateMessage, CS_HREDRAW, CS_VREDRAW,
        LWA_COLORKEY, MSG, SM_CXSCREEN, SM_CYSCREEN, WM_DESTROY, WNDCLASSW, WS_EX_LAYERED,
        WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP, WS_VISIBLE,
    },
};

use crate::{
    popup,
    utils::{str_to_color, ArcMutex},
    WindowsApi, TRANSPARENCY_COLOUR,
};

lazy_static! {
    pub static ref POPUP_INSTANCE: ArcMutex<Option<Popup>> =
        Arc::new(Mutex::new(Popup::create().ok()));
    pub static ref TIME_TO_CLOSE: ArcMutex<Instant> = ArcMutex(Instant::now());
    pub static ref POPUP_FONT: HFONT = WindowsApi::get_font(25);
}

pub struct Popup {
    pub(crate) hwnd: isize,
}

impl Popup {
    pub const fn hwnd(&self) -> HWND {
        HWND(self.hwnd)
    }

    unsafe extern "system" fn window_proc(
        hwnd: HWND,
        msg: u32,
        w_param: WPARAM,
        l_param: LPARAM,
    ) -> LRESULT {
        match msg {
            WM_DESTROY => {
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => return DefWindowProcW(hwnd, msg, w_param, l_param),
        }
    }

    fn create() -> Result<Popup> {
        if !popup::ENABLED.load_consume() {
            return Err(anyhow!("Can not create popups if are disabled"));
        }

        let name = WindowsApi::pcwstr("komorbei popup\0");
        let class = WindowsApi::pcwstr("KomorebiPopup\0");

        let h_module = WindowsApi::module_handle_w()?;

        let wnd_class = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(Self::window_proc),
            hInstance: h_module.into(),
            lpszClassName: class.clone().get(),
            hbrBackground: WindowsApi::create_solid_brush(TRANSPARENCY_COLOUR),
            ..Default::default()
        };

        WindowsApi::register_class_w(&wnd_class)?;

        let (hwnd_sender, hwnd_receiver) = crossbeam_channel::bounded::<HWND>(1);

        std::thread::spawn(move || -> Result<()> {
            unsafe {
                let screen_width = GetSystemMetrics(SM_CXSCREEN);
                let screen_height = GetSystemMetrics(SM_CYSCREEN);

                let width = popup::WIDTH.load_consume();
                let height = popup::HEIGHT.load_consume();
                let x = popup::POS_X
                    .lock()
                    .or_else(|| Some((screen_width - width) / 2))
                    .unwrap();
                let y = popup::POS_Y
                    .lock()
                    .or_else(|| Some((screen_height - height) / 2))
                    .unwrap();

                let hwnd = CreateWindowExW(
                    WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_LAYERED,
                    class.get(),
                    name.get(),
                    WS_POPUP | WS_VISIBLE,
                    x,
                    y,
                    width,
                    height,
                    None,
                    None,
                    h_module,
                    None,
                );

                SetLayeredWindowAttributes(hwnd, COLORREF(TRANSPARENCY_COLOUR), 0, LWA_COLORKEY)?;
                hwnd_sender.send(hwnd)?;
                let mut msg = MSG::default();
                while GetMessageW(&mut msg, hwnd, 0, 0).into() {
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
            }

            Ok(())
        });

        Ok(Self {
            hwnd: hwnd_receiver.recv()?.0,
        })
    }
}

static TIMER_EXIST: AtomicBool = AtomicBool::new(false);
static TIME_LIMIT: u64 = 1000;

pub fn show_message(text: &str) -> Result<()> {
    let mut popup_box = Rect {
        left: 0,
        top: 0,
        right: popup::WIDTH.load_consume(),
        bottom: popup::HEIGHT.load_consume(),
    };

    if let Some(instance) = POPUP_INSTANCE.lock().deref() {
        *TIME_TO_CLOSE.lock() = Instant::now() + std::time::Duration::from_millis(TIME_LIMIT);

        let border_color = str_to_color(&popup::BORDER_COLOR.lock().to_string());
        let background_color = str_to_color(&popup::BACKGROUND.lock().to_string());
        let text_color = str_to_color(&popup::TEXT_COLOR.lock().to_string());

        WindowsApi::restore_window(instance.hwnd());

        unsafe {
            let mut text = text.encode_utf16().collect::<Vec<u16>>();
            let hdc = GetDC(instance.hwnd());

            // drawing border
            SelectObject(hdc, CreatePen(PS_SOLID, 0, border_color));
            SelectObject(hdc, CreateSolidBrush(border_color));
            WindowsApi::round_rect(hdc, &popup_box, 30);

            // drawing inner box
            popup_box.add_gui_padding(popup::BORDER_WIDTH.load_consume());
            SelectObject(hdc, CreatePen(PS_SOLID, 0, background_color));
            SelectObject(hdc, CreateSolidBrush(background_color));
            WindowsApi::round_rect(hdc, &popup_box, 24);

            // drawing text
            popup_box.add_gui_padding(10);
            SetTextColor(hdc, text_color);
            SetBkMode(hdc, TRANSPARENT);
            SelectObject(hdc, POPUP_FONT.clone());
            DrawTextW(
                hdc,
                &mut text,
                &mut popup_box.into(),
                DT_SINGLELINE | DT_CENTER | DT_VCENTER | DT_END_ELLIPSIS,
            );

            ReleaseDC(instance.hwnd(), hdc);
        }

        let hwnd = instance.hwnd;
        if !TIMER_EXIST.load(Ordering::Relaxed) {
            *TIME_TO_CLOSE.lock() = Instant::now() + std::time::Duration::from_millis(TIME_LIMIT);
            TIMER_EXIST.store(true, Ordering::Relaxed);

            std::thread::spawn(move || -> Result<()> {
                while Instant::now() < *TIME_TO_CLOSE.lock() {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                WindowsApi::hide_window(HWND(hwnd));
                TIMER_EXIST.store(false, Ordering::Relaxed);
                Ok(())
            });
        }
    }
    Ok(())
}
