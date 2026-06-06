use anyhow::{Context, Result};
use aws_config::BehaviorVersion;
use aws_credential_types::Credentials;
use aws_sdk_s3::{config::Region, primitives::ByteStream};
use camino::Utf8Path;

use crate::{config::StorageConfig, secret};

pub struct Client {
    inner: aws_sdk_s3::Client,
    bucket: String,
}

impl Client {
    pub async fn new(config: StorageConfig, credentials: secret::Credentials) -> Result<Self> {
        let sdk_credentials = Credentials::new(
            credentials.access_key_id,
            credentials.secret_access_key,
            None,
            None,
            "filelift",
        );

        let sdk_config = aws_config::defaults(BehaviorVersion::latest())
            .credentials_provider(sdk_credentials)
            .region(Region::new(config.region))
            .endpoint_url(config.endpoint)
            .load()
            .await;

        let inner = aws_sdk_s3::Client::new(&sdk_config);

        Ok(Self {
            inner,
            bucket: config.bucket,
        })
    }

    pub async fn upload_file(&self, path: &Utf8Path, key: &str) -> Result<()> {
        let body = ByteStream::from_path(path.as_std_path())
            .await
            .with_context(|| format!("failed to read {path}"))?;
        let content_type = mime_guess::from_path(path.as_std_path()).first_or_octet_stream();

        self.inner
            .put_object()
            .bucket(&self.bucket)
            .key(key)
            .content_type(content_type.essence_str())
            .body(body)
            .send()
            .await
            .with_context(|| format!("failed to upload {path} to s3 key {key}"))?;

        Ok(())
    }
}
