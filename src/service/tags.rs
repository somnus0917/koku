//! 标签：跨类目聚合用，交易与标签多对多关联。

use chrono::Utc;
use rusqlite::params;

use super::*;
use crate::domain::Tag;
use crate::error::{KokuError, Result};
use crate::service::BookkeepingService;

impl BookkeepingService {
    /// 设置某笔交易的标签（整体替换）：自动创建不存在的标签、解除已移除的关联。
    /// 返回按输入顺序去重后的标签列表。
    #[cfg(test)]
    pub fn set_transaction_tags(
        &mut self,
        transaction_id: i64,
        names: Vec<String>,
    ) -> Result<Vec<Tag>> {
        self.transaction(transaction_id)?;
        let tx = self.conn.transaction()?;
        let tags = Self::set_transaction_tags_in_tx(&tx, transaction_id, &names)?;
        tx.commit()?;
        Ok(tags)
    }

    /// 事务内：整体替换某笔交易的标签（与 [`BookkeepingService::set_transaction_tags`]
    /// 同语义，供统一编辑路径在同一事务内调用）。
    pub(super) fn set_transaction_tags_in_tx(
        tx: &SqlTransaction<'_>,
        transaction_id: i64,
        names: &[String],
    ) -> Result<Vec<Tag>> {
        let mut seen: Vec<String> = Vec::new();
        for name in names {
            let trimmed = validate_tag_name(name)?;
            if !seen.iter().any(|existing| existing == &trimmed) {
                seen.push(trimmed);
            }
        }

        let now = timestamp(Utc::now());
        let mut tags = Vec::with_capacity(seen.len());
        for name in &seen {
            tx.execute(
                "INSERT INTO tags(name, created_at) VALUES (?1, ?2) ON CONFLICT(name) DO NOTHING",
                params![name, now],
            )?;
            let tag_id = tx.query_row("SELECT id FROM tags WHERE name = ?1", [name], |row| {
                row.get::<_, i64>(0)
            })?;
            tags.push(Tag {
                id: tag_id,
                name: name.clone(),
            });
        }

        tx.execute(
            "DELETE FROM transaction_tags WHERE transaction_id = ?1",
            [transaction_id],
        )?;
        for tag in &tags {
            tx.execute(
                "INSERT OR IGNORE INTO transaction_tags(transaction_id, tag_id) VALUES (?1, ?2)",
                params![transaction_id, tag.id],
            )?;
        }
        // 清理不再被任何交易引用的孤儿标签，避免它们在自动补全里堆积。
        tx.execute(
            "DELETE FROM tags WHERE id NOT IN (SELECT DISTINCT tag_id FROM transaction_tags)",
            [],
        )?;
        Ok(tags)
    }

    /// 全部标签（按名称排序），供表单建议与列表筛选。
    pub fn all_tags(&self) -> Result<Vec<Tag>> {
        let mut statement = self
            .conn
            .prepare("SELECT id, name FROM tags ORDER BY name")?;
        let rows = statement.query_map([], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?;
        rows.map(|row| {
            let (id, name) = row?;
            Ok(Tag { id, name })
        })
        .collect()
    }
}

pub(crate) fn validate_tag_name(name: &str) -> Result<String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(KokuError::InvalidInput(
            "tag name cannot be empty".to_owned(),
        ));
    }
    if trimmed.contains(',') {
        return Err(KokuError::InvalidInput(
            "tag name cannot contain commas".to_owned(),
        ));
    }
    if trimmed.chars().count() > 32 {
        return Err(KokuError::InvalidInput(
            "tag name must be 32 characters or fewer".to_owned(),
        ));
    }
    Ok(trimmed.to_owned())
}
