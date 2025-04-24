#![no_std]
#![feature(impl_trait_in_assoc_type)]

pub mod models;

#[cfg(feature = "embedded")]
pub mod config;

#[cfg(feature = "embedded")]
pub mod tasks;

#[cfg(feature = "embedded")]
pub mod network;
