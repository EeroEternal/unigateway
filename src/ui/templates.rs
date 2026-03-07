pub const LAYOUT: &str = r##"<!DOCTYPE html>
<html lang="en" data-theme="emerald">
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
              <span class="font-medium text-[15px]">Dashboard</span>
            </a>
          </li>
          <li>
            <a hx-get="/admin/providers" hx-target="#main-content" hx-push-url="true" class="flex gap-3 px-4 py-3 rounded-xl transition-all duration-200 hover:bg-white/10 active:scale-[0.98]">
              <svg xmlns="http://www.w3.org/2000/svg" class="h-5 w-5 opacity-70" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2"><path stroke-linecap="round" stroke-linejoin="round" d="M21 12a9 9 0 01-9 9m9-9a9 9 0 00-9-9m9 9H3m9 9a9 9 0 01-9-9m9-9c1.657 0 3 4.03 3 9s-1.343 9-3 9m0-18c-1.657 0-3 4.03-3 9s1.343 9 3 9" /></svg>
              <span class="font-medium text-[15px]">Providers</span>
            </a>
          </li>
          <li>
            <a hx-get="/admin/api-keys" hx-target="#main-content" hx-push-url="true" class="flex gap-3 px-4 py-3 rounded-xl transition-all duration-200 hover:bg-white/10 active:scale-[0.98]">
              <svg xmlns="http://www.w3.org/2000/svg" class="h-5 w-5 opacity-70" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2"><path stroke-linecap="round" stroke-linejoin="round" d="M15 7a2 2 0 012 2m4 0a6 6 0 01-7.743 5.743L11 17H9v2H7v2H4a1 1 0 01-1-1v-2.586a1 1 0 01.293-.707l5.964-5.964A6 6 0 1121 9z" /></svg>
              <span class="font-medium text-[15px]">API Keys</span>
            </a>
          </li>
          <li>
            <a hx-get="/admin/services" hx-target="#main-content" hx-push-url="true" class="flex gap-3 px-4 py-3 rounded-xl transition-all duration-200 hover:bg-white/10 active:scale-[0.98]">
              <svg xmlns="http://www.w3.org/2000/svg" class="h-5 w-5 opacity-70" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2"><path stroke-linecap="round" stroke-linejoin="round" d="M4 7h16M7 12h10M9 17h6" /></svg>
              <span class="font-medium text-[15px]">Services</span>
            </a>
          </li>
          <li>
            <a hx-get="/admin/logs" hx-target="#main-content" hx-push-url="true" class="flex gap-3 px-4 py-3 rounded-xl transition-all duration-200 hover:bg-white/10 active:scale-[0.98]">
              <svg xmlns="http://www.w3.org/2000/svg" class="h-5 w-5 opacity-70" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2"><path stroke-linecap="round" stroke-linejoin="round" d="M9 12h6m-6 4h6m2 5H7a2 2 0 01-2-2V5a2 2 0 012-2h5.586a1 1 0 01.707.293l5.414 5.414a1 1 0 01.293.707V19a2 2 0 01-2 2z" /></svg>
              <span class="font-medium text-[15px]">Request Logs</span>
            </a>
          </li>
          <li>
            <a hx-get="/admin/settings" hx-target="#main-content" hx-push-url="true" class="flex gap-3 px-4 py-3 rounded-xl transition-all duration-200 hover:bg-white/10 active:scale-[0.98]">
              <svg xmlns="http://www.w3.org/2000/svg" class="h-5 w-5 opacity-70" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2"><path stroke-linecap="round" stroke-linejoin="round" d="M10.325 4.317c.426-1.756 2.924-1.756 3.35 0a1.724 1.724 0 002.573 1.066c1.543-.94 3.31.826 2.37 2.37a1.724 1.724 0 001.065 2.572c1.756.426 1.756 2.924 0 3.35a1.724 1.724 0 00-1.066 2.573c.94 1.543-.826 3.31-2.37 2.37a1.724 1.724 0 00-2.572 1.065c-.426 1.756-2.924 1.756-3.35 0a1.724 1.724 0 00-2.573-1.066c-1.543.94-3.31-.826-2.37-2.37a1.724 1.724 0 00-1.065-2.572c-1.756-.426-1.756-2.924 0-3.35a1.724 1.724 0 001.066-2.573c-.94-1.543.826-3.31 2.37-2.37.996.608 2.296.07 2.572-1.065z" /><path stroke-linecap="round" stroke-linejoin="round" d="M15 12a3 3 0 11-6 0 3 3 0 016 0z" /></svg>
              <span class="font-medium text-[15px]">Settings</span>
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
                    <span class="font-medium text-[15px]">Logout</span>
                </button>
            </form>
        </div>
      </div>
    </div>
  </div>
</body>
</html>"##;

pub const SIMPLE_LAYOUT: &str = r##"<!DOCTYPE html>
<html lang="en" data-theme="emerald">
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
              <p class="text-[13px] text-slate-500 font-medium mt-1">Admin Login</p>
          </div>
      </div>

      <div class="bg-slate-50 border border-slate-100 rounded-lg p-3 text-[12px] text-slate-500 font-medium text-center">
        Default Credentials: <span class="font-bold text-slate-700">admin</span> / <span class="font-bold text-slate-700">admin123</span>
      </div>

      <form method="post" action="/login" class="space-y-4">
        <div class="form-control">
          <label class="label"><span class="label-text font-bold text-slate-500 text-xs">Username</span></label>
          <input class="input h-11 bg-slate-50 border-slate-200 rounded-lg focus:border-brand/50 focus:ring-0 font-medium text-sm transition-all" name="username" placeholder="admin" value="admin" />
        </div>
        <div class="form-control">
          <label class="label"><span class="label-text font-bold text-slate-500 text-xs">Password</span></label>
          <input class="input h-11 bg-slate-50 border-slate-200 rounded-lg focus:border-brand/50 focus:ring-0 font-medium text-sm transition-all" type="password" name="password" placeholder="Enter password" />
        </div>
        <button class="btn bg-brand hover:bg-brand/90 text-white border-none h-11 min-h-0 w-full rounded-xl font-bold shadow-md shadow-brand/10 mt-2" type="submit">Login</button>
      </form>
    </div>
  </div>
</div>
"##;

