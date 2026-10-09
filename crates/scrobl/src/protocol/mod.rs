//! The I/O-free protocol core: method descriptions, request construction,
//! signing, and response and error decoding.

mod method;
pub mod methods;

pub use method::{Auth, MethodSpec, Paging, ParamSpec, Requirement, Verb};
