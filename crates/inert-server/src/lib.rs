//! # inert-server
//!
//! Server SDK for the Inert protocol.
//!
//! ```ignore
//! let mut app = App::new(Bundle::load("manifest.signed.cbor".as_ref())?, MyState::default());
//! app.screen("overview", |s, _| Ok(Data::new().set("balance", val::dec(12000, 2))));
//! app.action("transfer", |s, a| { s.transfer(a.choice("from"), a.decimal("amount"))?; Ok(Next::screen("overview")) });
//! serve(app.into_shared()?, "0.0.0.0:7443", tls).await
//! ```
//!
//! Handlers only ever see validated input, and every screen they produce is
//! checked against the signed template before it is sent.

pub mod app;
pub mod bundle;
pub mod dev;
pub mod json;
pub mod val;

pub use app::{App, Next, Reject, serve, session_over};
pub use bundle::Bundle;
pub use inert_proto as proto;
pub use val::{Data, Params};
