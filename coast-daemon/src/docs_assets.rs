use rust_embed::Embed;

/// Embedded markdown docs source tree.
#[derive(Embed)]
#[folder = "../docs/"]
#[prefix = ""]
pub struct DocsAssets;

/// Embedded per-locale docs search indexes for daemon/CLI usage.
#[derive(Embed)]
#[folder = "../search-indexes/"]
#[prefix = ""]
pub struct SearchIndexAssets;
