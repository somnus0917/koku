//! 生成 bcrypt 密码哈希（本地预览 / 初始化引导用）。
//!
//! 用法：`cargo run --bin gen_hash -- <password>`
//! 不传密码时使用预览默认密码 `koku-preview`。
//! 复用应用自身的 bcrypt 依赖，保证哈希与登录校验完全兼容。

fn main() {
    let password = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "koku-preview".to_owned());
    match bcrypt::hash(&password, bcrypt::DEFAULT_COST) {
        Ok(hash) => println!("{hash}"),
        Err(error) => {
            eprintln!("failed to hash password: {error}");
            std::process::exit(1);
        }
    }
}
