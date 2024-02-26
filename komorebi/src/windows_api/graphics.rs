use komorebi_core::Rect;
use windows::Win32::Graphics::Gdi::{CreateFontW, RoundRect, CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET, DEFAULT_PITCH, FF_DONTCARE, HDC, HFONT, OUT_OUTLINE_PRECIS};
use windows::core::PCWSTR;

use super::WindowsApi;

impl WindowsApi {
    pub fn round_rect(hdc: HDC, rect: &Rect, border_radius: i32) {
        unsafe {
            RoundRect(
                hdc,
                rect.left,
                rect.top,
                rect.right,
                rect.bottom,
                border_radius,
                border_radius,
            );
        }
    }

    pub fn get_font(font_size: i32) -> HFONT {
        unsafe {
            CreateFontW(
                font_size,
                0,
                0,
                0,
                400,
                0,
                0,
                0,
                DEFAULT_CHARSET.0.into(),
                OUT_OUTLINE_PRECIS.0.into(),
                CLIP_DEFAULT_PRECIS.0.into(), 
                CLEARTYPE_QUALITY.0.into(),
                 (DEFAULT_PITCH.0 | FF_DONTCARE.0).into(),
                PCWSTR::null(),
            )
        }
    }
}
