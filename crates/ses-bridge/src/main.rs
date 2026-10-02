mod bridge;
mod config;
mod hmac;
mod mime;
mod s3event;

use std::sync::Arc;

use lambda_runtime::{run, service_fn, Error, LambdaEvent};
use serde_json::{json, Value};

use crate::bridge::Bridge;
use crate::config::Config;
use crate::s3event::S3Event;

struct App {
    bridge: Bridge,
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .without_time()
        .init();

    let config = Config::from_env()?;
    let app = Arc::new(App {
        bridge: Bridge::new(config).await?,
    });

    run(service_fn(move |event: LambdaEvent<S3Event>| {
        let app = Arc::clone(&app);
        async move { handle(event, app).await }
    }))
    .await
}

async fn handle(event: LambdaEvent<S3Event>, app: Arc<App>) -> Result<Value, Error> {
    let (event, _context) = event.into_parts();
    let mut processed = 0usize;

    for record in &event.records {
        // Ignore S3 test notifications and non-create events.
        if !record.event_name.starts_with("ObjectCreated:") {
            continue;
        }
        app.bridge.process(record).await?;
        processed += 1;
    }

    Ok(json!({ "processed": processed }))
}
