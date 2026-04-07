//! Ecosystem registry client for Open Harness
//!
//! This crate provides functionality for interacting with Open Harness
//! ecosystem registries, enabling discovery, search, and metadata
//! retrieval of MCP servers, skills, and plugins.

#![forbid(unsafe_code)]
#![deny(unused_must_use)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::dbg_macro)]
#![deny(clippy::todo)]

pub mod cache;
pub mod client;
pub mod search;
pub mod types;

pub use cache::RegistryCache;
pub use client::{MultiRegistryClient, RegistryClient};
pub use search::{SearchQuery, SortField, SortOrder};
pub use types::SearchResult;
pub use types::{
    ComponentDependency, ComponentMetadata, ComponentType, PackageInfo, RegistryConfig,
    RegistryError, RegistryResult,
};
