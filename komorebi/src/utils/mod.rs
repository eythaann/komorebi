use std::sync::Arc;

use windows::Win32::Foundation::COLORREF;
use parking_lot::Mutex;

pub fn rgb_to_bgr_hex(hex: u32) -> u32 {
    let red = (hex >> 16) & 0xFF;
    let green = (hex >> 8) & 0xFF;
    let blue = hex & 0xFF;

    let bgr_hex = (blue << 16) | (green << 8) | red;

    bgr_hex
}

/// If recive #000000 it will return 1 instead 0 because we use 0 for transparency
pub fn str_to_color(hex_string: &str) -> COLORREF {
    if let Ok(hex_value) = u32::from_str_radix(&hex_string[1..], 16) {
        let mut color = rgb_to_bgr_hex(hex_value);
        if color == 0 {
            color = 1;
        }
        COLORREF(color)
    } else {
        COLORREF(1)
    }
}

pub type ArcMutex<T> = Arc<Mutex<T>>;
#[allow(non_snake_case)]
pub fn ArcMutex<T>(val: T) -> Arc<Mutex<T>> {
    Arc::new(Mutex::new(val))
}