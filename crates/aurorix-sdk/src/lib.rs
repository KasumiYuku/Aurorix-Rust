pub mod context;
pub mod error;
pub mod facade;
pub mod plugin;
pub mod registry;

pub use context::{extract, Context, Extract};
pub use error::{SdkError, SdkResult};
pub use facade::{keyboard, render, render_for};
pub use plugin::{Plugin, PluginMeta};
pub use registry::PluginRegistry;

pub use aurorix_buttons::Builder;
pub use aurorix_core::{Scene, Target};
pub use aurorix_protocol;
pub use aurorix_template::{Args, Registry};
