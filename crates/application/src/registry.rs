//! The single registry of tools. The UI and CLI read it and never hardcode tool lists.

use domain::ToolId;
use serde::Serialize;
use std::collections::BTreeMap;
use std::sync::Arc;

use crate::error::AppError;
use crate::tool::Tool;

/// What the UI needs to show one tool and build its option form.
#[derive(Debug, Clone, Serialize)]
pub struct ToolDescriptor {
    /// Tool id.
    pub id: ToolId,
    /// Label used in names, for example `rotated`.
    pub label: String,
    /// JSON Schema of the options.
    pub options_schema: schemars::Schema,
}

/// Holds every registered tool.
#[derive(Default)]
pub struct ToolRegistry {
    tools: BTreeMap<ToolId, Arc<dyn Tool>>,
}

impl ToolRegistry {
    /// Creates an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a tool. Registering the same id twice is a programming error and is reported.
    pub fn register(&mut self, tool: Arc<dyn Tool>) -> Result<(), AppError> {
        let id = tool.id();
        if self.tools.contains_key(&id) {
            return Err(AppError::internal(&format!(
                "registering the {id} tool twice"
            )));
        }
        self.tools.insert(id, tool);
        Ok(())
    }

    /// Looks up a tool.
    pub fn get(&self, id: ToolId) -> Option<&Arc<dyn Tool>> {
        self.tools.get(&id)
    }

    /// Describes every tool in id order.
    pub fn list(&self) -> Vec<ToolDescriptor> {
        self.tools
            .values()
            .map(|t| ToolDescriptor {
                id: t.id(),
                label: t.label().to_owned(),
                options_schema: t.options_schema(),
            })
            .collect()
    }

    /// Number of registered tools.
    pub fn len(&self) -> usize {
        self.tools.len()
    }

    /// True when no tool is registered.
    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::RotateLikeTool;

    #[test]
    fn registers_lists_and_rejects_duplicates() {
        let mut reg = ToolRegistry::new();
        assert!(reg.is_empty());
        reg.register(Arc::new(RotateLikeTool)).unwrap();
        assert_eq!(reg.len(), 1);
        assert!(reg.get(ToolId::Rotate).is_some());
        let listed = reg.list();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, ToolId::Rotate);
        assert!(reg.register(Arc::new(RotateLikeTool)).is_err());
    }

    #[test]
    fn descriptors_serialize_with_a_schema() {
        let mut reg = ToolRegistry::new();
        reg.register(Arc::new(RotateLikeTool)).unwrap();
        let json = serde_json::to_value(reg.list()).unwrap();
        assert_eq!(json[0]["id"], "rotate");
        assert!(json[0]["options_schema"].is_object());
    }
}
