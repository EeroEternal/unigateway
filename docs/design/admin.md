# Admin UI Support (UniAdmin and Other External Clients)

This document describes the headless admin capabilities exposed by UniGateway for external management clients.

## Scope

UniGateway does not include a built-in web admin UI. External clients should call JSON APIs under `/api/admin/*`.

## Authentication

- When `UNIGATEWAY_ADMIN_TOKEN` is configured:
  - Every `/api/admin/*` request must include header `x-admin-token` with the same token.
  - Missing or invalid token returns `401 Unauthorized`.
- When `UNIGATEWAY_ADMIN_TOKEN` is not configured:
  - Admin APIs follow existing local/trusted-network behavior.

## Endpoints for Admin UI

### 1) List Modes

- Method/Path: `GET /api/admin/modes`
- Optional query: `detailed=true`
  - `false` (default): summary records for selector UIs
  - `true`: full mode view including providers and keys

Summary response shape:

```json
{
  "success": true,
  "data": [
    {
      "id": "fast",
      "name": "Fast",
      "routing_strategy": "round_robin",
      "is_default": true,
      "provider_count": 1,
      "provider_names": ["deepseek-main"]
    }
  ]
}
```

### 2) Set Default Mode

- Method/Path: `POST /api/admin/preferences/default-mode`
- Request:

```json
{"mode_id":"strong"}
```

- Success response:

```json
{
  "success": true,
  "data": {
    "mode_id": "strong"
  }
}
```

- Error behavior:
  - Unknown `mode_id` returns `400` with `{ "success": false, "error": "..." }`

### 3) Rebind API Key to Service

- Method/Path: `PATCH /api/admin/api-keys`
- Request:

```json
{"key":"ugk_fast_123","service_id":"strong"}
```

- Success response:

```json
{
  "success": true,
  "data": {
    "key": "ugk_fast_123",
    "service_id": "strong"
  }
}
```

- Error behavior:
  - Unknown key or unknown service returns `400` with `{ "success": false, "error": "..." }`

## Routing Semantics

- Runtime request routing follows `api_key.service_id`.
- `preferences.default_mode` mainly affects CLI defaults and integration guidance.

For a one-click mode switch UX, external UIs should call:

1. `POST /api/admin/preferences/default-mode`
2. `PATCH /api/admin/api-keys`

in that order.

## Recommended Frontend Workflow

1. Call `GET /api/admin/modes` to populate mode selector.
2. Let user choose target mode and optional target key.
3. Submit default mode update.
4. Submit key rebind update.
5. Refresh mode list and key list to confirm final state.

## Networking and CORS

- Preferred for production and team use:
  - Run UniAdmin and UniGateway behind the same reverse proxy and origin.
- Local development only:
  - Temporary CORS allowances can be used in frontend/dev proxy setup.
  - Do not expose permissive CORS with public listeners.

## Validation and Stability

Implemented tests cover:

- `401` on missing admin token for protected handlers
- `400` for unknown mode in default-mode API
- `400` for unknown key/service in key rebind API
- Key rebind preserves non-routing fields (quota, used quota, limits)
