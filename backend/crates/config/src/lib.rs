use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::{env, net::SocketAddr, time::Duration};
use url::Url;

const DEVELOPMENT_EVENT_TOKEN: &str = "development-event-token-change-me";
const DEVELOPMENT_WEBHOOK_MASTER_KEY: &str = "development-webhook-master-key-change-me";

#[derive(Clone, Deserialize)]
pub struct Settings {
    pub app_env: String,
    pub http_addr: SocketAddr,
    pub database_url: String,
    pub nats_url: String,
    pub console_origin: String,
    pub aws_region: String,
    pub ses_configuration_set: Option<String>,
    pub delivery_provider: String,
    pub smtp_host: Option<String>,
    pub smtp_port: u16,
    pub smtp_security: String,
    pub smtp_username: Option<String>,
    pub smtp_password: Option<String>,
    pub smtp_helo_name: Option<String>,
    pub smtp_timeout_seconds: u64,
    pub account_email_from: Option<String>,
    pub account_email_api_key: Option<String>,
    pub auth_email_delivery_enabled: bool,
    pub turnstile_site_key: Option<String>,
    pub turnstile_secret_key: Option<String>,
    pub cloudflare_oauth_client_id: Option<String>,
    pub cloudflare_oauth_client_secret: Option<String>,
    pub cloudflare_oauth_scopes: String,
    pub domain_provider: String,
    pub event_ingest_token: String,
    pub webhook_signing_master_key: String,
    pub ses_events_queue_url: Option<String>,
    pub ses_events_topic_arn: Option<String>,
    pub internal_api_url: String,
    pub object_storage_provider: String,
    pub object_storage_endpoint: Option<String>,
    pub object_storage_region: String,
    pub object_storage_bucket: Option<String>,
    pub object_storage_access_key_id: Option<String>,
    pub object_storage_secret_access_key: Option<String>,
    pub email_content_retention_days: u32,
    pub api_key_rate_limit_per_minute: u32,
    pub client_ip_rate_limit_per_minute: u32,
    pub trust_proxy_headers: bool,
    pub workspace_monthly_email_limit: u64,
    pub workspace_concurrent_email_limit: u32,
    pub log_level: String,
    pub db_min_connections: u32,
    pub db_max_connections: u32,
    pub db_acquire_timeout_seconds: u64,
}

