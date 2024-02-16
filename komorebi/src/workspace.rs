use std::collections::VecDeque;
use std::num::NonZeroUsize;
use std::sync::atomic::Ordering;

use color_eyre::eyre::anyhow;
use color_eyre::Result;
use getset::CopyGetters;
use getset::Getters;
use getset::MutGetters;
use getset::Setters;
use schemars::JsonSchema;
use serde::Serialize;

use komorebi_core::Axis;
use komorebi_core::CustomLayout;
use komorebi_core::CycleDirection;
use komorebi_core::DefaultLayout;
use komorebi_core::Layout;
use komorebi_core::OperationDirection;
use komorebi_core::Rect;
use windows::Win32::Foundation::HWND;

use crate::container::Container;
use crate::ring::Ring;
use crate::static_config::WorkspaceConfig;
use crate::static_config::top_bar::TOP_BAR_HEIGH;

use crate::window::Window;
use crate::window::WindowDetails;
use crate::windows_api::WindowsApi;
use crate::DEFAULT_CONTAINER_PADDING;
use crate::DEFAULT_WORKSPACE_PADDING;
use crate::INITIAL_CONFIGURATION_LOADED;
use crate::NO_TITLEBAR;
use crate::REMOVE_TITLEBARS;
use crate::STACK_BY_CATEGORY;

#[allow(clippy::struct_field_names)]
#[derive(Debug, Clone, Serialize, Getters, CopyGetters, MutGetters, Setters, JsonSchema)]
pub struct Workspace {
    #[getset(get = "pub", set = "pub")]
    name: Option<String>,
    containers: Ring<Container>,
    #[getset(get = "pub", get_mut = "pub", set = "pub")]
    monocle_container: Option<Container>,
    #[serde(skip_serializing)]
    #[getset(get_copy = "pub", set = "pub")]
    monocle_container_restore_idx: Option<usize>,
    #[getset(get = "pub", get_mut = "pub")]
    floating_windows: Vec<Window>,
    #[getset(get = "pub", get_mut = "pub", set = "pub")]
    layout: Layout,
    #[getset(get = "pub", get_mut = "pub", set = "pub")]
    layout_rules: Vec<(usize, Layout)>,
    #[getset(get_copy = "pub", set = "pub")]
    layout_flip: Option<Axis>,
    #[getset(get_copy = "pub", set = "pub")]
    workspace_padding: Option<i32>,
    #[getset(get_copy = "pub", set = "pub")]
    container_padding: Option<i32>,
    #[serde(skip_serializing)]
    #[getset(get = "pub", set = "pub")]
    latest_layout: Vec<Rect>,
    #[getset(get = "pub", get_mut = "pub", set = "pub")]
    resize_dimensions: Vec<Option<Rect>>,
    #[getset(get = "pub", set = "pub")]
    tile: bool,
}

impl_ring_elements!(Workspace, Container);

impl Default for Workspace {
    fn default() -> Self {
        Self {
            name: None,
            containers: Ring::default(),
            monocle_container: None,
            monocle_container_restore_idx: None,
            floating_windows: Vec::default(),
            layout: Layout::Default(DefaultLayout::BSP),
            layout_rules: vec![],
            layout_flip: None,
            workspace_padding: Option::from(DEFAULT_WORKSPACE_PADDING.load(Ordering::SeqCst)),
            container_padding: Option::from(DEFAULT_CONTAINER_PADDING.load(Ordering::SeqCst)),
            latest_layout: vec![],
            resize_dimensions: vec![],
            tile: true,
        }
    }
}

