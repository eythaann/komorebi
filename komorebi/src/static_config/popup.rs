use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};

use crossbeam_utils::atomic::AtomicConsume;
use lazy_static::lazy_static;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::utils::ArcMutex;

use super::KomorebiConfig;

lazy_static! {
    pub static ref ENABLED: AtomicBool = AtomicBool::new(true);
    pub static ref POS_X: ArcMutex<Option<i32>> = ArcMutex(None);
    pub static ref POS_Y: ArcMutex<Option<i32>> = ArcMutex(None);
    pub static ref HEIGHT: AtomicI32 = AtomicI32::new(60);
    pub static ref WIDTH: AtomicI32 = AtomicI32::new(280);
    pub static ref TEXT_COLOR: ArcMutex<String> = ArcMutex("#33333".to_string());
    pub static ref BORDER_COLOR: ArcMutex<String> = ArcMutex("#1f1f1f".to_string());
    pub static ref BORDER_WIDTH: AtomicI32 = AtomicI32::new(0);
    pub static ref BACKGROUND: ArcMutex<String> = ArcMutex("#efefef".to_string());
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct PopupsConfig {
    enable: Option<bool>,
    x: Option<i32>,
    y: Option<i32>,
    height: Option<i32>,
    width: Option<i32>,
    text_color: Option<String>,
    border_color: Option<String>,
    border_width: Option<i32>,
    background: Option<String>,
}

impl KomorebiConfig for PopupsConfig {
    fn clone_globals() -> Self {
        Self {
            enable: Some(ENABLED.load_consume()),
            x: POS_X.lock().clone(),
            y: POS_Y.lock().clone(),
            height: Some(HEIGHT.load_consume()),
            width: Some(WIDTH.load_consume()),
            text_color: Some(TEXT_COLOR.lock().clone()),
            border_color: Some(BORDER_COLOR.lock().clone()),
            border_width: Some(BORDER_WIDTH.load_consume()),
            background: Some(BACKGROUND.lock().clone()),
        }
    }

    fn apply_to_globals(&self) -> color_eyre::eyre::Result<()> {
        if let Some(enabled) = self.enable {
            ENABLED.store(enabled, Ordering::SeqCst);
        }
        if let Some(x) = self.x {
            *POS_X.lock() = Some(x);
        }
        if let Some(y) = self.y {
            *POS_Y.lock() = Some(y);
        }
        if let Some(height) = self.height {
            HEIGHT.store(height, Ordering::SeqCst);
        }
        if let Some(width) = self.width {
            WIDTH.store(width, Ordering::SeqCst);
        }
        if let Some(text_color) = &self.text_color {
            *TEXT_COLOR.lock() = text_color.to_string();
        }
        if let Some(border_color) = &self.border_color {
            *BORDER_COLOR.lock() = border_color.to_string();
        }
        if let Some(border_width) = self.border_width {
            BORDER_WIDTH.store(border_width, Ordering::SeqCst);
        }
        if let Some(background) = &self.background {
            *BACKGROUND.lock() = background.to_string();
        }
        Ok(())
    }
}
