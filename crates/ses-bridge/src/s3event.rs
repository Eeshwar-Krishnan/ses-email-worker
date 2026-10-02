use serde::Deserialize;

/// Minimal shape of an S3 `ObjectCreated` notification delivered to Lambda.
#[derive(Deserialize, Debug)]
pub struct S3Event {
    #[serde(rename = "Records", default)]
    pub records: Vec<S3EventRecord>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct S3EventRecord {
    pub event_name: String,
    pub aws_region: String,
    pub event_time: String,
    pub s3: S3Entity,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct S3Entity {
    pub bucket: S3Bucket,
    pub object: S3Object,
}

#[derive(Deserialize, Debug)]
pub struct S3Bucket {
    pub name: String,
}

#[derive(Deserialize, Debug)]
pub struct S3Object {
    pub key: String,
    pub size: Option<i64>,
}

impl S3EventRecord {
    /// S3 URL-encodes object keys in event notifications, using `+` for spaces.
    pub fn key(&self) -> String {
        decode_key(&self.s3.object.key)
    }
}

pub fn decode_key(key: &str) -> String {
    // S3 uses form encoding for keys, so `+` means space rather than a literal plus.
    let normalized = key.replace('+', "%20");
    percent_decode(&normalized)
}

/// Decodes `%XX` sequences without pulling in a full URL crate.
fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hi = (bytes[i + 1] as char).to_digit(16);
            let lo = (bytes[i + 2] as char).to_digit(16);
            if let (Some(hi), Some(lo)) = (hi, lo) {
                out.push((hi * 16 + lo) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }

    String::from_utf8_lossy(&out).into_owned()
}
