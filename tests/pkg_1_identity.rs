
#[path = "common/mod.rs"]
mod common;

#[path = "pkg_1_identity/test_token_issuance.rs"]
pub mod test_token_issuance;

// Placeholder for future tests (commented out until files exist to avoid compilation errors)
#[path = "pkg_1_identity/test_agent_update.rs"]
pub mod test_agent_update;
#[path = "pkg_1_identity/test_metadata_ops.rs"]
mod test_metadata_ops;
#[path = "pkg_1_identity/test_registration.rs"]
pub mod test_registration;
// #[path = "pkg_1_identity/test_metadata_ops.rs"]
// pub mod test_metadata_ops;
#[path = "pkg_1_identity/test_service_configs.rs"]
pub mod test_service_configs;
#[path = "pkg_1_identity/test_views.rs"]
pub mod test_views;
// #[path = "pkg_1_identity/test_views.rs"]
// pub mod test_views;
#[path = "pkg_1_identity/test_error_paths.rs"]
pub mod test_error_paths;
