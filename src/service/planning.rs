//! 导入模板、账单与储蓄目标的轻量持久化。
use super::*;
use crate::domain::{Bill, ImportProfile, SavingsGoal};
use crate::error::{KokuError, Result};
use chrono::{NaiveDate, Utc};
use rusqlite::{params, OptionalExtension};
use rust_decimal::Decimal;

impl BookkeepingService {
    pub fn import_profiles(&self) -> Result<Vec<ImportProfile>> {
        let mut s=self.conn.prepare("SELECT id,name,format,account_id,category_id,currency,created_at,updated_at FROM import_profiles ORDER BY name")?;
        let rows = s.query_map([], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
                r.get(6)?,
                r.get(7)?,
            ))
        })?;
        rows.map(|r| profile_from_row(r?)).collect()
    }
    pub fn save_import_profile(
        &mut self,
        id: Option<i64>,
        name: String,
        format: String,
        account: Option<i64>,
        category: Option<i64>,
        currency: Option<String>,
    ) -> Result<ImportProfile> {
        validate_profile(self, &name, &format, account, category)?;
        let now = timestamp(Utc::now());
        let id = match id {
            Some(id) => {
                let updated = self.conn.execute(
                    "UPDATE import_profiles SET name=?1,format=?2,account_id=?3,category_id=?4,currency=?5,updated_at=?6 WHERE id=?7",
                    params![name.trim(), format, account, category, currency.as_deref().map(str::trim), now, id],
                )?;
                if updated != 1 {
                    return Err(KokuError::NotFound {
                        entity: "import profile",
                        id,
                    });
                }
                id
            }
            None => {
                self.conn.execute(
                    "INSERT INTO import_profiles(name,format,account_id,category_id,currency,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?6)",
                    params![name.trim(), format, account, category, currency.as_deref().map(str::trim), now],
                )?;
                self.conn.last_insert_rowid()
            }
        };
        self.import_profile(id)
    }
    pub fn delete_import_profile(&mut self, id: i64) -> Result<()> {
        if self
            .conn
            .execute("DELETE FROM import_profiles WHERE id=?1", [id])?
            != 1
        {
            return Err(KokuError::NotFound {
                entity: "import profile",
                id,
            });
        };
        Ok(())
    }
    fn import_profile(&self, id: i64) -> Result<ImportProfile> {
        self.conn
            .query_row(
                "SELECT id,name,format,account_id,category_id,currency,created_at,updated_at FROM import_profiles WHERE id=?1",
                [id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?, row.get(7)?)),
            )
            .optional()?
            .map(profile_from_row)
            .transpose()?
            .ok_or(KokuError::NotFound { entity: "import profile", id })
    }
    pub fn bills(&self) -> Result<Vec<Bill>> {
        let mut s=self.conn.prepare("SELECT id,name,account_id,category_id,amount,due_day,active,note,created_at,updated_at FROM bills ORDER BY active DESC,due_day,name")?;
        let rows = s.query_map([], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
                r.get(6)?,
                r.get(7)?,
                r.get(8)?,
                r.get(9)?,
            ))
        })?;
        rows.map(|r| bill_from_row(r?)).collect()
    }
    #[allow(clippy::too_many_arguments)]
    pub fn save_bill(
        &mut self,
        id: Option<i64>,
        name: String,
        account: i64,
        category: i64,
        amount: Decimal,
        due_day: u32,
        active: bool,
        note: String,
    ) -> Result<Bill> {
        let name = required_planning_name(name, "bill")?;
        positive_amount(amount)?;
        if !(1..=31).contains(&due_day) {
            return Err(KokuError::InvalidInput("due day must be 1..31".into()));
        };
        self.account(account)?;
        let c = self.category(category)?;
        if c.kind != crate::domain::CategoryKind::Expense {
            return Err(KokuError::CategoryKindMismatch {
                expected: "expense",
                actual: c.kind.as_str(),
            });
        };
        let now = timestamp(Utc::now());
        let id = match id {
            Some(id) => {
                if self.conn.execute("UPDATE bills SET name=?1,account_id=?2,category_id=?3,amount=?4,due_day=?5,active=?6,note=?7,updated_at=?8 WHERE id=?9",params![name,account,category,decimal_to_db(amount),due_day,active,note,now,id])?!=1{return Err(KokuError::NotFound{entity:"bill",id})};
                id
            }
            None => {
                self.conn.execute("INSERT INTO bills(name,account_id,category_id,amount,due_day,active,note,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?8)",params![name,account,category,decimal_to_db(amount),due_day,active,note,now])?;
                self.conn.last_insert_rowid()
            }
        };
        self.bill(id)
    }
    pub fn delete_bill(&mut self, id: i64) -> Result<()> {
        if self.conn.execute("DELETE FROM bills WHERE id=?1", [id])? != 1 {
            return Err(KokuError::NotFound { entity: "bill", id });
        };
        Ok(())
    }
    fn bill(&self, id: i64) -> Result<Bill> {
        self.conn
            .query_row(
                "SELECT id,name,account_id,category_id,amount,due_day,active,note,created_at,updated_at FROM bills WHERE id=?1",
                [id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?, row.get(7)?, row.get(8)?, row.get(9)?)),
            )
            .optional()?
            .map(bill_from_row)
            .transpose()?
            .ok_or(KokuError::NotFound { entity: "bill", id })
    }
    pub fn savings_goals(&self) -> Result<Vec<SavingsGoal>> {
        let mut s=self.conn.prepare("SELECT id,name,account_id,target_amount,current_amount,target_date,created_at,updated_at FROM savings_goals ORDER BY target_date,name")?;
        let rows = s.query_map([], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
                r.get(6)?,
                r.get(7)?,
            ))
        })?;
        rows.map(|r| goal_from_row(r?)).collect()
    }
    pub fn save_savings_goal(
        &mut self,
        id: Option<i64>,
        name: String,
        account: Option<i64>,
        target: Decimal,
        current: Decimal,
        target_date: Option<NaiveDate>,
    ) -> Result<SavingsGoal> {
        let name = required_planning_name(name, "savings goal")?;
        positive_amount(target)?;
        if current < Decimal::ZERO {
            return Err(KokuError::InvalidInput(
                "goal current amount cannot be negative".into(),
            ));
        };
        if let Some(id) = account {
            self.account(id)?;
        }
        let now = timestamp(Utc::now());
        let date = target_date.map(|v| v.to_string());
        let id = match id {
            Some(id) => {
                if self.conn.execute("UPDATE savings_goals SET name=?1,account_id=?2,target_amount=?3,current_amount=?4,target_date=?5,updated_at=?6 WHERE id=?7",params![name,account,decimal_to_db(target),decimal_to_db(current),date,now,id])?!=1{return Err(KokuError::NotFound{entity:"savings goal",id})};
                id
            }
            None => {
                self.conn.execute("INSERT INTO savings_goals(name,account_id,target_amount,current_amount,target_date,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?6)",params![name,account,decimal_to_db(target),decimal_to_db(current),date,now])?;
                self.conn.last_insert_rowid()
            }
        };
        self.savings_goal(id)
    }
    pub fn delete_savings_goal(&mut self, id: i64) -> Result<()> {
        if self
            .conn
            .execute("DELETE FROM savings_goals WHERE id=?1", [id])?
            != 1
        {
            return Err(KokuError::NotFound {
                entity: "savings goal",
                id,
            });
        };
        Ok(())
    }
    fn savings_goal(&self, id: i64) -> Result<SavingsGoal> {
        self.conn
            .query_row(
                "SELECT id,name,account_id,target_amount,current_amount,target_date,created_at,updated_at FROM savings_goals WHERE id=?1",
                [id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?, row.get(7)?)),
            )
            .optional()?
            .map(goal_from_row)
            .transpose()?
            .ok_or(KokuError::NotFound { entity: "savings goal", id })
    }
}

