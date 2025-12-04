use egui::{
    emath::TSTransform, epaint::TextShape, lerp, pos2, vec2, Align, Align2, Button, Color32,
    CornerRadius, CursorIcon, Frame, Id, Key, LayerId, Layout, NumExt, Order, Popup,
    PopupCloseBehavior, Pos2, Rect, Response, ScrollArea, Sense, Shape, Stroke, StrokeKind,
    TextStyle, Ui, UiBuilder, Vec2, WidgetText,
};
use std::f32::consts::FRAC_PI_2;

use crate::dock_area::tab_removal::{ForcedRemoval, TabRemoval};
use crate::node::LeafNode;
use crate::{
    dock_area::{
        drag_and_drop::{DragData, DragDropState, HoverData, TreeComponent},
        state::State,
    },
    utils::{fade_visuals, rect_set_size_centered, rect_stroke_box},
    DockArea, Node, NodeIndex, Style, SurfaceIndex, TabBarPosition, TabIndex, TabStyle, TabViewer,
};

use crate::tab_viewer::OnCloseResponse;

impl<Tab> DockArea<'_, Tab> {
    pub(super) fn show_leaf(
        &mut self,
        ui: &mut Ui,
        state: &mut State,
        (surface_index, node_index): (SurfaceIndex, NodeIndex),
        tab_viewer: &mut impl TabViewer<Tab = Tab>,
        fade_style: Option<(&Style, f32)>,
    ) {
        assert!(self.dock_state[surface_index][node_index].is_leaf());
        if self.dock_state[surface_index][node_index]
            .get_leaf()
            .is_some_and(|leaf| leaf.hidden)
        {
            return;
        }
        let collapsed = self.dock_state[surface_index][node_index].is_collapsed();

        let rect = self.dock_state[surface_index][node_index]
            .rect()
            .expect("This node must be a leaf");
        let default_position = fade_style
            .map(|(style, _)| style.tab_bar.position)
            .unwrap_or_else(|| self.style.as_ref().unwrap().tab_bar.position);
        let collapse_allowed = tab_viewer.allow_collapse(surface_index, node_index);
        let position = {
            let leaf = self.dock_state[surface_index][node_index]
                .get_leaf_mut()
                .expect("This node must be a leaf");
            tab_viewer
                .tab_bar_position_for_node(surface_index, node_index)
                .or_else(|| {
                    // Find the first tab that has a position override, instead of just checking tab[0]
                    // This prevents the tab bar position from changing when tabs are reordered
                    leaf.tabs
                        .iter()
                        .find_map(|tab| tab_viewer.tab_bar_position(tab))
                })
                .unwrap_or(default_position)
        };
        let layout = match position {
            TabBarPosition::Top => Layout::top_down_justified(Align::Min),
            TabBarPosition::Bottom => Layout::bottom_up(Align::Min),
            TabBarPosition::Left => Layout::left_to_right(Align::Min),
            TabBarPosition::Right => Layout::right_to_left(Align::Min),
        };
        let ui = &mut ui.new_child(
            UiBuilder::new()
                .max_rect(rect)
                .layout(layout)
                .id_salt((node_index, "node")),
        );
        let spacing = ui.spacing().item_spacing;
        ui.spacing_mut().item_spacing = Vec2::ZERO;
        ui.set_clip_rect(rect);

        if self.dock_state[surface_index][node_index].tabs_count() == 0 {
            return;
        }
        let active_index = self.dock_state[surface_index][node_index]
            .get_leaf()
            .map(|leaf| leaf.active)
            .unwrap_or(TabIndex(0));

        let tabbar_rect = self.tab_bar(
            ui,
            state,
            (surface_index, node_index),
            tab_viewer,
            fade_style.map(|(style, _)| style),
            collapsed,
            position,
            collapse_allowed,
            active_index,
        );
        self.tab_body(
            ui,
            state,
            (surface_index, node_index),
            tab_viewer,
            spacing,
            tabbar_rect,
            fade_style,
            collapsed,
            position,
        );

        let tabs = self.dock_state[surface_index][node_index]
            .tabs_mut()
            .expect("This node must be a leaf here");
        for (tab_index, tab) in tabs.iter_mut().enumerate() {
            if tab_viewer.force_close(tab) {
                self.to_remove.push(TabRemoval::Tab(
                    surface_index,
                    node_index,
                    TabIndex(tab_index),
                    ForcedRemoval(true),
                ));
            }
        }
    }

    fn tab_bar(
        &mut self,
        ui: &mut Ui,
        state: &mut State,
        (surface_index, node_index): (SurfaceIndex, NodeIndex),
        tab_viewer: &mut impl TabViewer<Tab = Tab>,
        fade_style: Option<&Style>,
        collapsed: bool,
        position: TabBarPosition,
        collapse_allowed: bool,
        active_index: TabIndex,
    ) -> Rect {
        assert!(self.dock_state[surface_index][node_index].is_leaf());

        let style = match fade_style {
            Some(s) => s.clone(),
            None => self.style.as_ref().unwrap().clone(),
        };
        let is_vertical = position.is_vertical();
        let bar_size = if is_vertical {
            vec2(style.tab_bar.height, ui.available_height())
        } else {
            vec2(ui.available_width(), style.tab_bar.height)
        };
        let (tabbar_outer_rect, tabbar_response) = ui.allocate_exact_size(bar_size, Sense::hover());
        let pad = Style::TAB_BAR_EDGE_PADDING;
        let pad_vec = match position {
            TabBarPosition::Top => vec2(pad, 0.0),
            TabBarPosition::Bottom => vec2(pad, pad),
            TabBarPosition::Left | TabBarPosition::Right => Vec2::ZERO,
        };
        let tabbar_outer_rect = Rect::from_min_max(
            tabbar_outer_rect.min + pad_vec,
            tabbar_outer_rect.max - pad_vec,
        );
        ui.painter().rect_filled(
            tabbar_outer_rect,
            style.tab_bar.corner_radius,
            style.tab_bar.bg_fill,
        );

        let tabbar_outer_rect = tabbar_outer_rect - style.tab_bar.inner_margin;
        let px = ui.ctx().pixels_per_point().recip();
        let button_padding = 2.0 * px;

        let bar_len = if is_vertical {
            tabbar_outer_rect.height()
        } else {
            tabbar_outer_rect.width()
        };
        let scroll_bar_width = bar_len;
        if bar_len == 0.0 {
            return tabbar_outer_rect;
        }

        let show_collapse_button = self.show_leaf_collapse_buttons && collapse_allowed;
        let collapse_space = if show_collapse_button {
            Style::TAB_COLLAPSE_BUTTON_SIZE + button_padding
        } else {
            0.0
        };
        let add_gap = if self.show_add_buttons { px } else { 0.0 };
        let add_size = if self.show_add_buttons {
            Style::TAB_ADD_BUTTON_SIZE
        } else {
            0.0
        };
        let fullscreen_extra = if self.dock_state[surface_index][node_index].fullscreen_toggle() {
            style.tab_bar.height
        } else {
            0.0
        };
        let tail_padding_min = if let Some(provider) = self.tab_bar_tail_padding.as_deref_mut() {
            provider(surface_index, node_index, active_index)
        } else {
            style.tab_bar.tail_padding
        } + fullscreen_extra;
        let fullscreen_button_primary =
            if self.dock_state[surface_index][node_index].fullscreen_toggle() {
                style.tab_bar.height
            } else {
                0.0
            };

        // Length dedicated to tabs + tail (collapse button already removed).
        let tab_tail_len = (bar_len - collapse_space).at_least(0.0);
        // Inside that area we place: tabs (scrollable) + optional gap/+ button + tail padding.
        let tabs_and_tail_len = (tab_tail_len - add_gap - add_size).at_least(0.0);

        let (tab_hovered, actual_primary, tail_padding, tail_start, available_primary) = {
            let leaf = self.dock_state[surface_index][node_index]
                .get_leaf_mut()
                .expect("This node must be a leaf");

            let collapse_offset = if show_collapse_button {
                if is_vertical {
                    vec2(0.0, Style::TAB_COLLAPSE_BUTTON_SIZE + button_padding)
                } else {
                    vec2(Style::TAB_COLLAPSE_BUTTON_SIZE + button_padding, 0.0)
                }
            } else {
                Vec2::ZERO
            };
            let scroll_offset = if is_vertical {
                vec2(0.0, -leaf.scroll)
            } else {
                vec2(-leaf.scroll, 0.0)
            };
            let tabs_origin = tabbar_outer_rect.min + collapse_offset + scroll_offset;
            let tabs_size = if is_vertical {
                vec2(tabbar_outer_rect.width(), tabs_and_tail_len)
            } else {
                vec2(tabs_and_tail_len, tabbar_outer_rect.height())
            };
            let tabbar_inner_rect = Rect::from_min_size(tabs_origin, tabs_size);

            let tabs_layout = if is_vertical {
                Layout::top_down(Align::Min)
            } else {
                Layout::left_to_right(Align::Center)
            };

            let tabs_ui = &mut ui.new_child(
                UiBuilder::new()
                    .max_rect(tabbar_inner_rect)
                    .layout(tabs_layout)
                    .id_salt("tabs"),
            );
            tabs_ui.spacing_mut().item_spacing = Vec2::ZERO;

            let mut clip_rect =
                Rect::from_min_size(tabbar_outer_rect.min + collapse_offset, tabs_size);
            tabs_ui.set_clip_rect(clip_rect);

            // Desired size for tabs in "expanded" mode.
            // fill_tab_bar and auto_tail are mutually exclusive:
            // - fill_tab_bar: tabs expand, tail_padding fixed
            // - auto_tail: tail_padding expands, tabs use actual width
            let prefered_width = if style.tab_bar.fill_tab_bar && !style.tab_bar.auto_tail {
                Some(tabs_and_tail_len / (leaf.tabs.len() as f32).at_least(1.0))
            } else {
                None
            };

            let (tab_hovered, actual_primary) = self.tabs(
                tabs_ui,
                state,
                (surface_index, node_index),
                tab_viewer,
                tabbar_outer_rect,
                prefered_width,
                fade_style,
                position,
                collapse_allowed,
            );

            let tail_padding = {
                // tabs_and_tail_len = tabs + gap/+ + tail
                let max_tabs_without_overflow =
                    (tabs_and_tail_len - tail_padding_min - fullscreen_button_primary)
                        .at_least(0.0);
                if style.tab_bar.auto_tail && actual_primary <= max_tabs_without_overflow {
                    (tabs_and_tail_len - actual_primary).at_least(tail_padding_min)
                } else {
                    tail_padding_min
                }
            };

            // Visible length for tabs after reserving tail padding.
            let space_without_tail = (tabs_and_tail_len - tail_padding).at_least(0.0);
            let visible_primary = if actual_primary <= space_without_tail {
                actual_primary
            } else {
                space_without_tail
            };

            if is_vertical {
                clip_rect.set_height(visible_primary);
            } else {
                clip_rect.set_width(visible_primary);
            }
            tabs_ui.set_clip_rect(clip_rect);

            let tail_start = match position {
                TabBarPosition::Top | TabBarPosition::Bottom => (tabbar_outer_rect.left()
                    + collapse_space
                    + visible_primary
                    + add_gap
                    + add_size)
                    .at_most(tabbar_outer_rect.right()),
                TabBarPosition::Left | TabBarPosition::Right => (tabbar_outer_rect.top()
                    + collapse_space
                    + visible_primary
                    + add_gap
                    + add_size)
                    .at_most(tabbar_outer_rect.bottom()),
            };

            (
                tab_hovered,
                actual_primary,
                tail_padding,
                tail_start,
                visible_primary,
            )
        };

        // Draw hline from tab end to start of tail padding.
        let style = match fade_style {
            Some(style) => style.clone(),
            None => self.style.as_ref().unwrap().clone(),
        };

        let tabs_start_primary = match position {
            TabBarPosition::Top | TabBarPosition::Bottom => {
                tabbar_outer_rect.left() + collapse_space
            }
            TabBarPosition::Left | TabBarPosition::Right => {
                tabbar_outer_rect.top() + collapse_space
            }
        };
        // Block pointer interactions over hidden tabs when overflow; leave add button/tail interactive.
        if actual_primary > available_primary {
            let block_start = tabs_start_primary + available_primary + add_gap + add_size;
            let block_rect = match position {
                TabBarPosition::Top | TabBarPosition::Bottom => Rect::from_min_max(
                    pos2(block_start, tabbar_outer_rect.top()),
                    pos2(
                        (tabs_start_primary + actual_primary).at_most(tabbar_outer_rect.right()),
                        tabbar_outer_rect.bottom(),
                    ),
                ),
                TabBarPosition::Left | TabBarPosition::Right => Rect::from_min_max(
                    pos2(tabbar_outer_rect.left(), block_start),
                    pos2(
                        tabbar_outer_rect.right(),
                        (tabs_start_primary + actual_primary).at_most(tabbar_outer_rect.bottom()),
                    ),
                ),
            };
            ui.allocate_rect(block_rect, Sense::hover());
        }
        let tabs_end_primary = (tabs_start_primary + actual_primary).at_most(tail_start);
        let line_start = tabs_end_primary.min(tail_start);
        let line_end = tail_start.max(line_start);

        match position {
            TabBarPosition::Top => {
                ui.painter().hline(
                    line_start..=line_end,
                    tabbar_outer_rect.bottom() - px,
                    (px, style.tab_bar.hline_color),
                );
            }
            TabBarPosition::Bottom => {
                ui.painter().hline(
                    line_start..=line_end,
                    tabbar_outer_rect.top() + px,
                    (px, style.tab_bar.hline_color),
                );
            }
            TabBarPosition::Left => {
                ui.painter().vline(
                    tabbar_outer_rect.right() - px,
                    line_start..=line_end,
                    (px, style.tab_bar.hline_color),
                );
            }
            TabBarPosition::Right => {
                ui.painter().vline(
                    tabbar_outer_rect.left() + px,
                    line_start..=line_end,
                    (px, style.tab_bar.hline_color),
                );
            }
        };

        // Add button placed just before tail padding (inside tab/tail area).
        if self.show_add_buttons {
            let plus_rect = match position {
                TabBarPosition::Top | TabBarPosition::Bottom => {
                    let plus_end = (tail_start - add_gap).at_least(tabbar_outer_rect.left());
                    let plus_start = (plus_end - add_size).at_least(tabbar_outer_rect.left());
                    Rect::from_min_size(
                        pos2(plus_start, tabbar_outer_rect.top()),
                        vec2(add_size, tabbar_outer_rect.height()),
                    )
                }
                TabBarPosition::Left | TabBarPosition::Right => {
                    let plus_end = (tail_start - add_gap).at_least(tabbar_outer_rect.top());
                    let plus_start = (plus_end - add_size).at_least(tabbar_outer_rect.top());
                    Rect::from_min_size(
                        pos2(tabbar_outer_rect.left(), plus_start),
                        vec2(tabbar_outer_rect.width(), add_size),
                    )
                }
            };
            self.tab_plus(
                ui,
                surface_index,
                node_index,
                tab_viewer,
                plus_rect,
                fade_style,
                position,
            );
        }

        if show_collapse_button {
            self.tab_collapse(
                ui,
                surface_index,
                node_index,
                tabbar_outer_rect,
                fade_style,
                collapsed,
                position,
                show_collapse_button,
            )
        }

        // Custom tail content (reserved by tail_padding).
        if tail_padding > 0.0 {
            let mut tail_rect = match position {
                TabBarPosition::Top | TabBarPosition::Bottom => Rect::from_min_size(
                    pos2(tail_start, tabbar_outer_rect.top()),
                    vec2(tail_padding, tabbar_outer_rect.height()),
                ),
                TabBarPosition::Left | TabBarPosition::Right => Rect::from_min_size(
                    pos2(tabbar_outer_rect.left(), tail_start),
                    vec2(tabbar_outer_rect.width(), tail_padding),
                ),
            };

            // Built-in fullscreen button at the far end of tail padding.
            if self.dock_state[surface_index][node_index].fullscreen_toggle() {
                let button_size = match position {
                    TabBarPosition::Top | TabBarPosition::Bottom => {
                        vec2(style.tab_bar.height, tail_rect.height())
                    }
                    TabBarPosition::Left | TabBarPosition::Right => {
                        vec2(tail_rect.width(), style.tab_bar.height)
                    }
                };
                let button_rect = match position {
                    TabBarPosition::Top | TabBarPosition::Bottom => Rect::from_min_size(
                        pos2(tail_rect.right() - button_size.x, tail_rect.top()),
                        button_size,
                    ),
                    TabBarPosition::Left | TabBarPosition::Right => Rect::from_min_size(
                        pos2(tail_rect.left(), tail_rect.bottom() - button_size.y),
                        button_size,
                    ),
                };
                // Paint a solid background to avoid seeing underlying tabs through the button.
                ui.painter()
                    .rect_filled(button_rect, CornerRadius::ZERO, style.tab_bar.bg_fill);
                let is_fullscreen = self.dock_state.fullscreen_active();
                let resp = ui.allocate_rect(button_rect, Sense::click());
                if resp.clicked() {
                    ui.ctx().data_mut(|d| {
                        d.insert_temp(
                            self.id.with("fullscreen_request"),
                            Some((surface_index, node_index, active_index)),
                        )
                    });
                }
                if resp.hovered() {
                    ui.output_mut(|o| o.cursor_icon = CursorIcon::PointingHand);
                }
                // Draw SVG-like icon with 4 polylines, scaled to rect.
                let painter = ui.painter();
                let to_screen = |x: f32, y: f32| {
                    let scale = button_rect.size().min_elem() / 256.0;
                    let offset = button_rect.center().to_vec2();
                    offset + vec2((x - 128.0) * scale, (y - 128.0) * scale)
                };
                let stroke = Stroke {
                    width: style.tab_bar.height * 0.08,
                    color: ui.visuals().widgets.inactive.fg_stroke.color,
                };
                let icon_polylines: &[&[(f32, f32)]] = if is_fullscreen {
                    &[
                        &[(208.0, 96.0), (160.0, 96.0), (160.0, 48.0)],
                        &[(48.0, 160.0), (96.0, 160.0), (96.0, 208.0)],
                        &[(160.0, 208.0), (160.0, 160.0), (208.0, 160.0)],
                        &[(96.0, 48.0), (96.0, 96.0), (48.0, 96.0)],
                    ]
                } else {
                    &[
                        &[(168.0, 48.0), (208.0, 48.0), (208.0, 88.0)],
                        &[(88.0, 208.0), (48.0, 208.0), (48.0, 168.0)],
                        &[(208.0, 168.0), (208.0, 208.0), (168.0, 208.0)],
                        &[(48.0, 88.0), (48.0, 48.0), (88.0, 48.0)],
                    ]
                };
                for poly in icon_polylines {
                    let points: Vec<Pos2> = poly
                        .iter()
                        .map(|(x, y)| to_screen(*x, *y).to_pos2())
                        .collect();
                    painter.line(points, stroke);
                }
                // Shrink tail rect for custom content to avoid overlap with fullscreen button.
                tail_rect = match position {
                    TabBarPosition::Top | TabBarPosition::Bottom => Rect::from_min_size(
                        tail_rect.min,
                        vec2(
                            (tail_rect.width() - button_size.x).at_least(0.0),
                            tail_rect.height(),
                        ),
                    ),
                    TabBarPosition::Left | TabBarPosition::Right => Rect::from_min_size(
                        tail_rect.min,
                        vec2(
                            tail_rect.width(),
                            (tail_rect.height() - button_size.y).at_least(0.0),
                        ),
                    ),
                };
            }

            if let Some(tail_cb) = self.tab_bar_tail_content.as_deref_mut() {
                let tail_ui = &mut ui.new_child(
                    UiBuilder::new()
                        .max_rect(tail_rect)
                        .layout(match position {
                            TabBarPosition::Top | TabBarPosition::Bottom => {
                                Layout::left_to_right(Align::Center)
                            }
                            TabBarPosition::Left | TabBarPosition::Right => {
                                Layout::top_down(Align::Center)
                            }
                        })
                        .id_salt((node_index, "tab_tail")),
                );
                // Paint background over tail to avoid tab overlap.
                ui.painter()
                    .rect_filled(tail_rect, CornerRadius::ZERO, style.tab_bar.bg_fill);
                tail_cb(tail_ui, surface_index, node_index, active_index);
            } else {
                // Still paint tail background if no custom content.
                ui.painter()
                    .rect_filled(tail_rect, CornerRadius::ZERO, style.tab_bar.bg_fill);
            }
        }

        let (scroll_ui_first, scroll_ui_last) = match position {
            TabBarPosition::Top | TabBarPosition::Left => (true, false),
            TabBarPosition::Bottom | TabBarPosition::Right => (false, true),
        };

        if scroll_ui_first {
            self.tab_bar_scroll(
                ui,
                state,
                (surface_index, node_index),
                actual_primary,
                available_primary,
                scroll_bar_width,
                &tabbar_response,
                tab_hovered,
                fade_style,
                position,
                tabbar_outer_rect,
            );
        }

        if scroll_ui_last {
            self.tab_bar_scroll(
                ui,
                state,
                (surface_index, node_index),
                actual_primary,
                available_primary,
                scroll_bar_width,
                &tabbar_response,
                tab_hovered,
                fade_style,
                position,
                tabbar_outer_rect,
            );
        }

        tabbar_outer_rect
    }

    #[allow(clippy::too_many_arguments)]
    fn tabs(
        &mut self,
        tabs_ui: &mut Ui,
        state: &mut State,
        (surface_index, node_index): (SurfaceIndex, NodeIndex),
        tab_viewer: &mut impl TabViewer<Tab = Tab>,
        _tabbar_outer_rect: Rect,
        preferred_width: Option<f32>,
        fade: Option<&Style>,
        position: TabBarPosition,
        _collapse_allowed: bool,
    ) -> (bool, f32) {
        let mut tab_hovered = false;
        let is_vertical = position.is_vertical();

        assert!(self.dock_state[surface_index][node_index].is_leaf());

        let focused = self.dock_state.focused_leaf();
        let tabs_len = {
            let tabs = self.dock_state[surface_index][node_index]
                .tabs()
                .expect("This node must be a leaf here");
            tabs.len()
        };
        let hline_color = fade
            .map(|s| s.tab_bar.hline_color)
            .unwrap_or_else(|| self.style.as_ref().unwrap().tab_bar.hline_color);
        let mut min_main: Option<f32> = None;
        let mut max_main: Option<f32> = None;
        let mut min_cross: Option<f32> = None;
        let mut max_cross: Option<f32> = None;
        let mut baseline_coord: Option<f32> = None;

        for tab_index in 0..tabs_len {
            let id = self
                .id
                .with((surface_index, "surface"))
                .with((node_index, "node"))
                .with((tab_index, "tab"));
            let tab_index = TabIndex(tab_index);

            // Check if this is the last tab in the node
            // Allow dragging single tab only if:
            // 1. It's in a window surface (not main), OR
            // 2. The node doesn't have always_keep set to true
            let is_last_tab = tabs_len == 1;
            let leaf_always_keep = self.dock_state[surface_index][node_index]
                .get_leaf()
                .map(|leaf| leaf.always_keep())
                .unwrap_or(false);
            let can_drag_last_tab = !surface_index.is_main() || !leaf_always_keep;

            let is_being_dragged = tabs_ui.ctx().is_being_dragged(id)
                && tabs_ui.input(|i| i.pointer.is_decidedly_dragging())
                && self.draggable_tabs
                && (!is_last_tab || can_drag_last_tab); // Allow dragging last tab in certain cases

            if is_being_dragged {
                tabs_ui.output_mut(|o| o.cursor_icon = CursorIcon::Grabbing);
            }

            let (is_active, label, tab_style, closeable) = {
                let leaf = self.dock_state[surface_index][node_index]
                    .get_leaf_mut()
                    .expect("This node must be a leaf");
                let style = fade.unwrap_or_else(|| self.style.as_ref().unwrap());
                let tab_style = tab_viewer.tab_style_override(&leaf.tabs[tab_index.0], &style.tab);
                (
                    leaf.active == tab_index || is_being_dragged,
                    tab_viewer.title(&mut leaf.tabs[tab_index.0]),
                    tab_style.unwrap_or(style.tab.clone()),
                    tab_viewer.is_closeable(&leaf.tabs[tab_index.0])
                        && (!is_last_tab || can_drag_last_tab), // Same logic for closing
                )
            };

            // 全屏状态下强制隐藏关闭按钮。
            let show_close_button =
                self.show_close_buttons && closeable && !self.dock_state.fullscreen_active();

            let (response, title_id) = if is_being_dragged {
                let layer_id = LayerId::new(Order::Tooltip, id);
                let response = tabs_ui
                    .scope_builder(UiBuilder::new().layer_id(layer_id), |ui| {
                        self.tab_title(
                            ui,
                            &tab_style,
                            id,
                            label,
                            is_active && Some((surface_index, node_index)) == focused,
                            is_active,
                            is_being_dragged,
                            preferred_width,
                            show_close_button,
                            position,
                            fade,
                            !is_last_tab || can_drag_last_tab, // draggable
                        )
                    })
                    .response;
                let title_id = response.id;

                let response =
                    tabs_ui.interact(response.rect, id.with("dragged"), Sense::click_and_drag());

                if let Some(pointer_pos) = tabs_ui.ctx().pointer_interact_pos() {
                    let start = *state.drag_start.get_or_insert(pointer_pos);
                    let delta = pointer_pos - start;
                    if delta.x.abs() > 30.0 || delta.y.abs() > 6.0 {
                        tabs_ui
                            .ctx()
                            .transform_layer_shapes(layer_id, TSTransform::new(delta, 1.0));

                        tabs_ui.memory_mut(|mem| {
                            mem.data.insert_temp(
                                self.id.with("drag_data"),
                                Some(DragData {
                                    src: TreeComponent::Tab(surface_index, node_index, tab_index),
                                    rect: self.dock_state[surface_index][node_index]
                                        .rect()
                                        .unwrap(),
                                }),
                            );
                        });
                    }
                }

                (response, title_id)
            } else {
                let (mut response, close_response) = self.tab_title(
                    tabs_ui,
                    &tab_style,
                    id,
                    label,
                    is_active && Some((surface_index, node_index)) == focused,
                    is_active,
                    is_being_dragged,
                    preferred_width,
                    show_close_button,
                    position,
                    fade,
                    !is_last_tab || can_drag_last_tab, // draggable
                );
                let title_id = response.id;
                let close_clicked = close_response.is_some_and(|res| res.clicked());
                let is_lonely_tab = self.dock_state[surface_index].num_tabs() == 1;

                if self.show_tab_name_on_hover {
                    let tabs = self.dock_state[surface_index][node_index]
                        .tabs_mut()
                        .expect("This node must be a leaf");
                    let tab = &mut tabs[tab_index.0];
                    response = response.on_hover_ui(|ui| {
                        ui.label(tab_viewer.title(tab));
                    });
                }

                if self.tab_context_menus {
                    let eject_button =
                        Button::new(&self.dock_state.translations.tab_context_menu.eject_button);
                    let move_to_main_button = Button::new(
                        &self
                            .dock_state
                            .translations
                            .tab_context_menu
                            .move_to_main_button,
                    );
                    let close_button =
                        Button::new(&self.dock_state.translations.tab_context_menu.close_button);

                    response.context_menu(|ui| {
                        let leaf = self.dock_state[surface_index][node_index]
                            .get_leaf_mut()
                            .expect("This node must be a leaf");
                        let tab = &mut leaf.tabs[tab_index.0];

                        tab_viewer.context_menu(ui, tab, surface_index, node_index);

                        // Show "Move to Main Window" button if in a window surface
                        if !surface_index.is_main() && ui.add(move_to_main_button).clicked() {
                            // Store the request to move tab back to main window
                            ui.ctx().data_mut(|d| {
                                d.insert_temp(
                                    self.id.with("move_to_main_request"),
                                    Some((surface_index, node_index, tab_index)),
                                );
                            });
                            ui.close();
                        }

                        // Show "Eject" button if in main window or not the only tab
                        if (surface_index.is_main() || !is_lonely_tab)
                            && tab_viewer.allowed_in_windows(tab)
                            && ui.add(eject_button).clicked()
                        {
                            self.to_detach.push((surface_index, node_index, tab_index));
                            ui.close();
                        }
                        if show_close_button && ui.add(close_button).clicked() {
                            match tab_viewer.on_close(tab) {
                                OnCloseResponse::Close => self.to_remove.push(TabRemoval::Tab(
                                    surface_index,
                                    node_index,
                                    tab_index,
                                    ForcedRemoval(false),
                                )),
                                OnCloseResponse::Focus => {
                                    leaf.active = tab_index;
                                    self.new_focused = Some((surface_index, node_index));
                                }
                                OnCloseResponse::Ignore => (),
                            }
                            ui.close();
                        }
                    });
                }

                if close_clicked {
                    self.to_remove.push(TabRemoval::Tab(
                        surface_index,
                        node_index,
                        tab_index,
                        ForcedRemoval(false),
                    ));
                }

                if let Some(pos) = state.last_hover_pos {
                    // Use response.rect.contains instead of
                    // response.hovered as the dragged tab covers
                    // the underlying tab
                    if state.drag_start.is_some() && response.rect.contains(pos) {
                        self.tab_hover_rect = Some((response.rect, tab_index));
                    }
                }

                (response, title_id)
            };

            if response.hovered() {
                tab_hovered = true;
            }

            let leaf = self.dock_state[surface_index][node_index]
                .get_leaf_mut()
                .unwrap();
            let tab = &mut leaf.tabs[tab_index.0];
            match position {
                TabBarPosition::Top | TabBarPosition::Bottom => {
                    min_main = Some(
                        min_main
                            .unwrap_or(response.rect.left())
                            .min(response.rect.left()),
                    );
                    max_main = Some(
                        max_main
                            .unwrap_or(response.rect.right())
                            .max(response.rect.right()),
                    );
                    min_cross = Some(
                        min_cross
                            .unwrap_or(response.rect.top())
                            .min(response.rect.top()),
                    );
                    max_cross = Some(
                        max_cross
                            .unwrap_or(response.rect.bottom())
                            .max(response.rect.bottom()),
                    );
                    baseline_coord.get_or_insert_with(|| {
                        if position == TabBarPosition::Top {
                            response.rect.bottom()
                        } else {
                            response.rect.top()
                        }
                    });
                }
                TabBarPosition::Left | TabBarPosition::Right => {
                    min_main = Some(
                        min_main
                            .unwrap_or(response.rect.top())
                            .min(response.rect.top()),
                    );
                    max_main = Some(
                        max_main
                            .unwrap_or(response.rect.bottom())
                            .max(response.rect.bottom()),
                    );
                    min_cross = Some(
                        min_cross
                            .unwrap_or(response.rect.left())
                            .min(response.rect.left()),
                    );
                    max_cross = Some(
                        max_cross
                            .unwrap_or(response.rect.right())
                            .max(response.rect.right()),
                    );
                    baseline_coord.get_or_insert_with(|| {
                        if position == TabBarPosition::Left {
                            response.rect.right()
                        } else {
                            response.rect.left()
                        }
                    });
                }
            }

            // Active tab indicator line (drawn inside the tab rect).
            if is_active {
                let border_px = 2.0 * tabs_ui.ctx().pixels_per_point().recip();
                let marker_px = 5.0 * tabs_ui.ctx().pixels_per_point().recip();
                let marker_offset = marker_px * 0.5;
                let border_color = hline_color;
                let marker_color = if focused.is_some_and(|f| f == (surface_index, node_index)) {
                    Color32::from_rgb(68, 114, 234)
                } else {
                    hline_color
                };
                let y0 = response.rect.top() + marker_px;
                let y1 = response.rect.bottom() - marker_px;
                let x0 = response.rect.left() + marker_px;
                let x1 = response.rect.right() - marker_px;
                let indicator_painter = tabs_ui
                    .ctx()
                    .layer_painter(LayerId::new(
                        Order::Foreground,
                        self.id
                            .with(("tab_indicator", surface_index, node_index, tab_index.0)),
                    ))
                    .with_clip_rect(tabs_ui.clip_rect());
                match position {
                    TabBarPosition::Top => {
                        indicator_painter.vline(
                            response.rect.left(),
                            y0..=y1,
                            (border_px, border_color),
                        );
                        indicator_painter.vline(
                            response.rect.right(),
                            y0..=y1,
                            (border_px, border_color),
                        );
                        indicator_painter.hline(
                            response.rect.x_range(),
                            response.rect.bottom() - marker_offset,
                            (marker_px, marker_color),
                        );
                    }
                    TabBarPosition::Bottom => {
                        indicator_painter.vline(
                            response.rect.left(),
                            y0..=y1,
                            (border_px, border_color),
                        );
                        indicator_painter.vline(
                            response.rect.right(),
                            y0..=y1,
                            (border_px, border_color),
                        );
                        indicator_painter.hline(
                            response.rect.x_range(),
                            response.rect.top() + marker_offset,
                            (marker_px, marker_color),
                        );
                    }
                    TabBarPosition::Left => {
                        indicator_painter.vline(
                            response.rect.right() - marker_offset * 3.0,
                            y0..=y1,
                            (marker_px, marker_color),
                        );
                        indicator_painter.hline(
                            x0..=x1,
                            response.rect.top(),
                            (border_px, border_color),
                        );
                        indicator_painter.hline(
                            x0..=x1,
                            response.rect.bottom(),
                            (border_px, border_color),
                        );
                    }
                    TabBarPosition::Right => {
                        indicator_painter.vline(
                            response.rect.left() + marker_offset,
                            y0..=y1,
                            (marker_px, marker_color),
                        );
                        indicator_painter.hline(
                            x0..=x1,
                            response.rect.top(),
                            (border_px, border_color),
                        );
                        indicator_painter.hline(
                            x0..=x1,
                            response.rect.bottom(),
                            (border_px, border_color),
                        );
                    }
                }
            }

            // Separators between tabs (always draw).
            if tab_index.0 + 1 != tabs_len {
                let px = tabs_ui.ctx().pixels_per_point().recip();
                let color = hline_color;
                match position {
                    TabBarPosition::Top | TabBarPosition::Bottom => {
                        tabs_ui.painter().vline(
                            response.rect.right(),
                            response.rect.y_range(),
                            (px, color),
                        );
                    }
                    TabBarPosition::Left | TabBarPosition::Right => {
                        tabs_ui.painter().hline(
                            response.rect.x_range(),
                            response.rect.bottom(),
                            (px, color),
                        );
                    }
                }
            }

            if response.clicked()
                || (tabs_ui.memory(|m| m.has_focus(title_id))
                    && tabs_ui.input(|i| i.key_pressed(Key::Enter) || i.key_pressed(Key::Space)))
            {
                leaf.active = tab_index;
                self.new_focused = Some((surface_index, node_index));
            }

            tab_viewer.on_tab_button(tab, &response);

            if self.show_close_buttons && tab_viewer.is_closeable(tab) && response.middle_clicked()
            {
                self.to_remove.push(TabRemoval::Tab(
                    surface_index,
                    node_index,
                    tab_index,
                    ForcedRemoval(false),
                ));
            }
        }

        // Baseline across all tabs.
        if let (Some(min_main), Some(max_main), Some(_min_cross), Some(_max_cross), Some(coord)) =
            (min_main, max_main, min_cross, max_cross, baseline_coord)
        {
            let px = 2.0 * tabs_ui.ctx().pixels_per_point().recip();
            let color = hline_color;
            match position {
                TabBarPosition::Top => {
                    tabs_ui
                        .painter()
                        .hline(min_main..=max_main, coord - px, (px, color));
                }
                TabBarPosition::Bottom => {
                    tabs_ui
                        .painter()
                        .hline(min_main..=max_main, coord + px, (px, color));
                }
                TabBarPosition::Left => {
                    let offset = 3.0 * px;
                    tabs_ui
                        .painter()
                        .vline(coord - offset, min_main..=max_main, (px, color));
                }
                TabBarPosition::Right => {
                    tabs_ui
                        .painter()
                        .vline(coord + px, min_main..=max_main, (px, color));
                }
            }
        }

        let actual_primary = match (min_main, max_main) {
            (Some(min_m), Some(max_m)) => (max_m - min_m).at_least(0.0),
            _ => {
                if is_vertical {
                    tabs_ui.min_rect().height()
                } else {
                    tabs_ui.min_rect().width()
                }
            }
        };

        (tab_hovered, actual_primary)
    }

    /// Draws the tab add button.
    #[allow(clippy::too_many_arguments)]
    fn tab_plus(
        &mut self,
        ui: &mut Ui,
        surface_index: SurfaceIndex,
        node_index: NodeIndex,
        tab_viewer: &mut impl TabViewer<Tab = Tab>,
        rect: Rect,
        fade_style: Option<&Style>,
        position: TabBarPosition,
    ) {
        let ui = &mut ui.new_child(
            UiBuilder::new()
                .max_rect(rect)
                .layout(Layout::left_to_right(Align::Center))
                .id_salt((node_index, "tab_add")),
        );

        let (rect, mut response) = ui.allocate_exact_size(ui.available_size(), Sense::click());

        response = response.on_hover_cursor(CursorIcon::PointingHand);

        let style = fade_style.unwrap_or_else(|| self.style.as_ref().unwrap());
        let hovered = response.hovered() || response.has_focus();
        // 默认与 tail 背景一致，hover 时使用按钮高亮色。
        let fill = if hovered {
            style.buttons.add_tab_bg_fill
        } else {
            style.tab_bar.bg_fill
        };
        ui.painter().rect_filled(rect, CornerRadius::ZERO, fill);
        let color = if hovered {
            style.buttons.add_tab_active_color
        } else {
            style.buttons.add_tab_color
        };

        let mut plus_rect = rect;

        rect_set_size_centered(&mut plus_rect, Vec2::splat(Style::TAB_ADD_PLUS_SIZE));

        ui.painter().line_segment(
            [plus_rect.center_top(), plus_rect.center_bottom()],
            Stroke::new(1.0, color),
        );
        ui.painter().line_segment(
            [plus_rect.right_center(), plus_rect.left_center()],
            Stroke::new(1.0, color),
        );

        // Draw button left border.
        let stroke = Stroke::new(
            ui.ctx().pixels_per_point().recip(),
            style.buttons.add_tab_border_color,
        );
        if position.is_vertical() {
            ui.painter()
                .hline(rect.x_range(), rect.top(), stroke.clone());
        } else {
            ui.painter().vline(rect.left(), rect.y_range(), stroke);
        }

        let popup_id = ui.id().with("tab_add_popup");
        if self.show_add_popup {
            Popup::from_toggle_button_response(&response)
                .id(popup_id)
                .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
                .show(|ui| {
                    tab_viewer.add_popup(ui, surface_index, node_index);
                });
        }

        if response.clicked() {
            tab_viewer.on_add(surface_index, node_index);
        }
    }

    /// Draws the close all button.
    /// Draws the collapse button.
    fn tab_collapse(
        &mut self,
        ui: &mut Ui,
        surface_index: SurfaceIndex,
        node_index: NodeIndex,
        tabbar_outer_rect: Rect,
        fade_style: Option<&Style>,
        collapsed: bool,
        position: TabBarPosition,
        _show_collapse_button: bool,
    ) {
        let rect = match position {
            TabBarPosition::Top | TabBarPosition::Bottom => Rect::from_min_size(
                tabbar_outer_rect.left_top(),
                vec2(Style::TAB_COLLAPSE_BUTTON_SIZE, tabbar_outer_rect.height()),
            ),
            TabBarPosition::Left | TabBarPosition::Right => Rect::from_min_size(
                tabbar_outer_rect.left_top(),
                vec2(tabbar_outer_rect.width(), Style::TAB_COLLAPSE_BUTTON_SIZE),
            ),
        };

        let ui = &mut ui.new_child(
            UiBuilder::new()
                .max_rect(rect)
                .layout(Layout::left_to_right(Align::Center))
                .id_salt((node_index, "tab_collapse")),
        );

        let (rect, mut response) = ui.allocate_exact_size(ui.available_size(), Sense::click());

        response = response.on_hover_cursor(CursorIcon::PointingHand);

        let style = fade_style.unwrap_or_else(|| self.style.as_ref().unwrap());

        // Whether we're on "secondary button mode" due to modifier keys
        let on_secondary_button = self.is_on_secondary_button(surface_index, ui, &response);

        let color = if response.hovered() || response.has_focus() {
            ui.painter().rect_filled(
                rect,
                CornerRadius::ZERO,
                style.buttons.collapse_tabs_bg_fill,
            );
            style.buttons.collapse_tabs_active_color
        } else {
            style.buttons.collapse_tabs_color
        };

        let mut arrow_rect = rect;
        rect_set_size_centered(&mut arrow_rect, Vec2::splat(Style::TAB_COLLAPSE_ARROW_SIZE));

        if on_secondary_button {
            // Collapse the entire window
            Self::draw_chevron_down(ui, style, color, arrow_rect);
        } else {
            // Draw arrow.
            Self::draw_arrow(collapsed, ui, color, arrow_rect);
        }

        // Draw button right border.
        let stroke = Stroke::new(
            ui.ctx().pixels_per_point().recip(),
            style.buttons.collapse_tabs_border_color,
        );
        if position.is_vertical() {
            ui.painter()
                .hline(rect.x_range(), rect.bottom(), stroke.clone());
        } else {
            ui.painter().vline(rect.right(), rect.y_range(), stroke);
        }

        if response.clicked() {
            if on_secondary_button {
                self.window_toggle_minimized(surface_index);
            } else {
                self.dock_state[surface_index][node_index].set_collapsed(!collapsed);
                self.dock_state[surface_index].node_update_collapsed(node_index);
                self.window_update_collapsed(surface_index, node_index);
            }
        }

        if !surface_index.is_main() && self.secondary_button_context_menu {
            response.context_menu(|ui| {
                if ui
                    .button(&self.dock_state.translations.leaf.minimize_button)
                    .clicked()
                {
                    ui.close();
                    self.window_toggle_minimized(surface_index);
                }
            });
        }

        if !on_secondary_button {
            self.show_tooltip_hints(surface_index, response);
        }
    }

    fn show_tooltip_hints(&mut self, surface_index: SurfaceIndex, response: Response) -> Response {
        if !surface_index.is_main()
            && self.show_secondary_button_hint
            && (self.secondary_button_context_menu || self.secondary_button_on_modifier)
        {
            let hint = if self.secondary_button_context_menu && self.secondary_button_on_modifier {
                &self
                    .dock_state
                    .translations
                    .leaf
                    .minimize_button_modifier_menu_hint
            } else if self.secondary_button_context_menu {
                &self.dock_state.translations.leaf.minimize_button_menu_hint
            } else {
                &self
                    .dock_state
                    .translations
                    .leaf
                    .minimize_button_modifier_hint
            };
            return response.on_hover_text(hint);
        }
        response
    }

    fn is_on_secondary_button(
        &self,
        surface_index: SurfaceIndex,
        ui: &mut Ui,
        response: &Response,
    ) -> bool {
        !surface_index.is_main()
            && self.secondary_button_on_modifier
            && ui.input(|i| {
                i.modifiers
                    .matches_logically(self.secondary_button_modifiers)
            })
            && (response.hovered() || response.has_focus() || response.is_pointer_button_down_on())
    }

    fn draw_arrow(collapsed: bool, ui: &mut Ui, color: Color32, arrow_rect: Rect) {
        ui.painter().add(Shape::convex_polygon(
            if collapsed {
                // Arrow pointing rightwards.
                vec![
                    arrow_rect.left_top(),
                    arrow_rect.right_center(),
                    arrow_rect.left_bottom(),
                ]
            } else {
                // Arrow pointing downwards.
                vec![
                    arrow_rect.left_top(),
                    arrow_rect.right_top(),
                    arrow_rect.center_bottom(),
                ]
            },
            color,
            Stroke::NONE,
        ));
    }

    fn draw_chevron_down(ui: &mut Ui, style: &Style, color: Color32, arrow_rect: Rect) {
        ui.painter().add(Shape::convex_polygon(
            // Arrow pointing downwards.
            vec![
                arrow_rect.left_top(),
                arrow_rect.right_top(),
                arrow_rect.center(),
            ],
            color,
            Stroke::NONE,
        ));

        // Chevron pointing downwards.
        ui.painter().add(Shape::convex_polygon(
            vec![
                arrow_rect.left_center(),
                arrow_rect.right_center(),
                arrow_rect.center_bottom(),
            ],
            color,
            Stroke::NONE,
        ));
        let color = style.buttons.minimize_window_bg_fill;
        ui.painter().add(Shape::convex_polygon(
            vec![
                arrow_rect
                    .left_center()
                    .lerp(arrow_rect.right_center(), 0.25),
                arrow_rect
                    .left_center()
                    .lerp(arrow_rect.right_center(), 0.75),
                arrow_rect.center().lerp(arrow_rect.center_bottom(), 0.5),
            ],
            color,
            Stroke::NONE,
        ));
    }

    /// Updates the collapsed state of the node and its parents.
    fn window_update_collapsed(&mut self, surface_index: SurfaceIndex, node_index: NodeIndex) {
        let surface = &mut self.dock_state[surface_index];
        let collapsed = surface[node_index].is_collapsed();
        if !collapsed {
            if let Some(window_state) = self.dock_state.get_window_state_mut(surface_index) {
                window_state.set_new(true);
            }
        } else if surface.root_node().is_some_and(|root| root.is_collapsed()) {
            let root_index = NodeIndex::root();
            let surface_height = if surface.root_node().is_some() {
                surface[root_index].rect().unwrap().height()
            } else {
                0.0
            };
            if let Some(window_state) = self.dock_state.get_window_state_mut(surface_index) {
                window_state.set_expanded_height(surface_height);
            }
        }
    }

    /// * `active` means "the tab that is opened in the parent panel".
    /// * `focused` means "the tab that was last interacted with".
    ///
    /// Returns the main button response plus the response of the close button, if any.
    #[allow(clippy::too_many_arguments)]
    fn tab_title(
        &mut self,
        ui: &mut Ui,
        tab_style: &TabStyle,
        id: Id,
        label: WidgetText,
        focused: bool,
        active: bool,
        is_being_dragged: bool,
        preferred_width: Option<f32>,
        show_close_button: bool,
        position: TabBarPosition,
        fade: Option<&Style>,
        draggable: bool, // Whether this tab can be dragged
    ) -> (Response, Option<Response>) {
        let style = fade.unwrap_or_else(|| self.style.as_ref().unwrap());
        let raw_galley = label
            .clone()
            .into_galley(ui, None, f32::INFINITY, TextStyle::Button);
        let x_spacing = 8.0;
        let y_spacing = 6.0;
        let text_width = raw_galley.size().x + 2.0 * x_spacing;
        let text_height = raw_galley.size().y + 2.0 * y_spacing;
        let tab_thickness = if position.is_vertical() {
            style.tab_bar.height.max(text_height)
        } else {
            style.tab_bar.height
        };
        let close_button_size = if show_close_button {
            Style::TAB_CLOSE_BUTTON_SIZE.min(tab_thickness)
        } else {
            0.0
        };

        // Compute total width of the tab bar.
        let text_primary = text_width;
        let minimum_width = tab_style
            .minimum_width
            .unwrap_or(0.0)
            .at_least(text_primary + close_button_size);
        let tab_primary = preferred_width.unwrap_or(0.0).at_least(minimum_width);

        let tab_size = if position.is_vertical() {
            vec2(tab_thickness, tab_primary)
        } else {
            vec2(tab_primary, tab_thickness)
        };
        let max_text_extent = if position.is_vertical() {
            (tab_size.y - close_button_size - 2.0 * y_spacing).at_least(0.0)
        } else {
            (tab_size.x - close_button_size - 2.0 * x_spacing).at_least(0.0)
        };
        let galley = label.into_galley(ui, None, max_text_extent, TextStyle::Button);
        let (_, tab_rect) = ui.allocate_space(tab_size);
        let mut response = ui.interact(tab_rect, id, Sense::click_and_drag());
        if ui.ctx().dragged_id().is_none() && self.draggable_tabs && draggable {
            response = response.on_hover_cursor(CursorIcon::Grab);
        }

        let tab_style = if focused || is_being_dragged {
            if response.has_focus() {
                &tab_style.focused_with_kb_focus
            } else {
                &tab_style.focused
            }
        } else if active {
            if response.has_focus() {
                &tab_style.active_with_kb_focus
            } else {
                &tab_style.active
            }
        } else if response.hovered() {
            &tab_style.hovered
        } else if response.has_focus() {
            &tab_style.inactive_with_kb_focus
        } else {
            &tab_style.inactive
        };

        // Fill without drawing an outline; rely on separators between tabs instead of per-tab borders.
        ui.painter()
            .rect_filled(tab_rect, tab_style.corner_radius, tab_style.bg_fill);

        let mut text_rect = tab_rect;
        if position.is_vertical() {
            text_rect.set_height(text_rect.height() - close_button_size);
            let pos_center = text_rect.shrink2(vec2(y_spacing, y_spacing)).center();
            let pos = pos_center - galley.rect.center().to_vec2();
            let angle = match position {
                TabBarPosition::Left => -FRAC_PI_2,
                TabBarPosition::Right => FRAC_PI_2,
                _ => 0.0,
            };
            let text_shape = TextShape::new(pos, galley.clone(), tab_style.text_color)
                .with_override_text_color(tab_style.text_color)
                .with_angle_and_anchor(angle, Align2::CENTER_CENTER);
            ui.painter().add(text_shape);
        } else {
            text_rect.set_width(text_rect.width() - close_button_size);
            let text_pos = {
                let pos = Align2::CENTER_CENTER
                    .pos_in_rect(&text_rect.shrink2(vec2(x_spacing, y_spacing)));
                pos - galley.size() / 2.0
            };

            ui.painter()
                .add(TextShape::new(text_pos, galley, tab_style.text_color));
        }

        let close_response = show_close_button.then(|| {
            let mut close_button_rect = tab_rect;
            if position.is_vertical() {
                close_button_rect.set_top(text_rect.bottom() - Style::TAB_CLOSE_BUTTON_OFFSET);
            } else {
                close_button_rect.set_left(text_rect.right() - Style::TAB_CLOSE_BUTTON_OFFSET);
            }
            close_button_rect =
                Rect::from_center_size(close_button_rect.center(), Vec2::splat(close_button_size));

            let close_response = ui
                .interact(close_button_rect, id.with("close-button"), Sense::click())
                .on_hover_cursor(CursorIcon::PointingHand);

            let show_icon =
                close_response.hovered() || close_response.has_focus() || response.hovered();
            if show_icon {
                let color = if close_response.hovered() || close_response.has_focus() {
                    style.buttons.close_tab_active_color
                } else {
                    style.buttons.close_tab_color
                };

                if close_response.hovered() || close_response.has_focus() {
                    let mut corner_radius = tab_style.corner_radius;
                    corner_radius.nw = 0;
                    corner_radius.sw = 0;

                    ui.painter().rect_filled(
                        close_button_rect,
                        corner_radius,
                        style.buttons.close_tab_bg_fill,
                    );
                }

                let mut x_rect = close_button_rect;
                rect_set_size_centered(&mut x_rect, Vec2::splat(Style::TAB_CLOSE_X_SIZE));
                ui.painter().line_segment(
                    [x_rect.left_top(), x_rect.right_bottom()],
                    Stroke::new(1.0, color),
                );
                ui.painter().line_segment(
                    [x_rect.right_top(), x_rect.left_bottom()],
                    Stroke::new(1.0, color),
                );
            }

            close_response
        });

        (response, close_response)
    }

    #[allow(clippy::too_many_arguments)]
    fn tab_bar_scroll(
        &mut self,
        ui: &mut Ui,
        state: &State,
        (surface_index, node_index): (SurfaceIndex, NodeIndex),
        actual_width: f32,
        available_width: f32,
        scroll_bar_width: f32,
        tabbar_response: &Response,
        tab_hovered: bool,
        fade_style: Option<&Style>,
        position: TabBarPosition,
        tabbar_rect: Rect,
    ) {
        assert_ne!(available_width, 0.0);

        let leaf = self.dock_state[surface_index][node_index]
            .get_leaf_mut()
            .expect("This node must be a leaf");
        let overflow = (actual_width - available_width).at_least(0.0);
        let style = fade_style.unwrap_or_else(|| self.style.as_ref().unwrap());

        // Compare to 1.0 and not 0.0 to avoid drawing a scroll bar due
        // to floating point precision issue during tab drawing.
        if overflow > 1.0 {
            let show_scrollbar = tabbar_response.hovered() || tab_hovered;
            if style.tab_bar.show_scroll_bar_on_overflow {
                // Draw scroll bar
                let bar_height = 3.0;
                let scroll_bar_rect = match position {
                    TabBarPosition::Top => Rect::from_min_size(
                        tabbar_rect.left_top(),
                        vec2(scroll_bar_width, bar_height),
                    ),
                    TabBarPosition::Bottom => Rect::from_min_size(
                        tabbar_rect.left_bottom() - vec2(0.0, bar_height),
                        vec2(scroll_bar_width, bar_height),
                    ),
                    TabBarPosition::Left => Rect::from_min_size(
                        tabbar_rect.left_top(),
                        vec2(bar_height, scroll_bar_width),
                    ),
                    TabBarPosition::Right => Rect::from_min_size(
                        tabbar_rect.right_top() - vec2(bar_height, 0.0),
                        vec2(bar_height, scroll_bar_width),
                    ),
                };

                // Compute scroll bar handle position and size.
                let overflow_ratio = actual_width / available_width;
                let scroll_ratio = leaf.scroll / overflow;

                let scroll_bar_handle_size = if position.is_vertical() {
                    overflow_ratio.recip() * scroll_bar_rect.height()
                } else {
                    overflow_ratio.recip() * scroll_bar_rect.width()
                };
                let scroll_bar_handle_start = if position.is_vertical() {
                    lerp(
                        scroll_bar_rect.top()..=scroll_bar_rect.bottom() - scroll_bar_handle_size,
                        scroll_ratio,
                    )
                } else {
                    lerp(
                        scroll_bar_rect.left()..=scroll_bar_rect.right() - scroll_bar_handle_size,
                        scroll_ratio,
                    )
                };
                let scroll_bar_handle_rect = if position.is_vertical() {
                    Rect::from_min_size(
                        pos2(scroll_bar_rect.min.x, scroll_bar_handle_start),
                        vec2(bar_height, scroll_bar_handle_size),
                    )
                } else {
                    Rect::from_min_size(
                        pos2(scroll_bar_handle_start, scroll_bar_rect.min.y),
                        vec2(scroll_bar_handle_size, bar_height),
                    )
                };

                let scroll_bar_handle_response = ui.interact(
                    scroll_bar_handle_rect,
                    self.id.with((node_index, "node")),
                    Sense::drag(),
                );

                let handle_range = if position.is_vertical() {
                    scroll_bar_rect.height() - scroll_bar_handle_size
                } else {
                    scroll_bar_rect.width() - scroll_bar_handle_size
                };
                let points_to_scroll_coefficient = if handle_range > 0.0 {
                    overflow / handle_range
                } else {
                    0.0
                };

                if scroll_bar_handle_response.dragged() && handle_range > 0.0 {
                    let pointer = scroll_bar_handle_response.interact_pointer_pos().unwrap();
                    let offset = if position.is_vertical() {
                        (pointer.y - scroll_bar_rect.top()) - scroll_bar_handle_size * 0.5
                    } else {
                        (pointer.x - scroll_bar_rect.left()) - scroll_bar_handle_size * 0.5
                    };
                    let t = (offset / handle_range).clamp(0.0, 1.0);
                    leaf.scroll = lerp(0.0..=overflow, t);
                }

                if let Some(pos) = state.last_hover_pos {
                    if scroll_bar_rect.contains(pos) {
                        let scroll_delta = ui.input(|i| {
                            if position.is_vertical() {
                                i.smooth_scroll_delta.y
                            } else {
                                i.smooth_scroll_delta.y + i.smooth_scroll_delta.x
                            }
                        });
                        leaf.scroll -= scroll_delta * points_to_scroll_coefficient;
                    }
                }

                // Draw the bar.
                if show_scrollbar {
                    ui.painter()
                        .rect_filled(scroll_bar_rect, 0.0, ui.visuals().extreme_bg_color);

                    ui.painter().rect_filled(
                        scroll_bar_handle_rect,
                        bar_height / 2.0,
                        ui.visuals()
                            .widgets
                            .style(&scroll_bar_handle_response)
                            .bg_fill,
                    );
                }
            }

            // Handle user input.
            if tabbar_response.hovered() || tab_hovered {
                let scroll_delta = ui.input(|i| {
                    if position.is_vertical() {
                        i.smooth_scroll_delta.y
                    } else {
                        i.smooth_scroll_delta.y + i.smooth_scroll_delta.x
                    }
                });
                leaf.scroll -= scroll_delta;
            }
        }

        leaf.scroll = leaf.scroll.clamp(0.0, overflow);
    }

    #[allow(clippy::too_many_arguments)]
    fn tab_body(
        &mut self,
        ui: &mut Ui,
        state: &State,
        (surface_index, node_index): (SurfaceIndex, NodeIndex),
        tab_viewer: &mut impl TabViewer<Tab = Tab>,
        spacing: Vec2,
        tabbar_rect: Rect,
        fade: Option<(&Style, f32)>,
        collapsed: bool,
        _position: TabBarPosition,
    ) {
        let (body_rect, _body_response) =
            ui.allocate_exact_size(ui.available_size_before_wrap(), Sense::hover());

        let leaf = self.dock_state[surface_index][node_index]
            .get_leaf_mut()
            .expect("This node must be a leaf");
        let LeafNode {
            rect,
            viewport,
            tabs,
            active,
            ..
        } = leaf;
        if !collapsed {
            if let Some(tab) = tabs.get_mut(active.0) {
                if *viewport != body_rect {
                    *viewport = body_rect;
                    tab_viewer.on_rect_changed(tab);
                }

                if ui.input(|i| i.pointer.any_click()) {
                    if let Some(pos) = state.last_hover_pos {
                        if body_rect.contains(pos)
                            && Some(ui.layer_id()) == ui.ctx().layer_id_at(pos)
                        {
                            self.new_focused = Some((surface_index, node_index));
                        }
                    }
                }

                let (style, fade_factor) =
                    fade.unwrap_or_else(|| (self.style.as_ref().unwrap(), 1.0));
                let tabs_styles = tab_viewer.tab_style_override(tab, &style.tab);

                let tabs_style = tabs_styles.as_ref().unwrap_or(&style.tab);

                if tab_viewer.clear_background(tab) {
                    ui.painter().rect_filled(
                        body_rect,
                        tabs_style.tab_body.corner_radius,
                        tabs_style.tab_body.bg_fill,
                    );
                }

                // Construct a new ui with the correct tab id.
                //
                // We are forced to use `Ui::new` because other methods (eg: push_id) always mix
                // the provided id with their own which would cause tabs to change id when moved
                // from node to node.
                let id = self.id.with(tab_viewer.id(tab));
                ui.ctx().check_for_id_clash(id, body_rect, "a tab with id");
                let ui = &mut Ui::new(
                    ui.ctx().clone(),
                    id,
                    UiBuilder::new().max_rect(body_rect).layer_id(ui.layer_id()),
                );
                ui.set_clip_rect(Rect::from_min_max(ui.cursor().min, ui.clip_rect().max));

                // Use initial spacing for ui.
                ui.spacing_mut().item_spacing = spacing;

                // Offset the background rectangle up to hide the top border behind the clip rect.
                // To avoid anti-aliasing lines when the stroke width is not divisible by two, we
                // need to calculate the effective anti-aliased stroke width.
                let effective_stroke_width = (tabs_style.tab_body.stroke.width / 2.0).ceil() * 2.0;
                let tab_body_rect = ui
                    .clip_rect()
                    .expand2(vec2(effective_stroke_width, effective_stroke_width));
                ui.painter().rect_stroke(
                    rect_stroke_box(tab_body_rect, tabs_style.tab_body.stroke.width),
                    tabs_style.tab_body.corner_radius,
                    tabs_style.tab_body.stroke,
                    StrokeKind::Inside,
                );

                ScrollArea::new(tab_viewer.scroll_bars(tab)).show(ui, |ui| {
                    Frame::new()
                        .inner_margin(tabs_style.tab_body.inner_margin)
                        .show(ui, |ui| {
                            if fade_factor != 1.0 {
                                fade_visuals(ui.visuals_mut(), fade_factor);
                            }
                            let available_rect = ui.available_rect_before_wrap();
                            ui.expand_to_include_rect(available_rect);
                            tab_viewer.ui(ui, tab);
                        });
                });
            }
        }

        // change hover destination
        if let Some(pointer) = state.last_hover_pos {
            // Prevent borrow checker issues.
            let rect = rect.to_owned();

            // if the dragged tab isn't allowed in a window,
            // it's unnecessary to change the hover state
            let is_dragged_valid = match &state.dnd {
                Some(DragDropState {
                    drag: DragData { src, .. },
                    ..
                }) => match *src {
                    TreeComponent::Tab(d_surf, d_node, d_tab) => {
                        if let Node::Leaf(leaf) = &mut self.dock_state[d_surf][d_node] {
                            tab_viewer.allowed_in_windows(&mut leaf.tabs[d_tab.0])
                                || surface_index == SurfaceIndex::main()
                        } else {
                            true
                        }
                    }
                    _ => unreachable!("collections of nodes can't be dragged (yet)"),
                },
                _ => true,
            };

            // Use rect.contains instead of response.hovered as the dragged tab covers
            // the underlying responses.
            if state.drag_start.is_some() && rect.contains(pointer) && is_dragged_valid {
                let on_title_bar = tabbar_rect.contains(pointer);
                let (dst, tab) = {
                    match self.tab_hover_rect {
                        Some((rect, tab_index)) => (
                            TreeComponent::Tab(surface_index, node_index, tab_index),
                            Some(rect),
                        ),
                        None => (
                            TreeComponent::Node(surface_index, node_index),
                            on_title_bar.then_some(tabbar_rect),
                        ),
                    }
                };

                ui.memory_mut(|mem| {
                    mem.data.insert_temp(
                        self.id.with("hover_data"),
                        Some(HoverData { rect, dst, tab }),
                    );
                });
            }
        }
    }
}