pub const LOGIN_ERROR_PAGE: &str = r##"
<div class="min-h-screen flex items-center justify-center px-4">
  <div class="alert alert-error max-w-md">
    <span>Invalid username or password</span>
  </div>
  <div class="fixed bottom-8">
    <a class="btn btn-outline" href="/login">Back to Login</a>
  </div>
</div>
"##;

pub const ADMIN_PAGE: &str = r##"
<div class="space-y-8">
  <div class="flex justify-between items-end">
    <div>
      <h2 class="text-3xl font-bold tracking-tighter text-slate-900">Dashboard</h2>
      <p class="text-slate-500 mt-1.5 text-[15px] font-medium">Real-time overview of AI Gateway</p>
    </div>
    <div class="flex items-center gap-2 text-xs font-bold text-emerald-700 bg-emerald-50 px-4 py-2 rounded-lg border border-emerald-200">
      <span class="w-2 h-2 rounded-full bg-emerald-500"></span>
      Gateway Healthy
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
          <h3 class="text-lg font-bold text-slate-800 tracking-tight">Request Trend (Last 7 Days)</h3>
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
              <span class="text-[10px] font-bold text-slate-400">Mon</span>
          </div>
          <div class="flex-1 flex flex-col items-center gap-2">
              <div class="w-full bg-slate-50 rounded-lg relative h-32 overflow-hidden border border-slate-100">
                  <div class="absolute bottom-0 w-full bg-brand/80 h-16 rounded-t-sm transition-all hover:bg-brand active:scale-95 cursor-pointer"></div>
              </div>
              <span class="text-[10px] font-bold text-slate-400">Tue</span>
          </div>
          <div class="flex-1 flex flex-col items-center gap-2">
              <div class="w-full bg-slate-50 rounded-lg relative h-32 overflow-hidden border border-slate-100">
                  <div class="absolute bottom-0 w-full bg-slate-900/10 h-28 rounded-t-sm transition-all hover:bg-brand active:scale-95 cursor-pointer"></div>
              </div>
              <span class="text-[10px] font-bold text-slate-400">Wed</span>
          </div>
          <div class="flex-1 flex flex-col items-center gap-2">
              <div class="w-full bg-slate-50 rounded-lg relative h-32 overflow-hidden border border-slate-100">
                  <div class="absolute bottom-0 w-full bg-brand/80 h-20 rounded-t-sm transition-all hover:bg-brand active:scale-95 cursor-pointer"></div>
              </div>
              <span class="text-[10px] font-bold text-slate-400">Thu</span>
          </div>
          <div class="flex-1 flex flex-col items-center gap-2">
              <div class="w-full bg-slate-50 rounded-lg relative h-32 overflow-hidden border border-slate-100">
                  <div class="absolute bottom-0 w-full bg-slate-900/10 h-32 rounded-t-sm transition-all hover:bg-brand active:scale-95 cursor-pointer"></div>
              </div>
              <span class="text-[10px] font-bold text-slate-400">Fri</span>
          </div>
          <div class="flex-1 flex flex-col items-center gap-2">
              <div class="w-full bg-slate-50 rounded-lg relative h-32 overflow-hidden border border-slate-100">
                  <div class="absolute bottom-0 w-full bg-brand/80 h-14 rounded-t-sm transition-all hover:bg-brand active:scale-95 cursor-pointer"></div>
              </div>
              <span class="text-[10px] font-bold text-slate-400">Sat</span>
          </div>
          <div class="flex-1 flex flex-col items-center gap-2">
              <div class="w-full bg-slate-50 rounded-lg relative h-32 overflow-hidden border border-slate-100">
                  <div class="absolute bottom-0 w-full bg-slate-900/10 h-22 rounded-t-sm transition-all hover:bg-brand active:scale-95 cursor-pointer"></div>
              </div>
              <span class="text-[10px] font-bold text-slate-400">Sun</span>
          </div>
      </div>
    </div>

    <div class="bg-white rounded-xl p-8 shadow-sm border border-slate-200">
      <h3 class="text-lg font-bold text-slate-800 tracking-tight">Core Endpoints</h3>
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
    <div class="text-[11px] font-bold uppercase tracking-widest text-slate-400">TOTAL REQUESTS</div>
    <div class="w-10 h-10 bg-slate-50 border border-slate-100 rounded-lg flex items-center justify-center text-slate-400 group-hover:bg-brand group-hover:text-white transition-colors">
      <svg xmlns="http://www.w3.org/2000/svg" class="h-5 w-5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2.5"><path stroke-linecap="round" stroke-linejoin="round" d="M13 16h-1v-4h-1m1-4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z" /></svg>
    </div>
  </div>
  <div>
    <div class="text-4xl font-bold text-slate-900 tracking-tighter mb-1">{{total}}</div>
    <div class="flex items-center gap-1.5 text-[11px] font-bold text-emerald-600">
        <svg xmlns="http://www.w3.org/2000/svg" class="h-3 w-3" viewBox="0 0 20 20" fill="currentColor"><path fill-rule="evenodd" d="M12 7a1 1 0 110-2h5a1 1 0 011 1v5a1 1 0 11-2 0V8.414l-4.293 4.293a1 1 0 01-1.414 0L8 10.414l-4.293 4.293a1 1 0 01-1.414-1.414l5-5a1 1 0 011.414 0L11 10.586 14.586 7H12z" clip-rule="evenodd" /></svg>
        +8% vs Yesterday
    </div>
   </div>
 </div>

 <div class="bg-white rounded-xl p-6 shadow-sm border border-slate-200 flex flex-col justify-between group transition-all hover:border-brand/30">
   <div class="flex justify-between items-start mb-4">
    <div class="text-[11px] font-bold uppercase tracking-widest text-slate-400">API KEYS</div>
    <div class="w-10 h-10 bg-slate-50 border border-slate-100 rounded-lg flex items-center justify-center text-slate-400 group-hover:bg-brand group-hover:text-white transition-colors">
      <svg xmlns="http://www.w3.org/2000/svg" class="h-5 w-5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2.5"><path stroke-linecap="round" stroke-linejoin="round" d="M15 7a2 2 0 012 2m4 0a6 6 0 01-7.743 5.743L11 17H9v2H7v2H4a1 1 0 01-1-1v-2.586a1 1 0 01.293-.707l5.964-5.964A6 6 0 1121 9z" /></svg>
    </div>
  </div>
  <div>
    <div class="text-4xl font-bold text-slate-900 tracking-tighter mb-1">{{api_keys}}</div>
    <div class="text-[11px] font-bold text-slate-400">active gateway tokens</div>
  </div>
