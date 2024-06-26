pub mod applications_configuration;
pub mod top_bar;
pub mod popup;

use crate::border::Border;
use crate::colour::Colour;
use crate::current_virtual_desktop;
use crate::monitor::Monitor;
use crate::ring::Ring;
use crate::window_manager::WindowManager;
use crate::window_manager_event::WindowManagerEvent;
use crate::windows_api::WindowsApi;
use crate::workspace::Workspace;
use crate::BORDER_COLOUR_CURRENT;
use crate::BORDER_COLOUR_MONOCLE;
use crate::BORDER_COLOUR_SINGLE;
use crate::BORDER_COLOUR_STACK;
use crate::BORDER_ENABLED;
use crate::BORDER_HWND;
use crate::BORDER_OFFSET;
use crate::BORDER_OVERFLOW_IDENTIFIERS;
use crate::BORDER_WIDTH;
use crate::DATA_DIR;
use crate::DEFAULT_CONTAINER_PADDING;
use crate::DEFAULT_WORKSPACE_PADDING;
use crate::DISPLAY_INDEX_PREFERENCES;
use crate::FINISH_MINIMIZE_ANIMATION;
use crate::FLOAT_IDENTIFIERS;
use crate::HIDING_BEHAVIOUR;
use crate::LAYERED_WHITELIST;
use crate::MANAGE_IDENTIFIERS;
use crate::MONITOR_INDEX_PREFERENCES;
use crate::NATIVE_ANIMATION_DELAY;
use crate::OBJECT_NAME_CHANGE_ON_LAUNCH;
use crate::REGEX_IDENTIFIERS;
use crate::STACK_BY_CATEGORY;
use crate::TRAY_AND_MULTI_WINDOW_IDENTIFIERS;
use crate::UNMANAGE_IDENTIFIERS;
use crate::WORKSPACE_RULES;
use crate::top_bar::TopBarConfig;
use crate::popup::PopupsConfig;
use crate::applications_configuration::SETTINGS_BY_APP;
use color_eyre::Result;
use crossbeam_channel::Receiver;
use hotwatch::EventKind;
use hotwatch::Hotwatch;
use komorebi_core::config_generation::ApplicationConfigurationGenerator;
use komorebi_core::config_generation::ApplicationOptions;
use komorebi_core::config_generation::IdWithIdentifier;
use komorebi_core::config_generation::MatchingStrategy;
use komorebi_core::resolve_home_path;
use komorebi_core::ApplicationIdentifier;
use komorebi_core::DefaultLayout;
use komorebi_core::FocusFollowsMouseImplementation;
use komorebi_core::HidingBehaviour;
use komorebi_core::Layout;
use komorebi_core::MoveBehaviour;
use komorebi_core::OperationBehaviour;
use komorebi_core::Rect;
use komorebi_core::SocketMessage;
use komorebi_core::WindowContainerBehaviour;
use parking_lot::Mutex;
use parking_lot::MutexGuard;
use regex::Regex;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;
use std::collections::HashSet;
use std::io::ErrorKind;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use uds_windows::UnixListener;
use uds_windows::UnixStream;