impl Workspace {
    pub fn load_static_config(&mut self, config: &WorkspaceConfig) -> Result<()> {
        self.name = Option::from(config.name.clone());

        if config.container_padding.is_some() {
            self.set_container_padding(config.container_padding);
        }

        if config.workspace_padding.is_some() {
            self.set_workspace_padding(config.workspace_padding);
        }

        if let Some(layout) = &config.layout {
            self.layout = Layout::Default(*layout);
            self.tile = true;
        }

        if let Some(pathbuf) = &config.custom_layout {
            let layout = CustomLayout::from_path(pathbuf)?;
            self.layout = Layout::Custom(layout);
            self.tile = true;
        }

        if config.custom_layout.is_none() && config.layout.is_none() {
            self.tile = false;
        }

        if let Some(layout_rules) = &config.layout_rules {
            let mut all_rules = vec![];
            for (count, rule) in layout_rules {
                all_rules.push((*count, Layout::Default(*rule)));
            }

            self.set_layout_rules(all_rules);
        }

        if let Some(layout_rules) = &config.custom_layout_rules {
            let rules = self.layout_rules_mut();
            for (count, pathbuf) in layout_rules {
                let rule = CustomLayout::from_path(pathbuf)?;
                rules.push((*count, Layout::Custom(rule)));
            }
        }

        Ok(())
    }

    pub fn hide(&mut self, omit: Option<HWND>) -> Result<()> {
        for window in self.floating_windows_mut().iter_mut().rev() {
            if omit.is_none() || omit.unwrap() != window.hwnd() {
                window.hide();
            }
        }

        for container in self.containers_mut().iter_mut().rev() {
            container.hide(omit)?;
        }

        if let Some(container) = self.monocle_container_mut() {
            container.hide(omit)?;
        }
        Ok(())
    }

    pub fn get_window_to_be_maximized(&self) -> Option<&Window> {
        if let Some(container) = self.monocle_container() {
            for window in container.windows() {
                if window.is_programmatically_maximized() {
                    return Some(window);
                }
            }
        }

        for window in self.floating_windows() {
            if window.is_programmatically_maximized() {
                return Some(window);
            }
        }

        for container in self.containers() {
            for window in container.windows() {
                if window.is_programmatically_maximized() {
                    return Some(window);
                }
            }
        }
        None
    }

    pub fn restore(&mut self, mouse_follows_focus: bool) -> Result<()> {
        if let Some(maximized) = self.get_window_to_be_maximized() {
            maximized.maximize();
            maximized.focus(mouse_follows_focus)?;

            for window in self.floating_windows() {
                if window.hwnd != maximized.hwnd {
                    window.restore()?;
                }
            }

            return Ok(());
        }

        for container in self.containers_mut() {
            container.restore()?;
        }

        if let Some(container) = self.monocle_container_mut() {
            container.restore()?;
        }

        for window in self.floating_windows() {
            window.restore()?;
        }

        if let Some(container) = self.focused_container_mut() {
            container.focus_window(container.focused_window_idx());
        }

        Ok(())
    }

