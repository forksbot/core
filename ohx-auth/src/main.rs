#![feature(associated_type_defaults)]

mod http;
mod auth_config;
mod user_store;
mod refresh_tokens;

use env_logger::{Env, TimestampPrecision, DEFAULT_FILTER_ENV};
use std::path::Path;
use structopt::StructOpt;
use log::{info, error};
use snafu::Error;

use libohxcore::{common_config, wait_until_known_time, shutdown_on_ctrl_c, key_filename, cert_filename, FileFormat, check_dir_access};
use http::service::HttpService;

mod create_system_auth_key;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Logging
    let mut builder = env_logger::Builder::from_env(Env::new().filter_or(DEFAULT_FILTER_ENV, "info"));
    builder
        .format_timestamp(Some(TimestampPrecision::Seconds))
        .format_module_path(false)
        .init();

    // Command line / environment / file configuration
    let config: auth_config::Config = auth_config::Config::from_args();
    let (shutdown_tx, mut shutdown_rx) = tokio::sync::mpsc::channel(1);

    create_root_directory(&config.common)?;
    check_dir_access(&config.common.get_certs_directory())?;
    wait_until_known_time(false).await?;

    let (cycle_fut, mut jwt_cycle_shutdown) = create_system_auth_key::CycleKey::new(config.common.get_certs_directory());
    tokio::spawn(async move { cycle_fut.run().await; });

    shutdown_on_ctrl_c(shutdown_tx.clone());

    let mut http_service = HttpService::new(config.common.get_root_directory(),
                                            key_filename(&config.common.get_certs_directory(), FileFormat::PEM),
                                            cert_filename(&config.common.get_certs_directory(), FileFormat::PEM));

    let http_shutdown = http_service.control();
    let mut shutdown = tokio::spawn(async move {
        let _ = shutdown_rx.recv().await;
        http_shutdown.shutdown().await;
        let _ = jwt_cycle_shutdown.send(()).await;
    });

    if let Err(e) = http_service.run().await {
        error!("{}", e);
    }

    let _ = shutdown.await;
    Ok(())
}

/// Creates all OHX root directory subdirectories required to run the OHX core service
fn create_root_directory(common_config: &common_config::Config) -> Result<(), std::io::Error> {
    let path = common_config.get_root_directory();
    if !common_config.create_root && !path.exists() {
        return Err(std::io::Error::new(std::io::ErrorKind::NotFound, "OHX Root directory does not exist. Consider using --create-root").into());
    }

    std::fs::create_dir_all(path.join("certs"))?;
    std::fs::create_dir_all(path.join("config"))?;
    Ok(())
}
