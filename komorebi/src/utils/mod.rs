use windows::Win32::Foundation::COLORREF;

pub fn rgb_to_bgr_hex(hex: u32) -> u32 {
    let red = (hex >> 16) & 0xFF;
    let green = (hex >> 8) & 0xFF;
    let blue = hex & 0xFF;

    let bgr_hex = (blue << 16) | (green << 8) | red;

    bgr_hex
}

pub fn str_to_color(hex_string: &str) -> COLORREF {
    if let Ok(hex_value) = u32::from_str_radix(&hex_string[1..], 16) {
        COLORREF(rgb_to_bgr_hex(hex_value))
    } else {
        COLORREF(0)
    }
}