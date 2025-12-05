use egui::{
    CentralPanel, Color32, Context, CornerRadius, CursorIcon, EventFilter, Frame, Key, Pos2, Rect,
    Sense, StrokeKind, Ui, Vec2,
};

use duplicate::duplicate;
use paste::paste;

use super::{drag_and_drop::TreeComponent, state::State, tab_removal::TabRemoval};
use crate::dock_area::tab_removal::ForcedRemoval;
use crate::tab_viewer::OnCloseResponse;
use crate::{
    utils::{expand_to_pixel, fade_dock_style, map_to_pixel},
    AllowedDrops, AllowedSplits, DockArea, Node, NodeIndex, OverlayType, Style, SurfaceIndex,
    TabDestination, TabIndex, TabInsert, TabViewer,
};

mod leaf;
mod main_surface;
mod window_surface;

impl<Tab> DockArea<'_, Tab> {
    /// Show the `DockArea` at the top level.
    ///
    /// This is the same as doing:
    ///
    /// ```
    /// # use egui_dock::{DockArea, DockState};
    /// # use egui::{CentralPanel, Frame};
    /// # struct TabViewer {}
    /// # impl egui_dock::TabViewer for TabViewer {
    /// #     type Tab = String;
    /// #     fn title(&mut self, tab: &mut Self::Tab) -> egui::WidgetText { (&*tab).into() }
    /// #     fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Self::Tab) {}
    /// # }
    /// # let mut tree: DockState<String> = DockState::new(vec![]);
    /// # let mut tab_viewer = TabViewer {};
    /// # egui::__run_test_ctx(|ctx| {
    /// CentralPanel::default()
    ///     .frame(Frame::central_panel(&ctx.style()).inner_margin(0.))
    ///     .show(ctx, |ui| {
    ///         DockArea::new(&mut tree).show_inside(ui, &mut tab_viewer);
    ///     });
    /// # });
    /// ```
    ///
    /// So you can't use the [`CentralPanel::show`] when using `DockArea`'s one.
    ///
    /// See also [`show_inside`](Self::show_inside).
    #[inline]
    pub fn show(self, ctx: &Context, tab_viewer: &mut impl TabViewer<Tab = Tab>)
    where
        Tab: Clone,
    {
        CentralPanel::default()
            .frame(
                Frame::central_panel(&ctx.style())
                    .inner_margin(0.)
                    .fill(Color32::TRANSPARENT),
            )
            .show(ctx, |ui| {
                self.show_inside(ui, tab_viewer);
            });
    }

    /// Shows the docking hierarchy inside a [`Ui`].
    ///
    /// See also [`show`](Self::show).
    pub fn show_inside(mut self, ui: &mut Ui, tab_viewer: &mut impl TabViewer<Tab = Tab>)
    where
        Tab: Clone,
    {
        self.style
            .get_or_insert(Style::from_egui(ui.style().as_ref()));
        // `content_rect` was added after egui 0.32.1, so fall back to the screen rect.
        self.window_bounds
            .get_or_insert(ui.ctx().input(|i| i.screen_rect()));

        let mut state = State::load(ui.ctx(), self.id);

        // Delay hover position one frame. On touch screens hover_pos() is None when any_released()
        if !ui.input(|i| i.pointer.any_released()) {
            state.last_hover_pos = ui.input(|i| i.pointer.hover_pos());
        }

        let (drag_data, hover_data) = ui.memory_mut(|mem| {
            (
                mem.data.remove_temp(self.id.with("drag_data")).flatten(),
                mem.data.remove_temp(self.id.with("hover_data")).flatten(),
            )
        });

        if let (Some(source), Some(hover)) = (drag_data, hover_data) {
            let style = self.style.as_ref().unwrap();
            state.set_drag_and_drop(source, hover, ui.ctx(), style);
            let tab_dst = self.show_drag_drop_overlay(ui, &mut state, tab_viewer);
            if ui.input(|i| i.pointer.primary_released()) {
                if let Some(destination) = tab_dst {
                    let source = {
                        match state.dnd.as_ref().unwrap().drag.src {
                            TreeComponent::Tab(src_surf, src_node, src_tab) => {
                                (src_surf, src_node, src_tab)
                            }
                            _ => todo!(
                                "collections of tabs, like nodes and surfaces can't be docked (yet)"
                            ),
                        }
                    };
                    let allow_move = match destination {
                        TabDestination::Node(dst_surface, dst_node, _) => self
                            .dock_state
                            .get_tab(source)
                            .map(|tab| tab_viewer.allow_move_to(tab, dst_surface, dst_node))
                            .unwrap_or(true),
                        TabDestination::Window(_) => true,
                        TabDestination::EmptySurface(_) => false,
                    };
                    if allow_move {
                        self.dock_state.move_tab(source, destination);
                    }
                }
            }
        }

        if ui.input(|i| i.pointer.primary_released()) {
            state.reset_drag();
        }

        let style = self.style.as_ref().unwrap();
        let fade_surface =
            self.hovered_window_surface(&mut state, style.overlay.feel.fade_hold_time, ui.ctx());
        let fade_style = {
            fade_surface.is_some().then(|| {
                let mut fade_style = style.clone();
                fade_dock_style(&mut fade_style, style.overlay.surface_fade_opacity);
                (fade_style, style.overlay.surface_fade_opacity)
            })
        };

        for &surface_index in self.dock_state.valid_surface_indices().iter() {
            self.show_surface_inside(
                surface_index,
                ui,
                tab_viewer,
                &mut state,
                fade_style.as_ref().map(|(style, factor)| {
                    (style, *factor, fade_surface.unwrap_or(SurfaceIndex::main()))
                }),
            );
        }

        for removal in self.to_remove.drain(..).rev() {
            match removal {
                TabRemoval::Tab(surface, node, tab, ForcedRemoval(is_forced)) => {
                    if is_forced {
                        self.dock_state.remove_tab((surface, node, tab));
                    } else {
                        let leaf = &mut self.dock_state[surface][node].get_leaf_mut().unwrap();
                        match tab_viewer.on_close(&mut leaf.tabs[tab.0]) {
                            OnCloseResponse::Close => {
                                self.dock_state.remove_tab((surface, node, tab));
                            }
                            OnCloseResponse::Focus => {
                                leaf.active = tab;
                                self.new_focused = Some((surface, node));
                            }
                            OnCloseResponse::Ignore => {
                                // no-op
                            }
                        }
                    }
                }
                TabRemoval::Window(surface) => {
                    // Move all tabs back to main window instead of closing them
                    // Collect all tabs from the window surface
                    let mut tabs_to_move = Vec::new();
                    for node_index in self.dock_state[surface].breadth_first_index_iter() {
                        if let Some(leaf) = self.dock_state[surface][node_index].get_leaf() {
                            for tab_index in 0..leaf.tabs.len() {
                                tabs_to_move.push((surface, node_index, TabIndex(tab_index)));
                            }
                        }
                    }

                    // Move each tab to main window (in reverse to maintain order)
                    for (src_surface, src_node, src_tab) in tabs_to_move.into_iter().rev() {
                        // Check if main surface is empty
                        if self.dock_state.main_surface().is_empty() {
                            self.dock_state.move_tab(
                                (src_surface, src_node, src_tab),
                                TabDestination::EmptySurface(SurfaceIndex::main()),
                            );
                        } else {
                            // Try to use the original node ID and tab index
                            let window_state = self.dock_state.get_window_state(src_surface);
                            let original_node_id = window_state
                                .and_then(|ws| ws.original_node_id().map(|s| s.to_string()));
                            let original_tab_index =
                                window_state.and_then(|ws| ws.original_tab_index());

                            let dst_node = original_node_id
                                .and_then(|original_id| {
                                    // Find node by UUID
                                    self.dock_state.main_surface().find_node_by_id(&original_id)
                                })
                                // If original node not found, use focused leaf
                                .or_else(|| self.dock_state.main_surface().focused_leaf())
                                .or_else(|| {
                                    // Find the first visible, non-collapsed leaf node
                                    for node_index in
                                        self.dock_state.main_surface().breadth_first_index_iter()
                                    {
                                        if self.dock_state.main_surface()[node_index].is_leaf() {
                                            if let Some(leaf) = self.dock_state.main_surface()
                                                [node_index]
                                                .get_leaf()
                                            {
                                                if !leaf.hidden && !leaf.collapsed {
                                                    return Some(node_index);
                                                }
                                            }
                                        }
                                    }
                                    None
                                })
                                .unwrap_or(NodeIndex::root());

                            // Determine the insert position
                            let tab_insert = if let Some(original_index) = original_tab_index {
                                // Try to insert at original position
                                let leaf_len = self.dock_state.main_surface()[dst_node]
                                    .get_leaf()
                                    .map(|leaf| leaf.tabs.len())
                                    .unwrap_or(0);

                                // If original index is still valid, use it; otherwise append
                                if original_index <= leaf_len {
                                    TabInsert::Insert(TabIndex(original_index))
                                } else {
                                    TabInsert::Append
                                }
                            } else {
                                TabInsert::Append
                            };

                            self.dock_state.move_tab(
                                (src_surface, src_node, src_tab),
                                TabDestination::Node(SurfaceIndex::main(), dst_node, tab_insert),
                            );

                            // Ensure the destination leaf is visible
                            if let Some(leaf) =
                                self.dock_state.main_surface_mut()[dst_node].get_leaf_mut()
                            {
                                leaf.collapsed = false;
                                leaf.hidden = false;
                            }
                        }
                    }

                    // Now remove the empty surface
                    self.dock_state.remove_surface(surface);
                }
            }
        }

        for (surface_index, node_index, tab_index) in self.to_detach.drain(..).rev() {
            let mouse_pos = state.last_hover_pos;
            self.dock_state.detach_tab(
                (surface_index, node_index, tab_index),
                Rect::from_min_size(
                    mouse_pos.unwrap_or(Pos2::ZERO),
                    self.dock_state[surface_index][node_index]
                        .rect()
                        .map_or(Vec2::new(100., 150.), |rect| rect.size()),
                ),
            );
        }

        // Handle move_to_main_request
        if let Some(Some((src_surface, src_node, src_tab))) = ui.ctx().data_mut(|d| {
            d.remove_temp::<Option<(SurfaceIndex, NodeIndex, TabIndex)>>(
                self.id.with("move_to_main_request"),
            )
        }) {
            // Check if main surface is empty
            if self.dock_state.main_surface().is_empty() {
                // If main surface is empty, use EmptySurface destination
                self.dock_state.move_tab(
                    (src_surface, src_node, src_tab),
                    TabDestination::EmptySurface(SurfaceIndex::main()),
                );
            } else {
                // Try to use the original node ID and tab index
                let window_state = self.dock_state.get_window_state(src_surface);
                let original_node_id =
                    window_state.and_then(|ws| ws.original_node_id().map(|s| s.to_string()));
                let original_tab_index = window_state.and_then(|ws| ws.original_tab_index());

                let dst_node = original_node_id
                    .and_then(|original_id| {
                        // Find node by UUID
                        self.dock_state.main_surface().find_node_by_id(&original_id)
                    })
                    // If original node not found (e.g., was deleted), use focused leaf
                    .or_else(|| self.dock_state.main_surface().focused_leaf())
                    .or_else(|| {
                        // Find the first visible, non-collapsed leaf node
                        for node_index in self.dock_state.main_surface().breadth_first_index_iter()
                        {
                            if self.dock_state.main_surface()[node_index].is_leaf() {
                                if let Some(leaf) =
                                    self.dock_state.main_surface()[node_index].get_leaf()
                                {
                                    if !leaf.hidden && !leaf.collapsed {
                                        return Some(node_index);
                                    }
                                }
                            }
                        }
                        None
                    })
                    .unwrap_or(NodeIndex::root());

                // Determine the insert position
                let tab_insert = if let Some(original_index) = original_tab_index {
                    // Try to insert at original position
                    let leaf_len = self.dock_state.main_surface()[dst_node]
                        .get_leaf()
                        .map(|leaf| leaf.tabs.len())
                        .unwrap_or(0);

                    // If original index is still valid, use it; otherwise append
                    if original_index <= leaf_len {
                        TabInsert::Insert(TabIndex(original_index))
                    } else {
                        TabInsert::Append
                    }
                } else {
                    TabInsert::Append
                };

                self.dock_state.move_tab(
                    (src_surface, src_node, src_tab),
                    TabDestination::Node(SurfaceIndex::main(), dst_node, tab_insert),
                );

                // Ensure the destination leaf is visible and scrolled to show new tab
                if let Some(leaf) = self.dock_state.main_surface_mut()[dst_node].get_leaf_mut() {
                    leaf.collapsed = false;
                    leaf.hidden = false;
                    leaf.scroll = 0.0; // Reset scroll to beginning
                }

                // Set focus to the destination node
                self.new_focused = Some((SurfaceIndex::main(), dst_node));
            }
        }

        if let Some(focused) = self.new_focused {
            self.dock_state.set_focused_node_and_surface(focused);
        }

        // Handle fullscreen toggle requests emitted by tail buttons.
        if let Some(Some((surf, node, tab))) = ui.ctx().data_mut(|d| {
            d.remove_temp::<Option<(SurfaceIndex, NodeIndex, TabIndex)>>(
                self.id.with("fullscreen_request"),
            )
        }) {
            let _ = self.dock_state.toggle_fullscreen((surf, node, tab));
        }

        state.store(ui.ctx(), self.id);
    }

    /// Returns some when windows are fading, and what surface index is being hovered over
    #[inline(always)]
    fn hovered_window_surface(
        &self,
        state: &mut State,
        hold_time: f32,
        ctx: &Context,
    ) -> Option<SurfaceIndex> {
        if let Some(dnd_state) = &state.dnd {
            if dnd_state.is_locked(self.style.as_ref().unwrap(), ctx) {
                state.window_fade =
                    Some((ctx.input(|i| i.time), dnd_state.hover.dst.surface_address()));
            }
        }

        state.window_fade.and_then(|(time, surface)| {
            ctx.request_repaint();
            (hold_time > (ctx.input(|i| i.time) - time) as f32).then_some(surface)
        })
    }

    /// Resolve where a dragged tab would land given it's dropped this frame, returns `None` when the resulting drop is an invalid move.
    fn show_drag_drop_overlay(
        &mut self,
        ui: &Ui,
        state: &mut State,
        tab_viewer: &impl TabViewer<Tab = Tab>,
    ) -> Option<TabDestination> {
        let drag_state = state.dnd.as_mut().unwrap();
        let style = self.style.as_ref().unwrap();

        let (target_allowed, target_family_id) = match drag_state.hover.dst {
            TreeComponent::Node(surface, node) | TreeComponent::Tab(surface, node, _) => {
                let node = &self.dock_state[surface][node];
                (
                    node.allowed_drops()
                        .cloned()
                        .unwrap_or_else(AllowedDrops::all),
                    node.family_id().map(str::to_string),
                )
            }
            TreeComponent::Surface(surface) => {
                if let Some(root) = self.dock_state[surface].root_node() {
                    (
                        root.allowed_drops()
                            .cloned()
                            .unwrap_or_else(AllowedDrops::all),
                        root.family_id().map(str::to_string),
                    )
                } else {
                    (AllowedDrops::all(), None)
                }
            }
        };

        let src_family_id = match drag_state.drag.src {
            TreeComponent::Tab(surface, node, _) | TreeComponent::Node(surface, node) => self
                .dock_state[surface][node]
                .family_id()
                .map(str::to_string),
            TreeComponent::Surface(surface) => self.dock_state[surface]
                .root_node()
                .and_then(|node| node.family_id().map(str::to_string)),
        };

        let family_ok = match (src_family_id.as_deref(), target_family_id.as_deref()) {
            (Some(a), Some(b)) => a == b,
            _ => true,
        };

        let deserted_node = {
            match (
                drag_state.drag.src.node_address(),
                drag_state.hover.dst.node_address(),
            ) {
                ((src_surf, Some(src_node)), (dst_surf, Some(dst_node))) => {
                    src_surf == dst_surf
                        && src_node == dst_node
                        && self.dock_state[src_surf][src_node].tabs_count() == 1
                }
                _ => false,
            }
        };

        // Not all scenarios can house all splits.
        let restricted_splits = if drag_state.hover.dst.is_surface() || deserted_node {
            AllowedSplits::None
        } else {
            AllowedSplits::All
        };
        let node_splits = target_allowed.to_allowed_splits();
        let allowed_splits = if family_ok {
            (self.allowed_splits & restricted_splits) & node_splits
        } else {
            AllowedSplits::None
        };

        let allowed_in_window = match drag_state.drag.src {
            TreeComponent::Tab(surface, node, tab) => {
                let Node::Leaf(leaf) = &mut self.dock_state[surface][node] else {
                    unreachable!("tab drags can only come from leaf nodes")
                };
                tab_viewer.allowed_in_windows(&mut leaf.tabs[tab.0])
            }
            _ => todo!("collections of tabs, like nodes or surfaces, can't be dragged! (yet)"),
        };
        // 仅在同族且目标允许浮动时才允许窗口化，否则跨族悬停不触发浮动/投放。
        let windows_allowed = family_ok && allowed_in_window && target_allowed.float;
        // 同节点内也允许 tab 区域 drop，以便避免与浮窗区域冲突。
        let tabs_allowed = family_ok && target_allowed.tabs;

        if let Some(pointer) = state.last_hover_pos {
            drag_state.pointer = pointer;
        }

        let window_bounds = self.window_bounds.unwrap();
        match (style.overlay.overlay_type, drag_state.is_on_title_bar()) {
            (OverlayType::HighlightedAreas, _) | (_, true) => drag_state.resolve_traditional(
                ui,
                style,
                allowed_splits,
                windows_allowed,
                tabs_allowed,
                window_bounds,
            ),
            (OverlayType::Widgets, false) => drag_state.resolve_icon_based(
                ui,
                style,
                allowed_splits,
                windows_allowed,
                tabs_allowed,
                window_bounds,
            ),
        }
    }

    /// Show a single surface of a [`DockState`].
    fn show_surface_inside(
        &mut self,
        surf_index: SurfaceIndex,
        ui: &mut Ui,
        tab_viewer: &mut impl TabViewer<Tab = Tab>,
        state: &mut State,
        fade_style: Option<(&Style, f32, SurfaceIndex)>,
    ) {
        if surf_index.is_main() {
            self.show_root_surface_inside(ui, tab_viewer, state);
        } else {
            self.show_window_surface(ui, surf_index, tab_viewer, state, fade_style);
        }
    }

    fn render_nodes(
        &mut self,
        ui: &mut Ui,
        tab_viewer: &mut impl TabViewer<Tab = Tab>,
        state: &mut State,
        surf_index: SurfaceIndex,
        fade_style: Option<(&Style, f32)>,
    ) {
        // First compute all rect sizes in the node graph.
        let max_rect = self.allocate_area_for_root_node(ui, surf_index);
        for node_index in self.dock_state[surf_index].breadth_first_index_iter() {
            if self.dock_state[surf_index][node_index].is_parent() {
                self.compute_rect_sizes(ui, (surf_index, node_index), max_rect);
            }
        }

        // Then, draw the bodies of each leaves.
        for node_index in self.dock_state[surf_index].breadth_first_index_iter() {
            if self.dock_state[surf_index][node_index].is_leaf() {
                self.show_leaf(ui, state, (surf_index, node_index), tab_viewer, fade_style);
            }
        }

        // Finally, draw separators so that their "interaction zone" is above
        // bodies (see `SeparatorStyle::extra_interact_width`).
        let fade_style = fade_style.map(|(style, _)| style);
        for node_index in self.dock_state[surf_index].breadth_first_index_iter() {
            if self.dock_state[surf_index][node_index].is_parent() {
                self.show_separator(ui, (surf_index, node_index), fade_style);
            }
        }
    }

    fn allocate_area_for_root_node(&mut self, ui: &mut Ui, surface: SurfaceIndex) -> Rect {
        let style = self.style.as_ref().unwrap();
        let mut rect = ui.available_rect_before_wrap();

        if let Some(margin) = style.dock_area_padding {
            rect.min += margin.left_top();
            rect.max -= margin.right_bottom();
        }

        ui.painter().rect_stroke(
            rect,
            style.main_surface_border_rounding,
            style.main_surface_border_stroke,
            StrokeKind::Inside,
        );
        if surface == SurfaceIndex::main() {
            rect = rect.expand(-style.main_surface_border_stroke.width / 2.0);
        }
        ui.allocate_rect(rect, Sense::hover());

        if self.dock_state[surface].is_empty() {
            return rect;
        }
        self.dock_state[surface][NodeIndex::root()].set_rect(rect);
        rect
    }

    fn compute_rect_sizes(
        &mut self,
        ui: &Ui,
        (surface_index, node_index): (SurfaceIndex, NodeIndex),
        max_rect: Rect,
    ) {
        assert!(self.dock_state[surface_index][node_index].is_parent());

        let style = self.style.as_ref().unwrap();
        let pixels_per_point = ui.ctx().pixels_per_point();

        let left_collapsed_count =
            self.dock_state[surface_index][node_index.left()].collapsed_leaf_count();
        let right_collapsed_count =
            self.dock_state[surface_index][node_index.right()].collapsed_leaf_count();
        let left_collapsed = self.dock_state[surface_index][node_index.left()].is_collapsed();
        let right_collapsed = self.dock_state[surface_index][node_index.right()].is_collapsed();

        if left_collapsed || right_collapsed {
            if let Node::Vertical(split) = &mut self.dock_state[surface_index][node_index] {
                let rect = split.rect();
                debug_assert!(!rect.any_nan() && rect.is_finite());
                let rect = expand_to_pixel(rect, pixels_per_point);

                if left_collapsed {
                    // EITHER only left collapsed OR left and right both collapsed
                    let border_y =
                        rect.min.y + (left_collapsed_count as f32) * style.tab_bar.height;
                    let left_separator_border = map_to_pixel(
                        border_y - style.separator.width * 0.5,
                        pixels_per_point,
                        f32::round,
                    );
                    let right_separator_border = map_to_pixel(
                        border_y + style.separator.width * 0.5,
                        pixels_per_point,
                        f32::round,
                    );
                    let left = rect
                        .intersect(Rect::everything_above(left_separator_border))
                        .intersect(max_rect);
                    let right = rect
                        .intersect(Rect::everything_below(right_separator_border))
                        .intersect(max_rect);
                    self.dock_state[surface_index][node_index.left()].set_rect(left);
                    self.dock_state[surface_index][node_index.right()].set_rect(right);
                } else {
                    // Only right collapsed
                    let border_y =
                        rect.max.y - (right_collapsed_count as f32) * style.tab_bar.height;
                    let left_separator_border = map_to_pixel(
                        border_y - style.separator.width * 0.5,
                        pixels_per_point,
                        f32::round,
                    );
                    let right_separator_border = map_to_pixel(
                        border_y + style.separator.width * 0.5,
                        pixels_per_point,
                        f32::round,
                    );
                    let left = rect
                        .intersect(Rect::everything_above(left_separator_border))
                        .intersect(max_rect);
                    let right = rect
                        .intersect(Rect::everything_below(right_separator_border))
                        .intersect(max_rect);
                    self.dock_state[surface_index][node_index.left()].set_rect(left);
                    self.dock_state[surface_index][node_index.right()].set_rect(right);
                }
                return;
            }
        }

        duplicate! {
            [
                orientation   dim_point  dim_size  left_of    right_of;
                [Horizontal]  [x]        [width]   [left_of]  [right_of];
                [Vertical]    [y]        [height]  [above]    [below];
            ]
            if let Node::orientation(split) = &mut self.dock_state[surface_index][node_index] {
                let rect = split.rect;
                debug_assert!(!rect.any_nan() && rect.is_finite());
                let rect = expand_to_pixel(rect, pixels_per_point);

                let midpoint = rect.min.dim_point + rect.dim_size() * split.fraction;
                let left_separator_border = map_to_pixel(
                    midpoint - style.separator.width * 0.5,
                    pixels_per_point,
                    f32::round
                );
                let right_separator_border = map_to_pixel(
                    midpoint + style.separator.width * 0.5,
                    pixels_per_point,
                    f32::round
                );

                paste! {
                    let left = rect.intersect(Rect::[<everything_ left_of>](left_separator_border)).intersect(max_rect);
                    let right = rect.intersect(Rect::[<everything_ right_of>](right_separator_border)).intersect(max_rect);
                }

                self.dock_state[surface_index][node_index.left()].set_rect(left);
                self.dock_state[surface_index][node_index.right()].set_rect(right);
            }
        }
    }

    fn show_separator(
        &mut self,
        ui: &mut Ui,
        (surface_index, node_index): (SurfaceIndex, NodeIndex),
        fade_style: Option<&Style>,
    ) {
        assert!(self.dock_state[surface_index][node_index].is_parent());

        // If either of the children is collapsed, we don't want the user to interact with the separator
        if (self.dock_state[surface_index][node_index.left()].is_collapsed()
            || self.dock_state[surface_index][node_index.right()].is_collapsed())
            && self.dock_state[surface_index][node_index].is_vertical()
        {
            return;
        }

        let style = fade_style.unwrap_or_else(|| self.style.as_ref().unwrap());
        let pixels_per_point = ui.ctx().pixels_per_point();

        duplicate! {
            [
                orientation   dim_point  dim_size;
                [Horizontal]  [x]        [width];
                [Vertical]    [y]        [height];
            ]
            if let Node::orientation(split) = &mut self.dock_state[surface_index][node_index] {
                let rect = split.rect;
                let mut separator = rect;

                let midpoint = rect.min.dim_point + rect.dim_size() * split.fraction;
                separator.min.dim_point = midpoint - style.separator.width * 0.5;
                separator.max.dim_point = midpoint + style.separator.width * 0.5;

                let mut expand = Vec2::ZERO;
                expand.dim_point += style.separator.extra_interact_width / 2.0;
                let interact_rect = separator.expand2(expand);

                let response = ui.allocate_rect(interact_rect, Sense::click_and_drag())
                    .on_hover_and_drag_cursor(paste!{ CursorIcon::[<Resize orientation>]});

                let should_respond_to_arrow_keys = ui.input(|i| i.modifiers.command || i.modifiers.shift);

                if response.has_focus() {
                    // Prevent the default behaviour of removing focus from the separators when the
                    // arrow keys are pressed
                    ui.memory_mut(|m| m.set_focus_lock_filter(response.id, EventFilter {
                        horizontal_arrows: should_respond_to_arrow_keys,
                        vertical_arrows: should_respond_to_arrow_keys,
                        tab: false,
                        escape: false
                    }));
                }

                let arrow_key_offset = if response.has_focus() && should_respond_to_arrow_keys {
                    if ui.input(|i| i.key_pressed(Key::ArrowUp)) {
                        Some(egui::vec2(0., -16.))
                    } else if ui.input(|i| i.key_pressed(Key::ArrowDown)) {
                        Some(egui::vec2(0., 16.))
                    } else if ui.input(|i| i.key_pressed(Key::ArrowLeft)) {
                        Some(egui::vec2(-16., 0.))
                    } else if ui.input(|i| i.key_pressed(Key::ArrowRight)) {
                        Some(egui::vec2(16., 0.))
                    } else {
                        None
                    }
                } else {
                    None
                };

                let midpoint = rect.min.dim_point + rect.dim_size() * split.fraction;
                separator.min.dim_point = map_to_pixel(
                    midpoint - style.separator.width * 0.5,
                    pixels_per_point,
                    f32::round,
                );
                separator.max.dim_point = map_to_pixel(
                    midpoint + style.separator.width * 0.5,
                    pixels_per_point,
                    f32::round,
                );

                let color = if response.dragged() {
                    style.separator.color_dragged
                } else if response.hovered() || response.has_focus() {
                    style.separator.color_hovered
                } else {
                    style.separator.color_idle
                };

                ui.painter().rect_filled(separator, CornerRadius::ZERO, color);

                // Update 'fraction' interaction after drawing separator,
                // otherwise it may overlap on other separator / bodies when
                // shrunk fast.
                let range = rect.max.dim_point - rect.min.dim_point;
                let min_size = (style.tab_bar.height + style.separator.extra)
                    .max(style.separator.width);
                let min = (min_size / range).min(0.5);
                let mut max = 1.0 - min;
                if let Some(limit) = style.separator.max_fraction {
                    let limit = paste! { limit.[<dim_point>] };
                    if limit > 0.0 {
                        max = max.min(limit.clamp(0.0, 1.0));
                    }
                }
                let max = max.max(min);
                let (min, max) = (min.min(max), max.max(min));
                let delta = arrow_key_offset.unwrap_or(response.drag_delta()).dim_point;
                split.fraction = (split.fraction + delta / range).clamp(min, max);

                if response.double_clicked() {
                    split.fraction = 0.5;
                }
            }
        }
    }
}
