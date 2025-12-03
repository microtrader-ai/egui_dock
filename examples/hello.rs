#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;

use eframe::NativeOptions;
use egui::Color32;
use egui::{
    color_picker::{color_edit_button_srgba, Alpha},
    vec2, CentralPanel, ComboBox, CornerRadius, Frame, Slider, TopBottomPanel, Ui, ViewportBuilder,
    WidgetText,
};
use uuid::Uuid;

use egui_dock::tab_viewer::OnCloseResponse;
use egui_dock::{
    AllowedDrops, AllowedSplits, DockArea, DockState, NodeIndex, OverlayType, Style, SurfaceIndex,
    TabBarPosition, TabInteractionStyle, TabViewer,
};

/// Adds a widget with a label next to it, can be given an extra parameter in order to show a hover text
macro_rules! labeled_widget {
    ($ui:expr, $x:expr, $l:expr) => {
        $ui.horizontal(|ui| {
            ui.add($x);
            ui.label($l);
        });
    };
    ($ui:expr, $x:expr, $l:expr, $d:expr) => {
        $ui.horizontal(|ui| {
            ui.add($x).on_hover_text($d);
            ui.label($l).on_hover_text($d);
        });
    };
}

// Creates a slider which has a unit attached to it
// When given an extra parameter it will be used as a multiplier (e.g 100.0 when working with percentages)
macro_rules! unit_slider {
    ($val:expr, $range:expr) => {
        egui::Slider::new($val, $range)
    };
    ($val:expr, $range:expr, $unit:expr) => {
        egui::Slider::new($val, $range).custom_formatter(|value, decimal_range| {
            egui::emath::format_with_decimals_in_range(value, decimal_range) + $unit
        })
    };
    ($val:expr, $range:expr, $unit:expr, $mul:expr) => {
        egui::Slider::new($val, $range)
            .custom_formatter(|value, decimal_range| {
                egui::emath::format_with_decimals_in_range(value * $mul, decimal_range) + $unit
            })
            .custom_parser(|string| string.parse::<f64>().ok().map(|valid| valid / $mul))
    };
}

fn main() -> eframe::Result<()> {
    std::env::set_var("RUST_BACKTRACE", "1");
    let options = NativeOptions {
        viewport: ViewportBuilder::default().with_inner_size(vec2(1024.0, 1024.0)),
        ..Default::default()
    };
    eframe::run_native(
        "My egui App",
        options,
        Box::new(|cc| {
            // Set dark theme
            cc.egui_ctx.set_visuals(egui::Visuals::dark());
            Ok(Box::<MyApp>::default())
        }),
    )
}

struct MyContext {
    pub title: String,
    pub age: u32,
    pub style: Option<Style>,
    open_tabs: HashSet<String>,

    node_positions: HashMap<NodeIndex, TabBarPosition>,
    left_root: NodeIndex,
    left_bottom: NodeIndex,
    right_top: NodeIndex,
    bottom_root: NodeIndex,
    tab_bar_position: TabBarPosition,
    show_close_buttons: bool,
    draggable_tabs: bool,
    show_tab_name_on_hover: bool,
    allowed_splits: AllowedSplits,
    show_leaf_close_all: bool,
    show_leaf_collapse: bool,
    show_secondary_button_hint: bool,
    secondary_button_on_modifier: bool,
    secondary_button_context_menu: bool,
    next_tab_id: usize,
}

struct MyApp {
    context: MyContext,
    tree: DockState<String>,
}

impl MyContext {
    fn is_descendant(candidate: NodeIndex, root: NodeIndex) -> bool {
        let mut current = candidate;
        loop {
            if current == root {
                return true;
            }
            if let Some(parent) = current.parent() {
                current = parent;
            } else {
                return false;
            }
        }
    }
}

impl TabViewer for MyContext {
    type Tab = String;

    fn title(&mut self, tab: &mut Self::Tab) -> WidgetText {
        tab.as_str().into()
    }

    fn ui(&mut self, ui: &mut Ui, tab: &mut Self::Tab) {
        let name = tab.as_str();
        match name {
            "Simple Demo" => self.simple_demo(ui),
            "Style Editor" => self.style_editor(ui),
            _ => {
                // Display different content based on tab type
                if name.starts_with("Regular Tab") {
                    ui.label(format!("Content of {}. This is a regular tab.", name));
                } else if name.starts_with("Fancy Tab") {
                    ui.label(
                        egui::RichText::new(format!("Content of {}. This tab is fancy!", name))
                            .italics()
                            .size(20.0)
                            .color(egui::Color32::from_rgb(255, 128, 64)),
                    );
                } else {
                    ui.label(name);
                }
            }
        }
    }

