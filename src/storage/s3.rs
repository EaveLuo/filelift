use anyhow::{Context, Result};
use aws_sdk_s3::{
    config::{BehaviorVersion, Credentials, Region},
    primitives::ByteStream,
};
use camino::Utf8Path;

use crate::{secret, target::UploadTarget};

pub struct Client {
    inner: aws_sdk_s3::Client,
    bucket: String,
}

impl Client {
    pub async fn new(target: UploadTarget, credentials: secret::Credentials) -> Result<Self> {
        let sdk_credentials = Credentials::new(
            credentials.access_key_id,
            credentials.secret_access_key,
            None,
            None,
            "filelift",
        );

        let sdk_config = aws_sdk_s3::Config::builder()
            .behavior_version(BehaviorVersion::latest())
            .credentials_provider(sdk_credentials)
            .region(Region::new(target.region))
            .endpoint_url(target.endpoint)
            .build();

        let inner = aws_sdk_s3::Client::from_conf(sdk_config);

        Ok(Self {
            inner,
            bucket: target.bucket,
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

    pub async fn check_connectivity(&self) -> Result<()> {
        self.inner
            .head_bucket()
            .bucket(&self.bucket)
            .send()
            .await
            .context("target connectivity check failed")?;

        Ok(())
    }
}
