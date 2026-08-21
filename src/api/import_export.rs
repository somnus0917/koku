//! 流水导入/导出 API：CSV 导出与 CSV/QIF/OFX 批量导入。

use std::collections::HashMap;

use axum::extract::{DefaultBodyLimit, Extension, Multipart, Query, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::Response;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;

use crate::error::{KokuError, Result};
use crate::importer::{self, ImportFormat, ImportPreview};
use crate::service::ImportResult;

use super::state::{lock_ledger, ApiResponse, AppState, AuthenticatedUser};

/// 上传体、UTF-8 文本和解析结果会同时短暂存在内存中，限制为 16 MiB 可把峰值
/// 控制在普通自托管实例可承受的范围内。
const MAX_IMPORT_BYTES: usize = 16 * 1024 * 1024;
const MAX_IMPORT_ISSUES: usize = 500;

struct ParsedImport {
    format: ImportFormat,
    rows: Vec<importer::ImportRow>,
    issues: Vec<importer::ParseIssue>,
    account_id: i64,
    category_id: Option<i64>,
    currency: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ExportQuery {
    year: Option<i32>,
    month: Option<u32>,
}

/// 把单个单元格转成 CSV 字段：含逗号/引号/换行时用引号包裹并转义引号。
fn csv_field(value: &str) -> String {
    if value.contains(',') || value.contains('"') || value.contains('\n') || value.contains('\r') {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_owned()
    }
}

/// 对用户可控的自由文本做公式注入防护：以 `=` `+` `-` `@` 开头的字段前置 `'`，
/// 避免在 Excel/Sheets 打开时被当作公式执行。仅用于文本字段，不用于数字列。
fn neutralize_formula(value: &str) -> String {
    if matches!(value.as_bytes().first(), Some(b'=' | b'+' | b'-' | b'@')) {
        format!("'{value}")
    } else {
        value.to_owned()
    }
}

async fn api_export_transactions(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    Query(query): Query<ExportQuery>,
) -> Result<Response> {
    let service = lock_ledger(&state, user.user_id).await?;
    // 分页拉全量（复用既有 1000 上限，逐页累积）。
    let mut all = Vec::new();
    let mut offset = 0_u32;
    loop {
        let page = match (query.year, query.month) {
            (Some(year), Some(month)) => {
                service.transactions_in_month(year, month, 1000, offset)?
            }
            (None, None) => service.transactions(1000, offset)?,
            _ => {
                return Err(KokuError::InvalidInput(
                    "year and month must be provided together".to_owned(),
                ))
            }
        };
        let count = page.len();
        all.extend(page);
        offset += count as u32;
        if count < 1000 {
            break;
        }
    }

    let account_names: HashMap<i64, String> = service
        .accounts()?
        .into_iter()
        .map(|account| (account.id, account.name))
        .collect();
    let category_names: HashMap<i64, String> = service
        .categories()?
        .into_iter()
        .map(|category| (category.id, category.name))
        .collect();

    let mut csv = String::from(
        "id,kind,account,target_account,category,payee,amount,currency,settled_amount,occurred_at,note,raw_description,voided_at\n",
    );
    for tx in &all {
        let account = neutralize_formula(
            &account_names
                .get(&tx.account_id)
                .cloned()
                .unwrap_or_default(),
        );
        let target_account = neutralize_formula(
            &tx.to_account_id
                .and_then(|id| account_names.get(&id).cloned())
                .unwrap_or_default(),
        );
        let category = neutralize_formula(
            &tx.category_id
                .and_then(|id| category_names.get(&id).cloned())
                .unwrap_or_default(),
        );
        let payee = neutralize_formula(tx.payee_name.as_deref().unwrap_or_default());
        let note = neutralize_formula(&tx.note);
        let raw_description = neutralize_formula(tx.raw_description.as_deref().unwrap_or_default());
        let fields = [
            tx.id.to_string(),
            tx.kind.as_str().to_owned(),
            account,
            target_account,
            category,
            payee,
            tx.amount.normalize().to_string(),
            tx.currency.clone(),
            tx.settled_amount.normalize().to_string(),
            tx.occurred_at.to_rfc3339(),
            note,
            raw_description,
            tx.voided_at
                .map(|value| value.to_rfc3339())
                .unwrap_or_default(),
        ];
        csv.push_str(
            &fields
                .iter()
                .map(|field| csv_field(field))
                .collect::<Vec<_>>()
                .join(","),
        );
        csv.push('\n');
    }

    let filename = match (query.year, query.month) {
        (Some(year), Some(month)) => format!("koku-transactions-{year}-{month:02}.csv"),
        _ => "koku-transactions.csv".to_owned(),
    };
    let mut response = Response::new(axum::body::Body::from(csv));
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/csv; charset=utf-8"),
    );
    let disposition = HeaderValue::from_str(&format!("attachment; filename=\"{filename}\""))
        .map_err(|error| KokuError::InvalidInput(format!("invalid filename: {error}")))?;
    response
        .headers_mut()
        .insert(header::CONTENT_DISPOSITION, disposition);
    Ok(response)
}

/// 批量导入流水（CSV/QIF/OFX）：multipart 字段
/// `file`（必填）、`format`（csv|qif|ofx|auto，缺省 auto）、`account_id`（必填）、
/// `category_id`（可选默认分类）、`currency`（可选默认币种）。
async fn parse_import_upload(mut multipart: Multipart) -> Result<ParsedImport> {
    let mut file_bytes: Option<Vec<u8>> = None;
    let mut file_name: Option<String> = None;
    let mut format: Option<String> = None;
    let mut account_id: Option<i64> = None;
    let mut category_id: Option<i64> = None;
    let mut currency: Option<String> = None;
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|error| KokuError::InvalidInput(format!("invalid multipart upload: {error}")))?
    {
        match field.name() {
            Some("file") => {
                file_name = field.file_name().map(str::to_owned);
                let bytes = field.bytes().await.map_err(|error| {
                    KokuError::InvalidInput(format!("could not read upload: {error}"))
                })?;
                file_bytes = Some(bytes.to_vec());
            }
            Some("format") => format = Some(field.text().await.unwrap_or_default()),
            Some("account_id") => {
                account_id = field.text().await.ok().and_then(|value| value.parse().ok())
            }
            Some("category_id") => {
                category_id = field.text().await.ok().and_then(|value| value.parse().ok())
            }
            Some("currency") => currency = Some(field.text().await.unwrap_or_default()),
            _ => {}
        }
    }
    let file_bytes = file_bytes.ok_or_else(|| {
        KokuError::InvalidInput("multipart field \"file\" is required".to_owned())
    })?;
    if file_bytes.len() > MAX_IMPORT_BYTES {
        return Err(KokuError::InvalidInput(format!(
            "import file exceeds the {MAX_IMPORT_BYTES} byte limit"
        )));
    }
    let account_id = account_id.ok_or_else(|| {
        KokuError::InvalidInput("multipart field \"account_id\" is required".to_owned())
    })?;
    let text = String::from_utf8_lossy(&file_bytes).into_owned();

    // 解析放到阻塞线程，避免大文件解析拖住异步 worker。
    let (format, rows, issues) = tokio::task::spawn_blocking(move || -> Result<(
        ImportFormat,
        Vec<importer::ImportRow>,
        Vec<importer::ParseIssue>,
    )> {
        let format = match format.as_deref() {
            Some(value) if value.eq_ignore_ascii_case("auto") => {
                file_name
                    .as_deref()
                    .and_then(ImportFormat::from_filename)
                    .unwrap_or_else(|| importer::sniff_format(&text))
            }
            Some(value) => ImportFormat::from_str(value)?,
            None => importer::sniff_format(&text),
        };
        importer::parse(&text, format).map(|(rows, issues)| (format, rows, issues))
    })
    .await
    .map_err(|error| KokuError::InvalidInput(format!("import parse task failed: {error}")))??;

    Ok(ParsedImport {
        format,
        rows,
        issues,
        account_id,
        category_id,
        currency,
    })
}

