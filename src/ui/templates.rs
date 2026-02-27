pub const LAYOUT: &str = r##"<!DOCTYPE html>
<html lang="zh-CN" data-theme="emerald">
<head>
  <meta charset="UTF-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1.0" />
  <title>{{title}}</title>
  <script src="https://unpkg.com/htmx.org@1.9.12"></script>
  <script src="https://cdn.tailwindcss.com"></script>
  <link href="https://cdn.jsdelivr.net/npm/daisyui@4.12.10/dist/full.min.css" rel="stylesheet" type="text/css" />
  <link href="https://fonts.googleapis.com/css2?family=Inter:wght@400;500;600;700&display=swap" rel="stylesheet">
  <script>
    tailwind.config = {
      theme: {
        extend: {
          fontFamily: {
            sans: ['Inter', 'system-ui', 'sans-serif'],
          },
          colors: {
            brand: '#346467',
            teal: {
              700: '#346467',
              800: '#2A4F51',
              900: '#1C2B2B',
            }
          }
        }
      }
    }
  </script>
  <style>
    body { font-family: 'Inter', system-ui, sans-serif; }
    .sidebar-active { background-color: #346467 !important; color: white !important; }
    .sidebar-active:hover { background-color: #2A4F51 !important; }
    .menu a.active { background-color: #346467 !important; color: white !important; }
    .menu a:hover { background-color: rgba(52, 100, 103, 0.1); }
  </style>
</head>
<body class="bg-[#F8FAFC] min-h-screen text-[#1E293B] antialiased">
  <div class="drawer lg:drawer-open">
    <input id="my-drawer" type="checkbox" class="drawer-toggle" />
    <div class="drawer-content flex flex-col">
      <!-- Top Navbar for Mobile -->
      <div class="navbar bg-white lg:hidden shadow-sm border-b border-slate-200">
        <div class="flex-none">
          <label for="my-drawer" class="btn btn-square btn-ghost">
            <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" class="inline-block w-6 h-6 stroke-slate-600"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M4 6h16M4 12h16M4 18h16"></path></svg>
          </label>
        </div>
        <div class="flex-1 px-2">
          <span class="text-xl font-bold text-brand tracking-tight">UniGateway</span>
        </div>
      </div>

      <!-- Main Content Area -->
      <main id="main-content" class="p-6 lg:p-10 max-w-[1600px] mx-auto w-full">
        {{body}}
      </main>
    </div>
    <div class="drawer-side z-40">
      <label for="my-drawer" aria-label="close sidebar" class="drawer-overlay"></label>
      <div class="menu p-0 w-64 min-h-full bg-[#1C2B2B] text-white flex flex-col border-r border-[#1C2B2B]">
        <div class="flex items-center gap-3 py-8 px-6 mb-2">
            <div class="w-10 h-10 bg-brand rounded-xl flex items-center justify-center shadow-lg shadow-brand/20">
                <svg xmlns="http://www.w3.org/2000/svg" class="h-6 w-6 text-white" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round">
                    <path d="m13 2-2 4h3l-2 4h3l-9 12 2-9h-3l2-4h-3l9-12z"></path>
                </svg>
            </div>
            <span class="text-2xl font-bold tracking-tighter">UniGateway</span>
        </div>

        <ul class="px-4 space-y-1 flex-1">
          <li>
            <a hx-get="/admin/dashboard" hx-target="#main-content" hx-push-url="true" class="flex gap-3 px-4 py-3 rounded-xl transition-all duration-200 hover:bg-white/10 active:scale-[0.98]">
              <svg xmlns="http://www.w3.org/2000/svg" class="h-5 w-5 opacity-70" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2"><path stroke-linecap="round" stroke-linejoin="round" d="M4 5a1 1 0 011-1h4a1 1 0 011 1v5a1 1 0 01-1 1H5a1 1 0 01-1-1V5zM14 5a1 1 0 011-1h4a1 1 0 011 1v2a1 1 0 01-1 1h-4a1 1 0 01-1-1V5zM4 15a1 1 0 011-1h4a1 1 0 011 1v4a1 1 0 01-1 1H5a1 1 0 01-1-1v-4zM14 15a1 1 0 011-1h4a1 1 0 011 1v4a1 1 0 01-1 1h-4a1 1 0 01-1-1v-4z" /></svg>
              <span class="font-medium text-[15px]">仪表盘</span>
            </a>
          </li>
          <li>
            <a hx-get="/admin/providers" hx-target="#main-content" hx-push-url="true" class="flex gap-3 px-4 py-3 rounded-xl transition-all duration-200 hover:bg-white/10 active:scale-[0.98]">
              <svg xmlns="http://www.w3.org/2000/svg" class="h-5 w-5 opacity-70" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2"><path stroke-linecap="round" stroke-linejoin="round" d="M21 12a9 9 0 01-9 9m9-9a9 9 0 00-9-9m9 9H3m9 9a9 9 0 01-9-9m9-9c1.657 0 3 4.03 3 9s-1.343 9-3 9m0-18c-1.657 0-3 4.03-3 9s1.343 9 3 9" /></svg>
              <span class="font-medium text-[15px]">模型管理</span>
            </a>
          </li>
          <li>
            <a hx-get="/admin/api-keys" hx-target="#main-content" hx-push-url="true" class="flex gap-3 px-4 py-3 rounded-xl transition-all duration-200 hover:bg-white/10 active:scale-[0.98]">
              <svg xmlns="http://www.w3.org/2000/svg" class="h-5 w-5 opacity-70" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2"><path stroke-linecap="round" stroke-linejoin="round" d="M15 7a2 2 0 012 2m4 0a6 6 0 01-7.743 5.743L11 17H9v2H7v2H4a1 1 0 01-1-1v-2.586a1 1 0 01.293-.707l5.964-5.964A6 6 0 1121 9z" /></svg>
              <span class="font-medium text-[15px]">API Keys</span>
            </a>
          </li>
          <li>
            <a hx-get="/admin/logs" hx-target="#main-content" hx-push-url="true" class="flex gap-3 px-4 py-3 rounded-xl transition-all duration-200 hover:bg-white/10 active:scale-[0.98]">
              <svg xmlns="http://www.w3.org/2000/svg" class="h-5 w-5 opacity-70" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2"><path stroke-linecap="round" stroke-linejoin="round" d="M9 12h6m-6 4h6m2 5H7a2 2 0 01-2-2V5a2 2 0 012-2h5.586a1 1 0 01.707.293l5.414 5.414a1 1 0 01.293.707V19a2 2 0 01-2 2z" /></svg>
              <span class="font-medium text-[15px]">请求日志</span>
            </a>
          </li>
          <li>
            <a hx-get="/admin/settings" hx-target="#main-content" hx-push-url="true" class="flex gap-3 px-4 py-3 rounded-xl transition-all duration-200 hover:bg-white/10 active:scale-[0.98]">
              <svg xmlns="http://www.w3.org/2000/svg" class="h-5 w-5 opacity-70" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2"><path stroke-linecap="round" stroke-linejoin="round" d="M10.325 4.317c.426-1.756 2.924-1.756 3.35 0a1.724 1.724 0 002.573 1.066c1.543-.94 3.31.826 2.37 2.37a1.724 1.724 0 001.065 2.572c1.756.426 1.756 2.924 0 3.35a1.724 1.724 0 00-1.066 2.573c.94 1.543-.826 3.31-2.37 2.37a1.724 1.724 0 00-2.572 1.065c-.426 1.756-2.924 1.756-3.35 0a1.724 1.724 0 00-2.573-1.066c-1.543.94-3.31-.826-2.37-2.37a1.724 1.724 0 00-1.065-2.572c-1.756-.426-1.756-2.924 0-3.35a1.724 1.724 0 001.066-2.573c-.94-1.543.826-3.31 2.37-2.37.996.608 2.296.07 2.572-1.065z" /><path stroke-linecap="round" stroke-linejoin="round" d="M15 12a3 3 0 11-6 0 3 3 0 016 0z" /></svg>
              <span class="font-medium text-[15px]">设置</span>
            </a>
          </li>
        </ul>

        <div class="mt-auto px-4 pb-8">
            <div class="bg-white/5 border border-white/10 rounded-xl p-4 mb-6">
                <div class="flex items-center gap-3">
                    <div class="relative flex items-center justify-center">
                        <div class="w-2.5 h-2.5 rounded-full bg-emerald-500"></div>
                        <div class="absolute w-2.5 h-2.5 rounded-full bg-emerald-500 animate-ping opacity-75"></div>
                    </div>
                    <div>
                        <div class="text-[10px] text-white/40 font-bold uppercase tracking-wider">System Status</div>
                        <div class="text-xs font-semibold text-white/90">Running Smoothly</div>
                    </div>
                </div>
            </div>
            <form method="post" action="/logout">
                <button class="btn btn-ghost btn-block btn-md justify-start gap-4 text-white/50 hover:text-white hover:bg-white/10 rounded-xl px-4 transition-all" type="submit">
                    <svg xmlns="http://www.w3.org/2000/svg" class="h-5 w-5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2"><path stroke-linecap="round" stroke-linejoin="round" d="M17 16l4-4m0 0l-4-4m4 4H7m6 4v1a3 3 0 01-3 3H6a3 3 0 01-3-3V7a3 3 0 013-3h4a3 3 0 013 3v1" /></svg>
                    <span class="font-medium text-[15px]">退出登录</span>
                </button>
            </form>
        </div>
      </div>
    </div>
  </div>
</body>
</html>"##;

pub const SIMPLE_LAYOUT: &str = r##"<!DOCTYPE html>
<html lang="zh-CN" data-theme="emerald">
<head>
  <meta charset="UTF-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1.0" />
  <title>{{title}}</title>
  <script src="https://unpkg.com/htmx.org@1.9.12"></script>
  <script src="https://cdn.tailwindcss.com"></script>
  <link href="https://cdn.jsdelivr.net/npm/daisyui@4.12.10/dist/full.min.css" rel="stylesheet" type="text/css" />
  <link href="https://fonts.googleapis.com/css2?family=Inter:wght@400;500;600;700&display=swap" rel="stylesheet">
  <script>
    tailwind.config = {
      theme: {
        extend: {
          fontFamily: {
            sans: ['Inter', 'system-ui', 'sans-serif'],
          },
          colors: {
            brand: '#346467',
          }
        }
      }
    }
  </script>
</head>
<body class="bg-[#F8FAFC] antialiased">
  {{body}}
</body>
</html>"##;

pub const LOGIN_PAGE: &str = r##"
<div class="min-h-screen flex items-center justify-center px-4 bg-[#F8FAFC]">
  <div class="card w-full max-w-md bg-white shadow-2xl border border-slate-200 rounded-xl overflow-hidden p-8">
    <div class="space-y-6">
      <div class="flex flex-col items-center gap-3">
          <div class="w-14 h-14 bg-brand rounded-2xl flex items-center justify-center shadow-lg shadow-brand/20">
              <svg xmlns="http://www.w3.org/2000/svg" class="h-8 w-8 text-white" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round">
                  <path d="m13 2-2 4h3l-2 4h3l-9 12 2-9h-3l2-4h-3l9-12z"></path>
              </svg>
          </div>
          <div class="text-center">
              <h1 class="text-2xl font-bold text-slate-900 tracking-tighter">UniGateway</h1>
              <p class="text-[13px] text-slate-500 font-medium mt-1">管理员登录</p>
          </div>
      </div>

      <div class="bg-slate-50 border border-slate-100 rounded-lg p-3 text-[12px] text-slate-500 font-medium text-center">
        默认凭据: <span class="font-bold text-slate-700">admin</span> / <span class="font-bold text-slate-700">admin123</span>
      </div>

      <form method="post" action="/login" class="space-y-4">
        <div class="form-control">
          <label class="label"><span class="label-text font-bold text-slate-500 text-xs">用户名</span></label>
          <input class="input h-11 bg-slate-50 border-slate-200 rounded-lg focus:border-brand/50 focus:ring-0 font-medium text-sm transition-all" name="username" placeholder="admin" value="admin" />
        </div>
        <div class="form-control">
          <label class="label"><span class="label-text font-bold text-slate-500 text-xs">密码</span></label>
          <input class="input h-11 bg-slate-50 border-slate-200 rounded-lg focus:border-brand/50 focus:ring-0 font-medium text-sm transition-all" type="password" name="password" placeholder="请输入密码" />
        </div>
        <button class="btn bg-brand hover:bg-brand/90 text-white border-none h-11 min-h-0 w-full rounded-xl font-bold shadow-md shadow-brand/10 mt-2" type="submit">登录系统</button>
      </form>
    </div>
  </div>
</div>
"##;

pub const LOGIN_ERROR_PAGE: &str = r##"
<div class="min-h-screen flex items-center justify-center px-4">
  <div class="alert alert-error max-w-md">
    <span>用户名或密码错误</span>
  </div>
  <div class="fixed bottom-8">
    <a class="btn btn-outline" href="/login">返回登录</a>
  </div>
</div>
"##;

pub const ADMIN_PAGE: &str = r##"
<div class="space-y-8">
  <div class="flex justify-between items-end">
    <div>
      <h2 class="text-3xl font-bold tracking-tighter text-slate-900">仪表盘</h2>
      <p class="text-slate-500 mt-1.5 text-[15px] font-medium">AI 网关运行实时概览</p>
    </div>
    <div class="text-xs font-bold text-slate-400 bg-slate-100/50 px-4 py-2 rounded-lg border border-slate-200">
      2024年4月12日
    </div>
  </div>

  <div
    id="stats-box"
    hx-get="/admin/stats"
    hx-trigger="load, every 10s"
    class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-4"
  >
    <div class="bg-white rounded-xl p-6 shadow-sm border border-slate-200 animate-pulse h-32"></div>
    <div class="bg-white rounded-xl p-6 shadow-sm border border-slate-200 animate-pulse h-32"></div>
    <div class="bg-white rounded-xl p-6 shadow-sm border border-slate-200 animate-pulse h-32"></div>
    <div class="bg-white rounded-xl p-6 shadow-sm border border-slate-200 animate-pulse h-32"></div>
  </div>

  <div class="grid grid-cols-1 xl:grid-cols-3 gap-6">
    <div class="xl:col-span-2 bg-white rounded-xl p-8 shadow-sm border border-slate-200">
      <div class="flex justify-between items-center mb-8">
        <div>
          <h3 class="text-lg font-bold text-slate-800 tracking-tight">请求量趋势 (近 7 天)</h3>
          <p class="text-xs text-slate-400 font-bold mt-1 uppercase tracking-wider">Daily Distribution</p>
        </div>
        <div class="flex gap-2 items-center">
            <div class="w-2.5 h-2.5 rounded-full bg-brand"></div>
            <span class="text-[10px] uppercase font-bold text-slate-400 tracking-widest">Requests</span>
        </div>
      </div>
      <div class="h-64 flex items-end justify-between gap-2 pt-4">
          <!-- Simple CSS Bar Chart Mockup -->
          <div class="flex-1 flex flex-col items-center gap-2">
              <div class="w-full bg-slate-50 rounded-lg relative h-32 overflow-hidden border border-slate-100">
                  <div class="absolute bottom-0 w-full bg-slate-900/10 h-24 rounded-t-sm transition-all hover:bg-brand active:scale-95 cursor-pointer"></div>
              </div>
              <span class="text-[10px] font-bold text-slate-400">周一</span>
          </div>
          <div class="flex-1 flex flex-col items-center gap-2">
              <div class="w-full bg-slate-50 rounded-lg relative h-32 overflow-hidden border border-slate-100">
                  <div class="absolute bottom-0 w-full bg-brand/80 h-16 rounded-t-sm transition-all hover:bg-brand active:scale-95 cursor-pointer"></div>
              </div>
              <span class="text-[10px] font-bold text-slate-400">周二</span>
          </div>
          <div class="flex-1 flex flex-col items-center gap-2">
              <div class="w-full bg-slate-50 rounded-lg relative h-32 overflow-hidden border border-slate-100">
                  <div class="absolute bottom-0 w-full bg-slate-900/10 h-28 rounded-t-sm transition-all hover:bg-brand active:scale-95 cursor-pointer"></div>
              </div>
              <span class="text-[10px] font-bold text-slate-400">周三</span>
          </div>
          <div class="flex-1 flex flex-col items-center gap-2">
              <div class="w-full bg-slate-50 rounded-lg relative h-32 overflow-hidden border border-slate-100">
                  <div class="absolute bottom-0 w-full bg-brand/80 h-20 rounded-t-sm transition-all hover:bg-brand active:scale-95 cursor-pointer"></div>
              </div>
              <span class="text-[10px] font-bold text-slate-400">周四</span>
          </div>
          <div class="flex-1 flex flex-col items-center gap-2">
              <div class="w-full bg-slate-50 rounded-lg relative h-32 overflow-hidden border border-slate-100">
                  <div class="absolute bottom-0 w-full bg-slate-900/10 h-32 rounded-t-sm transition-all hover:bg-brand active:scale-95 cursor-pointer"></div>
              </div>
              <span class="text-[10px] font-bold text-slate-400">周五</span>
          </div>
          <div class="flex-1 flex flex-col items-center gap-2">
              <div class="w-full bg-slate-50 rounded-lg relative h-32 overflow-hidden border border-slate-100">
                  <div class="absolute bottom-0 w-full bg-brand/80 h-14 rounded-t-sm transition-all hover:bg-brand active:scale-95 cursor-pointer"></div>
              </div>
              <span class="text-[10px] font-bold text-slate-400">周六</span>
          </div>
          <div class="flex-1 flex flex-col items-center gap-2">
              <div class="w-full bg-slate-50 rounded-lg relative h-32 overflow-hidden border border-slate-100">
                  <div class="absolute bottom-0 w-full bg-slate-900/10 h-22 rounded-t-sm transition-all hover:bg-brand active:scale-95 cursor-pointer"></div>
              </div>
              <span class="text-[10px] font-bold text-slate-400">周日</span>
          </div>
      </div>
    </div>

    <div class="bg-white rounded-xl p-8 shadow-sm border border-slate-200">
      <h3 class="text-lg font-bold text-slate-800 tracking-tight">核心接口</h3>
      <p class="text-xs text-slate-400 font-bold mt-1 uppercase tracking-wider mb-6">Active API Endpoints</p>
      <div class="space-y-3">
          <div class="group flex items-center justify-between p-4 bg-slate-50 hover:bg-brand/[0.04] border border-transparent hover:border-brand/10 rounded-xl transition-all cursor-default">
              <div class="flex items-center gap-3">
                  <div class="w-1 h-6 bg-brand rounded-full"></div>
                  <div>
                    <div class="text-[13px] font-bold text-slate-700 font-mono tracking-tight">/v1/chat/completions</div>
                    <div class="text-[10px] text-slate-400 font-bold uppercase tracking-widest mt-0.5">847 requests</div>
                  </div>
              </div>
              <span class="badge badge-sm bg-white border-slate-200 text-slate-400 font-bold text-[10px] uppercase rounded px-2.5 h-auto py-1 shadow-sm">OpenAI</span>
          </div>
          <div class="group flex items-center justify-between p-4 bg-slate-50 hover:bg-brand/[0.04] border border-transparent hover:border-brand/10 rounded-xl transition-all cursor-default">
              <div class="flex items-center gap-3">
                  <div class="w-1 h-6 bg-brand/40 rounded-full"></div>
                  <div>
                    <div class="text-[13px] font-bold text-slate-700 font-mono tracking-tight">/v1/messages</div>
                    <div class="text-[10px] text-slate-400 font-bold uppercase tracking-widest mt-0.5">437 requests</div>
                  </div>
              </div>
              <span class="badge badge-sm bg-white border-slate-200 text-slate-400 font-bold text-[10px] uppercase rounded px-2.5 h-auto py-1 shadow-sm">Anthropic</span>
          </div>
          <div class="group flex items-center justify-between p-4 bg-slate-50 hover:bg-brand/[0.04] border border-transparent hover:border-brand/10 rounded-xl transition-all cursor-default">
              <div class="flex items-center gap-3">
                  <div class="w-1 h-6 bg-brand/20 rounded-full"></div>
                  <div>
                    <div class="text-[13px] font-bold text-slate-700 font-mono tracking-tight">/v1/embeddings</div>
                    <div class="text-[10px] text-slate-400 font-bold uppercase tracking-widest mt-0.5">128 requests</div>
                  </div>
              </div>
              <span class="badge badge-sm bg-white border-slate-200 text-slate-400 font-bold text-[10px] uppercase rounded px-2.5 h-auto py-1 shadow-sm">OpenAI</span>
          </div>
      </div>
    </div>
  </div>
</div>
"##;

pub const STATS_PARTIAL: &str = r##"
<div class="bg-white rounded-xl p-6 shadow-sm border border-slate-200 flex flex-col justify-between group transition-all hover:border-brand/30">
  <div class="flex justify-between items-start mb-4">
    <div class="text-[11px] font-bold uppercase tracking-widest text-slate-400">总请求数</div>
    <div class="w-10 h-10 bg-slate-50 border border-slate-100 rounded-lg flex items-center justify-center text-slate-400 group-hover:bg-brand group-hover:text-white transition-colors">
      <svg xmlns="http://www.w3.org/2000/svg" class="h-5 w-5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2.5"><path stroke-linecap="round" stroke-linejoin="round" d="M13 16h-1v-4h-1m1-4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z" /></svg>
    </div>
  </div>
  <div>
    <div class="text-4xl font-bold text-slate-900 tracking-tighter mb-1">{{total}}</div>
    <div class="flex items-center gap-1.5 text-[11px] font-bold text-emerald-600">
        <svg xmlns="http://www.w3.org/2000/svg" class="h-3 w-3" viewBox="0 0 20 20" fill="currentColor"><path fill-rule="evenodd" d="M12 7a1 1 0 110-2h5a1 1 0 011 1v5a1 1 0 11-2 0V8.414l-4.293 4.293a1 1 0 01-1.414 0L8 10.414l-4.293 4.293a1 1 0 01-1.414-1.414l5-5a1 1 0 011.414 0L11 10.586 14.586 7H12z" clip-rule="evenodd" /></svg>
        +8% 较昨日
    </div>
  </div>
</div>

<div class="bg-white rounded-xl p-6 shadow-sm border border-slate-200 flex flex-col justify-between group transition-all hover:border-brand/30">
  <div class="flex justify-between items-start mb-4">
    <div class="text-[11px] font-bold uppercase tracking-widest text-slate-400">OpenAI 占比</div>
    <div class="w-10 h-10 bg-slate-50 border border-slate-100 rounded-lg flex items-center justify-center text-slate-400 group-hover:bg-brand group-hover:text-white transition-colors">
      <svg xmlns="http://www.w3.org/2000/svg" class="h-5 w-5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2.5"><path stroke-linecap="round" stroke-linejoin="round" d="M9 19v-6a2 2 0 00-2-2H5a2 2 0 00-2 2v6a2 2 0 002 2h2a2 2 0 002-2zm0 0V9a2 2 0 012-2h2a2 2 0 012 2v10m-6 0a2 2 0 002 2h2a2 2 0 002-2m0 0V5a2 2 0 012-2h2a2 2 0 012 2v14a2 2 0 01-2 2h-2a2 2 0 01-2-2z" /></svg>
    </div>
  </div>
  <div>
    <div class="text-4xl font-bold text-slate-900 tracking-tighter mb-1">{{openai_count}}</div>
    <div class="text-[11px] font-bold text-slate-400">通过 /v1/chat</div>
  </div>
</div>

<div class="bg-white rounded-xl p-6 shadow-sm border border-slate-200 flex flex-col justify-between group transition-all hover:border-brand/30">
  <div class="flex justify-between items-start mb-4">
    <div class="text-[11px] font-bold uppercase tracking-widest text-slate-400">Anthropic 占比</div>
    <div class="w-10 h-10 bg-slate-50 border border-slate-100 rounded-lg flex items-center justify-center text-slate-400 group-hover:bg-brand group-hover:text-white transition-colors">
      <svg xmlns="http://www.w3.org/2000/svg" class="h-5 w-5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2.5"><path stroke-linecap="round" stroke-linejoin="round" d="M20 7l-8-4-8 4m16 0l-8 4m8-4v10l-8 4m0-10L4 7m8 4v10M4 7v10l8 4" /></svg>
    </div>
  </div>
  <div>
    <div class="text-4xl font-bold text-slate-900 tracking-tighter mb-1">{{anthropic_count}}</div>
    <div class="text-[11px] font-bold text-slate-400">通过 /v1/messages</div>
  </div>
</div>

<div class="bg-white rounded-xl p-6 shadow-sm border border-slate-200 flex flex-col justify-between group transition-all hover:border-brand/30">
  <div class="flex justify-between items-start mb-4">
    <div class="text-[11px] font-bold uppercase tracking-widest text-slate-400">预估花费</div>
    <div class="w-10 h-10 bg-slate-50 border border-slate-100 rounded-lg flex items-center justify-center text-slate-400 group-hover:bg-brand group-hover:text-white transition-colors">
      <svg xmlns="http://www.w3.org/2000/svg" class="h-5 w-5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2.5"><path stroke-linecap="round" stroke-linejoin="round" d="M12 8c-1.657 0-3 .895-3 2s1.343 2 3 2 3 .895 3 2-1.343 2-3 2m0-8c1.11 0 2.08.402 2.599 1M12 8V7m0 1v8m0 0v1m0-1c-1.11 0-2.08-.402-2.599-1M21 12a9 9 0 11-18 0 9 9 0 0118 0z" /></svg>
    </div>
  </div>
  <div>
    <div class="text-4xl font-bold text-slate-900 tracking-tighter mb-1">$12.50</div>
    <div class="flex items-center gap-1.5 text-[11px] font-bold text-rose-600">
        <svg xmlns="http://www.w3.org/2000/svg" class="h-3 w-3" viewBox="0 0 20 20" fill="currentColor"><path fill-rule="evenodd" d="M12 13a1 1 0 110 2H7a1 1 0 01-1-1V9a1 1 0 112 0v3.586l4.293-4.293a1 1 0 011.414 0l4.293 4.293V11a1 1 0 112 0v5a1 1 0 01-1 1h-5a1 1 0 110-2h3.586L13 12.414l-1 1z" clip-rule="evenodd" /></svg>
        -2% 较上周
    </div>
  </div>
</div>
"##;

pub const PROVIDERS_PAGE: &str = r##"
<div class="space-y-8">
  <div class="flex justify-between items-end pb-4">
    <div>
      <h2 class="text-3xl font-bold tracking-tighter text-slate-900">模型供应商</h2>
      <p class="text-slate-500 mt-1.5 text-[15px] font-medium">配置并管理上游 AI 提供商</p>
    </div>
    <button
      onclick="document.getElementById('add_provider_modal').showModal()"
      class="btn bg-brand hover:bg-brand/90 text-white border-none rounded-xl font-bold shadow-lg shadow-brand/20 px-6 h-11 min-h-0"
    >
      <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4 mr-2" viewBox="0 0 20 20" fill="currentColor"><path fill-rule="evenodd" d="M10 3a1 1 0 011 1v5h5a1 1 0 110 2h-5v5a1 1 0 11-2 0v-5H4a1 1 0 110-2h5V4a1 1 0 011-1z" clip-rule="evenodd" /></svg>
      添加供应商
    </button>
  </div>

  <div class="bg-white rounded-xl shadow-sm border border-slate-200 overflow-hidden">
    <div class="overflow-x-auto">
      <table class="table w-full border-separate border-spacing-0">
        <thead>
          <tr class="bg-slate-50/50">
            <th class="py-4 px-8 text-[11px] font-bold uppercase tracking-widest text-slate-400 border-b border-slate-200">提供商名称</th>
            <th class="py-4 px-8 text-[11px] font-bold uppercase tracking-widest text-slate-400 border-b border-slate-200">API 类型</th>
            <th class="py-4 px-8 text-[11px] font-bold uppercase tracking-widest text-slate-400 border-b border-slate-200">Endpoint</th>
            <th class="py-4 px-8 text-[11px] font-bold uppercase tracking-widest text-slate-400 border-b border-slate-200 text-right">操作</th>
          </tr>
        </thead>
        <tbody id="providers-list" hx-get="/admin/providers/list" hx-trigger="load">
          <!-- Lists will be loaded here -->
        </tbody>
      </table>
    </div>
  </div>

  <dialog id="add_provider_modal" class="modal">
    <div class="modal-box bg-white rounded-xl p-8 max-w-md border border-slate-200 shadow-2xl">
      <h3 class="font-bold text-2xl text-slate-900 tracking-tight mb-6">添加供应商</h3>
      <form hx-post="/admin/providers/create" hx-target="#providers-list" hx-swap="innerHTML" class="space-y-5">
        <div class="form-control">
          <label class="label"><span class="label-text font-bold text-slate-500 text-xs">显示名称</span></label>
          <input name="name" type="text" placeholder="例如: OpenAI" class="input h-11 bg-slate-50 border-slate-200 rounded-lg focus:border-brand/50 focus:ring-0 font-medium text-sm transition-all" required />
        </div>
        <div class="form-control">
          <label class="label"><span class="label-text font-bold text-slate-500 text-xs">API 类型</span></label>
          <select name="api_type" class="select h-11 bg-slate-50 border-slate-200 rounded-lg focus:border-brand/50 focus:ring-0 font-medium text-sm transition-all">
            <option value="openai">OpenAI</option>
            <option value="anthropic">Anthropic</option>
          </select>
        </div>
        <div class="form-control">
          <label class="label"><span class="label-text font-bold text-slate-500 text-xs">Endpoint ID</span></label>
          <input name="endpoint_id" type="text" placeholder="例如: openai / deepseek" class="input h-11 bg-slate-50 border-slate-200 rounded-lg focus:border-brand/50 focus:ring-0 font-medium text-sm transition-all" required />
        </div>
        <div class="form-control">
          <label class="label"><span class="label-text font-bold text-slate-500 text-xs">Base URL 覆盖（可选）</span></label>
          <input name="base_url" type="text" placeholder="例如: https://api.deepseek.com" class="input h-11 bg-slate-50 border-slate-200 rounded-lg focus:border-brand/50 focus:ring-0 font-medium text-sm transition-all" />
        </div>
        <div class="form-control">
          <label class="label"><span class="label-text font-bold text-slate-500 text-xs">API Key</span></label>
          <input name="api_key" type="password" placeholder="sk-..." class="input h-11 bg-slate-50 border-slate-200 rounded-lg focus:border-brand/50 focus:ring-0 font-medium text-sm transition-all" required />
        </div>
        <div class="modal-action pt-4 flex gap-3">
          <button type="button" onclick="document.getElementById('add_provider_modal').close()" class="btn btn-ghost h-11 min-h-0 rounded-xl font-bold flex-1 border border-slate-200 text-slate-600">取消</button>
          <button type="submit" onclick="document.getElementById('add_provider_modal').close()" class="btn bg-brand hover:bg-brand/90 text-white border-none h-11 min-h-0 rounded-xl font-bold flex-1 shadow-md shadow-brand/10">确认添加</button>
        </div>
      </form>
    </div>
  </dialog>
</div>
"##;

pub const PROVIDERS_LIST_PARTIAL: &str = r##"
{{rows}}
"##;

pub const KEYS_LIST_PARTIAL: &str = r##"
{{rows}}
"##;

pub const LOGS_LIST_PARTIAL: &str = r##"
{{rows}}
"##;

pub const KEYS_PAGE: &str = r##"
<div class="space-y-8">
  <div class="flex justify-between items-end pb-4">
    <div>
      <h2 class="text-3xl font-bold tracking-tighter text-slate-900">API 令牌</h2>
      <p class="text-slate-500 mt-1.5 text-[15px] font-medium">管理访问此网关的授权 Key</p>
    </div>
    <button
      hx-post="/admin/api-keys/create"
      hx-target="#keys-list"
      class="btn bg-brand hover:bg-brand/90 text-white border-none rounded-xl font-bold shadow-lg shadow-brand/20 px-6 h-11 min-h-0"
    >
      <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4 mr-2" viewBox="0 0 20 20" fill="currentColor"><path fill-rule="evenodd" d="M10 3a1 1 0 011 1v5h5a1 1 0 110 2h-5v5a1 1 0 11-2 0v-5H4a1 1 0 110-2h5V4a1 1 0 011-1z" clip-rule="evenodd" /></svg>
      新建 Key
    </button>
  </div>

  <div class="bg-white rounded-xl shadow-sm border border-slate-200 overflow-hidden">
    <div class="overflow-x-auto">
      <table class="table w-full border-separate border-spacing-0">
        <thead>
          <tr class="bg-slate-50/50">
            <th class="py-4 px-8 text-[11px] font-bold uppercase tracking-widest text-slate-400 border-b border-slate-200">API Key</th>
            <th class="py-4 px-8 text-[11px] font-bold uppercase tracking-widest text-slate-400 border-b border-slate-200">状态</th>
            <th class="py-4 px-8 text-[11px] font-bold uppercase tracking-widest text-slate-400 border-b border-slate-200">创建时间</th>
            <th class="py-4 px-8 text-[11px] font-bold uppercase tracking-widest text-slate-400 border-b border-slate-200 text-right">操作</th>
          </tr>
        </thead>
        <tbody id="keys-list" hx-get="/admin/keys/list" hx-trigger="load">
          <!-- Lists will be loaded here -->
        </tbody>
      </table>
    </div>
  </div>
</div>
"##;

pub const LOGS_PAGE: &str = r##"
<div class="space-y-8">
  <div class="pb-4">
    <h2 class="text-3xl font-bold tracking-tighter text-slate-900">请求日志</h2>
    <p class="text-slate-500 mt-1.5 text-[15px] font-medium">实时监控 API 调用记录</p>
  </div>

  <div class="bg-white rounded-xl shadow-sm border border-slate-200 overflow-hidden">
    <div class="overflow-x-auto">
      <table class="table w-full border-separate border-spacing-0">
        <thead>
          <tr class="bg-slate-50/50">
            <th class="py-4 px-8 text-[11px] font-bold uppercase tracking-widest text-slate-400 border-b border-slate-200">时间</th>
            <th class="py-4 px-8 text-[11px] font-bold uppercase tracking-widest text-slate-400 border-b border-slate-200">路径</th>
            <th class="py-4 px-8 text-[11px] font-bold uppercase tracking-widest text-slate-400 border-b border-slate-200">状态</th>
            <th class="py-4 px-8 text-[11px] font-bold uppercase tracking-widest text-slate-400 border-b border-slate-200">耗时</th>
            <th class="py-4 px-8 text-[11px] font-bold uppercase tracking-widest text-slate-400 border-b border-slate-200">端点</th>
          </tr>
        </thead>
        <tbody id="logs-list" hx-get="/admin/logs/list" hx-trigger="load, every 30s">
          <!-- Logs will be loaded here -->
        </tbody>
      </table>
    </div>
  </div>
</div>
"##;

pub const SETTINGS_PAGE: &str = r##"
<div class="space-y-8">
  <div class="pb-4">
    <h2 class="text-3xl font-bold tracking-tighter text-slate-900">系统设置</h2>
    <p class="text-slate-500 mt-1.5 text-[15px] font-medium">配置网关全局参数</p>
  </div>

  <div class="grid grid-cols-1 lg:grid-cols-2 gap-6">
    <div class="bg-white rounded-xl p-8 shadow-sm border border-slate-200">
      <h3 class="text-lg font-bold text-slate-800 mb-6 tracking-tight">安全设置</h3>
      <div class="space-y-6">
        <div class="form-control">
          <label class="label"><span class="label-text font-bold text-slate-500 text-xs">管理员密码</span></label>
          <div class="join w-full">
            <input type="password" value="********" class="input h-11 bg-slate-50 border-slate-200 rounded-l-lg focus:border-brand/50 focus:ring-0 font-medium join-item flex-1 text-sm" readonly />
            <button class="btn bg-brand hover:bg-brand/90 text-white border-none join-item rounded-r-lg font-bold px-6 h-11 min-h-0">修改</button>
          </div>
        </div>
      </div>
    </div>

    <div class="bg-white rounded-xl p-8 shadow-sm border border-slate-200">
      <h3 class="text-lg font-bold text-slate-800 mb-6 tracking-tight">运行参数</h3>
      <div class="space-y-4">
        <label class="flex items-center justify-between p-4 bg-slate-50 rounded-lg cursor-pointer hover:bg-slate-100 transition-colors border border-transparent hover:border-slate-200">
          <div>
            <span class="block font-bold text-slate-700 text-sm">记录详细日志</span>
            <span class="block text-[10px] text-slate-400 font-bold uppercase tracking-widest mt-0.5">Full Request/Response Body</span>
          </div>
          <input type="checkbox" checked class="toggle toggle-success toggle-sm" />
        </label>
        <label class="flex items-center justify-between p-4 bg-slate-50 rounded-lg cursor-pointer hover:bg-slate-100 transition-colors border border-transparent hover:border-slate-200">
          <div>
            <span class="block font-bold text-slate-700 text-sm">开启公开注册</span>
            <span class="block text-[10px] text-slate-400 font-bold uppercase tracking-widest mt-0.5">Allow public signups</span>
          </div>
          <input type="checkbox" class="toggle toggle-success toggle-sm" />
        </label>
      </div>
    </div>
  </div>
</div>
"##;
