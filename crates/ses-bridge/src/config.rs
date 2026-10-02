use std::env;

use lambda_runtime::Error;

/// Runtime configuration for the bridge Lambda. Everything comes from
/// environment variables so the function image stays generic.
#[derive(Clone, Debug)]
pub struct Config {
    /// Base URL of the Cloudflare bridge Worker, e.g. `https://bridge.example.workers.dev`.
    pub bridge_url: String,
    /// Shared secret used for the HMAC-signed bridge requests.
    pub hmac_secret: String,
    /// Key prefix required for objects created in R2.
    pub r2_key_prefix: String,
    /// Lifetime of the presigned R2 PUT URL, in seconds.
    pub presign_ttl_seconds: u64,
}

impl Config {
    pub fn from_env() -> Result<Self, Error> {
        let bridge_url = required("BRIDGE_URL")?.trim_end_matches('/').to_string();
        let hmac_secret = required("BRIDGE_HMAC_SECRET")?;
        let r2_key_prefix = env::var("R2_KEY_PREFIX").unwrap_or_else(|_| "inbound".to_string());
        let presign_ttl_seconds = env::var("PRESIGN_TTL_SECONDS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(3600);

        Ok(Self {
            bridge_url,
            hmac_secret,
            r2_key_prefix,
            presign_ttl_seconds,
        })
    }
}

fn required(name: &str) -> Result<String, Error> {
    env::var(name).map_err(|_| format!("missing required environment variable {name}").into())
}
