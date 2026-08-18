//! Payee 商户/收款方与自动分类学习：字符串归一化、别名学习、历史分类统计与预测。
//!
//! 设计原则（第一版）：
//! - 不做模糊匹配/编辑距离/AI；只做保守、可解释的字符串归一化。
//! - 学习只发生在用户明确操作（保存/纠正交易）时；导入只消费已有知识。
//! - 阈值集中定义在下方常量中，不散落 magic number。

use chrono::Utc;
use rusqlite::{params, OptionalExtension};
use rust_decimal::Decimal;

use super::*;
use crate::domain::{CategoryPrediction, Payee};
use crate::error::{KokuError, Result};

/// 自动应用分类所需的最少历史样本数。
pub const AUTO_CATEGORY_MIN_SAMPLES: u64 = 3;
/// 自动应用分类的置信度阈值（>= 该值且样本足够时自动应用）。
pub const AUTO_CATEGORY_CONFIDENCE: Decimal = Decimal::from_parts(95, 0, 0, false, 2); // 0.95
/// 返回分类建议的置信度下限（>= 该值但未达自动阈值时返回建议，不自动确认）。
pub const SUGGEST_CATEGORY_CONFIDENCE: Decimal = Decimal::from_parts(70, 0, 0, false, 2); // 0.70

/// 常见支付渠道前缀：识别前剥掉，避免同一商户因渠道不同被拆成多个 alias。
const CHANNEL_PREFIXES: &[&str] = &["支付宝-", "微信支付-", "微信-", "财付通-"];

