//! 用户可见的账本活动轨迹。

use chrono::Utc;
use rusqlite::params;

use super::*;
use crate::domain::ActivityEvent;
use crate::error::Result;
use crate::service::BookkeepingService;

impl BookkeepingService {
    /// 记录一次已经完成的账本操作。实体允许后续删除，轨迹仍保留其摘要。
    pub fn record_activity(
        &mut self,
        action: impl Into<String>,
        entity_type: impl Into<String>,
        entity_id: i64,
        summary: impl Into<String>,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO activity_events(action, entity_type, entity_id, summary, occurred_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![action.into(), entity_type.into(), entity_id, summary.into(), timestamp(Utc::now())],
        )?;
        Ok(())
    }

    pub fn activity_events(&self, limit: u32) -> Result<Vec<ActivityEvent>> {
        let mut statement = self.conn.prepare(
            "SELECT id, action, entity_type, entity_id, summary, occurred_at
             FROM activity_events ORDER BY occurred_at DESC, id DESC LIMIT ?1",
        )?;
        let rows = statement.query_map([i64::from(limit.clamp(1, 200))], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
            ))
        })?;
        let mut events = Vec::new();
        for row in rows {
            let (id, action, entity_type, entity_id, summary, occurred_at) = row?;
            events.push(ActivityEvent {
                id,
                action,
                entity_type,
                entity_id,
                summary,
                occurred_at: parse_timestamp(&occurred_at)?,
            });
        }
        Ok(events)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newest_activity_appears_first_and_is_limited() -> Result<()> {
        let mut service = BookkeepingService::in_memory()?;
        service.record_activity("transaction.created", "transaction", 1, "第一笔")?;
        service.record_activity("transaction.updated", "transaction", 1, "已修改")?;
        let events = service.activity_events(1)?;
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].action, "transaction.updated");
        Ok(())
    }
}
