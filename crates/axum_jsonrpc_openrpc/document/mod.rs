//! OpenRPC document model + generation.

mod build;
mod examples;
mod schema;
mod types;

#[cfg(test)]
mod tests;

pub use build::build_document;
pub use types::{
    OpenRpcComponents, OpenRpcContent, OpenRpcDocument, OpenRpcError, OpenRpcExample,
    OpenRpcExampleParam, OpenRpcExampleResult, OpenRpcInfo, OpenRpcMethod, OpenRpcServer,
    OpenRpcTag,
};