    pub fn update(
        &mut self,
        work_area: &Rect,
        offset: Option<Rect>,
        invisible_borders: &Rect,
    ) -> Result<()> {
        if !INITIAL_CONFIGURATION_LOADED.load(Ordering::SeqCst) {
            return Ok(());
        }

        let container_padding = self.container_padding();
        let mut adjusted_work_area = offset.map_or_else(
            || *work_area,
            |offset| {
                let mut with_offset = *work_area;
                with_offset.left += offset.left;
                with_offset.top += offset.top;
                with_offset.right -= offset.right;
                with_offset.bottom -= offset.bottom;

                with_offset
            },
        );

        adjusted_work_area.add_padding(self.workspace_padding());

        self.enforce_resize_constraints();

        if !self.layout_rules().is_empty() {
            let mut updated_layout = None;

            for rule in self.layout_rules() {
                if self.containers().len() >= rule.0 {
                    updated_layout = Option::from(rule.1.clone());
                }
            }

            if let Some(updated_layout) = updated_layout {
                if !matches!(updated_layout, Layout::Default(DefaultLayout::BSP)) {
                    self.set_layout_flip(None);
                }

                self.set_layout(updated_layout);
            }
        }

        if *self.tile() {
            if let Some(container) = self.monocle_container_mut() {
                if let Some(window) = container.focused_window_mut() {
                    adjusted_work_area.add_padding(container_padding);
                    if !window.is_maximized() {
                        window.set_position(&adjusted_work_area, invisible_borders, true)?;
                    }
                };
            } else if !self.containers().is_empty() {
                let layouts = self.layout().as_boxed_arrangement().calculate(
                    &adjusted_work_area,
                    NonZeroUsize::new(self.containers().len()).ok_or_else(|| {
                        anyhow!(
                            "there must be at least one container to calculate a workspace layout"
                        )
                    })?,
                    self.container_padding(),
                    self.layout_flip(),
                    self.resize_dimensions(),
                );

                let should_remove_titlebars = REMOVE_TITLEBARS.load(Ordering::SeqCst);
                let no_titlebar = NO_TITLEBAR.lock().clone();

                let containers = self.containers();
                for (i, container) in containers.into_iter().enumerate() {
                    if let (Some(window), Some(layout)) =
                        (container.focused_window(), layouts.get(i))
                    {
                        if should_remove_titlebars && no_titlebar.contains(&window.exe()?) {
                            window.remove_title_bar()?;
                        } else if no_titlebar.contains(&window.exe()?) {
                            window.add_title_bar()?;
                        }

                        if !window.is_maximized() {
                            let mut window_layout = layout.clone();
                            if let Some(top_bar) = container.top_bar() {
                                top_bar.set_position(
                                    &top_bar.get_position_from_container_layout(layout),
                                    false,
                                )?;
                                top_bar.update(container.windows())?;
                                let height = *TOP_BAR_HEIGH.lock();
                                let total_height = height + self.container_padding().or(Some(0)).unwrap();
                                window_layout.top += total_height;
                                window_layout.bottom -= total_height;
                            }
                            window.set_position(&window_layout, invisible_borders, false)?;
                        }
                    }
                }

                self.set_latest_layout(layouts);
            }
        }

        // Always make sure that the length of the resize dimensions vec is the same as the
        // number of layouts / containers. This should never actually truncate as the remove_window
        // function takes care of cleaning up resize dimensions when destroying empty containers
        let container_count = self.containers().len();
        self.resize_dimensions_mut().resize(container_count, None);

        Ok(())
    }

    pub fn reap_orphans(&mut self) -> Result<(usize, usize)> {
        let mut hwnds = vec![];
        let mut floating_hwnds = vec![];

        for window in self.visible_windows_mut().into_iter().flatten() {
            if !window.is_window() {
                hwnds.push(window.hwnd);
            }
        }

        for window in self.floating_windows() {
            if !window.is_window() {
                floating_hwnds.push(window.hwnd);
            }
        }

        for hwnd in &hwnds {
            tracing::debug!("reaping hwnd: {}", hwnd);
            self.remove_window(*hwnd)?;
        }

        for hwnd in &floating_hwnds {
            tracing::debug!("reaping floating hwnd: {}", hwnd);
            self.floating_windows_mut()
                .retain(|w| !floating_hwnds.contains(&w.hwnd));
        }

        let mut container_to_delete = vec![];
        for container in self.containers() {
            if container.windows().is_empty() {
                container_to_delete.push(container.id().clone());
            }
        }

        self.containers_mut()
            .retain(|c| !container_to_delete.contains(c.id()));

        Ok((
            hwnds.len() + floating_hwnds.len(),
            container_to_delete.len(),
        ))
    }

    pub fn container_for_window(&self, hwnd: isize) -> Option<&Container> {
        self.containers().get(self.container_idx_for_window(hwnd)?)
    }