    fn context_menu(
        &mut self,
        ui: &mut Ui,
        tab: &mut Self::Tab,
        _surface: SurfaceIndex,
        _node: NodeIndex,
    ) {
        match tab.as_str() {
            "Simple Demo" => self.simple_demo_menu(ui),
            _ => {
                ui.label(tab.to_string());
                ui.label("This is a context menu");
            }
        }
    }

    fn is_closeable(&self, tab: &Self::Tab) -> bool {
        let name = tab.as_str();
        name == "Inspector"
            || name == "Style Editor"
            || name == "Simple Demo"
            || name == "Extremely Long Tab Name That Should Scroll"
            || name.starts_with("Extra Tab ")
            || name.starts_with("New Tab ")
            || name.starts_with("Regular Tab ")
            || name.starts_with("Fancy Tab ")
            || name.starts_with("Inspector ")
    }

    fn on_close(&mut self, tab: &mut Self::Tab) -> OnCloseResponse {
        self.open_tabs.remove(tab);
        OnCloseResponse::Close
    }

    fn tab_bar_position_for_node(
        &self,
        _surface_index: SurfaceIndex,
        node_index: NodeIndex,
    ) -> Option<TabBarPosition> {
        self.node_positions.get(&node_index).copied()
    }

    fn allow_move_to(
        &self,
        tab: &Self::Tab,
        surface_index: SurfaceIndex,
        node_index: NodeIndex,
    ) -> bool {
        if !surface_index.is_main() {
            return true;
        }
        match tab.as_str() {
            "Inspector" | "Hierarchy" => Self::is_descendant(node_index, self.left_root),
            "File Browser" | "Asset Manager" => Self::is_descendant(node_index, self.bottom_root),
            _ => true,
        }
    }

    fn allow_collapse(&self, _surface_index: SurfaceIndex, node_index: NodeIndex) -> bool {
        // 右2显示折叠按钮，右1隐藏
        Self::is_descendant(node_index, self.bottom_root)
    }
}

impl MyContext {
    fn simple_demo_menu(&mut self, ui: &mut Ui) {
        ui.label("Egui widget example");
        ui.menu_button("Sub menu", |ui| {
            ui.label("hello :)");
        });
    }

    fn simple_demo(&mut self, ui: &mut Ui) {
        ui.heading("My egui Application");

        ui.horizontal(|ui| {
            ui.label("Your name: ");
            ui.text_edit_singleline(&mut self.title);
        });
        ui.add(Slider::new(&mut self.age, 0..=120).text("age"));
        if ui.button("Click each year").clicked() {
            self.age += 1;
        }
        ui.label(format!("Hello '{}', age {}", &self.title, &self.age));
    }

