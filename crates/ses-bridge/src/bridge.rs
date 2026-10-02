use std::time::{SystemTime, UNIX_EPOCH};

use lambda_runtime::Error;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tracing::{info, warn};

use crate::config::Config;
use crate::hmac;
use crate::mime;
use crate::s3event::S3EventRecord;

const CONTENT_TYPE: &str = "message/rfc822";

pub struct Bridge {
    config: Config,
    s3: aws_sdk_s3::Client,
    http: reqwest::Client,
}

impl Bridge {
    pub async fn new(config: Config) -> Result<Self, Error> {
        let aws_config = aws_config::defaults(aws_config::BehaviorVersion::latest())
            .load()
            .await;
        let s3 = aws_sdk_s3::Client::new(&aws_config);
        let http = reqwest::Client::builder()
            .user_agent(concat!("ses-bridge/", env!("CARGO_PKG_VERSION")))
            .build()?;

        Ok(Self { config, s3, http })
    }

    /// Reads one object from S3, mirrors it to R2, removes the S3 copy and
    /// enqueues the metadata on Cloudflare Queues.
    pub async fn process(&self, record: &S3EventRecord) -> Result<(), Error> {
        let bucket = record.s3.bucket.name.clone();
        let key = record.key();

        let raw = match self.fetch_object(&bucket, &key).await? {
            Some(raw) => raw,
            // Already processed by an earlier (retried) invocation.
            None => return Ok(()),
        };

        let parsed = mime::parse(&raw);
        let recipients = parsed.recipients();
        let size = raw.len() as i64;
        let sha256 = hex::encode(Sha256::digest(&raw));
        let r2_key = self.r2_key(record, &key);
        let received_at = iso8601(now_secs());

        info!(
            message_id = %key,
            bytes = raw.len(),
            r2_key = %r2_key,
            recipients = ?parsed.recipients(),
            "mirroring inbound email"
        );

        let presign: PresignResponse = self
            .signed_post(
                "/presign",
                &PresignRequest {
                    key: r2_key.clone(),
                    content_type: CONTENT_TYPE.to_string(),
                    content_length: raw.len() as u64,
                    expires_in: self.config.presign_ttl_seconds,
                },
            )
            .await?;

        self.put_r2(&presign.url, raw).await?;

        self.delete_object(&bucket, &key).await?;

        self.signed_post::<_, serde_json::Value>(
            "/enqueue",
            &EnqueueRequest {
                message_id: key.clone(),
                r2_key,
                s3_bucket: bucket,
                s3_key: key,
                aws_region: record.aws_region.clone(),
                from: parsed.from,
                recipients,
                to: parsed.to,
                cc: parsed.cc,
                subject: parsed.subject,
                date: parsed.date,
                header_message_id: parsed.header_message_id,
                size: record.s3.object.size.unwrap_or(size),
                sha256,
                received_at,
            },
        )
        .await?;

        Ok(())
    }

    async fn fetch_object(&self, bucket: &str, key: &str) -> Result<Option<Vec<u8>>, Error> {
        match self.s3.get_object().bucket(bucket).key(key).send().await {
            Ok(output) => {
                let bytes = output.body.collect().await?.into_bytes();
                Ok(Some(bytes.to_vec()))
            }
            Err(err) => {
                let missing = err
                    .as_service_error()
                    .map(|service| service.is_no_such_key())
                    .unwrap_or(false);
                if missing {
                    warn!(bucket, key, "S3 object already gone, skipping");
                    Ok(None)
                } else {
                    Err(err.into())
                }
            }
        }
    }

    async fn delete_object(&self, bucket: &str, key: &str) -> Result<(), Error> {
        self.s3
            .delete_object()
            .bucket(bucket)
            .key(key)
            .send()
            .await?;
        Ok(())
    }

    async fn put_r2(&self, url: &str, raw: Vec<u8>) -> Result<(), Error> {
        let response = self
            .http
            .put(url)
            .header(reqwest::header::CONTENT_TYPE, CONTENT_TYPE)
            .body(raw)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(format!("R2 upload failed with {status}: {body}").into());
        }

        Ok(())
    }

    async fn signed_post<B: Serialize, R: DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<R, Error> {
        let body = serde_json::to_vec(body)?;
        let signature = hmac::sign(&self.config.hmac_secret, "POST", path, &body);
        let url = format!("{}{}", self.config.bridge_url, path);

        let response = self
            .http
            .post(url)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .header("x-timestamp", signature.timestamp)
            .header("x-nonce", signature.nonce)
            .header("x-signature", signature.signature)
            .body(body)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(format!("bridge request {path} failed with {status}: {text}").into());
        }

        Ok(response.json::<R>().await?)
    }

    fn r2_key(&self, record: &S3EventRecord, message_id: &str) -> String {
        format!(
            "{}/{}/{}",
            self.config.r2_key_prefix,
            date_path(&record.event_time),
            message_id
        )
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PresignRequest {
    key: String,
    content_type: String,
    content_length: u64,
    expires_in: u64,
}

#[derive(Deserialize)]
struct PresignResponse {
    url: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct EnqueueRequest {
    message_id: String,
    r2_key: String,
    s3_bucket: String,
    s3_key: String,
    aws_region: String,
    from: Option<String>,
    recipients: Vec<String>,
    to: Vec<String>,
    cc: Vec<String>,
    subject: Option<String>,
    date: Option<String>,
    header_message_id: Option<String>,
    size: i64,
    sha256: String,
    received_at: String,
}

/// Turns `2026-10-02T...` into `2026/10/02`, falling back to `unknown`.
fn date_path(event_time: &str) -> String {
    let date = event_time.get(0..10).unwrap_or_default();
    if date.len() == 10 && date.as_bytes()[4] == b'-' && date.as_bytes()[7] == b'-' {
        date.replace('-', "/")
    } else {
        "unknown".to_string()
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before unix epoch")
        .as_secs()
}

/// Minimal RFC 3339 timestamp (UTC) without pulling in a date library.
fn iso8601(secs: u64) -> String {
    let days = secs / 86_400;
    let secs_of_day = secs % 86_400;
    let (hour, minute, second) = (
        secs_of_day / 3600,
        (secs_of_day % 3600) / 60,
        secs_of_day % 60,
    );
    let (year, month, day) = civil_from_days(days as i64);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

/// Howard Hinnant's days-from-civil algorithm, inverted.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_epoch() {
        assert_eq!(iso8601(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso8601(1_700_000_000), "2023-11-14T22:13:20Z");
    }

    #[test]
    fn builds_date_path() {
        assert_eq!(date_path("2026-10-02T12:00:00.000Z"), "2026/10/02");
        assert_eq!(date_path("garbage"), "unknown");
    }
}