    pub fn focus_container_by_window(&mut self, hwnd: isize) -> Result<()> {
        let container_idx = self
            .container_idx_for_window(hwnd)
            .ok_or_else(|| anyhow!("there is no container/window"))?;

        let container = self
            .containers_mut()
            .get_mut(container_idx)
            .ok_or_else(|| anyhow!("there is no container"))?;

        let window_idx = container
            .idx_for_window(hwnd)
            .ok_or_else(|| anyhow!("there is no window"))?;

        let mut should_load = false;

        if container.focused_window_idx() != window_idx {
            should_load = true
        }

        container.focus_window(window_idx);

        if should_load {
            container.load_focused_window()?;
        }

        self.focus_container(container_idx);

        Ok(())
    }

    pub fn container_idx_from_current_point(&self) -> Option<usize> {
        let mut idx = None;

        let point = WindowsApi::cursor_pos().ok()?;

        for (i, _container) in self.containers().iter().enumerate() {
            if let Some(rect) = self.latest_layout().get(i) {
                if rect.contains_point((point.x, point.y)) {
                    idx = Option::from(i);
                }
            }
        }

        idx
    }

    pub fn hwnd_from_exe(&self, exe: &str) -> Option<isize> {
        for container in self.containers() {
            if let Some(hwnd) = container.hwnd_from_exe(exe) {
                return Option::from(hwnd);
            }
        }

        if let Some(container) = self.monocle_container() {
            if let Some(hwnd) = container.hwnd_from_exe(exe) {
                return Option::from(hwnd);
            }
        }

        for window in self.floating_windows() {
            if let Ok(window_exe) = window.exe() {
                if exe == window_exe {
                    return Option::from(window.hwnd);
                }
            }
        }

        None
    }

    pub fn contains_managed_window(&self, hwnd: isize) -> bool {
        for container in self.containers() {
            if container.contains_window(hwnd) {
                return true;
            }
        }

        if let Some(container) = self.monocle_container() {
            if container.contains_window(hwnd) {
                return true;
            }
        }

        false
    }

    pub fn is_focused_window_monocle_or_maximized(&self) -> Result<bool> {
        let hwnd = WindowsApi::foreground_window()?;

        if let Some(container) = self.monocle_container() {
            if container.contains_window(hwnd) {
                return Ok(true);
            }
        }

        Ok(false)
    }

    pub fn contains_window(&self, hwnd: isize) -> bool {
        for container in self.containers() {
            if container.contains_window(hwnd) {
                return true;
            }
        }

        if let Some(container) = self.monocle_container() {
            if container.contains_window(hwnd) {
                return true;
            }
        }

        for window in self.floating_windows() {
            if hwnd == window.hwnd {
                return true;
            }
        }

        false
    }

    pub fn promote_container(&mut self) -> Result<()> {
        let resize = self.resize_dimensions_mut().remove(0);
        let container = self
            .remove_focused_container()
            .ok_or_else(|| anyhow!("there is no container"))?;

        let primary_idx = match self.layout() {
            Layout::Default(_) => 0,
            Layout::Custom(layout) => layout.first_container_idx(
                layout
                    .primary_idx()
                    .ok_or_else(|| anyhow!("this custom layout does not have a primary column"))?,
            ),
        };

        self.containers_mut().insert(primary_idx, container);
        self.resize_dimensions_mut().insert(primary_idx, resize);

        self.focus_container(primary_idx);

        Ok(())
    }

    pub fn add_container(&mut self, container: Container) {
        self.containers_mut().push_back(container);
        self.focus_last_container();
    }

    pub fn insert_container_at_idx(&mut self, idx: usize, container: Container) {
        self.containers_mut().insert(idx, container);
    }

    pub fn remove_container_by_idx(&mut self, idx: usize) -> Option<Container> {
        if idx < self.resize_dimensions().len() {
            self.resize_dimensions_mut().remove(idx);
        }

        if idx < self.containers().len() {
            return self.containers_mut().remove(idx);
        }

        None
    }