    fn style_editor(&mut self, ui: &mut Ui) {
        ui.heading("Style Editor");

        ui.collapsing("DockArea Options", |ui| {
            ui.checkbox(&mut self.show_close_buttons, "Show close buttons");
            ui.checkbox(&mut self.draggable_tabs, "Draggable tabs");
            ui.checkbox(&mut self.show_tab_name_on_hover, "Show tab name on hover");
            ui.checkbox(
                &mut self.show_leaf_close_all,
                "Show close all button on tab bars",
            );
            ui.checkbox(
                &mut self.show_leaf_collapse,
                "Show collaspse button on tab bars",
            );
            ui.checkbox(
                &mut self.secondary_button_on_modifier,
                "Enable secondary buttons when modifiers (Shift by default) are pressed",
            );
            ui.checkbox(
                &mut self.secondary_button_context_menu,
                "Enable secondary buttons in right-click context menus",
            );
            ui.checkbox(
                &mut self.show_secondary_button_hint,
                "Show tooltip hints for secondary buttons",
            );
            ComboBox::new("cbox:allowed_splits", "Split direction(s)")
                .selected_text(format!("{:?}", self.allowed_splits))
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.allowed_splits, AllowedSplits::All, "All");
                    ui.selectable_value(
                        &mut self.allowed_splits,
                        AllowedSplits::LeftRightOnly,
                        "LeftRightOnly",
                    );
                    ui.selectable_value(
                        &mut self.allowed_splits,
                        AllowedSplits::TopBottomOnly,
                        "TopBottomOnly",
                    );
                    ui.selectable_value(&mut self.allowed_splits, AllowedSplits::None, "None");
                });
        });

        let style = self.style.as_mut().unwrap();

        ui.collapsing("Border", |ui| {
            egui::Grid::new("border").show(ui, |ui| {
                ui.label("Width:");
                ui.add(Slider::new(
                    &mut style.main_surface_border_stroke.width,
                    1.0..=50.0,
                ));
                ui.end_row();

                ui.label("Color:");
                color_edit_button_srgba(
                    ui,
                    &mut style.main_surface_border_stroke.color,
                    Alpha::OnlyBlend,
                );
                ui.end_row();

                ui.label("Corner radius:");
                corner_radius_ui(ui, &mut style.main_surface_border_rounding);
                ui.end_row();
            });
        });

        ui.collapsing("Separator", |ui| {
            egui::Grid::new("separator").show(ui, |ui| {
                ui.label("Width:");
                ui.add(Slider::new(&mut style.separator.width, 1.0..=50.0));
                ui.end_row();

                ui.label("Extra Interact Width:");
                ui.add(Slider::new(
                    &mut style.separator.extra_interact_width,
                    0.0..=50.0,
                ));
                ui.end_row();

                ui.label("Offset limit:");
                ui.add(Slider::new(&mut style.separator.extra, 1.0..=300.0));
                ui.end_row();

                ui.label("Idle color:");
                color_edit_button_srgba(ui, &mut style.separator.color_idle, Alpha::OnlyBlend);
                ui.end_row();

                ui.label("Hovered color:");
                color_edit_button_srgba(ui, &mut style.separator.color_hovered, Alpha::OnlyBlend);
                ui.end_row();

                ui.label("Dragged color:");
                color_edit_button_srgba(ui, &mut style.separator.color_dragged, Alpha::OnlyBlend);
                ui.end_row();
            });
        });

        ui.collapsing("Tabs", |ui| {
            ui.separator();

            ui.checkbox(&mut style.tab_bar.fill_tab_bar, "Expand tabs");
            ui.checkbox(
                &mut style.tab_bar.show_scroll_bar_on_overflow,
                "Show scroll bar on tab overflow",
            );
            ui.checkbox(
                &mut style.tab.hline_below_active_tab_name,
                "Show a line below the active tab name",
            );
            ui.horizontal(|ui| {
                ui.add(Slider::new(&mut style.tab_bar.height, 20.0..=50.0));
                ui.label("Tab bar height");
            });
            ComboBox::new("tab_bar_position", "Tab bar position")
                .selected_text(format!("{:?}", style.tab_bar.position))
                .show_ui(ui, |ui| {
                    for position in [
                        TabBarPosition::Top,
                        TabBarPosition::Bottom,
                        TabBarPosition::Left,
                        TabBarPosition::Right,
                    ] {
                        ui.selectable_value(
                            &mut style.tab_bar.position,
                            position,
                            format!("{position:?}"),
                        );
                    }
                });

            ComboBox::new("add_button_align", "Add button align")
                .selected_text(format!("{:?}", style.buttons.add_tab_align))
                .show_ui(ui, |ui| {
                    for align in [egui_dock::TabAddAlign::Left, egui_dock::TabAddAlign::Right] {
                        ui.selectable_value(
                            &mut style.buttons.add_tab_align,
                            align,
                            format!("{align:?}"),
                        );
                    }
                });

            ui.separator();

            fn tab_style_editor_ui(ui: &mut Ui, tab_style: &mut TabInteractionStyle) {
                ui.separator();

                ui.label("Corner radius");
                labeled_widget!(
                    ui,
                    Slider::new(&mut tab_style.corner_radius.nw, 0..=15),
                    "North-West"
                );
                labeled_widget!(
                    ui,
                    Slider::new(&mut tab_style.corner_radius.ne, 0..=15),
                    "North-East"
                );
                labeled_widget!(
                    ui,
                    Slider::new(&mut tab_style.corner_radius.sw, 0..=15),
                    "South-West"
                );
                labeled_widget!(
                    ui,
                    Slider::new(&mut tab_style.corner_radius.se, 0..=15),
                    "South-East"
                );

                ui.separator();

                egui::Grid::new("tabs_colors").show(ui, |ui| {
                    ui.label("Title text color:");
                    color_edit_button_srgba(ui, &mut tab_style.text_color, Alpha::OnlyBlend);
                    ui.end_row();

                    ui.label("Outline color:")
                        .on_hover_text("The outline around the active tab name.");
                    color_edit_button_srgba(ui, &mut tab_style.outline_color, Alpha::OnlyBlend);
                    ui.end_row();

                    ui.label("Background color:");
                    color_edit_button_srgba(ui, &mut tab_style.bg_fill, Alpha::OnlyBlend);
                    ui.end_row();
                });
            }

            ui.collapsing("Active", |ui| {
                tab_style_editor_ui(ui, &mut style.tab.active);
            });

            ui.collapsing("Inactive", |ui| {
                tab_style_editor_ui(ui, &mut style.tab.inactive);
            });

            ui.collapsing("Focused", |ui| {
                tab_style_editor_ui(ui, &mut style.tab.focused);
            });

            ui.collapsing("Hovered", |ui| {
                tab_style_editor_ui(ui, &mut style.tab.hovered);
            });

            ui.separator();

            egui::Grid::new("tabs_colors").show(ui, |ui| {
                ui.label("Close button color unfocused:");
                color_edit_button_srgba(ui, &mut style.buttons.close_tab_color, Alpha::OnlyBlend);
                ui.end_row();

                ui.label("Close button color focused:");
                color_edit_button_srgba(
                    ui,
                    &mut style.buttons.close_tab_active_color,
                    Alpha::OnlyBlend,
                );
                ui.end_row();

                ui.label("Close button background color:");
                color_edit_button_srgba(ui, &mut style.buttons.close_tab_bg_fill, Alpha::OnlyBlend);
                ui.end_row();

                ui.label("Bar background color:");
                color_edit_button_srgba(ui, &mut style.tab_bar.bg_fill, Alpha::OnlyBlend);
                ui.end_row();

                ui.label("Horizontal line color:").on_hover_text(
                    "The line separating the tab name area from the tab content area",
                );
                color_edit_button_srgba(ui, &mut style.tab_bar.hline_color, Alpha::OnlyBlend);
                ui.end_row();
            });
        });

        ui.collapsing("Tab body", |ui| {
            ui.separator();

            ui.label("Corner radius");
            corner_radius_ui(ui, &mut style.tab.tab_body.corner_radius);

            ui.label("Stroke width:");
            ui.add(Slider::new(
                &mut style.tab.tab_body.stroke.width,
                0.0..=10.0,
            ));
            ui.end_row();

            egui::Grid::new("tab_body_colors").show(ui, |ui| {
                ui.label("Stroke color:");
                color_edit_button_srgba(ui, &mut style.tab.tab_body.stroke.color, Alpha::OnlyBlend);
                ui.end_row();

                ui.label("Background color:");
                color_edit_button_srgba(ui, &mut style.tab.tab_body.bg_fill, Alpha::OnlyBlend);
                ui.end_row();
            });
        });
        ui.collapsing("Overlay", |ui| {
            let selected_text = match style.overlay.overlay_type {
                OverlayType::HighlightedAreas => "Highlighted Areas",
                OverlayType::Widgets => "Widgets",
            };
            ui.label("Overlay Style:");
            ComboBox::new("overlay styles", "")
                .selected_text(selected_text)
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut style.overlay.overlay_type,
                        OverlayType::HighlightedAreas,
                        "Highlighted Areas",
                    );
                    ui.selectable_value(
                        &mut style.overlay.overlay_type,
                        OverlayType::Widgets,
                        "Widgets",
                    );
                });
            ui.collapsing("Feel", |ui|{
                labeled_widget!(
                    ui,
                    unit_slider!(&mut style.overlay.feel.center_drop_coverage, 0.0..=1.0, "%", 100.0),
                    "Center drop coverage",
                    "how big the area where dropping a tab into the center of another should be."
                );
                labeled_widget!(
                    ui,
                    unit_slider!(&mut style.overlay.feel.fade_hold_time, 0.0..=4.0, "s"),
                    "Fade hold time",
                    "How long faded windows should hold their fade before unfading, in seconds."
                );
                labeled_widget!(
                    ui,
                    unit_slider!(&mut style.overlay.feel.max_preference_time, 0.0..=4.0, "s"),
                    "Max preference time",
                    "How long the overlay may prefer to stick to a surface despite hovering over another, in seconds."
                );
                labeled_widget!(
                    ui,
                    unit_slider!(&mut style.overlay.feel.window_drop_coverage, 0.0..=1.0, "%", 100.0),
                    "Window drop coverage",
                    "How big the area for undocking a window should be. [is overshadowed by center drop coverage]"
                );
                labeled_widget!(
                    ui,
                    unit_slider!(&mut style.overlay.feel.interact_expansion, 1.0..=100.0, "ps"),
                    "Interact expansion",
                    "How much extra interaction area should be allocated for buttons on the overlay"
                );
            });

            ui.collapsing("Visuals", |ui|{
                labeled_widget!(
                    ui,
                    unit_slider!(&mut style.overlay.max_button_size, 10.0..=500.0, "ps"),
                    "Max button size",
                    "The max length of a side on a overlay button in egui points"
                );
                labeled_widget!(
                    ui,
                    unit_slider!(&mut style.overlay.button_spacing, 0.0..=50.0, "ps"),
                    "Button spacing",
                    "Spacing between buttons on the overlay, in egui units."
                );
                labeled_widget!(
                    ui,
                    unit_slider!(&mut style.overlay.surface_fade_opacity, 0.0..=1.0, "%", 100.0),
                    "Window fade opacity",
                    "how visible windows are when dragging a tab behind them."
                );
                labeled_widget!(
                    ui,
                    egui::Slider::new(&mut style.overlay.selection_stroke_width, 0.0..=50.0),
                    "Selection stroke width",
                    "width of a selection which uses a outline stroke instead of filled rect."
                );
                egui::Grid::new("overlay style preferences").show(ui, |ui| {
                    ui.label("Button color:");
                    color_edit_button_srgba(ui, &mut style.overlay.button_color, Alpha::OnlyBlend);
                    ui.end_row();

                    ui.label("Button border color:");
                    color_edit_button_srgba(ui, &mut style.overlay.button_border_stroke.color, Alpha::OnlyBlend);
                    ui.end_row();

                    ui.label("Selection color:");
                    color_edit_button_srgba(ui, &mut style.overlay.selection_color, Alpha::OnlyBlend);
                    ui.end_row();

                    ui.label("Button stroke color:");
                    color_edit_button_srgba(ui, &mut style.overlay.button_border_stroke.color, Alpha::OnlyBlend);
                    ui.end_row();

                    ui.label("Button stroke width:");
                    ui.add(Slider::new(&mut style.overlay.button_border_stroke.width, 0.0..=50.0));
                    ui.end_row();
                });
            });

            ui.collapsing("Hover highlight", |ui|{
                egui::Grid::new("leaf highlighting prefs").show(ui, |ui|{
                    ui.label("Fill color:");
                    color_edit_button_srgba(ui, &mut style.overlay.hovered_leaf_highlight.color, Alpha::OnlyBlend);
                    ui.end_row();

                    ui.label("Stroke color:");
                    color_edit_button_srgba(ui, &mut style.overlay.hovered_leaf_highlight.stroke.color, Alpha::OnlyBlend);
                    ui.end_row();

                    ui.label("Stroke width:");
                    ui.add(Slider::new(&mut style.overlay.hovered_leaf_highlight.stroke.width, 0.0..=50.0));
                    ui.end_row();

                    ui.label("Expansion:");
                    ui.add(Slider::new(&mut style.overlay.hovered_leaf_highlight.expansion, -50.0..=50.0));
                    ui.end_row();
                });
                ui.label("Corner radius:");
                corner_radius_ui(ui, &mut style.overlay.hovered_leaf_highlight.corner_radius);
            })
        });
    }
}

