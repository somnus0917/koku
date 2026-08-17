//! Cloudflare R2 备份上传：S3 兼容 API 的 SigV4 签名客户端（手写实现）。
//!
//! 用途：备份 zip 推送到 R2 实现异地冗余。完全可选——未配置 `KOKU_R2_*`
//! 时应用行为不变（仅本地备份）。
//!
//! 实现说明：R2 只要求 SigV4 的 `PUT/HEAD/GET/DELETE` 与 `x-amz-content-sha256`
//! 头，无需 XML 列表解析；手写签名可保持依赖树最小（仅新增 `hmac`），
//! 并与项目其余"最小自包含"工具（TOTP、限流）风格一致。

use std::time::Duration;

use hmac::{Hmac, Mac};
use sha2_010::{Digest, Sha256};

use crate::error::{KokuError, Result};

type HmacSha256 = Hmac<Sha256>;

/// R2 配置（环境变量）；缺任一必需项时视为未启用。
#[derive(Debug, Clone)]
pub struct R2Config {
    pub account_id: String,
    pub access_key_id: String,
    pub secret_access_key: String,
    pub bucket: String,
    /// 对象前缀（如 `koku`），对象键为 `{prefix}/{备份文件名}`。
    pub prefix: String,
}

impl R2Config {
    /// 从环境变量解析；未配置 `KOKU_R2_ACCOUNT_ID` 时返回 `Ok(None)`。
    /// 配置了 Account ID 但缺少其余必填项时报错（配置不完整）。
    pub fn from_env() -> Result<Option<Self>> {
        let account_id = match std::env::var("KOKU_R2_ACCOUNT_ID") {
            Ok(value) if !value.trim().is_empty() => value.trim().to_owned(),
            Ok(_) | Err(std::env::VarError::NotPresent) => return Ok(None),
            Err(error) => {
                return Err(KokuError::AuthConfiguration(format!(
                    "could not read KOKU_R2_ACCOUNT_ID: {error}"
                )))
            }
        };
        let required = |name: &str| -> Result<String> {
            std::env::var(name)
                .map(|value| value.trim().to_owned())
                .map_err(|_| {
                    KokuError::AuthConfiguration(format!(
                        "{name} is required when KOKU_R2_ACCOUNT_ID is set"
                    ))
                })
        };
        Ok(Some(Self {
            account_id,
            access_key_id: required("KOKU_R2_ACCESS_KEY_ID")?,
            secret_access_key: required("KOKU_R2_SECRET_ACCESS_KEY")?,
            bucket: required("KOKU_R2_BUCKET")?,
            prefix: std::env::var("KOKU_R2_PREFIX")
                .unwrap_or_else(|_| "koku".to_owned())
                .trim()
                .trim_matches('/')
                .to_owned(),
        }))
    }
}

/// R2 S3 客户端（复用 reqwest 连接池）。
#[derive(Debug, Clone)]
pub struct R2Client {
    pub config: R2Config,
    pub(crate) endpoint: String,
    host: String,
    http: reqwest::Client,
}