/// 只解析并展示导入摘要；用户确认前不会写入账本。
async fn api_preview_import_transactions(
    Extension(_user): Extension<AuthenticatedUser>,
    multipart: Multipart,
) -> Result<Json<ApiResponse<ImportPreview>>> {
    let parsed = parse_import_upload(multipart).await?;
    Ok(Json(ApiResponse::new(importer::preview(
        parsed.format,
        &parsed.rows,
        parsed.issues,
    ))))
}

async fn api_import_transactions(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    multipart: Multipart,
) -> Result<(StatusCode, Json<ApiResponse<ImportResult>>)> {
    let parsed = parse_import_upload(multipart).await?;
    let mut result = lock_ledger(&state, user.user_id)
        .await?
        .import_transactions(
            parsed.format,
            parsed.account_id,
            parsed.category_id,
            parsed.currency,
            parsed.rows,
        )?;
    // 解析阶段跳过/失败的行（缺日期、缺金额、非收支类型等）并入结果。
    let parse_failures = parsed.issues.len();
    result.failed += parse_failures;
    result.issues.extend(
        parsed
            .issues
            .into_iter()
            .take(MAX_IMPORT_ISSUES.saturating_sub(result.issues.len())),
    );
    Ok((StatusCode::CREATED, Json(ApiResponse::new(result))))
}

async fn api_undo_import_batch(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    axum::extract::Path(batch_id): axum::extract::Path<String>,
) -> Result<Json<ApiResponse<serde_json::Value>>> {
    let undone = lock_ledger(&state, user.user_id)
        .await?
        .undo_import_batch(&batch_id)?;
    Ok(Json(ApiResponse::new(
        serde_json::json!({ "undone": undone }),
    )))
}

pub(super) fn router() -> Router<AppState> {
    Router::new()
        .route("/api/transactions/export", get(api_export_transactions))
        .route(
            "/api/transactions/import/preview",
            post(api_preview_import_transactions).layer(DefaultBodyLimit::max(MAX_IMPORT_BYTES)),
        )
        .route(
            "/api/transactions/import",
            post(api_import_transactions).layer(DefaultBodyLimit::max(MAX_IMPORT_BYTES)),
        )
        .route(
            "/api/transactions/import/{batch_id}/undo",
            post(api_undo_import_batch),
        )
}

#[cfg(test)]
mod tests {
    use super::{csv_field, neutralize_formula};

    #[test]
    fn csv_field_escapes_only_when_needed() {
        assert_eq!(csv_field("plain"), "plain");
        assert_eq!(csv_field("a,b"), "\"a,b\"");
        assert_eq!(csv_field("say \"hi\""), "\"say \"\"hi\"\"\"");
        assert_eq!(csv_field("line\nbreak"), "\"line\nbreak\"");
    }

    #[test]
    fn neutralize_formula_prefixes_spreadsheet_formula_leads() {
        assert_eq!(neutralize_formula("plain"), "plain");
        assert_eq!(neutralize_formula("=SUM(A1)"), "'=SUM(A1)");
        assert_eq!(neutralize_formula("+1+1"), "'+1+1");
        assert_eq!(neutralize_formula("-1+1"), "'-1+1");
        assert_eq!(neutralize_formula("@cmd"), "'@cmd");
        // 非开头的 =+-@ 不受影响。
        assert_eq!(neutralize_formula("abc=1"), "abc=1");
    }
}