</div>

<div class="bg-white rounded-xl p-6 shadow-sm border border-slate-200 flex flex-col justify-between group transition-all hover:border-brand/30">
  <div class="flex justify-between items-start mb-4">
    <div class="text-[11px] font-bold uppercase tracking-widest text-slate-400">PROVIDERS</div>
    <div class="w-10 h-10 bg-slate-50 border border-slate-100 rounded-lg flex items-center justify-center text-slate-400 group-hover:bg-brand group-hover:text-white transition-colors">
      <svg xmlns="http://www.w3.org/2000/svg" class="h-5 w-5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2.5"><path stroke-linecap="round" stroke-linejoin="round" d="M17 20h5V4H2v16h5m10 0v-8H7v8m10 0H7" /></svg>
    </div>
  </div>
  <div>
    <div class="text-4xl font-bold text-slate-900 tracking-tighter mb-1">{{providers}}</div>
    <div class="text-[11px] font-bold text-slate-400">configured upstreams</div>
  </div>
</div>

  <div class="bg-white rounded-xl p-6 shadow-sm border border-slate-200 flex flex-col justify-between group transition-all hover:border-brand/30">
    <div class="flex justify-between items-start mb-4">
      <div class="text-[11px] font-bold uppercase tracking-widest text-slate-400">SERVICES</div>
      <div class="w-10 h-10 bg-slate-50 border border-slate-100 rounded-lg flex items-center justify-center text-slate-400 group-hover:bg-brand group-hover:text-white transition-colors">
        <svg xmlns="http://www.w3.org/2000/svg" class="h-5 w-5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2.5"><path stroke-linecap="round" stroke-linejoin="round" d="M3 7h18M6 12h12M9 17h6" /></svg>
      </div>
    </div>
    <div>
      <div class="text-4xl font-bold text-slate-900 tracking-tighter mb-1">{{services}}</div>
      <div class="text-[11px] font-bold text-slate-400">routing services available</div>
    </div>
  </div>
"##;

pub const PROVIDERS_PAGE: &str = r##"
<div class="space-y-8">
  <div class="flex justify-between items-end pb-4">
    <div>
      <h2 class="text-3xl font-bold tracking-tighter text-slate-900">Model Providers</h2>
      <p class="text-slate-500 mt-1.5 text-[15px] font-medium">Configure and manage upstream AI providers</p>
    </div>
    <button
      onclick="document.getElementById('add_provider_modal').showModal()"
      class="btn bg-brand hover:bg-brand/90 text-white border-none rounded-xl font-bold shadow-lg shadow-brand/20 px-6 h-11 min-h-0"
    >
      <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4 mr-2" viewBox="0 0 20 20" fill="currentColor"><path fill-rule="evenodd" d="M10 3a1 1 0 011 1v5h5a1 1 0 110 2h-5v5a1 1 0 11-2 0v-5H4a1 1 0 110-2h5V4a1 1 0 011-1z" clip-rule="evenodd" /></svg>
      Add Provider
    </button>
  </div>

  <div class="bg-white rounded-xl shadow-sm border border-slate-200 overflow-hidden">
    <div class="overflow-x-auto">
      <table class="table w-full border-separate border-spacing-0">
        <thead>
          <tr class="bg-slate-50/50">
            <th class="py-4 px-8 text-[11px] font-bold uppercase tracking-widest text-slate-400 border-b border-slate-200">DISPLAY NAME</th>
            <th class="py-4 px-8 text-[11px] font-bold uppercase tracking-widest text-slate-400 border-b border-slate-200">PROVIDER NAME</th>
            <th class="py-4 px-8 text-[11px] font-bold uppercase tracking-widest text-slate-400 border-b border-slate-200">ENDPOINT</th>
            <th class="py-4 px-8 text-[11px] font-bold uppercase tracking-widest text-slate-400 border-b border-slate-200">BOUND SERVICES</th>
            <th class="py-4 px-8 text-[11px] font-bold uppercase tracking-widest text-slate-400 border-b border-slate-200 text-right">ACTIONS</th>
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
      <h3 class="font-bold text-2xl text-slate-900 tracking-tight mb-6">Add Provider</h3>
      <form id="add_provider_form" hx-post="/admin/providers/create" hx-target="#providers-list" hx-swap="innerHTML" hx-on::after-request="if(event.detail.successful) this.reset()" class="space-y-5">
        <div class="form-control">
          <label class="label"><span class="label-text font-bold text-slate-500 text-xs">Display Name</span></label>
          <input name="name" type="text" placeholder="e.g. OpenAI" class="input h-11 bg-slate-50 border-slate-200 rounded-lg focus:border-brand/50 focus:ring-0 font-medium text-sm transition-all" required />
        </div>
        <div class="form-control">
          <label class="label"><span class="label-text font-bold text-slate-500 text-xs">Provider Name</span></label>
          <select name="provider_type" class="select select-bordered w-full bg-slate-50 text-sm" onchange="updatePlaceholders(this)">
            <option disabled selected>Select Provider</option>
            {{provider_options}}
          </select>
        </div>
        <div class="form-control">
          <label class="label"><span class="label-text font-bold text-slate-500 text-xs">Endpoint ID</span></label>
          <input name="endpoint_id" type="text" placeholder="e.g. openai / deepseek" class="input h-11 bg-slate-50 border-slate-200 rounded-lg focus:border-brand/50 focus:ring-0 font-medium text-sm transition-all" required />
        </div>
        <div class="form-control">
          <label class="label"><span class="label-text font-bold text-slate-500 text-xs">Base URL Override (Optional)</span></label>
          <input name="base_url" type="text" placeholder="e.g. https://api.deepseek.com" class="input h-11 bg-slate-50 border-slate-200 rounded-lg focus:border-brand/50 focus:ring-0 font-medium text-sm transition-all" />
        </div>
        <div class="form-control">
          <label class="label"><span class="label-text font-bold text-slate-500 text-xs">API Key</span></label>
          <input name="api_key" type="password" placeholder="sk-..." class="input h-11 bg-slate-50 border-slate-200 rounded-lg focus:border-brand/50 focus:ring-0 font-medium text-sm transition-all" required />
        </div>
        <div class="modal-action pt-4 flex gap-3">
          <button type="button" onclick="document.getElementById('add_provider_modal').close()" class="btn btn-ghost h-11 min-h-0 rounded-xl font-bold flex-1 border border-slate-200 text-slate-600">Cancel</button>
          <button type="submit" onclick="document.getElementById('add_provider_modal').close()" class="btn bg-brand hover:bg-brand/90 text-white border-none h-11 min-h-0 rounded-xl font-bold flex-1 shadow-md shadow-brand/10">Confirm Add</button>
        </div>
      </form>
    </div>
  </dialog>
  <script>
    function openProviderDetail(providerId) {
      if (window.htmx) {
        window.htmx.ajax('GET', `/admin/providers/${encodeURIComponent(providerId)}`, '#main-content');
        if (history && history.pushState) {
          history.pushState({}, '', `/admin/providers/${encodeURIComponent(providerId)}`);
        }
      } else {
        window.location.href = `/admin/providers/${encodeURIComponent(providerId)}`;
      }
    }

    function updatePlaceholders(select) {
      const type = select.value;
      const modal = document.getElementById('add_provider_modal');
      const nameInput = modal.querySelector('input[name="name"]');
      const endpointInput = modal.querySelector('input[name="endpoint_id"]');
      const baseUrlInput = modal.querySelector('input[name="base_url"]');

      const placeholders = {
        'openai': { name: 'OpenAI', endpoint: 'openai', url: 'https://api.openai.com/v1' },
        'anthropic': { name: 'Anthropic', endpoint: 'anthropic', url: 'https://api.anthropic.com' },
        'azure-openai': { name: 'Azure OpenAI', endpoint: 'azure-openai', url: 'https://{resource}.openai.azure.com' },
        'google': { name: 'Google Gemini', endpoint: 'google', url: 'https://generativelanguage.googleapis.com' },
        'vertex-ai': { name: 'Vertex AI', endpoint: 'vertex-ai', url: 'https://{region}-aiplatform.googleapis.com' },
        'aws-bedrock': { name: 'AWS Bedrock', endpoint: 'aws-bedrock', url: 'https://bedrock-runtime.{region}.amazonaws.com' },
        'deepseek': { name: 'DeepSeek', endpoint: 'deepseek', url: 'https://api.deepseek.com' },
        'groq': { name: 'Groq', endpoint: 'groq', url: 'https://api.groq.com/openai/v1' },
        'xai': { name: 'xAI', endpoint: 'xai', url: 'https://api.x.ai/v1' },
        'cohere': { name: 'Cohere', endpoint: 'cohere', url: 'https://api.cohere.ai/v1' },
        'cloudflare': { name: 'Cloudflare', endpoint: 'cloudflare', url: 'https://api.cloudflare.com/client/v4/accounts/{id}/ai/run' },
        'ollama': { name: 'Ollama', endpoint: 'ollama', url: 'http://localhost:11434' },
      };

      const config = placeholders[type];
      if (config) {
        if (nameInput) nameInput.placeholder = 'e.g. ' + config.name;
        if (endpointInput) endpointInput.placeholder = 'e.g. ' + config.endpoint;
        if (baseUrlInput) baseUrlInput.placeholder = 'e.g. ' + config.url;
      }
    }
  </script>
