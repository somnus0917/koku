//! TOTP 二步验证工具：基于 `totp-rs` 的密钥生成、校验与 otpauth URI 构建。
//!
//! 密钥为 20 字节随机数（160 位，满足 RFC 4226/6238 的最小密钥强度），
//! RFC 4648 Base32 编码（无填充）；校验用 SHA1、6 位、30 秒步长、允许 1 步时钟偏移。

use crate::error::{KokuError, Result};

/// 生成新的 Base32 编码 TOTP 密钥。
pub fn generate_secret_base32() -> Result<String> {
    let mut bytes = [0_u8; 20];
    getrandom::fill(&mut bytes)
        .map_err(|error| KokuError::InvalidInput(format!("rng failure: {error}")))?;
    Ok(base32_encode(&bytes))
}

/// 校验用户输入的 6 位动态码；失败返回 `Ok(false)`，参数非法返回 `Err`。
pub fn verify_code(secret_base32: &str, code: &str) -> Result<bool> {
    let totp = build_totp(secret_base32, None, "")?;
    totp.check_current(code.trim())
        .map_err(|error| KokuError::InvalidInput(format!("totp verification failed: {error}")))
}

/// 生成 otpauth:// 迁移 URI（供 Authenticator 扫码/粘贴）。
pub fn otpauth_uri(secret_base32: &str, issuer: &str, account: &str) -> Result<String> {
    let totp = build_totp(secret_base32, Some(issuer), account)?;
    Ok(totp.get_url())
}

fn build_totp(
    secret_base32: &str,
    issuer: Option<&str>,
    account_name: &str,
) -> Result<totp_rs::TOTP> {
    let secret = totp_rs::Secret::Encoded(secret_base32.trim().to_ascii_uppercase())
        .to_bytes()
        .map_err(|error| KokuError::InvalidInput(format!("invalid totp secret: {error}")))?;
    totp_rs::TOTP::new(
        totp_rs::Algorithm::SHA1,
        6,
        1,
        30,
        secret,
        issuer.map(str::to_owned),
        account_name.to_owned(),
    )
    .map_err(|error| KokuError::InvalidInput(format!("invalid totp parameters: {error}")))
}

/// RFC 4648 Base32 编码（无填充；大写）。
fn base32_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    let mut output = String::with_capacity((bytes.len() * 8).div_ceil(5));
    let mut buffer: u32 = 0;
    let mut bits: u32 = 0;
    for byte in bytes {
        buffer = (buffer << 8) | u32::from(*byte);
        bits += 8;
        while bits >= 5 {
            output.push(ALPHABET[((buffer >> (bits - 5)) & 0x1f) as usize] as char);
            bits -= 5;
        }
    }
    if bits > 0 {
        output.push(ALPHABET[((buffer << (5 - bits)) & 0x1f) as usize] as char);
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base32_encoding_matches_rfc4648() {
        // RFC 4648 第 10 节测试向量（无填充）。
        assert_eq!(base32_encode(b"foobar"), "MZXW6YTBOI");
        assert_eq!(base32_encode(b"foo"), "MZXW6");
        assert_eq!(base32_encode(b""), "");
    }

    #[test]
    fn generated_secret_roundtrips_through_verifier() -> Result<()> {
        let secret = generate_secret_base32()?;
        assert_eq!(secret.len(), 32); // 20 字节 -> 32 个 Base32 字符
                                      // 生成当前动态码并校验通过。
        let totp = build_totp(&secret, None, "")?;
        let code = totp
            .generate_current()
            .map_err(|error| KokuError::InvalidInput(error.to_string()))?;
        assert!(verify_code(&secret, &code)?);
        // 错误动态码校验失败（不抛错）。
        assert!(!verify_code(&secret, "000000")?);
        Ok(())
    }

    #[test]
    fn otpauth_uri_contains_secret_and_issuer() -> Result<()> {
        // 32 字符 Base32 = 160 位，满足最小密钥长度要求。
        let uri = otpauth_uri("OBWGC2LOFVZXI4TJNZTS243FMNZGK5BNGEZDG", "Koku", "somnus")?;
        assert!(uri.starts_with("otpauth://totp/"));
        assert!(uri.contains("secret=OBWGC2LOFVZXI4TJNZTS243FMNZGK5BNGEZDG"));
        assert!(uri.contains("issuer=Koku"));
        Ok(())
    }
}