impl Default for MyApp {
    fn default() -> Self {
        let tab_bar_position = parse_tab_bar_position(std::env::args());
        let mut dock_state =
            DockState::new(vec!["Simple Demo".to_owned(), "Style Editor".to_owned()]);
        "Undock".clone_into(&mut dock_state.translations.tab_context_menu.eject_button);
        // Split root vertically: left column + right column (initial content)
        let [right_column, left_root] = dock_state.main_surface_mut().split_left(
            NodeIndex::root(),
            0.3,
            vec!["Inspector".to_owned()],
        );

        // Right column: split horizontally into top (existing content) and bottom (File/Asset)
        let [right_top, bottom_node] = dock_state.main_surface_mut().split_below(
            right_column,
            0.7,
            vec!["File Browser".to_owned(), "Asset Manager".to_owned()],
        );
        // Add an extra long title tab to the right-top area to demonstrate clipping/scrolling
        if let Some(leaf) = dock_state[SurfaceIndex::main()][right_top].get_leaf_mut() {
            leaf.append_tab("Extremely Long Tab Name That Should Scroll".to_owned());
            for i in 0..10 {
                leaf.append_tab(format!("Extra Tab {i}"));
            }
        }
        // Add extra tabs to bottom/right2 for scrollbar testing.
        if let Some(leaf) = dock_state[SurfaceIndex::main()][bottom_node].get_leaf_mut() {
            for i in 0..10 {
                leaf.append_tab(format!("Bottom Extra {i}"));
            }
        }

        // Left column: split horizontally into two leaves
        let [left_top, left_bottom] =
            dock_state
                .main_surface_mut()
                .split_below(left_root, 0.5, vec!["Hierarchy".to_owned()]);
        // Add extra tabs to left_bottom for scrollbar testing.
        if let Some(leaf) = dock_state[SurfaceIndex::main()][left_bottom].get_leaf_mut() {
            for i in 0..10 {
                leaf.append_tab(format!("Left Bottom Extra {i}"));
            }
        }

        // Set always_keep = true for all initial nodes in main surface
        dock_state[SurfaceIndex::main()][right_top].set_always_keep(true);
        dock_state[SurfaceIndex::main()][bottom_node].set_always_keep(true);
        dock_state[SurfaceIndex::main()][left_top].set_always_keep(true);
        dock_state[SurfaceIndex::main()][left_bottom].set_always_keep(true);
        // 右1 只允许同族移动：赋予独立 family_id，使其与其他节点不同
        let right_family = Uuid::new_v4().to_string();
        dock_state[SurfaceIndex::main()][right_top].set_family_id(right_family);
        // 右1 允许左右和浮动，禁用上下
        dock_state[SurfaceIndex::main()][right_top].set_allowed_drops(AllowedDrops {
            left: true,
            right: true,
            top: false,
            bottom: false,
            float: true,
            tabs: true,
        });
        // 右2 禁用浮动（其余保持默认）
        dock_state[SurfaceIndex::main()][bottom_node].set_allowed_drops(AllowedDrops {
            float: false,
            ..AllowedDrops::all()
        });

        let mut open_tabs = HashSet::new();

        for node in dock_state[SurfaceIndex::main()].iter() {
            if let Some(tabs) = node.tabs() {
                for tab in tabs {
                    open_tabs.insert(tab.clone());
                }
            }
        }

        // Set node-level positions (no tab-level positions)
        let mut node_positions = HashMap::new();
        node_positions.insert(left_top, TabBarPosition::Left); // Inspector node
        node_positions.insert(left_bottom, TabBarPosition::Left); // Hierarchy + extras node
        node_positions.insert(bottom_node, TabBarPosition::Bottom);
        node_positions.insert(right_top, tab_bar_position);
        let context = MyContext {
            title: "Hello".to_string(),
            age: 24,
            style: None,
            open_tabs,

            node_positions,
            left_root,
            left_bottom,
            right_top,
            bottom_root: bottom_node,
            tab_bar_position,
            show_leaf_close_all: true,
            show_leaf_collapse: true,
            show_secondary_button_hint: true,
            secondary_button_on_modifier: true,
            secondary_button_context_menu: true,
            show_close_buttons: true,
            draggable_tabs: true,
            show_tab_name_on_hover: false,
            allowed_splits: AllowedSplits::default(),
            next_tab_id: 0,
        };

        Self {
            context,
            tree: dock_state,
        }
    }
}

