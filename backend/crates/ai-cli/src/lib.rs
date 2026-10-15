pub mod engine;
pub mod cli;
pub mod call;
pub mod registry;
pub mod adapters;

#[cfg(any(test, feature = "mock"))]
pub mod mock;

pub use engine::{
    AiEngine, AiError, AiInvocation, AiPurpose, AiRawResult, Capabilities, RunFile, TokenUsage,
    VisionMode,
};
pub use cli::{which, CliAdapter, CliEngine, CommandSpec, StreamItem};
pub use call::{call_json, CallRecord, CallSpec};
pub use registry::{BackendConfig, Registry};

#[cfg(any(test, feature = "mock"))]
pub use mock::MockEngine;
