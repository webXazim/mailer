use anyhow::{bail, Context, Result};
use axum::{
    body::Bytes,
    http::{header, Method, Request},
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use http_body_util::{BodyExt, Full};
use hyper_rustls::HttpsConnectorBuilder;
use hyper_util::{client::legacy::Client as HttpClient, rt::TokioExecutor};
use rand::rngs::OsRng;
use rsa::{
    pkcs8::{EncodePrivateKey, EncodePublicKey, LineEnding},
    RsaPrivateKey, RsaPublicKey,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::time::Duration;
use uuid::Uuid;

#[derive(Clone)]
pub(crate) struct Client {
    endpoint: String,
    token: String,
}

pub(crate) struct ProvisionedDomain {
    pub domain_id: String,
    pub signature_id: String,
    pub selector: String,
    pub dkim_value: String,
}

impl Client {
    pub(crate) fn new(base_url: String, token: String) -> Self {
        Self {
            endpoint: format!("{}/api", base_url.trim_end_matches('/')),
            token,
        }
    }

    pub(crate) async fn provision(
        &self,
        name: &str,
        return_path: &str,
    ) -> Result<ProvisionedDomain> {
        let domain_id = match self.find_domain(name).await? {
            Some(id) => {
                self.update_domain(&id, true, Some(return_path)).await?;
                id
            }
            None => self.create_domain(name, return_path).await?,
        };
        if let Some(signature) = self.find_signature(&domain_id).await? {
            return Ok(signature);
        }
        self.create_signature(&domain_id, None).await
    }

    pub(crate) fn rotation_selector(&self, domain_id: &str) -> String {
        new_selector(domain_id)
    }

    pub(crate) async fn rotate(
        &self,
        domain_id: &str,
        selector: &str,
    ) -> Result<ProvisionedDomain> {
        if let Some(signature) = self.find_signature_by_selector(domain_id, selector).await? {
            return Ok(signature);
        }
        self.create_signature(domain_id, Some(selector)).await
    }

    pub(crate) async fn disable(&self, domain_id: &str) -> Result<()> {
        self.update_domain(domain_id, false, None).await
    }

    pub(crate) async fn destroy_signature(&self, signature_id: &str) -> Result<()> {
        let response = self
            .call("x:DkimSignature/set", json!({"destroy": [signature_id]}))
            .await?;
        if response
            .get("destroyed")
            .and_then(Value::as_array)
            .is_some_and(|ids| ids.iter().any(|id| id.as_str() == Some(signature_id)))
        {
            Ok(())
        } else {
            bail!("Stalwart did not confirm DKIM signature retirement")
        }
    }

    async fn find_domain(&self, name: &str) -> Result<Option<String>> {
        let response = self
            .call("x:Domain/query", json!({"filter": {"name": name}}))
            .await?;
        let ids = response
            .get("ids")
            .and_then(Value::as_array)
            .context("Stalwart domain query omitted ids")?;
        for id in ids.iter().filter_map(Value::as_str) {
            let domain = self.get_one("x:Domain/get", id).await?;
            if domain
                .get("name")
                .and_then(Value::as_str)
                .is_some_and(|value| value.eq_ignore_ascii_case(name))
            {
                return Ok(Some(id.to_owned()));
            }
        }
        Ok(None)
    }

    async fn create_domain(&self, name: &str, return_path: &str) -> Result<String> {
        let response = self
            .call(
                "x:Domain/set",
                json!({"create": {"mailer": {
                    "name": name,
                    "aliases": {(return_path): true},
                    "isEnabled": true,
                    "certificateManagement": {"@type": "Manual"},
                    "dkimManagement": {"@type": "Manual"},
                    "dnsManagement": {"@type": "Manual"},
                    "subAddressing": {"@type": "Enabled"}
                }}}),
            )
            .await?;
        created_id(&response, "mailer")
    }

    async fn update_domain(
        &self,
        id: &str,
        enabled: bool,
        return_path: Option<&str>,
    ) -> Result<()> {
        let changes = match return_path {
            Some(value) => json!({"isEnabled": enabled, "aliases": {(value): true}}),
            None => json!({"isEnabled": enabled}),
        };
        let response = self
            .call("x:Domain/set", json!({"update": {(id): changes}}))
            .await?;
        if response
            .get("updated")
            .and_then(Value::as_object)
            .is_some_and(|value| value.contains_key(id))
        {
            Ok(())
        } else {
            ensure_no_set_error(&response)?;
            bail!("Stalwart did not confirm the domain update")
        }
    }

    async fn find_signature(&self, domain_id: &str) -> Result<Option<ProvisionedDomain>> {
        let response = self
            .call(
                "x:DkimSignature/query",
                json!({"filter": {"domainId": domain_id}}),
            )
            .await?;
        let Some(id) = response
            .get("ids")
            .and_then(Value::as_array)
            .and_then(|ids| ids.first())
            .and_then(Value::as_str)
        else {
            return Ok(None);
        };
        self.signature(domain_id, id).await.map(Some)
    }

    async fn find_signature_by_selector(
        &self,
        domain_id: &str,
        selector: &str,
    ) -> Result<Option<ProvisionedDomain>> {
        let response = self
            .call(
                "x:DkimSignature/query",
                json!({"filter": {"domainId": domain_id}}),
            )
            .await?;
        for id in response
            .get("ids")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            let signature = self.signature(domain_id, id).await?;
            if signature.selector == selector {
                return Ok(Some(signature));
            }
        }
        Ok(None)
    }

    async fn signature(&self, domain_id: &str, id: &str) -> Result<ProvisionedDomain> {
        let signature = self.get_one("x:DkimSignature/get", id).await?;
        let selector = signature
            .get("selector")
            .and_then(Value::as_str)
            .context("Stalwart DKIM signature omitted selector")?
            .to_owned();
        let public_key = signature
            .get("publicKey")
            .and_then(Value::as_str)
            .context("Stalwart DKIM signature omitted publicKey")?;
        Ok(ProvisionedDomain {
            domain_id: domain_id.to_owned(),
            signature_id: id.to_owned(),
            selector,
            dkim_value: dkim_value_from_pem(public_key)?,
        })
    }

    async fn create_signature(
        &self,
        domain_id: &str,
        selector: Option<&str>,
    ) -> Result<ProvisionedDomain> {
        let private_key =
            RsaPrivateKey::new(&mut OsRng, 2048).context("unable to generate DKIM key")?;
        let private_pem = private_key.to_pkcs8_pem(LineEnding::LF)?.to_string();
        let public_der = RsaPublicKey::from(&private_key).to_public_key_der()?;
        let selector = selector
            .map(str::to_owned)
            .unwrap_or_else(|| new_selector(domain_id));
        let response = self
            .call(
                "x:DkimSignature/set",
                json!({"create": {"mailer": {
                    "@type": "Dkim1RsaSha256",
                    "domainId": domain_id,
                    "privateKey": {"@type": "Text", "secret": private_pem},
                    "selector": selector
                }}}),
            )
            .await?;
        let signature_id = created_id(&response, "mailer")?;
        Ok(ProvisionedDomain {
            domain_id: domain_id.to_owned(),
            signature_id,
            selector,
            dkim_value: format!("v=DKIM1; k=rsa; p={}", BASE64.encode(public_der.as_bytes())),
        })
    }

    async fn get_one(&self, method: &str, id: &str) -> Result<Value> {
        let response = self.call(method, json!({"ids": [id]})).await?;
        response
            .get("list")
            .and_then(Value::as_array)
            .and_then(|list| list.first())
            .cloned()
            .context("Stalwart object was not found")
    }

    async fn call(&self, method: &str, arguments: Value) -> Result<Value> {
        let body = serde_json::to_vec(&json!({
            "methodCalls": [[method, arguments, "c1"]],
            "using": ["urn:ietf:params:jmap:core", "urn:stalwart:jmap"]
        }))?;
        let connector = HttpsConnectorBuilder::new()
            .with_native_roots()?
            .https_or_http()
            .enable_http1()
            .build();
        let client: HttpClient<_, Full<Bytes>> =
            HttpClient::builder(TokioExecutor::new()).build(connector);
        let request = Request::builder()
            .method(Method::POST)
            .uri(&self.endpoint)
            .header(header::AUTHORIZATION, format!("Bearer {}", self.token))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Full::new(Bytes::from(body)))?;
        let response =
            tokio::time::timeout(Duration::from_secs(15), client.request(request)).await??;
        let status = response.status();
        let bytes = response.into_body().collect().await?.to_bytes();
        if !status.is_success() {
            bail!("Stalwart API returned {status}");
        }
        let payload: Value = serde_json::from_slice(&bytes)?;
        let call = payload
            .get("methodResponses")
            .and_then(Value::as_array)
            .and_then(|calls| calls.first())
            .and_then(Value::as_array)
            .context("invalid Stalwart JMAP response")?;
        if call.first().and_then(Value::as_str) == Some("error") {
            bail!(
                "Stalwart JMAP error: {}",
                call.get(1).unwrap_or(&Value::Null)
            );
        }
        call.get(1)
            .cloned()
            .context("Stalwart JMAP response omitted arguments")
    }
}

