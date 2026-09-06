use anyhow::Result;
use http_body_util::{BodyExt, Full};
use hyper::{body::Bytes, Request, StatusCode};
use hyper_rustls::HttpsConnectorBuilder;
use hyper_util::{client::legacy::Client, rt::TokioExecutor};
use serde::Deserialize;
use sqlx::Row;
use std::time::Duration;
use uuid::Uuid;

#[derive(Deserialize)]
struct MailerResponse {
    data: MailerMessage,
}

#[derive(Deserialize)]
struct MailerMessage {
    id: Uuid,
}

enum MailerFailure {
    Retryable(String),
    Permanent(String),
}

enum AccountDelivery {
    Submitted(Uuid),
}

struct AccountMessage<'a> {
    id: Uuid,
    from: &'a str,
    recipient: &'a str,
    subject: &'a str,
    text: &'a str,
    html: Option<&'a str>,
}

pub async fn run(
    pool: db::DbPool,
    from: Option<String>,
    api_url: String,
    api_key: Option<String>,
    mut stop: tokio::sync::watch::Receiver<bool>,
) -> Result<()> {
    let connector = HttpsConnectorBuilder::new()
        .with_native_roots()?
        .https_or_http()
        .enable_http1()
        .build();
    let client: Client<_, Full<Bytes>> = Client::builder(TokioExecutor::new())
        .pool_idle_timeout(Duration::from_secs(30))
        .build(connector);
    let mut interval = tokio::time::interval(Duration::from_secs(2));
    loop {
        tokio::select! {_=stop.changed()=>break,_=interval.tick()=>{}}
        let Some(from) = from.as_deref() else {
            continue;
        };
        let normalized_from = normalize_sender(from);
        let row=sqlx::query("UPDATE account_emails SET status='processing',attempts=attempts+1,updated_at=now() WHERE id=(SELECT id FROM account_emails WHERE status='queued' AND available_at<=now() AND expires_at>now() ORDER BY available_at FOR UPDATE SKIP LOCKED LIMIT 1) RETURNING id,recipient,subject,body,html_body,attempts")
            .fetch_optional(&pool).await?;
        let Some(row) = row else { continue };
        let id: Uuid = row.get("id");
        let recipient: String = row.get("recipient");
        let subject: String = row.get("subject");
        let body: String = row.get("body");
        let html_body: Option<String> = row.get("html_body");
        let attempts: i32 = row.get("attempts");

        let outcome = if let Some(key) = api_key.as_deref() {
            send_via_mailer(
                &client,
                &api_url,
                key,
                AccountMessage {
                    id,
                    from: &normalized_from,
                    recipient: &recipient,
                    subject: &subject,
                    text: &body,
                    html: html_body.as_deref(),
                },
            )
            .await
            .map(AccountDelivery::Submitted)
        } else {
            Err(MailerFailure::Permanent(
                "Account email requires ACCOUNT_EMAIL_API_KEY".into(),
            ))
        };

        match outcome {
            Ok(AccountDelivery::Submitted(email_id)) => {
                sqlx::query("UPDATE account_emails SET status='submitted',body='',html_body=NULL,mailer_email_id=$2,updated_at=now() WHERE id=$1").bind(id).bind(email_id).execute(&pool).await?;
            }
            Err(failure) => {
                let (retry, reason) = match failure {
                    MailerFailure::Retryable(reason) => (attempts < 5, reason),
                    MailerFailure::Permanent(reason) => (false, reason),
                };
                sqlx::query("UPDATE account_emails SET status=$2,body=CASE WHEN $2='queued' THEN body ELSE '' END,html_body=CASE WHEN $2='queued' THEN html_body ELSE NULL END,last_error=$3,available_at=now()+interval '30 seconds',updated_at=now() WHERE id=$1").bind(id).bind(if retry {"queued"} else {"failed"}).bind(reason).execute(&pool).await?;
            }
        }
    }
    Ok(())
}

fn normalize_sender(value: &str) -> String {
    let value = value.trim();
    let Some(address_start) = value.rfind('<') else {
        return value.to_owned();
    };
    if !value.ends_with('>') {
        return value.to_owned();
    }
    let display_name = value[..address_start].trim();
    let address = &value[address_start + 1..value.len() - 1];
    let display_name = display_name
        .strip_prefix('<')
        .and_then(|name| name.strip_suffix('>'))
        .unwrap_or(display_name)
        .trim();
    if display_name.is_empty() {
        address.to_owned()
    } else {
        format!("{display_name} <{address}>")
    }
}

#[cfg(test)]
mod tests {
    use super::normalize_sender;

    #[test]
    fn removes_accidental_brackets_around_display_name() {
        assert_eq!(
            normalize_sender("<CrescentSphere Mailer> <no-reply@mailer.crescentsphere.com>"),
            "CrescentSphere Mailer <no-reply@mailer.crescentsphere.com>"
        );
    }

    #[test]
    fn preserves_valid_sender_forms() {
        assert_eq!(
            normalize_sender("CrescentSphere Mailer <no-reply@mailer.crescentsphere.com>"),
            "CrescentSphere Mailer <no-reply@mailer.crescentsphere.com>"
        );
        assert_eq!(
            normalize_sender("no-reply@mailer.crescentsphere.com"),
            "no-reply@mailer.crescentsphere.com"
        );
    }
}

async fn send_via_mailer<C>(
    client: &Client<C, Full<Bytes>>,
    api_url: &str,
    api_key: &str,
    message: AccountMessage<'_>,
) -> Result<Uuid, MailerFailure>
where
    C: hyper_util::client::legacy::connect::Connect + Clone + Send + Sync + 'static,
{
    let payload = serde_json::to_vec(&serde_json::json!({
        "from": message.from,
        "to": [message.recipient],
        "subject": message.subject,
        "text": message.text,
        "html": message.html,
        "environment": "production"
    }))
    .map_err(|error| MailerFailure::Permanent(error.to_string()))?;
    let endpoint = format!("{}/v1/emails", api_url.trim_end_matches('/'));
    let request = Request::post(endpoint)
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {api_key}"))
        .header("idempotency-key", format!("account-email-{}", message.id))
        .body(Full::new(Bytes::from(payload)))
        .map_err(|error| MailerFailure::Permanent(error.to_string()))?;
    let response = tokio::time::timeout(Duration::from_secs(10), client.request(request))
        .await
        .map_err(|_| MailerFailure::Retryable("Mailer API request timed out".into()))?
        .map_err(|error| MailerFailure::Retryable(format!("Mailer API request failed: {error}")))?;
    let status = response.status();
    if !status.is_success() {
        let reason = format!("Mailer API rejected account email with HTTP {status}");
        return Err(
            if status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error() {
                MailerFailure::Retryable(reason)
            } else {
                MailerFailure::Permanent(reason)
            },
        );
    }
    let bytes = response
        .into_body()
        .collect()
        .await
        .map_err(|error| {
            MailerFailure::Retryable(format!("Unable to read Mailer response: {error}"))
        })?
        .to_bytes();
    let response: MailerResponse = serde_json::from_slice(&bytes)
        .map_err(|error| MailerFailure::Retryable(format!("Invalid Mailer response: {error}")))?;
    Ok(response.data.id)
}
