#![no_std]

#[cfg(feature = "alloc")]
extern crate alloc;

pub mod fields;
#[cfg(feature = "alloc")]
pub mod models;
pub mod routes;
pub mod status;
