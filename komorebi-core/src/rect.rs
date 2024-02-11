use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use windows::Win32::Foundation::RECT;

#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize, Eq, PartialEq, JsonSchema)]
pub struct Rect {
    /// The left point in a Win32 Rect
    pub left: i32,
    /// The top point in a Win32 Rect
    pub top: i32,
    /// The right point in a Win32 Rect
    pub right: i32,
    /// The bottom point in a Win32 Rect
    pub bottom: i32,
}

impl From<RECT> for Rect {
    fn from(rect: RECT) -> Self {
        Self {
            left: rect.left,
            top: rect.top,
            right: rect.right - rect.left,
            bottom: rect.bottom - rect.top,
        }
    }
}

impl From<Rect> for RECT {
    fn from(rect: Rect) -> Self {
        Self {
            left: rect.left,
            top: rect.top,
            right: rect.right,
            bottom: rect.bottom,
        }
    }
}

impl Rect {
    pub fn add_padding<T>(&mut self, padding: T)
    where
        T: Into<Option<i32>>,
    {
        if let Some(padding) = padding.into() {
            self.left += padding;
            self.top += padding;
            self.right -= padding * 2;
            self.bottom -= padding * 2;
        }
    }

    pub fn left_padding(&mut self, padding: i32){
        self.left += padding;
    }

    pub fn right_padding(&mut self, padding: i32){
        self.right -= padding;
    }

    #[must_use]
    pub const fn contains_point(&self, point: (i32, i32)) -> bool {
        point.0 >= self.left
            && point.0 <= self.left + self.right
            && point.1 >= self.top
            && point.1 <= self.top + self.bottom
    }
}
