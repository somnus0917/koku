//! 控制台演示：`--demo` 入口、演示账本种子与终端输出。

use chrono::{Datelike, Utc};
use rust_decimal::Decimal;

use crate::domain::{Account, AccountType, CategoryKind, MonthlySummary};
use crate::error::Result;
use crate::service::BookkeepingService;

fn money(amount: Decimal, currency: &str) -> String {
    format!("{amount:.2} {currency}")
}

fn print_demo(accounts: &[Account], summary: &MonthlySummary) -> Result<()> {
    println!("\n╭──────────────────────────────────────────────────────╮");
    println!("│                  Koku · 本月财务概览                 │");
    println!("╰──────────────────────────────────────────────────────╯");
    println!("  月份       {:04}-{:02}", summary.year, summary.month);
    println!(
        "  总收入     {}",
        money(summary.total_income, &summary.currency)
    );
    println!(
        "  总支出     {}",
        money(summary.total_expense, &summary.currency)
    );
    println!("  净结余     {}", money(summary.net, &summary.currency));

    println!("\n┌──────────────── 账户实时余额 ────────────────┐");
    println!("  {:<16} {:<6} {:>18}", "账户", "类型", "余额");
    println!("  ────────────────────────────────────────────");
    for account in accounts {
        println!(
            "  {:<16} {:<6} {:>18}",
            account.name,
            account.account_type.label(),
            money(account.balance, &account.currency)
        );
    }
    println!("└──────────────────────────────────────────────┘");

    println!("\n┌──────────────── 支出分类明细 ────────────────┐");
    if summary.expenses_by_category.is_empty() {
        println!("  本月暂无支出");
    } else {
        println!("  {:<14} {:>18} {:>10}", "分类", "金额", "占比");
        println!("  ────────────────────────────────────────────");
        for item in &summary.expenses_by_category {
            println!(
                "  {:<14} {:>18} {:>9.2}%",
                item.category_name,
                money(item.amount, &summary.currency),
                item.percentage
            );
        }
    }
    println!("└──────────────────────────────────────────────┘");

    println!("\n可序列化统计 DTO（JSON）：");
    println!(
        "{}",
        serde_json::to_string_pretty(summary).map_err(|error| {
            crate::error::KokuError::InvalidInput(format!("failed to serialize summary: {error}"))
        })?
    );
    Ok(())
}

pub fn run_demo() -> Result<()> {
    let mut service = BookkeepingService::in_memory()?;
    let alipay =
        service.create_account("支付宝", AccountType::Cash, "CNY", Decimal::new(120_000, 2))?;
    let cmb = service.create_account(
        "招商 Visa",
        AccountType::Savings,
        "CNY",
        Decimal::new(800_000, 2),
    )?;
    let _credit = service.create_account("信用卡", AccountType::Credit, "CNY", Decimal::ZERO)?;

    let salary = service.create_category("工资", CategoryKind::Income)?;
    let food = service.create_category("餐饮", CategoryKind::Expense)?;
    let transit = service.create_category("交通", CategoryKind::Expense)?;
    let shopping = service.create_category("购物", CategoryKind::Expense)?;
    let now = Utc::now();

    service.record_income(cmb.id, salary.id, Decimal::new(850_000, 2), now, "八月工资")?;
    service.record_expense(alipay.id, food.id, Decimal::new(6_850, 2), now, "晚餐")?;
    service.record_transfer(
        cmb.id,
        alipay.id,
        Decimal::new(100_000, 2),
        Decimal::new(100_000, 2),
        now,
        "日常消费金",
    )?;
    service.record_expense(alipay.id, transit.id, Decimal::new(1_200, 2), now, "地铁")?;
    service.record_expense_in_currency(
        cmb.id,
        shopping.id,
        Decimal::new(3_280, 2),
        "USD",
        Decimal::new(23_616, 2),
        now,
        "海外软件订阅",
    )?;

    let cancelled = service.record_expense(
        alipay.id,
        food.id,
        Decimal::new(2_580, 2),
        now,
        "误记的午餐",
    )?;
    service.void_transaction(cancelled.id)?;

    let summary = service.monthly_summary(now.year(), now.month(), "CNY")?;
    print_demo(&service.accounts()?, &summary)
}

/// 首次启动时为空数据库生成演示账本。
pub fn seed_demo_data(service: &mut BookkeepingService) -> Result<()> {
    if !service.is_empty()? {
        return Ok(());
    }

    let alipay =
        service.create_account("支付宝", AccountType::Cash, "CNY", Decimal::new(328_000, 2))?;
    let cmb = service.create_account(
        "招商 Visa",
        AccountType::Savings,
        "CNY",
        Decimal::new(2_856_000, 2),
    )?;
    let cash = service.create_account(
        "现金钱包",
        AccountType::Cash,
        "CNY",
        Decimal::new(56_000, 2),
    )?;
    let credit = service.create_account(
        "信用卡",
        AccountType::Credit,
        "CNY",
        Decimal::new(126_000, 2),
    )?;

    let salary = service.create_category("工资", CategoryKind::Income)?;
    let side_job = service.create_category("副业", CategoryKind::Income)?;
    let food = service.create_category("餐饮", CategoryKind::Expense)?;
    let transit = service.create_category("交通", CategoryKind::Expense)?;
    let shopping = service.create_category("购物", CategoryKind::Expense)?;
    let home = service.create_category("居家", CategoryKind::Expense)?;
    let entertainment = service.create_category("娱乐", CategoryKind::Expense)?;

    let now = Utc::now();
    service.record_income(
        cmb.id,
        salary.id,
        Decimal::new(1_280_000, 2),
        now,
        "本月工资",
    )?;
    service.record_income(
        alipay.id,
        side_job.id,
        Decimal::new(168_000, 2),
        now,
        "设计项目尾款",
    )?;
    service.record_expense(alipay.id, food.id, Decimal::new(6_850, 2), now, "梧桐小馆")?;
    service.record_expense(
        alipay.id,
        transit.id,
        Decimal::new(1_200, 2),
        now,
        "地铁通勤",
    )?;
    service.record_expense(cmb.id, home.id, Decimal::new(280_000, 2), now, "房租")?;
    service.record_expense(
        credit.id,
        shopping.id,
        Decimal::new(38_900, 2),
        now,
        "生活用品",
    )?;
    service.record_expense(cash.id, food.id, Decimal::new(2_400, 2), now, "咖啡")?;
    service.record_expense(
        alipay.id,
        entertainment.id,
        Decimal::new(4_500, 2),
        now,
        "电影",
    )?;
    service.record_expense_in_currency(
        cmb.id,
        shopping.id,
        Decimal::new(32_80, 2),
        "USD",
        Decimal::new(23_616, 2),
        now,
        "海外软件订阅",
    )?;
    service.record_transfer(
        cmb.id,
        alipay.id,
        Decimal::new(100_000, 2),
        Decimal::new(100_000, 2),
        now,
        "日常消费金",
    )?;
    Ok(())
}