impl Settings {
    pub fn from_env() -> Result<Self> {
        let app_env = env::var("APP_ENV").unwrap_or_else(|_| "development".into());
        let http_addr = env::var("HTTP_ADDR")
            .unwrap_or_else(|_| "0.0.0.0:8080".into())
            .parse()
            .context("HTTP_ADDR must be a valid socket address")?;
        let database_url = required("DATABASE_URL")?;
        let nats_url = env::var("NATS_URL").unwrap_or_else(|_| "nats://127.0.0.1:4222".into());
        let console_origin =
            env::var("CONSOLE_ORIGIN").unwrap_or_else(|_| "http://localhost:5173".into());
        let aws_region = env::var("AWS_REGION").unwrap_or_else(|_| "ap-southeast-1".into());
        let ses_configuration_set = optional("SES_CONFIGURATION_SET");
        let delivery_provider = env::var("DELIVERY_PROVIDER").unwrap_or_else(|_| "ses".into());
        let smtp_host = optional("SMTP_HOST");
        let smtp_port_value = parse_u32("SMTP_PORT", 465)?;
        let smtp_port =
            u16::try_from(smtp_port_value).context("SMTP_PORT is outside the port range")?;
        if smtp_port == 0 {
            bail!("SMTP_PORT must be positive");
        }
        let smtp_security = env::var("SMTP_SECURITY").unwrap_or_else(|_| "implicit_tls".into());
        let smtp_username = optional("SMTP_USERNAME");
        let smtp_password = optional("SMTP_PASSWORD");
        let smtp_helo_name = optional("SMTP_HELO_NAME");
        let smtp_timeout_seconds = parse_u64("SMTP_TIMEOUT_SECONDS", 30)?;
        let account_email_from = optional("ACCOUNT_EMAIL_FROM");
        let account_email_api_key = optional("ACCOUNT_EMAIL_API_KEY");
        let auth_email_delivery_enabled = parse_bool("AUTH_EMAIL_DELIVERY_ENABLED", false)?;
        let turnstile_site_key = optional("TURNSTILE_SITE_KEY");
        let turnstile_secret_key = optional("TURNSTILE_SECRET_KEY");
        let cloudflare_oauth_client_id = optional("CLOUDFLARE_OAUTH_CLIENT_ID");
        let cloudflare_oauth_client_secret = optional("CLOUDFLARE_OAUTH_CLIENT_SECRET");
        let cloudflare_oauth_scopes =
            env::var("CLOUDFLARE_OAUTH_SCOPES").unwrap_or_else(|_| "zone.read dns.write".into());
        let domain_provider = env::var("DOMAIN_PROVIDER").unwrap_or_else(|_| "disabled".into());
        let event_ingest_token =
            env::var("EVENT_INGEST_TOKEN").unwrap_or_else(|_| DEVELOPMENT_EVENT_TOKEN.into());
        let webhook_signing_master_key = env::var("WEBHOOK_SIGNING_MASTER_KEY")
            .unwrap_or_else(|_| DEVELOPMENT_WEBHOOK_MASTER_KEY.into());
        let ses_events_queue_url = env::var("SES_EVENTS_QUEUE_URL")
            .ok()
            .filter(|value| !value.trim().is_empty());
        let ses_events_topic_arn = env::var("SES_EVENTS_TOPIC_ARN")
            .ok()
            .filter(|value| !value.trim().is_empty());
        let internal_api_url =
            env::var("INTERNAL_API_URL").unwrap_or_else(|_| "http://127.0.0.1:8080".into());
        let object_storage_provider =
            env::var("OBJECT_STORAGE_PROVIDER").unwrap_or_else(|_| "disabled".into());
        let object_storage_endpoint = optional("OBJECT_STORAGE_ENDPOINT");
        let object_storage_region =
            env::var("OBJECT_STORAGE_REGION").unwrap_or_else(|_| "auto".into());
        let object_storage_bucket = optional("OBJECT_STORAGE_BUCKET");
        let object_storage_access_key_id = optional("OBJECT_STORAGE_ACCESS_KEY_ID");
        let object_storage_secret_access_key = optional("OBJECT_STORAGE_SECRET_ACCESS_KEY");
        let email_content_retention_days = parse_u32("EMAIL_CONTENT_RETENTION_DAYS", 30)?;
        let api_key_rate_limit_per_minute = parse_u32("API_KEY_RATE_LIMIT_PER_MINUTE", 60)?;
        let client_ip_rate_limit_per_minute = parse_u32("CLIENT_IP_RATE_LIMIT_PER_MINUTE", 300)?;
        let trust_proxy_headers = parse_bool("TRUST_PROXY_HEADERS", false)?;
        let workspace_monthly_email_limit = parse_u64("WORKSPACE_MONTHLY_EMAIL_LIMIT", 100_000)?;
        let workspace_concurrent_email_limit = parse_u32("WORKSPACE_CONCURRENT_EMAIL_LIMIT", 100)?;
        if api_key_rate_limit_per_minute == 0
            || client_ip_rate_limit_per_minute == 0
            || workspace_monthly_email_limit == 0
            || workspace_concurrent_email_limit == 0
        {
            bail!("abuse limits must be positive");
        }
        if !(1..=3650).contains(&email_content_retention_days) {
            bail!("EMAIL_CONTENT_RETENTION_DAYS must be between 1 and 3650");
        }
        if !matches!(domain_provider.as_str(), "disabled" | "ses") {
            bail!("DOMAIN_PROVIDER must be disabled or ses");
        }
        if !matches!(delivery_provider.as_str(), "ses" | "smtp") {
            bail!("DELIVERY_PROVIDER must be ses or smtp");
        }
        if !matches!(smtp_security.as_str(), "implicit_tls" | "starttls") {
            bail!("SMTP_SECURITY must be implicit_tls or starttls");
        }
        if smtp_timeout_seconds == 0 || smtp_timeout_seconds > 300 {
            bail!("SMTP_TIMEOUT_SECONDS must be between 1 and 300");
        }
        if delivery_provider == "smtp"
            && (smtp_host.is_none()
                || smtp_username.is_none()
                || smtp_password.is_none()
                || smtp_helo_name.is_none())
        {
            bail!("SMTP_HOST, SMTP_USERNAME, SMTP_PASSWORD, and SMTP_HELO_NAME are required when DELIVERY_PROVIDER=smtp");
        }
        if ses_events_queue_url.is_some() != ses_events_topic_arn.is_some() {
            bail!("SES_EVENTS_QUEUE_URL and SES_EVENTS_TOPIC_ARN must be set together");
        }
        if !matches!(object_storage_provider.as_str(), "disabled" | "r2" | "s3") {
            bail!("OBJECT_STORAGE_PROVIDER must be disabled, r2, or s3");
        }
        if object_storage_provider != "disabled"
            && (object_storage_endpoint.is_none()
                || object_storage_bucket.is_none()
                || object_storage_access_key_id.is_none()
                || object_storage_secret_access_key.is_none())
        {
            bail!("OBJECT_STORAGE_ENDPOINT, OBJECT_STORAGE_BUCKET, OBJECT_STORAGE_ACCESS_KEY_ID, and OBJECT_STORAGE_SECRET_ACCESS_KEY are required when object storage is enabled");
        }
        if cloudflare_oauth_client_id.is_some() != cloudflare_oauth_client_secret.is_some() {
            bail!("CLOUDFLARE_OAUTH_CLIENT_ID and CLOUDFLARE_OAUTH_CLIENT_SECRET must be set together");
        }
        let log_level = env::var("RUST_LOG").unwrap_or_else(|_| "info,sqlx=warn".into());
        let db_min_connections = parse_u32("DB_MIN_CONNECTIONS", 2)?;
        let db_max_connections = parse_u32("DB_MAX_CONNECTIONS", 10)?;
        let db_acquire_timeout_seconds = parse_u64("DB_ACQUIRE_TIMEOUT_SECONDS", 5)?;
        if db_min_connections == 0 || db_max_connections < db_min_connections {
            bail!("DB_MAX_CONNECTIONS must be greater than or equal to DB_MIN_CONNECTIONS, and both must be positive");
        }
        if app_env == "production" {
            if turnstile_site_key.is_none()
                || turnstile_secret_key
                    .as_ref()
                    .is_none_or(|value| value.len() < 20)
            {
                bail!("Production requires TURNSTILE_SITE_KEY and a valid TURNSTILE_SECRET_KEY");
            }
            if auth_email_delivery_enabled && account_email_from.is_none() {
                bail!("ACCOUNT_EMAIL_FROM is required when AUTH_EMAIL_DELIVERY_ENABLED=true");
            }
            if database_url.contains("localhost") || database_url.contains("REPLACE_WITH") {
                bail!("DATABASE_URL must use production credentials and host in production");
            }
            if nats_url.contains("localhost") {
                bail!("NATS_URL must not point to localhost in production");
            }
            if nats_url.contains("mailer-development") || !nats_url.contains('@') {
                bail!("NATS_URL must use authenticated credentials in production");
            }
            if !console_origin.starts_with("https://") {
                bail!("CONSOLE_ORIGIN must use HTTPS in production");
            }
            let console_url =
                Url::parse(&console_origin).context("CONSOLE_ORIGIN must be a valid URL")?;
            if console_url.host_str().is_none()
                || console_url.path() != "/" && !console_url.path().is_empty()
            {
                bail!("CONSOLE_ORIGIN must be an origin without a path");
            }
            let internal_url =
                Url::parse(&internal_api_url).context("INTERNAL_API_URL must be a valid URL")?;
            if internal_url.host_str().is_none() {
                bail!("INTERNAL_API_URL must include a host");
            }
            if domain_provider != "ses" {
                bail!("DOMAIN_PROVIDER must be ses in production");
            }
            if delivery_provider == "ses" {
                if ses_configuration_set.is_none() {
                    bail!("SES_CONFIGURATION_SET is required when DELIVERY_PROVIDER=ses");
                }
                if ses_events_queue_url.is_none()
                    || ses_events_topic_arn.is_none()
                    || ses_events_queue_url
                        .as_deref()
                        .is_some_and(|value| value.contains("ACCOUNT_ID"))
                    || ses_events_topic_arn
                        .as_deref()
                        .is_some_and(|value| value.contains("ACCOUNT_ID"))
                {
                    bail!("SES_EVENTS_QUEUE_URL and SES_EVENTS_TOPIC_ARN are required when DELIVERY_PROVIDER=ses");
                }
            }
            if object_storage_provider == "disabled" {
                bail!("OBJECT_STORAGE_PROVIDER must be enabled in production");
            }
            if event_ingest_token.len() < 32
                || event_ingest_token == DEVELOPMENT_EVENT_TOKEN
                || event_ingest_token.starts_with("REPLACE_WITH")
            {
                bail!("EVENT_INGEST_TOKEN must contain at least 32 characters in production");
            }
            if webhook_signing_master_key.len() < 32
                || webhook_signing_master_key == DEVELOPMENT_WEBHOOK_MASTER_KEY
                || webhook_signing_master_key.starts_with("REPLACE_WITH")
            {
                bail!(
                    "WEBHOOK_SIGNING_MASTER_KEY must contain at least 32 characters in production"
                );
            }
            if object_storage_access_key_id
                .as_deref()
                .is_some_and(|value| value.starts_with("REPLACE_WITH"))
                || object_storage_secret_access_key
                    .as_deref()
                    .is_some_and(|value| value.starts_with("REPLACE_WITH"))
            {
                bail!("object storage credentials must be replaced in production");
            }
        }
        Ok(Self {
            app_env,
            http_addr,
            database_url,
            nats_url,
            console_origin,
            aws_region,
            ses_configuration_set,
            delivery_provider,
            smtp_host,
            smtp_port,
            smtp_security,
            smtp_username,
            smtp_password,
            smtp_helo_name,
            smtp_timeout_seconds,
            account_email_from,
            account_email_api_key,
            auth_email_delivery_enabled,
            turnstile_site_key,
            turnstile_secret_key,
            cloudflare_oauth_client_id,
            cloudflare_oauth_client_secret,
            cloudflare_oauth_scopes,
            domain_provider,
            event_ingest_token,
            webhook_signing_master_key,
            ses_events_queue_url,
            ses_events_topic_arn,
            internal_api_url,
            object_storage_provider,
            object_storage_endpoint,
            object_storage_region,
            object_storage_bucket,
            object_storage_access_key_id,
            object_storage_secret_access_key,
            email_content_retention_days,
            api_key_rate_limit_per_minute,
            client_ip_rate_limit_per_minute,
            trust_proxy_headers,
            workspace_monthly_email_limit,
            workspace_concurrent_email_limit,
            log_level,
            db_min_connections,
            db_max_connections,
            db_acquire_timeout_seconds,
        })
    }

    pub fn db_acquire_timeout(&self) -> Duration {
        Duration::from_secs(self.db_acquire_timeout_seconds)
    }
}

fn required(name: &str) -> Result<String> {
    env::var(name).with_context(|| format!("{name} is required"))
}

fn optional(name: &str) -> Option<String> {
    env::var(name).ok().filter(|value| !value.trim().is_empty())
}

fn parse_u32(name: &str, default: u32) -> Result<u32> {
    env::var(name)
        .unwrap_or_else(|_| default.to_string())
        .parse()
        .with_context(|| format!("{name} must be a positive integer"))
}

fn parse_u64(name: &str, default: u64) -> Result<u64> {
    env::var(name)
        .unwrap_or_else(|_| default.to_string())
        .parse()
        .with_context(|| format!("{name} must be an integer"))
}

fn parse_bool(name: &str, default: bool) -> Result<bool> {
    env::var(name)
        .unwrap_or_else(|_| default.to_string())
        .parse()
        .with_context(|| format!("{name} must be true or false"))
}
