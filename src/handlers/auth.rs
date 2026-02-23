use std::sync::Arc;
use axum::{
    extract::State,
    http::{header, HeaderMap, StatusCode},
    response::{Html, IntoResponse, Redirect},
    Form,
};
use rand::{distributions::Alphanumeric, Rng};

use crate::db::{hash_password, models::LoginForm};
use crate::server::AppState;

fn html_layout(title: &str, body: &str) -> String {
    format!(
        r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
  <meta charset="UTF-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1.0" />
  <title>{title}</title>
  <script src="https://unpkg.com/htmx.org@1.9.12"></script>
  <script src="https://cdn.tailwindcss.com"></script>
  <link href="https://cdn.jsdelivr.net/npm/daisyui@4.12.10/dist/full.min.css" rel="stylesheet" type="text/css" />
  <script>
    tailwind.config = {{
      theme: {{
        extend: {{
          colors: {{
            brand: '#3C6E71',
            brandLight: '#D9E6E7'
          }}
        }}
      }}
    }}
  </script>
</head>
<body class="bg-base-200 min-h-screen">{body}</body>
</html>"#
    )
}

pub async fn login_page() -> impl IntoResponse {
    Html(html_layout(
        "UniGateway Login",
        r#"
<div class="min-h-screen flex items-center justify-center px-4">
  <div class="card w-full max-w-md bg-base-100 shadow-xl">
    <div class="card-body">
      <h1 class="card-title text-brand text-2xl">UniGateway</h1>
      <p class="text-sm text-base-content/70">默认管理员：admin / admin123</p>
      <form method="post" action="/login" class="space-y-3 mt-4">
        <input class="input input-bordered w-full" name="username" placeholder="用户名" value="admin" />
        <input class="input input-bordered w-full" type="password" name="password" placeholder="密码" />
        <button class="btn w-full bg-brand text-white border-none hover:opacity-90" type="submit">登录</button>
      </form>
    </div>
  </div>
</div>
"#,
    ))
}

pub fn get_cookie_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .and_then(|raw| {
            raw.split(';').find_map(|part| {
                let item = part.trim();
                item.strip_prefix("unigateway_session=").map(|v| v.to_string())
            })
        })
}

pub async fn login(
    State(state): State<Arc<AppState>>,
    Form(form): Form<LoginForm>,
) -> impl IntoResponse {
    let user = sqlx::query_as::<_, (i64, String)>(
        "SELECT id, password_hash FROM users WHERE username = ?",
    )
    .bind(&form.username)
    .fetch_optional(&state.pool)
    .await;

    let Ok(Some((user_id, password_hash))) = user else {
        return Html(html_layout("登录失败", "<div class='p-8'><p>用户名或密码错误</p><a class='link text-brand' href='/login'>返回登录</a></div>")).into_response();
    };

    if hash_password(&form.password) != password_hash {
        return Html(html_layout("登录失败", "<div class='p-8'><p>用户名或密码错误</p><a class='link text-brand' href='/login'>返回登录</a></div>")).into_response();
    }

    let token: String = rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(40)
        .map(char::from)
        .collect();

    if sqlx::query("INSERT INTO sessions(token, user_id) VALUES(?, ?)")
        .bind(&token)
        .bind(user_id)
        .execute(&state.pool)
        .await
        .is_err()
    {
        return (StatusCode::INTERNAL_SERVER_ERROR, "session create failed").into_response();
    }

    let mut headers = HeaderMap::new();
    if let Ok(cookie) =
        format!("unigateway_session={token}; Path=/; HttpOnly; SameSite=Lax").parse()
    {
        headers.insert(header::SET_COOKIE, cookie);
    }

    (headers, Redirect::to("/admin")).into_response()
}

pub async fn logout(State(state): State<Arc<AppState>>, headers: HeaderMap) -> impl IntoResponse {
    if let Some(token) = get_cookie_token(&headers) {
        let _ = sqlx::query("DELETE FROM sessions WHERE token = ?")
            .bind(token)
            .execute(&state.pool)
            .await;
    }

    let mut response = Redirect::to("/login").into_response();
    if let Ok(cookie) = "unigateway_session=; Path=/; Max-Age=0; HttpOnly; SameSite=Lax".parse() {
        response.headers_mut().insert(header::SET_COOKIE, cookie);
    }
    response
}

pub async fn ensure_login(pool: &sqlx::SqlitePool, headers: &HeaderMap) -> bool {
    let Some(token) = get_cookie_token(headers) else {
        return false;
    };

    match sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM sessions WHERE token = ?")
        .bind(token)
        .fetch_one(pool)
        .await
    {
        Ok(count) => count > 0,
        Err(_) => false,
    }
}
