//! Top-level lib.rs for `cranelift_module`.

#![deny(missing_docs, trivial_numeric_casts, unused_extern_crates)]
#![warn(unused_import_braces)]
#![cfg_attr(feature = "std", deny(unstable_features))]
#![cfg_attr(feature = "clippy", plugin(clippy(conf_file = "../../clippy.toml")))]
#![cfg_attr(feature = "cargo-clippy", allow(clippy::new_without_default))]
#![cfg_attr(
    feature = "cargo-clippy",
    warn(
        clippy::float_arithmetic,
        clippy::mut_mut,
        clippy::nonminimal_bool,
        clippy::option_map_unwrap_or,
        clippy::option_map_unwrap_or_else,
        clippy::print_stdout,
        clippy::unicode_not_nfc,
        clippy::use_self
    )
)]
#![no_std]
#![cfg_attr(not(feature = "std"), feature(alloc))]

#[cfg(not(feature = "std"))]
#[macro_use]
extern crate alloc as std;
#[cfg(feature = "std")]
#[macro_use]
extern crate std;

#[cfg(not(feature = "std"))]
use hashmap_core::{map as hash_map, HashMap};
#[cfg(feature = "std")]
use std::collections::{hash_map, HashMap};

mod backend;
mod data_context;
mod module;

pub use crate::backend::Backend;
pub use crate::data_context::{DataContext, DataDescription, Init};
pub use crate::module::{
    DataId, FuncId, FuncOrDataId, Linkage, Module, ModuleError, ModuleNamespace, ModuleResult,
};

/// Version number of this crate.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