impl R2Client {
    pub fn new(config: R2Config) -> Self {
        let host = format!("{}.r2.cloudflarestorage.com", config.account_id);
        Self {
            endpoint: format!("https://{host}"),
            host,
            config,
            http: reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(10))
                .timeout(Duration::from_secs(120))
                .user_agent("koku/0.1 (personal ledger backup)")
                .build()
                .expect("failed to build r2 http client"),
        }
    }

    /// 对象完整键：`{prefix}/{filename}`（prefix 为空时直接用 filename）。
    pub fn object_key(&self, filename: &str) -> String {
        if self.config.prefix.is_empty() {
            filename.to_owned()
        } else {
            format!("{}/{}", self.config.prefix, filename)
        }
    }

    /// R2 是 path-style：URL 与 SigV4 canonical URI 都要带桶名（`/{bucket}/{key}`）。
    fn canonical_uri(&self, key: &str) -> String {
        format!("/{}/{}", self.config.bucket, key)
    }

    /// 上传对象（PUT，带 SigV4 签名与 Content-SHA256）。
    pub async fn put_object(&self, key: &str, bytes: &[u8], content_type: &str) -> Result<()> {
        let now = UtcNow::new();
        let payload_hash = hex_encode(&Sha256::digest(bytes));
        let headers = vec![
            ("host", self.host.clone()),
            ("x-amz-content-sha256", payload_hash.clone()),
            ("x-amz-date", now.amz_date()),
            ("content-type", content_type.to_owned()),
        ];
        let authorization = self.authorization("PUT", key, "", &headers, &payload_hash, &now)?;
        let response = self
            .http
            .put(format!("{}{}", self.endpoint, self.canonical_uri(key)))
            .header("x-amz-content-sha256", payload_hash)
            .header("x-amz-date", now.amz_date())
            .header("content-type", content_type)
            .header("authorization", authorization)
            .body(bytes.to_vec())
            .send()
            .await
            .map_err(|error| KokuError::RateSource(format!("r2 put failed: {error}")))?;
        ensure_success(response, "r2 put").await
    }

    /// 检查对象是否存在；返回对象大小（不存在返回 None）。
    pub async fn head_object(&self, key: &str) -> Result<Option<u64>> {
        let now = UtcNow::new();
        let payload_hash = EMPTY_SHA256.to_owned();
        let headers = vec![
            ("host", self.host.clone()),
            ("x-amz-content-sha256", payload_hash.clone()),
            ("x-amz-date", now.amz_date()),
        ];
        let authorization = self.authorization("HEAD", key, "", &headers, &payload_hash, &now)?;
        let response = self
            .http
            .head(format!("{}{}", self.endpoint, self.canonical_uri(key)))
            .header("x-amz-content-sha256", payload_hash)
            .header("x-amz-date", now.amz_date())
            .header("authorization", authorization)
            .send()
            .await
            .map_err(|error| KokuError::RateSource(format!("r2 head failed: {error}")))?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !response.status().is_success() {
            return Err(KokuError::RateSource(format!(
                "r2 head returned HTTP {}",
                response.status()
            )));
        }
        Ok(response
            .headers()
            .get(reqwest::header::CONTENT_LENGTH)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse().ok()))
    }

    /// 下载对象。
    pub async fn get_object(&self, key: &str) -> Result<Vec<u8>> {
        let now = UtcNow::new();
        let payload_hash = EMPTY_SHA256.to_owned();
        let headers = vec![
            ("host", self.host.clone()),
            ("x-amz-content-sha256", payload_hash.clone()),
            ("x-amz-date", now.amz_date()),
        ];
        let authorization = self.authorization("GET", key, "", &headers, &payload_hash, &now)?;
        let response = self
            .http
            .get(format!("{}{}", self.endpoint, self.canonical_uri(key)))
            .header("x-amz-content-sha256", payload_hash)
            .header("x-amz-date", now.amz_date())
            .header("authorization", authorization)
            .send()
            .await
            .map_err(|error| KokuError::RateSource(format!("r2 get failed: {error}")))?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response
                .text()
                .await
                .unwrap_or_default()
                .chars()
                .take(300)
                .collect::<String>();
            return Err(KokuError::RateSource(format!(
                "r2 get returned HTTP {status}: {body}"
            )));
        }
        let bytes = response
            .bytes()
            .await
            .map_err(|error| KokuError::RateSource(format!("r2 get body failed: {error}")))?;
        Ok(bytes.to_vec())
    }

    /// 删除对象。
    pub async fn delete_object(&self, key: &str) -> Result<()> {
        let now = UtcNow::new();
        let payload_hash = EMPTY_SHA256.to_owned();
        let headers = vec![
            ("host", self.host.clone()),
            ("x-amz-content-sha256", payload_hash.clone()),
            ("x-amz-date", now.amz_date()),
        ];
        let authorization = self.authorization("DELETE", key, "", &headers, &payload_hash, &now)?;
        let response = self
            .http
            .delete(format!("{}{}", self.endpoint, self.canonical_uri(key)))
            .header("x-amz-content-sha256", payload_hash)
            .header("x-amz-date", now.amz_date())
            .header("authorization", authorization)
            .send()
            .await
            .map_err(|error| KokuError::RateSource(format!("r2 delete failed: {error}")))?;
        ensure_success(response, "r2 delete").await
    }

    /// 构造 SigV4 Authorization 头。
    fn authorization(
        &self,
        method: &str,
        key: &str,
        canonical_query: &str,
        headers: &[(&str, String)],
        payload_hash: &str,
        now: &UtcNow,
    ) -> Result<String> {
        // SigV4 要求 canonical headers 与 SignedHeaders 都按（小写）名称字典序排列。
        let mut sorted: Vec<(String, String)> = headers
            .iter()
            .map(|(name, value)| (name.to_lowercase(), value.trim().to_owned()))
            .collect();
        sorted.sort_by(|a, b| a.0.cmp(&b.0));
        let canonical_headers: String = sorted
            .iter()
            .map(|(name, value)| format!("{name}:{value}\n"))
            .collect();
        let signed_headers: Vec<&str> = sorted.iter().map(|(name, _)| name.as_str()).collect();
        let signed_headers = signed_headers.join(";");
        let canonical_uri = self.canonical_uri(key);
        let canonical_request = format!(
            "{method}\n{canonical_uri}\n{canonical_query}\n{canonical_headers}\n{signed_headers}\n{payload_hash}"
        );
        let string_to_sign = format!(
            "AWS4-HMAC-SHA256\n{}\n{}/auto/s3/aws4_request\n{}",
            now.amz_date(),
            now.date_stamp(),
            hex_encode(&Sha256::digest(canonical_request.as_bytes()))
        );
        let signature = hex_encode(&sign(&self.signing_key(now), string_to_sign.as_bytes()));
        Ok(format!(
            "AWS4-HMAC-SHA256 Credential={}/{}/auto/s3/aws4_request, SignedHeaders={signed_headers}, Signature={signature}",
            self.config.access_key_id,
            now.date_stamp(),
        ))
    }

    fn signing_key(&self, now: &UtcNow) -> Vec<u8> {
        let date_key = sign(
            format!("AWS4{}", self.config.secret_access_key).as_bytes(),
            now.date_stamp().as_bytes(),
        );
        let region_key = sign(&date_key, b"auto");
        let service_key = sign(&region_key, b"s3");
        sign(&service_key, b"aws4_request")
    }
}