</div>
"##;

pub const PROVIDER_DETAIL_PAGE: &str = r##"
<div class="space-y-8">
  <div class="flex flex-col gap-3 lg:flex-row lg:items-end lg:justify-between">
    <div>
      <h2 class="text-3xl font-bold tracking-tighter text-slate-900">{{provider_name}}</h2>
      <p class="text-slate-500 mt-1.5 text-[15px] font-medium">Review provider settings and all bound services</p>
    </div>
    <button onclick="window.htmx ? window.htmx.ajax('GET', '/admin/providers', '#main-content') : window.location.href='/admin/providers'" class="btn h-11 min-h-0 rounded-xl border border-slate-200 bg-white px-5 font-bold text-slate-700">Back to Providers</button>
  </div>

  <div class="grid grid-cols-1 xl:grid-cols-2 gap-6">
    <div class="bg-white rounded-xl shadow-sm border border-slate-200 p-6 space-y-4">
      <div>
        <div class="text-[11px] font-bold uppercase tracking-widest text-slate-400 mb-2">Provider Name</div>
        <div class="inline-flex items-center rounded-md border border-slate-200 bg-slate-50 px-3 py-1.5 text-[11px] font-bold uppercase tracking-widest text-slate-600">{{provider_type}}</div>
      </div>
      <div>
        <div class="text-[11px] font-bold uppercase tracking-widest text-slate-400 mb-2">Endpoint ID</div>
        <code class="block rounded-lg border border-slate-100 bg-slate-50 px-3 py-2 text-[12px] font-mono text-slate-600">{{endpoint_id}}</code>
      </div>
      <div>
        <div class="text-[11px] font-bold uppercase tracking-widest text-slate-400 mb-2">Base URL</div>
        <code class="block rounded-lg border border-slate-100 bg-slate-50 px-3 py-2 text-[12px] font-mono text-slate-600 break-all">{{base_url}}</code>
      </div>
    </div>

    <div class="bg-white rounded-xl shadow-sm border border-slate-200 overflow-hidden">
      <div class="px-6 py-5 border-b border-slate-200 bg-slate-50/50">
        <h3 class="text-lg font-bold text-slate-900">Bound Services</h3>
      </div>
      <div class="overflow-x-auto">
        <table class="table w-full border-separate border-spacing-0">
          <thead>
            <tr class="bg-slate-50/50">
              <th class="py-4 px-6 text-[11px] font-bold uppercase tracking-widest text-slate-400 border-b border-slate-200">Name</th>
              <th class="py-4 px-6 text-[11px] font-bold uppercase tracking-widest text-slate-400 border-b border-slate-200">Service ID</th>
              <th class="py-4 px-6 text-[11px] font-bold uppercase tracking-widest text-slate-400 border-b border-slate-200">Created At</th>
            </tr>
          </thead>
          <tbody>
            {{service_rows}}
          </tbody>
        </table>
      </div>
    </div>
  </div>

  <script>
    function openServiceDetail(serviceId) {
      if (window.htmx) {
        window.htmx.ajax('GET', `/admin/services/${encodeURIComponent(serviceId)}`, '#main-content');
        if (history && history.pushState) {
          history.pushState({}, '', `/admin/services/${encodeURIComponent(serviceId)}`);
        }
      } else {
        window.location.href = `/admin/services/${encodeURIComponent(serviceId)}`;
      }
    }
  </script>
