use crate::setup::RunConfig;
use crate::{appkey, utils::wait_for_yagna};
use anyhow::{bail, Context, Result};
use futures::{future::FutureExt, select};
use std::{io, process::Stdio};

use crate::utils::is_yagna_running;
use tokio::prelude::*;
use tokio::process::Command;

async fn reader_to_log<T: tokio::io::AsyncRead + Unpin>(name: String, reader: T) {
    let mut reader = tokio::io::BufReader::new(reader);
    let mut buf = Vec::new();
    loop {
        match reader.read_until(b'\n', &mut buf).await {
            Ok(len) => {
                if len > 0 {
                    eprintln!(
                        "{}: {}",
                        name,
                        String::from_utf8_lossy(&strip_ansi_escapes::strip(&buf).unwrap())
                            .trim_end()
                    );
                    buf.clear();
                } else {
                    break;
                }
            }
            Err(e) => {
                log::error!("{} output error: {}", name, e);
            }
        }
    }
}

fn spawn(name: &str, command: &mut Command) -> Result<tokio::process::Child> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .with_context(|| format!("Failed to spawn {}", name))?;

    // TODO: redirect output to files or something
    tokio::spawn(reader_to_log(
        format!("{} stdout", name),
        child.stdout.take().unwrap(),
    ));
    tokio::spawn(reader_to_log(
        format!("{} stderr", name),
        child.stderr.take().unwrap(),
    ));

    Ok(child)
}

fn handle_ctrl_c(result: io::Result<()>) -> Result</*exit code*/ i32> {
    if result.is_ok() {
        log::info!("Got ctrl+c. Bye!");
    }
    result.context("Couldn't listen to signals")?;
    Ok(0)
}

fn handle_subprocess(
    name: &str,
    result: io::Result<std::process::ExitStatus>,
) -> Result</*exit code*/ i32> {
    match result {
        Ok(exit_status) => {
            bail!("{} exited too early, {}", name, exit_status);
        }
        Err(e) => {
            bail!("Failed to spawn {}: {}", name, e);
        }
    }
}

pub async fn run(mut config: RunConfig) -> Result</*exit code*/ i32> {
    crate::setup::setup(&mut config, false).await?;
    if is_yagna_running().await? {
        bail!("service already running")
    }

    let service = spawn(
        "yagna service",
        Command::new("yagna").arg("service").arg("run"),
    )?;
    wait_for_yagna().await?;

    let app_key = appkey::get_app_key().await?;
    let mut provider_args = vec!["run", "--app-key", &app_key];
    if let Some(node_name) = config.node_name.as_ref() {
        provider_args.push("--node-name");
        provider_args.push(&node_name);
    }
    let provider = spawn(
        "ya-provider",
        Command::new("ya-provider").args(&provider_args),
    )?;

    let ctrl_c = tokio::signal::ctrl_c();

    log::info!("Golem provider is running");
    select!(
        result = ctrl_c.fuse() => handle_ctrl_c(result),
        result = service.fuse() => handle_subprocess("yagna service", result),
        result = provider.fuse() => handle_subprocess("ya-provider", result),
    )
}