/// 归一化商户/收款方名称：trim + 全角空格转半角 + 合并连续空白。
fn normalize_payee_name(input: &str) -> String {
    input
        .trim()
        .replace('　', " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// 归一化交易描述（保守、可解释）：
/// 1. trim + 全角空格转半角 + 合并连续空白
/// 2. 去掉常见支付渠道前缀（如 `支付宝-`）
/// 3. 去掉明显是订单号/流水号的尾部数字串（尾部整段纯数字 >= 6 位，或尾随连续数字 >= 8 位）
///
/// 不引入编辑距离、模糊匹配或外部商户库；同一描述必须稳定得到同一结果。
fn normalize_description(input: &str) -> String {
    let collapsed = normalize_payee_name(input);
    let mut s = collapsed.as_str();
    for prefix in CHANNEL_PREFIXES {
        if let Some(rest) = s.strip_prefix(prefix) {
            s = rest.trim_start();
            break;
        }
    }
    // 尾部整段为纯数字（如 "订单 20260815"）→ 去掉该段。
    let mut pieces: Vec<&str> = s.split(' ').collect();
    if pieces.len() > 1 {
        if let Some(last) = pieces.last() {
            if last.len() >= 6 && last.chars().all(|ch| ch.is_ascii_digit()) {
                pieces.pop();
            }
        }
    }
    let joined = pieces.join(" ");
    s = &joined;
    // 结尾连续数字 >= 8 位（如 "上海拉扎斯信息科技有限公司20260815001"）→ 去掉。
    let trimmed_end = s.trim_end_matches(|ch: char| ch.is_ascii_digit());
    if s.len() - trimmed_end.len() >= 8 {
        s = trimmed_end.trim_end();
    }
    s.to_owned()
}

impl BookkeepingService {
    /// 搜索 Payee（名称包含查询词，按名称排序；空查询返回全部）。自动补全用。
    pub fn search_payees(&self, query: &str, limit: u32) -> Result<Vec<Payee>> {
        let limit = limit.clamp(1, 200);
        let pattern = format!("%{}%", query.trim());
        let mut statement = self.conn.prepare(
            "SELECT id, name, created_at FROM payees WHERE name LIKE ?1 ORDER BY name, id LIMIT ?2",
        )?;
        let rows = statement.query_map(params![pattern, limit], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        let mut result = Vec::new();
        for row in rows {
            let (id, name, created_at) = row?;
            result.push(Payee {
                id,
                name,
                created_at: parse_timestamp(&created_at)?,
            });
        }
        Ok(result)
    }

    /// 按名称取得 Payee，不存在时创建（名称做保守归一化：trim + 合并空白）。
    /// 名称唯一，同一用户的账本内去重。
    pub fn get_or_create_payee(&mut self, name: &str) -> Result<Payee> {
        let name = normalize_payee_name(name);
        if name.is_empty() {
            return Err(KokuError::InvalidInput(
                "payee name cannot be empty".to_owned(),
            ));
        }
        if let Some((id, stored_name, created_at)) = self
            .conn
            .query_row(
                "SELECT id, name, created_at FROM payees WHERE name = ?1",
                [&name],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .optional()?
        {
            return Ok(Payee {
                id,
                name: stored_name,
                created_at: parse_timestamp(&created_at)?,
            });
        }
        let created_at = Utc::now();
        self.conn.execute(
            "INSERT INTO payees(name, created_at) VALUES (?1, ?2)",
            params![&name, timestamp(created_at)],
        )?;
        Ok(Payee {
            id: self.conn.last_insert_rowid(),
            name,
            created_at,
        })
    }

    /// 把用户确认的 Payee 应用到交易。
    ///
    /// - `payee_name`: `Some(非空)` = 设置（查找/创建）；`Some(空串)` = 清除 Payee；
    ///   `None` = 保持不变。
    /// - 仅当 Payee 真正变化时才更新交易并学习 alias（防止改 note/金额反复累加）。
    /// - 分类学习不在此处进行：保存完成后由调用方统一调用
    ///   [`BookkeepingService::confirm_transaction_learning`]（可追踪、幂等）。
    pub fn set_transaction_payee(
        &mut self,
        transaction_id: i64,
        payee_name: Option<&str>,
    ) -> Result<Transaction> {
        let transaction = self.transaction(transaction_id)?;
        let resolved = match payee_name.map(str::trim) {
            Some(name) if !name.is_empty() => Some(self.get_or_create_payee(name)?.id),
            _ => None,
        };
        if resolved != transaction.payee_id {
            if let Some(payee_id) = resolved {
                if let Some(raw) = &transaction.raw_description {
                    self.learn_alias(raw, payee_id)?;
                }
            }
            self.conn.execute(
                "UPDATE transactions SET payee_id = ?1 WHERE id = ?2",
                params![resolved, transaction_id],
            )?;
        }
        self.transaction(transaction_id)
    }

    /// 导入专用：把机器识别出的原始描述与 Payee 写回交易（不触发学习）。
    pub(crate) fn set_import_metadata(
        &mut self,
        transaction_id: i64,
        raw_description: &str,
        payee_id: Option<i64>,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE transactions SET raw_description = ?1, payee_id = ?2 WHERE id = ?3",
            params![raw_description, payee_id, transaction_id],
        )?;
        Ok(())
    }

    /// 记录「原始描述 → Payee」的一次人工确认：归一化描述后写入/更新 alias。
    ///
    /// - 描述不可归一化（为空）时忽略，不产生 alias。
    /// - 已存在同一归一化描述时视为纠正：目标 Payee 改为最新确认值，
    ///   同时 `confirmed_count` +1、`last_used_at` 更新。
    pub fn learn_alias(&mut self, raw_description: &str, payee_id: i64) -> Result<()> {
        let normalized = normalize_description(raw_description);
        if normalized.is_empty() {
            return Ok(());
        }
        let now = Utc::now();
        self.conn.execute(
            "INSERT INTO merchant_aliases(normalized_description, payee_id, confirmed_count, last_used_at, created_at)
             VALUES (?1, ?2, 1, ?3, ?3)
             ON CONFLICT(normalized_description) DO UPDATE SET
                 payee_id = excluded.payee_id,
                 confirmed_count = merchant_aliases.confirmed_count + 1,
                 last_used_at = excluded.last_used_at",
            params![normalized, payee_id, timestamp(now)],
        )?;
        Ok(())
    }

    /// 按原始描述识别 Payee：归一化后查 alias 表；命中时刷新 `last_used_at`。
    pub fn resolve_payee_for_description(&self, raw_description: &str) -> Result<Option<Payee>> {
        let normalized = normalize_description(raw_description);
        if normalized.is_empty() {
            return Ok(None);
        }
        let hit = self
            .conn
            .query_row(
                "SELECT p.id, p.name, p.created_at
                 FROM merchant_aliases a JOIN payees p ON p.id = a.payee_id
                 WHERE a.normalized_description = ?1",
                [&normalized],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .optional()?;
        let Some((id, name, created_at)) = hit else {
            return Ok(None);
        };
        self.conn.execute(
            "UPDATE merchant_aliases SET last_used_at = ?1 WHERE normalized_description = ?2",
            params![timestamp(Utc::now()), normalized],
        )?;
        Ok(Some(Payee {
            id,
            name,
            created_at: parse_timestamp(&created_at)?,
        }))
    }

    /// 确认一笔交易当前的 (Payee, Category) 为学习样本（幂等）。
    ///
    /// 每一笔「人工确认参与学习的交易」最多贡献一份 Payee → Category 样本：
    /// 1. 读取交易当前的 payee_id / category_id；
    /// 2. 若该交易之前贡献过旧样本（`transaction_learning`），先撤销旧贡献
    ///    （`payee_category_stats.count - 1`，减到 0 删除该行）；
    /// 3. 若当前 Payee + Category 都存在，给新组合 +1；
    /// 4. 更新 `transaction_learning`（当前组合与已记录一致时直接返回，不重复累加）。
    pub fn confirm_transaction_learning(&mut self, transaction_id: i64) -> Result<()> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let transaction = Self::transaction_in_tx(&tx, transaction_id)?;
        let current_pair = match (transaction.payee_id, transaction.category_id) {
            (Some(payee_id), Some(category_id)) => Some((payee_id, category_id)),
            _ => None,
        };
        let old: Option<(i64, i64)> = tx
            .query_row(
                "SELECT payee_id, category_id FROM transaction_learning WHERE transaction_id = ?1",
                [transaction_id],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
            )
            .optional()?;
        // 幂等：当前组合与已记录一致 → 无变化，不重复累加。
        if old == current_pair {
            return Ok(());
        }
        // 撤销旧贡献。
        if let Some((old_payee, old_category)) = old {
            tx.execute(
                "UPDATE payee_category_stats SET count = count - 1
                 WHERE payee_id = ?1 AND category_id = ?2",
                params![old_payee, old_category],
            )?;
            tx.execute(
                "DELETE FROM payee_category_stats
                 WHERE payee_id = ?1 AND category_id = ?2 AND count <= 0",
                params![old_payee, old_category],
            )?;
        }
        // 加入新贡献（或清除 Payee 时撤销后不新增）。
        match current_pair {
            Some((payee_id, category_id)) => {
                let now = timestamp(Utc::now());
                tx.execute(
                    "INSERT INTO payee_category_stats(payee_id, category_id, count, last_used_at)
                     VALUES (?1, ?2, 1, ?3)
                     ON CONFLICT(payee_id, category_id) DO UPDATE SET
                         count = payee_category_stats.count + 1,
                         last_used_at = excluded.last_used_at",
                    params![payee_id, category_id, now],
                )?;
                tx.execute(
                    "INSERT INTO transaction_learning(transaction_id, payee_id, category_id, updated_at)
                     VALUES (?1, ?2, ?3, ?4)
                     ON CONFLICT(transaction_id) DO UPDATE SET
                         payee_id = excluded.payee_id,
                         category_id = excluded.category_id,
                         updated_at = excluded.updated_at",
                    params![transaction_id, payee_id, category_id, now],
                )?;
            }
            None => {
                tx.execute(
                    "DELETE FROM transaction_learning WHERE transaction_id = ?1",
                    [transaction_id],
                )?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// 按 Payee 历史分类统计预测分类。
    ///
    /// 返回 `None` 的三种情况：无统计、总样本数不足 [`AUTO_CATEGORY_MIN_SAMPLES`]、
    /// 置信度低于 [`SUGGEST_CATEGORY_CONFIDENCE`]。已归档分类不计入统计。
    pub fn predict_category(&self, payee_id: i64) -> Result<Option<CategoryPrediction>> {
        let total: u64 = self.conn.query_row(
            "SELECT COALESCE(SUM(count), 0) FROM payee_category_stats WHERE payee_id = ?1",
            [payee_id],
            |row| row.get(0),
        )?;
        if total < AUTO_CATEGORY_MIN_SAMPLES {
            return Ok(None);
        }
        let top = self
            .conn
            .query_row(
                "SELECT s.category_id, c.name, s.count
                 FROM payee_category_stats s
                 JOIN categories c ON c.id = s.category_id AND c.archived_at IS NULL
                 WHERE s.payee_id = ?1
                 ORDER BY s.count DESC, s.category_id
                 LIMIT 1",
                [payee_id],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, u64>(2)?,
                    ))
                },
            )
            .optional()?;
        let Some((category_id, category_name, top_count)) = top else {
            return Ok(None);
        };
        let confidence = Decimal::from(top_count) / Decimal::from(total);
        let auto_applied = confidence >= AUTO_CATEGORY_CONFIDENCE;
        if !auto_applied && confidence < SUGGEST_CATEGORY_CONFIDENCE {
            return Ok(None);
        }
        Ok(Some(CategoryPrediction {
            category_id,
            category_name,
            confidence,
            auto_applied,
        }))
    }

    /// 清除全部自动分类学习数据（merchant_aliases / payee_category_stats /
    /// transaction_learning）。
    ///
    /// 不删除 Payee、不删除交易、不修改已有交易分类——仅让后续不再复用旧知识。
    pub fn clear_payee_learning(&mut self) -> Result<()> {
        self.conn.execute("DELETE FROM merchant_aliases", [])?;
        self.conn.execute("DELETE FROM payee_category_stats", [])?;
        self.conn.execute("DELETE FROM transaction_learning", [])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::AccountType;
    use rusqlite::Connection;
    use rust_decimal::Decimal;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// 建一个带默认分类的账本，返回 (service, 餐饮, 购物) 分类 id。
    fn seeded() -> Result<(BookkeepingService, i64, i64)> {
        let mut service = BookkeepingService::in_memory()?;
        service.ensure_default_categories()?;
        let food = service
            .categories()?
            .into_iter()
            .find(|item| item.name == "餐饮")
            .unwrap();
        let shopping = service
            .categories()?
            .into_iter()
            .find(|item| item.name == "购物")
            .unwrap();
        Ok((service, food.id, shopping.id))
    }

    /// 通过真实交易确认一笔 (Payee, Category) 学习样本，返回交易 id。
    /// 绕过 alias：直接设置 payee_id（测试只关心分类统计）。
    fn confirm_tx(
        service: &mut BookkeepingService,
        payee_id: i64,
        category_id: i64,
    ) -> Result<i64> {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let account = service.create_account(
            format!("学习账户-{n}"),
            AccountType::Cash,
            "CNY",
            Decimal::from(100000_u32),
        )?;
        let tx = service.record_expense(
            account.id,
            category_id,
            Decimal::from(10_u32),
            chrono::Utc::now(),
            "学习样本",
        )?;
        service.conn.execute(
            "UPDATE transactions SET payee_id = ?1 WHERE id = ?2",
            params![payee_id, tx.id],
        )?;
        service.confirm_transaction_learning(tx.id)?;
        Ok(tx.id)
    }

    /// 读取某 (Payee, Category) 的统计计数；无记录返回 0。
    fn stats_count(service: &BookkeepingService, payee_id: i64, category_id: i64) -> Result<u64> {
        Ok(service
            .conn
            .query_row(
                "SELECT count FROM payee_category_stats WHERE payee_id = ?1 AND category_id = ?2",
                params![payee_id, category_id],
                |row| row.get::<_, u64>(0),
            )
            .optional()?
            .unwrap_or(0))
    }

    #[test]
    fn payee_is_created_and_deduplicated_by_normalized_name() -> Result<()> {
        let mut service = BookkeepingService::in_memory()?;
        let first = service.get_or_create_payee(" 饿了么 ")?;
        let second = service.get_or_create_payee("饿了么")?;
        assert_eq!(first.id, second.id);
        assert_eq!(first.name, "饿了么");
        // 全角空格与连续空白同样归并。
        let third = service.get_or_create_payee("星巴克　咖啡")?;
        let fourth = service.get_or_create_payee("星巴克  咖啡")?;
        assert_eq!(third.id, fourth.id);
        assert_eq!(third.name, "星巴克 咖啡");
        assert_eq!(service.search_payees("", 100)?.len(), 2);
        Ok(())
    }

    #[test]
    fn payee_name_cannot_be_blank() -> Result<()> {
        let mut service = BookkeepingService::in_memory()?;
        assert!(service.get_or_create_payee("   ").is_err());
        Ok(())
    }

    #[test]
    fn payee_search_matches_substring() -> Result<()> {
        let mut service = BookkeepingService::in_memory()?;
        service.get_or_create_payee("麦当劳")?;
        service.get_or_create_payee("麦德龙")?;
        service.get_or_create_payee("星巴克")?;
        let hits = service.search_payees("麦", 10)?;
        let names: Vec<&str> = hits.iter().map(|payee| payee.name.as_str()).collect();
        assert_eq!(names, vec!["麦当劳", "麦德龙"]);
        Ok(())
    }

    #[test]
    fn normalize_description_is_stable_and_removes_noise() {
        // 同一商户、不同订单号 → 同一归一化结果。
        let a = normalize_description("支付宝-上海拉扎斯信息科技有限公司20260815001");
        let b = normalize_description("支付宝-上海拉扎斯信息科技有限公司20260901002");
        assert_eq!(a, b);
        assert_eq!(a, "上海拉扎斯信息科技有限公司");
        // trim 与全角空格。
        assert_eq!(normalize_description("  微信支付- 饿了么  "), "饿了么");
        assert_eq!(normalize_description("支付宝-京东　商城"), "京东 商城");
        // 纯数字尾段去掉。
        assert_eq!(normalize_description("星巴克 20260815"), "星巴克");
        // 短数字尾缀（如 iPhone 15）不受影响。
        assert_eq!(normalize_description("iPhone 15"), "iPhone 15");
        // 无前缀无订单号 → 原样。
        assert_eq!(normalize_description("全家便利店"), "全家便利店");
    }

    #[test]
    fn normalize_description_is_pure_and_total() {
        for input in [
            "支付宝-上海拉扎斯信息科技有限公司",
            " 微信支付-京东商城 ",
            "财付通-深圳腾讯计算机系统有限公司",
            "7-Eleven 20260815001",
        ] {
            let once = normalize_description(input);
            assert_eq!(
                once,
                normalize_description(input),
                "normalize must be stable"
            );
            assert_eq!(
                once,
                normalize_description(&once),
                "normalize must be idempotent"
            );
        }
    }

    #[test]
    fn confirmed_description_saves_alias_and_recognizes_next_same_row() -> Result<()> {
        let mut service = BookkeepingService::in_memory()?;
        let eleme = service.get_or_create_payee("饿了么")?;
        // 用户第一次处理：把原始描述确认给「饿了么」。
        service.learn_alias("支付宝-上海拉扎斯信息科技有限公司20260815001", eleme.id)?;
        // 下一次相同描述（不同订单号）自动识别。
        let recognized = service
            .resolve_payee_for_description("支付宝-上海拉扎斯信息科技有限公司20260901002")?
            .unwrap();
        assert_eq!(recognized.id, eleme.id);
        assert_eq!(recognized.name, "饿了么");
        Ok(())
    }

    #[test]
    fn correcting_payee_updates_the_alias_target() -> Result<()> {
        let mut service = BookkeepingService::in_memory()?;
        let eleme = service.get_or_create_payee("饿了么")?;
        let jd = service.get_or_create_payee("京东")?;
        service.learn_alias("支付宝-上海拉扎斯信息科技有限公司", eleme.id)?;
        // 用户把同一个描述纠正为「京东」→ 最新人工确认覆盖。
        service.learn_alias("支付宝-上海拉扎斯信息科技有限公司", jd.id)?;
        let recognized = service
            .resolve_payee_for_description("支付宝-上海拉扎斯信息科技有限公司")?
            .unwrap();
        assert_eq!(recognized.id, jd.id);
        // 别名确认计数累计。
        let count: u64 = service.conn.query_row(
            "SELECT confirmed_count FROM merchant_aliases WHERE normalized_description = ?1",
            ["上海拉扎斯信息科技有限公司"],
            |row| row.get(0),
        )?;
        assert_eq!(count, 2);
        Ok(())
    }

    // ------------------------------------------------------------------
    // 学习统计的可追踪语义（每笔交易最多贡献一份样本）
    // ------------------------------------------------------------------

    #[test]
    fn first_confirmation_contributes_one_sample() -> Result<()> {
        let (mut service, food, _) = seeded()?;
        let payee = service.get_or_create_payee("星巴克")?;
        confirm_tx(&mut service, payee.id, food)?;
        assert_eq!(stats_count(&service, payee.id, food)?, 1);
        Ok(())
    }

    #[test]
    fn editing_note_only_keeps_sample_unchanged() -> Result<()> {
        let (mut service, food, _) = seeded()?;
        let payee = service.get_or_create_payee("星巴克")?;
        let tx_id = confirm_tx(&mut service, payee.id, food)?;
        // 只改备注（不涉及 Payee/Category）→ 确认后统计不变。
        service.update_transaction(
            tx_id,
            Some("新备注".to_owned()),
            None,
            None,
            None,
            None,
            None,
        )?;
        service.confirm_transaction_learning(tx_id)?;
        assert_eq!(stats_count(&service, payee.id, food)?, 1);
        Ok(())
    }

    #[test]
    fn changing_category_moves_the_contribution() -> Result<()> {
        let (mut service, food, shopping) = seeded()?;
        let payee = service.get_or_create_payee("星巴克")?;
        let tx_id = confirm_tx(&mut service, payee.id, food)?;
        // 用户把分类从餐饮改为购物 → 餐饮撤销、购物 +1。
        service.update_transaction(tx_id, None, None, Some(shopping), None, None, None)?;
        service.confirm_transaction_learning(tx_id)?;
        assert_eq!(stats_count(&service, payee.id, food)?, 0);
        assert_eq!(stats_count(&service, payee.id, shopping)?, 1);
        Ok(())
    }

    #[test]
    fn changing_payee_moves_the_contribution() -> Result<()> {
        let (mut service, food, _) = seeded()?;
        let starbucks = service.get_or_create_payee("星巴克")?;
        let luckin = service.get_or_create_payee("瑞幸")?;
        let tx_id = confirm_tx(&mut service, starbucks.id, food)?;
        // 用户把 Payee 从星巴克改为瑞幸（分类不变）→ 旧贡献撤销、新贡献 +1。
        service.set_transaction_payee(tx_id, Some("瑞幸"))?;
        service.confirm_transaction_learning(tx_id)?;
        assert_eq!(stats_count(&service, starbucks.id, food)?, 0);
        assert_eq!(stats_count(&service, luckin.id, food)?, 1);
        Ok(())
    }

    #[test]
    fn clearing_payee_revokes_the_contribution() -> Result<()> {
        let (mut service, food, _) = seeded()?;
        let payee = service.get_or_create_payee("星巴克")?;
        let tx_id = confirm_tx(&mut service, payee.id, food)?;
        // 清除 Payee → 撤销旧贡献，交易不再贡献学习样本。
        service.set_transaction_payee(tx_id, Some(""))?;
        service.confirm_transaction_learning(tx_id)?;
        assert_eq!(stats_count(&service, payee.id, food)?, 0);
        let remaining: i64 = service.conn.query_row(
            "SELECT COUNT(*) FROM transaction_learning WHERE transaction_id = ?1",
            [tx_id],
            |row| row.get(0),
        )?;
        assert_eq!(remaining, 0);
        Ok(())
    }

    #[test]
    fn repeated_confirmation_never_grows_the_stats() -> Result<()> {
        let (mut service, food, _) = seeded()?;
        let payee = service.get_or_create_payee("星巴克")?;
        let tx_id = confirm_tx(&mut service, payee.id, food)?;
        // 重复 PATCH 相同 Payee/Category → 幂等，统计不增长。
        service.confirm_transaction_learning(tx_id)?;
        service.confirm_transaction_learning(tx_id)?;
        service.set_transaction_payee(tx_id, Some("星巴克"))?;
        service.confirm_transaction_learning(tx_id)?;
        assert_eq!(stats_count(&service, payee.id, food)?, 1);
        Ok(())
    }

    #[test]
    fn multiple_transactions_accumulate_independent_samples() -> Result<()> {
        let (mut service, food, _) = seeded()?;
        let payee = service.get_or_create_payee("饿了么")?;
        confirm_tx(&mut service, payee.id, food)?;
        confirm_tx(&mut service, payee.id, food)?;
        assert_eq!(stats_count(&service, payee.id, food)?, 2);
        Ok(())
    }

    #[test]
    fn insufficient_samples_never_auto_classify() -> Result<()> {
        let (mut service, food, _) = seeded()?;
        let payee = service.get_or_create_payee("星巴克")?;
        // 只有 1 次确认，即使 confidence = 100% 也不预测。
        confirm_tx(&mut service, payee.id, food)?;
        assert!(service.predict_category(payee.id)?.is_none());
        // 2 次同样不足。
        confirm_tx(&mut service, payee.id, food)?;
        assert!(service.predict_category(payee.id)?.is_none());
        Ok(())
    }

    #[test]
    fn high_confidence_with_enough_samples_auto_applies() -> Result<()> {
        let (mut service, food, shopping) = seeded()?;
        let payee = service.get_or_create_payee("饿了么")?;
        // 餐饮 38 次、其他 1 次 → 38/39 ≈ 97.4% ≥ 95% 且样本 ≥ 3。
        for _ in 0..38 {
            confirm_tx(&mut service, payee.id, food)?;
        }
        confirm_tx(&mut service, payee.id, shopping)?;
        let prediction = service.predict_category(payee.id)?.unwrap();
        assert!(prediction.auto_applied);
        assert_eq!(prediction.category_id, food);
        assert!(prediction.confidence >= AUTO_CATEGORY_CONFIDENCE);
        Ok(())
    }

    #[test]
    fn medium_confidence_returns_suggestion_without_auto_apply() -> Result<()> {
        let (mut service, food, shopping) = seeded()?;
        let payee = service.get_or_create_payee("京东")?;
        // 7/10 = 70%：达到建议阈值，但未到自动应用阈值。
        for _ in 0..7 {
            confirm_tx(&mut service, payee.id, food)?;
        }
        for _ in 0..3 {
            confirm_tx(&mut service, payee.id, shopping)?;
        }
        let prediction = service.predict_category(payee.id)?.unwrap();
        assert!(!prediction.auto_applied);
        assert_eq!(prediction.category_id, food);
        assert!(prediction.confidence >= SUGGEST_CATEGORY_CONFIDENCE);
        assert!(prediction.confidence < AUTO_CATEGORY_CONFIDENCE);
        Ok(())
    }

    #[test]
    fn unstable_distribution_does_not_classify() -> Result<()> {
        let (mut service, food, shopping) = seeded()?;
        let payee = service.get_or_create_payee("京东")?;
        for _ in 0..2 {
            confirm_tx(&mut service, payee.id, food)?;
            confirm_tx(&mut service, payee.id, shopping)?;
        }
        // 2/4 = 50% < 70%：不预测。
        assert!(service.predict_category(payee.id)?.is_none());
        Ok(())
    }

    #[test]
    fn correcting_category_shifts_the_stats() -> Result<()> {
        let (mut service, food, shopping) = seeded()?;
        let payee = service.get_or_create_payee("星巴克")?;
        for _ in 0..3 {
            confirm_tx(&mut service, payee.id, food)?;
        }
        let before = service.predict_category(payee.id)?.unwrap();
        assert!(before.auto_applied);
        assert_eq!(before.category_id, food);
        // 用户反复纠正为「购物」→ 购物 confidence 逐步超过餐饮。
        for _ in 0..57 {
            confirm_tx(&mut service, payee.id, shopping)?;
        }
        let after = service.predict_category(payee.id)?.unwrap();
        assert!(after.auto_applied);
        assert_eq!(after.category_id, shopping);
        Ok(())
    }

    #[test]
    fn archived_categories_are_not_predicted() -> Result<()> {
        let (mut service, food, _) = seeded()?;
        let payee = service.get_or_create_payee("饿了么")?;
        for _ in 0..5 {
            confirm_tx(&mut service, payee.id, food)?;
        }
        service.delete_category(food)?; // 归档后不再预测
        assert!(service.predict_category(payee.id)?.is_none());
        Ok(())
    }

    #[test]
    fn learning_data_is_isolated_between_ledgers() -> Result<()> {
        let (mut service_a, food, _) = seeded()?;
        let mut service_b = BookkeepingService::in_memory()?;
        service_b.ensure_default_categories()?;
        let payee = service_a.get_or_create_payee("饿了么")?;
        service_a.learn_alias("支付宝-上海拉扎斯信息科技有限公司", payee.id)?;
        confirm_tx(&mut service_a, payee.id, food)?;
        // B 账本看不到 A 的学习数据。
        assert!(service_b
            .resolve_payee_for_description("支付宝-上海拉扎斯信息科技有限公司")?
            .is_none());
        assert!(service_b.search_payees("", 100)?.is_empty());
        let b_payee = service_b.get_or_create_payee("饿了么")?;
        assert!(service_b.predict_category(b_payee.id)?.is_none());
        Ok(())
    }

    #[test]
    fn clear_learning_keeps_payees_and_transactions() -> Result<()> {
        let (mut service, food, _) = seeded()?;
        let payee = service.get_or_create_payee("饿了么")?;
        service.learn_alias("支付宝-上海拉扎斯信息科技有限公司", payee.id)?;
        let tx_id = confirm_tx(&mut service, payee.id, food)?;
        service.clear_payee_learning()?;
        // 学习数据清空，但 Payee 与交易仍在。
        assert!(service
            .resolve_payee_for_description("支付宝-上海拉扎斯信息科技有限公司")?
            .is_none());
        assert_eq!(service.search_payees("", 100)?.len(), 1);
        assert_eq!(stats_count(&service, payee.id, food)?, 0);
        assert!(service.transaction(tx_id).is_ok());
        Ok(())
    }

    /// 旧库迁移：老 schema（无 payees 表、无新列）打开后自动补齐，
    /// 已有交易不受影响；新库初始化表顺序正确（payees 先于 transactions）。
    #[test]
    fn legacy_database_migrates_payee_schema_without_touching_data() -> Result<()> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(
            r#"
            CREATE TABLE accounts (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL UNIQUE,
                account_type TEXT NOT NULL,
                currency TEXT NOT NULL,
                balance TEXT NOT NULL,
                created_at TEXT NOT NULL
            );
            CREATE TABLE categories (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                kind TEXT NOT NULL,
                created_at TEXT NOT NULL,
                UNIQUE(name, kind)
            );
            CREATE TABLE transactions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                kind TEXT NOT NULL,
                account_id INTEGER NOT NULL REFERENCES accounts(id),
                to_account_id INTEGER REFERENCES accounts(id),
                category_id INTEGER REFERENCES categories(id),
                amount TEXT NOT NULL,
                target_amount TEXT,
                occurred_at TEXT NOT NULL,
                note TEXT NOT NULL DEFAULT '',
                voided_at TEXT
            );
            INSERT INTO accounts(name, account_type, currency, balance, created_at)
                VALUES ('Legacy', 'cash', 'CNY', '456.78', '2026-08-15T00:00:00Z');
            INSERT INTO categories(name, kind, created_at)
                VALUES ('Legacy Food', 'expense', '2026-08-15T00:00:00Z');
            INSERT INTO transactions(kind, account_id, category_id, amount, occurred_at, note)
                VALUES ('expense', 1, 1, '12.34', '2026-08-15T00:00:00Z', 'legacy note');
            "#,
        )?;

        let service = BookkeepingService::from_connection(conn)?;
        // 新表已创建。
        let payee_count: i64 =
            service
                .conn
                .query_row("SELECT COUNT(*) FROM payees", [], |row| row.get(0))?;
        assert_eq!(payee_count, 0);
        let learning_table: i64 = service.conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'transaction_learning'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(learning_table, 1);
        // transactions 已带新列。
        let has_payee_column: bool = service.conn.query_row(
            "SELECT COUNT(*) > 0 FROM pragma_table_info('transactions') WHERE name = 'payee_id'",
            [],
            |row| row.get(0),
        )?;
        let has_raw_column: bool = service
            .conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM pragma_table_info('transactions') WHERE name = 'raw_description'",
                [],
                |row| row.get(0),
            )?;
        assert!(has_payee_column && has_raw_column);
        // 已有交易完整保留且可正常读取。
        let tx = service.transaction(1)?;
        assert_eq!(tx.note, "legacy note");
        assert_eq!(tx.amount, Decimal::from_str_exact("12.34").unwrap());
        assert_eq!(tx.category_id, Some(1));
        assert!(tx.payee_id.is_none());
        assert!(tx.raw_description.is_none());
        Ok(())
    }

    /// 全新数据库初始化：payees 必须先于 transactions 创建（外键引用）。
    #[test]
    fn fresh_database_orders_payees_before_transactions() -> Result<()> {
        let mut service = BookkeepingService::in_memory()?;
        service.ensure_default_categories()?;
        let payee_pos: i64 = service.conn.query_row(
            "SELECT rowid FROM sqlite_master WHERE type = 'table' AND name = 'payees'",
            [],
            |row| row.get(0),
        )?;
        let tx_pos: i64 = service.conn.query_row(
            "SELECT rowid FROM sqlite_master WHERE type = 'table' AND name = 'transactions'",
            [],
            |row| row.get(0),
        )?;
        assert!(
            payee_pos < tx_pos,
            "payees must be created before transactions"
        );
        // 外键在启用状态下可用：插入带 payee 的交易。
        let account = service.create_account(
            "外键测试",
            AccountType::Cash,
            "CNY",
            Decimal::from(1000_u32),
        )?;
        let payee = service.get_or_create_payee("饿了么")?;
        let food = service
            .categories()?
            .into_iter()
            .find(|item| item.name == "餐饮")
            .unwrap();
        let tx = service.record_expense(
            account.id,
            food.id,
            Decimal::from(10_u32),
            chrono::Utc::now(),
            "测试",
        )?;
        service.set_transaction_payee(tx.id, Some("饿了么"))?;
        service.confirm_transaction_learning(tx.id)?;
        assert_eq!(stats_count(&service, payee.id, food.id)?, 1);
        Ok(())
    }
}