</div>
"##;

pub const PROVIDERS_LIST_PARTIAL: &str = r##"
{{rows}}
"##;

pub const KEYS_LIST_PARTIAL: &str = r##"
{{rows}}
"##;

pub const SERVICES_LIST_PARTIAL: &str = r##"
{{rows}}
"##;

pub const KEYS_PAGE: &str = r##"
<div class="space-y-8">
  <div class="flex justify-between items-end pb-4">
    <div>
      <h2 class="text-3xl font-bold tracking-tighter text-slate-900">API Keys</h2>
      <p class="text-slate-500 mt-1.5 text-[15px] font-medium">Manage gateway access keys</p>
    </div>
    <button
      onclick="openAddKeyModal()"
      class="btn bg-brand hover:bg-brand/90 text-white border-none rounded-xl font-bold shadow-lg shadow-brand/20 px-6 h-11 min-h-0"
    >
      <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4 mr-2" viewBox="0 0 20 20" fill="currentColor"><path fill-rule="evenodd" d="M10 3a1 1 0 011 1v5h5a1 1 0 110 2h-5v5a1 1 0 11-2 0v-5H4a1 1 0 110-2h5V4a1 1 0 011-1z" clip-rule="evenodd" /></svg>
      New Key
    </button>
  </div>

  <div class="bg-white rounded-xl shadow-sm border border-slate-200 overflow-hidden">
    <div class="overflow-x-auto">
      <table class="table w-full border-separate border-spacing-0">
        <thead>
          <tr class="bg-slate-50/50">
            <th class="py-4 px-8 text-[11px] font-bold uppercase tracking-widest text-slate-400 border-b border-slate-200">Name</th>
            <th class="py-4 px-8 text-[11px] font-bold uppercase tracking-widest text-slate-400 border-b border-slate-200">API Key</th>
            <th class="py-4 px-8 text-[11px] font-bold uppercase tracking-widest text-slate-400 border-b border-slate-200">Status</th>
            <th class="py-4 px-8 text-[11px] font-bold uppercase tracking-widest text-slate-400 border-b border-slate-200">Service</th>
            <th class="py-4 px-8 text-[11px] font-bold uppercase tracking-widest text-slate-400 border-b border-slate-200">Created At</th>
            <th class="py-4 px-8 text-[11px] font-bold uppercase tracking-widest text-slate-400 border-b border-slate-200 text-right">Actions</th>
          </tr>
        </thead>
        <tbody id="keys-list" hx-get="/admin/api-keys/list" hx-trigger="load">
        </tbody>
      </table>
    </div>
  </div>

  <dialog id="add_key_modal" class="modal">
    <div class="modal-box bg-white rounded-xl p-8 max-w-md border border-slate-200 shadow-2xl">
      <h3 class="font-bold text-2xl text-slate-900 tracking-tight mb-6">Create API Key</h3>
      <div id="add_key_error" class="hidden mb-4 rounded-lg border border-rose-200 bg-rose-50 px-4 py-3 text-sm font-medium text-rose-700"></div>
      <form id="add_key_form" hx-post="/admin/api-keys/create" hx-target="#keys-list" hx-swap="innerHTML" class="space-y-5">
        <div class="form-control">
          <label class="label"><span class="label-text font-bold text-slate-500 text-xs">Name</span></label>
          <input name="name" type="text" placeholder="e.g. Web App" class="input h-11 bg-slate-50 border-slate-200 rounded-lg focus:border-brand/50 focus:ring-0 font-medium text-sm transition-all" required />
        </div>
        <div class="form-control">
          <label class="label"><span class="label-text font-bold text-slate-500 text-xs">Providers</span></label>
          <div class="max-h-44 overflow-y-auto rounded-lg border border-slate-200 bg-slate-50 p-2 space-y-2">
            {{provider_multi_items}}
          </div>
          <div class="text-[11px] text-slate-400 font-medium mt-2">Selected providers will form a service. A service is the routing group used by this key, and can be reviewed or updated later on the Services page.</div>
        </div>
        <div class="modal-action pt-4 flex gap-3">
          <button type="button" onclick="document.getElementById('add_key_modal').close()" class="btn btn-ghost h-11 min-h-0 rounded-xl font-bold flex-1 border border-slate-200 text-slate-600">Cancel</button>
          <button id="create_token_submit" type="submit" class="btn bg-brand hover:bg-brand/90 text-white border-none h-11 min-h-0 rounded-xl font-bold flex-1 shadow-md shadow-brand/10">Create</button>
        </div>
      </form>
    </div>
  </dialog>

  <dialog id="token_created_modal" class="modal">
    <div class="modal-box bg-white rounded-xl p-8 max-w-xl border border-slate-200 shadow-2xl">
      <h3 class="font-bold text-2xl text-slate-900 tracking-tight mb-3">Created</h3>
      <p class="text-sm text-slate-500 font-medium mb-5">Copy and save this API key now. It will only be shown once after creation.</p>
      <div class="space-y-4">
        <div>
          <div class="text-[11px] font-bold uppercase tracking-widest text-slate-400 mb-2">API KEY</div>
          <div class="flex gap-2">
            <input id="created_token_value" type="text" readonly class="input h-11 bg-slate-50 border-slate-200 rounded-lg flex-1 font-mono text-sm" />
            <button type="button" onclick="copyCreatedToken()" class="btn h-11 min-h-0 rounded-xl border border-slate-200 bg-white text-slate-700 font-bold">Copy</button>
          </div>
        </div>
        <div>
          <div class="text-[11px] font-bold uppercase tracking-widest text-slate-400 mb-2">Service Name</div>
          <div id="created_token_service_name" class="rounded-lg border border-slate-200 bg-slate-50 px-4 py-3 text-sm font-semibold text-slate-700"></div>
        </div>
        <div>
          <div class="text-[11px] font-bold uppercase tracking-widest text-slate-400 mb-2">Service ID</div>
          <div id="created_token_service" class="rounded-lg border border-slate-200 bg-slate-50 px-4 py-3 text-sm font-semibold text-slate-700"></div>
        </div>
        <div class="text-[12px] leading-6 text-slate-500">A service is the routing group used by this key. You can review its bound providers and update it later from the Services page.</div>
        <div class="flex flex-wrap gap-3">
          <button type="button" onclick="openServicesPageAndCloseResult()" class="btn h-10 min-h-0 rounded-xl border border-slate-200 bg-white px-4 font-bold text-slate-700">Open Services</button>
        </div>
      </div>
      <div class="modal-action pt-6">
        <button type="button" onclick="document.getElementById('token_created_modal').close()" class="btn bg-brand hover:bg-brand/90 text-white border-none h-11 min-h-0 rounded-xl font-bold px-6">Done</button>
      </div>
    </div>
  </dialog>
  <div id="toast_message" class="hidden fixed right-6 top-6 z-[100] rounded-xl border border-emerald-200 bg-white px-4 py-3 shadow-xl">
    <div id="toast_message_text" class="text-sm font-semibold text-slate-700"></div>
  </div>
  <script>
    let toastTimer;

    function showToast(message) {
      const wrap = document.getElementById('toast_message');
      const text = document.getElementById('toast_message_text');
      if (!wrap || !text) return;
      text.textContent = message;
      wrap.classList.remove('hidden');
      clearTimeout(toastTimer);
      toastTimer = setTimeout(() => wrap.classList.add('hidden'), 2200);
    }

    function setCreateTokenSubmitting(isSubmitting) {
      const button = document.getElementById('create_token_submit');
      if (!button) return;
      button.disabled = isSubmitting;
      button.textContent = isSubmitting ? 'Creating...' : 'Create';
    }

    function showCreateTokenError(message) {
      const el = document.getElementById('add_key_error');
      if (!el) return;
      if (message) {
        el.textContent = message;
        el.classList.remove('hidden');
      } else {
        el.textContent = '';
        el.classList.add('hidden');
      }
    }

    function openAddKeyModal() {
      const form = document.getElementById('add_key_form');
      if (form) form.reset();
      showCreateTokenError('');
      setCreateTokenSubmitting(false);
      document.querySelectorAll('#add_key_form input[name="provider_ids[]"]').forEach((el) => {
        el.checked = false;
      });
      const modal = document.getElementById('add_key_modal');
      if (modal) modal.showModal();
    }

    function openServicesPage() {
      if (window.htmx) {
        window.htmx.ajax('GET', '/admin/services', '#main-content');
        if (history && history.pushState) {
          history.pushState({}, '', '/admin/services');
        }
      } else {
        window.location.href = '/admin/services';
      }
    }

    function openServicesPageAndCloseResult() {
      const modal = document.getElementById('token_created_modal');
      if (modal) modal.close();
      openServicesPage();
    }

    function openServiceDetail(serviceId) {
      if (window.htmx) {
        window.htmx.ajax('GET', `/admin/services/${encodeURIComponent(serviceId)}`, '#main-content');
        if (history && history.pushState) {
          history.pushState({}, '', `/admin/services/${encodeURIComponent(serviceId)}`);
        }
      } else {
        window.location.href = `/admin/services/${encodeURIComponent(serviceId)}`;
      }
    }

    function openApiKeyDetail(apiKey) {
      if (window.htmx) {
        window.htmx.ajax('GET', `/admin/api-keys/${encodeURIComponent(apiKey)}`, '#main-content');
        if (history && history.pushState) {
          history.pushState({}, '', `/admin/api-keys/${encodeURIComponent(apiKey)}`);
        }
      } else {
        window.location.href = `/admin/api-keys/${encodeURIComponent(apiKey)}`;
      }
    }

    function copyCreatedToken() {
      const input = document.getElementById('created_token_value');
      if (!input) return;
      input.select();
      input.setSelectionRange(0, 99999);
      navigator.clipboard.writeText(input.value);
      showToast('API key copied');
    }

    function copyApiKey(value) {
      if (!value) return;
      navigator.clipboard.writeText(value).then(() => showToast('API key copied'));
    }

    document.body.addEventListener('htmx:beforeRequest', function(event) {
      if (event.target && event.target.id === 'add_key_form') {
        showCreateTokenError('');
        setCreateTokenSubmitting(true);
      }
    });

    document.body.addEventListener('htmx:responseError', function(event) {
      if (event.target && event.target.id === 'add_key_form') {
        const xhr = event.detail.xhr;
        showCreateTokenError(xhr && xhr.responseText ? xhr.responseText : 'Create failed');
        setCreateTokenSubmitting(false);
      }
    });

    document.body.addEventListener('htmx:afterRequest', function(event) {
      if (event.target && event.target.id === 'add_key_form') {
        setCreateTokenSubmitting(false);
      }
    });

    document.body.addEventListener('api-key-created', function(event) {
      const detail = event.detail || {};
      const tokenInput = document.getElementById('created_token_value');
      const serviceBox = document.getElementById('created_token_service');
      const serviceNameBox = document.getElementById('created_token_service_name');
      if (tokenInput) tokenInput.value = detail.key || '';
      if (serviceBox) serviceBox.textContent = detail.service_id || 'default';
      if (serviceNameBox) serviceNameBox.textContent = detail.service_name || 'Default Service';
      const form = document.getElementById('add_key_form');
      if (form) form.reset();
      const addModal = document.getElementById('add_key_modal');
      if (addModal) addModal.close();
      if (detail.key && navigator.clipboard && navigator.clipboard.writeText) {
        navigator.clipboard.writeText(detail.key).then(() => showToast('Created and copied'));
      } else {
        showToast('Created');
      }
      const resultModal = document.getElementById('token_created_modal');
      if (resultModal) resultModal.showModal();
    });

    async function deleteApiKey(apiKey) {
      if (!confirm('Delete this API key?')) return;
      const response = await fetch(`/admin/api-keys/${encodeURIComponent(apiKey)}`, {
        method: 'DELETE',
        headers: { 'HX-Request': 'true' }
      });
      if (!response.ok) {
        showToast('Delete failed');
        return;
      }
      const html = await response.text();
      const list = document.getElementById('keys-list');
      if (list) list.innerHTML = html;
      showToast('Deleted');
    }
  </script>
