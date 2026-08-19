use anyhow::{Context, Result};
use aws_sdk_s3::{config::Credentials, primitives::ByteStream, Client};
use config::Settings;
use sha2::{Digest, Sha256};

#[derive(Clone)]
pub struct ObjectStore {
    client: Client,
    bucket: String,
}

impl ObjectStore {
    pub async fn from_settings(settings: &Settings) -> Result<Option<Self>> {
        if settings.object_storage_provider == "disabled" {
            return Ok(None);
        }
        let access_key = settings
            .object_storage_access_key_id
            .clone()
            .context("object storage access key is missing")?;
        let secret_key = settings
            .object_storage_secret_access_key
            .clone()
            .context("object storage secret is missing")?;
        let endpoint = settings
            .object_storage_endpoint
            .clone()
            .context("object storage endpoint is missing")?;
        let bucket = settings
            .object_storage_bucket
            .clone()
            .context("object storage bucket is missing")?;
        let credentials =
            Credentials::new(access_key, secret_key, None, None, "mailer-object-storage");
        let sdk = aws_config::defaults(aws_config::BehaviorVersion::latest())
            .region(aws_config::Region::new(
                settings.object_storage_region.clone(),
            ))
            .credentials_provider(credentials)
            .endpoint_url(endpoint)
            .load()
            .await;
        let config = aws_sdk_s3::config::Builder::from(&sdk)
            .force_path_style(true)
            .build();
        Ok(Some(Self {
            client: Client::from_conf(config),
            bucket,
        }))
    }

    pub async fn put(&self, key: &str, content: Vec<u8>) -> Result<Vec<u8>> {
        let checksum = Sha256::digest(&content).to_vec();
        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(key)
            .body(ByteStream::from(content))
            .content_type("application/json")
            .send()
            .await?;
        Ok(checksum)
    }

    pub async fn get_verified(&self, key: &str, expected_checksum: &[u8]) -> Result<Vec<u8>> {
        let result = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await?;
        let bytes = result.body.collect().await?.into_bytes().to_vec();
        if Sha256::digest(&bytes).as_slice() != expected_checksum {
            anyhow::bail!("object checksum mismatch");
        }
        Ok(bytes)
    }

    pub async fn delete(&self, key: &str) -> Result<()> {
        self.client
            .delete_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await?;
        Ok(())
    }
}
