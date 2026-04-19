mod chat;
mod dispatch;
mod embeddings;
mod responses;
mod targeting;

#[cfg(test)]
mod tests;

pub use dispatch::{
    HostDispatchOutcome, HostDispatchTarget, HostProtocol, HostRequest,
    anthropic_requested_model_alias, dispatch_request,
};
