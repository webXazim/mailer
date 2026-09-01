use anyhow::{bail, Context, Result};
use aws_sdk_sqs::types::Message;
use base64::{engine::general_purpose::STANDARD, Engine};
use http_body_util::{BodyExt, Full};
use hyper::{body::Bytes, Request};
use hyper_rustls::HttpsConnectorBuilder;
use hyper_util::{client::legacy::Client, rt::TokioExecutor};
use rsa::{pkcs1v15::VerifyingKey, pkcs8::DecodePublicKey, signature::Verifier, RsaPublicKey};
use serde::Deserialize;
use sha1::Sha1;
use sha2::Sha256;
use std::time::Duration;
use url::Url;
use x509_cert::{
    der::{DecodePem, Encode},
    Certificate,
};

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct SnsEnvelope {
    #[serde(rename = "Type")]
    message_type: String,
    #[serde(rename = "MessageId")]
    message_id: String,
    #[serde(rename = "TopicArn")]
    topic_arn: String,
    #[serde(rename = "Message")]
    message: String,
    #[serde(rename = "Timestamp")]
    timestamp: String,
    #[serde(rename = "SignatureVersion")]
    signature_version: String,
    #[serde(rename = "Signature")]
    signature: String,
    #[serde(rename = "SigningCertURL")]
    signing_cert_url: String,
    #[serde(rename = "Subject")]
    subject: Option<String>,
}

#[derive(Deserialize)]
struct SesNotification {
    #[serde(rename = "eventType", alias = "notificationType")]
    event_type: String,
    mail: SesMail,
    #[serde(flatten)]
    data: serde_json::Map<String, serde_json::Value>,
}
#[derive(Deserialize)]
struct SesMail {
    #[serde(rename = "messageId")]
    message_id: String,
    timestamp: chrono::DateTime<chrono::Utc>,
    #[serde(default)]
    destination: Vec<String>,
}

pub async fn run(
    sqs: aws_sdk_sqs::Client,
    queue_url: String,
    topic_arn: Option<String>,
    api_url: String,
    ingest_token: String,
    region: String,
) -> Result<()> {
    let https = HttpsConnectorBuilder::new()
        .with_native_roots()?
        .https_only()
        .enable_http1()
        .build();
    let cert_client: Client<_, Full<Bytes>> = Client::builder(TokioExecutor::new()).build(https);
    let http = hyper_util::client::legacy::connect::HttpConnector::new();
    let api_client: Client<_, Full<Bytes>> = Client::builder(TokioExecutor::new()).build(http);
    loop {
        let response = sqs
            .receive_message()
            .queue_url(&queue_url)
            .max_number_of_messages(10)
            .wait_time_seconds(20)
            .visibility_timeout(60)
            .send()
            .await?;
        for message in response.messages() {
            let receipt = message
                .receipt_handle()
                .context("SQS message has no receipt handle")?;
            match process_message(
                &cert_client,
                &api_client,
                message,
                topic_arn.as_deref(),
                &api_url,
                &ingest_token,
                &region,
            )
            .await
            {
                Ok(()) => {
                    sqs.delete_message()
                        .queue_url(&queue_url)
                        .receipt_handle(receipt)
                        .send()
                        .await?;
                }
                Err(error) => {
                    tracing::error!(error = %error, "SES event message rejected; leaving it for SQS redrive");
                }
            }
        }
    }
}

async fn process_message<C1, C2>(
    cert_client: &Client<C1, Full<Bytes>>,
    api_client: &Client<C2, Full<Bytes>>,
    message: &Message,
    expected_topic: Option<&str>,
    api_url: &str,
    ingest_token: &str,
    region: &str,
) -> Result<()>
where
    C1: hyper_util::client::legacy::connect::Connect + Clone + Send + Sync + 'static,
    C2: hyper_util::client::legacy::connect::Connect + Clone + Send + Sync + 'static,
{
    let body = message.body().context("SQS message has no body")?;
    let envelope: SnsEnvelope = serde_json::from_str(body).context("invalid SNS envelope")?;
    if envelope.message_type != "Notification"
        || expected_topic.is_some_and(|value| value != envelope.topic_arn)
    {
        bail!("unexpected SNS message");
    }
    verify_sns_signature(cert_client, &envelope, region).await?;
    let notification: SesNotification =
        serde_json::from_str(&envelope.message).context("invalid SES notification")?;
    // These events are not part of the public contract; acknowledge rather than poison the queue.
    if matches!(
        notification.event_type.as_str(),
        "Send" | "DeliveryDelay" | "Subscription"
    ) {
        return Ok(());
    }
    let event = normalize(notification, envelope.message_id)?;
    let payload = serde_json::to_vec(&event)?;
    let endpoint = format!("{}/internal/v1/ses/events", api_url.trim_end_matches('/'));
    let request = Request::post(endpoint)
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {ingest_token}"))
        .body(Full::new(Bytes::from(payload)))?;
    let response =
        tokio::time::timeout(Duration::from_secs(10), api_client.request(request)).await??;
    if !response.status().is_success() {
        bail!("normalized SES endpoint returned {}", response.status());
    }
    Ok(())
}

