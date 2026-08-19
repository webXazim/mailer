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
    #[serde(rename = "eventType")]
    event_type: String,
    mail: SesMail,
    delivery: Option<SesDelivery>,
    bounce: Option<SesBounce>,
    complaint: Option<SesComplaint>,
    reject: Option<SesReject>,
    #[serde(rename = "failure")]
    failure: Option<SesFailure>,
}
#[derive(Deserialize)]
struct SesMail {
    #[serde(rename = "messageId")]
    message_id: String,
    timestamp: chrono::DateTime<chrono::Utc>,
}
#[derive(Deserialize)]
struct SesDelivery {
    timestamp: chrono::DateTime<chrono::Utc>,
    recipients: Vec<String>,
}
#[derive(Deserialize)]
struct SesBounce {
    timestamp: chrono::DateTime<chrono::Utc>,
    bounce_type: String,
    bounced_recipients: Vec<SesRecipient>,
}
#[derive(Deserialize)]
struct SesComplaint {
    timestamp: chrono::DateTime<chrono::Utc>,
    complained_recipients: Vec<SesRecipient>,
}
#[derive(Deserialize)]
struct SesReject {
    timestamp: chrono::DateTime<chrono::Utc>,
    reason: Option<String>,
}
#[derive(Deserialize)]
struct SesFailure {
    timestamp: chrono::DateTime<chrono::Utc>,
    recipients: Vec<String>,
    failure_type: Option<String>,
}
#[derive(Deserialize)]
struct SesRecipient {
    #[serde(rename = "emailAddress")]
    email_address: String,
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
    let (event_type, occurred_at, recipients, bounce_type, details) =
        match notification.event_type.to_ascii_lowercase().as_str() {
            "delivery" => {
                let value = notification.delivery.context("delivery data missing")?;
                (
                    "delivery",
                    value.timestamp,
                    value.recipients,
                    None,
                    serde_json::json!({}),
                )
            }
            "bounce" => {
                let value = notification.bounce.context("bounce data missing")?;
                (
                    "bounce",
                    value.timestamp,
                    value
                        .bounced_recipients
                        .into_iter()
                        .map(|recipient| recipient.email_address)
                        .collect(),
                    Some(value.bounce_type),
                    serde_json::json!({}),
                )
            }
            "complaint" => {
                let value = notification.complaint.context("complaint data missing")?;
                (
                    "complaint",
                    value.timestamp,
                    value
                        .complained_recipients
                        .into_iter()
                        .map(|recipient| recipient.email_address)
                        .collect(),
                    None,
                    serde_json::json!({}),
                )
            }
            "reject" => {
                let value = notification.reject.context("reject data missing")?;
                (
                    "reject",
                    value.timestamp,
                    vec![],
                    None,
                    serde_json::json!({"reason": value.reason}),
                )
            }
            "renderingfailure" => {
                let value = notification.failure.context("failure data missing")?;
                (
                    "rendering_failure",
                    value.timestamp,
                    value.recipients,
                    None,
                    serde_json::json!({"failureType": value.failure_type}),
                )
            }
            "open" => (
                "open",
                notification.mail.timestamp,
                vec![],
                None,
                serde_json::json!({}),
            ),
            "click" => (
                "click",
                notification.mail.timestamp,
                vec![],
                None,
                serde_json::json!({}),
            ),
            value => bail!("unsupported SES event type: {value}"),
        };
    Ok(
        serde_json::json!({"eventId": event_id, "messageId": notification.mail.message_id, "eventType": event_type, "occurredAt": occurred_at, "recipients": recipients, "bounceType": bounce_type, "details": details}),
    )
}
