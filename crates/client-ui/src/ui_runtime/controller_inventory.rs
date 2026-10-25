//! Controller container navigation and semantic actions, independent of hardware backends.
//!
//! Navigation tokens identify a target; action admission still belongs to the
//! inventory ledger and the current screen's authority.

pub mod actions;
mod lifetime;
pub mod navigation;
pub mod session;