fn created_id(response: &Value, key: &str) -> Result<String> {
    ensure_no_set_error(response)?;
    response
        .get("created")
        .and_then(|value| value.get(key))
        .and_then(|value| value.get("id"))
        .and_then(Value::as_str)
        .map(str::to_owned)
        .context("Stalwart did not return a created object id")
}

fn ensure_no_set_error(response: &Value) -> Result<()> {
    if let Some(errors) = response
        .get("notCreated")
        .or_else(|| response.get("notUpdated"))
    {
        if errors.as_object().is_some_and(|value| !value.is_empty()) {
            bail!("Stalwart rejected the change: {errors}");
        }
    }
    Ok(())
}

fn dkim_value_from_pem(public_key: &str) -> Result<String> {
    let encoded: String = public_key
        .lines()
        .filter(|line| !line.starts_with("-----"))
        .map(str::trim)
        .collect();
    BASE64
        .decode(&encoded)
        .context("Stalwart returned an invalid DKIM public key")?;
    Ok(format!("v=DKIM1; k=rsa; p={encoded}"))
}

fn new_selector(seed: &str) -> String {
    let digest = Sha256::digest(format!("{seed}:{}", Uuid::new_v4()).as_bytes());
    format!(
        "cs{}-{}",
        chrono::Utc::now().format("%Y%m%d"),
        hex::encode(&digest[..4])
    )
}

#[cfg(test)]
mod tests {
    use super::{dkim_value_from_pem, new_selector};

    #[test]
    fn converts_public_pem_to_dns_value() {
        let pem = "-----BEGIN PUBLIC KEY-----\nYWJj\n-----END PUBLIC KEY-----";
        assert_eq!(dkim_value_from_pem(pem).unwrap(), "v=DKIM1; k=rsa; p=YWJj");
    }

    #[test]
    fn selector_is_a_dns_label() {
        let selector = new_selector("domain-id");
        assert!(selector.len() < 64);
        assert!(selector
            .chars()
            .all(|value| value.is_ascii_alphanumeric() || value == '-'));
    }
}
