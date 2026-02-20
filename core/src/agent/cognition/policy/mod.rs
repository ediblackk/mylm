//! Policy modules for kernel behavior

pub mod approval;

pub use approval::{ApprovalPolicy, requires_approval};
