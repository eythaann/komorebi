use std::{
    collections::{HashMap, VecDeque}, ops::Deref, time::Duration
};

use color_eyre::eyre::Result;
use crossbeam_utils::atomic::AtomicConsume;
use lazy_static::lazy_static;
use parking_lot::Mutex;
use schemars::JsonSchema;
use windows::Win32::{
    Foundation::{COLORREF, HWND, LPARAM, LRESULT, WPARAM},
    Graphics::Gdi::{
        CreatePen, CreateSolidBrush, DrawTextW, GetDC, ReleaseDC, SelectObject, SetBkColor,
        SetTextColor, DT_CENTER, DT_END_ELLIPSIS, DT_SINGLELINE, DT_VCENTER, PS_SOLID,
    },
    UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, PostQuitMessage,
        RegisterClassW, SetLayeredWindowAttributes, TranslateMessage, CS_HREDRAW, CS_VREDRAW,
        LWA_COLORKEY, MSG, SW_SHOW, WM_DESTROY, WM_LBUTTONDOWN, WNDCLASSW, WS_EX_LAYERED,
        WS_EX_TOOLWINDOW, WS_POPUP, WS_VISIBLE,
    },
};

use komorebi_core::Rect;

use crate::{
    static_config::top_bar::{TAB_BACKGROUND, TAB_TEXT_COLOR, TAB_WIDTH, TOP_BAR_HEIGH}, utils::str_to_color, window::Window,
    window_manager_event::WindowManagerEvent, windows_api::WindowsApi, winevent::WinEvent,
    winevent_listener::WINEVENT_CALLBACK_CHANNEL, DEFAULT_CONTAINER_PADDING, TRANSPARENCY_COLOUR,
};

lazy_static! {
    static ref WINDOWS_BY_BAR_HWNDS: Mutex<HashMap<isize, VecDeque<isize>>> =
        Mutex::new(HashMap::new());
}

#[derive(Debug, JsonSchema)]
pub struct TopBar {
    pub(crate) hwnd: isize,
    pub is_cloned: bool,
}

impl Default for TopBar {
    fn default() -> Self {
        Self {
            hwnd: 0,
            is_cloned: false,
        }
    }
}

impl Clone for TopBar {
    fn clone(&self) -> Self {
        Self {
            hwnd: self.hwnd,
            is_cloned: true,
        }
    }
}

impl Drop for TopBar {
    fn drop(&mut self) {
        if !self.is_cloned {
            let _ = WindowsApi::close_window(self.hwnd());
        }
    }
}

impl TopBar {
    unsafe extern "system" fn window_proc(
        hwnd: HWND,
        msg: u32,
        w_param: WPARAM,
        l_param: LPARAM,
    ) -> LRESULT {
        match msg {
            WM_LBUTTONDOWN => {
                let win_hwnds_by_topbar = WINDOWS_BY_BAR_HWNDS.lock();
                if let Some(win_hwnds) = win_hwnds_by_topbar.get(&hwnd.0) {
                    let x = l_param.0 as i32 & 0xFFFF;
                    let y = (l_param.0 as i32 >> 16) & 0xFFFF;

                    let width = TAB_WIDTH.lock().clone();
                    let height = TOP_BAR_HEIGH.lock().clone();
                    let gap = DEFAULT_CONTAINER_PADDING.load_consume();

                    for (index, win_hwnd) in win_hwnds.iter().enumerate() {
                        let left = gap + (index as i32 * (width + gap));
                        let right = left + width;
                        let top = 0;
                        let bottom = height;

                        if x >= left && x <= right && y >= top && y <= bottom {
                            let window = Window { hwnd: *win_hwnd };
                            let event_sender = WINEVENT_CALLBACK_CHANNEL.lock();
                            let _ = event_sender.0.send(WindowManagerEvent::FocusChange(
                                WinEvent::ObjectFocus,
                                window,
                            ));
                            let _ = event_sender.0.send(WindowManagerEvent::ForceUpdate(window));
                        }
                    }
                }
                WINDOWS_BY_BAR_HWNDS.force_unlock();
                LRESULT(0)
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => return DefWindowProcW(hwnd, msg, w_param, l_param),
        }
    }

    pub fn create() -> Result<TopBar> {
        let name = WindowsApi::pcwstr("Top Bar Komorebi\0");
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
                    WS_POPUP | WS_VISIBLE,
                    0,
                    0,
                    0,
                    0,
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
                    std::thread::sleep(Duration::from_millis(10));
                }
            }

            Ok(())
        });

        Ok(Self {
            hwnd: hwnd_receiver.recv()?.0,
            ..Default::default()
        })
    }

    pub const fn hwnd(&self) -> HWND {
        HWND(self.hwnd)
    }

    pub fn set_position(&self, layout: &Rect, top: bool) -> Result<()> {
        WindowsApi::position_window(self.hwnd(), layout, top)
    }

    pub fn get_position_from_container_layout(&self, layout: &Rect) -> Rect {
        Rect {
            bottom: TOP_BAR_HEIGH.lock().clone(),
            ..layout.clone()
        }
    }

    pub fn update(&self, windows: &VecDeque<Window>) -> Result<()> {
        let width = TAB_WIDTH.lock().clone();
        let height = TOP_BAR_HEIGH.lock().clone();
        let gap = DEFAULT_CONTAINER_PADDING.load_consume();
        let background = str_to_color(TAB_BACKGROUND.lock().deref());
        let text_color = str_to_color(TAB_TEXT_COLOR.lock().deref());

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
                let mut tab_box = Rect {
                    top: 0,
                    left,
                    right: left + width,
                    bottom: height,
                };

                WindowsApi::round_rect(hdc, &tab_box, 8);

                let mut window_title = window.title()?.encode_utf16().collect::<Vec<u16>>();

                tab_box.left_padding(10);
                tab_box.right_padding(10);

                DrawTextW(
                    hdc,
                    &mut window_title,
                    &mut tab_box.into(),
                    DT_SINGLELINE | DT_CENTER | DT_VCENTER | DT_END_ELLIPSIS,
                );
            }

            ReleaseDC(self.hwnd(), hdc);
        }

        let mut windows_hwdns: VecDeque<isize> = VecDeque::new();
        for window in windows {
            windows_hwdns.push_back(window.hwnd);
        }

        WINDOWS_BY_BAR_HWNDS.lock().insert(self.hwnd, windows_hwdns);
        Ok(())
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
