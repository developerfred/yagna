/*
    The service that binds this payment driver into yagna via GSB.
*/

// Extrernal crates
use std::sync::Arc;

// Workspace uses
use ya_payment_driver::{
    bus,
    cron::Cron,
    dao::{init, DbExecutor},
    model::GenericError,
};
use ya_service_api_interfaces::Provider;

// Local uses
use crate::driver::ZksyncDriver;

pub struct ZksyncService;

impl ZksyncService {
    pub async fn gsb<Context: Provider<Self, DbExecutor>>(context: &Context) -> anyhow::Result<()> {
        log::debug!("Connecting ZksyncService to gsb...");

        // TODO: Read and validate env
        log::debug!("Environment variables validated");

        // Init database
        let db: DbExecutor = context.component();
        init(&db).await.map_err(GenericError::new)?;
        log::debug!("Database initialised");

        // Load driver
        let driver = ZksyncDriver::new(db.clone());
        driver.load_active_accounts().await;
        let driver_rc = Arc::new(driver);
        bus::bind_service(&db, driver_rc.clone()).await?;
        log::debug!("Driver loaded");

        // Start cron
        Cron::new(driver_rc.clone());
        log::debug!("Cron started");

        log::info!("Succesfully connected ZksyncService to gsb.");
        Ok(())
    }
}
