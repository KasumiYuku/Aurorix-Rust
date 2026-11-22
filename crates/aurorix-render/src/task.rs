use std::time::Duration;

use bytes::Bytes;

use crate::context::RenderContext;
use crate::error::RenderError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum RenderPriority {
    Low,
    #[default]
    Normal,
    High,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RenderTask {
    pub name: String,
    pub priority: RenderPriority,
    pub estimated_bytes: u64,
}

impl RenderTask {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            priority: RenderPriority::default(),
            estimated_bytes: 0,
        }
    }

    pub fn with_priority(mut self, priority: RenderPriority) -> Self {
        self.priority = priority;
        self
    }

    pub fn with_estimated_bytes(mut self, estimated_bytes: u64) -> Self {
        self.estimated_bytes = estimated_bytes;
        self
    }

    pub fn name(&self) -> &str {
        &self.name
    }
}

impl Default for RenderTask {
    fn default() -> Self {
        Self::new("render")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RenderTicket {
    id: u64,
}

impl RenderTicket {
    pub fn new(id: u64) -> Self {
        Self { id }
    }

    pub fn id(&self) -> u64 {
        self.id
    }
}

#[derive(Debug)]
pub struct RenderOutput {
    pub ticket: u64,
    pub bytes: Bytes,
    pub elapsed: Duration,
}

pub type RenderJob = Box<dyn FnOnce(&RenderContext) -> Result<Bytes, RenderError> + Send>;
