#![no_std]

pub mod models;
pub mod network;

#[cfg(feature = "embedded")]
pub mod app;
#[cfg(feature = "embedded")]
pub mod config;