impl eframe::App for MyApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Ensure dark theme is always applied
        ctx.set_visuals(egui::Visuals::dark());

        TopBottomPanel::top("egui_dock::MenuBar").show(ctx, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button("View", |ui| {
                    let left2_visible = !self
                        .tree
                        .leaf_hidden((SurfaceIndex::main(), self.context.left_bottom))
                        .unwrap_or(false);
                    if ui
                        .selectable_label(left2_visible, "Left2 Visible")
                        .clicked()
                    {
                        self.tree.set_leaf_hidden(
                            (SurfaceIndex::main(), self.context.left_bottom),
                            left2_visible,
                        );
                        ui.close();
                    }

                    // allow certain tabs to be toggled
                    for tab in &["File Browser", "Asset Manager"] {
                        if ui
                            .selectable_label(self.context.open_tabs.contains(*tab), *tab)
                            .clicked()
                        {
                            if let Some(index) = self.tree.find_tab(&tab.to_string()) {
                                self.tree.remove_tab(index);
                                self.context.open_tabs.remove(*tab);
                            } else {
                                self.tree[SurfaceIndex::main()]
                                    .push_to_focused_leaf(tab.to_string());
                            }

                            ui.close();
                        }
                    }
                });
            })
        });
        CentralPanel::default()
            // When displaying a DockArea in another UI, it looks better
            // to set inner margins to 0.
            .frame(Frame::central_panel(&ctx.style()).inner_margin(0.))
            .show(ctx, |ui| {
                let style = self.context.style.get_or_insert_with(|| {
                    let mut style = Style::from_egui(ui.style());
                    style.tab_bar.position = self.context.tab_bar_position;
                    // 右1/右2 纵向分隔最小保留 5px
                    style.separator.extra = 5.0;
                    style.separator.color_hovered = Color32::from_rgb(52, 118, 207);
                    style.separator.color_dragged = Color32::from_rgb(52, 118, 207);
                    style.separator.width = 2.0;
                    // 右1 尾部空白 25px
                    style.tab_bar.tail_padding = 25.0;
                    style.tab_bar.auto_tail = true;
                    style.tab_bar.fill_tab_bar = false; // auto_tail 和 fill_tab_bar 互斥
                                                        // 水平分割最多 50%，垂直不限制
                    style.separator.max_fraction = Some(vec2(0.5, 1.0));
                    style
                });
                self.context.tab_bar_position = style.tab_bar.position;
                let style = style.clone();
                let tail_target = self.context.right_top;
                let tail_titles: Arc<Vec<String>> = self.tree[SurfaceIndex::main()][tail_target]
                    .get_leaf()
                    .map(|leaf| Arc::new(leaf.tabs.clone()))
                    .unwrap_or_else(|| Arc::new(Vec::new()));

                DockArea::new(&mut self.tree)
                    .style(style)
                    .show_close_buttons(self.context.show_close_buttons)
                    .draggable_tabs(self.context.draggable_tabs)
                    .show_tab_name_on_hover(self.context.show_tab_name_on_hover)
                    .allowed_splits(self.context.allowed_splits)
                    .show_leaf_close_all_buttons(self.context.show_leaf_close_all)
                    // 使用 TabViewer::allow_collapse 控制各区域折叠按钮
                    .show_leaf_collapse_buttons(true)
                    .show_secondary_button_hint(self.context.show_secondary_button_hint)
                    .secondary_button_on_modifier(self.context.secondary_button_on_modifier)
                    .secondary_button_context_menu(self.context.secondary_button_context_menu)
                    .tab_bar_tail_padding({
                        let titles = tail_titles.clone();
                        move |surface, node, tab| {
                            if surface == SurfaceIndex::main() && node == tail_target {
                                if let Some(current) = titles.get(tab.0) {
                                    if current == "Simple Demo" {
                                        return 60.0;
                                    }
                                }
                                40.0
                            } else {
                                0.0
                            }
                        }
                    })
                    .tab_bar_tail_content({
                        let titles = tail_titles.clone();
                        move |ui, surface, node, tab| {
                            if surface == SurfaceIndex::main() && node == tail_target {
                                let label =
                                    titles.get(tab.0).map(|s| s.as_str()).unwrap_or_default();
                                ui.horizontal(|ui| {
                                    if label == "Simple Demo" {
                                        let add_response = ui.small_button("+");
                                        // Show popup menu when + button is clicked
                                        let popup_id = ui.id().with("custom_add_popup");
                                        if add_response.clicked() {
                                            ui.memory_mut(|mem| mem.toggle_popup(popup_id));
                                        }
                                        egui::popup_below_widget(
                                            ui,
                                            popup_id,
                                            &add_response,
                                            egui::PopupCloseBehavior::CloseOnClickOutside,
                                            |ui| {
                                                ui.set_min_width(120.0);
                                                ui.style_mut().visuals.button_frame = false;

                                                if ui.button("Regular Tab").clicked() {
                                                    ui.ctx().data_mut(|d| {
                                                        d.insert_temp(
                                                            egui::Id::new("add_tab_request"),
                                                            Some((surface, node, "Regular")),
                                                        )
                                                    });
                                                    ui.memory_mut(|mem| mem.close_popup(popup_id));
                                                }

                                                if ui.button("Fancy Tab").clicked() {
                                                    ui.ctx().data_mut(|d| {
                                                        d.insert_temp(
                                                            egui::Id::new("add_tab_request"),
                                                            Some((surface, node, "Fancy")),
                                                        )
                                                    });
                                                    ui.memory_mut(|mem| mem.close_popup(popup_id));
                                                }

                                                if ui.button("Inspector Tab").clicked() {
                                                    ui.ctx().data_mut(|d| {
                                                        d.insert_temp(
                                                            egui::Id::new("add_tab_request"),
                                                            Some((surface, node, "Inspector")),
                                                        )
                                                    });
                                                    ui.memory_mut(|mem| mem.close_popup(popup_id));
                                                }
                                            },
                                        );

                                        if ui.small_button("-").clicked() {
                                            // Handle remove action if needed
                                        }
                                    } else {
                                        let add_response = ui.small_button("+");
                                        let popup_id = ui.id().with("custom_add_popup");
                                        if add_response.clicked() {
                                            ui.memory_mut(|mem| mem.toggle_popup(popup_id));
                                        }
                                        egui::popup_below_widget(
                                            ui,
                                            popup_id,
                                            &add_response,
                                            egui::PopupCloseBehavior::CloseOnClickOutside,
                                            |ui| {
                                                ui.set_min_width(120.0);
                                                ui.style_mut().visuals.button_frame = false;

                                                if ui.button("Regular Tab").clicked() {
                                                    ui.ctx().data_mut(|d| {
                                                        d.insert_temp(
                                                            egui::Id::new("add_tab_request"),
                                                            Some((surface, node, "Regular")),
                                                        )
                                                    });
                                                    ui.memory_mut(|mem| mem.close_popup(popup_id));
                                                }

                                                if ui.button("Fancy Tab").clicked() {
                                                    ui.ctx().data_mut(|d| {
                                                        d.insert_temp(
                                                            egui::Id::new("add_tab_request"),
                                                            Some((surface, node, "Fancy")),
                                                        )
                                                    });
                                                    ui.memory_mut(|mem| mem.close_popup(popup_id));
                                                }

                                                if ui.button("Inspector Tab").clicked() {
                                                    ui.ctx().data_mut(|d| {
                                                        d.insert_temp(
                                                            egui::Id::new("add_tab_request"),
                                                            Some((surface, node, "Inspector")),
                                                        )
                                                    });
                                                    ui.memory_mut(|mem| mem.close_popup(popup_id));
                                                }
                                            },
                                        );
                                    }
                                });
                            }
                        }
                    })
                    .show_inside(ui, &mut self.context);

                // Handle add tab request from tail_content button
                if let Some(Some((surface, node, tab_type))) = ctx.data_mut(|d| {
                    d.remove_temp::<Option<(SurfaceIndex, NodeIndex, &str)>>(egui::Id::new(
                        "add_tab_request",
                    ))
                }) {
                    if surface == SurfaceIndex::main() && node == self.context.right_top {
                        // Add a new tab based on type
                        self.context.next_tab_id += 1;
                        let new_tab = match tab_type {
                            "Regular" => format!("Regular Tab {}", self.context.next_tab_id),
                            "Fancy" => format!("Fancy Tab {}", self.context.next_tab_id),
                            "Inspector" => format!("Inspector {}", self.context.next_tab_id),
                            _ => format!("New Tab {}", self.context.next_tab_id),
                        };
                        self.context.open_tabs.insert(new_tab.clone());
                        if let Some(leaf) = self.tree[surface][node].get_leaf_mut() {
                            leaf.append_tab(new_tab);
                        }
                    }
                }
            });
    }
}

fn corner_radius_ui(ui: &mut Ui, corner_radius: &mut CornerRadius) {
    labeled_widget!(ui, Slider::new(&mut corner_radius.nw, 0..=15), "North-West");
    labeled_widget!(ui, Slider::new(&mut corner_radius.ne, 0..=15), "North-East");
    labeled_widget!(ui, Slider::new(&mut corner_radius.sw, 0..=15), "South-West");
    labeled_widget!(ui, Slider::new(&mut corner_radius.se, 0..=15), "South-East");
}

fn parse_tab_bar_position(args: impl IntoIterator<Item = String>) -> TabBarPosition {
    for arg in args.into_iter().skip(1) {
        if let Some(value) = arg.strip_prefix("--tab-pos=") {
            return match value.to_ascii_lowercase().as_str() {
                "bottom" => TabBarPosition::Bottom,
                "left" => TabBarPosition::Left,
                "right" => TabBarPosition::Right,
                _ => TabBarPosition::Top,
            };
        }
    }
    TabBarPosition::Top
}
