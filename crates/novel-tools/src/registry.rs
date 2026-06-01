use crate::{Tool, ToolError};
use std::collections::HashMap;

pub struct ToolRegistry {
    tools: HashMap<String, Box<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    pub fn register(&mut self, tool: Box<dyn Tool>) {
        self.tools.insert(tool.name().to_string(), tool);
    }

    pub fn get(&self, name: &str) -> Option<&dyn Tool> {
        self.tools.get(name).map(|t| t.as_ref())
    }

    pub fn names(&self) -> Vec<String> {
        let mut n: Vec<_> = self.tools.keys().cloned().collect();
        n.sort();
        n
    }

    pub fn resolve(&self, name: &str) -> Result<&dyn Tool, ToolError> {
        self.get(name)
            .ok_or_else(|| ToolError::UnknownTool(name.into()))
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use crate::default_registry;

    #[test]
    fn default_registry_has_novel_tools() {
        let reg = default_registry();
        assert!(reg.get("CharacterSearch").is_some());
        assert!(reg.get("PlotGraph").is_some());
        assert!(reg.get("TrackingQuery").is_some());
    }
}
