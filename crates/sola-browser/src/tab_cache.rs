//! Parked profile workspace (chrome side).
//!
//! Switching profiles **parks** the whole workspace (helper browsers +
//! chrome snapshot) so returning does not reload pages. A park lives as
//! long as the window, like the live strip: nothing is evicted for idle
//! time or tab count. Only deleting the profile drops it.

use crate::engine::{TabId, TabInfo};

/// Chrome-side snapshot of a profile workspace (tab strip + focus).
#[derive(Debug, Clone)]
pub struct WorkspaceSnapshot {
    pub tabs: Vec<TabInfo>,
    pub active: TabId,
    pub sidebar_w: f32,
    pub groups: crate::groups::Groups,
    pub recently_closed: Vec<crate::session::ClosedTab>,
}