    fn container_idx_for_window(&self, hwnd: isize) -> Option<usize> {
        let mut idx = None;
        for (i, x) in self.containers().iter().enumerate() {
            if x.contains_window(hwnd) {
                idx = Option::from(i);
            }
        }

        idx
    }

    pub fn remove_window(&mut self, hwnd: isize) -> Result<()> {
        if self.floating_windows().iter().any(|w| w.hwnd == hwnd) {
            self.floating_windows_mut().retain(|w| w.hwnd != hwnd);
            return Ok(());
        }

        if let Some(container) = self.monocle_container_mut() {
            if let Some(window_idx) = container
                .windows()
                .iter()
                .position(|window| window.hwnd == hwnd)
            {
                container
                    .remove_window_by_idx(window_idx)
                    .ok_or_else(|| anyhow!("there is no window"))?;

                if container.windows().is_empty() {
                    self.set_monocle_container(None);
                    self.set_monocle_container_restore_idx(None);
                }

                return Ok(());
            }
        }

        let container_idx = self
            .container_idx_for_window(hwnd)
            .ok_or_else(|| anyhow!("there is no window"))?;

        let container = self
            .containers_mut()
            .get_mut(container_idx)
            .ok_or_else(|| anyhow!("there is no container"))?;

        let window_idx = container
            .windows()
            .iter()
            .position(|window| window.hwnd == hwnd)
            .ok_or_else(|| anyhow!("there is no window"))?;

        container
            .remove_window_by_idx(window_idx)
            .ok_or_else(|| anyhow!("there is no window"))?;

        if container.windows().is_empty() {
            self.containers_mut()
                .remove(container_idx)
                .ok_or_else(|| anyhow!("there is no container"))?;

            // Whenever a container is empty, we need to remove any resize dimensions for it too
            if self.resize_dimensions().get(container_idx).is_some() {
                self.resize_dimensions_mut().remove(container_idx);
            }

            self.focus_previous_container();
        } else {
            container.load_focused_window()?;
        }

        Ok(())
    }

    pub fn remove_focused_container(&mut self) -> Option<Container> {
        let focused_idx = self.focused_container_idx();
        let container = self.remove_container_by_idx(focused_idx);
        self.focus_previous_container();

        container
    }

    pub fn remove_container(&mut self, idx: usize) -> Option<Container> {
        let container = self.remove_container_by_idx(idx);
        self.focus_previous_container();
        container
    }

    pub fn new_idx_for_direction(&self, direction: OperationDirection) -> Option<usize> {
        let len = NonZeroUsize::new(self.containers().len())?;

        direction.destination(
            self.layout().as_boxed_direction().as_ref(),
            self.layout_flip(),
            self.focused_container_idx(),
            len,
        )
    }
    pub fn new_idx_for_cycle_direction(&self, direction: CycleDirection) -> Option<usize> {
        Option::from(direction.next_idx(
            self.focused_container_idx(),
            NonZeroUsize::new(self.containers().len())?,
        ))
    }

    pub fn move_window_to_container(&mut self, target_container_idx: usize) -> Result<()> {
        let focused_idx = self.focused_container_idx();

        let container = self
            .focused_container_mut()
            .ok_or_else(|| anyhow!("there is no container"))?;

        let window = container
            .remove_focused_window()
            .ok_or_else(|| anyhow!("there is no window"))?;

        // This is a little messy
        let adjusted_target_container_index = if container.windows().is_empty() {
            self.containers_mut().remove(focused_idx);
            self.resize_dimensions_mut().remove(focused_idx);

            if focused_idx < target_container_idx {
                target_container_idx - 1
            } else {
                target_container_idx
            }
        } else {
            container.load_focused_window()?;
            target_container_idx
        };

        let target_container = self
            .containers_mut()
            .get_mut(adjusted_target_container_index)
            .ok_or_else(|| anyhow!("there is no container"))?;

        target_container.add_window(window);

        self.focus_container(adjusted_target_container_index);
        self.focused_container_mut()
            .ok_or_else(|| anyhow!("there is no container"))?
            .load_focused_window()?;

        Ok(())
    }

