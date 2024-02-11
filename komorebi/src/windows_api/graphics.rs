use komorebi_core::Rect;
use windows::Win32::Graphics::Gdi::{RoundRect, HDC};

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
}