async fn verify_sns_signature<C>(
    client: &Client<C, Full<Bytes>>,
    envelope: &SnsEnvelope,
    region: &str,
) -> Result<()>
where
    C: hyper_util::client::legacy::connect::Connect + Clone + Send + Sync + 'static,
{
    let cert_url = Url::parse(&envelope.signing_cert_url)?;
    if cert_url.scheme() != "https"
        || cert_url.host_str() != Some(&format!("sns.{region}.amazonaws.com"))
        || !cert_url.path().ends_with(".pem")
    {
        bail!("invalid SNS signing certificate URL");
    }
    let response = tokio::time::timeout(
        Duration::from_secs(5),
        client.get(cert_url.as_str().parse()?),
    )
    .await??;
    if !response.status().is_success() {
        bail!("SNS signing certificate fetch failed");
    }
    let cert = response.into_body().collect().await?.to_bytes();
    let certificate = Certificate::from_pem(std::str::from_utf8(&cert)?)?;
    let public_key_der = certificate
        .tbs_certificate
        .subject_public_key_info
        .to_der()?;
    let key = RsaPublicKey::from_public_key_der(&public_key_der)?;
    let mut canonical = format!(
        "Message\n{}\nMessageId\n{}\n",
        envelope.message, envelope.message_id
    );
    if let Some(subject) = &envelope.subject {
        canonical.push_str(&format!("Subject\n{subject}\n"));
    }
    canonical.push_str(&format!(
        "Timestamp\n{}\nTopicArn\n{}\nType\n{}\n",
        envelope.timestamp, envelope.topic_arn, envelope.message_type
    ));
    let signature = STANDARD.decode(&envelope.signature)?;
    match envelope.signature_version.as_str() {
        "1" => VerifyingKey::<Sha1>::new(key).verify(
            canonical.as_bytes(),
            &rsa::pkcs1v15::Signature::try_from(signature.as_slice())?,
        )?,
        "2" => VerifyingKey::<Sha256>::new(key).verify(
            canonical.as_bytes(),
            &rsa::pkcs1v15::Signature::try_from(signature.as_slice())?,
        )?,
        _ => bail!("unsupported SNS signature version"),
    }
    Ok(())
}

fn normalize(notification: SesNotification, event_id: String) -> Result<serde_json::Value> {
    use serde_json::json;
    let kind = notification
        .event_type
        .to_ascii_lowercase()
        .replace([' ', '_'], "");
    let (event_type, key, recipient_key) = match kind.as_str() {
        "delivery" => ("delivery", "delivery", "recipients"),
        "bounce" => ("bounce", "bounce", "bouncedRecipients"),
        "complaint" => ("complaint", "complaint", "complainedRecipients"),
        "reject" => ("reject", "reject", ""),
        "renderingfailure" => ("rendering_failure", "failure", ""),
        "open" => ("open", "open", ""),
        "click" => ("click", "click", ""),
        _ => bail!("unsupported SES event type"),
    };
    let details = notification
        .data
        .get(key)
        .context("SES event data missing")?;
    let occurred_at = match details.get("timestamp").and_then(|v| v.as_str()) {
        Some(value) => value.parse::<chrono::DateTime<chrono::Utc>>()?,
        None => notification.mail.timestamp,
    };
    let recipients: Vec<String> = if recipient_key.is_empty() {
        notification.mail.destination
    } else {
        details
            .get(recipient_key)
            .and_then(|v| v.as_array())
            .context("SES recipients missing")?
            .iter()
            .map(|v| {
                v.as_str()
                    .or_else(|| v.get("emailAddress").and_then(|v| v.as_str()))
                    .map(str::to_owned)
                    .context("invalid SES recipient")
            })
            .collect::<Result<_>>()?
    };
    Ok(
        json!({"eventId":event_id,"messageId":notification.mail.message_id,"eventType":event_type,
        "occurredAt":occurred_at,"recipients":recipients,"bounceType":details.get("bounceType"),"details":details}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn normalize_fixture(kind: &str, key: &str, data: serde_json::Value) -> serde_json::Value {
        let mut value = json!({"eventType":kind,"mail":{"messageId":"provider-id","timestamp":"2026-08-31T00:00:00Z","destination":["r@example.com"]}});
        value[key] = data;
        normalize(serde_json::from_value(value).unwrap(), "event-id".into()).unwrap()
    }
    #[test]
    fn aws_feedback_shapes() {
        let delivery = normalize_fixture(
            "Delivery",
            "delivery",
            json!({"timestamp":"2026-08-31T00:01:00Z","recipients":["r@example.com"]}),
        );
        assert_eq!(delivery["eventType"], "delivery");
        let bounce = normalize_fixture(
            "Bounce",
            "bounce",
            json!({"timestamp":"2026-08-31T00:01:00Z","bounceType":"Permanent","bouncedRecipients":[{"emailAddress":"r@example.com"}]}),
        );
        assert_eq!(bounce["bounceType"], "Permanent");
        assert_eq!(bounce["recipients"][0], "r@example.com");
        let complaint = normalize_fixture(
            "Complaint",
            "complaint",
            json!({"timestamp":"2026-08-31T00:01:00Z","complainedRecipients":[{"emailAddress":"r@example.com"}]}),
        );
        assert_eq!(complaint["recipients"][0], "r@example.com");
        assert_eq!(
            normalize_fixture("Reject", "reject", json!({"reason":"Bad content"}))["eventType"],
            "reject"
        );
        assert_eq!(
            normalize_fixture(
                "Rendering Failure",
                "failure",
                json!({"errorMessage":"Missing variable","templateName":"Test"})
            )["eventType"],
            "rendering_failure"
        );
        for kind in ["Open", "Click"] {
            let value = normalize_fixture(
                kind,
                &kind.to_lowercase(),
                json!({"timestamp":"2026-08-31T00:01:00Z","ipAddress":"192.0.2.1","link":"https://example.com"}),
            );
            assert_eq!(value["occurredAt"], "2026-08-31T00:01:00Z");
            assert_eq!(value["details"]["link"], "https://example.com");
        }
    }
}