    pub fn new_container_for_focused_window(&mut self) -> Result<()> {
        let focused_container_idx = self.focused_container_idx();

        let container = self
            .focused_container_mut()
            .ok_or_else(|| anyhow!("there is no container"))?;

        let window = container
            .remove_focused_window()
            .ok_or_else(|| anyhow!("there is no window"))?;

        if container.windows().is_empty() {
            self.containers_mut().remove(focused_container_idx);
            self.resize_dimensions_mut().remove(focused_container_idx);
        } else {
            container.load_focused_window()?;
        }

        self.new_container_for_window(window);
        Ok(())
    }

    pub fn unfloat_focused_window(&mut self) -> Result<()> {
        let window = self
            .remove_focused_floating_window()
            .ok_or_else(|| anyhow!("there is no floating window"))?;

        if *STACK_BY_CATEGORY.lock() {
            if let Some(container) = self.get_container_by_category(window) {
                container.add_window(window);
                return Ok(());
            }
        }

        let focused_idx = self.focused_container_idx();
        let mut container = Container::default();
        container.add_window(window);
        self.containers_mut().insert(focused_idx, container);
        self.resize_dimensions_mut().insert(focused_idx, None);

        Ok(())
    }

    pub fn get_container_by_category(&mut self, window: Window) -> Option<&mut Container> {
        if let Some(category) = window.category() {
            for container in self.containers_mut() {
                if container.categories().iter().any(|s| s.eq(&category)) {
                    return Option::from(container);
                }
            }
        }
        None
    }

    pub fn new_container_for_window(&mut self, window: Window) {
        if *STACK_BY_CATEGORY.lock() {
            if let Some(container) = self.get_container_by_category(window) {
                container.add_window(window);
                return;
            }
        }

        let next_idx = if self.containers().is_empty() {
            0
        } else {
            self.focused_container_idx() + 1
        };

        let mut container = Container::default();
        container.add_window(window);

        if next_idx > self.containers().len() {
            self.containers_mut().push_back(container);
        } else {
            self.containers_mut().insert(next_idx, container);
        }

        if next_idx > self.resize_dimensions().len() {
            self.resize_dimensions_mut().push(None);
        } else {
            self.resize_dimensions_mut().insert(next_idx, None);
        }

        self.focus_container(next_idx);
    }

    pub fn float_focused_window(&mut self) -> Result<()> {
        let window = if let Some(monocle_container) = self.monocle_container_mut() {
            let window = monocle_container
                .remove_focused_window()
                .ok_or_else(|| anyhow!("there is no window"))?;

            if monocle_container.windows().is_empty() {
                self.set_monocle_container(None);
                self.set_monocle_container_restore_idx(None);
            } else {
                monocle_container.load_focused_window()?;
            }

            window
        } else {
            let focused_idx = self.focused_container_idx();

            let container = self
                .focused_container_mut()
                .ok_or_else(|| anyhow!("there is no container"))?;

            let window = container
                .remove_focused_window()
                .ok_or_else(|| anyhow!("there is no window"))?;

            if container.windows().is_empty() {
                self.containers_mut().remove(focused_idx);
                self.resize_dimensions_mut().remove(focused_idx);
            } else {
                container.load_focused_window()?;
            }

            window
        };

        self.floating_windows_mut().push(window);

        Ok(())
    }

    pub fn focused_window(&self) -> Option<&Window> {
        let focused_floating_window = self.focused_floating_window();
        if focused_floating_window.is_some() {
            focused_floating_window
        } else if let Some(monocle_container) = self.monocle_container() {
            monocle_container.focused_window()
        } else if let Some(container) = self.focused_container() {
            container.focused_window()
        } else {
            None
        }
    }

