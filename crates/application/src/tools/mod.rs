//! Tool implementations. Each tool depends on ports only and is registered here, once.

mod combine;
mod common;
mod document;
mod insert;
mod optimize;
mod rotate;
mod secure;
mod select;
mod split;

pub use common::{ToolDeps, pages_label};

use std::sync::Arc;

use crate::registry::ToolRegistry;

/// Registers every tool. The worker and the CLI both call this so they never disagree.
///
/// A duplicate id is a programming error that the registry reports; here it would mean two tools
/// claim one id, so it is surfaced to the caller.
pub fn register_all(
    registry: &mut ToolRegistry,
    deps: &ToolDeps,
) -> Result<(), crate::error::AppError> {
    registry.register(Arc::new(combine::MergeTool::new(deps.clone())))?;
    registry.register(Arc::new(combine::AlternateMixTool::new(deps.clone())))?;
    registry.register(Arc::new(combine::OrganizeTool::new(deps.clone())))?;
    registry.register(Arc::new(select::SplitRangesTool::new(deps.clone())))?;
    registry.register(Arc::new(select::SplitEveryNTool::new(deps.clone())))?;
    registry.register(Arc::new(select::ExtractPagesTool::new(deps.clone())))?;
    registry.register(Arc::new(select::RemovePagesTool::new(deps.clone())))?;
    registry.register(Arc::new(split::SplitByBookmarksTool::new(deps.clone())))?;
    registry.register(Arc::new(split::SplitInHalfTool::new(deps.clone())))?;
    registry.register(Arc::new(split::SplitBySizeTool::new(deps.clone())))?;
    registry.register(Arc::new(split::SplitByTextTool::new(deps.clone())))?;
    registry.register(Arc::new(rotate::RotateTool::new(deps.clone())))?;
    registry.register(Arc::new(insert::InsertPagesTool::new(deps.clone())))?;
    registry.register(Arc::new(optimize::CompressTool::new(deps.clone())))?;
    registry.register(Arc::new(optimize::RepairTool::new(deps.clone())))?;
    registry.register(Arc::new(optimize::LinearizeTool::new(deps.clone())))?;
    registry.register(Arc::new(document::RemoveAnnotationsTool::new(deps.clone())))?;
    registry.register(Arc::new(document::EditMetadataTool::new(deps.clone())))?;
    registry.register(Arc::new(document::FlattenTool::new(deps.clone())))?;
    registry.register(Arc::new(secure::ProtectTool::new(deps.clone())))?;
    registry.register(Arc::new(secure::UnlockTool::new(deps.clone())))?;
    registry.register(Arc::new(secure::SanitizeTool::new(deps.clone())))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::ToolId;

    #[test]
    fn every_known_tool_id_is_registered_with_matching_schema() {
        let mut registry = ToolRegistry::new();
        register_all(&mut registry, &ToolDeps::unavailable()).unwrap();
        assert_eq!(registry.len(), ToolId::ALL.len());
        for id in ToolId::ALL {
            let tool = registry
                .get(id)
                .unwrap_or_else(|| panic!("{id} is not registered"));
            assert_eq!(tool.id(), id);
            assert!(
                tool.options_schema().as_value().is_object(),
                "{id} has a schema"
            );
        }
    }

    #[test]
    fn default_options_of_every_tool_pass_their_own_schema_round_trip() {
        for id in ToolId::ALL {
            let opts = domain::ToolOptions::defaults_for(id);
            let json = serde_json::to_value(&opts).unwrap();
            let back: domain::ToolOptions = serde_json::from_value(json).unwrap();
            assert_eq!(back, opts, "{id}");
        }
    }
}
