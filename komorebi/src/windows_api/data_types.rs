
use windows::core::PCWSTR;

use super::WindowsApi;

#[derive(Clone)]
#[allow(non_camel_case_types)]
pub struct SAFE_PCWSTR(Vec<u16>);
impl SAFE_PCWSTR {
    pub fn get(self) -> PCWSTR {
        PCWSTR(self.0.as_ptr())
    }
}

impl WindowsApi {
    pub fn pcwstr(string: &str) -> SAFE_PCWSTR {
        SAFE_PCWSTR(string.encode_utf16().collect::<Vec<u16>>())
    }
}
