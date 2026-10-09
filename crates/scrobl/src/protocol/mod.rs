//! The I/O-free protocol core: method descriptions, request construction,
//! signing, and response and error decoding.

mod method;
pub mod methods;
mod request;
mod response;
mod sign;
#[cfg(test)]
pub(crate) mod testing;

pub use method::{Auth, MethodSpec, Paging, ParamSpec, Requirement, Verb};
#[doc(hidden)]
pub use request::prepare_with_root;
pub use request::{Credentials, HttpRequest, ParamValue, Request, prepare};
pub use response::{HttpResponse, Raw, decode};
