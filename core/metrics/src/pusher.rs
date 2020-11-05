use actix_web::client::Client;
use std::time::Duration;
use tokio::time;

use ya_core_model::identity::{self, IdentityInfo};
use ya_service_api::MetricsCtx;
use ya_service_bus::typed as bus;

pub fn spawn(ctx: MetricsCtx) {
    if !ctx.push_enabled {
        log::info!("Metrics pusher disabled");
        return;
    }

    log::debug!("Starting metrics pusher");
    tokio::task::spawn_local(async move {
        push_forever(ctx.push_host_url.unwrap().as_str()).await;
    });
    log::info!("Metrics pusher started");
}

pub async fn push_forever(host_url: &str) {
    let node_identity = match try_get_default_id().await {
        Ok(default_id) => default_id,
        Err(e) => {
            log::warn!("Couldn't determine node_id. Giving up. Err({})", e);
            return;
        }
    };
    let push_url = get_push_url(host_url, node_identity).await.unwrap();

    let mut push_interval = time::interval(Duration::from_secs(5));
    let client = Client::build().timeout(Duration::from_secs(4)).finish();
    log::info!("Starting metrics pusher");
    loop {
        push_interval.tick().await;
        push(&client, push_url.clone()).await;
    }
}

pub async fn push(client: &Client, push_url: String) {
    let current_metrics = crate::service::export_metrics().await;
    let res = client
        .put(push_url.as_str())
        .send_body(current_metrics)
        .await;
    log::trace!("Pushed current metrics {:#?}", res);
}

async fn get_default_id() -> anyhow::Result<IdentityInfo> {
    let default_id = bus::service(identity::BUS_ID)
        .call(identity::Get::ByDefault)
        .await??
        .ok_or(anyhow::anyhow!("Default identity not found"))?;
    Ok(default_id)
}

async fn try_get_default_id() -> anyhow::Result<IdentityInfo> {
    let mut interval = time::interval(Duration::from_secs(10));
    let mut last_error = None;
    for _ in 0..3 {
        interval.tick().await;
        match get_default_id().await {
            Ok(default_id) => return Ok(default_id),
            Err(e) => {
                log::debug!("Couldn't determine node_id. {:?}", e);
                last_error = Some(e);
            }
        }
    }
    Err(last_error.unwrap_or(anyhow::anyhow!("Undefined error")))
}

async fn get_push_url(host_url: &str, id: IdentityInfo) -> anyhow::Result<String> {
    let base = url::Url::parse(host_url)?;
    let url = base
        .join("/metrics/job/community.1/")?
        .join(&format!("instance/{}/", id.node_id))?
        .join(&format!(
            "hostname/{}",
            id.alias.unwrap_or(id.node_id.to_string())
        ))?;
    Ok(String::from(url.as_str()))
}
