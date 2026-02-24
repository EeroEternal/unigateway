pub const LAYOUT: &str = r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
  <meta charset="UTF-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1.0" />
  <title>{{title}}</title>
  <script src="https://unpkg.com/htmx.org@1.9.12"></script>
  <script src="https://cdn.tailwindcss.com"></script>
  <link href="https://cdn.jsdelivr.net/npm/daisyui@4.12.10/dist/full.min.css" rel="stylesheet" type="text/css" />
  <script>
    tailwind.config = {
      theme: {
        extend: {
          colors: {
            brand: '#3C6E71',
            brandLight: '#D9E6E7'
          }
        }
      }
    }
  </script>
</head>
<body class="bg-base-200 min-h-screen">{{body}}</body>
</html>"#;

pub const LOGIN_PAGE: &str = r#"
<div class="min-h-screen flex items-center justify-center px-4">
  <div class="card w-full max-w-md bg-base-100 shadow-xl">
    <div class="card-body">
      <h1 class="card-title text-brand text-2xl">UniGateway</h1>
      <p class="text-sm text-base-content/70">默认管理员：admin / admin123</p>
      <form method="post" action="/login" class="space-y-3 mt-4">
        <label class="form-control w-full">
          <div class="label"><span class="label-text">用户名</span></div>
          <input class="input input-bordered w-full" name="username" placeholder="admin" value="admin" />
        </label>
        <label class="form-control w-full">
          <div class="label"><span class="label-text">密码</span></div>
          <input class="input input-bordered w-full" type="password" name="password" placeholder="请输入密码" />
        </label>
        <button class="btn btn-primary w-full" type="submit">登录</button>
      </form>
    </div>
  </div>
</div>
"#;

pub const LOGIN_ERROR_PAGE: &str = r#"
<div class="min-h-screen flex items-center justify-center px-4">
  <div class="alert alert-error max-w-md">
    <span>用户名或密码错误</span>
  </div>
  <div class="fixed bottom-8">
    <a class="btn btn-outline" href="/login">返回登录</a>
  </div>
</div>
"#;

pub const ADMIN_PAGE: &str = r#"
<div class="navbar bg-base-100 shadow-sm px-6">
  <div class="flex-1">
    <a class="text-xl font-bold text-brand">UniGateway</a>
  </div>
  <div class="flex-none">
    <form method="post" action="/logout"><button class="btn btn-sm btn-outline">退出</button></form>
  </div>
</div>

<div class="p-6 space-y-6 max-w-5xl mx-auto">
  <div class="alert bg-brandLight text-brand border-none">
    <span>轻量开源版：OpenAI + Anthropic 网关，SQLite 统计。</span>
  </div>

  <div
    id="stats-box"
    hx-get="/admin/stats"
    hx-trigger="load, every 10s"
    class="grid grid-cols-1 md:grid-cols-3 gap-4"
  ></div>

  <div class="card bg-base-100 shadow">
    <div class="card-body">
      <h2 class="card-title">接口</h2>
      <ul class="list-disc list-inside text-sm space-y-1">
        <li>POST /v1/chat/completions (OpenAI 兼容)</li>
        <li>POST /v1/messages (Anthropic 兼容)</li>
        <li>GET /v1/models</li>
        <li>GET /metrics</li>
        <li>GET /health</li>
      </ul>
    </div>
  </div>
</div>
"#;

pub const STATS_PARTIAL: &str = r#"
<div class="stat bg-base-100 rounded-box shadow">
  <div class="stat-title">总请求</div>
  <div class="stat-value text-brand">{{total}}</div>
</div>
<div class="stat bg-base-100 rounded-box shadow">
  <div class="stat-title">OpenAI 兼容</div>
  <div class="stat-value text-brand">{{openai_count}}</div>
</div>
<div class="stat bg-base-100 rounded-box shadow">
  <div class="stat-title">Anthropic 兼容</div>
  <div class="stat-value text-brand">{{anthropic_count}}</div>
</div>
"#;
