# UniGateway Development Progress

## Current Status (v0.1.1)

UniGateway is a lightweight, open-source LLM gateway compatible with OpenAI and Anthropic APIs.

### Implemented Features

1.  **Core Server**
    *   Built with Axum web framework.
    *   SQLite database integration using SQLx for persistent storage.
    *   Graceful shutdown support.

2.  **API Endpoints**
    *   `POST /v1/chat/completions`: OpenAI-compatible chat completion endpoint.
    *   `POST /v1/messages`: Anthropic-compatible messages endpoint.
    *   `GET /v1/models`: List available models.
    *   `GET /health`: Health check endpoint.

3.  **Admin Dashboard**
    *   Web interface accessible at `/admin`.
    *   **Authentication**: Simple login mechanism (currently hardcoded or config-based).
    *   **Dashboard**: Overview of request statistics (Total Requests, OpenAI vs Anthropic usage).
    *   **Provider Management**: Add, list, and delete LLM providers.
    *   **Tech Stack**: Server-side rendering with Askama templates, HTMX for dynamic interactions, TailwindCSS + DaisyUI for styling.

4.  **LLM Integration**
    *   Integration with `llm-connector` crate for unifying different LLM providers.
    *   Support for streaming (SSE) and non-streaming responses.
    *   Configurable API keys and base URLs via environment variables or per-request overrides.

5.  **Data Persistence**
    *   `providers` table: Stores configuration for upstream LLM providers.
    *   `request_stats` table: Logs basic metrics for every request (provider, endpoint, status, latency).

### Recent Changes (v0.1.1)

*   **Refactored Admin UI**: Migrated from inline HTML strings to **Askama** templates for better maintainability and type safety.
*   **UI Improvements**: Replaced native HTML select dropdowns with **DaisyUI** custom dropdowns for a more polished look.
*   **Version Bump**: Updated project version to 0.1.1.

## Roadmap & Next Steps

### Immediate Tasks

1.  **Testing**
    *   Verify SSE (Server-Sent Events) streaming for chat endpoints.
    *   Thoroughly test provider management (CRUD operations).
    *   Test failover scenarios when a provider is down.

2.  **Dynamic Provider Routing**
    *   Currently, the chat handler uses a static configuration or simple logic.
    *   **Goal**: Update `src/handlers/chat.rs` to dynamically select a provider from the database based on the requested model or a routing strategy.

3.  **Authentication & Security**
    *   Implement API Key management for *clients* connecting to UniGateway (not just upstream keys).
    *   Rate limiting per API key.

4.  **Observability**
    *   Enhance logging with `tracing`.
    *   Add more detailed metrics to the dashboard (e.g., tokens usage, cost estimation, error rates over time).

5.  **Documentation**
    *   Add API documentation (Swagger/OpenAPI).
    *   Write a "Getting Started" guide for users.

### Long-term Goals

*   **Load Balancing**: Distribute traffic across multiple API keys/providers.
*   **Caching**: Cache common responses to reduce costs and latency.
*   **Plugin System**: Allow custom middleware for request/response modification.
