//! Where a package stands against the release, in one word, and the
//! sentence that says it plainly. Every view marks a package the same way.

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Standing {
    /// A build that is not the release's, from a commit no remote has, or
    /// from uncommitted changes: the only copy of that work is here.
    NotPushed,
    /// The newest release moves it.
    Update,
    /// A build that is not the release's, from work that is on a remote
    /// (or that cannot be checked).
    Local,
    /// On the origin, not in this machine's catalog.
    OriginOnly,
    /// In the catalog, not linked.
    Off,
    /// The release's build, or one this machine added that no release
    /// carries, and nothing newer is known.
    Current,
}

impl Standing {
    pub const ALL: [Standing; 6] = [
        Standing::NotPushed,
        Standing::Update,
        Standing::Local,
        Standing::OriginOnly,
        Standing::Off,
        Standing::Current,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Standing::Current => "Current",
            Standing::Update => "Update",
            Standing::Local => "Local build",
            Standing::NotPushed => "Not pushed",
            Standing::Off => "Off",
            Standing::OriginOnly => "On origin only",
        }
    }

    /// The plural for a list's heading.
    pub fn heading(self) -> &'static str {
        match self {
            Standing::Current => "Current",
            Standing::Update => "Updates",
            Standing::Local => "Local builds",
            Standing::NotPushed => "Not pushed",
            Standing::Off => "Off",
            Standing::OriginOnly => "On origin only",
        }
    }
}
