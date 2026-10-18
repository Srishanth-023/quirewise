//! Tool implementations. Each tool depends on ports only and is registered here, once.

use crate::registry::ToolRegistry;

/// Registers every tool. The worker and the CLI both call this so they never disagree.
///
/// Tools arrive phase by phase (MASTER_PROMPT.md section 15); Phase 1 has none yet.
pub fn register_all(_registry: &mut ToolRegistry) {}
