//! 报销附件：把交易的小票/发票图片以 BLOB 存进 SQLite。

use chrono::Utc;
use rusqlite::{params, OptionalExtension};

use super::*;
use crate::domain::Receipt;
use crate::error::{KokuError, Result};
use crate::service::BookkeepingService;

/// 单张附件上限（约 12 MiB），防止请求撑爆内存。
pub const MAX_RECEIPT_BYTES: usize = 12 * 1024 * 1024;

/// 允许作为小票附件存储的 Content-Type 白名单。
///
/// 取图接口会把该值原样设成响应头，且前端以整页导航打开；如果放行
/// `text/html` / `image/svg+xml` 这类可携带脚本的类型，可能被当作同源页面
/// 渲染执行（存储型 XSS）。这里只允许图片与 PDF；随后还会按文件签名校验，
/// 取回响应也会设置 `nosniff`，避免浏览器将伪造内容当作 HTML 执行。
const ALLOWED_RECEIPT_CONTENT_TYPES: &[&str] = &[
    "image/jpeg",
    "image/png",
    "image/webp",
    "image/heic",
    "application/pdf",
];

impl BookkeepingService {
    /// 给交易挂上（或覆盖）一张小票附件。交易必须存在。
    pub fn attach_receipt(
        &mut self,
        transaction_id: i64,
        content_type: String,
        data: Vec<u8>,
    ) -> Result<Receipt> {
        if data.len() > MAX_RECEIPT_BYTES {
            return Err(KokuError::InvalidInput(format!(
                "receipt too large ({} bytes exceeds {MAX_RECEIPT_BYTES})",
                data.len()
            )));
        }
        let content_type = normalize_receipt_content_type(&content_type);
        validate_receipt_content(&content_type, &data)?;
        self.transaction(transaction_id)?;
        self.conn.execute(
            "INSERT INTO receipts(transaction_id, content_type, data, created_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(transaction_id)
             DO UPDATE SET content_type = excluded.content_type, data = excluded.data, created_at = excluded.created_at",
            params![transaction_id, content_type, data, timestamp(Utc::now())],
        )?;
        self.receipt(transaction_id)
    }

    /// 附件的元数据（不含字节）。
    pub fn receipt(&self, transaction_id: i64) -> Result<Receipt> {
        let row = self
            .conn
            .query_row(
                "SELECT transaction_id, content_type, length(data), created_at
                 FROM receipts WHERE transaction_id = ?1",
                [transaction_id],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                },
            )
            .optional()?
            .ok_or(KokuError::NotFound {
                entity: "receipt",
                id: transaction_id,
            })?;
        Ok(Receipt {
            transaction_id: row.0,
            content_type: row.1,
            byte_length: row.2 as usize,
            created_at: parse_timestamp(&row.3)?,
        })
    }

    /// 附件的原始字节与 content-type（供 GET 直接回图）。
    pub fn receipt_bytes(&self, transaction_id: i64) -> Result<(String, Vec<u8>)> {
        let row = self
            .conn
            .query_row(
                "SELECT content_type, data FROM receipts WHERE transaction_id = ?1",
                [transaction_id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?)),
            )
            .optional()?
            .ok_or(KokuError::NotFound {
                entity: "receipt",
                id: transaction_id,
            })?;
        Ok(row)
    }
}

/// 归一化小票的 Content-Type：白名单外的声明降级为普通二进制附件。
///
/// 不拒绝用户保存文件，但绝不允许未知类型以可执行、可渲染 MIME 同源返回。
fn normalize_receipt_content_type(value: &str) -> String {
    let mime = value
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    if ALLOWED_RECEIPT_CONTENT_TYPES.contains(&mime.as_str()) {
        mime
    } else {
        "application/octet-stream".to_owned()
    }
}

/// MIME 头由浏览器客户端提供，不能作为文件类型的唯一依据。这里只做轻量的
/// 魔数校验，以阻止把 HTML/脚本伪装成白名单类型后由取图接口同源返回。
fn validate_receipt_content(content_type: &str, data: &[u8]) -> Result<()> {
    let matches_type = match content_type {
        "image/jpeg" => data.starts_with(&[0xff, 0xd8, 0xff]),
        "image/png" => data.starts_with(b"\x89PNG\r\n\x1a\n"),
        "image/webp" => data.len() >= 12 && &data[..4] == b"RIFF" && &data[8..12] == b"WEBP",
        // HEIC 属于 ISO Base Media File Format；文件类型盒必须位于文件开头。
        "image/heic" => data.len() >= 12 && &data[4..8] == b"ftyp",
        "application/pdf" => data.starts_with(b"%PDF-"),
        // 未知类型已被降级；响应端会强制以 attachment 下载，不在浏览器内渲染。
        "application/octet-stream" => true,
        _ => false,
    };
    if matches_type {
        Ok(())
    } else {
        Err(KokuError::InvalidInput(
            "receipt content does not match its declared content type".to_owned(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_receipt_magic_bytes() {
        assert!(validate_receipt_content("image/png", b"\x89PNG\r\n\x1a\nbody").is_ok());
        assert!(validate_receipt_content("application/pdf", b"%PDF-1.7\n").is_ok());
        assert!(validate_receipt_content("image/webp", b"RIFF1234WEBPbody").is_ok());
        assert!(validate_receipt_content("image/heic", b"0000ftypheic").is_ok());
        assert!(validate_receipt_content("image/jpeg", b"<script>alert(1)</script>").is_err());
        assert!(validate_receipt_content("application/pdf", b"<html></html>").is_err());
    }

    #[test]
    fn downgrades_untrusted_content_types_to_binary() {
        assert_eq!(
            normalize_receipt_content_type("text/html"),
            "application/octet-stream"
        );
        assert_eq!(
            normalize_receipt_content_type("image/svg+xml"),
            "application/octet-stream"
        );
        assert_eq!(
            normalize_receipt_content_type("IMAGE/PNG; charset=binary"),
            "image/png"
        );
    }
}
