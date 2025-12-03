/// Per-node drop permissions.
///
/// Used to decide which drop zones to render and accept.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[cfg_attr(feature = "serde", serde(default))]
pub struct AllowedDrops {
    /// Allow dropping to the left (horizontal split).
    pub left: bool,
    /// Allow dropping to the right (horizontal split).
    pub right: bool,
    /// Allow dropping above (vertical split).
    pub top: bool,
    /// Allow dropping below (vertical split).
    pub bottom: bool,
    /// Allow turning into a floating window.
    pub float: bool,
    /// Allow dropping into the tab bar / merging tabs.
    pub tabs: bool,
}

impl AllowedDrops {
    /// All directions and float enabled.
    pub const fn all() -> Self {
        Self {
            left: true,
            right: true,
            top: true,
            bottom: true,
            float: true,
            tabs: true,
        }
    }

    /// Convert the enabled directions into [`crate::AllowedSplits`] shape.
    pub fn to_allowed_splits(&self) -> crate::AllowedSplits {
        let lr = self.left || self.right;
        let tb = self.top || self.bottom;
        match (lr, tb) {
            (true, true) => crate::AllowedSplits::All,
            (true, false) => crate::AllowedSplits::LeftRightOnly,
            (false, true) => crate::AllowedSplits::TopBottomOnly,
            (false, false) => crate::AllowedSplits::None,
        }
    }

    /// Check whether a specific split direction is allowed.
    pub fn split_allowed(&self, split: crate::Split) -> bool {
        match split {
            crate::Split::Left => self.left,
            crate::Split::Right => self.right,
            crate::Split::Above => self.top,
            crate::Split::Below => self.bottom,
        }
    }
}

impl Default for AllowedDrops {
    fn default() -> Self {
        Self::all()
    }
}
