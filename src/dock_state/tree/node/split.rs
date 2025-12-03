use egui::Rect;

use crate::AllowedDrops;

///the inner data of a [``Node::Horizontal``](crate::Node)/[``Node::Vertical``](crate::Node), which splits into two further nodes.
#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub struct SplitNode {
    /// The rectangle in which all children of this node are drawn.
    pub rect: Rect,

    /// The fraction taken by the top child of this node.
    pub fraction: f32,

    /// Whether all subnodes are collapsed.
    pub fully_collapsed: bool,

    /// The number of collapsed leaf subnodes.
    pub collapsed_leaf_count: i32,

    /// Drop permissions inherited from the family.
    #[cfg_attr(feature = "serde", serde(default = "AllowedDrops::all"))]
    pub allowed_drops: AllowedDrops,

    /// Family identifier used to constrain where tabs can be dropped.
    #[cfg_attr(feature = "serde", serde(default))]
    pub(crate) family_id: Option<String>,
}

impl SplitNode {
    /// Create a new ``SplitNode``
    pub fn new(
        rect: Rect,
        fraction: f32,
        fully_collapsed: bool,
        collapsed_leaf_count: i32,
        allowed_drops: AllowedDrops,
        family_id: Option<String>,
    ) -> Self {
        Self {
            rect,
            fraction,
            fully_collapsed,
            collapsed_leaf_count,
            allowed_drops,
            family_id,
        }
    }
    /// Set the Area which this ``SplitNode`` occupies.
    #[inline]
    pub fn set_rect(&mut self, new_rect: Rect) {
        self.rect = new_rect;
    }

    /// Get the Area which this ``SplitNode`` occupies.
    pub fn rect(&self) -> Rect {
        self.rect
    }
}
