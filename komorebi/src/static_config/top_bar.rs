use lazy_static::lazy_static;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::utils::ArcMutex;

use super::KomorebiConfig;

lazy_static! {
    pub static ref TOP_BAR_HEIGH: ArcMutex<i32> = ArcMutex(40);
    pub static ref TOP_BAR_MODE: ArcMutex<String> = ArcMutex("Always".to_string());
    pub static ref TAB_TEXT_COLOR: ArcMutex<String> = ArcMutex("#efefef".to_string());
    pub static ref TAB_BACKGROUND: ArcMutex<String> = ArcMutex("#333333".to_string());
    pub static ref TAB_WIDTH: ArcMutex<i32> = ArcMutex(200);
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct TabsConfig {
    width: Option<i32>,
    color: Option<String>,
    background: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct TopBarConfig {
    height: Option<i32>,
    mode: Option<String>,
    tabs: Option<TabsConfig>,
}

impl Default for TopBarConfig {
    fn default() -> Self {
        Self {
            mode: Some(TOP_BAR_MODE.lock().clone()),
            height: Some(TOP_BAR_HEIGH.lock().clone()),
            tabs: Some(TabsConfig {
                width: Some(TAB_WIDTH.lock().clone()),
                color: Some(TAB_TEXT_COLOR.lock().clone()),
                background: Some(TAB_BACKGROUND.lock().clone()),
            }),
        }
    }
}

impl KomorebiConfig for TopBarConfig {
    fn apply_to_globals(&self) -> color_eyre::eyre::Result<()> {
        if let Some(height) = self.height {
            *TOP_BAR_HEIGH.lock() = height;
        }
        if let Some(mode) = &self.mode {
            if ["Always", "OnStack", "Never"].contains(&mode.as_str()) {
                *TOP_BAR_MODE.lock() = mode.to_string();
            }
        }
        if let Some(tabs) = &self.tabs {
            if let Some(width) = tabs.width {
                *TAB_WIDTH.lock() = width;
            }
            if let Some(color) = &tabs.color {
                *TAB_TEXT_COLOR.lock() = color.to_string();
            }
            if let Some(background) = &tabs.background {
                *TAB_BACKGROUND.lock() = background.to_string();
            }
        }
        Ok(())
    }
}