</div>
"##;

pub const SERVICES_PAGE: &str = r##"
<div class="space-y-8">
  <div class="flex flex-col gap-3 lg:flex-row lg:items-end lg:justify-between">
    <div>
      <h2 class="text-3xl font-bold tracking-tighter text-slate-900">Services</h2>
      <p class="text-slate-500 mt-1.5 text-[15px] font-medium">Manage routing groups and provider bindings</p>
    </div>
    <button onclick="window.htmx ? window.htmx.ajax('GET', '/admin/api-keys', '#main-content') : window.location.href='/admin/api-keys'" class="btn h-11 min-h-0 rounded-xl border border-slate-200 bg-white px-5 font-bold text-slate-700">Back to API Keys</button>
  </div>

  <div class="bg-white rounded-xl shadow-sm border border-slate-200 overflow-hidden">
    <div class="overflow-x-auto">
      <table class="table w-full border-separate border-spacing-0">
        <thead>
          <tr class="bg-slate-50/50">
            <th class="py-4 px-8 text-[11px] font-bold uppercase tracking-widest text-slate-400 border-b border-slate-200">Name</th>
            <th class="py-4 px-8 text-[11px] font-bold uppercase tracking-widest text-slate-400 border-b border-slate-200">Service ID</th>
            <th class="py-4 px-8 text-[11px] font-bold uppercase tracking-widest text-slate-400 border-b border-slate-200">Providers</th>
            <th class="py-4 px-8 text-[11px] font-bold uppercase tracking-widest text-slate-400 border-b border-slate-200">API Keys</th>
            <th class="py-4 px-8 text-[11px] font-bold uppercase tracking-widest text-slate-400 border-b border-slate-200">Created At</th>
            <th class="py-4 px-8 text-[11px] font-bold uppercase tracking-widest text-slate-400 border-b border-slate-200 text-right">Actions</th>
          </tr>
        </thead>
        <tbody id="services-list" hx-get="/admin/services/list" hx-trigger="load">
        </tbody>
      </table>
    </div>
  </div>
  <div id="service_toast_message" class="hidden fixed right-6 top-6 z-[100] rounded-xl border border-emerald-200 bg-white px-4 py-3 shadow-xl">
    <div id="service_toast_message_text" class="text-sm font-semibold text-slate-700"></div>
  </div>
  <script>
    let serviceToastTimer;

    function showServiceToast(message) {
      const wrap = document.getElementById('service_toast_message');
      const text = document.getElementById('service_toast_message_text');
      if (!wrap || !text) return;
      text.textContent = message;
      wrap.classList.remove('hidden');
      clearTimeout(serviceToastTimer);
      serviceToastTimer = setTimeout(() => wrap.classList.add('hidden'), 2500);
    }

    async function deleteService(serviceId, tokenCount) {
      if (serviceId === 'default') {
        showServiceToast('Default service cannot be deleted');
        return;
      }
      let force = 0;
      if (tokenCount > 0) {
        const confirmed = confirm(`This service is still linked to ${tokenCount} API key(s).\n\nDeleting the service will also delete those API keys.\n\nDo you want to continue?`);
        if (!confirmed) return;
        force = 1;
      } else {
        if (!confirm('Delete this service?')) return;
      }
      const response = await fetch(`/admin/services/${encodeURIComponent(serviceId)}?force=${force}`, {
        method: 'DELETE',
        headers: { 'HX-Request': 'true' }
      });
      if (!response.ok) {
        const text = await response.text();
        showServiceToast(text || 'Delete failed');
        return;
      }
      const html = await response.text();
      const list = document.getElementById('services-list');
      if (list) list.innerHTML = html;
      showServiceToast('Service deleted');
    }

    function openServiceDetail(serviceId) {
      if (window.htmx) {
        window.htmx.ajax('GET', `/admin/services/${encodeURIComponent(serviceId)}`, '#main-content');
        if (history && history.pushState) {
          history.pushState({}, '', `/admin/services/${encodeURIComponent(serviceId)}`);
        }
      } else {
        window.location.href = `/admin/services/${encodeURIComponent(serviceId)}`;
      }
    }
  </script>