    pub fn toggle_maximize_focused_window(&mut self, mouse_follows_focus: bool) -> Result<()> {
        let focused_window = self
            .focused_window()
            .ok_or_else(|| anyhow!("there is no window"))?;
        if focused_window.is_maximized() {
            self.unmaximize_focused_window(mouse_follows_focus)?;
        } else {
            self.maximize_focused_window()?;
        }
        Ok(())
    }

    pub fn maximize_focused_window(&mut self) -> Result<()> {
        let focused_window = self
            .focused_window()
            .ok_or_else(|| anyhow!("there is no window"))?;
        focused_window.maximize();
        self.hide(Some(focused_window.hwnd()))?;
        Ok(())
    }

    pub fn unmaximize_focused_window(&mut self, mouse_follows_focus: bool) -> Result<()> {
        let focused_window = self
            .focused_window()
            .ok_or_else(|| anyhow!("there is no window"))?;
        focused_window.unmaximize();
        self.restore(mouse_follows_focus)
    }

    fn enforce_resize_constraints(&mut self) {
        match self.layout {
            Layout::Default(DefaultLayout::BSP) => self.enforce_resize_constraints_for_bsp(),
            Layout::Default(DefaultLayout::UltrawideVerticalStack) => {
                self.enforce_resize_for_ultrawide();
            }
            _ => self.enforce_no_resize(),
        }
    }

    fn enforce_resize_constraints_for_bsp(&mut self) {
        for (i, rect) in self.resize_dimensions_mut().iter_mut().enumerate() {
            if let Some(rect) = rect {
                // Even containers can't be resized to the bottom
                if i % 2 == 0 {
                    rect.bottom = 0;
                    // Odd containers can't be resized to the right
                } else {
                    rect.right = 0;
                }
            }
        }

        // The first container can never be resized to the left or the top
        if let Some(Some(first)) = self.resize_dimensions_mut().first_mut() {
            first.top = 0;
            first.left = 0;
        }

        // The last container can never be resized to the bottom or the right
        if let Some(Some(last)) = self.resize_dimensions_mut().last_mut() {
            last.bottom = 0;
            last.right = 0;
        }
    }

    fn enforce_resize_for_ultrawide(&mut self) {
        let resize_dimensions = self.resize_dimensions_mut();
        match resize_dimensions.len() {
            // Single window can not be resized at all
            0 | 1 => self.enforce_no_resize(),
            // Two windows can only be resized in the middle
            2 => {
                // Zero is actually on the right
                if let Some(mut right) = resize_dimensions[0] {
                    right.top = 0;
                    right.bottom = 0;
                    right.right = 0;
                }

                // One is on the left
                if let Some(mut left) = resize_dimensions[1] {
                    left.top = 0;
                    left.bottom = 0;
                    left.left = 0;
                }
            }
            // Three or more windows means 0 is in center, 1 is at the left, 2.. are a vertical
            // stack on the right
            _ => {
                // Central can be resized left or right
                if let Some(mut right) = resize_dimensions[0] {
                    right.top = 0;
                    right.bottom = 0;
                }

                // Left one can only be resized to the right
                if let Some(mut left) = resize_dimensions[1] {
                    left.top = 0;
                    left.bottom = 0;
                    left.left = 0;
                }

                // Handle stack on the right
                let stack_size = resize_dimensions[2..].len();
                for (i, rect) in resize_dimensions[2..].iter_mut().enumerate() {
                    if let Some(rect) = rect {
                        // No containers can resize to the right
                        rect.right = 0;

                        // First container in stack cant resize up
                        if i == 0 {
                            rect.top = 0;
                        } else if i == stack_size - 1 {
                            // Last cant be resized to the bottom
                            rect.bottom = 0;
                        }
                    }
                }
            }
        }
    }

    fn enforce_no_resize(&mut self) {
        for rect in self.resize_dimensions_mut().iter_mut().flatten() {
            rect.left = 0;
            rect.right = 0;
            rect.top = 0;
            rect.bottom = 0;
        }
    }