pub trait KomorebiConfig {
    fn clone_globals() -> Self;
    fn apply_to_globals(&self) -> Result<()>;
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct ActiveWindowBorderColours {
    /// Border colour when the container contains a single window
    pub single: Colour,
    /// Border colour when the container contains multiple windows
    pub stack: Colour,
    /// Border colour when the container is in monocle mode
    pub monocle: Colour,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct WorkspaceConfig {
    /// Name
    pub name: String,
    /// Layout (default: BSP)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layout: Option<DefaultLayout>,
    /// Custom Layout (default: None)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom_layout: Option<PathBuf>,
    /// Layout rules (default: None)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layout_rules: Option<HashMap<usize, DefaultLayout>>,
    /// Layout rules (default: None)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom_layout_rules: Option<HashMap<usize, PathBuf>>,
    /// Container padding (default: global)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub container_padding: Option<i32>,
    /// Container padding (default: global)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_padding: Option<i32>,
    /// Initial workspace application rules
    #[serde(skip_serializing_if = "Option::is_none")]
    pub initial_workspace_rules: Option<Vec<IdWithIdentifier>>,
    /// Permanent workspace application rules
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_rules: Option<Vec<IdWithIdentifier>>,
}

impl From<&Workspace> for WorkspaceConfig {
    fn from(value: &Workspace) -> Self {
        let mut layout_rules = HashMap::new();
        for (threshold, layout) in value.layout_rules() {
            match layout {
                Layout::Default(value) => {
                    layout_rules.insert(*threshold, *value);
                }
                Layout::Custom(_) => {}
            }
        }

        let workspace_rules = WORKSPACE_RULES.lock();
        let mut initial_ws_rules = vec![];
        let mut ws_rules = vec![];

        for (identifier, (_, _, is_initial)) in &*workspace_rules {
            if identifier.ends_with("exe") {
                let rule = IdWithIdentifier {
                    kind: ApplicationIdentifier::Exe,
                    id: identifier.clone(),
                    matching_strategy: None,
                };

                if *is_initial {
                    initial_ws_rules.push(rule);
                } else {
                    ws_rules.push(rule);
                }
            }
        }

        let initial_ws_rules = if initial_ws_rules.is_empty() {
            None
        } else {
            Option::from(initial_ws_rules)
        };
        let ws_rules = if ws_rules.is_empty() {
            None
        } else {
            Option::from(ws_rules)
        };

        let default_container_padding = DEFAULT_CONTAINER_PADDING.load(Ordering::SeqCst);
        let default_workspace_padding = DEFAULT_WORKSPACE_PADDING.load(Ordering::SeqCst);

        let container_padding = value.container_padding().and_then(|container_padding| {
            if container_padding == default_container_padding {
                None
            } else {
                Option::from(container_padding)
            }
        });

        let workspace_padding = value.workspace_padding().and_then(|workspace_padding| {
            if workspace_padding == default_workspace_padding {
                None
            } else {
                Option::from(workspace_padding)
            }
        });

        Self {
            name: value
                .name()
                .clone()
                .unwrap_or_else(|| String::from("unnamed")),
            layout: match value.layout() {
                Layout::Default(layout) => Option::from(*layout),
                // TODO: figure out how we might resolve file references in the future
                Layout::Custom(_) => None,
            },
            custom_layout: None,
            layout_rules: Option::from(layout_rules),
            // TODO: figure out how we might resolve file references in the future
            custom_layout_rules: None,
            container_padding,
            workspace_padding,
            initial_workspace_rules: initial_ws_rules,
            workspace_rules: ws_rules,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct MonitorConfig {
    /// Workspace configurations
    pub workspaces: Vec<WorkspaceConfig>,
    /// Monitor-specific work area offset (default: None)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub work_area_offset: Option<Rect>,
}

impl From<&Monitor> for MonitorConfig {
    fn from(value: &Monitor) -> Self {
        let mut workspaces = vec![];
        for w in value.workspaces() {
            workspaces.push(WorkspaceConfig::from(w));
        }

        Self {
            workspaces,
            work_area_offset: value.work_area_offset(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct AnimationsConfig {
    /// native windows animations delay for consecutive actions in ms (minimize, maximize, restore, etc)
    pub native_animations_delay: Option<u64>,
    /// wait for minimization before restore workspace
    pub finish_miminization_before_restore: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
/// The `komorebi.json` static configuration file reference for `v0.1.20`
pub struct StaticConfig {
    /// DEPRECATED from v0.1.22: no longer required
    #[serde(skip_serializing_if = "Option::is_none")]
    pub invisible_borders: Option<Rect>,
    /// Delta to resize windows by (default 50)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resize_delta: Option<i32>,
    /// Determine what happens when a new window is opened (default: Create)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window_container_behaviour: Option<WindowContainerBehaviour>,
    /// Determine what happens when a window is moved across a monitor boundary (default: Swap)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cross_monitor_move_behaviour: Option<MoveBehaviour>,
    /// Determine what happens when commands are sent while an unmanaged window is in the foreground (default: Op)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unmanaged_window_operation_behaviour: Option<OperationBehaviour>,
    /// Determine focus follows mouse implementation (default: None)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub focus_follows_mouse: Option<FocusFollowsMouseImplementation>,
    /// Enable or disable mouse follows focus (default: true)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mouse_follows_focus: Option<bool>,
    /// Path to applications.yaml from komorebi-application-specific-configurations (default: None)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app_specific_configuration_path: Option<PathBuf>,
    /// Width of the window border (default: 8)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(alias = "active_window_border_width")]
    pub border_width: Option<i32>,
    /// Offset of the window border (default: -1)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(alias = "active_window_border_offset")]
    pub border_offset: Option<i32>,
    /// Display an active window border (default: false)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_window_border: Option<bool>,
    /// Active window border colours for different container types
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_window_border_colours: Option<ActiveWindowBorderColours>,
    /// Global default workspace padding (default: 10)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_workspace_padding: Option<i32>,
    /// Global default container padding (default: 10)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_container_padding: Option<i32>,
    /// Monitor and workspace configurations
    #[serde(skip_serializing_if = "Option::is_none")]
    pub monitors: Option<Vec<MonitorConfig>>,
    /// Which Windows signal to use when hiding windows (default: minimize)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window_hiding_behaviour: Option<HidingBehaviour>,
    /// Global work area (space used for tiling) offset (default: None)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub global_work_area_offset: Option<Rect>,
    /// Animations configurations for windows, borders, etc.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub animations: Option<AnimationsConfig>,
    /// Individual window floating rules
    #[serde(skip_serializing_if = "Option::is_none")]
    pub float_rules: Option<Vec<IdWithIdentifier>>,
    /// Individual excluded window floating rules
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exclude_float_rules: Option<Vec<IdWithIdentifier>>,
    /// Individual unmanaged windows
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unmanage_rules: Option<Vec<IdWithIdentifier>>,
    /// Individual window force-manage rules
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manage_rules: Option<Vec<IdWithIdentifier>>,
    /// Identify border overflow applications
    #[serde(skip_serializing_if = "Option::is_none")]
    pub border_overflow_applications: Option<Vec<IdWithIdentifier>>,
    /// Identify tray and multi-window applications
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tray_and_multi_window_applications: Option<Vec<IdWithIdentifier>>,
    /// Identify applications that have the WS_EX_LAYERED extended window style
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layered_applications: Option<Vec<IdWithIdentifier>>,
    /// Identify applications that send EVENT_OBJECT_NAMECHANGE on launch (very rare)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub object_name_change_applications: Option<Vec<IdWithIdentifier>>,
    /// Set monitor index preferences
    #[serde(skip_serializing_if = "Option::is_none")]
    pub monitor_index_preferences: Option<HashMap<usize, Rect>>,
    /// Set display index preferences
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_index_preferences: Option<HashMap<usize, String>>,
    /// Top bar configurations
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_bar: Option<TopBarConfig>,
    /// Popups configurations
    #[serde(skip_serializing_if = "Option::is_none")]
    pub popups: Option<PopupsConfig>,
    /// Set categories stackin behavior.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_stack_by_category: Option<bool>,
}

impl From<&WindowManager> for StaticConfig {
    #[allow(clippy::too_many_lines)]
    fn from(value: &WindowManager) -> Self {
        let mut monitors = vec![];
        for m in value.monitors() {
            monitors.push(MonitorConfig::from(m));
        }

        let mut to_remove = vec![];

        let workspace_rules = WORKSPACE_RULES.lock();
        for (m_idx, m) in monitors.iter().enumerate() {
            for (w_idx, w) in m.workspaces.iter().enumerate() {
                if let Some(rules) = &w.initial_workspace_rules {
                    for iwsr in rules {
                        for (identifier, (monitor_idx, workspace_idx, _)) in &*workspace_rules {
                            if iwsr.id.eq(identifier)
                                && (*monitor_idx != m_idx || *workspace_idx != w_idx)
                            {
                                to_remove.push((m_idx, w_idx, iwsr.id.clone()));
                            }
                        }
                    }
                }

                if let Some(rules) = &w.workspace_rules {
                    for wsr in rules {
                        for (identifier, (monitor_idx, workspace_idx, _)) in &*workspace_rules {
                            if wsr.id.eq(identifier)
                                && (*monitor_idx != m_idx || *workspace_idx != w_idx)
                            {
                                to_remove.push((m_idx, w_idx, wsr.id.clone()));
                            }
                        }
                    }
                }
            }
        }

        for (m_idx, w_idx, id) in to_remove {
            if let Some(monitor) = monitors.get_mut(m_idx) {
                if let Some(workspace) = monitor.workspaces.get_mut(w_idx) {
                    if let Some(rules) = &mut workspace.workspace_rules {
                        rules.retain(|r| r.id != id);
                    }

                    if let Some(rules) = &mut workspace.initial_workspace_rules {
                        rules.retain(|r| r.id != id);
                    }
                }
            }
        }

        let border_colours = if BORDER_COLOUR_SINGLE.load(Ordering::SeqCst) == 0 {
            None
        } else {
            Option::from(ActiveWindowBorderColours {
                single: Colour::from(BORDER_COLOUR_SINGLE.load(Ordering::SeqCst)),
                stack: Colour::from(if BORDER_COLOUR_STACK.load(Ordering::SeqCst) == 0 {
                    BORDER_COLOUR_SINGLE.load(Ordering::SeqCst)
                } else {
                    BORDER_COLOUR_STACK.load(Ordering::SeqCst)
                }),
                monocle: Colour::from(if BORDER_COLOUR_MONOCLE.load(Ordering::SeqCst) == 0 {
                    BORDER_COLOUR_SINGLE.load(Ordering::SeqCst)
                } else {
                    BORDER_COLOUR_MONOCLE.load(Ordering::SeqCst)
                }),
            })
        };

        Self {
            invisible_borders: None,
            resize_delta: Option::from(value.resize_delta),
            window_container_behaviour: Option::from(value.window_container_behaviour),
            cross_monitor_move_behaviour: Option::from(value.cross_monitor_move_behaviour),
            unmanaged_window_operation_behaviour: Option::from(
                value.unmanaged_window_operation_behaviour,
            ),
            focus_follows_mouse: value.focus_follows_mouse,
            mouse_follows_focus: Option::from(value.mouse_follows_focus),
            app_specific_configuration_path: None,
            border_width: Option::from(BORDER_WIDTH.load(Ordering::SeqCst)),
            border_offset: Option::from(BORDER_OFFSET.load(Ordering::SeqCst)),
            active_window_border: Option::from(BORDER_ENABLED.load(Ordering::SeqCst)),
            active_window_border_colours: border_colours,
            default_workspace_padding: Option::from(
                DEFAULT_WORKSPACE_PADDING.load(Ordering::SeqCst),
            ),
            default_container_padding: Option::from(
                DEFAULT_CONTAINER_PADDING.load(Ordering::SeqCst),
            ),
            monitors: Option::from(monitors),
            window_hiding_behaviour: Option::from(*HIDING_BEHAVIOUR.lock()),
            global_work_area_offset: value.work_area_offset,
            animations: Option::from(AnimationsConfig {
                native_animations_delay: Option::from(
                    NATIVE_ANIMATION_DELAY.load(Ordering::SeqCst),
                ),
                finish_miminization_before_restore: Option::from(
                    FINISH_MINIMIZE_ANIMATION.load(Ordering::SeqCst),
                ),
            }),
            float_rules: None,
            exclude_float_rules: None,
            manage_rules: None,
            unmanage_rules: None,
            border_overflow_applications: None,
            tray_and_multi_window_applications: None,
            layered_applications: None,
            object_name_change_applications: None,
            monitor_index_preferences: Option::from(MONITOR_INDEX_PREFERENCES.lock().clone()),
            display_index_preferences: Option::from(DISPLAY_INDEX_PREFERENCES.lock().clone()),
            top_bar: Option::from(TopBarConfig::clone_globals()),
            popups: Option::from(PopupsConfig::clone_globals()),
            auto_stack_by_category: None,
        }
    }
}

impl StaticConfig {
    fn apply_global(
        rules: &mut Option<Vec<IdWithIdentifier>>,
        identifiers: &mut MutexGuard<'_, Vec<IdWithIdentifier>>,
        regex_identifiers: &mut MutexGuard<'_, HashMap<String, Regex>>,
    ) -> Result<()> {
        if let Some(rules) = rules {
            for rule in rules {
                if rule.matching_strategy.is_none() {
                    rule.matching_strategy = Option::from(MatchingStrategy::Legacy);
                }

                if !identifiers.contains(rule) {
                    identifiers.push(rule.clone());

                    if matches!(rule.matching_strategy, Some(MatchingStrategy::Regex)) {
                        let regex = Regex::new(&rule.id)?;
                        regex_identifiers.insert(rule.id.clone(), regex);
                    }
                }
            }
        }

        Ok(())
    }

    #[allow(clippy::cognitive_complexity, clippy::too_many_lines)]
    fn apply_globals(&mut self) -> Result<()> {
        if let Some(top_bar) = &self.top_bar {
            top_bar.apply_to_globals()?;
        }

        if let Some(popups) = &self.popups {
            popups.apply_to_globals()?;
        }

        if let Some(auto_stack_by_category) = self.auto_stack_by_category {
            let mut preferences = STACK_BY_CATEGORY.lock();
            *preferences = auto_stack_by_category
        }

        if let Some(monitor_index_preferences) = &self.monitor_index_preferences {
            let mut preferences = MONITOR_INDEX_PREFERENCES.lock();
            *preferences = monitor_index_preferences.clone();
        }

        if let Some(display_index_preferences) = &self.display_index_preferences {
            let mut preferences = DISPLAY_INDEX_PREFERENCES.lock();
            *preferences = display_index_preferences.clone();
        }

        if let Some(behaviour) = self.window_hiding_behaviour {
            let mut window_hiding_behaviour = HIDING_BEHAVIOUR.lock();
            *window_hiding_behaviour = behaviour;
        }

        if let Some(container) = self.default_container_padding {
            DEFAULT_CONTAINER_PADDING.store(container, Ordering::SeqCst);
        }

        if let Some(workspace) = self.default_workspace_padding {
            DEFAULT_WORKSPACE_PADDING.store(workspace, Ordering::SeqCst);
        }

        self.border_width.map_or_else(
            || {
                BORDER_WIDTH.store(8, Ordering::SeqCst);
            },
            |width| {
                BORDER_WIDTH.store(width, Ordering::SeqCst);
            },
        );

        BORDER_OFFSET.store(self.border_offset.unwrap_or(-1), Ordering::SeqCst);

        if let Some(animations) = &self.animations {
            if let Some(delay) = animations.native_animations_delay {
                NATIVE_ANIMATION_DELAY.store(delay, Ordering::SeqCst);
            }
            if let Some(val) = animations.finish_miminization_before_restore {
                FINISH_MINIMIZE_ANIMATION.store(val, Ordering::SeqCst);
            }
        }

        if let Some(colours) = &self.active_window_border_colours {
            BORDER_COLOUR_SINGLE.store(u32::from(colours.single), Ordering::SeqCst);
            BORDER_COLOUR_CURRENT.store(u32::from(colours.single), Ordering::SeqCst);
            BORDER_COLOUR_STACK.store(u32::from(colours.stack), Ordering::SeqCst);
            BORDER_COLOUR_MONOCLE.store(u32::from(colours.monocle), Ordering::SeqCst);
        }

        let mut float_identifiers: parking_lot::lock_api::MutexGuard<
            '_,
            parking_lot::RawMutex,
            Vec<IdWithIdentifier>,
        > = FLOAT_IDENTIFIERS.lock();
        let mut unmanage_identifiers = UNMANAGE_IDENTIFIERS.lock();
        let mut manage_identifiers = MANAGE_IDENTIFIERS.lock();
        let mut regex_identifiers = REGEX_IDENTIFIERS.lock();
        let mut tray_and_multi_window_identifiers = TRAY_AND_MULTI_WINDOW_IDENTIFIERS.lock();
        let mut border_overflow_identifiers = BORDER_OVERFLOW_IDENTIFIERS.lock();
        let mut object_name_change_identifiers = OBJECT_NAME_CHANGE_ON_LAUNCH.lock();
        let mut layered_identifiers = LAYERED_WHITELIST.lock();

        Self::apply_global(
            &mut self.float_rules,
            &mut float_identifiers,
            &mut regex_identifiers,
        )?;
        Self::apply_global(
            &mut self.manage_rules,
            &mut manage_identifiers,
            &mut regex_identifiers,
        )?;
        Self::apply_global(
            &mut self.unmanage_rules,
            &mut unmanage_identifiers,
            &mut regex_identifiers,
        )?;

        Self::apply_global(
            &mut self.object_name_change_applications,
            &mut object_name_change_identifiers,
            &mut regex_identifiers,
        )?;
        Self::apply_global(
            &mut self.layered_applications,
            &mut layered_identifiers,
            &mut regex_identifiers,
        )?;
        Self::apply_global(
            &mut self.border_overflow_applications,
            &mut border_overflow_identifiers,
            &mut regex_identifiers,
        )?;
        Self::apply_global(
            &mut self.tray_and_multi_window_applications,
            &mut tray_and_multi_window_identifiers,
            &mut regex_identifiers,
        )?;

        if let Some(path) = &self.app_specific_configuration_path {
            let path = resolve_home_path(path)?;
            let content = std::fs::read_to_string(path)?;
            let asc = ApplicationConfigurationGenerator::load(&content)?;

            for mut entry in asc {
                drop(regex_identifiers);
                SETTINGS_BY_APP.lock().add(entry.clone().into());
                regex_identifiers = REGEX_IDENTIFIERS.lock();

                // Todo remove all these
                if let Some(float) = entry.float_identifiers {
                    for f in float {
                        let mut without_comment: IdWithIdentifier = f.into();
                        if without_comment.matching_strategy.is_none() {
                            without_comment.matching_strategy =
                                Option::from(MatchingStrategy::Legacy);
                        }

                        if !float_identifiers.contains(&without_comment) {
                            float_identifiers.push(without_comment.clone());

                            if matches!(
                                without_comment.matching_strategy,
                                Some(MatchingStrategy::Regex)
                            ) {
                                let re = Regex::new(&without_comment.id)?;
                                regex_identifiers.insert(without_comment.id.clone(), re);
                            }
                        }
                    }
                }
                if let Some(options) = entry.options {
                    for o in options {
                        match o {
                            ApplicationOptions::Float => {
                                if entry.identifier.matching_strategy.is_none() {
                                    entry.identifier.matching_strategy =
                                        Option::from(MatchingStrategy::Legacy);
                                }

                                if !float_identifiers.contains(&entry.identifier) {
                                    float_identifiers.push(entry.identifier.clone());

                                    if matches!(
                                        entry.identifier.matching_strategy,
                                        Some(MatchingStrategy::Regex)
                                    ) {
                                        let re = Regex::new(&entry.identifier.id)?;
                                        regex_identifiers.insert(entry.identifier.id.clone(), re);
                                    }
                                }
                            }
                            ApplicationOptions::Unmanage => {
                                if entry.identifier.matching_strategy.is_none() {
                                    entry.identifier.matching_strategy =
                                        Option::from(MatchingStrategy::Legacy);
                                }

                                if !unmanage_identifiers.contains(&entry.identifier) {
                                    unmanage_identifiers.push(entry.identifier.clone());

                                    if matches!(
                                        entry.identifier.matching_strategy,
                                        Some(MatchingStrategy::Regex)
                                    ) {
                                        let re = Regex::new(&entry.identifier.id)?;
                                        regex_identifiers.insert(entry.identifier.id.clone(), re);
                                    }
                                }
                            }
                            ApplicationOptions::ObjectNameChange => {
                                if entry.identifier.matching_strategy.is_none() {
                                    entry.identifier.matching_strategy =
                                        Option::from(MatchingStrategy::Legacy);
                                }

                                if !object_name_change_identifiers.contains(&entry.identifier) {
                                    object_name_change_identifiers.push(entry.identifier.clone());

                                    if matches!(
                                        entry.identifier.matching_strategy,
                                        Some(MatchingStrategy::Regex)
                                    ) {
                                        let re = Regex::new(&entry.identifier.id)?;
                                        regex_identifiers.insert(entry.identifier.id.clone(), re);
                                    }
                                }
                            }
                            ApplicationOptions::Layered => {
                                if entry.identifier.matching_strategy.is_none() {
                                    entry.identifier.matching_strategy =
                                        Option::from(MatchingStrategy::Legacy);
                                }

                                if !layered_identifiers.contains(&entry.identifier) {
                                    layered_identifiers.push(entry.identifier.clone());

                                    if matches!(
                                        entry.identifier.matching_strategy,
                                        Some(MatchingStrategy::Regex)
                                    ) {
                                        let re = Regex::new(&entry.identifier.id)?;
                                        regex_identifiers.insert(entry.identifier.id.clone(), re);
                                    }
                                }
                            }
                            ApplicationOptions::BorderOverflow => {
                                if entry.identifier.matching_strategy.is_none() {
                                    entry.identifier.matching_strategy =
                                        Option::from(MatchingStrategy::Legacy);
                                }

                                if !border_overflow_identifiers.contains(&entry.identifier) {
                                    border_overflow_identifiers.push(entry.identifier.clone());

                                    if matches!(
                                        entry.identifier.matching_strategy,
                                        Some(MatchingStrategy::Regex)
                                    ) {
                                        let re = Regex::new(&entry.identifier.id)?;
                                        regex_identifiers.insert(entry.identifier.id.clone(), re);
                                    }
                                }
                            }
                            ApplicationOptions::TrayAndMultiWindow => {
                                if entry.identifier.matching_strategy.is_none() {
                                    entry.identifier.matching_strategy =
                                        Option::from(MatchingStrategy::Legacy);
                                }

                                if !tray_and_multi_window_identifiers.contains(&entry.identifier) {
                                    tray_and_multi_window_identifiers
                                        .push(entry.identifier.clone());

                                    if matches!(
                                        entry.identifier.matching_strategy,
                                        Some(MatchingStrategy::Regex)
                                    ) {
                                        let re = Regex::new(&entry.identifier.id)?;
                                        regex_identifiers.insert(entry.identifier.id.clone(), re);
                                    }
                                }
                            }
                            ApplicationOptions::Force => {
                                if entry.identifier.matching_strategy.is_none() {
                                    entry.identifier.matching_strategy =
                                        Option::from(MatchingStrategy::Legacy);
                                }

                                if !manage_identifiers.contains(&entry.identifier) {
                                    manage_identifiers.push(entry.identifier.clone());

                                    if matches!(
                                        entry.identifier.matching_strategy,
                                        Some(MatchingStrategy::Regex)
                                    ) {
                                        let re = Regex::new(&entry.identifier.id)?;
                                        regex_identifiers.insert(entry.identifier.id.clone(), re);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }

    #[allow(clippy::too_many_lines)]
    pub fn preload(
        path: &PathBuf,
        incoming: Receiver<WindowManagerEvent>,
    ) -> Result<WindowManager> {
        let content = std::fs::read_to_string(path)?;
        let mut value: Self = serde_json::from_str(&content)?;
        value.apply_globals()?;

        let socket = DATA_DIR.join("komorebi.sock");

        match std::fs::remove_file(&socket) {
            Ok(()) => {}
            Err(error) => match error.kind() {
                // Doing this because ::exists() doesn't work reliably on Windows via IntelliJ
                ErrorKind::NotFound => {}
                _ => {
                    return Err(error.into());
                }
            },
        };

        let listener = UnixListener::bind(&socket)?;

        let mut wm = WindowManager {
            monitors: Ring::default(),
            monitor_cache: HashMap::new(),
            incoming_events: incoming,
            command_listener: listener,
            is_paused: false,
            virtual_desktop_id: current_virtual_desktop(),
            work_area_offset: value.global_work_area_offset,
            window_container_behaviour: value
                .window_container_behaviour
                .unwrap_or(WindowContainerBehaviour::Create),
            cross_monitor_move_behaviour: value
                .cross_monitor_move_behaviour
                .unwrap_or(MoveBehaviour::Swap),
            unmanaged_window_operation_behaviour: value
                .unmanaged_window_operation_behaviour
                .unwrap_or(OperationBehaviour::Op),
            resize_delta: value.resize_delta.unwrap_or(50),
            focus_follows_mouse: value.focus_follows_mouse,
            mouse_follows_focus: value.mouse_follows_focus.unwrap_or(true),
            hotwatch: Hotwatch::new()?,
            has_pending_raise_op: false,
            pending_move_op: None,
            already_moved_window_handles: Arc::new(Mutex::new(HashSet::new())),
        };

        match value.focus_follows_mouse {
            None => WindowsApi::disable_focus_follows_mouse()?,
            Some(FocusFollowsMouseImplementation::Windows) => {
                WindowsApi::enable_focus_follows_mouse()?;
            }
            Some(FocusFollowsMouseImplementation::Komorebi) => {}
        };

        let bytes = SocketMessage::ReloadStaticConfiguration(path.clone()).as_bytes()?;

        wm.hotwatch.watch(path, move |event| match event.kind {
            // Editing in Notepad sends a NoticeWrite while editing in (Neo)Vim sends
            // a NoticeRemove, presumably because of the use of swap files?
            EventKind::Modify(_) | EventKind::Remove(_) => {
                let socket = DATA_DIR.join("komorebi.sock");
                let mut stream =
                    UnixStream::connect(socket).expect("could not connect to komorebi.sock");
                stream
                    .write_all(&bytes)
                    .expect("could not write to komorebi.sock");
            }
            _ => {}
        })?;

        Ok(wm)
    }

    pub fn postload(path: &PathBuf, wm: &Arc<Mutex<WindowManager>>) -> Result<()> {
        let content = std::fs::read_to_string(path)?;
        let value: Self = serde_json::from_str(&content)?;
        let mut wm = wm.lock();

        if let Some(monitors) = value.monitors {
            for (i, monitor) in monitors.iter().enumerate() {
                if let Some(m) = wm.monitors_mut().get_mut(i) {
                    m.ensure_workspace_count(monitor.workspaces.len());
                    m.set_work_area_offset(monitor.work_area_offset);

                    for (j, ws) in m.workspaces_mut().iter_mut().enumerate() {
                        ws.load_static_config(
                            monitor
                                .workspaces
                                .get(j)
                                .expect("no static workspace config"),
                        )?;
                    }
                }
            }
        }

        let _ = wm.enforce_workspace_rules();

        if value.active_window_border == Some(true) {
            if BORDER_HWND.load(Ordering::SeqCst) == 0 {
                Border::create("komorebi-border-window")?;
            }

            BORDER_ENABLED.store(true, Ordering::SeqCst);
            wm.show_border()?;
        }

        Ok(())
    }

    pub fn reload(path: &PathBuf, wm: &mut WindowManager) -> Result<()> {
        let content = std::fs::read_to_string(path)?;
        let mut value: Self = serde_json::from_str(&content)?;

        value.apply_globals()?;

        if let Some(monitors) = value.monitors {
            for (i, monitor) in monitors.iter().enumerate() {
                if let Some(m) = wm.monitors_mut().get_mut(i) {
                    m.ensure_workspace_count(monitor.workspaces.len());
                    m.set_work_area_offset(monitor.work_area_offset);

                    for (j, ws) in m.workspaces_mut().iter_mut().enumerate() {
                        ws.load_static_config(
                            monitor
                                .workspaces
                                .get(j)
                                .expect("no static workspace config"),
                        )?;
                    }
                }
            }
        }

        if value.active_window_border == Some(true) {
            if BORDER_HWND.load(Ordering::SeqCst) == 0 {
                Border::create("komorebi-border-window")?;
            }

            BORDER_ENABLED.store(true, Ordering::SeqCst);
            wm.show_border()?;
        } else {
            BORDER_ENABLED.store(false, Ordering::SeqCst);
            wm.hide_border()?;
        }

        if let Some(val) = value.window_container_behaviour {
            wm.window_container_behaviour = val;
        }

        if let Some(val) = value.cross_monitor_move_behaviour {
            wm.cross_monitor_move_behaviour = val;
        }

        if let Some(val) = value.unmanaged_window_operation_behaviour {
            wm.unmanaged_window_operation_behaviour = val;
        }

        if let Some(val) = value.resize_delta {
            wm.resize_delta = val;
        }

        if let Some(val) = value.mouse_follows_focus {
            wm.mouse_follows_focus = val;
        }

        wm.work_area_offset = value.global_work_area_offset;

        match value.focus_follows_mouse {
            None => WindowsApi::disable_focus_follows_mouse()?,
            Some(FocusFollowsMouseImplementation::Windows) => {
                WindowsApi::enable_focus_follows_mouse()?;
            }
            Some(FocusFollowsMouseImplementation::Komorebi) => {}
        };

        wm.focus_follows_mouse = value.focus_follows_mouse;

        let monitor_count = wm.monitors().len();

        for i in 0..monitor_count {
            wm.update_focused_workspace_by_monitor_idx(i)?;
        }

        Ok(())
    }
}