</div>
"##;

pub const SERVICE_DETAIL_PAGE: &str = r##"
<div class="space-y-8">
  <div class="flex flex-col gap-3 lg:flex-row lg:items-end lg:justify-between">
    <div>
      <h2 class="text-3xl font-bold tracking-tighter text-slate-900">{{service_name}}</h2>
      <p class="text-slate-500 mt-1.5 text-[15px] font-medium">Service ID: {{service_id}} · Created at {{created_at}}</p>
    </div>
    <button onclick="window.htmx ? window.htmx.ajax('GET', '/admin/services', '#main-content') : window.location.href='/admin/services'" class="btn h-11 min-h-0 rounded-xl border border-slate-200 bg-white px-5 font-bold text-slate-700">Back to Services</button>
  </div>

  <div class="grid grid-cols-1 xl:grid-cols-2 gap-6">
    <div class="bg-white rounded-xl shadow-sm border border-slate-200 overflow-hidden">
      <div class="px-6 py-5 border-b border-slate-200 bg-slate-50/50">
        <h3 class="text-lg font-bold text-slate-900">Bound Providers</h3>
      </div>
      <div class="overflow-x-auto">
        <table class="table w-full border-separate border-spacing-0">
          <thead>
            <tr class="bg-slate-50/50">
              <th class="py-4 px-6 text-[11px] font-bold uppercase tracking-widest text-slate-400 border-b border-slate-200">Name</th>
              <th class="py-4 px-6 text-[11px] font-bold uppercase tracking-widest text-slate-400 border-b border-slate-200">Type</th>
              <th class="py-4 px-6 text-[11px] font-bold uppercase tracking-widest text-slate-400 border-b border-slate-200">Endpoint</th>
            </tr>
          </thead>
          <tbody>
            {{provider_rows}}
          </tbody>
        </table>
      </div>
    </div>

    <div class="bg-white rounded-xl shadow-sm border border-slate-200 overflow-hidden">
      <div class="px-6 py-5 border-b border-slate-200 bg-slate-50/50">
        <h3 class="text-lg font-bold text-slate-900">API Keys Using This Service</h3>
      </div>
      <div class="overflow-x-auto">
        <table class="table w-full border-separate border-spacing-0">
          <thead>
            <tr class="bg-slate-50/50">
              <th class="py-4 px-6 text-[11px] font-bold uppercase tracking-widest text-slate-400 border-b border-slate-200">Name</th>
              <th class="py-4 px-6 text-[11px] font-bold uppercase tracking-widest text-slate-400 border-b border-slate-200">API Key</th>
              <th class="py-4 px-6 text-[11px] font-bold uppercase tracking-widest text-slate-400 border-b border-slate-200">Created At</th>
            </tr>
          </thead>
          <tbody>
            {{api_key_rows}}
          </tbody>
        </table>
      </div>
    </div>
  </div>
  <script>
    function openApiKeyDetail(apiKey) {
      if (window.htmx) {
        window.htmx.ajax('GET', `/admin/api-keys/${encodeURIComponent(apiKey)}`, '#main-content');
        if (history && history.pushState) {
          history.pushState({}, '', `/admin/api-keys/${encodeURIComponent(apiKey)}`);
        }
      } else {
        window.location.href = `/admin/api-keys/${encodeURIComponent(apiKey)}`;
      }
    }
  </script>
