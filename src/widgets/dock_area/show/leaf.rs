use egui::{
    emath::TSTransform, epaint::TextShape, lerp, pos2, vec2, Align, Align2, Button, Color32,
    CornerRadius, CursorIcon, Frame, Id, Key, LayerId, Layout, NumExt, Order, Popup,
    PopupCloseBehavior, Rect, Response, ScrollArea, Sense, Shape, Stroke, StrokeKind, TextStyle,
    Ui, UiBuilder, Vec2, WidgetText,
};
use std::{f32::consts::FRAC_PI_2, ops::RangeInclusive};

use crate::dock_area::tab_removal::{ForcedRemoval, TabRemoval};
use crate::node::LeafNode;
use crate::{
    dock_area::{
        drag_and_drop::{DragData, DragDropState, HoverData, TreeComponent},
        state::State,
    },
    utils::{fade_visuals, rect_set_size_centered, rect_stroke_box},
    DockArea, Node, NodeIndex, Style, SurfaceIndex, TabAddAlign, TabBarPosition, TabIndex,
    TabStyle, TabViewer,
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
                    leaf.tabs
                        .get(0)
                        .and_then(|tab| tab_viewer.tab_bar_position(tab))
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
        let tabbar_rect = self.tab_bar(
            ui,
            state,
            (surface_index, node_index),
            tab_viewer,
            fade_style.map(|(style, _)| style),
            collapsed,
            position,
            collapse_allowed,
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
    ) -> Rect {
        assert!(self.dock_state[surface_index][node_index].is_leaf());

        let style = fade_style.unwrap_or_else(|| self.style.as_ref().unwrap());
        let is_vertical = position.is_vertical();
        let bar_size = if is_vertical {
            vec2(style.tab_bar.height, ui.available_height())
        } else {
            vec2(ui.available_width(), style.tab_bar.height)
        };
        let (tabbar_outer_rect, tabbar_response) = ui.allocate_exact_size(bar_size, Sense::hover());
        ui.painter().rect_filled(
            tabbar_outer_rect,
            style.tab_bar.corner_radius,
            style.tab_bar.bg_fill,
        );

        let tabbar_outer_rect = tabbar_outer_rect - style.tab_bar.inner_margin;

        let mut available_primary = if is_vertical {
            tabbar_outer_rect.height()
        } else {
            tabbar_outer_rect.width()
        };
        let scroll_bar_width = available_primary;
        if available_primary == 0.0 {
            return tabbar_outer_rect;
        }

        // Reserve space for the buttons at the ends of the tab bar.

        if self.show_add_buttons {
            available_primary -= Style::TAB_ADD_BUTTON_SIZE;
        }

        if self.show_leaf_close_all_buttons {
            available_primary -= Style::TAB_CLOSE_ALL_BUTTON_SIZE;
        }

        let show_collapse_button = self.show_leaf_collapse_buttons && collapse_allowed;

        if show_collapse_button {
            available_primary -= Style::TAB_COLLAPSE_BUTTON_SIZE;
        }

        let (actual_primary, tab_hovered) = {
            let leaf = self.dock_state[surface_index][node_index]
                .get_leaf_mut()
                .expect("This node must be a leaf");

            let collapse_offset = if show_collapse_button {
                if is_vertical {
                    vec2(0.0, Style::TAB_COLLAPSE_BUTTON_SIZE)
                } else {
                    vec2(Style::TAB_COLLAPSE_BUTTON_SIZE, 0.0)
                }
            } else {
                Vec2::ZERO
            };
            let scroll_offset = if is_vertical {
                vec2(0.0, -leaf.scroll)
            } else {
                vec2(-leaf.scroll, 0.0)
            };
            let tabbar_inner_rect = Rect::from_min_size(
                tabbar_outer_rect.min + scroll_offset + collapse_offset,
                vec2(tabbar_outer_rect.width(), tabbar_outer_rect.height()),
            );

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

            let mut clip_rect = tabbar_outer_rect;
            if is_vertical {
                clip_rect.set_height(available_primary);
                if show_collapse_button {
                    clip_rect = clip_rect.translate(vec2(0.0, Style::TAB_COLLAPSE_BUTTON_SIZE));
                }
            } else {
                clip_rect.set_width(available_primary);
                if show_collapse_button {
                    clip_rect = clip_rect.translate(vec2(Style::TAB_COLLAPSE_BUTTON_SIZE, 0.0));
                }
            }
            tabs_ui.set_clip_rect(clip_rect);

            // Desired size for tabs in "expanded" mode.
            let prefered_width = style
                .tab_bar
                .fill_tab_bar
                .then_some(available_primary / (leaf.tabs.len() as f32));

            let tab_hovered = self.tabs(
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

            // Draw hline from tab end to edge of tab bar.
            let px = ui.ctx().pixels_per_point().recip();
            let style = fade_style.unwrap_or_else(|| self.style.as_ref().unwrap());

            match position {
                TabBarPosition::Top => {
                    ui.painter().hline(
                        tabs_ui
                            .min_rect()
                            .right()
                            .min(clip_rect.right())..=tabbar_outer_rect.right(),
                        tabbar_outer_rect.bottom() - px,
                        (px, style.tab_bar.hline_color),
                    );
                }
                TabBarPosition::Bottom => {
                    ui.painter().hline(
                        tabs_ui
                            .min_rect()
                            .right()
                            .min(clip_rect.right())..=tabbar_outer_rect.right(),
                        tabbar_outer_rect.top() + px,
                        (px, style.tab_bar.hline_color),
                    );
                }
                TabBarPosition::Left => {
                    ui.painter().vline(
                        tabbar_outer_rect.right() - px,
                        tabs_ui
                            .min_rect()
                            .bottom()
                            .min(clip_rect.bottom())..=tabbar_outer_rect.bottom(),
                        (px, style.tab_bar.hline_color),
                    );
                }
                TabBarPosition::Right => {
                    ui.painter().vline(
                        tabbar_outer_rect.left() + px,
                        tabs_ui
                            .min_rect()
                            .bottom()
                            .min(clip_rect.bottom())..=tabbar_outer_rect.bottom(),
                        (px, style.tab_bar.hline_color),
                    );
                }
            };

            // Add button at the ends of the tab bar.
            if self.show_add_buttons {
                let remaining_space = if is_vertical {
                    (clip_rect.height() - tabs_ui.min_rect().height()).at_least(0.0)
                } else {
                    (clip_rect.width() - tabs_ui.min_rect().width()).at_least(0.0)
                };
                let offset = match style.buttons.add_tab_align {
                    TabAddAlign::Left => remaining_space,
                    TabAddAlign::Right => 0.0,
                } + if self.show_leaf_close_all_buttons {
                    Style::TAB_CLOSE_ALL_BUTTON_SIZE
                } else {
                    0.0
                };
                self.tab_plus(
                    ui,
                    surface_index,
                    node_index,
                    tab_viewer,
                    tabbar_outer_rect,
                    offset,
                    fade_style,
                    position,
                );
            }

            if self.show_leaf_close_all_buttons {
                // Current leaf contains non-closable tabs.
                let disabled = self.dock_state[surface_index][node_index]
                    .get_leaf_mut()
                    .map(|leaf| !leaf.tabs.iter_mut().all(|tab| tab_viewer.is_closeable(tab)))
                    .expect("This node must be a leaf");

                // Current window contains non-closable tabs.
                let close_window_disabled = disabled
                    || !self.dock_state[surface_index].iter_mut().all(|node| {
                        node.get_leaf_mut().is_none_or(|leaf| {
                            leaf.tabs.iter_mut().all(|tab| tab_viewer.is_closeable(tab))
                        })
                    });

                self.tab_close_all(
                    ui,
                    surface_index,
                    node_index,
                    tabbar_outer_rect,
                    fade_style,
                    disabled,
                    close_window_disabled,
                    position,
                    show_collapse_button,
                )
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

            (
                if is_vertical {
                    tabs_ui.min_rect().height()
                } else {
                    tabs_ui.min_rect().width()
                },
                tab_hovered,
            )
        };

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
        );

        tabbar_outer_rect
    }

    #[allow(clippy::too_many_arguments)]
    fn tabs(
        &mut self,
        tabs_ui: &mut Ui,
        state: &mut State,
        (surface_index, node_index): (SurfaceIndex, NodeIndex),
        tab_viewer: &mut impl TabViewer<Tab = Tab>,
        tabbar_outer_rect: Rect,
        preferred_width: Option<f32>,
        fade: Option<&Style>,
        position: TabBarPosition,
        _collapse_allowed: bool,
    ) -> bool {
        let mut tab_hovered = false;

        assert!(self.dock_state[surface_index][node_index].is_leaf());

        let focused = self.dock_state.focused_leaf();
        let tabs_len = {
            let tabs = self.dock_state[surface_index][node_index]
                .tabs()
                .expect("This node must be a leaf here");
            tabs.len()
        };

        for tab_index in 0..tabs_len {
            let is_vertical = position.is_vertical();
            let id = self
                .id
                .with((surface_index, "surface"))
                .with((node_index, "node"))
                .with((tab_index, "tab"));
            let tab_index = TabIndex(tab_index);
            let is_being_dragged = tabs_ui.ctx().is_being_dragged(id)
                && tabs_ui.input(|i| i.pointer.is_decidedly_dragging())
                && self.draggable_tabs;

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
                    tab_viewer.is_closeable(&leaf.tabs[tab_index.0]),
                )
            };

            let show_close_button = self.show_close_buttons && closeable;

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
                if tab_index.0 != 0 {
                    let spacing = if is_vertical {
                        vec2(0.0, tab_style.spacing)
                    } else {
                        vec2(tab_style.spacing, 0.0)
                    };
                    tabs_ui.allocate_space(spacing);
                }
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
                    let close_button =
                        Button::new(&self.dock_state.translations.tab_context_menu.close_button);

                    response.context_menu(|ui| {
                        let leaf = self.dock_state[surface_index][node_index]
                            .get_leaf_mut()
                            .expect("This node must be a leaf");
                        let tab = &mut leaf.tabs[tab_index.0];

                        tab_viewer.context_menu(ui, tab, surface_index, node_index);
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

            // Paint hline below each tab unless its active (or option says otherwise).
            let leaf = self.dock_state[surface_index][node_index]
                .get_leaf_mut()
                .unwrap();
            let tab = &mut leaf.tabs[tab_index.0];
            let style = fade.unwrap_or_else(|| self.style.as_ref().unwrap());
            let tab_style = tab_viewer.tab_style_override(tab, &style.tab);
            let tab_style = tab_style.as_ref().unwrap_or(&style.tab);

            if !is_active || tab_style.hline_below_active_tab_name {
                let px = tabs_ui.ctx().pixels_per_point().recip();
                match position {
                    TabBarPosition::Top => {
                        tabs_ui.painter().hline(
                            response.rect.x_range(),
                            tabbar_outer_rect.bottom() - px,
                            (px, style.tab_bar.hline_color),
                        );
                    }
                    TabBarPosition::Bottom => {
                        tabs_ui.painter().hline(
                            response.rect.x_range(),
                            tabbar_outer_rect.top() + px,
                            (px, style.tab_bar.hline_color),
                        );
                    }
                    TabBarPosition::Left => {
                        tabs_ui.painter().vline(
                            tabbar_outer_rect.right() - px,
                            response.rect.y_range(),
                            (px, style.tab_bar.hline_color),
                        );
                    }
                    TabBarPosition::Right => {
                        tabs_ui.painter().vline(
                            tabbar_outer_rect.left() + px,
                            response.rect.y_range(),
                            (px, style.tab_bar.hline_color),
                        );
                    }
                };
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

        tab_hovered
    }

    /// Draws the tab add button.
    #[allow(clippy::too_many_arguments)]
    fn tab_plus(
        &mut self,
        ui: &mut Ui,
        surface_index: SurfaceIndex,
        node_index: NodeIndex,
        tab_viewer: &mut impl TabViewer<Tab = Tab>,
        tabbar_outer_rect: Rect,
        offset: f32,
        fade_style: Option<&Style>,
        position: TabBarPosition,
    ) {
        let rect = match position {
            TabBarPosition::Top | TabBarPosition::Bottom => Rect::from_min_size(
                pos2(
                    tabbar_outer_rect.right() - Style::TAB_ADD_BUTTON_SIZE - offset,
                    tabbar_outer_rect.top(),
                ),
                vec2(Style::TAB_ADD_BUTTON_SIZE, tabbar_outer_rect.height()),
            ),
            TabBarPosition::Left | TabBarPosition::Right => Rect::from_min_size(
                pos2(
                    tabbar_outer_rect.left(),
                    tabbar_outer_rect.bottom() - Style::TAB_ADD_BUTTON_SIZE - offset,
                ),
                vec2(tabbar_outer_rect.width(), Style::TAB_ADD_BUTTON_SIZE),
            ),
        };

        let ui = &mut ui.new_child(
            UiBuilder::new()
                .max_rect(rect)
                .layout(Layout::left_to_right(Align::Center))
                .id_salt((node_index, "tab_add")),
        );

        let (rect, mut response) = ui.allocate_exact_size(ui.available_size(), Sense::click());

        response = response.on_hover_cursor(CursorIcon::PointingHand);

        let style = fade_style.unwrap_or_else(|| self.style.as_ref().unwrap());
        let color = if response.hovered() || response.has_focus() {
            ui.painter()
                .rect_filled(rect, CornerRadius::ZERO, style.buttons.add_tab_bg_fill);
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
    #[allow(clippy::too_many_arguments)]
    #[allow(unused_assignments)]
    fn tab_close_all(
        &mut self,
        ui: &mut Ui,
        surface_index: SurfaceIndex,
        node_index: NodeIndex,
        tabbar_outer_rect: Rect,
        fade_style: Option<&Style>,
        disabled: bool,
        close_window_disabled: bool,
        position: TabBarPosition,
        show_collapse_button: bool,
    ) {
        let rect = match position {
            TabBarPosition::Top | TabBarPosition::Bottom => {
                let offset = if show_collapse_button {
                    Style::TAB_COLLAPSE_BUTTON_SIZE
                } else {
                    0.0
                };
                Rect::from_min_size(
                    pos2(
                        tabbar_outer_rect.right()
                            - Style::TAB_CLOSE_ALL_BUTTON_SIZE
                            - offset,
                        tabbar_outer_rect.top(),
                    ),
                    vec2(Style::TAB_CLOSE_ALL_BUTTON_SIZE, tabbar_outer_rect.height()),
                )
            }
            TabBarPosition::Left | TabBarPosition::Right => {
                let offset = if show_collapse_button {
                    Style::TAB_COLLAPSE_BUTTON_SIZE
                } else {
                    0.0
                };
                Rect::from_min_size(
                    pos2(
                        tabbar_outer_rect.left(),
                        tabbar_outer_rect.bottom()
                            - Style::TAB_CLOSE_ALL_BUTTON_SIZE
                            - offset,
                    ),
                    vec2(tabbar_outer_rect.width(), Style::TAB_CLOSE_ALL_BUTTON_SIZE),
                )
            }
        };

        let ui = &mut ui.new_child(
            UiBuilder::new()
                .max_rect(rect)
                .layout(Layout::left_to_right(Align::Center))
                .id_salt((node_index, "tab_close_all")),
        );

        let (rect, mut response) = ui.allocate_exact_size(ui.available_size(), Sense::click());

        let style = fade_style.unwrap_or_else(|| self.style.as_ref().unwrap());

        // Whether we're on "secondary button mode" due to modifier keys
        let on_secondary_button = self.is_on_secondary_button(surface_index, ui, &response);

        let mut stroke_color = if disabled {
            style.buttons.close_all_tabs_disabled_color
        } else if response.hovered() || response.has_focus() {
            if !(close_window_disabled && on_secondary_button) {
                ui.painter().rect_filled(
                    rect,
                    CornerRadius::ZERO,
                    style.buttons.close_all_tabs_bg_fill,
                );
            }
            style.buttons.close_all_tabs_active_color
        } else {
            style.buttons.close_all_tabs_color
        };

        let mut close_all_rect = rect;

        rect_set_size_centered(&mut close_all_rect, Vec2::splat(Style::TAB_CLOSE_ALL_SIZE));

        if !disabled {
            response = response.on_hover_cursor(CursorIcon::PointingHand);
        }

        if on_secondary_button {
            // Close the entire window
            if close_window_disabled {
                stroke_color = style.buttons.close_all_tabs_disabled_color;
                response = response
                    .on_hover_cursor(CursorIcon::NotAllowed)
                    .on_hover_text(
                        self.dock_state
                            .translations
                            .leaf
                            .close_all_button_disabled_tooltip
                            .as_str(),
                    );
            }
            Self::draw_close_window_symbol(ui, stroke_color, close_all_rect);
        } else {
            // Close all tabs in this leaf
            if !disabled {
                if !surface_index.is_main() && self.secondary_button_context_menu {
                    response.context_menu(|ui| {
                        ui.add_enabled_ui(!close_window_disabled, |ui| {
                            if ui
                                .button(&self.dock_state.translations.leaf.close_all_button)
                                .on_disabled_hover_text(
                                    self.dock_state
                                        .translations
                                        .leaf
                                        .close_all_button_disabled_tooltip
                                        .as_str(),
                                )
                                .clicked()
                            {
                                self.to_remove.push(TabRemoval::Window(surface_index));
                            }
                        });
                    });
                }
            } else {
                response = response
                    .on_hover_cursor(CursorIcon::NotAllowed)
                    .on_hover_text(
                        self.dock_state
                            .translations
                            .leaf
                            .close_button_disabled_tooltip
                            .as_str(),
                    );
            }

            if response.clicked() {
                if on_secondary_button {
                    if !close_window_disabled {
                        self.to_remove.push(TabRemoval::Window(surface_index));
                    }
                } else if !disabled {
                    self.to_remove
                        .push(TabRemoval::Node(surface_index, node_index));
                }
            }

            ui.painter().line_segment(
                [close_all_rect.left_top(), close_all_rect.right_bottom()],
                Stroke::new(1.0, stroke_color),
            );
            ui.painter().line_segment(
                [close_all_rect.right_top(), close_all_rect.left_bottom()],
                Stroke::new(1.0, stroke_color),
            );
        }

        // Draw button left border.
        let stroke = Stroke::new(
            ui.ctx().pixels_per_point().recip(),
            style.buttons.close_all_tabs_border_color,
        );
        if position.is_vertical() {
            ui.painter()
                .hline(rect.x_range(), rect.top(), stroke.clone());
        } else {
            ui.painter().vline(rect.left(), rect.y_range(), stroke);
        }

        if !disabled && !on_secondary_button {
            response = self.show_tooltip_hints(surface_index, response);
        }
    }

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

    fn draw_close_window_symbol(ui: &mut Ui, stroke_color: Color32, close_all_rect: Rect) {
        ui.painter().add(Shape::line(
            vec![
                close_all_rect
                    .right_center()
                    .lerp(close_all_rect.right_bottom(), 0.5),
                close_all_rect.right_bottom(),
                close_all_rect.left_bottom(),
                close_all_rect.left_top(),
                close_all_rect
                    .center_top()
                    .lerp(close_all_rect.left_top(), 0.5),
            ],
            Stroke::new(1.0, stroke_color),
        ));
        ui.painter().line_segment(
            [close_all_rect.center_top(), close_all_rect.right_center()],
            Stroke::new(1.0, stroke_color),
        );
        ui.painter().line_segment(
            [close_all_rect.center(), close_all_rect.right_top()],
            Stroke::new(1.0, stroke_color),
        );
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
    ) -> (Response, Option<Response>) {
        let style = fade.unwrap_or_else(|| self.style.as_ref().unwrap());
        let galley = label.into_galley(ui, None, f32::INFINITY, TextStyle::Button);
        let x_spacing = 8.0;
        let y_spacing = 6.0;
        let text_width = galley.size().x + 2.0 * x_spacing;
        let text_height = galley.size().y + 2.0 * y_spacing;
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
        let (_, tab_rect) = ui.allocate_space(tab_size);
        let mut response = ui.interact(tab_rect, id, Sense::click_and_drag());
        if ui.ctx().dragged_id().is_none() && self.draggable_tabs {
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

        // Draw the full tab first and then the stroke on top to avoid the stroke
        // mixing with the background color.
        ui.painter()
            .rect_filled(tab_rect, tab_style.corner_radius, tab_style.bg_fill);
        let stroke_rect = rect_stroke_box(tab_rect, 1.0);
        ui.painter().rect_stroke(
            stroke_rect,
            tab_style.corner_radius,
            Stroke::new(1.0, tab_style.outline_color),
            StrokeKind::Inside,
        );
        if !is_being_dragged {
            // Make the tab name area connect with the tab ui area.
            match position {
                TabBarPosition::Top => {
                    ui.painter().hline(
                        RangeInclusive::new(
                            stroke_rect.min.x + f32::max(tab_style.corner_radius.sw.into(), 1.5),
                            stroke_rect.max.x - f32::max(tab_style.corner_radius.se.into(), 1.5),
                        ),
                        stroke_rect.bottom(),
                        Stroke::new(2.0, tab_style.bg_fill),
                    );
                }
                TabBarPosition::Bottom => {
                    ui.painter().hline(
                        RangeInclusive::new(
                            stroke_rect.min.x + f32::max(tab_style.corner_radius.nw.into(), 1.5),
                            stroke_rect.max.x - f32::max(tab_style.corner_radius.ne.into(), 1.5),
                        ),
                        stroke_rect.top(),
                        Stroke::new(2.0, tab_style.bg_fill),
                    );
                }
                TabBarPosition::Left => {
                    ui.painter().vline(
                        stroke_rect.right(),
                        RangeInclusive::new(
                            stroke_rect.min.y + f32::max(tab_style.corner_radius.se.into(), 1.5),
                            stroke_rect.max.y - f32::max(tab_style.corner_radius.ne.into(), 1.5),
                        ),
                        Stroke::new(2.0, tab_style.bg_fill),
                    );
                }
                TabBarPosition::Right => {
                    ui.painter().vline(
                        stroke_rect.left(),
                        RangeInclusive::new(
                            stroke_rect.min.y + f32::max(tab_style.corner_radius.sw.into(), 1.5),
                            stroke_rect.max.y - f32::max(tab_style.corner_radius.nw.into(), 1.5),
                        ),
                        Stroke::new(2.0, tab_style.bg_fill),
                    );
                }
            };
        }

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
                let pos =
                    Align2::CENTER_CENTER.pos_in_rect(&text_rect.shrink2(vec2(x_spacing, 0.0)));
                pos - galley.size() / 2.0
            };

            ui.painter()
                .add(TextShape::new(text_pos, galley, tab_style.text_color));
        }

        let close_response = show_close_button.then(|| {
            let mut close_button_rect = tab_rect;
            if position.is_vertical() {
                close_button_rect.set_top(text_rect.bottom());
            } else {
                close_button_rect.set_left(text_rect.right());
            }
            close_button_rect =
                Rect::from_center_size(close_button_rect.center(), Vec2::splat(close_button_size));

            let close_response = ui
                .interact(close_button_rect, id.with("close-button"), Sense::click())
                .on_hover_cursor(CursorIcon::PointingHand);

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
            if style.tab_bar.show_scroll_bar_on_overflow {
                // Draw scroll bar
                let bar_height = 7.5;
                let bar_size = if position.is_vertical() {
                    vec2(bar_height, scroll_bar_width)
                } else {
                    vec2(scroll_bar_width, bar_height)
                };
                let (scroll_bar_rect, _scroll_bar_response) =
                    ui.allocate_exact_size(bar_size, Sense::click_and_drag());

                // Compute scroll bar handle position and size.
                let overflow_ratio = actual_width / available_width;
                let scroll_ratio = -leaf.scroll / overflow;

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

                // Coefficient to apply to input displacements so that we move the scroll by the correct amount.
                let points_to_scroll_coefficient = overflow
                    / if position.is_vertical() {
                        scroll_bar_rect.height() - scroll_bar_handle_size
                    } else {
                        scroll_bar_rect.width() - scroll_bar_handle_size
                    };

                let drag_delta = scroll_bar_handle_response.drag_delta();
                let drag_delta_primary = if position.is_vertical() {
                    drag_delta.y
                } else {
                    drag_delta.x
                };

                leaf.scroll -= drag_delta_primary * points_to_scroll_coefficient;

                if let Some(pos) = state.last_hover_pos {
                    if scroll_bar_rect.contains(pos) {
                        let scroll_delta = ui.input(|i| {
                            if position.is_vertical() {
                                i.smooth_scroll_delta.y
                            } else {
                                i.smooth_scroll_delta.y + i.smooth_scroll_delta.x
                            }
                        });
                        leaf.scroll += scroll_delta * points_to_scroll_coefficient;
                    }
                }

                // Draw the bar.
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

            // Handle user input.
            if tabbar_response.hovered() || tab_hovered {
                let scroll_delta = ui.input(|i| {
                    if position.is_vertical() {
                        i.smooth_scroll_delta.y
                    } else {
                        i.smooth_scroll_delta.y + i.smooth_scroll_delta.x
                    }
                });
                leaf.scroll += scroll_delta;
            }
        }

        leaf.scroll = leaf.scroll.clamp(-overflow, 0.0);
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
                let tab_body_rect = ui.clip_rect().expand2(vec2(
                    effective_stroke_width,
                    effective_stroke_width,
                ));
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