async fn ensure_success(response: reqwest::Response, context: &str) -> Result<()> {
    if !response.status().is_success() {
        let status = response.status();
        let body = response
            .text()
            .await
            .unwrap_or_default()
            .chars()
            .take(300)
            .collect::<String>();
        return Err(KokuError::RateSource(format!(
            "{context} returned HTTP {status}: {body}"
        )));
    }
    Ok(())
}

/// SHA-256 空负载哈希（HEAD/GET/DELETE 用）。
const EMPTY_SHA256: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

struct UtcNow {
    date_stamp: String,
    amz_date: String,
}

impl UtcNow {
    fn new() -> Self {
        let now = chrono::Utc::now();
        Self {
            date_stamp: now.format("%Y%m%d").to_string(),
            amz_date: now.format("%Y%m%dT%H%M%SZ").to_string(),
        }
    }

    fn date_stamp(&self) -> &str {
        &self.date_stamp
    }

    fn amz_date(&self) -> String {
        self.amz_date.clone()
    }
}

fn sign(key: &[u8], message: &[u8]) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(key).expect("hmac accepts any key length");
    mac.update(message);
    mac.finalize().into_bytes().to_vec()
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

/// 删除 R2 上超出保留份数的旧对象（与本地 `KOKU_BACKUP_KEEP` 对齐），
/// 幂等且失败仅告警（不影响本地备份）。
pub async fn prune_old_objects(r2: &R2Client, db_path: &std::path::Path, keep: usize) {
    let Ok(backups) = crate::backup::list_backups(db_path) else {
        return;
    };
    for old in backups.iter().skip(keep) {
        let key = r2.object_key(&old.filename);
        if let Err(error) = r2.delete_object(&key).await {
            tracing::warn!(target: "koku", key = %key, error = %error, "r2 prune delete failed");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_client() -> R2Client {
        R2Client::new(R2Config {
            account_id: "b4749c86984f7a88f573e4e307834846".to_owned(),
            access_key_id: "test-access-key".to_owned(),
            secret_access_key: "test-secret-key".to_owned(),
            bucket: "backups".to_owned(),
            prefix: "koku".to_owned(),
        })
    }

    #[test]
    fn object_key_joins_prefix() {
        let client = test_client();
        assert_eq!(
            client.object_key("koku-20260101-120000.zip"),
            "koku/koku-20260101-120000.zip"
        );
        let mut no_prefix = test_client();
        no_prefix.config.prefix = "".to_owned();
        assert_eq!(no_prefix.object_key("a.zip"), "a.zip");
    }

    #[test]
    fn hmac_sha256_signature_matches_rfc_test_vector() {
        // RFC 4231 Test Case 1：HMAC-SHA256(key="0b"*20, data="Hi There")。
        let key = [0x0b_u8; 20];
        let signature = sign(&key, b"Hi There");
        let expected = "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7";
        assert_eq!(hex_encode(&signature), expected);
    }

    #[test]
    fn config_is_disabled_without_account_id() -> Result<()> {
        std::env::remove_var("KOKU_R2_ACCOUNT_ID");
        assert!(R2Config::from_env()?.is_none());
        Ok(())
    }

    #[test]
    fn config_requires_full_credentials_when_enabled() {
        std::env::set_var("KOKU_R2_ACCOUNT_ID", "abc");
        std::env::remove_var("KOKU_R2_ACCESS_KEY_ID");
        std::env::remove_var("KOKU_R2_SECRET_ACCESS_KEY");
        std::env::remove_var("KOKU_R2_BUCKET");
        assert!(R2Config::from_env().is_err());
        std::env::set_var("KOKU_R2_ACCESS_KEY_ID", "ak");
        std::env::set_var("KOKU_R2_SECRET_ACCESS_KEY", "sk");
        std::env::set_var("KOKU_R2_BUCKET", "bk");
        let config = R2Config::from_env().unwrap().unwrap();
        assert_eq!(config.prefix, "koku");
        std::env::remove_var("KOKU_R2_ACCOUNT_ID");
        std::env::remove_var("KOKU_R2_BUCKET");
        std::env::remove_var("KOKU_R2_ACCESS_KEY_ID");
        std::env::remove_var("KOKU_R2_SECRET_ACCESS_KEY");
    }
}

#[cfg(test)]
mod live_tests {
    use super::*;

    /// 真实 R2 连通性测试：PUT → HEAD → GET → DELETE。
    /// 需要环境变量 KOKU_R2_*；默认忽略（CI 不跑），本地用 --ignored 手动执行。
    #[tokio::test]
    #[ignore]
    async fn live_put_head_get_delete_roundtrip() -> Result<()> {
        let config = R2Config::from_env()?.expect("KOKU_R2_* must be set for live test");
        let client = R2Client::new(config);
        let key = format!("koku/.sigv4-live-test-{}", std::process::id());
        let payload = b"koku-r2-sigv4-live-test-payload";

        client.put_object(&key, payload, "text/plain").await?;
        let size = client
            .head_object(&key)
            .await?
            .expect("object should exist");
        assert_eq!(size, payload.len() as u64);
        let fetched = client.get_object(&key).await?;
        assert_eq!(fetched, payload);
        client.delete_object(&key).await?;
        assert_eq!(client.head_object(&key).await?, None);
        Ok(())
    }
}
