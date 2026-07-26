#![deny(clippy::unwrap_used)]
#![cfg_attr(test, allow(clippy::unwrap_used))]
#![cfg_attr(test, allow(clippy::expect_used))]

mod error;
mod loader;
mod merger;

pub use error::SkillError;
pub use loader::{
    load_skill, load_skills_dir, load_skills_merged, resolve_skill_md, SkillDefinition,
};
pub use merger::format_skill_listing_description;
