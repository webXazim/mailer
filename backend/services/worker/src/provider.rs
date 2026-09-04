use super::delivery::{build_raw_message, Email, ProviderFailure};
use anyhow::{Context, Result};
use config::Settings;
use lettre::{
    address::{Address, Envelope},
    transport::smtp::{
        authentication::Credentials, client::Tls, extension::ClientId, Error as SmtpError,
    },
    AsyncSmtpTransport, AsyncTransport, Tokio1Executor,
};
use std::time::Duration;

pub(crate) struct DeliveryProviders {
    ses: Option<aws_sdk_sesv2::Client>,
    ses_configuration_set: Option<String>,
    smtp: Option<SmtpProvider>,
}

struct SmtpProvider {
    transport: AsyncSmtpTransport<Tokio1Executor>,
    message_id_domain: String,
    return_path_prefix: String,
}

impl DeliveryProviders {
    pub(crate) fn new(ses: Option<aws_sdk_sesv2::Client>, settings: &Settings) -> Result<Self> {
        let smtp = match settings.smtp_host.as_deref() {
            Some(host) => {
                let builder = match settings.smtp_security.as_str() {
                    "implicit_tls" => AsyncSmtpTransport::<Tokio1Executor>::relay(host),
                    "starttls" => AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(host),
                    _ => unreachable!("configuration validates SMTP_SECURITY"),
                }
                .context("unable to configure SMTP TLS")?;
                let username = settings
                    .smtp_username
                    .as_ref()
                    .context("SMTP_USERNAME is required when SMTP_HOST is set")?;
                let password = settings
                    .smtp_password
                    .as_ref()
                    .context("SMTP_PASSWORD is required when SMTP_HOST is set")?;
                let helo_name = settings
                    .smtp_helo_name
                    .as_ref()
                    .context("SMTP_HELO_NAME is required when SMTP_HOST is set")?;
                Some(SmtpProvider {
                    transport: builder
                        .port(settings.smtp_port)
                        .tls(match settings.smtp_security.as_str() {
                            "implicit_tls" => Tls::Wrapper(
                                lettre::transport::smtp::client::TlsParameters::new(
                                    host.to_owned(),
                                )
                                .context("invalid SMTP TLS hostname")?,
                            ),
                            "starttls" => Tls::Required(
                                lettre::transport::smtp::client::TlsParameters::new(
                                    host.to_owned(),
                                )
                                .context("invalid SMTP TLS hostname")?,
                            ),
                            _ => unreachable!(),
                        })
                        .credentials(Credentials::new(username.clone(), password.clone()))
                        .hello_name(ClientId::Domain(helo_name.clone()))
                        .timeout(Some(Duration::from_secs(settings.smtp_timeout_seconds)))
                        .build(),
                    message_id_domain: helo_name.clone(),
                    return_path_prefix: settings.mta_return_path_prefix.clone(),
                })
            }
            None => None,
        };
        Ok(Self {
            ses,
            ses_configuration_set: settings.ses_configuration_set.clone(),
            smtp,
        })
    }

    pub(crate) async fn submit(
        &self,
        provider: &str,
        email: &Email,
    ) -> Result<String, ProviderFailure> {
        match provider {
            "ses" => {
                let client = self.ses.as_ref().ok_or_else(|| {
                    ProviderFailure::Permanent("SES delivery provider is unavailable".into())
                })?;
                super::delivery::send_ses(client, email, self.ses_configuration_set.as_deref())
                    .await
            }
            "smtp" => {
                let smtp = self.smtp.as_ref().ok_or_else(|| {
                    ProviderFailure::Permanent("SMTP delivery provider is unavailable".into())
                })?;
                smtp.submit(email).await
            }
            _ => Err(ProviderFailure::Permanent(format!(
                "Unknown delivery provider: {provider}"
            ))),
        }
    }

    pub(crate) fn is_available(&self, provider: &str) -> bool {
        match provider {
            "ses" => self.ses.is_some(),
            "smtp" => self.smtp.is_some(),
            _ => false,
        }
    }
}

impl SmtpProvider {
    async fn submit(&self, email: &Email) -> Result<String, ProviderFailure> {
        let message_id = format!(
            "{}.{}@{}",
            email.id, email.attempt_id, self.message_id_domain
        );
        let raw = build_raw_message(email, Some(&message_id))?;
        let envelope_sender = envelope_sender(&email.sender, &self.return_path_prefix, email.id)?;
        let sender = envelope_sender
            .parse::<Address>()
            .map_err(|error| ProviderFailure::Permanent(format!("invalid sender: {error}")))?;
        let recipients = email
            .to
            .iter()
            .chain(&email.cc)
            .chain(&email.bcc)
            .map(|value| {
                value.parse::<Address>().map_err(|error| {
                    ProviderFailure::Permanent(format!("invalid recipient: {error}"))
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let envelope = Envelope::new(Some(sender), recipients)
            .map_err(|error| ProviderFailure::Permanent(error.to_string()))?;
        self.transport
            .send_raw(&envelope, &raw)
            .await
            .map_err(classify_smtp_error)?;
        Ok(message_id)
    }
}

fn mailbox_address(value: &str) -> &str {
    let value = value.trim();
    match (value.rfind('<'), value.rfind('>')) {
        (Some(start), Some(end)) if start < end => value[start + 1..end].trim(),
        _ => value,
    }
}

fn envelope_sender(
    value: &str,
    prefix: &str,
    email_id: uuid::Uuid,
) -> Result<String, ProviderFailure> {
    let sender_domain = mailbox_address(value)
        .rsplit_once('@')
        .map(|(_, domain)| domain)
        .ok_or_else(|| ProviderFailure::Permanent("invalid sender domain".into()))?;
    Ok(format!("mailer+{email_id}@{prefix}.{sender_domain}"))
}

fn classify_smtp_error(error: SmtpError) -> ProviderFailure {
    let reason = format!("SMTP submission failed: {error}");
    if error.is_transient() {
        ProviderFailure::Retryable(reason)
    } else if error.is_permanent() || error.is_client() {
        ProviderFailure::Permanent(reason)
    } else {
        // A transport/TLS/timeout failure may happen after DATA was accepted but
        // before the final response reached us. Automatic retry could duplicate.
        ProviderFailure::Ambiguous(reason)
    }
}

#[cfg(test)]
mod tests {
    use super::{envelope_sender, mailbox_address};
    use uuid::Uuid;

    #[test]
    fn extracts_envelope_sender_from_display_address() {
        assert_eq!(
            mailbox_address("Crescent Mail <no-reply@mailer.example>"),
            "no-reply@mailer.example"
        );
        assert_eq!(mailbox_address("sender@example.com"), "sender@example.com");
    }

    #[test]
    fn uses_aligned_bounce_subdomain_for_smtp_envelope() {
        let id = Uuid::parse_str("dc2b60ed-27e3-4faa-88bc-e653d868c082").unwrap();
        assert_eq!(
            envelope_sender("Sender <hello@mail.example.com>", "bounce", id).unwrap(),
            "mailer+dc2b60ed-27e3-4faa-88bc-e653d868c082@bounce.mail.example.com"
        );
    }
}
