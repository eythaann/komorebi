use std::{collections::VecDeque, time::Duration};

use color_eyre::eyre::Result;
use crossbeam_utils::atomic::AtomicConsume;
use schemars::JsonSchema;
use windows::Win32::{
    Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM},
    Graphics::Gdi::{
        CreatePen, CreateSolidBrush, DrawTextW, GetDC, ReleaseDC, RoundRect, SelectObject,
        SetBkColor, SetTextColor, DT_CENTER, DT_END_ELLIPSIS, DT_SINGLELINE, DT_VCENTER, PS_SOLID,
    },
    UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW,
        PostQuitMessage, RegisterClassW, SetLayeredWindowAttributes, TranslateMessage, CS_HREDRAW,
        CS_VREDRAW, LWA_COLORKEY, MSG, SW_SHOW, WM_DESTROY, WNDCLASSW, WS_EX_LAYERED,
        WS_EX_TOOLWINDOW, WS_POPUP,
    },
};

use komorebi_core::Rect;

use crate::{
    utils::str_to_color, window::Window, windows_api::WindowsApi, DEFAULT_CONTAINER_PADDING,
    TAB_BACKGROUND, TAB_HEIGH, TAB_TEXT_COLOR, TAB_WIDTH, TRANSPARENCY_COLOUR,
};

#[derive(Debug, Clone, JsonSchema)]
pub struct TopBar {
    pub(crate) hwnd: isize,
}

impl From<isize> for TopBar {
    fn from(hwnd: isize) -> Self {
        Self { hwnd }
    }
}

impl TopBar {
    pub const fn hwnd(&self) -> HWND {
        HWND(self.hwnd)
    }

    pub fn create(id: &str) -> Result<TopBar> {
        let name = WindowsApi::pcwstr(&format!("Top Bar Komorebi {id}\0"));
        let class = WindowsApi::pcwstr("Top_Bar_Komorebi\0");

        let h_module = WindowsApi::module_handle_w()?;

        let wnd_class = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(Self::window_proc),
            hInstance: h_module.into(),
            lpszClassName: class.clone().get(),
            hbrBackground: WindowsApi::create_solid_brush(TRANSPARENCY_COLOUR),
            ..Default::default()
        };

        unsafe {
            RegisterClassW(&wnd_class);
        }

        print!("\n\n creating top bar\n\n");

        let (hwnd_sender, hwnd_receiver) = crossbeam_channel::bounded::<HWND>(1);

        std::thread::spawn(move || -> Result<()> {
            unsafe {
                let hwnd = CreateWindowExW(
                    WS_EX_TOOLWINDOW | WS_EX_LAYERED,
                    class.get(),
                    name.get(),
                    WS_POPUP,
                    0,
                    0,
                    0,
                    0,
                    None,
                    None,
                    h_module,
                    None,
                );

                SetLayeredWindowAttributes(hwnd, COLORREF(0), 0, LWA_COLORKEY)?;
                hwnd_sender.send(hwnd)?;

                let mut msg = MSG::default();
                while GetMessageW(&mut msg, hwnd, 0, 0).into() {
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                    std::thread::sleep(Duration::from_millis(10));
                }
            }

            Ok(())
        });

        Ok(TopBar::from(hwnd_receiver.recv()?.0))
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

    pub fn set_position(&self, layout: &Rect, top: bool) -> Result<()> {
        WindowsApi::position_window(self.hwnd(), layout, top)
    }

    pub fn get_position_from_container_layout(&self, layout: &Rect) -> Rect {
        Rect {
            bottom: layout.top + TAB_HEIGH.load_consume(),
            ..layout.clone()
        }
    }

    pub fn update(&self, windows: &VecDeque<Window>) -> Result<()> {
        let width = TAB_WIDTH.load_consume();
        let height = TAB_HEIGH.load_consume();
        let gap = DEFAULT_CONTAINER_PADDING.load_consume();
        let background = str_to_color(&TAB_BACKGROUND.lock());
        let text_color = str_to_color(&TAB_TEXT_COLOR.lock());

        unsafe {
            let hdc = GetDC(self.hwnd());

            let hpen = CreatePen(PS_SOLID, 0, background);
            let hbrush = CreateSolidBrush(background);

            SelectObject(hdc, hpen);
            SelectObject(hdc, hbrush);
            SetTextColor(hdc, text_color);
            SetBkColor(hdc, background);

            for (i, window) in windows.iter().enumerate() {
                let left = gap + (i as i32 * (width + gap));
                let top = 0;

                let right = left + width;
                let bottom = height;

                RoundRect(hdc, left, 0, right, bottom, 12, 12);

                let mut window_title = window.title()?.encode_utf16().collect::<Vec<u16>>();

                let mut text_rect = RECT {
                    left: left + 5,
                    top,
                    right: right - 5,
                    bottom,
                };

                DrawTextW(
                    hdc,
                    &mut window_title,
                    &mut text_rect,
                    DT_SINGLELINE | DT_CENTER | DT_VCENTER | DT_END_ELLIPSIS,
                );
            }

            ReleaseDC(self.hwnd(), hdc);
        }
        Ok(())
    }

    pub fn destroy(&self) -> Result<()> {
        WindowsApi::close_window(self.hwnd())
    }

    pub fn hide(&self) -> Result<()> {
        WindowsApi::hide_window(self.hwnd());
        Ok(())
    }

    pub fn restore(&self) -> Result<()> {
        WindowsApi::show_window(self.hwnd(), SW_SHOW);
        Ok(())
    }
}
