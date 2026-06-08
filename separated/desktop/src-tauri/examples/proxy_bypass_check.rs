// 端到端验证 .no_proxy() 是否真的能绕过系统代理直连后端。
// 模拟用户场景:开了系统代理(HTTP_PROXY 指向不可达端口)的情况下,
// 默认 reqwest 客户端 vs .no_proxy() 客户端 对 127.0.0.1:10870 发 POST 的差异。

use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 模拟"用户开了代理":把 HTTP_PROXY 设为本地一个不存在的端口
    // 这样默认 reqwest 客户端会把 POST 请求转发给这个不可达代理 → 必失败
    std::env::set_var("HTTP_PROXY", "http://127.0.0.1:39999");
    std::env::set_var("HTTPS_PROXY", "http://127.0.0.1:39999");

    let url = "http://127.0.0.1:10870/readsubscriptions";
    let body = r#"{"url":"test"}"#;

    println!("=== 场景 A:默认 reqwest(走系统代理),POST ===");
    let default_client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()?;
    match default_client
        .post(url)
        .header("content-type", "application/json")
        .body(body)
        .send()
        .await
    {
        Ok(r) => println!("  ✗ 出乎意料地成功了:HTTP {}", r.status()),
        Err(e) => println!("  ✓ 如预期失败:{}", e),
    }

    println!("\n=== 场景 B:.no_proxy() 客户端,POST ===");
    let no_proxy_client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(5))
        .build()?;
    match no_proxy_client
        .post(url)
        .header("content-type", "application/json")
        .body(body)
        .send()
        .await
    {
        Ok(r) => {
            let status = r.status();
            let text = r.text().await.unwrap_or_default();
            println!("  ✓ 直连后端成功:HTTP {} body={}", status, &text[..text.len().min(80)]);
        }
        Err(e) => println!("  ✗ 失败:{}", e),
    }

    println!("\n=== 场景 C:.no_proxy() 客户端,GET /getversion ===");
    match no_proxy_client
        .get("http://127.0.0.1:10870/getversion")
        .send()
        .await
    {
        Ok(r) => {
            let text = r.text().await.unwrap_or_default();
            println!("  ✓ 直连后端成功:body={}", text);
        }
        Err(e) => println!("  ✗ 失败:{}", e),
    }

    Ok(())
}
