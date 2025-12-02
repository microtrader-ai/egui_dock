use egui::{Id, Pos2, Rect, Vec2, ViewportId};

/// The state of a [`Surface::Window`](crate::Surface::Window).
///
/// Doubles as a handle for the surface, allowing the user to set its size and position.
#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub struct WindowState {
    /// The [`Rect`] that this window was last taking up.
    screen_rect: Option<Rect>,

    /// Was this window dragged in the last frame?
    dragged: bool,

    /// The next position this window should be set to next frame.
    next_position: Option<Pos2>,

    /// The next size this window should be set to next frame.
    next_size: Option<Vec2>,

    /// The height of the window before it was fully collapsed
    expanded_height: Option<f32>,

    /// True the first frame this window is drawn.
    /// handles expanding after being fully collapsed, etc.
    pub(crate) new: bool,

    /// True if the window is minimized
    minimized: bool,

    /// The viewport ID for this window (used for independent viewports)
    #[cfg_attr(feature = "serde", serde(skip))]
    viewport_id: Option<ViewportId>,

    /// True if the viewport should be closed
    #[cfg_attr(feature = "serde", serde(skip))]
    should_close: bool,

    /// Remember the original node ID this window was detached from (for move back)
    #[cfg_attr(feature = "serde", serde(skip))]
    original_node_id: Option<String>,

    /// Remember the original tab index within the node (for move back)
    #[cfg_attr(feature = "serde", serde(skip))]
    original_tab_index: Option<usize>,
}

impl Default for WindowState {
    fn default() -> Self {
        Self {
            screen_rect: None,
            dragged: false,
            next_position: None,
            next_size: None,
            expanded_height: None,
            new: true,
            minimized: false,
            viewport_id: None,
            should_close: false,
            original_node_id: None,
            original_tab_index: None,
        }
    }
}

impl WindowState {
    /// Create a default window state.
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Set the position for this window in screen coordinates.
    pub fn set_position(&mut self, position: Pos2) -> &mut Self {
        self.next_position = Some(position);
        self
    }

    /// Set the size of this window in egui points.
    pub fn set_size(&mut self, size: Vec2) -> &mut Self {
        self.next_size = Some(size);
        self
    }

    /// Get the [`Rect`] which this window occupies.
    /// If this window hasn't been shown before, this will be [`Rect::NOTHING`].
    pub fn rect(&self) -> Rect {
        // The reason why we're unwrapping an Option with a default value instead of
        // just storing Rect::NOTHING for the None variant is that deserializing Rect::NOTHING
        // with serde_json causes a panic, because f32::INFINITY serializes into null in JSON.
        self.screen_rect.unwrap_or(Rect::NOTHING)
    }

    /// Returns if this window is currently being dragged or not.
    pub fn dragged(&self) -> bool {
        self.dragged
    }

    /// Set the height of this window when it is expanded.
    #[inline(always)]
    pub(crate) fn set_expanded_height(&mut self, height: f32) -> &mut Self {
        self.expanded_height = Some(height);
        self
    }

    #[inline(always)]
    pub(crate) fn set_new(&mut self, new: bool) -> &mut Self {
        self.new = new;
        self
    }

    #[inline(always)]
    pub(crate) fn next_position(&mut self) -> Option<Pos2> {
        self.next_position.take()
    }

    #[inline(always)]
    pub(crate) fn next_size(&mut self) -> Option<Vec2> {
        self.next_size.take()
    }

    #[inline(always)]
    pub(crate) fn expanded_height(&mut self) -> Option<f32> {
        self.expanded_height.take()
    }

    #[inline(always)]
    pub(crate) fn toggle_minimized(&mut self) {
        self.minimized = !self.minimized;
    }

    #[inline(always)]
    pub(crate) fn is_minimized(&self) -> bool {
        self.minimized
    }

    /// Get or create the viewport ID for this window.
    #[inline(always)]
    pub(crate) fn get_or_create_viewport_id(&mut self, base_id: Id) -> ViewportId {
        if let Some(id) = self.viewport_id {
            id
        } else {
            let id = ViewportId::from_hash_of(base_id);
            self.viewport_id = Some(id);
            id
        }
    }

    /// Mark this viewport as should be closed.
    #[inline(always)]
    pub(crate) fn mark_close(&mut self) {
        self.should_close = true;
    }

    /// Check if this viewport should be closed.
    #[inline(always)]
    pub(crate) fn should_close(&self) -> bool {
        self.should_close
    }

    /// Reset the close flag.
    #[inline(always)]
    pub(crate) fn reset_close(&mut self) {
        self.should_close = false;
    }

    /// Set the original node ID this window was detached from.
    #[inline(always)]
    pub(crate) fn set_original_node_id(&mut self, node_id: String) {
        self.original_node_id = Some(node_id);
    }

    /// Get the original node ID this window was detached from.
    #[inline(always)]
    pub(crate) fn original_node_id(&self) -> Option<&str> {
        self.original_node_id.as_deref()
    }

    /// Set the original tab index this window was detached from.
    #[inline(always)]
    pub(crate) fn set_original_tab_index(&mut self, tab_index: usize) {
        self.original_tab_index = Some(tab_index);
    }

    /// Get the original tab index this window was detached from.
    #[inline(always)]
    pub(crate) fn original_tab_index(&self) -> Option<usize> {
        self.original_tab_index
    }

    //the 'static in this case means that the `open` field is always `None`
    pub(crate) fn create_window(&mut self, id: Id, _bounds: Rect) -> egui::Window<'static> {
        let new = self.new;
        let mut window_constructor = egui::Window::new("")
            .id(id)
            // .constrain_to(bounds)  // Removed: allow windows to move outside main window
            .title_bar(false);

        if let Some(position) = self.next_position() {
            window_constructor = window_constructor.current_pos(position);
        }
        if let Some(size) = self.next_size() {
            window_constructor = window_constructor.fixed_size(size);
        }
        // Reset the height of the window if it is now expanded
        if new {
            if let Some(height) = self.expanded_height() {
                window_constructor = window_constructor.max_height(height).min_height(height);
            }
        }
        self.new = false;
        window_constructor
    }
}