    pub fn new_monocle_container(&mut self) -> Result<()> {
        let focused_idx = self.focused_container_idx();
        let container = self
            .containers_mut()
            .remove(focused_idx)
            .ok_or_else(|| anyhow!("there is no container"))?;

        // We don't remove any resize adjustments for a monocle, because when this container is
        // inevitably reintegrated, it would be weird if it doesn't go back to the dimensions
        // it had before

        self.set_monocle_container(Option::from(container));
        self.set_monocle_container_restore_idx(Option::from(focused_idx));
        self.focus_previous_container();

        self.monocle_container_mut()
            .as_mut()
            .ok_or_else(|| anyhow!("there is no monocle container"))?
            .load_focused_window()?;

        Ok(())
    }

    pub fn reintegrate_monocle_container(&mut self) -> Result<()> {
        let restore_idx = self
            .monocle_container_restore_idx()
            .ok_or_else(|| anyhow!("there is no monocle restore index"))?;

        let container = self
            .monocle_container_mut()
            .as_ref()
            .ok_or_else(|| anyhow!("there is no monocle container"))?;

        let container = container.clone();
        if restore_idx > self.containers().len() - 1 {
            self.containers_mut()
                .resize(restore_idx, Container::default());
        }

        self.containers_mut().insert(restore_idx, container);
        self.focus_container(restore_idx);
        self.focused_container_mut()
            .ok_or_else(|| anyhow!("there is no container"))?
            .load_focused_window()?;

        self.set_monocle_container(None);
        self.set_monocle_container_restore_idx(None);

        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub fn focus_container(&mut self, idx: usize) {
        tracing::info!("focusing container");

        self.containers.focus(idx);
    }

    pub fn swap_containers(&mut self, i: usize, j: usize) {
        self.containers.swap(i, j);
        self.focus_container(j);
    }

    pub fn focused_floating_window_idx(&self) -> Option<usize> {
        let hwnd = WindowsApi::foreground_window().ok()?;

        let mut idx = None;
        for (i, window) in self.floating_windows.iter().enumerate() {
            if hwnd == window.hwnd {
                idx = Option::from(i);
            }
        }
        idx
    }

    pub fn focused_floating_window(&self) -> Option<&Window> {
        let idx = self.focused_floating_window_idx();
        match idx {
            None => None,
            Some(idx) => self.floating_windows().get(idx),
        }
    }

    pub fn remove_focused_floating_window(&mut self) -> Option<Window> {
        let idx = self.focused_floating_window_idx();
        match idx {
            None => None,
            Some(idx) => {
                if self.floating_windows.get(idx).is_some() {
                    Option::from(self.floating_windows_mut().remove(idx))
                } else {
                    None
                }
            }
        }
    }

    pub fn visible_windows(&self) -> Vec<Option<&Window>> {
        let mut vec = vec![];
        for container in self.containers() {
            vec.push(container.focused_window());
        }

        vec
    }

    pub fn visible_window_details(&self) -> Vec<WindowDetails> {
        let mut vec: Vec<WindowDetails> = vec![];

        for container in self.containers() {
            if let Some(focused) = container.focused_window() {
                if let Ok(details) = (*focused).try_into() {
                    vec.push(details);
                }
            }
        }

        vec
    }

    pub fn visible_windows_mut(&mut self) -> Vec<Option<&mut Window>> {
        let mut vec = vec![];
        for container in self.containers_mut() {
            vec.push(container.focused_window_mut());
        }

        vec
    }

    fn focus_previous_container(&mut self) {
        let focused_idx = self.focused_container_idx();

        if focused_idx != 0 {
            self.focus_container(focused_idx - 1);
        }
    }

    fn focus_last_container(&mut self) {
        self.focus_container(self.containers().len() - 1);
    }
}