fn required_planning_name(value: String, entity: &str) -> Result<String> {
    let value = value.trim();
    if value.is_empty() {
        return Err(KokuError::InvalidInput(format!(
            "{entity} name cannot be empty"
        )));
    }
    if value.chars().count() > 80 {
        return Err(KokuError::InvalidInput(format!(
            "{entity} name must be 80 characters or fewer"
        )));
    }
    Ok(value.to_owned())
}
fn validate_profile(
    s: &BookkeepingService,
    name: &str,
    format: &str,
    account: Option<i64>,
    category: Option<i64>,
) -> Result<()> {
    if name.trim().is_empty() || name.chars().count() > 80 {
        return Err(KokuError::InvalidInput(
            "invalid import profile name".into(),
        ));
    };
    if !matches!(format, "auto" | "csv" | "qif" | "ofx") {
        return Err(KokuError::InvalidInput(
            "invalid import profile format".into(),
        ));
    };
    if let Some(id) = account {
        s.account(id)?;
    }
    if let Some(id) = category {
        s.category(id)?;
    }
    Ok(())
}
#[allow(clippy::type_complexity)]
fn profile_from_row(
    r: (
        i64,
        String,
        String,
        Option<i64>,
        Option<i64>,
        Option<String>,
        String,
        String,
    ),
) -> Result<ImportProfile> {
    Ok(ImportProfile {
        id: r.0,
        name: r.1,
        format: r.2,
        account_id: r.3,
        category_id: r.4,
        currency: r.5,
        created_at: parse_timestamp(&r.6)?,
        updated_at: parse_timestamp(&r.7)?,
    })
}
fn bill_from_row(
    r: (
        i64,
        String,
        i64,
        i64,
        String,
        u32,
        bool,
        String,
        String,
        String,
    ),
) -> Result<Bill> {
    Ok(Bill {
        id: r.0,
        name: r.1,
        account_id: r.2,
        category_id: r.3,
        amount: decimal_from_db(&r.4)?,
        due_day: r.5,
        active: r.6,
        note: r.7,
        created_at: parse_timestamp(&r.8)?,
        updated_at: parse_timestamp(&r.9)?,
    })
}
fn goal_from_row(
    r: (
        i64,
        String,
        Option<i64>,
        String,
        String,
        Option<String>,
        String,
        String,
    ),
) -> Result<SavingsGoal> {
    Ok(SavingsGoal {
        id: r.0,
        name: r.1,
        account_id: r.2,
        target_amount: decimal_from_db(&r.3)?,
        current_amount: decimal_from_db(&r.4)?,
        target_date: r
            .5
            .as_deref()
            .map(|value| NaiveDate::parse_from_str(value, "%Y-%m-%d"))
            .transpose()
            .map_err(|e| KokuError::InvalidInput(format!("invalid goal date: {e}")))?,
        created_at: parse_timestamp(&r.6)?,
        updated_at: parse_timestamp(&r.7)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{AccountType, CategoryKind};

    #[test]
    fn bill_and_goal_names_are_trimmed_and_bounded() -> Result<()> {
        let mut service = BookkeepingService::in_memory()?;
        let account = service.create_account("Cash", AccountType::Cash, "CNY", Decimal::ZERO)?;
        let category = service.create_category("Housing", CategoryKind::Expense)?;

        assert!(service
            .save_bill(
                None,
                "   ".to_owned(),
                account.id,
                category.id,
                Decimal::TEN,
                1,
                true,
                String::new(),
            )
            .is_err());
        assert!(service
            .save_savings_goal(
                None,
                "x".repeat(81),
                Some(account.id),
                Decimal::TEN,
                Decimal::ZERO,
                None,
            )
            .is_err());

        let bill = service.save_bill(
            None,
            "  Rent  ".to_owned(),
            account.id,
            category.id,
            Decimal::TEN,
            1,
            true,
            String::new(),
        )?;
        assert_eq!(bill.name, "Rent");
        Ok(())
    }
}