</div>
"##;

pub const API_KEY_DETAIL_PAGE: &str = r##"
<div class="space-y-8">
  <div class="flex flex-col gap-3 lg:flex-row lg:items-end lg:justify-between">
    <div>
      <h2 class="text-3xl font-bold tracking-tighter text-slate-900">{{api_key_name}}</h2>
      <p class="text-slate-500 mt-1.5 text-[15px] font-medium">Created at {{created_at}}</p>
    </div>
    <button onclick="window.htmx ? window.htmx.ajax('GET', '/admin/api-keys', '#main-content') : window.location.href='/admin/api-keys'" class="btn h-11 min-h-0 rounded-xl border border-slate-200 bg-white px-5 font-bold text-slate-700">Back to API Keys</button>
  </div>

  <div class="grid grid-cols-1 xl:grid-cols-2 gap-6">
    <div class="bg-white rounded-xl shadow-sm border border-slate-200 p-6">
      <div class="text-[11px] font-bold uppercase tracking-widest text-slate-400 mb-2">API Key</div>
      <code class="block rounded-lg bg-slate-50 border border-slate-200 px-4 py-3 text-[13px] font-mono text-slate-700 break-all">{{api_key_value}}</code>
      <button type="button" onclick="navigator.clipboard.writeText('{{api_key_value}}')" class="btn mt-4 h-10 min-h-0 rounded-xl border border-slate-200 bg-white px-4 font-bold text-slate-700">Copy</button>
    </div>
    <div class="bg-white rounded-xl shadow-sm border border-slate-200 p-6">
      <div class="text-[11px] font-bold uppercase tracking-widest text-slate-400 mb-2">Service</div>
      <button onclick="window.htmx ? window.htmx.ajax('GET', '/admin/services/{{service_id}}', '#main-content') : window.location.href='/admin/services/{{service_id}}'" class="text-left text-base font-bold text-brand hover:text-teal-800 transition-colors">{{service_name}}</button>
      <div class="mt-3 text-sm text-slate-500">Service ID: {{service_id}}</div>
    </div>
  </div>
</div>
"##;

pub const LOGS_PAGE: &str = r##"
<div class="space-y-8">
  <div class="pb-4">
    <h2 class="text-3xl font-bold tracking-tighter text-slate-900">Request Logs</h2>
    <p class="text-slate-500 mt-1.5 text-[15px] font-medium">Real-time monitoring of API calls</p>
  </div>

  <div class="bg-white rounded-xl shadow-sm border border-slate-200 overflow-hidden">
    <div class="overflow-x-auto">
      <table class="table w-full border-separate border-spacing-0">
        <thead>
          <tr class="bg-slate-50/50">
            <th class="py-4 px-8 text-[11px] font-bold uppercase tracking-widest text-slate-400 border-b border-slate-200">TIME</th>
            <th class="py-4 px-8 text-[11px] font-bold uppercase tracking-widest text-slate-400 border-b border-slate-200">PATH</th>
            <th class="py-4 px-8 text-[11px] font-bold uppercase tracking-widest text-slate-400 border-b border-slate-200">STATUS</th>
            <th class="py-4 px-8 text-[11px] font-bold uppercase tracking-widest text-slate-400 border-b border-slate-200">LATENCY</th>
            <th class="py-4 px-8 text-[11px] font-bold uppercase tracking-widest text-slate-400 border-b border-slate-200">ENDPOINT</th>
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
    <h2 class="text-3xl font-bold tracking-tighter text-slate-900">System Settings</h2>
    <p class="text-slate-500 mt-1.5 text-[15px] font-medium">Configure global gateway parameters</p>
  </div>

  <div class="grid grid-cols-1 lg:grid-cols-2 gap-6">
    <div class="bg-white rounded-xl p-8 shadow-sm border border-slate-200">
      <h3 class="text-lg font-bold text-slate-800 mb-6 tracking-tight">Security Settings</h3>
      <div class="space-y-6">
        <div class="form-control">
          <label class="label"><span class="label-text font-bold text-slate-500 text-xs">Admin Password</span></label>
          <div class="join w-full">
            <input type="password" value="********" class="input h-11 bg-slate-50 border-slate-200 rounded-l-lg focus:border-brand/50 focus:ring-0 font-medium join-item flex-1 text-sm" readonly />
            <button class="btn bg-brand hover:bg-brand/90 text-white border-none join-item rounded-r-lg font-bold px-6 h-11 min-h-0">Change</button>
          </div>
        </div>
      </div>
    </div>

    <div class="bg-white rounded-xl p-8 shadow-sm border border-slate-200">
      <h3 class="text-lg font-bold text-slate-800 mb-6 tracking-tight">Runtime Parameters</h3>
      <div class="space-y-4">
        <label class="flex items-center justify-between p-4 bg-slate-50 rounded-lg cursor-pointer hover:bg-slate-100 transition-colors border border-transparent hover:border-slate-200">
          <div>
            <span class="block font-bold text-slate-700 text-sm">Detailed Logging</span>
            <span class="block text-[10px] text-slate-400 font-bold uppercase tracking-widest mt-0.5">Full Request/Response Body</span>
          </div>
          <input type="checkbox" checked class="toggle toggle-success toggle-sm" />
        </label>
        <label class="flex items-center justify-between p-4 bg-slate-50 rounded-lg cursor-pointer hover:bg-slate-100 transition-colors border border-transparent hover:border-slate-200">
          <div>
            <span class="block font-bold text-slate-700 text-sm">Enable Public Registration</span>
            <span class="block text-[10px] text-slate-400 font-bold uppercase tracking-widest mt-0.5">Allow public signups</span>
          </div>
          <input type="checkbox" class="toggle toggle-success toggle-sm" />
        </label>
      </div>
    </div>
  </div>
</div>
"##;
