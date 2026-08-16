//! 报销附件：把交易的小票/发票图片以 BLOB 存进 SQLite。

use chrono::Utc;
use rusqlite::{params, OptionalExtension};

use super::*;
use crate::domain::Receipt;
use crate::error::{KokuError, Result};
use crate::service::BookkeepingService;

/// 单张附件上限（约 12 MiB），防止请求撑爆内存。
pub const MAX_RECEIPT_BYTES: usize = 12 * 1024 * 1024;

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
